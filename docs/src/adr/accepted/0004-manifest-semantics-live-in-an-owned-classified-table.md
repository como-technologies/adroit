# ADR-0004: Manifest semantics live in an owned classified() table

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

*Recorded retroactively: this decision shipped with `adroit manifest`
(`src/manifest.rs`); this record reconstructs it (see
[ADR-0001](./0001-reinstate-the-in-repo-adr-corpus.md)).*

Agent consumers need to *discover* the CLI surface machine-readably before
driving it: which commands exist, what they accept, what JSON they emit — and,
crucially, what they *do* (read vs write, idempotent or not, what stage of the
workflow they belong to, what runtime config they require, how they exit).

The first two layers can be derived mechanically: **syntax** from the clap
`Command` tree, **output schemas** from `schemars` over the `view` types. The
third layer — **semantics** — cannot be derived from anything: whether `check`
writes, whether `set-status` is idempotent, whether `plan` needs an AI provider
is owned knowledge about behavior. The question was where that knowledge should
live so it stays correct as the CLI grows.

## Decision Drivers

- Drift-proofing: a new subcommand must not silently ship without declared
  semantics.
- One source of truth: downstream filters (the MCP projection, agent
  allowlists) must derive from the manifest, not re-encode behavior knowledge.
- Honesty over cleverness: derived-looking semantics that are actually guesses
  are worse than an explicit owned table.
- Core-build friendliness: the manifest must reflect exactly the commands
  compiled into the running binary.

## Considered Options

1. **An owned `classified()` table** in `src/manifest.rs` — one entry per
   command declaring `reads` / `writes` / `idempotent` / `stage` /
   `json_output` / `requires` / `exit` — with a coverage test
   (`manifest_classifies_every_command`) that fails CI when a compiled command
   lacks an entry.
2. **Derive semantics from attributes/macros** on the command definitions
   (custom derive or annotation parsed at build time).
3. **Syntax-only manifest** — emit the clap tree and schemas, declare no
   semantics, and let each consumer hardcode its own behavior table.

## Decision Outcome

Chosen: **Option 1 — a hand-maintained, owned table guarded by a coverage
test**, because semantics are owned knowledge: a macro (option 2) only moves
where the human writes the same facts while adding build machinery, and
option 3 pushes the table into every consumer, guaranteeing divergence.

The drift risk of hand maintenance is answered structurally:
`manifest_classifies_every_command` walks the compiled clap tree and fails CI
on any command without a `classified()` entry, so the table cannot fall behind
the surface. `requires` captures runtime gating (`["ai", "ai.enabled"]`,
`["forge config"]`) so consumers can predict failures before invoking. The
manifest is handled before the store opens, and a core build that compiles
commands out drops their entries with them.

### Positive Consequences

- One queryable source of truth for command behavior; downstream allowlists
  and the MCP projection (see
  [ADR-0005](./0005-mcp-server-projects-only-read-only-manifest-verbs.md))
  filter on declared semantics instead of hardcoding verb lists.
- The coverage test turns "forgot to classify the new verb" from silent drift
  into a red CI.
- `manifest -o json` gives agent consumers a handshake artifact (`tool`,
  `manifest_schema`) plus the full catalog in one call.

### Negative Consequences

- The table is hand-maintained: the coverage test forces an entry to *exist*,
  but cannot verify the entry is *true* — a mis-described command (wrong
  `writes`, missing `requires`) survives until review or a downstream consumer
  is bitten.
- Per-verb granularity is coarse: a flag can escalate a read verb into a write
  (`review --forge`, `--out`), which per-verb semantics cannot express — the
  gap is real, verified live, and addressed by
  [ADR-0006](./0006-flag-level-escalation-semantics-in-the-manifest.md).
- Every new command costs one more deliberate classification decision
  (accepted: that is the point).

## Implementation

Shipped: `src/manifest.rs` with the three layers (clap-derived syntax,
schemars-derived output schemas, the owned `classified()` semantics table);
the `manifest_classifies_every_command` coverage test; the default-on
`manifest` feature (`dep:schemars`) that a core build drops. Follow-up
decisions extend the same table rather than adding parallel sources:
flag-level escalation
([ADR-0006](./0006-flag-level-escalation-semantics-in-the-manifest.md)) and
the publish reclassification
([ADR-0007](./0007-reclassify-publish-as-a-write-in-the-manifest.md)).
