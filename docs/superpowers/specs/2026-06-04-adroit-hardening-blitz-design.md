# adroit Hardening Blitz — Design Spec

- **Date:** 2026-06-04
- **Status:** Approved (design); pending implementation plan
- **Topic:** An AI-driven, one-time hardening campaign for the adroit ADR CLI
- **Author:** brett + Claude (brainstorming session)

## 1. Context & goal

adroit is a multi-surface Rust system (core lib + CLI + ratatui TUI + Axum/Vue
`serve` + GitHub/GitLab/Jira `forge`) with unusually explicit, documented
invariants — *byte-identical round-trips* for untouched ADRs, *idempotent
`relink` → no-op on a canonical repo*, *format-preserving writes*,
*status-encoded-by-directory* consistency, and *scheme-agnostic identity*. It
carries ~73 integration tests + ~250 unit tests but **no property-based or
fuzz testing**. We want to harden it before pushing toward Stage 2.

This is a **one-time hardening blitz**, not a permanent CI subsystem and not a
standalone reusable tool. The deliverable is **bugs found and fixed**, plus a
curated set of checked-in regression tests and a small lasting deterministic
harness. We optimize for *bugs found per unit of effort*.

## 2. Decisions locked in this session

| Question | Decision |
|---|---|
| Goal / lifespan | One-time hardening blitz (not a maintained platform) |
| "AI-driven" embodiment | **Hybrid** — a real Rust deterministic backbone, with Claude (live, in-session) as the explorer/triager that crystallizes findings into checked-in tests. No separate Claude-API binary. |
| Target surfaces | Core invariants (model-based), parsers (fuzz), forge fault-injection. **Web/serve security is out of scope.** |
| Core strategy | **Full reference-model oracle** (Approach A) — an executable spec of the intended repo state, diffed against reality after every command. Cheap byte-level laws layered on top. |
| Fuzz tooling | **Stable proptest only** — no nightly `cargo-fuzz` crate. Parser fuzzing rides in `just ci`. |
| `now_local` nondeterminism | Tamed with a **minimal fixed-clock/env seam behind test config** (the only production-code touch besides bug fixes). |
| "Done" deliverable | Harness + regressions stay checked in, plus a bug→fix→regression worklog. |

## 3. Scope

**In scope**
- A model-based oracle exercising the mutating CLI verbs across the
  format × layout × scheme matrix.
- Stable proptest property tests for the parsers.
- Forge fault-injection over the existing `HttpTransport` seam.
- A live explore→triage→crystallize loop (Claude) that feeds the above.

**Out of scope**
- Web/`serve` security (path traversal in `/api/browse`/`/api/workspace`,
  autolink injection, SSE/JSON abuse).
- A standalone Claude-API agent binary; a maintained generator platform;
  generalizing the harness to assessments/pulse/tuesday.
- `cargo-fuzz` / nightly coverage-guided fuzzing.

## 4. Architecture

Three deterministic Rust suites, plus a human/AI exploration loop that feeds
them. The only production-code changes are (a) bug fixes and (b) minor
`pub(crate)`/visibility tweaks plus one small fixed-clock seam.

```
                 ┌─────────────────────────────────────────────┐
   live, Claude  │  Explore → Triage → Crystallize loop (§4.4)  │
                 └───────────────┬─────────────────────────────┘
                                 │ regressions, grammar arms, seeds
            ┌────────────────────┼────────────────────┐
            ▼                    ▼                    ▼
   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐
   │ Oracle (§4.1)   │  │ Parsers (§4.2)  │  │ Forge faults (§4.3) │
   │ tests/oracle/   │  │ proptest props  │  │ tests/forge_faults  │
   │ tests/model.rs  │  │                 │  │ (--features forge)  │
   └────────┬────────┘  └────────┬────────┘  └──────────┬──────────┘
            └────────────── real adroit code ───────────┘
                       (TempDir sandbox, no network)
```

Layout:
- `tests/oracle/` — `ModelRepo`, `Command`, the invariant battery (the spec).
- `tests/model.rs` — proptest entry points driving sequences across the matrix.
- `tests/parsers.rs` (or per-module `#[cfg(test)]`) — parser properties.
- `tests/forge_faults.rs` — forge fault-injection, gated on `--features forge`.
- `proptest-regressions/` — committed seed files for deterministic replay.
- `justfile` — new `just model` recipe; the stable suites fold into `just ci`.

### 4.1 The oracle (core — Approach A)

**`Command`** mirrors the mutating verbs under test:
`New { title, category? }`, `SetStatus { ref, status }`,
`Supersede { old, new }`, `Renumber { old, new }`,
`SetReview { ref, date | clear }`, `SetBody { ref, text }`,
`Migrate { to }`, `Relink`. Read verbs (`list`/`show`/`search`/`check`) are
*assertions*, not state transitions.

**`ModelRepo`** = `BTreeMap<ModelRef, ModelAdr>` + config (profile / layout /
scheme), where `ModelAdr = { ref, slug, title, status, supersedes,
superseded_by, review_by, category, outbound_links }`. The model implements the
*intended* semantics:
- `SetStatus` in `by_status` implies a directory move; in `by_category` /
  `flat` it rewrites status in place.
- `Supersede` sets both reciprocal sides + the link note.
- `Renumber` rewrites the target's ref + self-refs and relabels inbound
  `[ADR-old](…)` links matched by basename.
- `Migrate` remaps refs/paths/format (refused to/from `by_category`).
- `Relink` canonicalizes every relative cross-ADR link.

**`Harness`** holds a real `Store` on a `TempDir` plus the model. `apply(cmd)`
runs the command against both, then `check_invariants()`:

1. **State agreement** — every model ADR exists on disk with matching
   ref/status/slug/title/supersedes/review_by; no missing or extra files;
   `status == containing directory` (`by_status`); zero duplicate refs.
2. **Byte invariants** (layered on the model) — ADRs the command did **not**
   touch are byte-identical to the pre-command snapshot; `relink` and `check`
   are no-ops on the resulting canonical repo; a no-op `set-status` round-trips
   byte-identical.
3. **Cross-path agreement** — the `query::*` view == the CLI-rendered view ==
   the model projection for the same repo.
4. **`adroit check` is clean** — no `Error`-severity problems after every step.

**Matrix** — the proptest strategy picks a valid config from
`{ markdown, frontmatter } × { by_status, flat, by_category } ×
{ sequential, date, uuid, per_category }` (with `per_category` ↔ `by_category`
the only valid pairing for that scheme/layout), then generates a command
sequence valid for that config.

**Shrinking → reproducible script** — on failure proptest minimizes the
`Vec<Command>` to a minimal failing sequence, which we render as a
ready-to-paste `#[test]` that replays those commands. This is the
crystallization path from a random failure to a permanent regression.

### 4.2 Parser properties (stable proptest)

proptest property tests over the pure parse/serialize seams:
`format::parse` / `parse_status_region` / `rewrite_review_by` /
`upsert_reference`; `frontmatter` parse; `links::rewrite_links` /
`relabel_links_to`; `config::parse_remote_url`; `naming::parse_ref`.

Properties asserted:
- **Never panic** on arbitrary + structured input.
- **`parse → serialize → parse` is a fixpoint** (round-trip stability).
- **serialize output re-parses to the same logical ADR.**
- **`rewrite_links` is idempotent** and leaves non-ADR spans byte-for-byte
  (external URLs, anchors, non-`.md` targets untouched).

These run on stable Rust inside `just ci`, so crashers and round-trip
regressions are caught on every push.

### 4.3 Forge fault-injection (`--features forge`)

A `HostileTransport` implementing the existing `HttpTransport` seam returns:
malformed / truncated JSON, unexpected status codes, 500s, empty and oversized
fields, wrong-repo payloads, and mid-orchestration partial failures (e.g. the
tracker issue is created, then the PR create call fails). Drive
`run_new` / `run_status_change` / `comment` / `run_reconcile` over the product
`{ transport fault } × { verb } × { dir match | mismatch }` and assert the
documented contract:

- **Graceful-offline always preserves the local ADR write** — the ADR is the
  durable record; a forge failure warns and keeps the local change.
- Never panics; never leaves a half-written or corrupted local record.
- `dir`-mismatch guards skip the forge side entirely (no writes to the wrong
  repo).

Zero network — `FakeTransport` / `HostileTransport` only.

### 4.4 The explore → triage → crystallize loop (the "AI" = Claude, live)

1. **Explore** — drive the real `adroit` binary on throwaway `TempDir`s with
   scenarios the generator grammar won't reach: hostile unicode titles,
   cross-directory number collisions (a generic ADR pattern — the same number
   legitimately existing in two status dirs), interleaved `migrate` + `relink`,
   tolerated-but-malformed input, status churn, and slug/uuid edge cases.
2. **Triage** — minimize to the smallest reproducer; classify (real bug /
   spec ambiguity / harness bug); name the violated invariant.
3. **Crystallize** — each confirmed bug becomes a focused regression `#[test]`
   **and** a new arm/weight in the `Command` grammar or a parser seed, so it
   can never silently return.
4. **Fix** — the production fix lands as a separate, reviewable change; the
   suite goes green.

The deterministic harness also feeds *back*: a proptest failure's shrunk
sequence becomes a regression `#[test]` via the same path.

## 5. Error handling, sandboxing & determinism

- **Sandbox** — everything runs on `TempDir`s; the harness never touches a real
  repo. Forge is `FakeTransport` / `HostileTransport` only — no network, and no
  `--yes` against a real remote.
- **Deterministic replay** — proptest regression seeds are committed so every
  discovered failure replays identically.
- **Nondeterminism risk (a): `now_local()`** — `review_by` / overdue math reads
  the local clock. Tamed with a minimal fixed-date injection seam behind test
  config (env / `ADROIT_DATE_SOURCE` + an injectable "today"). This is the only
  production-code touch beyond bug fixes; it stays minimal and test-gated.
- **Nondeterminism risk (b): git-derived history** — history/dates need a git
  work tree. The core oracle runs with `date_source = filesystem` to stay
  git-free; a separate, smaller suite inits a real git repo to exercise the
  history + relink-commit paths.

## 6. Deliverables & "done"

**Checked in:** `tests/oracle/`, `tests/model.rs`, parser property tests,
`tests/forge_faults.rs`, `proptest-regressions/`, the `just model` recipe + CI
wiring, and a `WORKLOG`/docs note listing every bug → fix → guarding regression.

**Stopping condition:**
- The model-based harness runs its case budget green across the full
  format × layout × scheme matrix.
- Parser properties green.
- Forge fault suite green.
- Every bug found is fixed and guarded by a committed regression test.

## 7. Sequencing (by bug-yield)

1. Oracle skeleton + the smallest matrix cell (`markdown` / `by_status` /
   `sequential`) → first bugs fast.
2. Full invariant battery + byte checks.
3. Widen the matrix (formats, layouts, schemes) — where `migrate` / `relink` /
   `renumber` bugs hide.
4. Parser properties.
5. Forge fault-injection.
6. Claude exploration interleaved throughout; crystallize as we go.

## 8. Open risks

- The oracle partially re-implements adroit's intended semantics, so the oracle
  itself can carry bugs. Mitigation: keep the model as *thin as the assertion
  requires*, prefer byte-level laws where re-deriving exact output would be as
  error-prone as the code under test, and treat oracle/code disagreement as a
  triage item (either could be wrong).
- Matrix size could blow up proptest runtime. Mitigation: weight common cells,
  cap sequence length, and run the wide matrix as a longer manual pass while CI
  runs a bounded budget.
- `now_local` seam must not change production behavior when unset. Mitigation:
  default path is byte-identical to today; the seam only activates under test
  config.
