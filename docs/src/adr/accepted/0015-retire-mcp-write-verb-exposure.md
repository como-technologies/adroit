# ADR-0015: Retire MCP write-verb exposure

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

ADR-0005 deliberately projects only the manifest's **read** verbs as MCP
tools, and the roadmap kept a follow-up open: an opt-in
`adroit mcp --allow-write` exposing the mutating verbs (`new`, `set-status`,
`supersede`) with `destructiveHint` annotations. The read-only property is
meanwhile load-bearing well beyond adroit: the portfolio's security posture
cites it, and two conformance tests
(`projected_tools_carry_no_escalating_flags`,
`mcp_projected_tools_expose_no_escalating_flags`) pin that no projected tool
can mutate the repo, the forge, or the filesystem.

The only agent consumer adroit has — the Adopt-stage engine — **mutates via
the CLI** (`set-status`, `plan --save`) where it gets exit codes, dry-runs,
and its own gating, and reads via `-o json`/MCP. Should the write-verb
follow-up stay open?

## Decision Drivers

- The read-only projection is a *portfolio security property*, not a missing
  feature: every consumer that wires `adroit mcp` into an agent today can
  reason "this server cannot write" without auditing flag sets.
- No consumer demand: the one agent integration chose the CLI for writes,
  deliberately (process boundaries, exit codes, `--dry-run`).
- The exposure is **additive whenever wanted**: the server projects the
  manifest's `reads`/`writes`/`escalates` classifications mechanically, so
  an `--allow-write` later is a filter change plus its own conformance
  tests, not a redesign.
- The mandate's built-or-retired bar for open follow-ups.

## Considered Options

1. **Retire the follow-up**: the MCP surface stays read-only by decision;
   reopen on demonstrated consumer need.
2. **Build `mcp --allow-write` now** behind an explicit opt-in flag.
3. **Defer silently** (leave the roadmap line as is).

## Decision Outcome

Chosen: **Option 1 — retire write-verb exposure**, because the strongest
property the MCP server has is the one option 2 would dilute: *categorical*
read-only-ness, enforced mechanically (`is_read_tool()` + escalation
stripping) and pinned by conformance tests. An opt-in flag turns "cannot
write" into "check the flags", for a capability no consumer wants — the
Adopt engine's CLI writes are a better interface for mutation (explicit
argv, exit codes, dry-run previews, process-level audit). Option 3 is the
prose hedge the mandate disallows.

**Reopen criterion:** an MCP **consumer** (not a hypothetical) that needs
write verbs over the protocol and brings its own confirmation UX — at which
point the exposure is the documented additive change: an explicit opt-in
flag, `destructiveHint` annotations, and conformance tests proving the
default remains read-only.

### Positive Consequences

- "adroit's MCP server cannot mutate anything" stays a one-sentence audit,
  cited by the portfolio's security review.
- The conformance tests keep their categorical form (no projected tool
  carries an escalating flag — no allowlist exceptions).
- Agent mutation keeps flowing through the CLI, where dry-run and exit-code
  semantics already exist and are classified in the manifest.

### Negative Consequences

- A future MCP-only client (no shell access) cannot author ADRs at all
  until this reopens — it must drive a human or a CLI-capable intermediary.
- The `destructiveHint` design sketched in the roadmap goes unexercised;
  if reopened, that design starts from a sketch, not running code.

## Implementation

Nothing to build — the read-only projection already exists (ADR-0005) with
its conformance tests. The roadmap's `--allow-write` line now cites this
ADR as its disposition.
