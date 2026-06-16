# ADR-0003: Statelessness and idempotency as architectural invariants

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

*Recorded retroactively: this invariant has governed every write path since the
early store work and is enforced by guard tests; this record reconstructs the
decision behind it (see
[ADR-0001](./0001-reinstate-the-in-repo-adr-corpus.md)).*

adroit operates on a directory of plain-markdown decision records that humans
also edit by hand, review in PRs, and read on a forge. As features accrued
(search, stats, graph, similarity, a web dashboard, forge sync), each one
invited some form of persistent runtime state — a search index, a cache, a
daemon, a lock file, cross-command memory. Every such addition would change
what a command's behavior *depends on* beyond the files in front of it, and
would make re-running a command on an already-converged repo produce drift.

The question had to be settled once, as an invariant, rather than re-litigated
per feature.

## Decision Drivers

- The corpus is shared with humans and git: anything not in the files is
  invisible to review and lost on clone.
- Re-runnable automation (CI, bots, agents) needs commands whose output depends
  only on visible inputs, and whose re-run is safe.
- PR-based workflows need minimal diffs — a command must not rewrite what it
  did not need to change.
- Simplicity: no daemon, no index lifecycle, no cache-invalidation class of
  bugs.

## Considered Options

1. **Filesystem-only state, converging writes**: a command's input is the ADR
   docs on disk plus config resolved at startup; mutating commands compute the
   target state and write only what differs; no persisted runtime state of any
   kind.
2. **A persisted index/cache** (e.g. an index file or embedded DB) maintained
   alongside the corpus for fast search/stats/similarity.
3. **A long-running daemon** owning corpus state, with the CLI as a client.

## Decision Outcome

Chosen: **Option 1 — the only state is the filesystem**, because every
consumer (human, git, CI, agent) can already see and reason about files, and
the corpus stays the single source of truth.

The invariant in full, as enforced today:

- A command's input is the ADR docs on disk plus config resolved at startup
  (flag > process-env > `.env` > `config.yaml` > default). No daemon, database,
  cache, index, lock file, or cross-command state. `adroit serve` reopens the
  store per request.
- **Converge, don't accumulate**: a mutating command reads current state,
  computes the target, and writes only what differs; a file already in target
  state round-trips byte-identical.
- **Idempotent verbs** (re-run = byte-identical): `set-status`, `supersede`,
  `set-review`, `relink`, `migrate`, `index`, `link`, `publish`, `check`.
- **Intentionally non-idempotent imperative events** (repeating repeats the
  event, by design): `new`, `renumber`, `notify`, forge/git side effects.
- **`--dry-run` is a true full preview** — it mutates nothing, local or forge.

Guard tests `commands_are_idempotent` and `dry_run_changes_nothing`
(`tests/cli.rs`) make the invariant regression-checked, and the model-based
oracle (`tests/model.rs`) exercises it across the format × layout × scheme
matrix.

### Positive Consequences

- Commands are deterministic functions of visible inputs — safe to re-run, easy
  to test, friendly to CI and agent consumers.
- No cache-invalidation or index-corruption bug class; `git clone` is a full
  backup and a full install.
- Minimal-diff writes keep PRs reviewable and merge conflicts rare.
- New surfaces (web, MCP) compose for free: they reopen the store and read.

### Negative Consequences

- Every invocation re-reads and re-parses the corpus — O(corpus) per command,
  with no cross-command speedup; very large corpora pay it every time.
- Retrieval (`similar.rs`) must recompute TF-IDF per invocation; the same rule
  binds any future embeddings rerank (see
  [ADR-0009](./0009-defer-embeddings-behind-a-measured-retrieval-miss-criterion.md)).
- Conveniences that need persistent state (undo history, watch-state, session
  memory) are off the table by design.
- Every new write path carries a proof burden: it must keep converging writes
  and dry-run purity, and extend the guard tests.

## Implementation

Standing enforcement, already shipped: the `Store` write path computes
minimal diffs (`format::serialize` round-trips byte-identical);
`tests/cli.rs::commands_are_idempotent` runs the idempotent verbs twice and
asserts byte-identical trees; `dry_run_changes_nothing` guards the preview
contract; CLAUDE.md documents the invariant as a hard rule for every change.
New write paths (e.g. `plan --save`, see
[ADR-0008](./0008-persist-implementation-plans-inside-the-adr-document.md))
must land with the same guards extended.
