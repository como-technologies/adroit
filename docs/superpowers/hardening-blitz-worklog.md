# adroit Hardening Blitz — Bug Worklog

A running log of bugs the hardening blitz found, their fix, and the regression
that guards each one. Spec:
[2026-06-04-adroit-hardening-blitz-design.md](specs/2026-06-04-adroit-hardening-blitz-design.md).

## Harness

- `tests/model.rs` — the model-based oracle: random mutating-command sequences
  run against the real `adroit` binary on a throwaway `TempDir`, with an
  in-memory oracle asserting state-agreement + status↔directory + clean
  `check` + link-canonicality after every command. Default 256 cases
  (`PROPTEST_CASES=N` for a wider soak). Currently covers the
  markdown / by_status / sequential cell.

## Bugs found & fixed

### 1. `supersede` reciprocal note used a non-canonical same-dir link

- **Found by:** oracle invariant (E) "repo is link-canonical after a status op"
  (`relink` is a no-op). Minimal sequence: `new; new; set-status 1 superseded;
  supersede 1 2` — i.e. the *newer* ADR is itself already in `superseded/`.
- **Cause:** `cmd_supersede` → `add_supersedes_note` (src/main.rs) appended the
  reciprocal `> Supersedes [..](link)` note to the newer ADR using the local
  `relative_link` helper, which omits the `./` prefix for a same-directory
  target. The canonical link engine `links::rel_link` (used by `relink`) emits
  `./`, so the note was born non-canonical and a follow-up `relink` would rewrite
  it.
- **Fix:** compute the note's link with `links::rel_link` (the canonical engine).
- **Regression:** `tests/cli.rs::supersede_when_new_is_already_superseded_leaves_links_canonical`.

### 2. In-place `supersede` wrote a non-canonical `## Status` link

- **Found by:** the same oracle invariant (E), deeper soak. Minimal sequence:
  both ADRs moved into `superseded/`, then `supersede 2 1` — the *old* ADR is
  already in `superseded/`, so it doesn't move.
- **Cause:** `Store::set_status_at` only reconciles links (`relink_after_move`)
  when the file actually changes directory. `Store::relative_link_to` built the
  `Superseded by [..](link)` link with the local `pathdiff` helper (no `./` for a
  same-dir target). On a normal supersede the old ADR moves, so the follow-up
  relink canonicalized it; but when the old ADR is already in `superseded/` there
  is no move, so the non-canonical link survived.
- **Fix:** `Store::relative_link_to` now routes through `links::rel_link` (so the
  link is canonical regardless of whether a move follows); the now-dead
  `pathdiff` helper was removed.
- **Regression:** `tests/cli.rs::supersede_in_place_writes_canonical_status_link`.

**Shared root cause.** Both bugs came from ad-hoc relative-link helpers
(`store::pathdiff`, `main::relative_link`) that diverged from the canonical
`links::rel_link` by dropping the same-dir `./` prefix. Supersession-link
generation now routes through the one canonical engine.

### 3. `NamingScheme::display` panicked on a multibyte uuid slug

- **Found by:** `tests/parsers.rs::naming_helpers_never_panic` (parser fuzz).
  Input: `display(Slug("a𐀀𐀀"))` under the uuid scheme.
- **Cause:** the uuid branch shortened the slug with `&s[..s.len().min(8)]`, a
  **byte** slice that panics when byte 8 lands inside a multibyte char. Reachable
  via a crafted id (`adroit show <…>`) or a crafted uuid-slug filename in the repo
  (`adroit list`/`show` would panic).
- **Fix:** take the first 8 *chars* (`s.chars().take(8)`) — byte-identical for a
  real ASCII-hex uuid, panic-free for any slug. (src/naming.rs)
- **Regression:** `src/naming.rs::uuid_display_tolerates_multibyte_slug` +
  the property test's saved seed.

### 5. `uuid`-scheme supersede produced a repo that failed `check`

- **Found by:** the full-matrix oracle (uuid cell). `supersede` then `check`.
- **Cause:** `naming::ref_in_link` returned the **full** `{uuid}-{slug}` filename
  stem, but a uuid ADR's identity is the bare `{uuid}` (`parse` splits the title
  off). The supersession link therefore never resolved — `check` reported
  "Superseded by ADR-… but no such ADR exists" and exited non-zero. (Contradicted
  the documented "uuid works end-to-end incl. supersede/check".)
- **Fix:** `ref_in_link` splits the title slug off for uuid, mirroring `parse`.
- **Regression:** `tests/cli.rs::uuid_scheme_supersede_passes_check` + a
  `naming` unit assertion.

### 6. `frontmatter` + a slug naming scheme failed with a cryptic error

- **Found by:** the full-matrix oracle (frontmatter × per_category/date/uuid).
- **Cause:** the frontmatter format is numeric-only (its YAML persists a
  `number:`), so it can't represent slug-based identity (date / uuid /
  per_category). `adroit new` failed deep in the serializer with "ADR number must
  be assigned before serializing" — a confusing error on a user-reachable config
  (the flags/config keys are independent).
- **Fix:** a clear up-front guard in `main.rs` — the `frontmatter` format requires
  the `sequential` scheme; otherwise bail with a message pointing at
  `--format markdown`.
- **Regression:** `tests/cli.rs::frontmatter_rejects_slug_naming_with_clear_error`.

### 7. `by_category` supersede wrote a broken link

- **Found by:** the full-matrix oracle (by_category cell). `supersede` then
  `check` reported a broken link.
- **Cause:** `Store::supersede` built the supersession link relative to
  `status_dir(Superseded)` unconditionally. But in `flat`/`by_category` the old
  ADR doesn't move to `superseded/` — it stays in its category dir — so the link
  got a spurious `./<category>/` segment and pointed at a nonexistent file.
- **Fix:** compute the link relative to where the old ADR actually ends up
  (`status_target_dir(old_path, Superseded)`), via the canonical `links::rel_link`.
  Removed the now-misleading `relative_link_to` helper.
- **Regression:** `tests/cli.rs::per_category_cross_category_supersede_passes_check`.

## Found — deferred (low severity)

### 4. `upsert_reference` is non-idempotent on input containing a lone `\r`

- **Found by:** `tests/parsers.rs::upsert_reference_is_idempotent`. Input
  `"#\r"` + label `A`: the second call appends a **duplicate** `## References`
  section.
- **Cause:** the helper detects the newline style as `\n` (the input has no
  `\r\n`), splits/joins on `\n`, which fuses the lone `\r` with the joined `\n`
  into a `\r\n`. The next call then detects `\r\n`, mis-splits the document, fails
  to find the existing `## References` heading, and creates a second one. The same
  class affects `rewrite_status` / `rewrite_review_by`.
- **Why deferred:** adroit only ever *writes* `\n` (or preserves an existing
  `\r\n`) — both are idempotent — so this triggers only on an externally-corrupted
  lone-CR (classic-Mac) file, which adroit never produces. A correct fix is a
  cross-cutting newline-normalization change across `format.rs` with real
  byte-preservation risk to the many round-trip-identical tests; not worth that
  risk for a degenerate input without a deliberate go-ahead.
- **Status:** the idempotence property tests are scoped to consistent-newline
  inputs (`arb_lf_text`) so they stay meaningful for realistic documents. Fix
  candidate: route all three rewriters through one newline-aware split that
  recognizes a lone `\r` as a separator.

### 8. `renumber` leaves stale supersession refs under `frontmatter`

- **Found by:** the full-matrix oracle (frontmatter × sequential): `new; new;
  supersede 2 1; renumber 2 3` leaves ADR-1's `superseded_by: 2` pointing at the
  now-nonexistent number.
- **Cause:** `renumber` updates references by **text-relabeling markdown links**
  (`relabel_links_to`). In the frontmatter profile, supersession is a YAML field
  (`superseded_by: 2`), not a markdown link, so it isn't rewritten — the pointer
  dangles. (The renamed ADR's own YAML `number:` field is likewise left stale,
  though identity still resolves via the body heading.) `check` doesn't catch it
  because frontmatter supersession isn't validated — a second gap.
- **Why deferred:** narrow (legacy frontmatter + renumber + an existing
  supersession to the renumbered ADR), and a correct fix makes `renumber`
  format-aware (parse + rewrite each ADR's YAML supersession refs, and the renamed
  ADR's `number:`), a moderate change to what is today pure text manipulation.
- **Status:** the oracle models the actual behavior (a frontmatter renumber
  doesn't follow supersession pointers) so it keeps hunting other bugs. Fix
  candidates: make `renumber` rewrite frontmatter YAML refs, and extend `check`
  to validate frontmatter supersession.

### 9. `per_category` *same-category* cross-ADR links don't resolve

- **Found by:** the full-matrix oracle (by_category cell), same-category
  `supersede`. The link `./0002-beta.md` is a valid file path but `check` reports
  the supersession as "no such ADR exists".
- **Cause:** a per_category identity is `category/NNNN`, and `naming::ref_in_link`
  recovers the category from the link's **path** — which works for a
  cross-category `../infra/0001-x.md` link but not for a canonical same-category
  `./0002-beta.md` link, which carries no category segment. Resolution needs the
  *source* file's category, which the scheme-agnostic `ref_in_link` (used by
  `relink` and `check`) isn't given.
- **Why deferred:** the proper fix threads source-category context through
  `ref_in_link` and its callers (`relink`, `relink_one`, `check`'s supersession +
  link checks, and `format`'s supersession parse) — a cross-cutting change to
  central relink/check logic with regression risk, for a peripheral scheme.
  Cross-category supersession (the common case) works after fix #7.
- **Status:** the oracle exercises cross-category per_category supersession and
  skips the same-category case (documented here). Fix candidate: a
  `ref_in_link_from(target, source_category)` that falls back to the source
  category when the target carries no category segment.

## Forge fault-injection (`tests/forge_faults.rs`, `--features forge`)

A `HostileTransport` returns arbitrary status codes and malformed / truncated /
wrong-typed / oversized / null response bodies (plus an injected connection
failure) to every `Forge` + `Tracker` method on all three adapters
(GitHub / GitLab / Jira).

- **Result: no parsing bug.** At 5000 cases the adapters never panic and always
  return a clean `Result` — the response parsing (built on the `HttpTransport`
  seam) is already robustly defensive. A positive finding.
- **Minor consistency fix:** `Jira::with_transport` was `#[cfg(test)]`-gated while
  the GitHub/GitLab equivalents are public; exposed it to match (so the
  fault-injection suite can build all three adapters over an injected transport).
  (src/forge/jira.rs)
- **Note:** `Jira`'s `Forge` impl is an intentional `unreachable!` guard (Jira is
  only ever wired as a Tracker); the suite exercises only its tracker side.
