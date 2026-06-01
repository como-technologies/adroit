# CLI Reference

## Global options

| Flag | Default | Description |
|---|---|---|
| `--dir <PATH>` / `-d` | `~/.local/share/adroit/` | Path to the ADR directory (env: `ADROIT_DIR`; overrides config) |
| `--format <markdown\|frontmatter>` | `markdown` | On-disk format profile (env: `ADROIT_FORMAT`; overrides config) |
| `--layout <by_status\|flat>` | `by_status` | Directory layout (env: `ADROIT_LAYOUT`; overrides config) |
| `--theme <default\|gruvbox>` | `default` | TUI markdown-preview color theme (env: `ADROIT_THEME`; overrides config) |
| `--review-overdue-days <N>` | `30` | Days after which a Proposed ADR with no `review_by` is flagged review-due; `0` disables (env: `ADROIT_REVIEW_OVERDUE_DAYS`; overrides config) |
| `--default-template <name\|path>` | `madr` | Default template for `new` — `madr`/`nygard` or a path (env: `ADROIT_TEMPLATE`; overrides config; `new --template` still wins) |
| `--date-source <auto\|git\|filesystem>` | `auto` | Where ADR dates/lifecycle come from: `auto` (git when available, else filesystem), `git` (require git; warn if unavailable/shallow), `filesystem` (never shell git) (env: `ADROIT_DATE_SOURCE`; overrides config) |
| `--version` | | Print version information |
| `--help` | | Print help |

All three flags are **global** — they work before *or* after the subcommand
(`adroit --dir X list` and `adroit list --dir X` are equivalent).

Each also reads from an environment variable, so you don't have to pass it on
every command: `ADROIT_DIR`, `ADROIT_FORMAT`, `ADROIT_LAYOUT`, `ADROIT_THEME`,
`ADROIT_REVIEW_OVERDUE_DAYS`, `ADROIT_TEMPLATE`, `ADROIT_DATE_SOURCE` (and, for
the web dashboard, `ADROIT_HOST` / `ADROIT_PORT`). A `.env` file in the current
directory (or a parent) is loaded automatically at startup, so you can keep your
repo location there. Copy the tracked `.env.example` to get started (your local
`.env` is git-ignored):

```sh
cp .env.example .env
# .env
ADROIT_DIR=/path/to/your-repo/src/adrs
```

Precedence for the ADR directory, highest first: the `--dir` flag, then the
`ADROIT_DIR` environment variable (a real shell variable wins over one from a
`.env` file), then a `dir` entry in `~/.config/adroit/config.yaml`, then the
default `~/.local/share/adroit/`.

adroit defaults to the **markdown / by-status** profile (status encoded by directory). See [ADR Format](./adr-format.md) for details on both profiles.

**Profile must match the repo.** adroit infers a repo's actual layout/format
from disk; if that disagrees with your configured `layout`/`format`, every
command **refuses to run** with a message (rather than silently hiding ADRs or
corrupting numbering). Either set `--layout`/`--format` (or config/`.env`) to
match, or run [`adroit migrate`](#adroit-migrate-yes) to convert the repo.

## Commands

### `adroit new <TITLE>`

Create a new ADR with the given title. The ADR directory is created automatically if it doesn't exist. In markdown mode the file is scaffolded from a template and written into the `proposed/` directory, then opened in your editor.

```sh
adroit new "Use PostgreSQL for primary datastore"
adroit new "Use Redis" --template nygard   # pick a template by name or path
adroit new "Use Redis" --no-edit           # skip opening the editor
```

| Flag | Description |
|---|---|
| `--template <name\|path>` | Template to scaffold from (`madr`, `nygard`, or a file path) |
| `--no-edit` | Do not open the editor after creating the ADR |

### `adroit list [--status <STATUS>]`

List ADRs as a table showing number, status, and title. Recurses into all status directories in by-status mode. Pass `--status` to filter.

```sh
adroit list
adroit list --status accepted
```

### `adroit show <NUMBER>`

Display a single ADR by its sequential number: its status, creation and
last-modified dates, supersession links, path, body, and — when the repo is a
git repository — a **History** timeline of its lifecycle (proposed → accepted /
rejected / superseded), with the date and commit subject of each transition.

```sh
adroit show 1
```

Dates and the timeline are read from **git history**, not the file: the first
commit that added the ADR is its creation, and each status change is a directory
move git records. Outside a git repository adroit falls back to the file's
modification time, and the timeline is omitted. See
[ADR Format](./adr-format.md#dates-come-from-git).

### `adroit status <NUMBER> <STATUS>`

Update the lifecycle status of an ADR. Status names are case-insensitive. In by-status markdown mode this **moves the file** to the matching status directory and rewrites the `## Status` section (minimal-diff).

Valid statuses: `proposed`, `accepted`, `rejected`, `deprecated`, `superseded`.

```sh
adroit status 1 accepted
```

### `adroit supersede <NEW> <OLD>`

Mark `<OLD>` as superseded by `<NEW>` in one command: moves the old ADR to `superseded/`, writes `Superseded by [ADR-<NEW>](...)` into its status, and adds a reciprocal `Supersedes [ADR-<OLD>](...)` note to the new ADR.

```sh
adroit supersede 6 2   # ADR-0006 supersedes ADR-0002
```

### `adroit set-review <NUMBER> <DATE>`

Set (or clear) an ADR's **review deadline** as an ISO-8601 `YYYY-MM-DD` date. A
still-`Proposed` ADR whose deadline has passed is flagged **review-due** in
`stats` and the web dashboard's "Review due" panel.

In markdown mode this writes a `Review by: <date>` line into the `## Status`
region (format-preserving — only that line changes). In frontmatter mode it sets
the optional `review_by` field. Pass `--clear` to remove the deadline.

```sh
adroit set-review 3 2026-07-15   # propose a review by July 15
adroit set-review 3 --clear      # remove the deadline
```

| Flag | Description |
|---|---|
| `--clear` | Remove the review deadline instead of setting one |

### `adroit search <TERM>`

Case-insensitive search across ADR titles and bodies (recursive). Prints number, status, and title for each match.

```sh
adroit search postgres
```

### `adroit check`

Validate the ADR repo and **exit non-zero if any problem is found** — a
structural CI gate. Problems are listed on stderr; a clean repo prints
`OK: N ADRs, no problems` and exits 0.

It checks for:

1. **Status ↔ directory mismatch** (by-status only): a file's `## Status`
   section declares a status that disagrees with the directory it lives in. A
   section with no explicit status word is fine (the directory is the source of
   truth).
2. **Duplicate numbers**: two ADR files sharing the same `NNNN`.
3. **Unparseable files**: a `.md` ADR with no `# ADR-NNNN: Title` heading.
4. **Broken supersession links**: a `Supersedes ADR-NNNN` / `Superseded by
   ADR-NNNN` note referencing a number that doesn't exist in the repo.
5. **Broken / stale cross-ADR links**: a relative `.md` link whose target file
   doesn't exist (broken), or that resolves to an existing file but not where
   that ADR number currently lives (stale — `adroit relink` fixes it). External
   URLs, anchors, and non-ADR links are ignored.

In the flat / frontmatter profile there is no directory-implied status, so the
directory-mismatch check is skipped; the others still apply.

```sh
adroit check
```

### `adroit relink`

Rewrite every cross-ADR relative link so it points at the ADR's **current**
location, then write back only the files that changed. Use it to repair links
left stale by file moves done outside adroit (and as a CI fix step). Status
changes (`status` / `supersede`) already relink automatically, so on a tidy repo
this is a no-op (`Links already canonical — nothing to relink.`). Idempotent;
links by external URL, anchor, or to non-ADR files are left untouched; ambiguous
duplicate numbers are skipped (and flagged by `check`).

```sh
adroit relink
```

### `adroit migrate [--yes]`

Convert the repo on disk to the **configured** layout/format. The source profile
is auto-detected; a layout change moves files between flat / by-status dirs
(bytes preserved), a format change re-serializes markdown ↔ frontmatter, and
cross-ADR links are fixed afterward (via `relink`). Filenames are kept as-is.

Set the *target* with `--layout` / `--format` (or config / `.env`), then run
migrate. It prints a **preview** by default; pass `--yes` to apply.

```sh
# Convert a by-status repo to flat:
adroit --layout flat migrate          # preview
adroit --layout flat migrate --yes    # apply

# Convert markdown ADRs to the frontmatter profile:
adroit --format frontmatter migrate --yes
```

Because adroit otherwise **refuses to operate** on a repo whose on-disk profile
doesn't match your config (see below), `migrate` is the supported way to change
your `layout`/`format` preference on an existing repo. It is idempotent (a repo
already in the target profile reports nothing to do).

### `adroit index [--check]`

Regenerate the ADR section of `SUMMARY.md`, grouped by status, preserving the non-ADR parts of the file. If no `summary_path` is configured and no `SUMMARY.md` is found next to or one level above the ADR directory, the generated block is printed to stdout.

With `--check`, adroit does **not** write: it compares what it *would* generate
against the on-disk `SUMMARY.md` and exits non-zero if they differ (printing
`SUMMARY.md is out of date — run \`adroit index\``), making it a CI gate for
documentation drift. If no `SUMMARY.md` is found it prints a note and exits 0.

```sh
adroit index           # regenerate SUMMARY.md (or print the block)
adroit index --check    # verify SUMMARY.md is up to date; non-zero if stale
```

| Flag | Description |
|---|---|
| `--check` | Verify `SUMMARY.md` is up to date without writing; exit non-zero if stale |

### `adroit review <NUMBER>`

Generate a **review-kickoff** document for an ADR — the doc the team writes when
opening an ADR for formal review. It mirrors the hand-written artifact: an H1
with the date and ADR number, a "What you're being asked to do" section, a
**Key docs** table (the ADR, the ADR README, the review-process guide), the
review timeline and quorum, what happens on the decision date, and a collapsible
"What the MR changes" block. Placeholders (`[TODO: ...]`) are left for the
proposer to fill in.

This is **pure generation** — it performs no git operations and does not modify
the ADR. The ADR is resolved by number through the store, so it works in
by-status mode and errors cleanly if the number isn't found.

Dates are computed from today using business days (weekends skipped):
the review period runs from today to today + `--days` business days, and the
decision date is the next business day after that.

```sh
adroit review 1                              # print to stdout
adroit review 1 --days 5 --quorum 3          # 5-business-day window, quorum 3
adroit review 1 --output review-kickoff.md   # write to a file
```

| Flag | Default | Description |
|---|---|---|
| `--days <N>` | config `review_days` (3) | Review period length in business days |
| `--quorum <N>` | config `review_quorum` (3) | Number of approvals required |
| `--output <PATH>` | — | Write the doc to a file instead of stdout |

### `adroit edit <NUMBER>`

Open an ADR in your editor.

```sh
adroit edit 1
```

adroit finds your editor using this precedence chain:

1. The `$VISUAL` or `$EDITOR` environment variable (session override)
2. The `editor` field in `config.yaml` (see [Configuration](#configuration))
3. Auto-detection — probes your PATH for common editors (nano, vim, nvim, VS Code, etc.)
4. Interactive prompt — if nothing is detected and you're in a terminal, adroit asks you to choose from the editors installed on your system. Your choice is saved to `config.yaml` so you're only asked once.

### `adroit serve` (requires the `web` feature)

Serve the read-only web dashboard (browse, search, stats, supersession graph)
over a local HTTP server. Built behind the `web` Cargo feature; without it the
command prints a rebuild hint and exits. See [Web Dashboard](../usage/web.md).

```sh
cargo run --features web -- serve                      # http://127.0.0.1:8080
cargo run --features web -- serve --host 0.0.0.0 --port 9000
```

| Flag | Default | Description |
|---|---|---|
| `--host <ADDR>` | `127.0.0.1` | Interface to bind (env: `ADROIT_HOST`) |
| `--port <N>` | `8080` | Port to listen on (env: `ADROIT_PORT`) |

### `adroit` (no command)

Launch the interactive TUI (browse, triage, in-terminal body editing). The TUI
opens the same ADR directory the CLI resolves — `adroit --dir X` launches the
TUI against `X`. In a non-interactive context (no TTY) it prints a hint and
exits instead of seizing the terminal. Built behind the `tui` Cargo feature
(on by default); without it, a hint points you at the CLI subcommands.

## Configuration

adroit stores configuration in `~/.config/adroit/config.yaml` (XDG on Linux, platform-appropriate elsewhere). The file is created automatically on first run with your detected editor.

```yaml
editor: vim
```

| Field | Type | Default | Description |
|---|---|---|---|
| `dir` | path | XDG data dir | ADR directory. Supports `~` and `$ENV_VAR` expansion. |
| `editor` | string | auto-detected | Preferred editor command. Include flags if needed (e.g. `code --wait`). |
| `format` | `markdown`\|`frontmatter` | `markdown` | On-disk serialization profile. |
| `layout` | `by_status`\|`flat` | `by_status` | Directory layout. |
| `status_dirs` | map | status name lowercased | Override the directory name for each status. |
| `default_template` | string | `madr` | Template used by `new`. |
| `templates_dir` | path | — | Directory of custom named templates (`<name>.md`). |
| `default_status` | status | `Proposed` | Status assigned to new ADRs. |
| `open_on_new` | bool | `true` | Open `$EDITOR` automatically after `new`. |
| `summary_path` | path | discovered | Path to a `SUMMARY.md` to regenerate on `index`. |
| `review_days` | int | `3` | Default review period (business days) for `review`. |
| `review_quorum` | int | `3` | Default approvals required for `review`. |
| `review_overdue_days` | int | `30` | A Proposed ADR older than this many days is flagged review-due even with no `review_by`. `0` disables age-based flagging. |
| `tui_theme` | `default`\|`gruvbox` | `default` | Color theme for the TUI markdown preview. |
| `date_source` | `auto`\|`git`\|`filesystem` | `auto` | Where ADR creation/lifecycle dates come from. `git` warns if history is unavailable/shallow; `filesystem` never shells git. |

All keys are optional; missing keys fall back to their defaults, so older config files keep working. You can edit this file at any time to change your defaults. Set `$VISUAL` or `$EDITOR` to override the editor for a single session.

### Path resolution for `dir`

Relative paths in the config file resolve from the XDG data directory (typically `~/.local/share/adroit/`), not from CWD:

```yaml
# Relative — resolves to ~/.local/share/adroit/my-project/
dir: my-project

# Tilde — expands to your home directory
dir: ~/decisions

# Absolute — used as-is
dir: /opt/company/adrs
```

The `--dir` CLI flag is different: it resolves relative paths from your current working directory, as you'd expect from a shell argument.
