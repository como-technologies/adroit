# Roadmap

Planned directions and known gaps — not dated commitments, just where adroit is
likely to grow next and what's deliberately deferred. Most are shaped to fit an
existing [seam](./architecture.md#seams-extend-by-adding-a-variant-not-editing-call-sites),
so they're "add a variant," not a rewrite.

## Authoring & AI

- **`compose` as a CLI verb.** Instruction-driven body (re)drafting already exists
  as a TUI assist (*AI: draft / revise body*); exposing it as
  `adroit compose <ID> "<instruction>"` would give the CLI symmetry.
- **Granular AI-draft review in the TUI.** Today an AI draft loads into the editor
  to keep / trim as a whole; a per-hunk diff with accept / reject is the richer next
  step.
- **Embeddings-based retrieval.** `ask` / `related` / `dedupe` use mechanical TF-IDF
  today. Semantic retrieval needs an embedding-capable provider plus a cache
  (Anthropic exposes no embeddings API), kept behind the same retrieval seam.

## Forge & trackers

- **More providers.** `github` / `gitlab` (plus the `jira` tracker) ship today;
  `gitea` / `bitbucket` are natural additions — one module + one `forge::open` arm
  each.
- **Live happy-path wiring.** Issue + PR creation against a mock HTTP server with a
  real git remote — the orchestration cores are unit-tested with mock adapters and
  the adapters are fault-injected, so this closes the live-glue test gap.

## Publishing & integrations

- **Confluence / Notion `publish` adapters.** `adroit publish` exports the accepted
  set offline today; hosted targets are the next publish backends.

## Web dashboard

- **OAuth device-flow + OS-keychain** credential storage (tokens are env / a local
  credential file today).
- **One-click "create MR"** from the dashboard — it stays read-only for now;
  authoring lives in the CLI / TUI.

## Editor

- **Undo / redo, selection, clipboard** in the in-TUI editor. The `EditorBuffer` is
  a deliberately minimal, pure plain-text editor (with vi modal keys); richer editing
  is a possible future swap.

## Agent surface

- **Structured command manifest.** Agents introspect adroit via `--help` +
  completions today; a machine-readable manifest would make discovery first-class.
  See [Automation & AI](../usage/automation.md).

> Want one of these? adroit's seams make most of them a contained change — the
> `/extend` skill ([Project Skills](./skills.md)) is the per-seam checklist.
