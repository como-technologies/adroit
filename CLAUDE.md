# adroit

A snappy CLI for managing Architecture Decision Records, built with Rust.
An interactive TUI (browse, triage, and in-terminal body editing) ships behind
the `tui` feature; a read-only web dashboard (`adroit serve`) with auto
live-reload ships behind the `web` feature.

## Working agreements (IMPORTANT — read first)

- **Never `git push` / force-push / create / merge / comment on PRs without the
  user's explicit, in-the-moment permission.** Committing locally is fine and
  expected; *publishing* is not. A one-time "push" or "create a PR" authorizes
  *that* action only — ask again every subsequent time. Do not infer standing
  permission.
- **Never write a bare `#<number>` in any GitHub-rendered text** — commit messages,
  PR titles/descriptions, PR/issue comments — because GitHub auto-links it to an
  unrelated issue/PR. Use `bug N` / `finding N` / `rule N` / plain `N` (or a table
  cell `| N |`, which does not link) instead. This applies to internal blitz / bug /
  check-rule numbers, which are NOT GitHub issues.
- **All documentation lives in the mdbook** (`docs/src/**`, wired into
  `docs/src/SUMMARY.md`, built with `just book`). Do NOT create standalone
  Markdown docs anywhere else (`docs/*.md`, parallel doc trees, ad-hoc reports).
  One doc system, one style. Contributor/dev docs go under the book's
  **Development** section.
- **Keep code and docs in sync, always.** When you change behavior, update the
  relevant mdbook page *and* this file in the same change, and verify by running
  the CLI — docs must reflect what the code ACTUALLY does. Periodically sweep
  code↔docs for drift. Run `just book` to confirm the manual builds.

## Design principles — statelessness & idempotency (architectural invariant)

adroit is **stateless** and its commands are **idempotent where it makes sense**.
Treat both as invariants every change must preserve:

- **The only state is the filesystem.** A command's entire input is the ADR
  documents on disk *plus* the config resolved at startup (flag > process-env >
  `.env` > `config.yaml` > default). There is no daemon, database, cache, index
  file, lock file, or session/cross-command state. The single process-global
  (`GIT_STRICT_WARNED` in `query.rs`) is a warn-once-per-process UX flag that
  resets on every invocation and never affects output. `adroit serve` reopens the
  store **per request**, so every response reflects current on-disk state; its only
  in-process state is the active-dir pointer + the live-reload watcher, both scoped
  to that one `serve` run.
- **Converge, don't accumulate.** A mutating command reads current on-disk state,
  computes the target, and writes **only what differs** (minimal-diff; a file
  already in the target state round-trips byte-identical). Re-running the same
  command with the same args on the same on-disk state is therefore a no-op.
- **Idempotent verbs** (re-run = byte-identical): `set-status`, `supersede`,
  `set-review`, `relink`, `migrate` (converges to a fixpoint, then "nothing to
  migrate"), `index`, `link`/link-rewriting, `publish`, `check` (read-only).
- **Intentionally non-idempotent verbs** are *imperative events*, not
  *desired-state assertions*, so repeating them repeats the event — by design:
  `new` (allocates the next ADR each run), `renumber old new` (one-shot rename;
  re-running fails because `old` no longer exists), `notify` (posts a fresh
  message), and the forge/git side effects (issue/PR creation, commit, push).
  `new` keeps its non-idempotent semantics but adds a **duplicate guard**
  (`dup_guard` in `main.rs`): on an exact (case-insensitive) title match it warns,
  lists the match + the top similar ADRs (`similar::rank` over the new title), and
  prompts `[y/N]` on a TTY (non-TTY warns + proceeds; `--force` skips). This
  catches the *accidental* re-run without pretending `new` is idempotent — and is
  the RFC's "dedupe before commit" idea wired into `new`.
- **New write paths must keep this true.** Don't introduce hidden persisted state
  (caches, lock files, a daemon) or a mutation that changes a file it didn't need
  to. The guard test `commands_are_idempotent` (in `tests/cli.rs`) runs the
  idempotent verbs twice and asserts the repo is byte-identical — keep it green.

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
- `src/links.rs` — cross-ADR relative-link parsing + rewriting (pure helpers)
- `tests/` — integration suites: `cli.rs` (binary + every regression), `model.rs`
  (model-based oracle over the format×layout×scheme×relink_scope matrix),
  `parsers.rs` + `fuzz_parsers.rs` (parser properties / bolero coverage-guided fuzz),
  `config_precedence.rs`, `date_source_git.rs`, `forge_faults.rs` + `forge_cli.rs`
  (`forge`, now in the default build). See the book's **Development → Testing & Fuzzing** page
  (`docs/src/dev/testing.md`); the campaign that built them is **Hardening & Quality**.
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

**The CLI emits the same JSON too (`-o`/`--output json`).** A global
`cli::OutputFormat` (`human` default, `json`) is honored by the read verbs —
`list`, `show`, `search`, `stats`, `graph`, `check` — which serialize the `view`
types via `serde_json` (a core dep) through the `print_json` helper in `main.rs`.
So an AI agent / script drives the CLI for the same shapes the web API returns
(`docs/src/usage/automation.md`). `check -o json` still exits non-zero on an
Error-severity problem (the CI gate). The destination flags on `publish` and
`review` are **`--out`** (long-only) so the short `-o` belongs to `--output`.
`stats` + `graph` are thin CLI verbs over `query::stats`/`query::graph` (added to
both `help_template`s — the `commands_are_all_grouped` guard). Note the five
on-disk *shape* globals (`--format/--layout/--naming/--date-source/--relink-scope`)
are **top-level-only** (env still binds); only `--dir` stays `global`.

**Help model.** `-h` and `--help` show the **same concise** help (command list +
`--dir`/`--output`); `--help-all` shows everything. Done with the canonical clap
recipe: `disable_help_flag = true` on the root, then custom **global** `help`
(`-h`/`--help`, `ArgAction::HelpShort`) + `help_all` (`--help-all`,
`ArgAction::HelpLong`) flags — `disable_help_flag` propagates to subcommands, so
the help flags are `global = true` to re-provide `-h`/`--help`/`--help-all`
everywhere. The repo-shape + command-default options carry `hide_short_help = true`
so they surface only under `--help-all`. (Do not re-add a built-in help flag.)

**Human output.** Colored via the `colored` crate (`status_color` /
`status_bar_color` / `edge_label` in `main.rs`); `main` calls
`colored::control::set_override(false)` when stdout isn't a terminal, so pipes /
`-o json` / `NO_COLOR` get plain text (the assert_cmd tests therefore see plain
output). `graph`'s human view is a **tree** (edges grouped under each source node,
`├─`/`└─` connectors, isolated ADRs as an `unconnected:` footnote); `stats` renders
the by-status breakdown + created-per-month as `print_bars` horizontal bar charts
(rnought/talaria `█`/`░` style). `-o json` output is never colored or charted.

**Repo validation is shared here too.** `query::check` runs the `adroit check`
rules (status/dir mismatch, duplicate identifiers, unparseable files, broken
supersession, broken/stale links, and a **duplicate-title** `Warning`) and returns
a structured `view::CheckReport` (`Problem` + `Severity` + `ProblemKind`). Both the
supersession and the cross-ADR-link checks are **scheme-aware** — they resolve a
link/ref to an ADR via the naming seam (`ref_in_link_from`), so date/uuid/
per_category links classify *stale* (ADR moved → warning) vs *broken* (no such ADR
→ error) correctly, not just numeric ones; supersession is validated in **both**
profiles (the markdown `## Status` note and the frontmatter YAML field). The CLI's
`cmd_check` renders that report — sorting `problem.message` so its output stays
**byte-identical** (the `check_*` integration tests are the guard) — and the web
`GET /api/check` endpoint serves it for the dashboard's repo-health panel.
`Stats.proposed_age` rows also carry a `review_due` flag so a surface can mark an
overdue Proposed ADR inline.

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

The source is configurable via `config.date_source` / `ADROIT_DATE_SOURCE` /
`--date-source` (`DateSource` enum on `StoreOptions`): `auto` (default — git when
available, silent filesystem fallback), `git` (strict — `query::open_history`
warns once via a process `AtomicBool` when the repo isn't git or is shallow, then
falls back), `filesystem` (skip git entirely — mtime/authored dates, no
timeline). `query::open_history` centralizes this; `load_resolved`/`detail` go
through it.

"Today" (for the `date` scheme's `YYYYMMDD-` slug and review-due math) comes from
`config::today_override()` then the system clock: a **test-only** `ADROIT_TODAY`
(ISO `YYYY-MM-DD`) env override pins it deterministically for tests/CI without
touching the clock; unset, behavior is unchanged. Distinct from `ADROIT_DATE_SOURCE`.

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
for the default ANSI theme, `into_text_with_theme` + `gruvbox_theme()`/
`warm_theme()` for the true-color themes); `render_preview` prepends the metadata
header and `m` toggles `TuiState::preview_raw` (rendered ↔ raw source). Themes are
`config::MarkdownTheme { Gruvbox (#[default]), Warm, Default }`, resolved from
`--theme` / `ADROIT_THEME` / `tui_theme` config (flag > env > config) and applied
via `TuiState::set_md_theme` (wired in `driver::run` after `TuiState::new`).
Fenced code blocks are **syntax-highlighted** via syntect: `render_markdown_body`
installs a `.with_code_block` hook (`highlight_code`) that resolves the language,
highlights through process-wide `OnceLock<SyntaxSet>`/`OnceLock<ThemeSet>` (the
embedded defaults; pure-Rust fancy-regex, no onig/C), and converts each syntect
style to a ratatui span (`to_ratatui_span` — fg + bold/italic/underline, bg
dropped). The syntect theme tracks the TUI theme (`syntect_theme_name`). We
hand-roll the span conversion because `syntect-tui` targets ratatui 0.29 and would
duplicate ratatui in the tree. The editor (`i`) always shows raw source.

**Whole-UI chrome.** The selected theme drives the entire interface, not just the
markdown body. `driver::chrome(theme) -> Chrome` centralizes the palette (accent,
muted, border, selection_bg, title) per theme; every render fn routes it — rounded
borders (`BorderType::Rounded`) that brighten to the accent on focus, a `▶ `
selection marker, themed titles. The layout is three rows: a top **breadcrumb**
status bar (`render_breadcrumb`: `adroit › <filter> › "<search>" · N ADRs · sort:…
· <theme>`), the list+preview body, and a two-line **footer** (`render_footer`) —
line 1 is the active input prompt (accent) or a transient status message colored
by severity (`toast_color`: failures red, else accent), line 2 the muted
context-aware key hints. `?` toggles a centered keybinding **help overlay**
(`render_help`, grouped by task; any key dismisses it — intercepted at the top of
`handle_key`). Empty panes show a friendly, context-aware hint rather than a blank
box: `empty_list_message(search, status)` (pure) distinguishes an empty repo
("No ADRs yet — press n") from a search/filter hiding everything, and the preview
shows a centered "No ADR selected" hint.

**Threaded reload + spinner.** The list query resolves git history per ADR, so on
a large repo it can take a moment. `apply_action` is now a **pure write step** that
returns an `Outcome { quit, reload: ReloadKind }` (`None`/`Preview`/`Full`) instead
of reloading inline; the **driver** decides how to refresh. A `Full` reload runs on
a worker thread (`spawn_reload` re-opens the store from `(config, dir)` — adroit is
stateless, so nothing mutable crosses the thread) and posts rows back over an
`mpsc` channel; the event loop polls the channel each frame, applies the rows +
one synchronous preview detail on arrival, and shows a `throbber-widgets-tui`
spinner (`render_spinner`, right of the breadcrumb) while `state.loading()`. A
`Preview` reload (selection moved via `Action::RefreshPreview`, or a body save)
just reloads the one detail synchronously — navigation never re-queries the list or
spawns a thread. `TuiState.loading` is a pure flag; the channel/thread/throbber
state live in the driver. The headless `apply_action` tests assert the write +
`ReloadKind` and call `reload` themselves to emulate the driver.

**Command palette (`:`).** A fuzzy command palette (`Mode::Palette { input,
index }`, opened with `:`) is the discoverable index of every TUI verb. The
commands are a single `PaletteCmd` enum + `PALETTE` const (title + key hint per
variant); adding a verb there surfaces it both by key and by name — the one place
to extend. Filtering is `fuzzy_rank(needle, items) -> Vec<usize>`, a pure helper
over **nucleo-matcher** (the helix/telescope engine; a `tui`-gated, terminal-free
dep — no extra ratatui) returning indices best-match-first (empty needle = all,
original order). `palette_confirm` runs the selected command via `run_palette_cmd`,
which mirrors the keybinding (mode-switch commands return `Action::None`; effectful
ones return `Action::Refresh`/`Quit`/`Edit`). The palette also exposes the theme
switchers, which have no direct key. `render_palette` is the centered overlay
(query line + matches with right-aligned hints). All of this lives in the pure,
headlessly-tested layer.

**AI assists (`Mode::AiPrompt`/`AiResult`).** The palette exposes five AI verbs
(`PaletteCmd::Ai*`): **draft/revise body** + **ask** open a free-form prompt
(`Mode::AiPrompt { input, kind }`), while **summarize**/**lint**/**plan** act on the
selected ADR directly. The pure layer builds an `Action::Ai(AiRequest)` (a
framework-free description carrying the selected ADR's address/title/body + any
instruction); the driver intercepts it like `Action::Edit`, runs it on a **worker
thread** (`spawn_ai`/`run_ai` re-open the provider + store from `(cfg, dir)` — `dyn
AiProvider` needn't be `Send`), and shows the "thinking" spinner. A `Draft` reply
loads the `AI_MARKER`-tagged body into the editor via `begin_edit_with` (opens in
**Normal** mode, `dirty` — `Ctrl-S` saves through `Store::set_body`, `Esc` warns);
a `Popup` reply (ask/summarize/lint/plan) shows scrollable read-only text
(`Mode::AiResult`, rendered as markdown). Only one AI call runs at a time; with no
provider the worker returns a clear "not configured" message. AI only ever writes
**prose** — identity/status stay mechanical.

**Fuzzy ADR pickers (`Mode::PickAdr`).** Two flows fuzzy-pick an ADR from the
list instead of typing an identifier, sharing one mode + overlay (`render_adr_picker`)
parameterized by `PickPurpose`: `Jump` (`Ctrl-P` / palette "Go to ADR…" → moves the
selection) and `Supersede` (`S` → choose the OLDER ADR; the selected "new" ADR is
excluded from candidates). Both reuse `fuzzy_rank` over `"<reference> <title>"`
labels and resolve back to a row index, then `pick_confirm` either re-selects the
row or emits `Action::Supersede { new, old }` (same `Store::supersede` write path).
Replaces the former type-the-id `Mode::Supersede`.

**Preview scrolling & mouse.** The preview is scrollable with a gutter scrollbar
(`Scrollbar`/`ScrollbarState`, shown only when content overflows). Because
`Paragraph::line_count` is private in ratatui 0.30, `render_preview` estimates the
wrapped height with `wrapped_line_count(text, width)` and reports it via
`TuiState::set_preview_metrics(lines, viewport)`, which clamps the offset (and
re-clamps when switching to a shorter ADR). Scroll keys (`handle_preview_key`):
`j`/`k` by line, `PageUp`/`PageDown` and `Ctrl-U`/`Ctrl-D` by viewport, `g`/`Home`
to top, `G`/`End` to bottom. The terminal enables mouse capture
(`EnableMouseCapture` in `setup`, `DisableMouseCapture` in `teardown`); the event
loop matches `Event::Mouse`, and `handle_mouse` wheel-scrolls the focused preview
or otherwise moves the list selection.

**In-TUI body editor (modal / vi).** Pressing `i` on the selected ADR enters
`Mode::Edit`, loading the body into an `EditorBuffer` — a pure multi-line
plain-text editor (`lines: Vec<String>` + char-based `cursor_row`/`cursor_col`).
Beyond the basics (`insert_char`/`insert_newline`/`backspace`/`move_*`/`home`/
`end`/`from_str`/`to_string`) it has vi operations: `delete_char` (x),
`delete_line` (dd), `open_below`/`open_above` (o/O), `move_word_forward`/`_back`
(w/b), `goto_first_line`/`goto_last_line` (gg/G). Still **no undo/selection/
clipboard** — that's the EditorBuffer's deliberate bar (the user chose to keep it
over swapping in a widget). The editor is **modal**: a pure `edit_insert: bool` on
`TuiState` is the Insert/Normal sub-mode (plus `edit_pending: Option<char>` for the
two-stroke `gg`/`dd`). It opens in **Insert** (matches vi's `i` + the prior
type-to-edit UX); `handle_edit_key` dispatches to `handle_edit_insert_key`
(type/Enter/Backspace/arrows/Tab; **Esc → Normal**) or `handle_edit_normal_key`
(`hjkl`/`w`/`b`/`0`/`$`/`gg`/`G` motions; `i`/`a`/`I`/`A`/`o`/`O` → Insert; `x`/`dd`;
**q/Esc → cancel**). **Ctrl-S** saves from either sub-mode; a dirty cancel needs a
`y`/Esc confirm; the title + footer show `INSERT`/`NORMAL` and `[modified]`. Save
goes through `Store::set_body`, which reads the ADR, replaces only `.body`, and
re-serializes via the existing `format::serialize` path — so frontmatter /
`## Status` / banner / status dir are untouched and an unedited round-trip is
byte-identical. `e` remains the external-`$EDITOR` escape hatch.

## Web dashboard (`web` feature)

`src/serve/` is a read-only Axum server gated behind the `web` Cargo feature;
`--no-default-features` and the `tui` feature never depend on axum/tokio/notify.
`adroit serve [--host --port]` exposes the shared `query` layer as a JSON API
(`/api/adrs`, `/api/adrs/{id}`, `/api/search`, `/api/stats`, `/api/graph`,
`/api/check`, plus `/api/workspace` + `/api/browse` for the in-browser directory
picker) and serves
an embedded Vue 3 SPA (`web/dist`, embedded via `rust-embed`). The store is
reopened per request, so every response reflects current on-disk state.
Markdown→HTML rendering is server-side (`pulldown-cmark`); `render_markdown`
post-processes the event stream to **autolink bare `http(s)://` URLs** (e.g. the
`## References` issue/PR links — CommonMark only autolinks `<url>`), skipping code
blocks and existing links. It is also the **XSS-sanitization seam**: pulldown-cmark
is not a sanitizer, so `render_markdown` escapes raw HTML events to visible text and
routes every link/image `dest_url` through `sanitize_href` (neutralizing
`javascript:` / `data:` / `vbscript:` → `#`) — a crafted ADR body can't run script
in the dashboard. No endpoint writes
ADRs — authoring stays in CLI/TUI; the one mutating route, `POST /api/workspace`,
only switches which directory the dashboard views (re-pointing the watcher), and
the ADR side imports only the read path.

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
  (`src/frontmatter.rs`). **Numeric-only** — its YAML persists a `number:`, so it
  pairs only with the `sequential` scheme. `main` bails up front with a clear
  message when `frontmatter` is combined with a slug scheme (`date`/`uuid`/
  `per_category`) rather than failing deep in the serializer.
- `layout = by_status` (default): ADRs grouped into `proposed/ accepted/
  rejected/ superseded/ deprecated/` subdirs; `README.md` and `adr-template.md`
  are skipped; `next_number` is the max across all subdirs + 1; status changes
  move the file. `layout = flat`: one directory (original behaviour).
  `layout = by_category`: each immediate subdirectory is a **category** (an
  area, not a status); status lives in the `## Status` section (the dir is the
  category, so `dir_status` is `None` and a status change rewrites **in place**,
  no move); numbering is **per category** (`Store::next_ref_in_category`), and
  ADRs are addressed by the `category/NNNN` composite. Pairs with the
  `per_category` naming scheme; `new` requires `--category`. `Adr.category` (set
  from the parent dir on read) carries the area. `migrate` to/from `by_category`
  is refused (categories/numbers can't be re-derived mechanically).

**Profile-mismatch guard + migrate.** `Store::detect_profile` infers the on-disk
layout/format from the files present (status-subdirs-with-numbered-`.md` ⇒
by_status, root-numbered-`.md` ⇒ flat; leading `---` ⇒ frontmatter); a stray
non-numbered `.md` doesn't count. `Store::profile_mismatch` compares that to the
configured opts, and `main.rs` **bails** (before dispatch) on any mismatch for
every command except `migrate` — otherwise a wrong `--layout`/`--format` would
silently hide ADRs (e.g. by_status read as flat lists nothing) or collide
numbers. `Store::migrate(apply)` is the conversion path (`adroit migrate`,
preview unless `--yes`; `--dry-run` forces preview): it reads through a
detected-source-profile `Store`,
moves files verbatim for a layout-only change or re-serializes via
`format::serialize` for a format change (filenames preserved; target collisions
refused), then `relink`s. `cmd_migrate` prints the plan / applies.

**`adroit config`** (`cmd_config`, handled in `main.rs` *before* the store is
opened, so it works on a mismatched repo) shows/gets/sets settings.
`Config::get_str`/`set_str` (in `config.rs`) are the typed key↔string accessors
(validate on set); `CONFIG_KEYS` is the key list, `env_var_for` maps a key to its
`ADROIT_*` var, and `upsert_env_file` writes `.env`. `config show` reports each
key's resolved value + source (flag / env / config / default), telling flag from
env by comparing the env var's value to what clap resolved. `set` writes
`config.yaml` (via `Config::save`) or, with `--local`, the project `.env`. Keys
include `relink_scope` (`all`/`self`/`none`, env `ADROIT_RELINK_SCOPE`, global
flag `--relink-scope`) — see "Cross-ADR link integrity" above.

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

### Cross-ADR link integrity (`src/links.rs` + `Store::relink`)

In by-status a status change moves the file between dirs, which would strand
relative links (`[..](../proposed/0009-x.md)`) in other ADRs and in the moved
file itself. `links::rewrite_links(content, source_dir, resolve)` is the pure
engine: it scans `](target)` spans, and for each *relative* `.md` target where
`resolve(target)` yields a path, rewrites it to the canonical relative path of
that ADR's current file (preserving `#anchors`, keeping `./` for same-dir);
external URLs / anchors / non-ADR links are left byte-for-byte. Resolving a link
target → ADR is the caller's job, so the engine is **scheme-agnostic**:
`Store::relink` builds a map keyed by each ADR's `reference()` (skipping
ambiguous duplicates) and resolves a target via `naming.ref_in_link(target)`, so
date/uuid slug links relink just like sequential numbers. It writes only files
that changed (idempotent → no-op on a canonical repo). `relink(apply)` with
`apply == false` is the dry-run path (`adroit relink --dry-run` reports
`changed_files` without writing).

**Relink scope on a status move (`relink_scope`).** After a move, `set_status_at`
reconciles links via `relink_after_move`, dispatching on the
`config::RelinkScope` carried on `StoreOptions`: `all` (default) calls
`relink(true)` to heal every inbound link (`adroit set-status`/`supersede`
self-heal the whole repo — best for a single author); `self` calls `relink_one(&new_path)`
to fix only the moved file's own outbound links (`relink_one` reuses the
`link_resolver_map` + `links::rewrite_links` on that one file), leaving neighbors'
inbound links for a later relink; `none` does nothing. `self`/`none` make a
status-change PR touch only the ADR it is about, so concurrent decision PRs never
collide on shared neighbors — the inbound links are then canonicalized by a
post-merge `adroit relink` on `main` (the "heal-on-main" / propose-on-branch
workflow; see `templates/ci/` and the "Concurrent contributors" manual page).
**The explicit `adroit relink` command, `renumber`, and `migrate` are always
full-scope** — only `set_status_at` consults `relink_scope`.

`adroit relink` exposes the full relink on demand (repairs repos edited outside
adroit, or runs as the post-merge bot); `cmd_check` adds check #5 (broken target
/ stale-vs-canonical). The broken-vs-stale split is **identity-based**: a link
whose target file is missing but which names an ADR that still exists elsewhere
is a **stale** link (`Severity::Warning` — `adroit relink` heals it), while a
link naming no existing ADR is **broken** (`Severity::Error`). `cmd_check` exits
non-zero only when an `Error`-severity problem is present (duplicate number,
broken link, status/dir mismatch, unparseable, broken supersession); a
warning-only report (e.g. a deferred-relink PR's transiently stale inbound links)
prints `OK: N ADRs, M warning(s)` and exits 0. `query.rs` resolves graph link
targets through the naming seam (`scheme.ref_in_link`), so the graph is
scheme-agnostic; `links::number_in_target` remains as a fuzz/parser-test target.

`Store::renumber` (`adroit renumber <old> <new> [--file]`) resolves a duplicate
number: rename the file, rewrite its heading + self-refs, then
`links::relabel_links_to` retargets+relabels inbound `[ADR-old](…)` links
matched by **basename** (so a same-number/different-slug sibling is untouched),
then `relink`. `--file` disambiguates when `old` has two files. In the
**frontmatter** profile supersession/typed-link refs are bare numbers in the
YAML block (not markdown links), so renumber also remaps those through the model
(`frontmatter::remap_numeric_refs`) — otherwise an inbound `superseded_by: <old>`
would be stranded; `check`'s frontmatter-supersession rule is the backstop.

The naming/identity **seam** (`src/naming.rs`) — `AdrRef` + `NamingScheme`
(`sequential`/`date`/`uuid`/`per_category`) + `Scope` — owns all scheme behavior
(`assign`/`parse`/`parse_ref`/`filename`/`display`/`heading`/`link_label`/
`ref_in_link`/`ref_in_link_from`/`ref_matches`/`scope`), so adding a scheme edits
only that module. (`ref_in_link_from(target, source_category)` is the
category-aware variant: a per_category same-category link like `./0002-x.md`
carries no category segment, so it's resolved relative to the source file's
category; a pass-through for every other scheme. `relink` and `check` route
through it.)
Consumers route through it: `Store::write`/`read` assign+parse identity via the
scheme and name files with `scheme.filename`; `next_ref`/`find_path_by_ref` and
the `set_*_ref` mutation methods address ADRs by `AdrRef`; `template::render`
fills `{{heading}}` from `scheme.heading`; `query::summary_of` fills
`AdrSummary.reference` from `scheme.display`; the CLI's `show`/`status`/
`set-status`/`edit`/`set-review` take an `<ID>` parsed by `scheme.parse_ref`; `check`'s duplicate
detection groups by scheme identity. **Sequential stays byte-identical** — the
additive identity model (`Adr.number: Option<Number>` + `Adr.slug:
Option<String>`, `Adr::reference()`) keeps it the no-op path, and the existing
unit + integration tests are the regression guard. `date` + `uuid` work
end-to-end (create/list/show/status/set-status/set-review/supersede/check) — supersession
is a scheme-agnostic `Option<AdrRef>` (serde-untagged in frontmatter; resolved
from the markdown link via `ref_in_note`), and the graph/related/view layer
carries scheme `reference` + `address` strings so the TUI and web SPA route
every ADR (incl. slug/uuid) by `address`. `renumber`/`review` are numeric-only
(a single global number — `sequential`) and bail otherwise. `per_category` is
wired via the `by_category` layout (per-category local numbering, `category/NNNN`
identity, status in-content).

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

## Forge integration (`forge` feature)

Opt-in adapters that drive the *process* lifecycle (a tracker issue + a
code-review PR/MR) alongside the ADR's *decision* lifecycle (issue #4). Gated
behind the `forge` Cargo feature, which adds a small **blocking** HTTP client
(`ureq`) — the core CLI stays synchronous; `--no-default-features`/`tui`/`web`
never depend on `forge`.

**Two roles, trait objects.** `src/forge/mod.rs` defines `Forge` (PR/MR host)
and `Tracker` (issue host). A *same-system* provider implements both over one
client + one token: `src/forge/github.rs` (`Github` is both, behind `Arc`,
`ADROIT_GITHUB_TOKEN`); GitLab (`Provider::Gitlab`) is the same shape (Phase
1b). A *split* setup (GitLab MRs + Jira issues) is reached later via the separate
`tracker` selection. `src/forge/noop.rs` is the null-object adapter for previews.

**Clean dispatch (two axes).** Compile-time: the `#[cfg(feature="forge")]` lives
on the `mod` line in `lib.rs`, on the `src/forge_hook.rs` facade (twin defs: real
when on, no-op when off), and on the **forge CLI surface** in `src/cli.rs` — the
`--forge`/`--dry-run`/`--yes` opt-in flags on shared verbs *and* the forge-only
commands (`init`/`auth`/`sync`/`notify`), so a no-forge build doesn't expose them
at all (no misleading flags or `unrecognized subcommand` no-ops; `publish` stays
— it's offline). The verb *handlers* (`cmd_new`/`cmd_set_status`/`cmd_supersede`)
still call `forge_hook::*` unconditionally (no-op twins) — they carry no
`#[cfg]`; `main`'s dispatch builds the `ForgeFlags` from the gated fields with a
small `#[cfg]` (`ForgeFlags::default()` when forge is off). The top-level
`help_template` is `cfg_attr`-selected so the no-forge build's `--help` omits the
"Forge integration" section. Runtime: `forge::open(&ForgeConfig)` is a **thin
dispatcher** (`match Provider` → `github::open(cfg)`); each provider module owns
its own construction (token env var, slug/host). Adding a provider = one match
arm + one module. HTTP is behind the `HttpTransport` seam so adapters are tested
with a `FakeTransport` (no network).

**Verbs wired** (all opt-in via `--forge`, with migrate-style `--dry-run`/
`--yes`; graceful-offline = warn + keep the local write; the ADR is the durable
record): `new` creates the issue + a draft PR off an `adr/NNNN-…` branch
(`src/git.rs` does branch/commit/push) and records both URLs in a
format-preserving `## References` section (`format::upsert_reference`);
`set-status accepted` verifies `review_quorum` approvals + CI then merges the PR
+ closes the issue (refuses if blocked; the whole op previews unless `--yes`),
and then **"pushes the relink commit"** (#4): `before_status_change` fast-forwards
the base branch to the merge, the local move relocates `proposed/ → accepted/` +
relinks on it, and `after_status_change` (in `forge_hook`) commits + pushes that —
so `accepted/` lands on `main` in one command. Best-effort: a dirty tree /
diverged base / rejected push restores the branch and leaves the move local with
a warning (`git.rs` gained `fetch`/`merge_ff_only`/`is_clean`/`toplevel`).
`set-status rejected`/`deprecated` close the PR + mark the issue won't-fix (no
relink commit — those don't merge);
`supersede` comments on + closes the old ADR's issue/PR. Each orchestration is
split into a testable core (`run_new`/`run_status_change`/`comment`) exercised
with mock/noop adapters. Read-side: `check --forge` appends
`ProblemKind::ForgeIntegration` drift warnings; `list --forge` enriches rows
(`forge::enrich` → `AdrSummary.forge_data`); `review`/`set-review --forge`
post a comment (kickoff / deadline) via the shared `forge::comment`.

**Providers.** `github` + `gitlab` (each a same-system Forge+Tracker via
`{github,gitlab}::open(cfg)`); `jira` is a split **Tracker** (`forge/jira.rs`,
REST v2) selected by `forge.tracker = jira` so a GitHub/GitLab forge pairs with
Jira issues — `forge::open` chooses forge and tracker independently. Jira auth
follows the deployment: Cloud uses Basic `email:token` (email set), Server/Data
Center uses a Bearer PAT (email omitted). GitHub/GitLab use the same token cloud
or self-hosted; only `forge.host` changes (GitHub Enterprise host includes the
`/api/v3` base).

**Cross-cutting verbs.** `adroit init` (interactive wizard: detect/confirm
provider+repo from the git remote → `config::parse_remote_url`, pick the tracker,
write `forge.*`, then optionally `./.env` (ADROIT_DIR), a repo-local
`adr-template.md`, and a pre-commit hook running `adroit check`; `--yes` runs it
non-interactively, `--print` previews), `adroit publish` (export accepted
ADRs to a dir — `src/publish.rs`, core/offline; Confluence/Notion adapters are
future), `adroit notify <id>` (POST to a Slack/Teams webhook via
`forge::notify`), and `adroit auth <github|gitlab|jira> [--token] [--email]`
(save a token to a dependency-free 0600 `credentials.yaml` next to the config —
`config::store_credential`/`load_credential`; `{github,gitlab,jira}::open`
resolve the token env → credential store → none). `adroit reconcile` syncs local
status to the forge after out-of-band changes (a merged MR / closed issue):
reports drift, and with `--yes` moves a merged PR's ADR to `accepted/`
(read-only on the forge; `forge::run_reconcile` is the testable core).
**Read-only dashboard.** Two forge-aware routes, both read-only and `null`/empty
without an active forge (built on always-compiled `forge_hook::*` twins so a
`web`-only build degrades cleanly): `GET /api/adrs/{id}/forge` (per-ADR, via
`enrich_one`) feeds `DetailView.vue`'s issue/PR panel; `GET /api/forge/summary`
(via `dashboard_summary`, with `AppState.review_quorum`) feeds the dashboard
tiles "Proposed without an MR" (local) + "MR approved · waiting on author"
(live). The dashboard never *writes* to the forge (the one-click "create MR"
button remains out of scope). **Still future:** OAuth device-flow + OS-keychain
credential storage, and Confluence/Notion `publish` adapters.

**Forge config is repo-scoped, not just global.** `forge.*` is one (global)
config, but the dashboard can switch ADR directories at runtime and the CLI runs
anywhere — so the active dir may belong to a *different* repo than `forge.repo`.
`dir_matches_forge(fcfg, dir)` compares the dir's `origin` remote to `forge.repo`;
a definite mismatch (different provider/slug) means the config doesn't apply here.
**Every** forge entry point guards on it — both reads (`enrich_with`,
`dashboard_summary`, `reconcile`, `check_repo`) and writes (`after_new`,
`before_status_change`, `on_supersede`, `comment`, `sync_pr`) call
`skip_dir_mismatch` (dir) / `skip_path_mismatch` (file → its dir) right after the
`cfg.forge` check, before `open(fcfg)`. On mismatch they warn once and skip the
forge side: the dashboard hides its cells, `list`/`check --forge` omit
enrichment, and the mutating verbs keep the local ADR record while creating /
merging *nothing* in the wrong repo. Undeterminable cases (no `repo` set, or no
recognizable remote) assume it applies — non-git ADR dirs aren't blocked.
`DetailView.vue` re-fetches its forge panel on `workspaceChanged` (not on every
live-reload tick).

**Config.** `config::ForgeConfig` (`Provider`, `repo`, `host`, `branch_prefix`,
`base_branch`, `tracker: TrackerProvider`) under `Config.forge`; tokens are
env-only (`#[serde(skip)]`). Scalar `forge.*` keys go through the usual
`get_str`/`set_str`/`CONFIG_KEYS`. `just lint-forge`/`test-forge` (folded into
`just ci`) cover the feature build.

## AI authoring (`ai` feature)

Opt-in AI-assisted authoring (RFC: issue 5; built on the `rig` framework). Same
shape as `forge`: a **synchronous** `AiProvider`
trait so verb handlers stay sync, with the async work bridged by a single
`block_on` at the CLI boundary — so `--no-default-features`/`tui`/`forge` never
pull in tokio.

**Always compiled** (`src/ai/mod.rs`, `src/ai_hook.rs`): the `AiProvider` trait,
`CompletionRequest`/`AiError` value types, the Socratic `Interview` +
`build_request`/`draft_body` logic, the `AI_MARKER`, and the `FakeProvider`
stand-in. So the interview flow is unit-testable with **no network and no `ai`
feature**. `ai_hook::open_provider(cfg)` is the facade (mirrors `forge_hook`):
it returns a `Box<dyn AiProvider>` or `None`, resolving in order — the
`ADROIT_AI_FAKE` test seam (offline echo) → the configured rig provider (the `ai`
feature is on by default; a provider is only built when `ai.enabled`, and never in
a `--no-default-features` core build) → `None`.

**`ai`-gated** (`src/ai/rig_provider.rs`): `RigProvider` wraps rig (aliased from
`rig-core` so `use rig::…` works) — Anthropic (`Client::builder().api_key(k)`) and
Ollama (`Client::new(Nothing)`, local) — holding a current-thread tokio runtime
and `block_on`-ing rig's async `agent(model).preamble(system).prompt(...)`.

**`new --interview`** (`run_interview` in `main.rs`): asks the fixed
`INTERVIEW_QUESTIONS` over **plain stdin** (robust on a non-TTY / piped test
input; prompts go to stderr), builds a corpus summary from `query::summaries`,
drafts via the provider, then **splices**: it keeps every line before the first
`## Context…` (the mechanical heading / `## Status` / stakeholders) and replaces
the prose with the marked draft, written through `Store::set_body_ref`. AI only
ever writes prose — identity/status/dates/links stay mechanical. Degrades to the
plain template when no provider is available, so the ADR is always created.

**`draft <ID>`** (`cmd_draft`): the **after-the-fact `new --interview`** — runs the
*same* interview on an existing ADR (you created it with a plain `new`), then opens
the editor. `new --interview` and `draft` share `interview_and_draft` (the Q&A →
`ai::draft_body` → `splice_ai_draft`); `run_interview` is just the `new`-side
provider-resolution that degrades to the template, whereas `draft` uses
`require_provider` (errors — the ADR already exists, no fallback). Iterative flow:
`new` → `draft` → `edit` → PR.

**`plan <ID>`** (`cmd_plan`, `ai::build_plan_request`/`draft_plan`): the
**read-only** companion — reads an ADR (`query::detail_at`) + corpus, asks the
provider for an ordered implementation checklist, prints it (or `--out`). Never
modifies the ADR; bails (not degrades) when no provider is available, since a
plan is inherently AI.

**`summarize <ID>`** (`cmd_summarize`, `ai::build_summary_request`/`draft_summary`):
a one-paragraph read-only TL;DR of an ADR (PR body / notify / decision log); prints
to stdout or `--out`; bails with no provider.

**`lint <ID>`** (`cmd_lint`, `src/lint.rs`): authoring-quality checks on one ADR,
**distinct from `check`** (structural repo validity). `lint::lint(body)` is the
deterministic core — leftover MADR placeholders, missing/empty
`### Negative Consequences`, `## Considered Options` with <2 items — returning
`Vec<LintFinding>` (`LintSource::Mechanical`/`Ai`, serde). It needs **no AI**, so
it's CI-usable; `--ai` appends one advisory finding from `ai::draft_lint`. Exits
non-zero on **mechanical** findings only (AI is advisory); `-o json` emits the
findings.

**`related <ID>` / `dedupe <ID>`** (`cmd_related`, `src/similar.rs`) are
retrieval verbs but **mechanical — NO AI/provider**: TF-IDF cosine over the corpus
(title + body). `related` excludes ADRs already linked to the target (link
candidates); `dedupe` includes them (duplicate-catching). Read-only; `-o json`.

**`ask "<q>"`** (`cmd_ask`, `ai::build_ask_request`/`draft_ask`) combines the two
halves: **mechanical retrieval** (reuse `similar::rank` with the question as a
transient target doc, via the shared `corpus_docs` helper) feeds the top ADR
excerpts to the **provider**, which answers with citations. Human output = answer
on stdout + `(sources: …)` on stderr; `-o json` = `{answer, sources}`. Bails with
no provider. The **embeddings** upgrade to similarity/retrieval is future work —
Anthropic has no embeddings API, so it needs a separate embedding-capable provider
+ a cache.

**`compose`** (`cmd_compose`; `ai::build_compose_request`/`draft_compose`, templates
`templates/ai/compose.{system,prompt}.md`): instruction-driven (re)drafting — given
an ADR's current body + a **free-form instruction** + corpus, the model returns a
complete revised body (prose only, marked with `AI_MARKER`). The **targeted,
iterative cousin of `draft`**: `draft` re-runs the fixed interview and redrafts
wholesale, whereas `compose` applies a specific instruction ("expand the negative
consequences") to the *current* body. `adroit compose <ID> "<instruction>"` splices
it over the prose via the shared `splice_ai_draft` (heading/status stay mechanical)
and opens the editor (`--no-edit` to skip); it's also the engine behind the TUI's
"AI: draft / revise body" assist. Requires a provider (no template fallback — the
ADR already exists). Its write path is `splice_ai_draft` → `set_body_ref`, identical
to `draft`'s, so the oracle's `Draft` op already covers the body-write invariants.

**TUI AI assists.** The TUI surfaces the AI verbs via the command palette
(`:`) — see the "Interactive TUI" section. They reuse `ai_hook::open_provider`
and the `ai::build_*`/`draft_*` cores, run on a worker thread (the `render_spinner`
"thinking" state), and degrade with a clear message when no provider is configured.

**Config.** `config::AiConfig` (`provider: AiProviderKind` anthropic/ollama,
`model`, `enabled` kill-switch, `host`) under `Config.ai` (`Option`, absent by
default). `config::resolve_ai(cfg.ai)` overlays `ADROIT_AI_*` env overrides
(`ENABLED`/`PROVIDER`/`MODEL`/`HOST`) on the config section, so AI is enablable via
env / `.env` with no `config.yaml` edit (what `ai_hook::open_provider` calls). The
key is env-only (`config::anthropic_key()` → `ADROIT_ANTHROPIC_KEY` / the
credential store). `serde_json` is a core dep; `rig`+`tokio` are `ai`-only.
`just lint-ai`/`test-ai` (folded into `just ci`) cover the feature build.

## Design principles & conventions (SOLID / DRY / Rust)

Rules a change must preserve. These aren't aspirational — the codebase already
follows them (an audit against the conventions we share with `~/repos/talaria`
found them applied consistently). Keep it that way.

### Search before adding (DRY)
Before adding a function, error variant, or helper, **grep for an existing one** —
recurring concerns have a single owner; reuse it rather than re-deriving:
- ADR identity / filenames / link refs → the **naming seam** (`naming.rs`); never
  hand-parse a ref — call `scheme.parse`/`ref_in_link`/`ref_matches`.
- relative links → `links::rel_link` (one common-prefix walk; don't write a second).
- config → store options → `StoreOptions::from_config` (the one place every surface
  maps config to a store; a new option is wired *once*).
- reading/deriving ADR data → the `query`/`view` layer (computed once; CLI, TUI,
  and web consume identical results).

A duplicated algorithm is a future-divergence bug — extract instead of copying.

### Simplicity first
Prefer the simpler solution; remove old paths rather than keep parallel versions;
don't add backwards-compat shims unless asked. (See the statelessness/idempotency
invariant above — *converge, don't accumulate*.)

### Lib/bin & error layering
- All logic lives in the **library crate**; `main.rs` is argument-marshalling +
  human-rendering glue. New reusable logic goes in a lib module, not `main.rs`.
- **Typed errors (`thiserror`) in the data/parse layer; `anyhow` in the binary and
  the thin orchestration/surface layers.** `adr`/`format`/`frontmatter`/`store`/
  `query`/`naming`/`links`/`config`/`template`/`git` expose `thiserror` enums that
  compose with `#[from]` / `#[error(transparent)]`. **Never stringify a typed
  cause** (`map_err(|e| E::X(e.to_string()))`) — add a `#[from]` variant. `main.rs`
  and the feature-gated surfaces (`serve`, `tui`) + forge orchestration may use
  `anyhow` (they warn-and-continue across git+HTTP+fs); the pure parsers stay
  `anyhow`-free.

### Seams are enums/traits with one owner (Open/Closed)
adroit is extended by **adding a variant to a seam**, not editing call sites. The
gold standard is the **naming seam**: every scheme behavior is a method on
`NamingScheme`/`AdrRef`, so adding a scheme edits one module.
- Behavior that varies by an enum (`Format`, `Layout`, `NamingScheme`) belongs as a
  **named method/predicate on that enum** (e.g. `NamingScheme::is_numeric`/`scope`),
  not an ad-hoc `match`/`== Variant` scattered across files. Adding a third
  `== Format::X` site is the signal to lift it onto `Format`.
- A new pluggable backend (forge/AI provider, publish target, tracker) is a **trait
  impl + one factory arm** (`forge::open`, `ai_hook::open_provider`).

### Trait design (capability, focused, colocated)
- A trait names a **capability** (verb/adjective) — `AiProvider`, `Tracker`,
  `HttpTransport` — not a data structure (`Manager`/`Database`).
- Keep traits **focused** (interface segregation), and never give a type a trait
  impl it can't honor: Jira is a `Tracker`, not a `Forge`, so it carries **no**
  panicking `Forge` impl (that breaks Liskov). The `(Option<dyn Forge>, Option<dyn
  Tracker>)` pair keeps the roles independent.
- **Colocate** a trait with its primary impl (no catch-all `src/traits/`).
- **Prefer generics over `dyn`** unless dispatch is genuinely runtime-selected.
  `dyn` is load-bearing at the forge factory (adapter chosen from config; roles can
  differ) and the `HttpTransport` seam (injected fake vs real) — elsewhere prefer
  `impl Trait`/generics.

### Dependency inversion & feature confinement
Depend on the trait/facade, not the concrete. The **hook facades** (`forge_hook`,
`ai_hook`) are always compiled, so verb handlers call them unconditionally and
carry **no `#[cfg]`**. A feature's `#[cfg(feature = …)]` is confined to three
places: the `mod` line in `lib.rs`, the facade's twin (real / no-op) defs, and the
CLI surface. Don't sprinkle `#[cfg(feature)]` through verb logic.

### Pure core, effectful shell
Transform logic is **pure, terminal/network/git-free functions**, unit-tested
headlessly: `format::*`, `links::*`, `naming::*`, `lint::lint`, `similar::rank`,
`template::*`, `history::parse_log`, the TUI `apply_action` layer, the forge
orchestration cores. Effects (fs/git/http) live in the shell (`Store`, `main`,
forge orchestration). Push the decidable part into a pure function; keep the I/O
thin around it.

### Test / production separation (hard rule)
**Never** put test-only state in a production type — no `Test` enum variant, no
`is_test`/`test_mode` field, no `if cfg!(test)` branch in production logic. adroit's
seams are the right way: documented **runtime env overrides** usable in production
(`ADROIT_AI_FAKE`, `ADROIT_TODAY`), `#[cfg(test)]` helpers (`Store::next_number`),
and injected fakes (`FakeProvider`, `FakeTransport`). Fixtures and fakes, never
production conditionals.

### Rust idioms
- **Newtypes** for domain ids (`AdrId`/`Number`/`Created`/`ReviewBy`/`AdrRef`) — no
  primitive obsession. `strum` for enum `Display`/`FromStr` (`ascii_case_insensitive`
  kept in sync with serde `rename_all`).
- `Cow<str>` where a transform is usually a no-op (`format::normalize_lone_cr`);
  borrow over `.clone()`, `&str` over `String` in signatures.
- **Document genuinely-infallible `expect`s**; never a silent `unwrap` on a fallible
  runtime path — degrade gracefully (`Option`/`Result`), as `git`/`history` do.
- Internal API is `pub(crate)`; **test-only** items are `#[cfg(test)]` so the binary
  doesn't carry them. Slice strings on **char** boundaries (`.chars()`), not bytes
  (a real fuzz-found bug in `naming::display`).
