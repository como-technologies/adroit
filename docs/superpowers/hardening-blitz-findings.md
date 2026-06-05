# adroit Hardening Blitz — Findings & Approach

**Date:** 2026-06-04
**Scope:** A one-time, AI-driven hardening campaign against the adroit ADR CLI.
**Branch:** `hardening-blitz`
**Companion docs:**
[design spec](specs/2026-06-04-adroit-hardening-blitz-design.md) ·
[running worklog](hardening-blitz-worklog.md)

---

## 1. Executive summary

I built a deterministic, bug-hunting test backbone for adroit and drove it hard.
The campaign found **4 real defects** and a minor API inconsistency:

| # | Defect | Severity | Status |
|---|--------|----------|--------|
| 1 | `supersede` reciprocal note used a non-canonical same-dir link | Low–med | **Fixed** + regression |
| 2 | In-place `supersede` wrote a non-canonical `## Status` link (no relink on a non-move) | Low–med | **Fixed** + regression |
| 3 | `NamingScheme::display` **panicked** on a multibyte uuid slug (`&s[..8]` byte slice) | Medium | **Fixed** + regression |
| 4 | `upsert_reference` non-idempotent on a lone-`\r` document | Low | **Deferred** (documented) |
| — | `Jira::with_transport` was `#[cfg(test)]`-gated unlike its siblings | Trivial | **Fixed** (consistency) |

Bugs #1–#3 are fixed at the root cause, each guarded by a focused regression test.
Bug #4 is a genuine but degenerate-input robustness gap (adroit never *writes*
lone-CR files); it is documented with a fix recommendation rather than chased with
a risky cross-cutting change.

The lasting artifacts are **three reusable test suites** that now run in CI and a
`just model` soak recipe:

- `tests/model.rs` — a model-based ("oracle") tester for the core write path.
- `tests/parsers.rs` — property/fuzz tests for the pure parsers.
- `tests/forge_faults.rs` — fault-injection for the forge HTTP adapters.

Full suite after the campaign: **213 lib unit + 73 CLI + 3 model + 9 parser + 1
forge-fault** tests green; `cargo clippy` clean in both feature modes; `cargo fmt`
clean.

---

## 2. Approach — how I tested

The guiding idea (chosen with the user up front): a **deterministic Rust backbone**
that encodes adroit's *documented* invariants, with **Claude as the live
explorer** that drives the binary, triages anomalies, and crystallizes each one
into a permanent regression. The AI finds unknown-unknowns; the deterministic
layer locks them down forever. Three complementary modalities:

### 2.1 Model-based "oracle" testing (the core)

adroit advertises unusually strong invariants — *byte-identical round-trips*,
*idempotent relink → no-op on a canonical repo*, *status-encoded-by-directory*,
*scheme-agnostic identity*. That is ideal ground for model-based testing.

`tests/model.rs` generates a **random sequence of mutating CLI commands** and runs
each against the **real `adroit` binary** on a throwaway `TempDir`. Crucially, it
drives the *actual binary* (not the library) so the full stack — `main.rs`
dispatch, template rendering, the `Store` write path — is exercised exactly as a
user runs it. In parallel, a tiny **in-memory oracle** tracks what the repo
*should* contain. The oracle is a pure **outcome predictor** — it never
re-implements adroit's serialization or move logic, only the *observable result*
of each verb — so the oracle itself stays small and is unlikely to carry its own
bugs (a risk the spec review explicitly flagged).

After **every** command it asserts:

- **(A) State agreement** — the set of ADRs on disk equals the oracle's, by
  identity; no missing, extra, or duplicate ADRs.
- **(B) Status ↔ directory** — in `by_status`, each ADR lives in the directory its
  status implies; in `flat`, status is read from content.
- **(C) Field agreement** — title, status, supersession pointer, and review date
  match the oracle.
- **(D) Clean `check`** — `query::check` reports zero `Error`-severity problems.
- **(E) Link-canonicality** — a `relink` dry-run rewrites **nothing** (the repo is
  always canonical after a status operation; relink is a true no-op).

Commands generated: `new`, `set-status`, `supersede`, `set-review`, `renumber`,
`relink`. Indices are taken modulo the live ADR count, so a sequence is always
valid. Commands are gated per cell (e.g. `renumber` is sequential-only).
**Matrix cells covered:** `markdown × by_status × sequential` and
`markdown × flat × sequential`.

A separate **metamorphic** property checks the gnarliest verb: a
`by_status → flat → by_status` **migrate round-trip is byte-identical** for
link-free repos (a verbatim layout move plus a no-op relink is the identity).

Invariant **(E)** alone found bugs #1 and #2.

### 2.2 Parser property/fuzz testing

`tests/parsers.rs` feeds the pure parse/serialize/rewrite/link/naming helpers
**arbitrary input** — including ASCII control characters, newlines, and multibyte
unicode placed at byte-prefix boundaries (the codebase already carried a
regression for an em-dash boundary panic, a hint this area was fragile). Two kinds
of property:

- **No panic** — every helper must tolerate any input without panicking. This
  found bug #3.
- **Algebraic laws** — `rewrite_status`, `rewrite_review_by(.., None)`,
  `upsert_reference`, and `links::rewrite_links` are **idempotent**; a parsed
  markdown ADR's body **round-trips** (parse → body → re-parse is a fixpoint).
  This found bug #4.

### 2.3 Forge fault-injection ("pen-testing" the HTTP adapters)

The forge adapters (GitHub / GitLab / Jira) parse **untrusted** HTTP responses
from third-party APIs. `tests/forge_faults.rs` builds each adapter over a
`HostileTransport` that returns arbitrary status codes and malformed / truncated /
wrong-typed / oversized / null bodies (plus an injected connection failure), then
calls **every** `Forge` and `Tracker` method, asserting the adapters **never
panic** and always return a clean `Result` — a garbage response must become an
`Err`, never a crash or a bogus `Ok`. At 5000 cases the adapters held up: a
**positive finding** that the response parsing is already robustly defensive.
(One adapter API inconsistency surfaced and was fixed; see §3.)

### 2.4 The explore → triage → crystallize loop

For each failure the harness shrank to a minimal sequence, I:
1. **Reproduced** it by hand with the real binary and inspected the on-disk files.
2. **Triaged** — real bug vs. spec ambiguity vs. harness/test bug.
3. **Crystallized** — wrote a focused, fast regression test (`tests/cli.rs` or a
   unit test) that fails on the bug.
4. **Fixed** the production code at root cause and confirmed the regression and the
   whole suite go green.

This is why every fix below is paired with a named regression: the property
tester *discovers*, the regression *locks it down* permanently.

---

## 3. Findings in detail

### Bug #1 — `supersede` reciprocal note: non-canonical same-dir link

- **Found by:** oracle invariant (E). Minimal sequence: `new; new;
  set-status 1 superseded; supersede 1 2` — i.e. the *newer* ADR is itself already
  in `superseded/`.
- **Symptom:** after `supersede`, a `relink` would still rewrite a file, so the
  repo was left non-canonical.
- **Root cause:** `cmd_supersede` → `add_supersedes_note` (src/main.rs) appended a
  reciprocal `> Supersedes [..](link)` note to the newer ADR using a local
  `relative_link` helper that omits the `./` prefix for a same-directory target.
  The canonical engine `links::rel_link` (which `relink` uses) emits `./`, so the
  note was born non-canonical and `relink` would "fix" it.
- **Fix & why:** compute the note's link with `links::rel_link` — the one
  canonical engine — so the note is born canonical. Routing through the single
  source of truth is the principled fix, not hand-patching the `./`.
- **Regression:** `tests/cli.rs::supersede_when_new_is_already_superseded_leaves_links_canonical`.

### Bug #2 — in-place `supersede`: non-canonical `## Status` link

- **Found by:** oracle invariant (E), deeper soak. Trigger: superseding an ADR that
  is **already** in `superseded/`, so it doesn't move.
- **Root cause:** `Store::set_status_at` only reconciles links (`relink_after_move`)
  when the file actually changes directory. `Store::relative_link_to` built the
  `Superseded by [..](link)` link with a local `pathdiff` helper that drops the
  same-dir `./`. On a normal supersede the old ADR moves, so the follow-up relink
  canonicalized it — but with no move, the non-canonical link survived.
- **Fix & why:** `Store::relative_link_to` now routes through `links::rel_link`, so
  the link is canonical *regardless* of whether a move follows. The now-dead
  `pathdiff` helper was deleted.
- **Regression:** `tests/cli.rs::supersede_in_place_writes_canonical_status_link`.

> **Shared root cause (lesson).** Bugs #1 and #2 both came from *ad-hoc copies* of
> the relative-link computation (`store::pathdiff`, `main::relative_link`) that
> drifted from the canonical `links::rel_link` by dropping the same-dir `./`.
> Supersession-link generation now routes through the one engine. The general
> lesson — duplicated "compute a relative path" helpers are a canonicalization
> hazard — is the kind of thing a model-based invariant ("relink is always a
> no-op") catches that example-based tests miss.

### Bug #3 — `NamingScheme::display` panics on a multibyte uuid slug

- **Found by:** `tests/parsers.rs::naming_helpers_never_panic`. Input:
  `display(Slug("a𐀀𐀀"))` under the uuid scheme.
- **Symptom:** `end byte index 8 is not a char boundary` — a panic. Reachable via a
  crafted id (`adroit show <…>`) or a crafted uuid-slug filename in the repo, which
  would crash `adroit list` / `show`.
- **Root cause:** the uuid branch shortened the slug with `&s[..s.len().min(8)]`, a
  **byte** slice that panics when byte 8 lands inside a multibyte char.
- **Fix & why:** take the first 8 **chars** (`s.chars().take(8)`). Byte-identical
  for a real ASCII-hex uuid; panic-free for any slug. Operating on chars, not raw
  byte offsets, is the correct way to truncate user-facing text.
- **Regression:** `src/naming.rs::uuid_display_tolerates_multibyte_slug`.

### Bug #4 — `upsert_reference` non-idempotent on a lone `\r` (deferred)

- **Found by:** `tests/parsers.rs::upsert_reference_is_idempotent`. Input `"#\r"`
  + label `A`: the second call appends a **duplicate** `## References` section.
- **Root cause:** the helper detects the newline style as `\n` (no `\r\n` present),
  splits/joins on `\n`, which fuses the lone `\r` with the joined `\n` into a
  `\r\n`. The next call then detects `\r\n`, mis-splits the document, fails to find
  the existing heading, and creates a second one. The same class affects
  `rewrite_status` / `rewrite_review_by`.
- **Why deferred:** adroit only ever *writes* `\n` (or preserves an existing
  `\r\n`) — both idempotent — so this triggers only on an externally-corrupted
  lone-CR (classic-Mac) file, which adroit never produces. A correct fix is a
  cross-cutting newline-normalization change across `format.rs` with real
  byte-preservation risk to the many round-trip-identical tests; not worth that
  risk for a degenerate input without a deliberate go-ahead.
- **Containment:** the idempotence property tests are scoped to consistent-newline
  inputs (`arb_lf_text`) so they stay meaningful for realistic documents. Fix
  candidate: route all three rewriters through one newline-aware split that
  recognizes a lone `\r` as a separator.

### Minor — `Jira::with_transport` visibility

The GitHub and GitLab adapters expose a public `with_transport` (transport
injection) constructor; Jira's was `#[cfg(test)]`-gated. Exposed it to match, so
the fault-injection suite can build all three adapters uniformly. (Also note:
`Jira`'s `Forge` impl is an intentional `unreachable!` guard — Jira is only ever
wired as a Tracker — so the suite exercises only its tracker side.)

---

## 4. Coverage achieved — and what is *not* covered

Honesty about the blast radius matters more than a green checkmark.

**Covered**
- Core write-path invariants under `markdown × {by_status, flat} × sequential`,
  across `new` / `set-status` / `supersede` / `set-review` / `renumber` / `relink`,
  soaked to ~1500 cases per cell.
- Migrate `by_status ↔ flat` round-trip byte-identity (link-free), ~800 cases.
- All pure parsers (markdown status region, supersession, references, review-by,
  link rewriting/relabeling, naming for all four schemes, `parse_remote_url`,
  frontmatter deserialize) for no-panic + idempotence/round-trip, ~1500 cases.
- Forge GitHub/GitLab/Jira response parsing against hostile HTTP, ~5000 cases.

**Not covered (remaining axes / out of scope)**
- **Matrix cells not yet driven by the oracle:** `frontmatter` format, and the
  `date` / `uuid` / `by_category`+`per_category` schemes. These need (a)
  format-aware oracle logic — e.g. under `frontmatter`, a plain `set-status` does
  **not** clear `superseded_by` (it persists as a YAML field), unlike `markdown`;
  and (b) a fixed-"today" clock seam (distinct from `ADROIT_DATE_SOURCE`) so the
  `date` scheme's `YYYYMMDD-` slug and review-due flagging are deterministic. The
  harness's `Profile` abstraction is built to extend to these cells.
- **`migrate` format-change round-trips** (markdown ↔ frontmatter) beyond the
  layout-only byte-identity check.
- **Web / `serve` security** (path traversal in the `/api/browse` directory picker,
  markdown→HTML autolink injection, SSE/JSON abuse) — explicitly de-scoped by the
  user at the outset.
- **git-history paths** (`src/history.rs`) — the oracle runs with
  `date_source=filesystem` to stay git-free; a git-backed suite would exercise the
  history + relink-commit paths.
- **Bug #4** remains unfixed by design (see §3).

---

## 5. How to run & extend

```sh
just test            # everything, incl. the blitz suites at proptest's default 256 cases
just test-forge      # adds the forge feature (runs tests/forge_faults.rs)
just model           # wide soak of the oracle + parser tests (PROPTEST_CASES, default 2000)
PROPTEST_CASES=10000 just model            # deeper soak
cargo test --features forge --test forge_faults    # forge fuzz only
```

- The blitz suites run inside `just ci` (via `just test` / `just test-forge`) at
  256 cases — fast enough for the gate. Use `just model` for the heavier soak.
- proptest persists any failure's seed under `tests/*.proptest-regressions`, which
  are **committed**, so a discovered failure replays deterministically forever.
- **To add a matrix cell:** add a `Profile { format, layout, naming }` and a new
  `#[test]` calling `run_cell(profile, &ops)` in `tests/model.rs`, plus any
  format-aware branches in the oracle's `apply` (see the `frontmatter` notes in
  §4). Gate per-scheme commands in `arb_op_sequential` accordingly.

---

## 6. Recommendations

1. **Land the fixes.** Bugs #1–#3 are clean, root-caused, regression-guarded fixes;
   they harden real user-facing behavior (`supersede` canonicality, crash-safety of
   `show`/`list`).
2. **Decide on bug #4.** If lone-CR robustness matters, route all three `format.rs`
   rewriters through a single newline-aware splitter — a focused, testable change.
3. **Extend the oracle to the remaining cells** (`frontmatter`, `date`, `uuid`,
   `by_category`) when convenient. The `frontmatter` cell in particular will likely
   surface the most new behavior because its set-status / supersession semantics
   diverge from `markdown`. Add the fixed-"today" seam first.
4. **Keep the suites in CI** (already wired). They are cheap at 256 cases and pay
   for themselves the next time the write path or a parser is touched.
