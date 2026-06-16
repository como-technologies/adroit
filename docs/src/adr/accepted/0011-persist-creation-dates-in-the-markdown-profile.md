# ADR-0011: Persist creation dates in the markdown profile

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

The markdown profile persists no creation date: `created` resolves from git
history when available, else file mtime, else "now". On a corpus without git
history that fallback is actively misleading — a clone resets mtime, and every
`set-status` or `plan --save` rewrite re-stamps `created` to the moment of the
rewrite. The iteration-1 read-path rehearsal caught exactly this: a consumer
treating `created` as decision provenance watched it move on every status
change. Wall-clock derivation is fragile even *with* git on the machine doing
the deriving — the iteration-2 integration gate flaked on a
`created <= last_modified` assertion when the host clock stepped backwards
between two commits.

Where should a markdown ADR's creation date live so that `created` is stable
decision provenance under rewrites, clones, and stepping clocks?

## Decision Drivers

- The statelessness invariant
  ([ADR-0003](./0003-statelessness-and-idempotency-as-architectural-invariants.md)):
  the document is the only state — provenance must live in the corpus, not in
  filesystem metadata that rewrites and clones destroy.
- Rewrite stability: `set-status`, `set-review`, and `plan --save` must never
  move `created` (the run-1 finding).
- Git history is the richer record where it exists (full lifecycle timeline,
  real timestamps) — the document date must complement it, not fight it.
- Format preservation: the markdown profile's minimal-diff writes and
  byte-identical round-trips are load-bearing for every verb.
- The frontmatter profile already persists a full `created:` timestamp in its
  YAML — the markdown profile is the gap.

## Considered Options

1. **Persist a `Created: YYYY-MM-DD` line in the `## Status` region**, stamped
   once by `new`, parsed like `Review by:` (format-preserving rewriter, same
   insertion anchor), with git history remaining authoritative under
   `date_source` auto/git.
2. **Derive exclusively from git** (`--date-source git` made mandatory):
   refuse or warn on non-git corpora, never trust mtime.
3. **A sidecar metadata file** (e.g. `.adroit/dates.yaml`) mapping ADR ids to
   creation dates.

## Decision Outcome

Chosen: **Option 1 — persist `Created: YYYY-MM-DD` in the document's
`## Status` region**, because the document is the corpus's unit of truth and
the `Review by:` line already proves the pattern: a human-readable,
format-preserved, mechanically parsed line in the status region. Git-only
(option 2) abandons the legitimate non-git corpus (a fresh export, a tarball,
a docs tree) and still inherits clone/shallow-fetch hazards; a sidecar
(option 3) violates the single-file model and ADR-0003 statelessness exactly
as ADR-0008 rejected for plans.

Resolution precedence for `created` becomes: **git first-add date → authored
document date (frontmatter `created:` or markdown `Created:`) → mtime →
now()**. Git stays first because where history exists it is the richer, harder
record; the document line makes the no-git case deterministic instead of
mtime-derived. The stamp is written once by `new` (CLI and TUI), never
re-stamped by any rewrite; pre-ADR-0011 documents simply have no line and keep
the old fallback — no migration, converge-don't-accumulate.

### Positive Consequences

- `show -o json` `created` is byte-stable across `set-status` and
  `plan --save` on a non-git corpus — pinned by a named CLI regression.
- Provenance survives clone, export, and CI checkout; no clock participates
  after the stamp.
- One mechanism, both profiles: the markdown line mirrors the frontmatter
  field; `query` resolution changes in exactly one place.
- The rewriter (`rewrite_created`) reuses the `Review by:` algebra — upsert
  idempotent, removal exact, untouched documents byte-identical — and is
  property- and fuzz-tested alongside it.

### Negative Consequences

- Two sources can disagree: a hand-edited `Created:` line wins over mtime but
  loses to git — the precedence is documented, but a surprised user may expect
  the line to be authoritative everywhere.
- Existing corpora gain the stamp only on newly created ADRs; mixed corpora
  (stamped and unstamped) resolve dates by different fallbacks until authors
  backfill — visible, not converged automatically.
- The status region grows another mechanical line that templates, examples,
  and parsers must tolerate (handled: the parser ignores unknown lines, and
  `Created:` is recognized case-insensitively).
- A date-only stamp truncates the timestamp to midnight UTC — coarser than
  frontmatter's full timestamp, by design (a decision date, not an audit log).

## Implementation

Landed with this decision (fix-train M4):

- `adr::CreatedOn` newtype (ISO `YYYY-MM-DD`, mirrors `ReviewBy`);
  `Adr.created_on` carries the parsed line.
- `format::parse_status_region` recognizes `Created:`;
  `format::rewrite_created` is the format-preserving upsert/remove rewriter
  (same insertion anchor as `Review by:`).
- `new` (CLI `cmd_new` + TUI `create_adr`) stamps the line at creation; no
  other verb writes it.
- `query::resolve_dates` inserts the document date between git and mtime.
- Tests: named CLI regression
  (`created_is_byte_stable_across_set_status_and_plan_save_without_git`, with
  a backdated mtime so the failure mode is deterministic), parser property
  (`rewrite_created_round_trips_and_is_idempotent`), bolero fuzz coverage in
  `fuzz_format_helpers`, and pinned commit dates in the git-date tests (the
  clock-step deflake).
