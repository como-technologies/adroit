# adroit

A snappy Rust CLI for managing Architecture Decision Records. An interactive TUI
(browse, triage, in-terminal body editing) ships behind the `tui` feature; a
read-only web dashboard (`adroit serve`) with auto live-reload behind `web`.

## Working agreements (IMPORTANT — read first)

- **Never `git push` / force-push / create / merge / comment on PRs without the
  user's explicit, in-the-moment permission.** Committing locally is fine;
  *publishing* is not. A one-time "push"/"create a PR" authorizes *that* action
  only — ask again every time. Do not infer standing permission.
- **Never write a bare `#<number>` in GitHub-rendered text** (commits, PR
  titles/bodies, comments) — GitHub auto-links it to an unrelated issue/PR. Use
  `bug N` / `finding N` / `rule N` / plain `N` (or table cell `| N |`). Applies to
  internal blitz/bug/check-rule numbers, which are NOT GitHub issues.
- **All documentation lives in the mdbook** (`docs/src/**`, wired into
  `docs/src/SUMMARY.md`, built with `just book`). Do NOT create standalone Markdown
  docs elsewhere. Contributor/dev docs go under the book's **Development** section.
- **Keep code and docs in sync, always.** Change behavior → update the mdbook page
  *and* this file in the same change, verify by running the CLI. Periodically sweep
  for drift; `just book` confirms the build.
- **Definition of done — run the matching skill, don't freelance (HARD RULE).** A
  feature/seam change is NOT complete until its skill checklist *and* the gate pass.
  Before calling work done / asking to push:
  - **Adding a seam variant** (forge/tracker provider, naming scheme, format,
    layout, publish adapter, template, config key, **CLI subcommand**) → run
    **`/extend`**; do every test + doc it lists (incl. the manifest `classified()`
    entry for a new subcommand).
  - **A new/changed parser of untrusted input, a mutating write path, or any
    invariant** → run **`/harden`**: an oracle `Op` in `tests/model.rs` for a new
    verb, a `tests/parsers.rs` no-panic + structural property AND a
    `tests/fuzz_parsers.rs` bolero target for a new parser — then **soak**
    (`PROPTEST_CASES=1500+`).
  - **Any behavior change** → **`/doc-sync`** the mdbook + this file, keeping the
    *enumerated* lists current: the oracle verb list and fuzz-target list in
    `docs/src/dev/testing.md`, the manifest `classified()` table, the help groups.
  - Finish with **`/gate`** (`just ci`) green.
  When unsure, run them all — a new verb that also reads a file format pulls in
  **all** of `/extend` + `/harden` + `/doc-sync`. Default to the checklist over
  judgment.

## Statelessness & idempotency (architectural invariant)

adroit is **stateless** and **idempotent where it makes sense**; both are invariants
every change must preserve.

- **The only state is the filesystem.** A command's input is the ADR docs on disk
  *plus* config resolved at startup (flag > process-env > `.env` > `config.yaml` >
  default). No daemon, database, cache, index, lock file, or cross-command state (the
  one process-global, `GIT_STRICT_WARNED` in `query.rs`, is a warn-once UX flag reset
  per invocation). `adroit serve` reopens the store **per request**; its only in-process
  state is the active-dir pointer + live-reload watcher.
- **Converge, don't accumulate.** A mutating command reads current state, computes
  the target, writes **only what differs** (minimal-diff; a file already in target
  state round-trips byte-identical). Re-running with the same args on the same state
  is a no-op.
- **Idempotent verbs** (re-run = byte-identical): `set-status`, `supersede`,
  `set-review`, `relink`, `migrate` (converges to a fixpoint then "nothing to
  migrate"), `index`, `link`/link-rewriting, `publish`, `check`. Forge **comments**
  converge too (`review`/`set-review --forge` upsert by a hidden marker — forge section).
- **Intentionally non-idempotent** *imperative events* (repeating repeats the event,
  by design): `new` (allocates the next ADR), `renumber old new` (one-shot;
  re-running fails because `old` is gone), `notify` (fresh message), `plan --save`
  (each save is a fresh AI generation; the *splice* converges — re-saving identical
  content is byte-identical — and the stored-plan *read* is a pure deterministic
  read), forge/git side
  effects. `new` adds a **duplicate guard** (`dup_guard`): on an exact (case-insensitive)
  title match it warns, lists similar ADRs (`similar::rank`), and prompts `[y/N]` on a TTY
  (non-TTY proceeds; `--force` skips).
- **New write paths must keep this true** — no hidden persisted state, no mutation
  that changes a file it didn't need to. Guard test `commands_are_idempotent`
  (`tests/cli.rs`) runs the idempotent verbs twice and asserts byte-identical.
- **`--dry-run` is a true full preview** — mutates nothing (local *or* forge),
  even without `--forge` (`new --dry-run` allocates no ADR/editor; mutating verbs
  gate the *local* write on `dry_run`). Guard: `dry_run_changes_nothing`.

## Build & Test

Always use `just` recipes — never raw `cargo`/`mdbook`.

**Dependencies: always pull the latest (HARD RULE).** Adding *or* bumping a dep → use
the **latest published version** (`cargo search <name>`). `Cargo.toml` groups deps by
the feature that pulls them in (core / manifest / tui / web / forge / ai / keychain);
keep a new dep in its group with a one-line "why". Do a major bump's code migration (feature
renames / API rewrites — e.g. keyring 4, ureq 3, schemars 1) **inline** and re-test. Don't
unilaterally split work into a separate issue/PR — the user decides.

```sh
just init        # install all project tools (clippy, rustfmt, cargo-watch, mdbook)
just ci          # full CI suite: fmt-check, lint, test, book build
just check       # type-check without building
just build       # debug build
just test        # all tests (unit + integration)
just unit        # unit tests only (--lib)
just lint        # clippy with -D warnings
just fmt         # auto-format
just fmt-check   # check formatting
just book        # build the mdbook user manual
just book-serve  # local book dev server with live reload
just run <args>  # run the binary
just adopt-slice # live-ollama dogfood rehearsal of the Adopt read slice (temp corpus;
                 # skips cleanly without a local ollama; not part of `just ci`)
```

## Project Layout

- `src/lib.rs` — library crate root; `src/main.rs` — thin binary, delegates to lib
- `src/view.rs` — plain serde view types (`AdrSummary`, `AdrDetail`, `Stats`,
  `Graph`): the shared contract for "what a surface can show"
- `src/query.rs` — shared read API over `Store`
  (`summaries`/`detail`/`search`/`stats`/`graph`), builds the view types
- `src/history.rs` — git-derived ADR dates + lifecycle (shells `git log`)
- `src/links.rs` — cross-ADR relative-link parsing/rewriting + `absolutize_links`
  (rel→blob URL for forge comments); pure
- `src/plan.rs` — persisted implementation plans (ADR-0008): the pure splice/extract
  engine behind `plan --save` and the stored-plan read; pure
- `tests/` — `cli.rs` (binary + regressions), `model.rs` (model-based oracle over the
  format×layout×scheme×relink_scope matrix), `parsers.rs` + `fuzz_parsers.rs`
  (properties / bolero fuzz), `config_precedence.rs`, `date_source_git.rs`,
  `forge_faults.rs` + `forge_cli.rs`. `tests/fixtures/golden-assessment.yaml` is the
  **vendored cross-repo ingest contract** — the assessments app's real-exporter
  golden (regenerate it in an assessments checkout — sibling `../assessments` or a clone
  per the suite resolution convention — via `just golden`, re-copy verbatim under the header);
  `import_golden_assessment_contract` pins the seeded backlog against it, and the
  env-gated `ADROIT_LIVE_OLLAMA=1` test exercises `import --ai` on live ollama.
  See `docs/src/dev/testing.md`.
- `docs/` — mdbook source (`book.toml` + `src/`), to GitHub Pages; output `docs/book/`
  (gitignored). `justfile` — all dev recipes.
- `adr/` — adroit's **own ADR corpus** (markdown / by-status), authored with the
  built binary via the `/adr` skill — **always `--dir adr`** (`ADROIT_DIR` points at
  the external dogfood repo, so a bare `adroit new` writes to the wrong corpus).
  Gated in CI: `just adr-check` (in `just ci`) runs the freshly built
  `adroit check --dir adr`. See the book's Development → Decision Records page.

## Read/query layer (shared by all surfaces)

Read/derive logic lives once in `src/query.rs`, returning the pure serde structs in
`src/view.rs`. Every surface consumes this seam — CLI read commands (`list`/`search`/`show`),
TUI, and the `adroit serve` JSON API call the same functions, so stats/search/graph are
computed identically. View types are filesystem- and UI-framework-free and derive `Serialize`.
Writes stay in the `Store` write path (CLI + TUI); the query layer never writes. Markdown→HTML
is deferred to the web surface (`AdrDetail::body_html` stays `None`).

**CLI emits the same JSON (`-o`/`--output json`).** A global `cli::OutputFormat`
(`human` default, `json`) is honored by read verbs `list`/`show`/`status`/`search`/
`stats`/`graph`/`check`/`lint`/`related`/`dedupe`/`ask`/`plan` **and the write verb
`import`** (its `view::ImportSummary` machine seed summary — `{source, assessment,
dry_run, seeded[], skipped[]}`, plus an optional `sanitized` `SanitizeReport` of
per-rule `--ai` draft-sanitizer drop counts (zero rules + non-`--ai`/clean runs
omit it; additive, `manifest_schema` stays 1); `--dry-run -o json` is the same
shape with `reference: null`, since identity is allocated only on write)
(`docs/src/usage/automation.md`; the `--output` long help enumerates them —
guarded by `output_long_help_names_every_json_read_verb` against the manifest's
`json_output` column). `check -o json` still exits
non-zero on an Error-severity problem (the CI gate). `status -o json` is the scalar
case — the typed `Status` as a JSON string (`"Accepted"`); its human form stays the
bare lowercase word.

**Agent discovery — `adroit manifest`** (`src/manifest.rs`, default-on `manifest` feature =
`dep:schemars`): a machine-readable JSON catalog of the CLI surface, three drift-proof layers:
**syntax** from the clap `Command` tree (`Cli::command()`); **output schemas** via
`schemars::schema_for!` of the `view` types; **semantics**
(`reads`/`writes`/`idempotent`/`stage`/`json_output`/`requires`/`exit`) an owned `classified()`
table — `manifest_classifies_every_command` fails CI if a compiled command lacks an entry.
Semantics are also **per-(verb, flag)**: the owned `escalation()` table marks args that
escalate a read verb — `escalates: "forge"` (`review --forge`/`--yes`/`--dry-run`,
`list`/`check --forge`), `"file-output"` (`review`/`plan`/`summarize --out`), or `"writes"`
(corpus mutation — `plan --save`/`--force`/`--dry-run`, plus `--regenerate`, which forces a
fresh provider call where the stored read is free; ADR-0008) — serialized on the arg in
`manifest -o json` (additive; `manifest_schema`
stays 1). `escalating_flags_on_read_verbs_are_classified` fails CI when a suspect flag
(`forge`/`yes`/`dry_run`/`out`/`save`/`force`/`regenerate`) on a read verb lacks an entry.
`publish` is classified
`writes=true` — producing an output tree IS a filesystem write (ADR-0007). `requires` captures
**runtime** gating (`["ai","ai.enabled"]`, `["forge config"]`); a verb's `cost`/`requires` are
its conservative worst case — `plan` declares `provider-call`+`ai`, but the stored-plan read
(ADR-0008) is local and provider-free. Handled
before the store opens; a core build drops the command + `schemars`.

**Agent surface — `adroit mcp`** (`src/mcp/`, default-on `mcp` feature = `manifest`): a
built-in **Model Context Protocol** server (JSON-RPC 2.0 over stdio) **projecting the
manifest's read verbs as MCP tools**. Hand-rolled sync stdio loop; `handle_line` is a pure
`&str -> Option<String>` (fuzzed via `fuzz_mcp_request`). **Read-only, flag set included:**
`Server::new` filters to `is_read_tool()` verbs (`reads && !writes && cost ∈ {local,
provider-call}` — fully mechanical, no special cases) **and strips args the manifest marks
`escalates`**, so no projected tool can mutate the repo, the forge, or the filesystem
(conformance pinned by `projected_tools_carry_no_escalating_flags` +
`mcp_projected_tools_expose_no_escalating_flags`). A
`tools/call` re-runs `adroit <verb> … -o json` as a subprocess with the resolved on-disk
shape as env (a new read verb auto-appears). Dispatched with the resolved `--dir`.
`publish`/`review` destination flags are **`--out`** (long-only) so `-o` = `--output`.
`stats` + `graph` are thin verbs over `query::stats`/`query::graph`. The five on-disk
*shape* globals (`--format/--layout/--naming/--date-source/--relink-scope`) are
**top-level-only** (env still binds); only `--dir` is `global`.

**Help model.** `-h`/`--help` show the **same concise** help (command list +
`--dir`/`--output`); `--help-all` shows everything. Built with `disable_help_flag = true` +
custom **global** `help`/`help_all` flags; repo-shape + command-default options carry
`hide_short_help = true`. Do not re-add a built-in help flag.

**Human output.** Colored via `colored` (disabled when stdout isn't a terminal, so pipes /
`-o json` / `NO_COLOR` get plain text). `graph`'s human view is a **tree** (`├─`/`└─`,
isolated ADRs as `unconnected:`); `stats` renders by-status + created-per-month as
horizontal bar charts (`█`/`░`). `-o json` is never colored/charted.

**Repo validation** lives here: `query::check` runs the `adroit check` rules (status/dir
mismatch, duplicate identifiers, unparseable files, broken supersession, broken/stale links,
**duplicate-title** `Warning`) → `view::CheckReport` (`Problem` + `Severity` + `ProblemKind`).
The supersession + link checks are **scheme-aware** (via `ref_in_link_from`, so
date/uuid/per_category links classify *stale* (moved → warning) vs *broken* (no ADR → error)),
validated in **both** profiles. `cmd_check` sorts messages so output is **byte-identical**
(`check_*` tests guard); web `GET /api/check` serves it. `Stats.proposed_age` rows carry a
`review_due` flag.

**Dates & lifecycle come from git (`src/history.rs`), creation dates also from the
document (ADR-0011).** A clone resets mtime, so `query` resolves `created`,
`last_modified`, and the status timeline from git: one `git log --follow --name-status`
per file → `AdrHistory { created, last_modified, events }` (pure `parse_log`,
unit-tested without git). In by-status each status change is a directory rename, so the
timeline (proposed → accepted/rejected/superseded) is rebuilt from renames; `status_of`
is injected as `Store::dir_status` (flat = no milestones). The markdown profile persists
a `Created: YYYY-MM-DD` line in the `## Status` region (`adr::CreatedOn` /
`Adr.created_on`, `format::rewrite_created` — format-preserving, `Review by:` algebra),
stamped **once** by `new` (CLI + TUI) and never re-stamped by rewrites — so on a non-git
corpus `created` is rewrite-stable provenance, not mtime (run-1 finding; regression
`created_is_byte_stable_across_set_status_and_plan_save_without_git`).
`query::load_resolved` resolves `created` per row (git → document date [frontmatter
`created:` / markdown `Created:`] → mtime → `now()`); `query::detail` adds
`AdrDetail.history` + `last_modified`.
Degrades gracefully (no git → fallback). Surfaced in `show`, the TUI header, the web
detail view.

Source via `config.date_source` / `ADROIT_DATE_SOURCE` / `--date-source` (`DateSource`
on `StoreOptions`): `auto` (default — git when available, silent fs fallback), `git`
(strict — warns once when not git/shallow, then falls back), `filesystem` (skip git —
mtime/authored dates, no timeline). `query::open_history` centralizes this.

"Today" (the `date` scheme's `YYYYMMDD-` slug + review-due math) comes from
`config::today_override()` then the system clock: a **test-only** `ADROIT_TODAY` (ISO
`YYYY-MM-DD`) pins it for tests/CI; unset, unchanged.

## Interactive TUI (`tui` feature)

`src/tui.rs` is a ratatui two-pane app (list + preview); `--no-default-features` builds
core lib + CLI with no ratatui/crossterm. Split into a pure terminal-free layer
(`TuiState`, `Mode`, `Action`, `EditorBuffer`, unit-tested headlessly) and a thin
`driver` submodule wiring crossterm + ratatui and running the headless `apply_action`
against a `Store`. Reads via `query`; writes via `Store`.

**Markdown preview & themes.** The preview renders the body as GFM via
`the-other-tui-markdown` (`tui`-gated, on `ratatui-core` 0.1, no duplicate ratatui); `m`
toggles `TuiState::preview_raw` (rendered ↔ raw). Themes are `config::MarkdownTheme { Gruvbox
(#[default]), Warm, Default }`, resolved from `--theme` / `ADROIT_THEME` / `tui_theme` (flag >
env > config). Fenced code is **syntax-highlighted** via syntect (process-wide `OnceLock`
caches, pure-Rust fancy-regex no onig/C; syntect theme tracks the TUI theme).

**Whole-UI chrome.** The theme drives the whole interface via `driver::chrome(theme)`
(accent/muted/border/selection_bg/title): rounded borders, a `▶ ` marker, themed titles. Three
rows: a top **breadcrumb** (`adroit › <filter> › "<search>" · N ADRs · sort · theme`), the
list+preview body, a two-line **footer** (active prompt or severity-colored status + key
hints). `?` toggles a keybinding **help overlay**; empty panes show a context-aware hint
(`empty_list_message`).

**Threaded reload + spinner.** `apply_action` is a **pure write step** returning `Outcome {
quit, reload: ReloadKind }` (`None`/`Preview`/`Full`); the **driver** refreshes. `Full`
reloads the list on a worker thread (re-opens the store from `(config, dir)`, stateless) over
an `mpsc` channel with a `throbber-widgets-tui` spinner; `Preview` (selection moved / body
save) reloads the one detail synchronously, never re-querying the list. `TuiState.loading`
is a pure flag.

**Command palette (`:`).** A fuzzy palette (`Mode::Palette`) indexes every TUI verb — a
single `PaletteCmd` enum + `PALETTE` const (the one place to extend; adding a verb surfaces
it by key and name). Filtering: `fuzzy_rank` over **nucleo-matcher** (helix/telescope engine,
`tui`-gated, terminal-free). Also exposes the theme switchers (no key).

**AI assists (`Mode::AiPrompt`/`AiResult`).** The palette exposes six AI verbs
(`PaletteCmd::Ai*`): **draft/revise body** + **ask** open a free-form prompt,
**summarize**/**lint**/**plan**/**plan-regenerate** act on the selected ADR. The pure layer
builds `Action::Ai(AiRequest)`; the driver runs it on a worker thread with the "thinking"
spinner. A `Draft` reply loads the `AI_MARKER`-tagged body into the editor; a `Popup` reply
shows scrollable read-only markdown. One call at a time; no provider → clear message.
**`AiPlan` reads the stored plan first** (ADR-0008 semantics, matching CLI `plan <ID>`):
with a marker-bracketed plan in the body, the pure layer opens the popup directly via
`plan::extract` — deterministic, provider-free, no thread; only a plan-less ADR requests
generation. `AiPlanRegenerate` is the explicit fresh-generation verb (CLI `--regenerate`);
its popup never overwrites the stored plan. Pinned by headless `TuiState` tests
(`plan_palette_surfaces_a_stored_plan_provider_free` + siblings).

**Fuzzy ADR pickers (`Mode::PickAdr`).** Two flows fuzzy-pick an ADR (one mode + overlay, by
`PickPurpose`): `Jump` (`Ctrl-P` → moves selection) and `Supersede` (`S` → the OLDER ADR).
Both reuse `fuzzy_rank`, then re-select or emit `Action::Supersede { new, old }`
(`Store::supersede` path).

**Preview scrolling & mouse.** Scrollable with a gutter scrollbar (`wrapped_line_count`
estimates height, since `Paragraph::line_count` is private in ratatui 0.30). Keys: `j`/`k`
line, `PageUp`/`PageDown` + `Ctrl-U`/`Ctrl-D` viewport, `g`/`G` top/bottom. Mouse wheel
scrolls the focused preview or moves the list selection.

**In-TUI body editor (modal / vi).** `i` enters `Mode::Edit`, loading the body into an
`EditorBuffer` — a pure multi-line plain-text editor (`lines: Vec<String>` + char-based
cursor) with vi ops but **no undo/selection/clipboard** (deliberate). **Modal**: a pure
`edit_insert: bool` is the Insert/Normal sub-mode (+ `edit_pending` for two-stroke
`gg`/`dd`); opens in **Insert**. Normal: `hjkl`/`w`/`b`/`0`/`$`/`gg`/`G` motions,
`i`/`a`/`I`/`A`/`o`/`O` → Insert, `x`/`dd`, **q/Esc → cancel**. Insert:
type/Enter/Backspace/arrows/Tab, **Esc → Normal**. **Ctrl-S** saves from either sub-mode;
a dirty cancel needs `y`/Esc; title + footer show `INSERT`/`NORMAL` + `[modified]`. Save
goes through `Store::set_body` (replaces only `.body`, re-serializes via
`format::serialize`), so frontmatter / `## Status` / banner / status dir are untouched and
an unedited round-trip is byte-identical. `e` is the external-`$EDITOR` escape hatch.

## Web dashboard (`web` feature)

`src/serve/` is a read-only Axum server; `--no-default-features`/`tui` never depend on
axum/tokio/notify. `adroit serve [--host --port]` exposes the `query` layer as a JSON API
(`/api/adrs`, `/api/adrs/{id}`, `/api/search`, `/api/stats`, `/api/graph`, `/api/check`,
plus `/api/workspace` + `/api/browse` for the in-browser directory picker) and serves an
embedded Vue 3 SPA (`web/dist` via `rust-embed`). Store reopened per request.
Markdown→HTML is server-side (`pulldown-cmark`); `render_markdown` **autolinks bare
`http(s)://` URLs** (skipping code blocks + existing links) and is the **XSS-sanitization
seam** (pulldown-cmark isn't a sanitizer): it escapes raw HTML events to text and routes
every link/image `dest_url` through `sanitize_href` (neutralizing
`javascript:`/`data:`/`vbscript:` → `#`). No endpoint writes ADRs; the one mutating route
`POST /api/workspace` only switches which dir the dashboard views (re-points the watcher).

- `src/serve/mod.rs` — router, API handlers, SPA serving, `AppState`.
- `src/serve/watch.rs` — the live-reload watcher.

**Auto live-reload.** A recursive `notify` watcher on the ADR dir (dedicated thread)
**coalesces** bursty events with a ~250ms debounce into one tick on a
`tokio::sync::broadcast` channel in `AppState`. SSE endpoint `GET /api/events` forwards one
`event: change` per tick; the Vue side opens an `EventSource` (`web/src/useLiveReload.ts`)
and re-fetches on `change`. Build the SPA with `just web-build` before `cargo build
--features web`; the embed dir has a `.gitkeep` so the crate builds without a Vue build
(server then serves a "run `just web-build`" hint while the JSON API stays live).

## Format profiles & layouts

Two on-disk profiles, via config (`format`, `layout`) or `--format`/`--layout`. Defaults:
markdown / by-status (status encoded by directory).

- `format = markdown` (default): MADR-style. Number + title from the H1 (`# ADR-NNNN:
  Title`); status from `## Status` + the directory. No YAML. Minimal-diff writes — a status
  change rewrites only the `## Status` line and `> State:` banner. `src/format.rs`.
- `format = frontmatter`: YAML-frontmatter + body (`src/frontmatter.rs`). **Numeric-only**
  — its YAML persists a `number:`, so it pairs only with `sequential`. `main` bails up front
  when combined with a slug scheme.
- `layout = by_status` (default): ADRs in `proposed/ accepted/ rejected/ superseded/
  deprecated/`; `README.md` + `adr-template.md` skipped; `next_number` = max across subdirs
  + 1; status changes move the file. `layout = flat`: one directory. `layout = by_category`:
  each immediate subdir is a **category** (an area, not status); status lives in `## Status`
  (so `dir_status` is `None`, a status change rewrites **in place**); numbering is **per
  category** (`Store::next_ref_in_category`), ADRs addressed by `category/NNNN`. Pairs with the
  `per_category` scheme; `new` requires `--category`; `Adr.category` carries the area;
  `migrate` to/from `by_category` is refused.

**Profile-mismatch guard + migrate.** `Store::detect_profile` infers on-disk layout/format
from files present (status-subdirs-with-numbered-`.md` ⇒ by_status, root-numbered ⇒ flat;
leading `---` ⇒ frontmatter). `Store::profile_mismatch` compares to configured opts; `main.rs`
**bails** before dispatch on any mismatch except for `migrate` (else a wrong
`--layout`/`--format` would silently hide ADRs or collide numbers). `Store::migrate(apply)`
(preview unless `--yes`; `--dry-run` forces preview) moves files verbatim for a layout-only
change or re-serializes via `format::serialize` for a format change (filenames preserved;
collisions refused), then `relink`s.

**`adroit config`** (`cmd_config`, in `main.rs` *before* the store opens, so it works on a
mismatched repo) shows/gets/sets settings. `Config::get_str`/`set_str` are the typed
key↔string accessors (validate on set); `CONFIG_KEYS` is the key list, `env_var_for` maps a
key to its `ADROIT_*` var. `config show` reports each key's resolved value + source
(flag/env/config/default); `set` writes `config.yaml` or, with `--local`, the project `.env`
(`upsert_env_file`). Keys include `relink_scope` (`all`/`self`/`none`, env
`ADROIT_RELINK_SCOPE`, flag `--relink-scope`).

Templates: `src/template.rs` (`madr`/`nygard` + custom file/`templates_dir`; a repo-local
`adr-template.md` wins). SUMMARY.md regeneration: `src/index.rs`.

`adroit review <number>` generates a review-kickoff doc from the built-in `review-kickoff`
template (business-day math in `review_window`). Pure generation, no git. Config `review_days`
(3) + `review_quorum` (3) supply defaults; `--days`/`--quorum`/`--output` override.

### Supersession round-trip

Both directions survive a read in BOTH profiles. Markdown: `format::parse_status_region`
parses the `## Status` region — `Superseded by [ADR-NNNN](...)` → `superseded_by`, `Supersedes
[ADR-NNNN](...)` → `supersedes` (tolerant of a bare `ADR-NNNN` + optional `>` banner).
Frontmatter: optional YAML fields. `query::graph` collapses the two reciprocal views into one
`Supersedes` edge (newer → older). The `Adr` model keeps them as `Option<Number>`;
`AdrSummary.supersedes` is a `Vec<u32>`.

### Cross-ADR link integrity (`src/links.rs` + `Store::relink`)

In by-status a status change moves the file between dirs, stranding relative links
(`[..](../proposed/0009-x.md)`) in other ADRs and the moved file.
`links::rewrite_links(content, source_dir, resolve)` is the pure engine: it scans
`](target)` spans and, for each *relative* `.md` target where `resolve(target)` yields a path,
rewrites it to the canonical relative path (preserving `#anchors`, keeping `./` same-dir);
external URLs / anchors / non-ADR links untouched. The engine is **scheme-agnostic** —
`Store::relink` keys a map by each ADR's `reference()` and resolves via `naming.ref_in_link`,
writing only changed files; `relink(apply=false)` is the dry-run.

**Relink scope on a status move (`relink_scope` on `StoreOptions`).** After a move,
`set_status_at` dispatches on `config::RelinkScope`: `all` (default) heals every inbound link;
`self` fixes only the moved file's outbound links; `none` does nothing. `self`/`none` make a
status-change PR touch only its own ADR, so concurrent decision PRs never collide — inbound
links are canonicalized by a post-merge `adroit relink` on `main` (the "heal-on-main"/
propose-on-branch workflow; `templates/ci/` + the "Concurrent contributors" page).
**`relink`, `renumber`, `migrate` are always full-scope** — only `set_status_at` consults
`relink_scope`.

`adroit relink` exposes the full relink on demand (repairs repos edited outside adroit, or
the post-merge bot). `cmd_check`'s link check is **identity-based**: a missing-target link
that still names an existing ADR is **stale** (`Severity::Warning` — `relink` heals it); a
link naming no existing ADR is **broken** (`Severity::Error`). `cmd_check` exits non-zero
only on an Error-severity problem; a warning-only report exits 0. `query.rs` resolves graph
link targets through `scheme.ref_in_link`.

`Store::renumber` (`adroit renumber <old> <new> [--file]`) resolves a duplicate number:
rename the file, rewrite its heading + self-refs, then `links::relabel_links_to`
retargets+relabels inbound `[ADR-old](…)` links matched by **basename** (a
same-number/different-slug sibling untouched), then `relink`. `--file` disambiguates when
`old` has two files. In **frontmatter**, supersession/typed refs are bare numbers in the
YAML, so renumber also remaps those via `frontmatter::remap_numeric_refs`.

The naming/identity **seam** (`src/naming.rs`) — `AdrRef` + `NamingScheme`
(`sequential`/`date`/`uuid`/`per_category`) + `Scope` — owns all scheme behavior
(`assign`/`parse`/`parse_ref`/`filename`/`display`/`heading`/`link_label`/`ref_in_link`/`ref_in_link_from`/`ref_matches`/`scope`),
so adding a scheme edits one module (`ref_in_link_from` is the category-aware variant: a
per_category same-category link `./0002-x.md` resolves relative to the source's category).
Consumers route through it: `Store::write`/`read` name files via `scheme.filename`;
`next_ref`/`find_path_by_ref` + `set_*_ref` address ADRs by `AdrRef`; the CLI parses `<ID>`
via `scheme.parse_ref`. **Sequential stays byte-identical** — the additive identity model
(`Adr.number: Option<Number>` + `Adr.slug: Option<String>`, `Adr::reference()`) keeps it the
no-op path. `date`/`uuid` work end-to-end — supersession is a scheme-agnostic `Option<AdrRef>`
(serde-untagged in frontmatter; resolved from the markdown link); the graph/view layer carries
`reference` + `address` strings so the TUI/web SPA route every ADR by `address`.
`renumber`/`review` are numeric-only and bail otherwise; `per_category` rides the
`by_category` layout.

### Review deadlines (`review_by`)

`Adr.review_by: Option<ReviewBy>` is an optional ISO-8601 (`YYYY-MM-DD`) deadline (a
`time::Date` newtype). Persists in both profiles: a `review_by` YAML field, and a `Review by:
YYYY-MM-DD` line in the `## Status` region in markdown (format-preserving). `query` sets
`AdrSummary.review_due = true` when an ADR is still `Proposed` and either has a `review_by`
on/before today **or** has aged past `review_overdue_days` (config, default 30; `0`/`None`
disables) from its creation date (threshold on `StoreOptions`). `Stats.review_due` collects
those rows (web Stats "Review due" tile). Set via `adroit set-review <number> <YYYY-MM-DD>`
(or `--clear`) → `Store::set_review_by`.

The no-subcommand TUI launches via `tui::run(cfg, dir)` (`dir` resolved from `--dir`/config,
same dir `serve` gets; store-opening seam `tui::open_store(cfg, dir)`).

## Forge integration (`forge` feature)

Opt-in adapters driving the *process* lifecycle (a tracker issue + a code-review PR/MR)
alongside the ADR's *decision* lifecycle. The `forge` feature adds a **blocking** HTTP client
(`ureq`); the core CLI stays synchronous, and `--no-default-features`/`tui`/`web` never depend
on `forge`.

**Two roles, trait objects.** `src/forge/mod.rs` defines `Forge` (PR/MR host) + `Tracker`
(issue host); a *same-system* provider implements both over one client + token
(`forge/{github,gitlab}.rs`, `ADROIT_{GITHUB,GITLAB}_TOKEN`), a *split* setup uses the
separate `tracker`. `forge/noop.rs` is the null-object preview adapter.

**Clean dispatch (two axes).** Compile-time: `#[cfg(feature="forge")]` lives only on the
`mod` line in `lib.rs`, the `src/forge_hook.rs` facade (twin real/no-op defs), and the
**forge CLI surface** in `src/cli.rs` — the `--forge`/`--dry-run`/`--yes` opt-in flags on
shared verbs *and* the forge-only commands (`init`/`auth`/`sync`/`notify`), so a no-forge
build doesn't expose them (`publish` stays — offline; `help_template` `cfg_attr`-omits the
Forge section). Verb handlers call `forge_hook::*` unconditionally (no-op
twins); `main` builds `ForgeFlags` with a small `#[cfg]`. Runtime: `forge::open(&ForgeConfig)`
is a thin dispatcher (`match Provider`); adding a provider = one match arm + module. HTTP is
behind the `HttpTransport` seam (tested with `FakeTransport`). `ADROIT_FORGE_WIRE=1` echoes
every request/response (method/url/body + status/body, **never headers/token**) to stderr from
the shared `rest_call` chokepoint — a read-only dogfood diagnostic for diffing live wire shapes
against the cassette.

**Verbs wired** (opt-in via `--forge`, `--dry-run`/`--yes`; graceful-offline = warn + keep
the local write): `new` creates the issue + a draft PR off an `adr/NNNN-…` branch (`src/git.rs`)
and records both URLs in a format-preserving `## References` section; `set-status accepted`
verifies `review_quorum` approvals + CI then merges the PR + closes the issue (refuses if
blocked; previews unless `--yes`), then **pushes the relink commit** (`before_status_change`
fast-forwards the base, the local `proposed/ → accepted/` move + relink lands via
`after_status_change` commit + push — so `accepted/` reaches `main` in one command; a
dirty/diverged push stays local with a warning). `set-status
rejected`/`deprecated` close the PR + mark the issue won't-fix; `supersede` closes the old
ADR's issue/PR (each orchestration has a testable core with mock/noop adapters). Read-side:
`check --forge` appends `ProblemKind::ForgeIntegration` warnings; `list --forge` enriches rows
(`AdrSummary.forge_data` — PR review/CI/merge/`pr_closed` state **and** the split tracker
issue's native `issue_state` open/closed, `enrich` reading both `forge` + `tracker`;
`ForgeData::status_parts` renders both, so a split setup shows `PR merged, issue closed` — a
closed-unmerged PR reads `PR closed`); `review --forge`
(`forge::review_kickoff`) **un-drafts** the PR
(`mark_ready`), upserts the kickoff comment (its relative links **absolutized** to
`Forge::web_blob_base` URLs so they resolve in a PR/Linear comment), @-mentions
`forge.reviewers`, tags a `review-by:<date>` label; `set-review --forge` upserts a comment +
sets the tracker's native due date. All via default-no-op trait methods
(`add_label`/`mark_ready`/`set_due_date`; comment upsert = `upsert_{pr,issue}_comment` over
`plan_upsert` + `comments_on_*`/`update_*_comment`; GitHub Issues have no due date, monday no
edit API → no-dup, no-refresh). `accepted` un-drafts before merge + takes `--quorum` (overrides
`review_quorum`).

**Providers.** `github` + `gitlab` (same-system Forge+Tracker); `jira` (REST v2), `linear`,
`monday` are split **Tracker**-only adapters (no `Forge`) chosen by `forge.tracker`. **Linear +
monday are GraphQL** (`forge/{linear,monday}.rs`): one `POST`, reusing `rest_call` + an
`errors[]` check (GraphQL returns 200 on error). Linear files to a **team** (`tracker_project`
= team key), maps `Transition`→workflow-state `type` (`completed`/`canceled`/`unstarted`), and
stores a **slug-stripped** URL so `read_refs` recovers `ENG-123` from the trailing segment
(resolved to the UUID by team-key+number); monday files an **item** to a board
(`tracker_project` = board id, `tracker_host` = subdomain), matching a Status-column label.
All split trackers are token-only (`ADROIT_{JIRA,LINEAR,MONDAY}_TOKEN`). Jira auth: Cloud Basic
`email:token`, Server/DC Bearer PAT. GitHub/GitLab use one token cloud or self-hosted; only
`forge.host` changes (GHE `/api/v3`). `gh_issues`/`gl_issues` alias `native`.

**Cross-cutting verbs.** `adroit init` (wizard: detect provider+repo from the remote, pick the
tracker, write `forge.*` + optionally `./.env` / `adr-template.md` / a pre-commit `adroit check`
hook; `--yes`/`--print`); `adroit publish --to
<target>` (render the accepted set via the `Publisher` seam (`src/publish/`): `static`
(default), `mdbook`, `mkdocs`, `hugo`, `docusaurus`, `jekyll`; core/offline, idempotent;
default via `publish_target`. adroit **produces** the tree; Confluence/Notion hosting stays the
consuming repo's CI); `adroit notify
<id>` (Slack/Teams webhook); `adroit auth <github|gitlab|jira|linear|monday> [--token]
[--email]` (token env → credential store → none); `adroit reconcile` syncs local status after
out-of-band changes (reports drift; `--yes` moves a merged PR's ADR to `accepted/`).

**Read-only dashboard.** Two forge-aware routes, `null`/empty without an active forge (on
`forge_hook::*` twins): `GET /api/adrs/{id}/forge` feeds `DetailView.vue`'s issue/PR panel;
`GET /api/forge/summary` (with `AppState.review_quorum`) feeds the dashboard tiles. Never
*writes* to the forge.

**Forge config is repo-scoped.** `forge.*` is global, but the active dir (dashboard switches
dirs; CLI runs anywhere) may belong to a *different* repo than `forge.repo`. `dir_matches_forge`
compares the dir's `origin` to `forge.repo`; on a definite mismatch **every** forge entry point
(`skip_dir_mismatch`/`skip_path_mismatch`, after the `cfg.forge` check) warns once and skips the
forge side — mutating verbs keep the local record, touching nothing in the wrong repo.
Undeterminable cases (no `repo`/no recognizable remote) apply.

**Config.** `config::ForgeConfig` (`Provider`, `repo`, `host`, `oauth_client_id`,
`branch_prefix`, `base_branch`, `reviewers`, `tracker: TrackerProvider`) under `Config.forge`;
tokens env-only (`#[serde(skip)]`). `forge` is a default feature, so `just lint`/`test` cover the build.

**Credential storage + device-flow auth (`keychain` feature).** Tokens (forge + anthropic key)
go through one seam — `config::load_credential`/`store_credential` — over a `CredentialBackend`
chain: the **OS keychain** (`keyring` pure-Rust backends, no C deps) first, then the `0600`
`FileBackend`. `keychain` is enabled by **both** `ai` and `forge`; the bare core is file-only.
`ADROIT_CREDENTIAL_STORE=auto|file|keychain` overrides (pins `file` for tests); `cmd_auth` never
echoes the token. `adroit auth github`/`gitlab` with no `--token` runs an **OAuth device-flow**
login (`src/forge/oauth.rs`, pure core over `HttpTransport`, `forge.oauth_client_id`;
manual-prompt fallback with no client id); `jira`/`linear`/`monday` token-only. `cmd_auth` also
accepts `anthropic` (stores the AI key in the same store, read by `config::anthropic_key`).

## AI authoring (`ai` feature)

Opt-in AI-assisted authoring (on `rig`). Same shape as `forge`: a **synchronous** `AiProvider`
trait, async bridged by a single `block_on` at the CLI boundary — `--no-default-features`/
`tui`/`forge` never pull in tokio. **AI only ever writes prose** — identity/status/dates/links
stay mechanical.

**Always compiled** (`src/ai/mod.rs`, `src/ai_hook.rs`): the `AiProvider` trait,
`CompletionRequest`/`AiError`, the Socratic `Interview` + `build_request`/`draft_body`,
`AI_MARKER`, the `FakeProvider` stand-in — so the interview flow is unit-testable with no
network and no `ai` feature. The facade `ai_hook::open_provider(cfg)` resolves `ADROIT_AI_FAKE`
(offline echo) → the rig provider (only when `ai.enabled`, never in a core build) → `None`.
Every AI draft is mechanically **sanitized** before the splice (`sanitize_draft`, shared by
`draft_body`/`draft_compose` → covers interview/`draft`/`compose`/`import --ai`/TUI): a
re-emitted leading H1 / `> State:` banner is dropped; a re-emitted `## Status` /
`## Stakeholders` skeleton section is dropped wherever it appears (the splice preserves the
document's own — a model copy is always a duplicate; run-1 ADR-0001/0005); echoed adroit
markers (`ai-suggested`/`seeded-from-assessment`) are dropped; trailing conversational
residue ("Please review this revised ADR body…" — run-1 ADR-0002; `RESIDUE_OPENERS`) is
stripped with the rule it orphans; **whole-line bracket placeholders** ("[Insert
implementation plan or other details as needed]" — run-2 playbook ADR-0010; `[Your Name]`)
are dropped wherever they appear, plus the rule a tail placeholder orphans — detected by
`lint::bracket_placeholder` (curated `PLACEHOLDER_OPENERS` on a word boundary; links,
checkboxes, citations, single-token `[section]` lines, and fenced/indented code never
match); and an unmanaged `## Implementation` section with real
content is retitled `## Implementation notes` — so no model output can read as the
hand-written section that blocks `plan --save` (ADR-0008 reserves that heading; echoes of
the marker-bracketed stored plan or the prompt-only template placeholder stay verbatim, and
plan-span content is never touched). Properties `ai_drafts_never_block_plan_save` +
`ai_drafts_never_duplicate_the_mechanical_preamble` +
`ai_drafts_never_carry_bracket_placeholder_lines` (`tests/parsers.rs`; bolero twins
`fuzz_ai_sanitizer`/`fuzz_lint`) pin the invariants;
found in the M5 ollama dogfood rehearsal and the iteration-1/-2 full-loop runs (book:
Development → The Adopt Read Slice). **Drops are observable, not silent**
(iteration-3 run-3 wart): the counting core `sanitize_draft_counted` returns a
`view::SanitizeReport` (per-rule drop counts — `bracket_placeholder`/`residue`/
`skeleton_echo`/`identity_echo`/`marker_echo`, non-blank lines only; the
`## Implementation` retitle keeps content, so it's NOT a drop), `sanitize_draft`
is the count-free wrapper, and `draft_compose_counted` surfaces it. `import --ai`
aggregates across seeds, prints a `sanitized: …` stderr line, and carries the
`sanitized` object in `-o json` (zeros-omitted; additive — `manifest_schema`
stays 1). Pinned by `import_ai_surfaces_sanitizer_drop_telemetry` +
`a_dropped_bracket_placeholder_is_always_counted`.

**`ai`-gated** (`src/ai/rig_provider.rs`): `RigProvider` wraps rig (aliased from `rig-core`) —
Anthropic and Ollama (local) — on a current-thread tokio runtime, `block_on`-ing rig's agent.
Every ollama request pins **`options.num_ctx = OLLAMA_NUM_CTX` (8192)** via
`additional_params` — ollama otherwise silently truncates prompts at its 2048-token default
(the iteration-2 suite root cause; KV-cache memory multiplies per `OLLAMA_NUM_PARALLEL`
runner at wider windows). Wire-shape-pinned by `tests/ai_rig.rs` (loopback fake ollama,
asserts the literal request JSON; `req.max_tokens` is deliberately NOT mapped to ollama —
`/api/chat` has no such field and generation stays unclipped).

**`new --interview`** (`run_interview`): asks the fixed `INTERVIEW_QUESTIONS` over **plain
stdin** (robust on non-TTY/piped), builds a corpus summary, drafts, then **splices**: keeps
every line before the first `## Context…` (mechanical heading / `## Status` / stakeholders) and
replaces the prose with the marked draft via `Store::set_body_ref`. Degrades to the plain
template with no provider.

**`draft <ID>`**: the after-the-fact `new --interview` — runs the same interview on an existing
ADR (shared `interview_and_draft`: Q&A → `ai::draft_body` → `splice_ai_draft`), then opens the
editor; `require_provider` (no fallback). Flow: `new` → `draft` → `edit` → PR.

**`plan <ID>`** (ADR-0008, `src/plan.rs`): generation reads an ADR + corpus and asks for an
ordered implementation checklist; **`--save` persists it INSIDE the ADR document** as a
`<!-- adroit:plan -->`…`<!-- /adroit:plan -->`-bracketed `## Implementation` section (pure
splice/extract engine in `src/plan.rs`; replaces the template's placeholder section in place,
tolerates trailing supersession banners, refuses a stored plan without `--force` and a
hand-written `## Implementation` always; `--save --dry-run` previews). With a stored plan the
bare `plan <ID>` is a **deterministic, provider-free read** (verbatim, byte-stable);
`--regenerate` forces a fresh call (print-only). `-o json` emits `view::Plan`
(`reference`/`title`/`plan`/`stored` — `stored` additive); `show -o json`/`AdrDetail` carries
the stored plan as `plan`. Generation bails (not degrades) with no provider; the AI body
splice (`draft`/`compose`/`import --ai`) **preserves** a stored plan section. **`summarize
<ID>`**: a one-paragraph read-only TL;DR (stdout or `--out`).

**`lint <ID>`** (`src/lint.rs`): authoring-quality checks, **distinct from `check`**
(structural validity). `lint::lint(body)` is the deterministic core — leftover MADR
placeholders, missing/empty Negative Consequences (`##` and `###` depth both accepted —
models write h2 where MADR nests h3; depth is shape, not substance), `## Considered
Options` with <2 recorded options (list items and `###` sub-headings both count — MADR's
long form and most models record options as sub-headings), a **Warning** on repeated
top-level (`##`) sections (the run-1 skeleton-echo shape), and a **Warning** on whole-line
bracket placeholders (`bracket_placeholder` — the run-2 novel-filler shape; fenced code
exempt). Findings carry `severity`
(reusing `view::Severity`): Errors gate the exit, Warnings (incl. the `--ai` advisory
finding) don't. Needs **no AI** (CI-usable); `--ai` appends one advisory finding. Exits
non-zero on **mechanical Error** findings only.

**`related <ID>` / `dedupe <ID>`** (`src/similar.rs`): retrieval but **mechanical — NO AI**:
TF-IDF cosine over the corpus. `related` excludes ADRs already linked to the target; `dedupe`
includes them.

**`ask "<q>"`**: **mechanical retrieval** (`similar::rank` with the question as a transient
target) feeds top ADR excerpts to the provider, which answers with citations. Human = answer +
`(sources: …)` on stderr; `-o json` = `{answer, sources}`. Bails with no provider.

**`compose`** (`adroit compose <ID> "<instruction>"`; templates
`templates/ai/compose.{system,prompt}.md`): instruction-driven (re)drafting — the model returns
a complete revised body (prose only) for a **free-form instruction** on the *current* body (vs
`draft`'s wholesale interview re-run). Splices via `splice_ai_draft` + opens the editor
(`--no-edit`); also the engine behind the TUI's AI body-revise. Requires a provider.

**Config.** `config::AiConfig` (`provider: AiProviderKind` anthropic/ollama, `model`, `enabled`
kill-switch, `host`) under `Config.ai` (`Option`, absent by default).
`config::resolve_ai(cfg.ai)` overlays `ADROIT_AI_*` env overrides
(`ENABLED`/`PROVIDER`/`MODEL`/`HOST`), so AI is enablable via env/`.env` with no `config.yaml`
edit. Key is env-only (`config::anthropic_key()` → `ADROIT_ANTHROPIC_KEY` / credential store).
`serde_json` is a core dep; `rig`+`tokio` are `ai`-only. `ai` is a default feature, so `just
lint`/`test` cover the build.

## Design principles & conventions (SOLID / DRY / Rust)

Rules a change must preserve (the codebase already follows them — an audit against
`~/repos/talaria`'s shared conventions found them consistent).

### Search before adding (DRY)
Before adding a function/error variant/helper, **grep for an existing one**:
- ADR identity / filenames / link refs → the **naming seam** (`naming.rs`); never hand-parse —
  call `scheme.parse`/`ref_in_link`/`ref_matches`.
- relative links → `links::rel_link` (one common-prefix walk).
- config → store options → `StoreOptions::from_config` (the one place).
- reading/deriving ADR data → the `query`/`view` layer.

A duplicated algorithm is a future-divergence bug — extract instead of copying.

### Simplicity first
Prefer the simpler solution; remove old paths rather than keep parallel versions; don't add
backwards-compat shims unless asked (see *converge, don't accumulate*).

### Lib/bin & error layering
- All logic in the **library crate**; `main.rs` is argument-marshalling + human-rendering glue.
- **Typed errors (`thiserror`) in the data/parse layer; `anyhow` in the binary + thin surface
  layers.** `adr`/`format`/`frontmatter`/`store`/`query`/`naming`/`links`/`config`/`template`/
  `git` expose `thiserror` enums composing with `#[from]` / `#[error(transparent)]`. **Never
  stringify a typed cause** (`map_err(|e| E::X(e.to_string()))`) — add a `#[from]` variant.
  `main.rs` + the feature-gated surfaces (`serve`, `tui`) + forge orchestration may use
  `anyhow`; pure parsers stay `anyhow`-free.

### Seams are enums/traits with one owner (Open/Closed)
Extend by **adding a variant to a seam**, not editing call sites. Gold standard: the **naming
seam** (every scheme behavior a method on `NamingScheme`/`AdrRef`).
- Behavior varying by an enum (`Format`, `Layout`, `NamingScheme`) belongs as a **named
  method/predicate on that enum** (e.g. `NamingScheme::is_numeric`/`scope`), not an ad-hoc
  `match`/`== Variant` scattered across files. A third `== Format::X` site is the signal to
  lift it onto `Format`.
- A new pluggable backend (forge/AI provider, publish target, tracker) is a **trait impl + one
  factory arm** (`forge::open`, `ai_hook::open_provider`).

### Trait design (capability, focused, colocated)
- A trait names a **capability** — `AiProvider`, `Tracker`, `HttpTransport` — not a data
  structure.
- Keep traits **focused**; never give a type an impl it can't honor (Jira is a `Tracker`, not
  a `Forge` — no panicking `Forge` impl; the `(Option<dyn Forge>, Option<dyn Tracker>)` pair
  keeps roles independent). **Colocate** a trait with its primary impl (no `src/traits/`).
- **Prefer generics over `dyn`** unless dispatch is genuinely runtime-selected (`dyn` is
  load-bearing at the forge factory + the `HttpTransport` seam).

### Dependency inversion & feature confinement
Depend on the trait/facade, not the concrete. The **hook facades** (`forge_hook`, `ai_hook`)
are always compiled, so verb handlers call them with **no `#[cfg]`**. A feature's
`#[cfg(feature = …)]` is confined to three places: the `mod` line in `lib.rs`, the facade's
twin defs, and the CLI surface.

### Pure core, effectful shell
Transform logic is **pure, terminal/network/git-free**, unit-tested headlessly: `format::*`,
`links::*`, `naming::*`, `lint::lint`, `similar::rank`, `template::*`, `history::parse_log`,
the TUI `apply_action` layer, the forge cores. Effects (fs/git/http) live in the shell
(`Store`, `main`). Push the decidable part into a pure function; keep I/O thin around it.

### Test / production separation (hard rule)
**Never** put test-only state in a production type — no `Test` enum variant, no
`is_test`/`test_mode` field, no `if cfg!(test)` branch in production logic. Use documented
**runtime env overrides** (`ADROIT_AI_FAKE`, `ADROIT_TODAY`), `#[cfg(test)]` helpers
(`Store::next_number`), and injected fakes (`FakeProvider`, `FakeTransport`).

### Rust idioms
- **Newtypes** for domain ids (`AdrId`/`Number`/`Created`/`ReviewBy`/`AdrRef`). `strum` for enum
  `Display`/`FromStr` (`ascii_case_insensitive` in sync with serde `rename_all`).
- `Cow<str>` where a transform is usually a no-op (`format::normalize_lone_cr`); borrow over
  `.clone()`, `&str` over `String` in signatures.
- **Document genuinely-infallible `expect`s**; never a silent `unwrap` on a fallible runtime
  path — degrade gracefully, as `git`/`history` do.
- Internal API is `pub(crate)`; **test-only** items are `#[cfg(test)]`. Slice strings on
  **char** boundaries (`.chars()`), not bytes (a real fuzz-found bug in `naming::display`).
