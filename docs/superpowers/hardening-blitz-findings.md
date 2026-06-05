# adroit Hardening Blitz — Findings & Approach

**Date:** 2026-06-04
**Scope:** A one-time, AI-driven hardening campaign against the adroit ADR CLI.
**Branch:** `hardening-blitz`
**Companion docs:**
[design spec](specs/2026-06-04-adroit-hardening-blitz-design.md) ·
[running worklog](hardening-blitz-worklog.md)

---

## 1. Executive summary

I built a deterministic, bug-hunting test backbone for adroit and drove it hard
across the whole feature surface. The campaign found **10 real defects** plus a
minor API inconsistency:

| # | Defect | Surface | Status |
|---|--------|---------|--------|
| 1 | `supersede` reciprocal note used a non-canonical same-dir link | core | **Fixed** |
| 2 | In-place `supersede` wrote a non-canonical `## Status` link | core | **Fixed** |
| 3 | `NamingScheme::display` **panicked** on a multibyte uuid slug | parser | **Fixed** |
| 4 | `upsert_reference` non-idempotent on a lone-`\r` document | parser | Deferred |
| 5 | `uuid` supersede produced a repo that **failed `check`** | core | **Fixed** |
| 6 | `frontmatter` + a slug scheme failed with a cryptic error | core | **Fixed** |
| 7 | `by_category` supersede wrote a **broken link** | core | **Fixed** |
| 8 | `renumber` strands a frontmatter supersession ref | core | **Detect-fixed** ¹ |
| 9 | `per_category` *same-category* cross-ADR links don't resolve | core | **Fixed** |
| 10 | **Stored XSS** — dashboard rendered raw HTML / `javascript:` | web | **Fixed** |
| 11 | `config show`/`get naming` ignored `--naming` / `ADROIT_NAMING` | config | **Fixed** |
| 12 | `check`'s cross-ADR link validation was numeric-only (broke slug heal-on-main) | core | **Fixed** |
| — | `Jira::with_transport` was `#[cfg(test)]`-gated unlike siblings | forge | **Fixed** |

**12 findings total; 11 fixed (or detect-fixed), each with a regression test** —
#11 and #12 came from the follow-up coverage-widening (config precedence + the
oracle's `relink_scope` variation), documented in
[`hardening-blitz-worklog.md`](hardening-blitz-worklog.md) and
[`../testing.md`](../testing.md). Only #4
(lone-CR) remains fully deferred; #8's *auto-fix* is deferred but it is no longer
silent — ¹ `check` now validates frontmatter supersession and flags the stranded
ref. (#9 and the #8 detection were fixed in a follow-up pass after the full-matrix
oracle surfaced them; see §3.)

The lasting artifacts are **four reusable test suites** wired into CI, a small
production test-seam, and a `just model` soak recipe:

- `tests/model.rs` — the model-based ("oracle") tester for the core write path,
  covering the full **format × layout × naming** matrix (9 valid cells) plus
  migrate round-trips.
- `tests/parsers.rs` — property/fuzz tests for the pure parsers.
- `tests/forge_faults.rs` — fault-injection for the forge HTTP adapters.
- `src/serve/mod.rs` tests — markdown-render XSS + directory-picker crash-safety
  (now run in CI via the new `just test-web`).

Suite totals after the campaign: **243 lib unit + 76 CLI + 3 model + 9 parser +
1 forge-fault** green (`--features web`), all serve tests green; `cargo clippy`
and `cargo fmt` clean across the default, `forge`, and `web` feature builds.

---

## 2. Approach — how I tested

The guiding idea (agreed with the user up front): a **deterministic Rust backbone**
that encodes adroit's *documented* invariants, with **Claude as the live explorer**
that drives the binary, triages anomalies, and crystallizes each one into a
permanent regression. The AI finds unknown-unknowns; the deterministic layer locks
them down forever. Four complementary modalities:

### 2.1 Model-based "oracle" testing (the core)

adroit advertises strong invariants — *byte-identical round-trips*, *idempotent
relink → no-op on a canonical repo*, *status-encoded-by-directory*,
*scheme-agnostic identity*. Ideal ground for model-based testing.

`tests/model.rs` generates a random **matrix cell** (format × layout × scheme) and
a random **sequence of mutating CLI commands**, runs each command against the
**real `adroit` binary** on a throwaway `TempDir` (so the full stack — `main.rs`
dispatch, templates, the `Store` write path — runs exactly as a user runs it), and
tracks what the repo *should* contain in a tiny in-memory **oracle**. The oracle is
a pure **outcome predictor**: it never re-implements adroit's serialization/move
logic. For schemes whose identity isn't deterministic (uuid; date with dedup) it
**reads the assigned identity back** from disk after `new`, then predicts
everything else — so the oracle stays small and is unlikely to carry its own bugs
(a risk the spec review flagged).

After **every** command it asserts:
- **(A)** the set of ADR identities on disk equals the oracle's (no lost / extra /
  duplicate ADRs);
- **(B)** in `by_status`, each ADR sits in the directory its status implies;
- **(C)** title, status, supersession pointer, review date, and category match;
- **(D)** `adroit check` reports zero `Error`-severity problems;
- **(E)** the repo is link-canonical — a `relink` dry-run rewrites **nothing**.

**Matrix covered (9 valid cells):** `{markdown,frontmatter} × {by_status,flat} ×
sequential`, `markdown × {by_status,flat} × {date,uuid}`, and `markdown ×
by_category × per_category`. (frontmatter pairs only with sequential — see #6.)
Commands: `new`, `set-status`, `supersede`, `set-review`, `renumber`, `relink`,
gated per cell. Two **metamorphic** properties cover migrate: a
`by_status ↔ flat` round-trip is **byte-identical**, and a `markdown ↔ frontmatter`
round-trip is **logically lossless** (number/title/status preserved, `check` clean).

This modality found bugs #1, #2, #5, #6, #7, #8, #9. Invariant (E) alone caught
the supersession link-canonicality bugs; (A)/(C) caught the identity and
stale-reference bugs; the `new`-time success check caught the cryptic frontmatter
failure.

**Determinism seam.** The `date` scheme's slug and review-due math read the local
clock. I added a minimal, test-only `ADROIT_TODAY` override
(`config::today_override`, consulted by `query::today` + `store::today_local`) so
the oracle is reproducible; the default (unset) path is unchanged, and it is
distinct from `ADROIT_DATE_SOURCE`.

### 2.2 Parser property / fuzz testing

`tests/parsers.rs` feeds the pure parse/serialize/rewrite/link/naming helpers
**arbitrary input** — control chars, newlines, multibyte unicode at byte-prefix
boundaries (the codebase already carried a regression for an em-dash boundary
panic). Properties: **no panic**, and **algebraic laws** (`rewrite_status`,
`rewrite_review_by(.., None)`, `upsert_reference`, `links::rewrite_links` are
idempotent; a parsed markdown ADR's body round-trips). Found bugs #3 and #4.

### 2.3 Forge fault-injection ("pen-testing" the HTTP adapters)

`tests/forge_faults.rs` builds each adapter (GitHub/GitLab/Jira) over a
`HostileTransport` returning arbitrary status codes and malformed/truncated/
wrong-typed/null bodies (plus injected offline), then calls **every** `Forge`/
`Tracker` method, asserting **no panic** and a clean `Result`. At 5000 cases the
adapters held up — a **positive finding** (robust response parsing). One adapter
API inconsistency surfaced and was fixed.

### 2.4 Web / serve security

The dashboard renders ADR bodies to HTML and exposes a directory picker. New
`serve` tests cover (a) the markdown→HTML render as an **XSS surface** — raw HTML
and dangerous URL schemes — and (b) the directory-picker endpoints' **crash-safety**
on hostile paths. Found bug #10.

### 2.5 The explore → triage → crystallize loop

For each failure (proptest shrank it to a minimal sequence) I reproduced it by hand
with the real binary, inspected the on-disk files, triaged (real bug / spec
ambiguity / harness bug), wrote a focused fast regression, fixed at root cause, and
confirmed the regression + whole suite go green. Every fix below is paired with a
named regression: the property tester *discovers*, the regression *locks it down*.

---

## 3. Findings in detail

### Fixed

**#1 — `supersede` reciprocal note: non-canonical same-dir link.** The reciprocal
`> Supersedes [..](link)` note (main.rs `add_supersedes_note`) used a local helper
that dropped the same-dir `./` the canonical `links::rel_link` emits, so a follow-up
`relink` would rewrite it. *Fix:* use `links::rel_link`.
Regression: `supersede_when_new_is_already_superseded_leaves_links_canonical`.

**#2 — In-place `supersede`: non-canonical `## Status` link.** `set_status_at` only
reconciles links when the file moves dirs; superseding an ADR already in
`superseded/` left the freshly-written link non-canonical. *Fix:* route
`relative_link_to` (now folded into `supersede`) through `links::rel_link`; removed
the dead `pathdiff`. Regression: `supersede_in_place_writes_canonical_status_link`.

> #1 and #2 shared a root cause: ad-hoc copies of the relative-path computation
> (`store::pathdiff`, `main::relative_link`) that drifted from the canonical
> `links::rel_link`. A model-based invariant ("relink is always a no-op") catches
> this class that example-based tests miss.

**#3 — `display` panicked on a multibyte uuid slug.** The uuid branch sliced
`&s[..s.len().min(8)]` (bytes), panicking when byte 8 split a multibyte char —
reachable via a crafted id / filename (`adroit show`/`list` would crash). *Fix:*
take the first 8 *chars*. Regression: `uuid_display_tolerates_multibyte_slug`.

**#5 — `uuid` supersede failed its own `check`.** `naming::ref_in_link` returned the
full `{uuid}-{slug}` filename stem, but a uuid ADR's identity is the bare `{uuid}`
(`parse` splits the title off), so the supersession link never resolved and
`check` reported it broken. *Fix:* `ref_in_link` splits the title slug off for
uuid, mirroring `parse`. Regression: `uuid_scheme_supersede_passes_check`.

**#6 — `frontmatter` + a slug scheme failed cryptically.** frontmatter is
numeric-only (its YAML persists a `number:`), so it can't hold slug identity
(date/uuid/per_category); `new` failed deep in the serializer with "ADR number must
be assigned before serializing" on a user-reachable config. *Fix:* a clear up-front
guard in `main.rs`. Regression: `frontmatter_rejects_slug_naming_with_clear_error`.

**#7 — `by_category` supersede wrote a broken link.** `Store::supersede` built the
link relative to `status_dir(Superseded)` unconditionally, but in `flat`/`by_category`
the old ADR stays in its dir (it doesn't move to `superseded/`), so the link got a
spurious `./<category>/` segment pointing nowhere. *Fix:* compute the link relative
to where the old ADR actually ends up (`status_target_dir`). Regression:
`per_category_cross_category_supersede_passes_check`.

**#10 — Stored XSS in the dashboard.** `render_markdown` emitted raw HTML
(`<script>`, `<img onerror=…>`) verbatim and didn't vet link schemes
(`javascript:`), since pulldown-cmark is not a sanitizer — a crafted ADR body
executed script when viewed in `adroit serve`. *Fix:* escape raw HTML events to
text and route every link/image `dest_url` through a new `sanitize_href`
(`javascript:`/`data:`/`vbscript:` → `#`). Regressions: three `render_markdown_*`
security tests.

**Minor — `Jira::with_transport` visibility.** Was `#[cfg(test)]`-gated while the
GitHub/GitLab equivalents are public; exposed it to match, for the fault-injection
suite.

**#9 — `per_category` same-category links didn't resolve.** `ref_in_link` recovered
the per_category identity from the link's path, which fails for a same-category
`./0002-x.md` link (no category segment) — so `check` falsely reported the
supersession broken. *Fix:* `ref_in_link_from` / `ref_in_note_from(target,
source_category)` (a pass-through for every other scheme) take the source file's
category; threaded into the one `query::check` supersession call and a `store::read`
re-resolution. Regression: `per_category_same_category_supersede_passes_check`.

**#8b — `check` now validates frontmatter supersession.** `check` previously checked
supersession only in markdown; it now also validates the frontmatter YAML
`superseded_by:` / `supersedes:` refs, so a stranded pointer (#8) is caught as a
`check` error rather than being silent. Regression:
`frontmatter_check_flags_stranded_supersession`.

### Deferred (real, but narrow + cross-cutting to fix)

**#4 — `upsert_reference` non-idempotent on a lone `\r`.** A lone CR (no `\r\n`)
defeats the helper's newline detection, duplicating the `## References` section on
the second call. adroit never *writes* lone-CR files, so this only triggers on
externally-corrupted input; a correct fix is a cross-cutting newline-normalization
across `format.rs` with byte-preservation risk. The idempotence property tests are
scoped to consistent newlines (`arb_lf_text`).

**#8 (auto-fix only) — `renumber` doesn't rewrite frontmatter supersession refs.**
In frontmatter, supersession is a YAML field, so renumbering a superseded-to ADR
strands the pointer. It is **no longer silent** (#8b: `check` now flags it); only
the *auto-fix* — making `renumber` format-aware (rewrite each ADR's YAML refs + the
renamed ADR's `number:`) — is deferred. Narrow (legacy frontmatter + renumber +
supersession); the oracle skips renumbering a supersession target under frontmatter.

---

## 4. Coverage — and what is *not* covered

**Covered**
- Core write-path invariants across the full 9-cell format × layout × scheme matrix
  (`new`/`set-status`/`supersede`/`set-review`/`renumber`/`relink`), soaked to
  ~1500 cases.
- Migrate `by_status ↔ flat` (byte-identical) and `markdown ↔ frontmatter`
  (logically lossless) round-trips.
- All pure parsers for no-panic + idempotence/round-trip (~1500 cases).
- Forge GitHub/GitLab/Jira response parsing against hostile HTTP (~5000 cases).
- Dashboard markdown-render XSS + directory-picker crash-safety.

**Not covered**
- #4 (lone-CR `upsert`) and #8's `renumber` *auto-fix* remain unfixed by design
  (#8 is now *detected* by `check`).
- The dashboard SPA itself (JS), SSE backpressure, and the live-reload watcher
  beyond its existing unit tests.
- git-history paths (`src/history.rs`) — the oracle runs `date_source=filesystem`
  to stay git-free; a git-backed suite would exercise the history + relink-commit
  paths.

---

## 5. How to run & extend

```sh
just test            # default-feature suite (incl. oracle + parsers) at 192/256 cases
just test-forge      # + forge feature (tests/forge_faults.rs)
just test-web        # + web feature (serve JSON API + markdown-render security)
just model           # wide soak of the oracle + parser tests (PROPTEST_CASES, default 2000)
PROPTEST_CASES=5000 just model
```

- All four suites now run in `just ci` (added `lint-web`/`test-web`; the `web`
  feature builds without a Vue SPA via the embed `.gitkeep`).
- proptest persists each failure's seed under `tests/*.proptest-regressions`
  (committed) so a discovered failure replays deterministically forever.
- **To add a matrix cell:** add a weighted arm to `arb_profile()` in
  `tests/model.rs`; per-scheme command behavior is gated in `Harness::apply`.
  Identity is read back from disk, so new schemes need no oracle prediction.

---

## 6. Recommendations

1. **Land the fixes.** #1–#3, #5–#7, #9, #10 are clean, root-caused,
   regression-guarded fixes hardening real user-facing behavior (supersede
   correctness across every scheme/layout, crash-safety of `show`/`list`, a genuine
   dashboard XSS, and per_category supersession). #8 is now detected by `check`.
2. **Optionally close the last two.** #8's `renumber` auto-fix (make `renumber`
   format-aware) and #4 (lone-CR `upsert`) are both narrow; #4 is best left, or
   handled by normalizing a lone `\r` on read rather than touching the byte-
   sensitive rewriters.
3. **Keep the suites in CI** (done). They're cheap at the default budgets and pay
   for themselves the next time the write path, a parser, or the renderer is
   touched. Use `just model` for deeper soaks before a release.
