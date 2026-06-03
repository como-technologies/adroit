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
- `src/links.rs` — cross-ADR relative-link parsing + rewriting (pure helpers)
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

**Repo validation is shared here too.** `query::check` runs the five `adroit
check` rules (status/dir mismatch, duplicate identifiers, unparseable files,
broken supersession, broken/stale links) and returns a structured
`view::CheckReport` (`Problem` + `Severity` + `ProblemKind`). The CLI's
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
(`/api/adrs`, `/api/adrs/{id}`, `/api/search`, `/api/stats`, `/api/graph`,
`/api/check`, plus `/api/workspace` + `/api/browse` for the in-browser directory
picker) and serves
an embedded Vue 3 SPA (`web/dist`, embedded via `rust-embed`). The store is
reopened per request, so every response reflects current on-disk state.
Markdown→HTML rendering is server-side (`pulldown-cmark`). No endpoint writes
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
  (`src/frontmatter.rs`).
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
workflow; see `ci-templates/` and the "Concurrent contributors" manual page).
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
prints `OK: N ADRs, M warning(s)` and exits 0. `query.rs` reuses
`links::number_in_target` for its (numeric) graph link parsing.

`Store::renumber` (`adroit renumber <old> <new> [--file]`) resolves a duplicate
number: rename the file, rewrite its heading + self-refs, then
`links::relabel_links_to` retargets+relabels inbound `[ADR-old](…)` links
matched by **basename** (so a same-number/different-slug sibling is untouched),
then `relink`. `--file` disambiguates when `old` has two files.

The naming/identity **seam** (`src/naming.rs`) — `AdrRef` + `NamingScheme`
(`sequential`/`date`/`uuid`/`per_category`) + `Scope` — owns all scheme behavior
(`assign`/`parse`/`parse_ref`/`filename`/`display`/`heading`/`link_label`/
`ref_in_link`/`ref_matches`/`scope`), so adding a scheme edits only that module.
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
+ closes the issue (refuses if blocked; the whole op previews unless `--yes`);
`set-status rejected`/`deprecated` close the PR + mark the issue won't-fix;
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

**Cross-cutting verbs.** `adroit init` (detect provider from the git remote →
`config::parse_remote_url` → write `forge.*`), `adroit publish` (export accepted
ADRs to a dir — `src/publish.rs`, core/offline; Confluence/Notion adapters are
future), `adroit notify <id>` (POST to a Slack/Teams webhook via
`forge::notify`), and `adroit auth <github|gitlab|jira> [--token] [--email]`
(save a token to a dependency-free 0600 `credentials.yaml` next to the config —
`config::store_credential`/`load_credential`; `{github,gitlab,jira}::open`
resolve the token env → credential store → none). **Read-only dashboard panel.**
`serve` exposes `GET /api/adrs/{id}/forge` (the only forge-aware route; built on
the always-compiled `forge_hook::enrich_one` so a `web`-only build returns JSON
`null`), which `DetailView.vue` fetches on demand to show the linked issue/PR +
PR approvals/CI/merged — the dashboard never *writes* to the forge. **Still
future:** OAuth device-flow + OS-keychain credential storage, and
Confluence/Notion `publish` adapters.

**Config.** `config::ForgeConfig` (`Provider`, `repo`, `host`, `branch_prefix`,
`base_branch`, `tracker: TrackerProvider`) under `Config.forge`; tokens are
env-only (`#[serde(skip)]`). Scalar `forge.*` keys go through the usual
`get_str`/`set_str`/`CONFIG_KEYS`. `just lint-forge`/`test-forge` (folded into
`just ci`) cover the feature build.

## Conventions

- Lib/bin separation: all logic in the library crate, `main.rs` is glue only
- Use `thiserror` for library error types, `anyhow` for the binary
- Use `strum` for enum Display derives
- Use newtypes for domain identifiers (e.g. `AdrId`, `Number`, `Created`)
