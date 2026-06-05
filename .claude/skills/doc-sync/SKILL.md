---
name: doc-sync
description: Use after changing adroit behavior (or for a periodic sweep) to keep code and docs in sync — update CLAUDE.md, README.md, and the mdbook (docs/src/), verify by running the CLI, build the book. Invoke for "sync the docs", "update docs", "docs sweep", or as the doc step of `gate`.
user-invocable: true
---

# Sync adroit docs with the code

**One doc system: the mdbook.** All docs live under `docs/src/**`, wired into
`docs/src/SUMMARY.md`, built with `just book`. NEVER create standalone `docs/*.md`
or parallel doc trees (no ad-hoc reports/guides). Contributor / dev docs go under
the book's **Development** section.

## Sweep — for each behavior you changed, update its surface
- **CLAUDE.md** — the architecture / seam doc (the canonical "how it works"). Keep
  the seam descriptions and method lists current.
- **README.md** — the top-level pitch + quickstart + links; links point at
  `docs/src/*` pages, never removed files.
- **mdbook `docs/src/`** — Installation, Quick Start, Using/Managing ADRs, CI, TUI,
  Web Dashboard, CLI Reference, ADR Format, and the Development pages
  (Testing & Fuzzing, Hardening & Quality).
- **`docs/src/SUMMARY.md`** — the nav, when adding a page.

## Verify by RUNNING, not reading
Document what the binary ACTUALLY does: run `adroit <cmd> --help` and the command
itself before writing. clap shows a flag even when it misbehaves; cross-check
code-vs-doc defaults (a port, a default value, a precedence order).

## Constraints
- `just book` must build with no broken links.
- Keep examples **generic** — never put a specific client's tech/titles in adroit
  docs, comments, or examples.
- No CHANGELOG yet — don't create one until asked.
