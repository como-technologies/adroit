# ADR-0005: MCP server projects only read-only manifest verbs

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

*Recorded retroactively: this decision shipped with `adroit mcp` (`src/mcp/`);
this record reconstructs it (see
[ADR-0001](./0001-reinstate-the-in-repo-adr-corpus.md)).*

Agent runtimes speak the Model Context Protocol; adroit should be usable as an
MCP server so an agent can query a decision corpus without shelling out. But an
MCP client is typically an LLM choosing tools autonomously — handing it
mutating verbs (`set-status`, `supersede`, `renumber`) or network/long-running
verbs by default turns every prompt-injection or model mistake into corpus
damage or forge side effects. The server needed a stance on what to expose,
and a way to keep that stance true as verbs are added.

## Decision Drivers

- Safety by default: an autonomous client must not be able to mutate the
  corpus or touch a forge through the default surface.
- One source of truth: the read/write knowledge already lives in the
  manifest's `classified()` table
  ([ADR-0004](./0004-manifest-semantics-live-in-an-owned-classified-table.md));
  the MCP layer should filter on it, not re-encode it.
- The sync-core invariant: no async MCP SDK pulling `tokio` into the default
  build.
- New read verbs should appear as MCP tools automatically, with no MCP-side
  edit.

## Considered Options

1. **Project only the manifest's read verbs as tools**: `Server::new` filters
   `classified()` to `is_read_tool()` (`reads && !writes && cost ∈ {local,
   provider-call}`); a `tools/call` re-runs `adroit <verb> … -o json` as a
   subprocess; hand-rolled synchronous JSON-RPC 2.0 over stdio.
2. **Expose the full CLI surface** as tools, relying on client-side
   confirmation for mutating calls.
3. **A separate wrapper tool** (external MCP server shelling into adroit),
   keeping MCP out of this codebase.

## Decision Outcome

Chosen: **Option 1 — a read-only projection of the manifest**, because it
makes the default surface safe without trusting client UX (option 2), and
keeps the projection in lockstep with the binary instead of drifting in a
second codebase (option 3).

The projection derives from the manifest's semantics table, so a new read verb
auto-appears as a tool and a verb classified as writing never does. The server
is a hand-rolled sync stdio loop; `handle_line` is a pure `&str ->
Option<String>` (fuzzed via `fuzz_mcp_request`), and `tools/call` re-runs the
binary as a subprocess with the resolved on-disk shape passed as env — the
statelessness invariant
([ADR-0003](./0003-statelessness-and-idempotency-as-architectural-invariants.md))
is what makes subprocess-per-call correct.

### Positive Consequences

- Repo-mutating, forge-touching, and long-running verbs are never exposed by
  default — the failure mode of a confused agent is a failed read, not a
  changed corpus.
- Zero MCP-side maintenance for new read verbs; the manifest remains the one
  place behavior is declared.
- No async runtime in the default build; the pure `handle_line` seam is
  trivially testable and fuzzable.

### Negative Consequences

- **Per-verb filtering is provably too coarse**: flags escalate read verbs
  into writes, and the live server projects `review` as "read-only" while its
  schema still carries `forge` / `yes` / `dry_run` / `out` — a real write
  leak, closed by flag-level semantics in
  [ADR-0006](./0006-flag-level-escalation-semantics-in-the-manifest.md).
- The `is_read_tool()` filter needed a hardcoded `publish` exclusion because
  the manifest misclassified it — a smell resolved by
  [ADR-0007](./0007-reclassify-publish-as-a-write-in-the-manifest.md).
- Write workflows over MCP are simply unavailable; clients that legitimately
  need them must shell out to the CLI instead.
- Subprocess-per-call pays process spawn + corpus re-parse on every tool call
  (the statelessness trade, accepted in ADR-0003).

## Implementation

Shipped: `src/mcp/` behind the default-on `mcp` feature (requires `manifest`);
`Server::new` filtering via `is_read_tool()`; subprocess dispatch with the
resolved `--dir`; the `fuzz_mcp_request` target. Follow-up: strip escalating
flags from projected tool schemas (ADR-0006) and delete the publish special
case (ADR-0007), making the projection mechanical end to end.
