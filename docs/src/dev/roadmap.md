# Roadmap

Directions and dispositions — not dated commitments. Most candidate items are
shaped to fit an existing
[seam](./architecture.md#seams-extend-by-adding-a-variant-not-editing-call-sites),
so they're "add a variant," not a rewrite. As of v0.2.0, every deferred lane
on this page is **retired by an accepted ADR with a recorded reopen
criterion** (the repo's `adr/accepted/` corpus — see
[Decision Records](./decisions.md)): retired is "not until the criterion
fires", never silent prose deferral.

## AI providers & retrieval

adroit's AI layer is built on the [rig](https://github.com/0xPlaygrounds/rig)
framework, behind the `AiProviderKind` seam (`anthropic` + `ollama` today).

- **More providers — retired (ADR-0016).** rig ships ~two dozen adapters
  (OpenAI, Gemini, Mistral, OpenRouter, …) and each is a one-line
  `AiProviderKind` variant + one rig-client arm in `rig_provider.rs` — which
  is exactly why the lane is retired rather than open-ended: anthropic +
  ollama cover the hosted and local lanes the suite actually runs. Reopen
  per-provider on a consumer with a concrete provider requirement (one
  `/extend` pass each).
- **Embeddings-based semantic retrieval — retired (ADR-0009).** `ask` /
  `related` / `dedupe` use mechanical TF-IDF, deterministically and offline.
  Deferred behind a **measured retrieval-miss** criterion that has never
  fired (no dogfood run has shown a miss, run-1 included). If it fires, rig's
  embeddings + `vector_store` drop in behind the same retrieval seam, paired
  with the files-derived cache design of the
  [read-model disposition](#deferred--retired-by-adr) (ADR-0014).
- **Granular AI-draft review in the TUI — retired (ADR-0017,** with the rest
  of the TUI widget lane**).** An AI draft loads into the editor to keep /
  trim as a whole; per-hunk accept/reject reopens only with a TUI-primary
  consumer who needs it.

## Interactive TUI

**Retired as a lane (ADR-0017).** The TUI uses a handful of ratatui widgets
(List, Paragraph, Scrollbar, …); several **out-of-the-box** ratatui widgets
map onto things adroit already computes — but every one of them is a
re-rendering of a view the CLI (`adroit stats`) or the web dashboard already
serves from the same `query`/`view` seam, and no TUI-primary consumer has
needed one. The one item that *was* load-bearing — the plan verb reading a
stored plan provider-free per ADR-0008 — was a consistency gap, and shipped
in v0.2.0. The retired candidates, each reopening individually on a
demonstrated TUI-primary need:

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

Providers grouped by seam — shipped, plus candidates (each a contained add,
**retired by ADR-0018**: the shipped set exceeds what any suite consumer
uses, and `/extend`-proven seam extensibility — not provider count — is the
property that matters; each candidate reopens on a consumer who actually
runs that system):

| Seam | Shipped | Candidates (retired, ADR-0018) |
|---|---|---|
| **Repo / PR host** (`Forge`) | GitHub, GitLab | Gitea / Forgejo, Bitbucket |
| **Issue tracker** (`Tracker`) | GitHub Issues, GitLab Issues, Jira, Linear + monday.com ([#12](https://github.com/como-technologies/adroit/issues/12); monday dogfood in progress, [#26](https://github.com/como-technologies/adroit/issues/26)), native | Azure DevOps Boards, Asana |
| **Publish target** (`Publisher`) | static, mdBook, MkDocs, Hugo, Docusaurus, Jekyll ([#8](https://github.com/como-technologies/adroit/issues/8)) | — (Confluence / Notion *hosting* is out of scope) |

Per-provider capability deepens behind the same traits — reviewer @-mentions + an MR
review-deadline label on `review --forge`, and the tracker's native due/target date
(Jira / GitLab / Linear / monday) on `set-review --forge`, shipped via the default-no-op
`Forge::add_label` / `Tracker::set_due_date` methods
([#11](https://github.com/como-technologies/adroit/issues/11)). The boundary that
keeps this in adroit's lane: its forge integration governs the **ADR lifecycle**
(propose-on-main, accept-via-MR, status sync, reviewer assignment), and `publish`
**produces** the accepted-set artifact (a static / `hugo-dir` / `docusaurus-dir`
tree) — it does not *host* it. The networked **Confluence / Notion push is the
consuming repo's CI** (e.g. the playbook repo's mdBook → Confluence pipeline), not
adroit; and code orchestration across forges is the Adopt engine's. Those are other
nodes' jobs (see the portfolio loop below).

## Web dashboard

Both candidates **retired (ADR-0018)** — the dashboard's read-only-ness is a
stated security property, and no SME session has stalled on either gap:

- **Per-repo branding** — a configurable company name / logo / accent color
  ([#16](https://github.com/como-technologies/adroit/issues/16)).
- **One-click "create MR"** from the dashboard — it stays read-only;
  authoring lives in the CLI / TUI
  ([#10](https://github.com/como-technologies/adroit/issues/10)).

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
  the **mutating** verbs (`new` / `set-status` / `supersede`) is **retired
  (ADR-0015)**: the categorical read-only projection is a portfolio security
  property pinned by conformance tests, and the one agent consumer mutates
  via the CLI deliberately. It reopens only for an MCP consumer that needs
  write verbs and brings its own confirmation UX — additive when it does,
  since the server just projects the manifest's `reads` / `writes` flags.

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
  each leaf carrying context / value / risk, as JSON / YAML / TOML) into a
  **proposed-ADR backlog** — one ADR per practice, mechanically — so the assessment
  becomes the decision backlog rather than dying in a doc. The optional `--ai`
  flesh-out pass (shipped) drafts richer prose for each seeded ADR from its
  assessment context, degrading to the mechanical seed when no provider is
  configured.
- **Emit (adroit → Adopt).** An accepted ADR plus its `plan` (the implementation
  checklist) is the decision context the **Adopt**-stage agentic engine (Conduit)
  turns into issues an agent works inside the team's own forge — read through the
  manifest / `-o json` / MCP, not by parsing files
  ([#22](https://github.com/como-technologies/adroit/issues/22) dogfoods that
  `plan -o json` ingest end-to-end through Conduit / tuesday). The exact read
  slice is **verified end to end on a live local model** — assessment →
  `import --ai` → accept → `plan --save` → the conduit-shaped `-o json` reads,
  with `plan` proven byte-deterministic — see
  [The Adopt read slice](./adopt-read-slice.md) (re-runnable via
  `just adopt-slice`). The PRs that engine
  ships are what **tuesday** then measures (effort / capacity), and which decisions
  actually landed re-opens the next assessment. adroit supplies *what was decided*;
  it does not run the build loop.
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

## Deferred / retired by ADR

- **Database-backed read model — retired (ADR-0014).** A database-backed
  store (SurrealDB / SQLite) was floated while dogfooding for richer
  relational queries. Plain markdown files stay the **only** source of truth
  (the ADR-0003 invariant): ADRs are git-reviewable and PR-diffable, there's
  no separate state to back up / migrate, and performance is
  millisecond-scale on every real corpus. Reopen criterion (now binding in
  the ADR): a concrete query/graph need at a corpus scale where re-scanning
  is measurably too slow — at which point an embedded index built *from* the
  files lands behind the existing `query` / `view` seam, doubling as the
  embeddings vector cache (ADR-0009) if that criterion ever fires too.
- **Published distribution (crates.io / prebuilt binaries) — retired
  (ADR-0013).** Build-from-source plus the local tagged release (ADR-0012)
  serves every consumer at the current rung; publishing is an owner-only
  act. Reopens with a declared self-serve direction.
- **Release discipline — adopted (ADR-0012).** Local annotated tags +
  version bumps + the book's [Changelog](../reference/changelog.md)
  chapter; tags stay local until the owner publishes.

> Want one of these? adroit's seams make most of them a contained change — the
> `/extend` skill ([Project Skills](./skills.md)) is the per-seam checklist.
