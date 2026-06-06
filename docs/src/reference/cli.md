# CLI Reference

## Global options

### Repo selection

`--dir` is **global** — inherited by every subcommand, shown under **Repo
selection** in each `--help`, and accepted before *or* after the subcommand
(`adroit --dir X list` and `adroit list --dir X` are equivalent).

The on-disk **shape** flags below (`--format`, `--layout`, `--naming`,
`--date-source`, `--relink-scope`) are also **global** — accepted before *or*
after the subcommand (`adroit --format frontmatter migrate` and `adroit migrate
--format frontmatter` are equivalent) — but **hidden** from the concise `-h` /
`--help` so the default help stays a clean command list. They surface on `adroit
--help-all` (the full reference). Most teams set them once via `config` / `.env`
(the `ADROIT_*` env var binds everywhere regardless of position) rather than
passing them per-command.

| Flag | Default | Description |
|---|---|---|
| `--dir <PATH>` / `-d` | `~/.local/share/adroit/` | Path to the ADR directory (env: `ADROIT_DIR`; overrides config) |
| `--format <markdown\|frontmatter>` | `markdown` | On-disk format profile (env: `ADROIT_FORMAT`; overrides config) |
| `--layout <by_status\|by_category\|flat>` | `by_status` | Directory layout: `by_status` (status by dir), `by_category` (dir = area, with `per_category` naming), or `flat` (env: `ADROIT_LAYOUT`; overrides config) |
| `--naming <sequential\|date\|uuid\|per_category>` | `sequential` | How ADR identifiers/filenames are formed (env: `ADROIT_NAMING`; overrides config). See [Naming schemes](./adr-format.md#naming-schemes) |
| `--date-source <auto\|git\|filesystem>` | `auto` | Where ADR dates/lifecycle come from: `auto` (git when available, else filesystem), `git` (require git; warn if unavailable/shallow), `filesystem` (never shell git) (env: `ADROIT_DATE_SOURCE`; overrides config) |
| `--relink-scope <all\|self\|none>` | `all` | How much a status-change move auto-relinks: `all` (heal every inbound link), `self` (only the moved file's own links), `none` (move only). `self`/`none` defer the rest to a post-merge `adroit relink` (env: `ADROIT_RELINK_SCOPE`; overrides config). See [Concurrent contributors](../usage/managing-adrs.md#concurrent-contributors--branching) |

### Command defaults

Top-level only — pass them *before* the subcommand (e.g. `adroit --theme gruvbox`)
or, more usually, set them in config / `.env`. The environment variable binds
everywhere regardless; the flag is kept off the concise help (it's under
`--help-all`), since just a few commands use each.

| Flag | Default | Description |
|---|---|---|
| `--theme <default\|gruvbox>` | `default` | TUI markdown-preview color theme; only the TUI and `serve` use it (env: `ADROIT_THEME`; overrides config) |
| `--review-overdue-days <N>` | `30` | Days after which a Proposed ADR with no `review_by` is flagged review-due; `0` disables. Used by `list`/`stats`/`check` (env: `ADROIT_REVIEW_OVERDUE_DAYS`; overrides config) |
| `--default-template <name\|path>` | `madr` | Default template for `new` — `madr`/`nygard` or a path (env: `ADROIT_TEMPLATE`; overrides config; `new --template` still wins) |

### Help

`-h` and `--help` show the **same concise** view — the command list plus the
everyday options (`--dir`, `--output`). **`--help-all`** adds every option in
full detail (the repo-shape + command-default flags above, with their possible
values). Both work on the top level and on each subcommand (`adroit new --help`
vs `adroit new --help-all`). `--version` prints the version.

### Output

Global — works before *or* after any verb (`adroit list -o json`). Selects how the
**read** verbs print their result.

| Flag | Default | Description |
|---|---|---|
| `-o`, `--output <human\|json>` | `human` | Result format for `list` / `show` / `search` / `stats` / `graph` / `check`. `json` emits the structured `view` types — the same contract the web API returns — for scripts and AI agents. Other verbs ignore it. |

`json` goes to **stdout**; warnings/errors go to **stderr**. `check -o json` still
exits non-zero on an Error-severity problem, so a CI gate or agent can branch on
the exit code while parsing the report from stdout. See
[Automation & AI](../usage/automation.md).

Each also reads from an environment variable, so you don't have to pass it on
every command: `ADROIT_DIR`, `ADROIT_FORMAT`, `ADROIT_LAYOUT`, `ADROIT_THEME`,
`ADROIT_REVIEW_OVERDUE_DAYS`, `ADROIT_TEMPLATE`, `ADROIT_DATE_SOURCE`,
`ADROIT_NAMING` (and, for the web dashboard, `ADROIT_HOST` / `ADROIT_PORT`).
A `.env` file in the current directory (or a parent) is loaded automatically
at startup, so you can keep your repo location there. Copy the tracked
`.env.example` to get started (your local `.env` is git-ignored):

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

These mirror `adroit --help`, grouped by workflow stage — reading top to bottom tracks a decision's life.

### Author a decision

#### `adroit new <TITLE>`

Create a new ADR with the given title. The ADR directory is created automatically if it doesn't exist. In markdown mode the file is scaffolded from a template and written into the `proposed/` directory, then opened in your editor.

```sh
adroit new "Use PostgreSQL for primary datastore"
adroit new "Use Redis" --template nygard   # pick a template by name or path
adroit new "Use Redis" --no-edit           # skip opening the editor
adroit new "Adopt feature flags" --interview   # AI drafts the body from a short Q&A
```

| Flag | Description |
|---|---|
| `--template <name\|path>` | Template to scaffold from (`madr`, `nygard`, or a file path) |
| `--no-edit` | Do not open the editor after creating the ADR |
| `--category <name>` / `-c` | Category subdirectory — **required** under the `by_category` layout, rejected otherwise |
| `--force` | Create even if an ADR with this exact title already exists (skip the duplicate guard) |
| `--interview` | Run a short Socratic interview and have the configured AI provider draft the body from your answers + the existing corpus (opt-in). See [AI-assisted authoring](../usage/automation.md#ai-assisted-authoring) |

**Duplicate guard.** `new` is an imperative event — it always allocates the next
number, so it is **not** idempotent (running it twice makes two ADRs). To catch
the *accidental* re-run, it checks for an ADR with the same (case-insensitive)
title first: it warns and lists the match plus the most similar existing ADRs
(via the same engine as [`dedupe`](#adroit-related-id--adroit-dedupe-id)), then,
on a terminal, prompts `[y/N]` before creating. On a non-terminal (scripts/CI) it
warns and proceeds; `--force` skips the check entirely.

With `--interview`, the identity, status, and heading stay mechanical — the AI
only writes the prose sections, marked `<!-- adroit:ai-suggested -->` for you to
review and edit before committing. If no provider is configured it degrades to
the plain template (the ADR is still created).

#### `adroit draft <ID>`

The **after-the-fact `new --interview`**: run the same AI interview on an ADR you
already created. Use it when you made an ADR with a plain `adroit new "Title"`
(a bare template) and want to fill it in later — at any point before review.

It asks the same Socratic questions, drafts the body from your answers + the
corpus, and **splices** it over the prose — the `# ADR-NNNN` heading and
`## Status` stay mechanical — marks it `<!-- adroit:ai-suggested -->`, then opens
your editor. So the iterative flow is: `new` → (`draft` whenever you want AI help)
→ `edit` / hand-tune → PR. Needs an AI provider (no template fallback, since the
ADR already exists).

```sh
adroit draft 2            # interview + draft ADR-0002, then open the editor to review
adroit draft 2 --no-edit  # draft it without opening the editor
```

| Flag | Description |
|---|---|
| `--no-edit` | Do not open the editor after drafting |

#### `adroit plan <ID>`

Draft an **AI implementation plan** for an (accepted) ADR: reads the ADR + the
existing corpus and asks the configured AI provider for an ordered, actionable
checklist (steps, components touched, testing, rollout, risks). **Read-only** —
it never modifies the ADR. Prints to stdout unless `--out <PATH>` is given. Needs
an AI provider — see [AI-assisted authoring](../usage/automation.md#ai-assisted-authoring).

```sh
adroit plan 21                       # print the plan
adroit plan 21 --out plan-0021.md    # write it to a file
```

| Flag | Description |
|---|---|
| `--out <PATH>` | Write the plan to a file instead of stdout |

#### `adroit edit <ID>`

Open an ADR in your editor (`<ID>` resolved as in [`show`](#adroit-show-id)).

```sh
adroit edit 1
```

adroit finds your editor using this precedence chain:

1. The `$VISUAL` or `$EDITOR` environment variable (session override)
2. The `editor` field in `config.yaml` (see [Configuration](#configuration))
3. Auto-detection — probes your PATH for common editors (nano, vim, nvim, VS Code, etc.)
4. Interactive prompt — if nothing is detected and you're in a terminal, adroit asks you to choose from the editors installed on your system. Your choice is saved to `config.yaml` so you're only asked once.

#### `adroit lint <ID>`

Check one ADR's **authoring quality** (read-only) — distinct from `check`, which
validates structural repo integrity. The mechanical checks need no AI: sections
still left as their italic `_…_` prompt, a missing or empty
`### Negative Consequences`, and fewer than two `## Considered Options`. The
prompt check is template-agnostic — any section whose only content is the prompt
the template shipped. `--ai` adds a model review against ADR best
practices + house style (needs a provider; see
[AI-assisted authoring](../usage/automation.md#ai-assisted-authoring)). Exits
**non-zero** on mechanical findings, so it works as an authoring gate; the AI
review is advisory. `-o json` emits the findings.

```sh
adroit lint 21            # mechanical checks
adroit lint 21 --ai       # + an AI review
adroit lint 21 -o json    # structured findings for an editor/agent
```

| Flag | Description |
|---|---|
| `--ai` | Also run an AI review (needs a configured AI provider) |

#### `adroit related <ID>` / `adroit dedupe <ID>`

Find ADRs textually similar to a given one — **mechanical** (TF-IDF cosine over
titles + bodies), no AI and no provider. `related` surfaces similar ADRs the
target **isn't already linked to** (candidates to `link`); `dedupe` includes the
linked ones and is framed for catching "did we already decide this?" before a new
ADR re-litigates a decision. Read-only; `-o json` emits the ranked matches
(`reference`, `title`, `score`).

```sh
adroit related 21            # similar ADRs you might want to link
adroit dedupe 21 -o json     # overlaps as JSON, highest score first
```

> Similarity is lexical for now (shared significant terms); a semantic
> (embeddings) upgrade is future work.

#### `adroit link <ID> <--relates-to|--depends-on|--refines> <TARGET>`

Add (or remove with `--remove`) a **typed relational link** from `<ID>` to
`<TARGET>` (both addressed as in [`show`](#adroit-show-id)). Exactly one of the
three kind flags names the target. The link is recorded in `<ID>`'s frontmatter,
listed by `adroit show`, and drawn as a distinct edge in the dashboard's
relationship graph. Adding validates that the target exists.

This is a **frontmatter-profile** feature; under the markdown profile it errors
with a hint to run `adroit --format frontmatter migrate`. See
[ADR Format → Relationships](./adr-format.md#relationships).

```sh
adroit link 6 --depends-on 2          # ADR-0006 depends on ADR-0002
adroit link 6 --relates-to 4
adroit link 6 --refines 3
adroit link 6 --depends-on 2 --remove
```

| Flag | Description |
|---|---|
| `--relates-to <TARGET>` | A non-directional related link |
| `--depends-on <TARGET>` | This ADR depends on the target |
| `--refines <TARGET>` | This ADR refines / elaborates the target |
| `--remove` | Remove the link instead of adding it |

### Review & decide

#### `adroit set-review <ID> <DATE>`

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

#### `adroit review <NUMBER>`

Generate a **review-kickoff** document for an ADR — the doc the team writes when
opening an ADR for formal review. It mirrors the hand-written artifact: an H1
with the date and ADR number, a "What you're being asked to do" section, a
**Key docs** table (the ADR, the ADR README, the review-process guide), the
review timeline and quorum, what happens on the decision date, and a collapsible
"What the MR changes" block. Placeholders (`[TODO: ...]`) are left for the
proposer to fill in.

This is **pure generation** — it performs no git operations and does not modify
the ADR. The ADR is resolved by number through the store, so it works in
by-status mode and errors cleanly if the number isn't found. Because the kickoff
doc is built around the ADR number, `review` is **numeric-only** (requires a
`sequential`/`per_category` scheme).

Dates are computed from today using business days (weekends skipped):
the review period runs from today to today + `--days` business days, and the
decision date is the next business day after that.

```sh
adroit review 1                              # print to stdout
adroit review 1 --days 5 --quorum 3          # 5-business-day window, quorum 3
adroit review 1 --out review-kickoff.md      # write to a file
```

| Flag | Default | Description |
|---|---|---|
| `--days <N>` | config `review_days` (3) | Review period length in business days |
| `--quorum <N>` | config `review_quorum` (3) | Number of approvals required |
| `--out <PATH>` | — | Write the doc to a file instead of stdout (long-only; `-o`/`--output` is the global result-format selector) |

#### `adroit summarize <ID>`

A one-paragraph, plain-language **AI TL;DR** of an ADR — for a PR description, a
chat notification, or a decision-log entry. Read-only; prints to stdout unless
`--out <PATH>`. Needs an AI provider (see
[AI-assisted authoring](../usage/automation.md#ai-assisted-authoring)).

```sh
adroit summarize 21
adroit summarize 21 --out tldr.md
```

#### `adroit set-status <ID> <STATUS>`

Set the lifecycle status of an ADR (`<ID>` resolved as in [`show`](#adroit-show-id)).
Status names are case-insensitive. In by-status markdown mode this **moves the
file** to the matching status directory and rewrites the `## Status` section
(minimal-diff), then reconciles cross-ADR links per
[`relink_scope`](#configuration). Mirrors [`set-review`](#adroit-set-review-id-date).

Valid statuses: `proposed`, `accepted`, `rejected`, `deprecated`, `superseded`.

```sh
adroit set-status 1 accepted
```

#### `adroit supersede <NEW> <OLD>`

Mark `<OLD>` as superseded by `<NEW>` in one command (each addressed as in
[`show`](#adroit-show-id)): moves the old ADR to `superseded/` (or rewrites its
status in place under `by_category`), writes `Superseded by [<NEW>](...)` into
its status, and adds a reciprocal `Supersedes [<OLD>](...)` note to the new ADR.
Works under every naming scheme — the supersession links carry the scheme's
reference (a number or a slug).

```sh
adroit supersede 6 2                 # sequential
adroit supersede 20260601-b 20260515-a   # date scheme
```

### Explore the corpus

#### `adroit list [--status <STATUS>]`

List ADRs as a table showing number, status, and title. Recurses into all status directories in by-status mode. Pass `--status` to filter.

```sh
adroit list
adroit list --status accepted
```

#### `adroit show <ID>`

Display a single ADR: its status, creation and
last-modified dates, supersession links, path, body, and — when the repo is a
git repository — a **History** timeline of its lifecycle (proposed → accepted /
rejected / superseded), with the date and commit subject of each transition.

`<ID>` is resolved through the configured [naming scheme](./adr-format.md#naming-schemes):
a number (`9` or `ADR-0009`) under `sequential`, the filename slug under `date`,
or a unique leading prefix of the UUID under `uuid`.

```sh
adroit show 1                       # sequential
adroit show 20260601-adopt-postgresql   # date scheme
```

Dates and the timeline are read from **git history**, not the file: the first
commit that added the ADR is its creation, and each status change is a directory
move git records. Outside a git repository adroit falls back to the file's
modification time, and the timeline is omitted. See
[ADR Format](./adr-format.md#dates-come-from-git).

#### `adroit status <ID>`

Print an ADR's current status — just the word, **lowercase** (`<ID>` resolved as
in [`show`](#adroit-show-id)). It's a focused, scriptable getter: the output
feeds straight into [`set-status`](#adroit-set-status-id-status) or a shell test,
and matches the by-status directory names. For the full record use
[`show`](#adroit-show-id) (whose `Status:` line is the capitalized display form).

```sh
adroit status 1            # -> proposed
[ "$(adroit status 1)" = accepted ] && echo "ready to publish"
```

#### `adroit search <TERM>`

Case-insensitive search across ADR titles and bodies (recursive). Prints number, status, and title for each match.

```sh
adroit search postgres
adroit search postgres -o json   # structured matches for scripts/agents
```

#### `adroit stats`

Repo statistics: total ADRs, a per-status breakdown (a colored bar chart), the
oldest still-`Proposed` ADRs (with review-due flags), and a created-per-month
histogram. `-o json` emits the full `view::Stats`.

```sh
adroit stats
adroit stats -o json
```

#### `adroit graph`

The ADR relationship graph — supersession plus typed (`relates_to` /
`depends_on` / `refines`) links. The human view is a **tree**: each ADR with
outgoing relationships, its edges indented beneath it (with an `unconnected:`
footnote for isolated ADRs); `-o json` emits `view::Graph` (the same nodes/edges
the web dashboard's relationship graph consumes).

```sh
adroit graph
adroit graph -o json
```

> Human output is colored (status, edge kinds, scores) when stdout is a terminal;
> it's plain under a pipe, `-o json`, or `NO_COLOR`.

#### `adroit ask "<question>"`

Ask a natural-language question of the ADR corpus. Retrieval is **mechanical**
(TF-IDF over your question picks the most relevant ADRs); the configured **AI
provider** then synthesizes an answer, citing the ADRs it used. Read-only. The
human view prints the answer to stdout and the sources to stderr; `-o json` emits
`{ "answer": …, "sources": [refs] }`. Needs an AI provider (see
[AI-assisted authoring](../usage/automation.md#ai-assisted-authoring)).

```sh
adroit ask "Why did we pick Postgres over MySQL?"
adroit ask "What did we decide about caching?" -o json
```

#### `adroit serve` (requires the `web` feature)

Serve the read-only web dashboard (browse, search, stats, relationship graph,
repo-health checks) over a local HTTP server. Built behind the `web` Cargo feature; without it the
command prints a rebuild hint and exits. See [Web Dashboard](../usage/web.md).

```sh
cargo run --features web -- serve                      # http://127.0.0.1:8080
cargo run --features web -- serve --host 0.0.0.0 --port 9000
```

| Flag | Default | Description |
|---|---|---|
| `--host <ADDR>` | `127.0.0.1` | Interface to bind (env: `ADROIT_HOST`) |
| `--port <N>` | `8080` | Port to listen on (env: `ADROIT_PORT`) |

#### `adroit` (no command)

Launch the interactive TUI (browse, triage, in-terminal body editing). The TUI
opens the same ADR directory the CLI resolves — `adroit --dir X` launches the
TUI against `X`. In a non-interactive context (no TTY) it prints a hint and
exits instead of seizing the terminal. Built behind the `tui` Cargo feature
(on by default); without it, a hint points you at the CLI subcommands.

### Maintain the repo

#### `adroit check`

Validate the ADR repo and **exit non-zero if any error-severity problem is
found** — a structural CI gate. Problems are listed on stderr. A clean repo
prints `OK: N ADRs, no problems`; a repo with only warnings prints
`OK: N ADRs, M warning(s)` and still exits 0 (so a deferred-relink PR branch,
whose inbound links aren't canonicalized yet, isn't blocked). Only **errors**
fail the build.

It checks for:

1. **Status ↔ directory mismatch** (by-status only): a file's `## Status`
   section declares a status that disagrees with the directory it lives in. A
   section with no explicit status word is fine (the directory is the source of
   truth).
2. **Duplicate identifiers**: two ADR files sharing the same identity under the
   configured [naming scheme](./adr-format.md#naming-schemes) — the same `NNNN`
   for `sequential`, or the same slug/uuid for `date`/`uuid`.
3. **Unparseable files**: a `.md` ADR with no `# ADR-NNNN: Title` heading.
4. **Broken supersession links**: a `Supersedes ADR-NNNN` / `Superseded by
   ADR-NNNN` note referencing a number that doesn't exist in the repo.
5. **Broken / stale cross-ADR links**: a relative `.md` link that points
   somewhere other than its ADR's current home. The split is identity-based: if
   the link names an ADR that still exists in the repo it's a **stale** link (a
   **warning** — `adroit relink` fixes it); if it names no existing ADR it's a
   **broken** link (an **error**). External URLs, anchors, and non-ADR links are
   ignored.
6. **Duplicate titles** (a **warning**): two or more ADRs share the same
   (case-insensitive) title — usually an accidental re-run of `new`. Titles *can*
   legitimately repeat, so this never fails the gate; it just surfaces the dups.

Of these, duplicate identifiers, status↔directory mismatch, unparseable files,
broken supersession links, and broken links are **errors** (they fail `check`);
a stale link is a **warning** (reported, but `check` still exits 0).

In the flat / frontmatter profile there is no directory-implied status, so the
directory-mismatch check is skipped; the others still apply.

```sh
adroit check
```

The same validation runs behind the web dashboard's **repo-health panel** (via
`GET /api/check`), so the issues `check` reports on the command line also show up
there — see [Web Dashboard](../usage/web.md).

#### `adroit relink`

Rewrite every cross-ADR relative link so it points at the ADR's **current**
location, then write back only the files that changed. Use it to repair links
left stale by file moves done outside adroit (and as the post-merge "heal-on-main"
CI step). Status changes (`status` / `supersede`) already relink automatically
**under the default `relink_scope = all`**, so on a tidy repo this is a no-op
(`Links already canonical — nothing to relink.`). Under `relink_scope = self` /
`none` a status move leaves neighbors' inbound links for this command to fix —
which is the point of running it on `main` after a merge (see
[Concurrent contributors](../usage/managing-adrs.md#concurrent-contributors--branching)).
This command is **always full-scope** regardless of `relink_scope`. Idempotent;
links by external URL, anchor, or to non-ADR files are left untouched; ambiguous
duplicate numbers are skipped (and flagged by `check`). Pass `--dry-run` to list
the files/links that would change without writing anything.

```sh
adroit relink              # rewrite stale links in place
adroit relink --dry-run    # show what would change, write nothing
```

#### `adroit renumber <OLD> <NEW> [--file <PATH>]`

Renumber a sequential ADR — to resolve a duplicate `NNNN` (e.g. two branches
that each created `0009`). It renames the file (slug preserved), rewrites its
`# ADR-NNNN:` heading, and **retargets + relabels every inbound reference**
(`[ADR-OLD](…)` → `[ADR-NEW](…)`), then relinks. References are matched by
filename, so a duplicate-numbered sibling with a different slug is left
untouched.

```sh
adroit renumber 9 21                       # ADR-0009 -> ADR-0021
# When two files share 0009, point at the one to move:
adroit renumber 9 21 --file proposed/0009-adopt-crossplane.md
```

`<NEW>` must be unused; an ambiguous `<OLD>` (two files) errors unless you pass
`--file`. (Applies to the sequential / per-category numbering schemes.)

#### `adroit migrate [--dry-run] [--yes]`

Convert the repo on disk to the **configured** layout/format. The source profile
is auto-detected; a layout change moves files between flat / by-status dirs
(bytes preserved), a format change re-serializes markdown ↔ frontmatter, and
cross-ADR links are fixed afterward (via `relink`). Filenames are kept as-is.

Set the *target* with `--layout` / `--format` (or config / `.env`), then run
migrate. It prints a **preview** by default (or with an explicit `--dry-run`);
pass `--yes` to apply. `--dry-run` overrides `--yes`, so a preview is never
destructive.

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

#### `adroit index [--check]`

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

#### `adroit publish --out <DIR>`

Export the accepted ADR set to a directory (static-dir publisher). `--out <OUT>`
is **required**; `--dry-run` previews the export without writing. Also honors the
global `-o`/`--output`.

```sh
adroit publish --out ./public/adrs     # export accepted ADRs to a static dir
adroit publish --out ./public/adrs --dry-run
```

### Configuration

#### `adroit config [show | get <key> | set <key> <value> [--local]]`

Inspect or change configuration.

- **`adroit config`** (or `config show`) lists every setting with its **resolved
  value and source** — `flag`, `env`, `config` (set in `config.yaml`), or
  `default` — which is the quickest way to answer "why is my layout `flat`?"
  given the precedence chain (flag > env/`.env` > `config.yaml` > default).
- **`adroit config get <key>`** prints one resolved value (scriptable).
- **`adroit config set <key> <value>`** persists to `config.yaml` (validated
  against the key's type). With **`--local`** it instead upserts `KEY=value` into
  a `.env` in the current directory (a per-project / per-machine override) — only
  for keys that have an environment variable.

```sh
adroit config                         # show all settings + where each came from
adroit config get layout
adroit config set date_source git     # -> ~/.config/adroit/config.yaml
adroit config set layout flat --local # -> ./.env  (ADROIT_LAYOUT=flat)
```

`config` works even when the repo's on-disk profile doesn't match your config
(it's about settings, not ADRs), so you can use it to diagnose and fix a
mismatch. `config get`/`set` cover the **scalar** keys in the
[Configuration](#configuration) table below; `status_dirs`, `templates_dir`, and
`summary_path` are set by editing `config.yaml` directly.

#### `adroit completions <SHELL>`

Print a shell completion script to stdout, generated from adroit's command tree
(so it always matches your installed version — and a build without the `forge`
feature omits the forge commands/flags). `<SHELL>` is `bash`, `zsh`, `fish`,
`powershell`, or `elvish`.

The quickest way (kubectl-style) — source it from your shell's startup file so
it loads every session:

```sh
# ~/.bashrc
. <(adroit completions bash)

# ~/.zshrc   (ensure `autoload -U compinit && compinit` runs after)
. <(adroit completions zsh)

# fish
adroit completions fish | source
```

Or install the script to the location your shell scans, which is faster to load
and survives without adroit on `PATH` at startup:

```sh
# bash (system-wide)
adroit completions bash | sudo tee /etc/bash_completion.d/adroit > /dev/null
# zsh (a dir on your $fpath, e.g.)
adroit completions zsh > ~/.zfunc/_adroit
# fish
adroit completions fish > ~/.config/fish/completions/adroit.fish
```

Completion covers subcommands, flags, and enum values (e.g. `--format
markdown|frontmatter`, `set-status <TAB>` → the status names).

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
| `layout` | `by_status`\|`flat`\|`by_category` | `by_status` | Directory layout. `by_category` makes each subdirectory an area (status lives in `## Status`); pairs with `per_category` naming. |
| `status_dirs` | map | status name lowercased | Override the directory name for each status (by-status layout). |
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
| `naming` | `sequential`\|`date`\|`uuid`\|`per_category` | `sequential` | How ADR identifiers/filenames are formed. Pick one for the repo's lifetime — see [Naming schemes](./adr-format.md#naming-schemes). |
| `relink_scope` | `all`\|`self`\|`none` | `all` | How much a status-change move auto-relinks. `all` heals every inbound link; `self` fixes only the moved file; `none` moves only. Use `self`/`none` for concurrent-PR teams and run `adroit relink` post-merge — see [Concurrent contributors](../usage/managing-adrs.md#concurrent-contributors--branching). |
| `forge.provider` | `none`\|`github`\|`gitlab` | `none` | Forge integration (the `forge` feature is in the default build; `none` keeps it off). `github` drives GitHub PRs + Issues. |
| `forge.repo` | `owner/repo` | — | The provider slug (GitHub `owner/repo`). Required when a provider is set. |
| `forge.host` | host | provider default | API host for self-managed / enterprise. GitLab self-hosted: the host (`gitlab.example.com`); GitHub Enterprise: the host incl. base path (`ghe.example.com/api/v3`). Same token auth as the cloud version. |
| `forge.branch_prefix` | string | `adr/` | Branch prefix `new --forge` generates (`adr/0021-…`). |
| `forge.base_branch` | string | `main` | Base branch PRs target. |
| `forge.tracker` | `native`\|`jira`\|… | `native` | Issue tracker; `native` = the forge's own issues. `jira` pairs a GitHub/GitLab forge with Jira. |
| `forge.tracker_project` | string | — | Split-tracker project key (e.g. the Jira project `OPS`). |
| `forge.tracker_host` | host | — | Split-tracker API host: `your-site.atlassian.net` for Jira Cloud, or a self-hosted host (`jira.example.com`) for Jira Server/Data Center. |

Tokens are **never** stored in config. They resolve in order: the environment
(`ADROIT_GITHUB_TOKEN` / `ADROIT_GITLAB_TOKEN` / `ADROIT_JIRA_TOKEN` +
`ADROIT_JIRA_EMAIL`), then a local credential file written by `adroit auth`. The
`forge` feature is in the default build (only a `--no-default-features` core omits
it). **Jira auth follows the deployment:**
set `ADROIT_JIRA_EMAIL` for Jira **Cloud** (Basic `email:token`); omit it for
Jira **Server/Data Center** and supply a Personal Access Token as
`ADROIT_JIRA_TOKEN` (Bearer). GitHub/GitLab use the same token whether cloud or
self-hosted — only `forge.host` changes. The integration is opt-in per command:

- `new` / `set-status` / `supersede` / `review` / `set-review` take `--forge`
  (+ `--dry-run` to preview, `--yes` to apply a mutation like a PR merge).
- `set-status <id> accepted --forge --yes` does the whole accept in one command:
  verify approvals/CI → merge the PR → close the issue → fast-forward the base
  branch → move `proposed/ → accepted/` + relink → **commit and push that relink
  commit to the base branch**, so `accepted/` lands on `main`. If the tree is
  dirty, the base diverged, or the push is rejected, it warns and leaves the move
  committed/uncommitted locally for you to push (`rejected`/`deprecated` close the
  PR instead, so they don't produce a relink commit).
- `check --forge` and `list --forge` add read-only forge awareness (drift checks
  / live PR state).
- `adroit reconcile` syncs local status with the forge after **out-of-band**
  changes (an MR merged or a tracker issue closed *outside* adroit): it reports
  drift, and with `--yes` fixes the clear case — a merged PR whose ADR isn't
  accepted — by moving it to `accepted/` (+ relink). It's **read-only on the
  forge** (never merges/closes); a closed issue on a still-proposed ADR is
  reported, not auto-fixed (accept vs won't-fix is ambiguous).
- `adroit init` is an interactive setup wizard: it detects the provider/repo
  from the git remote (confirm or override), asks for the issue tracker, writes
  `forge.*`, and optionally writes `./.env` (ADROIT_DIR — the token stays in your
  shell), drops a repo-local `adr-template.md` (MADR), and installs a pre-commit
  hook running `adroit check`. `--print` previews; `--yes` does the full setup
  non-interactively (detected forge + native tracker).
- `adroit publish --out <dir>` exports accepted ADRs (static-dir, core/offline);
  `adroit notify <id>` posts to a Slack/Teams webhook (`ADROIT_NOTIFY_WEBHOOK`).
- `adroit auth <github|gitlab|jira> [--token <T>] [--email <E>]` saves a token to
  a `0600` `credentials.yaml` beside the config (prompts if `--token` is omitted),
  so you don't have to re-export an env var each session. Environment variables
  still take precedence; `--email` stores the Jira account email.
- The `serve` dashboard shows a **read-only** Forge panel on each ADR's detail
  page (linked issue + PR, with PR approvals / CI / merged state), fetched from
  `GET /api/adrs/{id}/forge`. It only *reads* the forge — authoring stays in the
  CLI — and renders nothing unless a provider is configured and the ADR is linked.

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
