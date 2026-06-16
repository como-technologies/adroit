# ADR-0007: Reclassify publish as a write in the manifest

> State: Accepted

## Status

Accepted

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

`adroit publish --to <target>` renders the accepted set into an output tree on
disk. The manifest's `classified()` table
([ADR-0004](./0004-manifest-semantics-live-in-an-owned-classified-table.md))
currently classifies it as a non-writing verb — presumably because it never
mutates the *corpus* — and the MCP read-only projection
([ADR-0005](./0005-mcp-server-projects-only-read-only-manifest-verbs.md))
compensates with a hardcoded `c.name != "publish"` special case in
`src/mcp/tools.rs`.

That is the manifest lying and a consumer patching around the lie: producing
an output tree **is** a write to the filesystem, whatever it leaves untouched.
The special case is exactly the "consumer re-encodes behavior knowledge"
pattern the manifest exists to eliminate, and any *other* consumer of
`manifest -o json` that trusts `writes=false` on `publish` inherits the
misclassification without the patch.

## Decision Drivers

- Manifest honesty: declared semantics must describe actual behavior — a
  filesystem write is a write.
- One source of truth: no consumer-side special cases for facts the manifest
  should state (the ADR-0004 principle).
- Protect non-MCP consumers who filter on `writes` and do not know about the
  MCP layer's patch.

## Considered Options

1. **Reclassify `publish` as `writes=true` in `classified()`** and delete the
   hardcoded `publish` exclusion from the MCP projection.
2. **Keep the status quo** — `writes=false` plus the hardcoded MCP special
   case.
3. **Introduce a third semantic category** (e.g. "writes outside the corpus")
   distinct from corpus writes.

## Decision Outcome

Chosen: **Option 1 — declare `publish` a write and drop the special case**,
because it makes the manifest true and the MCP filter mechanical. Option 2
keeps a standing lie plus a patch only one consumer knows about. Option 3
encodes a real nuance, but no consumer needs the distinction today and the
flag-level escalation work
([ADR-0006](./0006-flag-level-escalation-semantics-in-the-manifest.md)) is the
natural place to express finer-grained write semantics if one ever does —
adding a category now would be speculative schema.

`publish` simply stops being projected as an MCP tool, which is the correct
default posture for a verb that writes to disk (ADR-0005's stance). It remains
fully available from the CLI, where it is idempotent and offline by design
([ADR-0003](./0003-statelessness-and-idempotency-as-architectural-invariants.md)).

This decision is accepted ahead of its implementation milestone; it lands with
the manifest escalation work (M2 in the iteration-1 direction).

### Positive Consequences

- The manifest stops lying; every consumer filtering on `writes` gets the safe
  answer with no out-of-band knowledge.
- One less hardcoded special case in the MCP layer (`is_read_tool()` becomes
  fully mechanical over declared semantics).
- Sets the precedent: when a classification is wrong, fix the table, not the
  consumer.

### Negative Consequences

- Any existing MCP client that used the projected `publish` tool loses it
  (correct, but a behavior change at the agent surface).
- `writes=true` is coarse: it cannot express that `publish` writes only the
  output tree and never the corpus — a real nuance deliberately left
  unexpressed until a consumer needs it.
- A `manifest -o json` field changes value, so downstream snapshots/fixtures
  pinned on the old classification need a refresh.

## Implementation

To land (M2, alongside ADR-0006): flip the `publish` entry to `writes=true` in
`src/manifest.rs::classified()`; delete the `c.name != "publish"` condition in
`src/mcp/tools.rs`; adjust the manifest/MCP regression tests that pinned the
old projection; doc-sync `automation.md`.
