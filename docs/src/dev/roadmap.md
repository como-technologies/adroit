# Roadmap

Planned directions and known gaps — not dated commitments, just where adroit is
likely to grow next and what's deliberately deferred. Most are shaped to fit an
existing [seam](./architecture.md#seams-extend-by-adding-a-variant-not-editing-call-sites),
so they're "add a variant," not a rewrite.

## AI providers & retrieval

adroit's AI layer is built on the [rig](https://github.com/0xPlaygrounds/rig)
framework, behind the `AiProviderKind` seam (`anthropic` + `ollama` today).

- **More providers.** rig already ships ~two dozen adapters (OpenAI, Google
  Gemini, Mistral, Cohere, Groq, DeepSeek, xAI, Perplexity, Azure OpenAI,
  OpenRouter, Together, llamafile, …). Each is a one-line `AiProviderKind` variant
  + one rig-client arm in `rig_provider.rs` — no new seam. High-value first picks:
  **OpenAI** and **Gemini** (the other two majors), and **OpenRouter** (a single
  adapter that fans out to dozens of hosted models behind one key).
- **Embeddings-based semantic retrieval.** `ask` / `related` / `dedupe` use
  mechanical TF-IDF today. rig also provides an **embeddings** API + a
  **`vector_store`** abstraction (in-memory + LSH out of the box, plus
  pluggable backends), and embedding-capable providers (OpenAI, Cohere, Gemini,
  Voyage AI). Semantic retrieval drops in behind the same retrieval seam — pairs
  naturally with the [read-model spike](#deferred--under-consideration) (the vector
  index can live in the same files-derived cache).
- **Granular AI-draft review in the TUI.** Today an AI draft loads into the editor
  to keep / trim as a whole; a per-hunk diff with accept / reject is the richer
  next step.

## Interactive TUI

The TUI uses a handful of ratatui widgets (List, Paragraph, Scrollbar, …); several
**out-of-the-box** ratatui widgets map onto things adroit already computes but only
surfaces on the CLI or the web dashboard:

- **Tabbed multi-view** (`Tabs`) — List · Stats · Graph, instead of a single
  list+preview, so the TUI reaches parity with the web dashboard's pages.
- **In-terminal Insights** (`BarChart` / `Sparkline` / `Chart`) — render
  `query::stats` (status breakdown, created-over-time, review backlog) in the
  terminal, the way `adroit stats` does on the CLI.
- **Relationship graph in the terminal** (`Canvas`) — draw the supersession + typed-
  link graph (`query::graph`) as nodes/edges on a Canvas; the web shows it as SVG,
  the terminal could show it live.
- **Richer list** (`Table`) — columns for reference / status / created / review-due
  / link-count, replacing the hand-rolled `List` rows.
- **Review at a glance** (`Calendar` for `review_by` deadlines; `Gauge` /
  `LineGauge` for review-quorum progress).
- **Editor polish** — undo / redo, selection, and clipboard. The `EditorBuffer` is
  a deliberately minimal pure plain-text editor (with vi modal keys); richer editing
  is a possible future swap (a ratatui-0.30 text-area widget, kept to one ratatui in
  the tree).

## Forge & trackers

- **More providers.** `github` / `gitlab` (plus the `jira` tracker) ship today;
  `gitea` / `bitbucket` are natural additions — one module + one `forge::open` arm
  each.
- **Live happy-path wiring.** OAuth device-flow login is now live-tested end-to-end
  (real transport against a mock server); the remaining gap is **issue + PR
  creation** against a mock HTTP server with a real git remote — the orchestration
  cores are unit-tested with mock adapters and the adapters are fault-injected, so
  this is the last live-glue piece.

## Publishing & integrations

- **Confluence / Notion `publish` adapters.** `adroit publish` exports the accepted
  set to a static dir offline today; hosted targets are the next publish backends —
  the **conduit** that lands a playbook where teams already read (see the portfolio
  loop below).

## Web dashboard

- **One-click "create MR"** from the dashboard — it stays read-only for now;
  authoring lives in the CLI / TUI.

## Agent surface

- **Structured command manifest — shipped**
  ([#17](https://github.com/como-technologies/adroit/issues/17)). `adroit manifest`
  emits a machine-readable JSON catalog of the CLI surface (commands + args +
  semantics, derived from the clap tree, plus `schemars` schemas of the `view`
  types) so agents discover and drive adroit without scraping `--help`. See
  [Automation & AI](../usage/automation.md#discovering-commands--adroit-manifest).
- **MCP tool catalog** (follow-up to #17). Wrap the manifest as
  [Model Context Protocol](https://modelcontextprotocol.io) tools — each command a
  tool with its args as a JSON Schema — so adroit is a first-class node for the
  portfolio's agent orchestration (below), either as an external wrapper or a built-in
  `adroit mcp` server.

## Portfolio integration — the Como loop

> Tracked as an epic: [#18](https://github.com/como-technologies/adroit/issues/18).

adroit isn't a standalone tool; it's the **Prescribe** stage of the
[TAPS portfolio](https://github.com/como-technologies/portfolio)'s closed loop —
**Assess → Prescribe → Adopt → Measure → re-assess** — where every stage emits an
artifact the next one consumes. ADRs are the atomic unit of the playbook adroit
authors. The strategic direction is to make adroit a **first-class node in that
artifact pipeline** rather than an island, via typed seams in and out:

- **Ingest (Assess → adroit).** A structured assessment (the `assessments` app's
  maturity model — Domain → Practice → Question leaves carrying context / value /
  risk) seeds **proposed ADRs**: each decision a practice implies becomes a draft
  ADR (and a reusable template), so the assessment doesn't die in a doc — it
  *becomes* the decision backlog. A natural fit for `new --interview` /
  `compose` with the assessment export as context.
- **Emit (adroit → Adopt / Measure).** An accepted ADR plus its `plan`
  (implementation checklist) feeds **tuesday** (effort / capacity), and `publish`
  feeds the hosted **playbook** (the publish conduit above). Which decisions
  actually landed becomes a **measure** signal.
- **One artifact contract.** Every tool in the loop produces / consumes **markdown**;
  keeping adroit's import/export shapes aligned with the assessment + playbook
  formats is what keeps the thread intact "with the seams closed."
- **Self-referential.** adroit dogfoods its own decisions (the `/adr` skill →
  `adroit --dir adr`), and the loop is self-feeding — assessments generate ADR
  templates, ADRs reference playbook guides, measurement re-opens the next
  assessment.

These are **directions**, not committed APIs — but they're the reason the seams
(`query`/`view` contract, `-o json`, the publish + AI seams) are shaped the way they
are.

## Deferred / under consideration

- **Database-backed read model (spike).** While dogfooding, a database-backed store
  (SurrealDB / SQLite) was floated for richer relational queries. **Decision for
  now: keep plain markdown files as the source of truth** — ADRs are git-reviewable
  and PR-diffable (the record *is* the reviewed artifact), there's no separate state
  to back up / migrate, and performance is already millisecond-scale on
  tens-to-hundreds of files. A *future, time-boxed* spike would evaluate an
  **embedded index built _from_ the files** (files stay authoritative) behind the
  existing `query` / `view` seam — for transitive dependency analysis or multi-hop
  graph queries the file scan + `query::graph` can't serve well, and doubling as the
  **embeddings vector cache** above. Entry criterion: a concrete query/graph need at
  a scale where re-scanning is too slow. Until then, the file model + the wiki-graph
  cover it.

> Want one of these? adroit's seams make most of them a contained change — the
> `/extend` skill ([Project Skills](./skills.md)) is the per-seam checklist.
