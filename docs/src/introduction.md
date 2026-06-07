# Introduction

**adroit** is a snappy terminal tool for managing Architecture Decision Records (ADRs).

The name hides **ADR** in plain sight — because good architecture decisions should be _adroit_: clever, skillful, and well-considered.

## What are ADRs?

Architecture Decision Records are short documents that capture important architectural decisions along with their context and consequences. They provide a decision log that helps current and future team members understand _why_ the system looks the way it does.

## What does adroit do?

adroit gives you a terminal-native interface for the full ADR lifecycle:

- **Create** new ADRs from templates with guided prompts
- **Draft with AI** (opt-in) — an interview-driven first draft, `lint` for
  authoring gaps, and corpus Q&A — see [The ADR Workflow](./usage/workflow.md)
- **Browse** and search your decision log in a rich, interactive TUI
- **Update** status as decisions are superseded, deprecated, or accepted
- **Link** related decisions together to build a navigable decision graph
- **Track** review deadlines and keep your `SUMMARY.md` index in sync

## Surfaces

adroit is one core library behind three surfaces: the **CLI** for fast capture
and scripting, an interactive **TUI** (`tui` feature) for browsing, triage, and
in-terminal editing — with a fuzzy command palette, a modal (vi) markdown editor,
and in-TUI [AI assists](./usage/tui.md#ai-assists-in-the-tui) — and a read-only
**web dashboard** (`adroit serve`, `web` feature) that browses/searches the repo,
shows stats, a relationship graph, and repo-health checks, and auto live-reloads
when ADR files change on disk. See [Interactive TUI](./usage/tui.md) and
[Web Dashboard](./usage/web.md).
