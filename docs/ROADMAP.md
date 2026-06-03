# Roadmap / tracking notes

Internal notes (not part of the published user manual). Tracks deferred ideas
and their rationale.

## Deferred: database-backed read model (spike)

**Status:** deferred — investigate after the typed-links + wiki-graph work lands.

**Context.** While dogfooding, Mike raised that managing linked, typed documents
"is tailor made for SurrealDB — it's embeddable and easy to use," and floated
SQLite ("stuff them in and generate the markdown on the fly," "regenerate the
entire site in a few milliseconds").

**Decision (for now): keep plain markdown files as the source of truth.** The
file-first model is a deliberate, valuable property:

- ADRs are **git-reviewable and PR-diffable** — the decision record *is* the
  reviewed artifact; a binary DB is not.
- No separate state to back up, migrate, or keep in sync; the repo is portable.
- Performance is **already** millisecond-scale (ADR repos are tiny — tens to
  low-hundreds of files; the store reads them all in well under the "few
  milliseconds" bar, and `adroit serve` reopens per request).
- The "structure and type information" Mike wants is now expressible **in the
  files**: the `frontmatter` profile + typed relational links (`relates_to` /
  `depends_on` / `refines`) + the relationship graph deliver typed, queryable
  structure without inverting the source of truth.

**What a spike would actually evaluate (later, time-boxed):** an **embedded
SurrealDB (or SQLite) read model built *from* the files** — i.e. an index/cache
the query + graph layers can hit for richer relational queries (path-finding,
"what depends on X transitively", multi-hop graph queries), with files remaining
authoritative. This is additive (a new read backend behind the existing
`query`/`view` seam), not a storage rewrite.

**Entry criteria to schedule the spike:** a concrete query/graph need the
current file-scan + `query::graph` can't serve well (e.g. transitive dependency
analysis at a scale where re-scanning is too slow, or graph queries the SVG
view can't express). Until then, the file model + the wiki-graph cover the need.
