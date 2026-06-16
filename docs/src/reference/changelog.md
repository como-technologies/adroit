# Changelog

Per-release changes, recorded here in the book (the one documentation
system — there is deliberately no standalone `CHANGELOG.md`). The release
discipline — local annotated tags on merged main, a `Cargo.toml` bump in the
release commit so `adroit --version` matches the tag, tags pushed only by the
owner — is ADR-0012 in the repo's own
[decision corpus](../dev/decisions.md) (`adr/accepted/`).

## v0.2.0 — 2026-06-12

The iteration-2 fix train: every machine-output friction the first full
portfolio loop (run-1) surfaced, fixed at root cause, plus the release and
scope decisions that make this the first *tagged* release.

### AI output sanitization

- `sanitize_draft` now strips **skeleton echoes** — duplicate `## Status` /
  `## Stakeholders` sections, `> State:` banners, and echoed adroit markers
  that small local models re-emit below the ai-suggested marker — and
  **trailing conversational residue** ("Please review this revised ADR
  body…"), including the horizontal rule such a closer orphans. Stored-plan
  spans always pass through verbatim. Covers `new --interview`, `draft`,
  `compose`, `import --ai`, and the TUI assists.

### Lint

- The Negative Consequences check accepts the section at `##` or `###`
  depth (depth is shape, not substance).
- New **warning** on repeated top-level (`##`) sections — the skeleton-echo
  shape — that previously passed silently.
- **Contract change (additive):** `LintFinding` gains a `severity` field
  (`"error"` / `"warning"`); `lint -o json` serializes it. Exit semantics
  refined: only mechanical *errors* exit non-zero; warnings and the `--ai`
  advisory finding don't gate.

### Created-date provenance (ADR-0011)

- The markdown profile persists `Created: YYYY-MM-DD` in the `## Status`
  region, stamped once by `new` (CLI + TUI) and never re-stamped by
  rewrites. Resolution precedence: git → authored document date → mtime →
  now. On a non-git corpus, `created` is now byte-stable across
  `set-status` and `plan --save`.

### TUI

- The plan palette verb reads a **stored plan provider-free** (ADR-0008
  semantics, matching CLI `plan <ID>`); fresh generation over a stored plan
  is a distinct, explicit verb ("AI: regenerate implementation plan").

### Decisions

- ADR-0011 created-date provenance; ADR-0012 tagged-release + changelog
  discipline; ADR-0013–0018 retire (with recorded reopen criteria) published
  distribution, the database-backed read model, MCP write verbs, additional
  AI providers, TUI widget expansion, and the candidate forge/tracker/web
  items. Embeddings remain retired under ADR-0009's measured-miss criterion.

### Release checklist (ADR-0012)

1. `just ci` green on merged main.
2. `Cargo.toml` version bumped in the release commit; changelog entry here.
3. `git tag -a vX.Y.Z -m "…"` on that commit (local; owner pushes, if ever).
4. `adroit --version` reports the tag's version.
5. Consumers advance their rev pin to the tag sha.

## v0.1.0 — baseline (never tagged)

Everything through iteration 1, before the release discipline existed:
the core CLI and store (markdown/frontmatter profiles, by-status / flat /
by-category layouts, four naming schemes), lifecycle verbs with link
healing, `check`/`lint`/`stats`/`graph`, templates and `publish` (six
targets), forge integration (GitHub + GitLab, five trackers), AI authoring
(anthropic + ollama; interview/draft/compose/ask/summarize), persisted
implementation plans (ADR-0008), the assessment ingest seam (`import`,
ADR-0010), the machine surface (`-o json`, `manifest`, read-only `mcp` —
ADR-0005), the TUI, and the read-only web dashboard. Consumers pinned git
shas; `--version` reported 0.1.0 throughout.
