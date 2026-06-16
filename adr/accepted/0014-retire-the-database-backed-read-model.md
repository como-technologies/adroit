# ADR-0014: Retire the database-backed read model

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

The roadmap has carried a "database-backed read model (spike)" since
dogfooding began: a SurrealDB/SQLite index for richer relational queries,
floated when the wiki-graph work made multi-hop questions imaginable. The
roadmap already leans "keep files as the source of truth", but as a prose
hedge, not a decision — it remains an open invitation to build an index.

Meanwhile the evidence accumulated the other way: every real corpus (this
repo's `adr/`, the dogfood repos, the run-1 seeded backlog) is tens of files;
the full re-scan behind `query` is millisecond-scale; and no dogfood run has
produced a query the file scan plus `query::graph` could not serve. Should
the spike stay open, or be retired by decision with its entry criterion on
record?

## Decision Drivers

- ADR-0003: the filesystem is the **only** state — an index is either
  derived-and-disposable (cache) or a second source of truth (violation).
- No measured need: re-scan latency is invisible at real corpus sizes, and
  no query has missed.
- The mandate's built-or-retired bar: an open-ended "spike someday" is
  neither.
- The `query`/`view` seam already isolates consumers from the storage
  strategy — an index later would be an internal swap, not a contract
  change.

## Considered Options

1. **Retire behind a measured entry criterion** (the ADR-0009 pattern):
   files remain the only store; the spike reopens on demonstrated need.
2. **Time-boxed spike now**: build the embedded files-derived index and
   measure it speculatively.
3. **Adopt a database outright** as the primary store.

## Decision Outcome

Chosen: **Option 1 — retire the read-model spike behind its entry
criterion**, because every driver points the same way: the invariant says
files, the measurements say files are fast enough, and the seam means
deferring costs nothing architecturally. A speculative spike (option 2)
produces a cache nobody queries and a maintenance surface; a primary
database (option 3) breaks ADR-0003 outright and makes the corpus
non-reviewable.

**Reopen criterion** (carried over from the roadmap, now binding): a
**concrete query or graph need, at a corpus scale where re-scanning is
measurably too slow** — e.g. transitive dependency analysis across hundreds
of ADRs with observed multi-second reads. If embeddings ever land
(ADR-0009's own criterion), the vector cache and this index share the
files-derived-cache design — the two reopen together or not at all.

### Positive Consequences

- ADR-0003 stays unqualified: corpus state is reviewable, diffable, and
  backup-free.
- No schema/migration/cache-invalidation surface to maintain for a need
  nobody has hit.
- The `query`/`view` seam keeps the swap available at the same price later.

### Negative Consequences

- A future genuinely-slow corpus pays the latency until someone notices and
  reopens — the criterion requires the pain to be felt first, by design.
- Multi-hop graph questions stay at "what `query::graph` computes"; an
  analyst wanting ad-hoc relational queries over ADR metadata has no SQL
  surface.
- "Retired" can read as "rejected forever" — it is not; the criterion is the
  contract (as with ADR-0009, whose criterion has simply never fired).

## Implementation

Nothing to build — this ADR records the disposition. The roadmap's
"Deferred / under consideration" entry now cites this ADR as the binding
form of its hedge.
