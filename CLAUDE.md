# adroit

A snappy CLI for managing Architecture Decision Records, built with Rust.
An interactive TUI (browse, triage, and in-terminal body editing) ships behind
the `tui` feature; a read-only web dashboard (`adroit serve`) with auto
live-reload ships behind the `web` feature.

## Build & Test

Always use `just` recipes — never raw `cargo` or `mdbook` commands.

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
```

## Project Layout

- `src/lib.rs` — library crate root
- `src/main.rs` — thin binary entry point, delegates to lib
- `src/view.rs` — plain serde view types (`AdrSummary`, `AdrDetail`, `Stats`,
  `Graph`): the single shared contract for "what a surface can show"
- `src/query.rs` — the shared read API over `Store`
  (`summaries`/`detail`/`search`/`stats`/`graph`) that builds the view types
- `src/history.rs` — git-derived ADR dates + lifecycle (shells `git log`)
- `tests/cli.rs` — integration tests against the compiled binary
- `docs/` — mdbook user manual source (`book.toml` + `src/`), published to
  GitHub Pages; build output goes to `docs/book/` (gitignored)
- `justfile` — all dev workflow recipes

## Read/query layer (shared by all surfaces)

Read/derive logic lives once in `src/query.rs` and returns the pure serde
structs in `src/view.rs`. Every surface consumes this seam: the CLI read
commands (`list`/`search`/`show`) call `query::*` and render the returned view
types, and the planned TUI and `adroit serve` web JSON API (behind future
`tui`/`web` Cargo features) will call the same functions so a stat/search/graph
is computed identically everywhere. View types are filesystem- and
UI-framework-free and derive `Serialize` so the web surface can emit them as
JSON with no extra mapping. Write logic stays in the `Store` write path (CLI +
future TUI only); the query layer never writes. Markdown→HTML rendering is
deliberately deferred to the web surface (`AdrDetail::body_html` stays `None`).

**Dates & lifecycle come from git (`src/history.rs`).** The markdown profile
persists no creation date and a clone resets mtime, so `query` resolves an
ADR's `created`, `last_modified`, and status timeline from git instead.
`history::open(dir)` probes for a work tree once (per query call); then one
`git log --follow --name-status` per file is parsed (pure `parse_log`,
unit-tested without git) into an `AdrHistory { created, last_modified, events }`.
In the by-status layout each status change is a directory rename git records, so
the timeline (proposed → accepted/rejected/superseded) is reconstructed from
renames; `status_of` is injected as `Store::dir_status` so custom dir names are
honored and flat layout yields no milestones. `query::load_resolved` resolves
`created` for every list/stats row (precedence: git → frontmatter-authored
`created:` → mtime → parsed `now()`); `query::detail` additionally fills
`AdrDetail.history` (`Vec<TimelineEvent>`) and `last_modified`. The module only
*reads* git and degrades gracefully (untracked file / no git → fallback). One
`git log` per ADR; fine for small ADR repos (a single-pass optimization is noted
in the module for large trees). Surfaced in `adroit show`, the TUI preview
header, and the web detail view.

## Interactive TUI (`tui` feature)

`src/tui.rs` is a ratatui two-pane app (list + preview) gated behind the `tui`
Cargo feature; `--no-default-features` builds the core lib + CLI with no
ratatui/crossterm. It is split into a pure, terminal-free layer (`TuiState`,
`Mode`, `Action`, and the `EditorBuffer`) that is unit-tested headlessly, and a
thin `driver` submodule that wires crossterm + ratatui and runs the headless
`apply_action` against a `Store`. Reads go through `query`; writes go through
`Store`.

**Markdown preview & themes.** The preview pane renders the ADR body as
GitHub-Flavored Markdown via `the-other-tui-markdown` (a `tui`-gated optional dep
on `ratatui-core` 0.1, so no duplicate ratatui in the tree). In the `driver`
module, `render_markdown_body(body, theme)` returns a ratatui `Text` (`into_text`
for the default ANSI theme, `into_text_with_theme` + `gruvbox_theme()` for
gruvbox); `render_preview` prepends the metadata header and `m` toggles
`TuiState::preview_raw` (rendered ↔ raw source). Themes are
`config::MarkdownTheme { Default, Gruvbox }`, resolved from `--theme` /
`ADROIT_THEME` / `tui_theme` config (flag > env > config) and applied via
`TuiState::set_md_theme` in `tui::run`. Code-block syntax highlighting is
deferred. The editor (`i`) always shows raw source.

**In-TUI body editor.** Pressing `i` on the selected ADR enters `Mode::Edit`,
loading the body into an `EditorBuffer` — a pure multi-line plain-text editor
(`lines: Vec<String>` + char-based `cursor_row`/`cursor_col`) with
`insert_char`, `insert_newline`, `backspace`, `move_left/right/up/down`,
`home`/`end`, and `from_str`/`to_string`. No undo/selection/highlighting — a
correct plain-text editor is the bar. Edit-mode keys: type to insert, Enter for
newline, arrows + Home/End to move, **Ctrl-S** to save, **Esc** to cancel (a
dirty buffer requires a `y`/Esc confirm; the title shows `[modified]`). Save
goes through `Store::set_body`, which reads the ADR, replaces only `.body`, and
re-serializes via the existing `format::serialize` path — so frontmatter /
`## Status` / banner / status dir are untouched and an unedited round-trip is
byte-identical. `e` remains the external-`$EDITOR` escape hatch.

## Web dashboard (`web` feature)

`src/serve/` is a read-only Axum server gated behind the `web` Cargo feature;
`--no-default-features` and the `tui` feature never depend on axum/tokio/notify.
`adroit serve [--host --port]` exposes the shared `query` layer as a JSON API
(`/api/adrs`, `/api/adrs/:n`, `/api/search`, `/api/stats`, `/api/graph`) and
serves an embedded Vue 3 SPA (`web/dist`, embedded via `rust-embed`). The store
is reopened per request, so every response reflects current on-disk state.
Markdown→HTML rendering is server-side (`pulldown-cmark`). It is strictly
read-only: no write endpoints, and the module imports only the read side.

- `src/serve/mod.rs` — router, API handlers, SPA serving, `AppState`.
- `src/serve/watch.rs` — the live-reload watcher (see below).

**Auto live-reload.** When ADR files change on disk (CLI/TUI/`$EDITOR` edits or
git operations), the open dashboard refreshes automatically. A single recursive
`notify` filesystem watcher on the resolved ADR dir runs on a dedicated thread;
raw events (which arrive in bursts) are **coalesced** with a ~250ms quiet-window
debounce into a single tick, published on a `tokio::sync::broadcast` channel
held in `AppState`. The SSE endpoint `GET /api/events` subscribes each browser
to that channel and forwards one `event: change` per tick (with periodic
keep-alive comments); the Vue side opens a native `EventSource`
(`web/src/useLiveReload.ts`) and re-fetches the current view on `change`
(auto-reconnects on drop). The watcher only observes — it adds no write paths.
Build the SPA with `just web-build` (runs `npm install && npm run build` into
`web/dist`) before `cargo build --features web`; the embed dir has a `.gitkeep`
so the crate builds without a Vue build present (the server then serves a "run
`just web-build`" hint while the JSON API stays live).

## Format profiles & layouts

adroit supports two on-disk profiles, selected via config (`format`, `layout`)
or the `--format` / `--layout` flags. Defaults are the markdown / by-status
convention (status encoded by directory).

- `format = markdown` (default): MADR-style markdown. Number + title from the
  H1 (`# ADR-NNNN: Title`); status from the `## Status` section and the
  directory. No YAML frontmatter. Writes are minimal-diff — a status change
  only rewrites the `## Status` line and `> State:` banner; unchanged
  round-trips are byte-identical. See `src/format.rs`.
- `format = frontmatter`: the original YAML-frontmatter + body format
  (`src/frontmatter.rs`).
- `layout = by_status` (default): ADRs grouped into `proposed/ accepted/
  rejected/ superseded/ deprecated/` subdirs; `README.md` and `adr-template.md`
  are skipped; `next_number` is the max across all subdirs + 1; status changes
  move the file. `layout = flat`: one directory (original behaviour).

Templates live in `src/template.rs` (built-in `madr` + `nygard`, plus custom
file/`templates_dir`, with a repo-local `adr-template.md` preferred). SUMMARY.md
regeneration lives in `src/index.rs`.

The `review` command (`adroit review <number>`) generates a review-kickoff doc
from the built-in `review-kickoff` template in `src/template.rs` (rendered via
`render_kickoff` / `KickoffParams`, with business-day date math in
`review_window`). It is pure generation — no git operations. Config keys
`review_days` (3) and `review_quorum` (3) supply the defaults; `--days`,
`--quorum`, and `--output` override them.

### Supersession round-trip

Both supersession directions survive a read from disk in BOTH profiles. In the
markdown profile `format::parse_status_region` parses the whole `## Status`
region: `Superseded by [ADR-NNNN](...)` → `superseded_by` and `Supersedes
[ADR-NNNN](...)` → `supersedes` (tolerant of a bare `ADR-NNNN` and an optional
`>` banner marker). In the frontmatter profile `supersedes`/`superseded_by` are
optional YAML fields (`skip_serializing_if`, so clean files stay clean).
`query::graph` collapses the two reciprocal views of one supersession into a
single `Supersedes` edge (newer → older) via `push_unique`. The `Adr` model
keeps `supersedes`/`superseded_by` as `Option<Number>` (a markdown ADR carries
at most one of each note); `AdrSummary.supersedes` stays a `Vec<u32>` for the
view contract and is filled from that single optional.

### Review deadlines (`review_by`)

`Adr.review_by: Option<ReviewBy>` is an optional ISO-8601 (`YYYY-MM-DD`) review
deadline (`ReviewBy` is a `time::Date` newtype mirroring `Created`). It persists
in both profiles: an optional `review_by` YAML field in frontmatter, and a
`Review by: YYYY-MM-DD` line inside the `## Status` region in markdown
(format-preserving — `format::rewrite_review_by` upserts/removes only that line;
unchanged docs are byte-identical). `query` sets `AdrSummary.review_due = true`
when an ADR is still `Proposed` and either has a `review_by` on/before today
(local, via `OffsetDateTime::now_local`) **or** has aged past
`review_overdue_days` (config, default 30; `0`/`None` disables) measured from its
resolved creation date — so an aging backlog surfaces with no per-ADR deadline.
The threshold rides on `StoreOptions` (carried from config by each surface's
`store_options` builder). `Stats.review_due` collects those rows (lighting up the
web Stats "Review due" tile/panel). Set the deadline from the CLI
with `adroit set-review <number> <YYYY-MM-DD>` (or `--clear`), wired through
`Store::set_review_by` (mirrors `set_status`/`set_body`).

The no-subcommand TUI is launched via `tui::run(cfg, dir)`, where `dir` is the
directory `main.rs` already resolved from `--dir`/config (same dir `serve`
receives), so `adroit --dir X` opens the TUI against `X`. The store-opening seam
is `tui::open_store(cfg, dir)`.

## Conventions

- Lib/bin separation: all logic in the library crate, `main.rs` is glue only
- Use `thiserror` for library error types, `anyhow` for the binary
- Use `strum` for enum Display derives
- Use newtypes for domain identifiers (e.g. `AdrId`, `Number`, `Created`)
