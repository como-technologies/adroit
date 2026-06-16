# ADR-0008: Persist implementation plans inside the ADR document

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

`adroit plan <ID>` is generate-only today: it requires an AI provider on every
call, its output is nondeterministic, and nothing is persisted. For a human
that is a usable drafting aid; for the workflow the portfolio direction
defines, it is the wrong shape. A plan is *decision content* — the "how" that
accompanies an accepted "what" — and downstream consumers in the Adopt stage
need to read it deterministically, provider-free, over the same read seam as
the rest of the corpus. Today any such consumer must supply AI environment to
a read, snapshot nondeterministic output, and accept that two reads of the
same decision can disagree — which blurs the boundary between *prescribing*
work and *adopting* it.

Where should a plan live so that reading one is deterministic and writing one
is an explicit Prescribe-time event?

## Decision Drivers

- The statelessness invariant
  ([ADR-0003](./0003-statelessness-and-idempotency-as-architectural-invariants.md)):
  the corpus is the only state, so a persisted plan must live in the corpus.
- The single-file model: `relink`, `check`, `publish`, `renumber`, and both
  format profiles must keep working without learning a new artifact type.
- Human reviewability: the plan should be reviewed with the decision it
  implements, in the same diff.
- Deterministic reads: `plan <ID>` after persistence must need no provider and
  return identical bytes on every call.

## Considered Options

1. **Splice the plan into the ADR document itself**: `plan --save` writes a
   `<!-- adroit:plan -->`-marked `## Implementation` section via the existing
   splice machinery; `plan <ID>` returns the stored section deterministically
   with no provider; `--regenerate` forces a fresh AI call.
2. **A sidecar file per ADR** (e.g. `adr/plans/NNNN-plan.md`) referenced from
   the document.
3. **An external store** (separate plans repo, database, or downstream-owned
   snapshots only).

## Decision Outcome

Chosen: **Option 1 — the plan persists inside the ADR document**, because the
document is already the unit everything else operates on. A sidecar (option 2)
doubles the file count, breaks the single-file model every verb assumes, and
splits a decision's review across two diffs. An external store (option 3)
violates the statelessness invariant outright or — in the
"downstream-snapshots-only" variant — leaves generation nondeterministic and
provider-bound forever, which is the status quo being rejected.

With this decision, plan *generation* becomes a Prescribe-time imperative
event (like `new` and `draft`: intentionally non-idempotent, ADR-0003), and
plan *reading* becomes a deterministic corpus read: `plan <ID>` with a stored
plan returns it verbatim with no provider; `plan -o json` gains an additive
`stored: bool`; the detail view carries the plan so `show -o json` exposes it.
The `--save` / `--force` / `--regenerate` flags are escalating writes under
[ADR-0006](./0006-flag-level-escalation-semantics-in-the-manifest.md), so the
MCP `plan` tool stays read-only and becomes deterministic.

This decision is accepted ahead of its implementation milestone (M3 in the
iteration-1 direction).

### Positive Consequences

- Reading a plan needs no AI environment and is byte-deterministic — exactly
  what an Adopt-stage consumer must snapshot and verify against.
- The plan is reviewed in the same PR hunk as the decision; staleness is at
  least *visible* in one document.
- No new artifact type: `relink` heals links inside plans, `check` validates
  the document, `publish` renders it, both format profiles round-trip it.
- The Prescribe/Adopt boundary becomes clean: generation (AI, human-gated)
  happens at accept time; consumption is a pure read.

### Negative Consequences

- The splice touches the minimal-diff / byte-identical round-trip invariants
  in **both** format profiles — a bug here corrupts the corpus every other
  verb depends on. Mitigation is non-negotiable: the model oracle, the
  `commands_are_idempotent` and `dry_run_changes_nothing` guards, and refusing
  to overwrite an existing plan without `--force`.
- ADR documents grow; a long checklist lives inside a decision record, and
  `show` output gets correspondingly heavier.
- A stored plan can drift stale against a later edit of the decision text with
  no mechanical staleness signal — keeping them coherent is an authoring
  discipline, not a checked invariant.
- `plan`'s JSON contract changes shape (additively); consumers pinning exact
  schemas need the `stored` field to be genuinely optional.

## Implementation

To land (M3): `plan --save` splicing the marked `## Implementation` section
(refuse overwrite without `--force`); provider-free deterministic read path;
`--regenerate`; additive `stored: bool` in `plan -o json` and an optional
`plan` on the detail view; escalation classification for `--save` / `--force`
/ `--regenerate` (ADR-0006 mechanism); oracle `Op` for the write path plus
idempotency and dry-run guards in both profiles; doc-sync.

Landed (M3, `src/plan.rs`) with three refinements the implementation surfaced:

- The section is bracketed by a marker **pair** — `<!-- adroit:plan -->` …
  `<!-- /adroit:plan -->`. The end marker keeps free-form plan markdown (its
  own `## ` sub-headings included) reading back verbatim; a single begin
  marker with a stop-at-the-next-heading rule would silently truncate it,
  breaking the verbatim guarantee this decision exists to give.
- `--save` replaces the templates' placeholder `## Implementation` section
  (the italic `_…_` prompt) in place, but refuses a hand-written one outright:
  only marked or placeholder content is ever overwritten, `--force` or not.
- The AI body splice (`draft` / `compose` / `import --ai`) re-splices a stored
  plan rather than discarding it with the prose — a stored plan is mechanical
  decision content, not prose. Found by the model oracle.
