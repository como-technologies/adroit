# ADR-0017: Retire TUI widget expansion

> State: Accepted

## Status

Accepted
Created: 2026-06-12

## Stakeholders

- Maintainer — Brett Fowle

## Context and Problem Statement

The roadmap carries a TUI widget lane: Tabs (List · Stats · Graph),
BarChart/Sparkline insights, a Canvas relationship graph, a Table list,
Calendar/Gauge review views, per-hunk AI-draft review, and editor
undo/selection/clipboard. All of it is real ratatui capability mapped onto
data adroit already computes — and none of it has a consumer. Every view in
the list is already served: `adroit stats` renders bar charts on the CLI,
`-o json` feeds machines, and the web dashboard renders stats, the graph,
and review tiles for humans who want visuals.

The one TUI gap that *was* load-bearing — the plan verb spawning a provider
call where ADR-0008 makes the stored plan a provider-free read — was a
correctness/consistency gap, and was fixed (fix-train M5), not deferred.
Should the rest of the widget lane stay open?

## Decision Drivers

- No SME-usable criterion touches TUI visuals: the Prescribe workflow an SME
  drives is authoring verbs + lifecycle, all present.
- Every listed view exists on another surface computed from the same
  `query`/`view` seam — the TUI variants are *re-renderings*, not new
  capability.
- The deliberate editor minimalism (no undo/selection/clipboard, vi-modal,
  `$EDITOR` escape hatch) is a recorded design choice in CLAUDE.md, not an
  accident awaiting fixing.
- The mandate's built-or-retired bar.

## Considered Options

1. **Retire the widget lane**: the TUI stays list + preview + palette +
   modal editor; reopen on demonstrated TUI-primary need.
2. **Build the parity views now** (Tabs/stats/graph in the terminal).
3. **Leave the lane open** as roadmap prose.

## Decision Outcome

Chosen: **Option 1 — retire TUI widget expansion**, because the lane's value
proposition is parity with surfaces that already exist, for a user persona
(terminal-primary, wants charts, won't open the dashboard) that no suite
consumer or dogfood session has produced. The M5 stored-plan fix is the
counterexample that proves the rule: that one was an ADR-0008 *consistency*
gap — semantics, not polish — and it was built, not retired. Polish without
a consumer (option 2) is exactly the speculative lane the design rules
reject; option 3 is the hedge the mandate disallows.

**Reopen criterion:** a TUI-primary user with a recurring need a listed
widget serves — e.g. an SME who works decisions exclusively in the terminal
and demonstrably needs the review Calendar or the stats view there. Each
reopen is one widget against the existing `query`/`view` data, individually
scoped.

### Positive Consequences

- The TUI stays small enough to keep its strongest property: the pure,
  headlessly-tested `TuiState` layer (the M5 fix landed as three headless
  tests precisely because the surface is small).
- One ratatui in the tree, no text-area/canvas dependency growth.
- Stats/graph rendering logic stays in exactly two places (CLI human
  renderer, web SPA) instead of three.

### Negative Consequences

- Terminal-only environments get tables and charts only via the CLI's
  static output, not interactively.
- The editor keeps lacking undo/selection/clipboard — a real annoyance for
  long in-TUI edits; the `$EDITOR` escape hatch (`e`) is the documented
  answer, and it is a workaround.

## Implementation

Nothing to build — this ADR records the disposition; the roadmap's TUI
section now cites it. M5's stored-plan surfacing stays, shipped, as the
consistency fix it was.
