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

## Forge, trackers & publishing

adroit reaches the outside world through narrow **Rust trait** seams, each
dispatched from config in `forge::open`, so a new provider is *one module + one
match arm* — no call-site changes:

- **`Forge`** — the PR/MR side (`open_pr` / `pr_state` / `merge_pr` / `comment_pr` …).
- **`Tracker`** — the issue side (`create_issue` / `transition` / `issue_state` …).
- **`Publisher`** — render the accepted set into a target's shape behind the
  `publish --to` flag: `static` (default), `mdbook`, `mkdocs`, `hugo`,
  `docusaurus`, `jekyll`. Pure + offline; adroit produces the tree, the consuming
  repo's CI hosts it ([#8](https://github.com/como-technologies/adroit/issues/8)).

Every adapter takes an injectable `HttpTransport`, so each is unit-tested against a
fault-injected mock and the lifecycle cores run on mock adapters; the remaining
live-glue gap is issue + PR creation against a mock HTTP server with a real git
remote, proven per provider pairing
([#13](https://github.com/como-technologies/adroit/issues/13)–[#15](https://github.com/como-technologies/adroit/issues/15)).

Providers grouped by seam — shipped, plus candidates (each a contained add):

| Seam | Shipped | Candidates |
|---|---|---|
| **Repo / PR host** (`Forge`) | GitHub, GitLab | Gitea / Forgejo, Bitbucket |
| **Issue tracker** (`Tracker`) | GitHub Issues, GitLab Issues, Jira, native (files-only) | Linear ([#12](https://github.com/como-technologies/adroit/issues/12)) |
| **Publish target** (`Publisher`) | static, mdBook, MkDocs, Hugo, Docusaurus, Jekyll ([#8](https://github.com/como-technologies/adroit/issues/8)) | — (Confluence / Notion *hosting* is out of scope) |

Per-provider capability deepens behind the same traits — reviewer @-mentions, review
deadlines, Jira due / Linear target dates
([#11](https://github.com/como-technologies/adroit/issues/11)). The boundary that
keeps this in adroit's lane: its forge integration governs the **ADR lifecycle**
(propose-on-main, accept-via-MR, status sync, reviewer assignment), and `publish`
**produces** the accepted-set artifact (a static / `hugo-dir` / `docusaurus-dir`
tree) — it does not *host* it. The networked **Confluence / Notion push is the
consuming repo's CI** (e.g. the playbook repo's mdBook → Confluence pipeline), not
adroit; and code orchestration across forges is the Adopt engine's. Those are other
nodes' jobs (see the portfolio loop below).

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
- **MCP server — shipped**
  ([#19](https://github.com/como-technologies/adroit/issues/19), follow-up to #17).
  `adroit mcp` is a built-in [Model Context Protocol](https://modelcontextprotocol.io)
  server (JSON-RPC 2.0 over stdio) that projects the manifest's read verbs as MCP
  tools — each verb a tool with its args as a JSON Schema — so the portfolio's
  **Adopt**-stage agent engine (and any agent) drives adroit to read decisions and
  plans without scraping `--help`. Behind the default-on `mcp` feature; a
  hand-rolled JSON-RPC/stdio loop, no async runtime. **Read-only** — the read verbs
  (`list` / `show` / `search` /
  `stats` / `graph` / `check` / `plan` / `related` / `dedupe` / …) as tools, so an
  agent reads decisions + plans but can't mutate the repo over the wire. Exposing
  the **mutating** verbs (`new` / `set-status` / `supersede`) is a later opt-in —
  gated behind an explicit `adroit mcp --allow-write` (annotated `destructiveHint`),
  or deferred to the MCP client's own confirmation — and additive, since the server
  just projects the manifest's `reads` / `writes` flags.

## Portfolio integration — the Como loop

> Tracked as an epic: [#18](https://github.com/como-technologies/adroit/issues/18).

adroit is the **Prescribe** node of the
[TAPS portfolio](https://github.com/como-technologies/portfolio)'s closed loop —
**Assess → Prescribe → Adopt → Measure → re-assess** — where every stage emits an
artifact the next consumes. adroit's job is deliberately narrow: **author and govern
decisions** (ADRs) and their implementation **plans**, and make them
machine-consumable. It is *not* the agent that writes the code, the layer that
orchestrates forges, or the system that hosts the playbook — those are other nodes
(below). Holding that line is what keeps the seams clean.

The seam is the **manifest** — with the `-o json` `view` contract and the MCP
catalog ([#19](https://github.com/como-technologies/adroit/issues/19)). Structured
JSON is how adroit's decisions cross into the rest of the loop, so a downstream agent
*reads* a decision instead of scraping prose. The ADRs and guides stay **markdown**
for humans; the *integration* contract is JSON.

- **Ingest (Assess → adroit) — shipped.** `adroit import --from-assessment <file>`
  turns an `assessments` export (the Domain → Practice → Question maturity model,
  each leaf carrying context / value / risk, as JSON / YAML) into a **proposed-ADR
  backlog** — one ADR per practice, mechanically — so the assessment becomes the
  decision backlog rather than dying in a doc. Drafting richer prose from a seed
  (feeding it to `draft` / `compose` as context) is the natural `--ai` follow-up.
- **Emit (adroit → Adopt).** An accepted ADR plus its `plan` (the implementation
  checklist) is the decision context the **Adopt**-stage agentic engine (Conduit)
  turns into issues an agent works inside the team's own forge — read through the
  manifest / `-o json` / MCP, not by parsing files. The PRs that engine ships are
  what **tuesday** then measures (effort / capacity), and which decisions actually
  landed re-opens the next assessment. adroit supplies *what was decided*; it does
  not run the build loop.
- **Stay in lane (the boundary that prevents overlap).** The forge-neutral,
  model-neutral *code* orchestration — the event router and PR/MR lifecycle that
  drive an agent identically across GitHub / GitLab / self-hosted forges — is the
  Adopt engine's net-new IP, not adroit's. Hosted distribution (the playbook repo's
  mdBook → Confluence CI, a future `publish` target) belongs to the publish seam, not
  to adroit's core. adroit's own forge integration stays scoped to ADR-lifecycle
  governance (the seam table above). Same loop, disjoint responsibilities.

These are **directions**, not committed APIs — but they're why the seams
(`query` / `view`, `-o json`, the manifest, the publish + AI seams) are shaped the
way they are: each is a typed boundary another node can consume.

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
