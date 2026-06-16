# ADR-0006: Flag-level escalation semantics in the manifest

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

The manifest's `classified()` table
([ADR-0004](./0004-manifest-semantics-live-in-an-owned-classified-table.md))
declares semantics **per verb**. That is too coarse, and the gap is verified
live: a flag can escalate a read verb into something else entirely —
`review --forge` un-drafts the PR, posts comments, and adds labels; `--out` on
`review` / `plan` / `summarize` writes arbitrary files; `list --forge` /
`check --forge` add network cost. The MCP server's "read-only" projection
([ADR-0005](./0005-mcp-server-projects-only-read-only-manifest-verbs.md))
filters per verb, so `tools/list` today projects `review` as read-only with
`forge`, `yes`, `dry_run`, and `out` still in its input schema — a real write
leak through the default agent surface.

Downstream consumers compensate with hardcoded denylists, which re-encodes
behavior knowledge outside the manifest and silently re-opens whenever a new
escalating flag ships.

## Decision Drivers

- Close the verified MCP write leak before any new MCP consumer appears.
- One source of truth (the principle behind ADR-0004): escalation belongs in
  the manifest, not in each consumer's denylist.
- Drift-proofing: a future forge-gated or `--out` flag on a read verb must not
  silently re-open the leak.
- Additivity: downstream consumers ignore unknown manifest fields, so the
  change must be purely additive to `manifest -o json`.

## Considered Options

1. **Declare escalation per (verb, flag) in `src/manifest.rs`**, serialize it
   as an `escalates` field on `OptionInfo` in `manifest -o json`, have the MCP
   projection strip escalating flags from tool schemas, and add a coverage
   test that fails CI when a forge-gated or `--out`-style flag on a read verb
   lacks classification.
2. **Hardcode flag denylists in the MCP layer** (`src/mcp/tools.rs`) and in
   each downstream consumer.
3. **Split escalating verbs into separate commands** (`review` vs
   `review-forge`), keeping per-verb semantics sufficient.

## Decision Outcome

Chosen: **Option 1 — flag-level escalation declared in the manifest**, because
it keeps the manifest the single source of truth and makes every downstream
filter mechanical. Option 2 is the current de-facto state that produced the
leak; option 3 would multiply the CLI surface and break the existing opt-in
flag idiom (`--forge` / `--dry-run` / `--yes`) that humans already use.

The known escalations to classify at adoption time: `review`: `forge`, `yes`,
`dry_run`, `out`; `plan`: `out`; `summarize`: `out`; `list` / `check`:
`forge`. Stripping the projected arguments is sufficient on the MCP side
because `build_argv` already ignores unknown keys. The coverage test mirrors
`manifest_classifies_every_command`: every flag matching the escalation
heuristics on a read verb must carry a classification, or CI fails.

This decision is accepted ahead of its implementation milestone; the
escalation table and MCP stripping land as the next structural change to the
manifest (M2 in the iteration-1 direction).

### Positive Consequences

- The MCP projection becomes truly read-only — flag set included — and stays
  that way mechanically.
- Downstream allowlists can be derived from `manifest -o json` instead of
  hand-maintained denylists.
- The manifest's honesty improves for human readers too: `--help`-level
  semantics and machine semantics stop disagreeing.

### Negative Consequences

- A second hand-maintained classification axis (per-flag, alongside per-verb)
  — the same "entry exists but may be untrue" residual risk as ADR-0004, only
  partially mitigated by the coverage test's heuristics.
- The manifest schema grows; consumers that want the safety must learn the
  `escalates` field (ignoring it is safe only because the MCP projection
  strips server-side).
- MCP clients lose legitimate uses of stripped flags (e.g. `plan --out`); the
  CLI remains the escape hatch.

## Implementation

To land (M2): the per-(verb, flag) escalation table in `src/manifest.rs`;
`escalates` serialized on `OptionInfo`; `mcp::Server::new` stripping
escalating flags from projected tool schemas; regression tests asserting the
`review` / `plan` / `summarize` tool schemas contain none of `forge` / `yes` /
`dry_run` / `out`; the coverage test failing CI on an unclassified escalating
flag; doc-sync of `automation.md` and `testing.md`.
