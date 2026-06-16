# ADR-0009: Defer embeddings behind a measured retrieval-miss criterion

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

adroit's retrieval — `related`, `dedupe`, and the grounding for `ask` — is
mechanical TF-IDF cosine over the corpus (`src/similar.rs`): deterministic, no
network, no keys, no index. The rig dependency
([ADR-0002](./0002-adopt-rig-for-adroit-s-ai-integration.md)) ships an
embeddings and vector-store surface, so the temptation to "upgrade" retrieval
to semantic embeddings is standing — it would be a small patch and a good
demo.

But no measured retrieval failure exists. No dogfood run has shown `dedupe`
missing an obvious duplicate or `related` missing an obviously relevant
record. Adding embeddings now would trade determinism, offline operation, and
zero-config simplicity for a quality improvement nobody has demonstrated
needing — and an unbounded "make retrieval smarter" lane invites exactly the
speculative complexity the codebase's design rules reject.

## Decision Drivers

- Simplicity first: no speculative machinery without a demonstrated need.
- The statelessness invariant
  ([ADR-0003](./0003-statelessness-and-idempotency-as-architectural-invariants.md)):
  no persisted index, whatever is decided.
- Determinism and offline operation are features of the current retrieval —
  CI and keyless environments rely on them.
- The door must stay open honestly: "deferred" needs a concrete entry
  criterion, or it is just drift waiting to be re-argued.

## Considered Options

1. **Defer with an explicit entry criterion**: `similar.rs` stays mechanical
   TF-IDF; an embeddings rerank is built **only if** dogfooding shows
   `dedupe` / `related` / `ask` missing obvious matches on a real seeded
   backlog. If triggered, the shape is fixed in advance: a local-model
   embedding rerank over the TF-IDF top-K candidates, no persisted index —
   recompute per invocation.
2. **Build the embeddings rerank now**, behind the `ai` config, since rig
   already provides the client surface.
3. **Close the door** — declare TF-IDF the permanent retrieval mechanism.

## Decision Outcome

Chosen: **Option 1 — defer behind a measured retrieval miss**, because it
keeps the mechanical path the default and the only path until reality, not
taste, demands more. Option 2 adds a network dependency, model variance, and a
second retrieval lane with no evidence of need. Option 3 pretends to a
certainty nobody has either — lexical retrieval may genuinely cap out on a
larger corpus of paraphrased decisions.

The pre-committed shape of the *triggered* version preserves every invariant:
rerank only (TF-IDF still selects candidates, so the mechanical path remains
primary and untouched), a local embedding model via the existing provider
seam, and **no persisted index** — embeddings are recomputed per invocation,
exactly as ADR-0003 requires. If the criterion ever fires, the triggering miss
gets recorded in the ADR that accepts the work.

### Positive Consequences

- `related` / `dedupe` / `ask` retrieval stays deterministic, offline,
  keyless, and CI-usable — no model variance in test assertions.
- No index lifecycle, no embedding-cache invalidation, no new state (the
  ADR-0003 bug classes stay impossible).
- The decision is cheap to revisit honestly: the entry criterion states
  exactly what evidence reopens it, and the implementation shape is already
  agreed.

### Negative Consequences

- Retrieval quality is capped at lexical matching until the criterion fires —
  paraphrased duplicates with disjoint vocabulary *can* slip past `dedupe`,
  and `ask` grounding inherits the same ceiling.
- The criterion depends on someone noticing and recording a miss during
  dogfooding; an unmeasured degradation could persist unrecorded.
- If triggered, per-invocation recomputation means every rerank pays an
  embedding call for the candidate set — the statelessness tax, accepted in
  advance.

## Implementation

Nothing to build now — that is the decision. Standing guardrails: `similar.rs`
remains mechanical and provider-free; any future rerank must arrive with
property tests proving the mechanical path is untouched, a recorded
triggering miss, and no persisted state. Until then, retrieval changes are
limited to mechanical improvements (tokenization, weighting) under the
existing tests.
