# Using adroit with Your Repo

The quick start writes ADRs to adroit's default data directory. In practice you
want adroit to manage the ADRs that live **inside a real project repo** — for
example a team playbook where decisions are committed alongside code and
published from `SUMMARY.md`. This guide ties the pieces together.

## 1. Point adroit at your ADR directory

Every command accepts a global `--dir` flag that takes precedence over config:

```sh
adroit --dir /path/to/your-repo/src/adrs list
```

adroit's default **markdown / by-status** profile expects (and creates on first
write) the lifecycle directories:

```
src/adrs/
  proposed/    accepted/    rejected/    superseded/    deprecated/
```

Status is the directory; the number and title come from the `# ADR-NNNN: Title`
heading. adroit speaks this status-by-directory convention out of the box, so it
can drive an existing repo of that shape without any conversion.

## 2. Make it the default (skip `--dir` every time)

You have two ways to make a directory the default so you can skip `--dir`:

**A `.env` file (per-repo, recommended for a checked-out repo).** adroit loads a
`.env` from the current directory (or a parent) at startup. Copy the tracked
`.env.example` and edit it (your local `.env` is git-ignored):

```sh
cp .env.example .env
# .env  (in your working tree)
ADROIT_DIR=/path/to/your-repo/src/adrs
```

Now plain `adroit list`, `adroit serve`, etc. target that repo. Every other
setting has a matching `ADROIT_*` variable that works the same way (`ADROIT_FORMAT`,
`ADROIT_LAYOUT`, `ADROIT_NAMING`, `ADROIT_DATE_SOURCE`, …, plus the dashboard's
`ADROIT_HOST` / `ADROIT_PORT`). A real shell environment variable overrides the
`.env` file.

> **Heads-up:** `ADROIT_DIR` is tilde / `$VAR`-expanded too, so `ADROIT_DIR=~/repo/adrs`
> works from a `.env` (the shell never sees it to expand the `~`). And if the
> resolved directory doesn't already exist, adroit creates it and **prints a
> warning** — so a typo'd path surfaces loudly instead of silently returning an
> empty repo.

**Or the user config (global default).** Set `dir` in
`~/.config/adroit/config.yaml` so plain `adroit` commands target your repo:

```yaml
dir: /path/to/your-repo/src/adrs
```

`dir` supports `~` and `$ENV_VAR` expansion. Relative values resolve from
adroit's data directory, so use an absolute path (or `~/…`) to point at a repo
elsewhere on disk. The full set of config keys — `format`, `layout`,
`status_dirs`, `default_template`, `templates_dir`, `default_status`,
`open_on_new`, `summary_path`, `review_days`, `review_quorum`,
`review_overdue_days`, `tui_theme`, `date_source`, `naming` — is documented in
the [CLI Reference](../reference/cli.md#configuration). Run `adroit config` to
see each one's resolved value and where it came from.

If your repo uses its own ADR template, drop it at `adrs/adr-template.md` (adroit
prefers a repo-local template) or set `templates_dir`/`default_template`.

## 3. The daily loop

```sh
# Capture a decision (lands in proposed/, opens your editor)
adroit new "Use PostgreSQL for the primary datastore"

# Find prior decisions mid-discussion
adroit search postgres

# Propose a review deadline; once it passes, the ADR shows as review-due
adroit set-review 9 2026-07-15

# Generate the review-kickoff doc when it's ready for a formal decision
adroit review 9 --out review-kickoff.md

# Record the outcome — moves the file + rewrites status in one step
adroit set-status 9 accepted
# ...or supersede an older decision with a newer one
adroit supersede 9 4

# Keep the published index in sync, then commit
adroit index
git add -A && git commit -m "ADR-0009: accept PostgreSQL"
```

Prefer an interactive surface? Run bare `adroit` for the [TUI](./tui.md)
(browse, triage, and edit in the terminal), or `adroit serve` for the read-only
[web dashboard](./web.md) (browse, search, stats, and a relationship graph that
auto-refreshes as you edit).

## 4. Keep `SUMMARY.md` in sync

If your repo publishes via mdBook (or Confluence from `SUMMARY.md`), `adroit
index` regenerates the ADR section grouped by status, preserving the rest of the
file. Point it explicitly with `summary_path` in config, or let adroit discover
a `SUMMARY.md` next to or one level above the ADR directory.

## 5. Gate it in CI

Two commands exit non-zero on a problem, so they drop straight into a CI job to
keep the ADR repo honest:

```sh
adroit check          # structural validation: status/dir mismatch, duplicate
                      # numbers, unparseable files, broken supersession links
adroit index --check  # fail if SUMMARY.md is stale (run `adroit index` locally)
```

`check` prints each problem to stderr and a one-line summary on failure; on a
clean repo it prints `OK: N ADRs, no problems`. `index --check` never writes —
it just verifies `SUMMARY.md` matches what `adroit index` would produce. Both
pass cleanly when run in a non-by-status profile where directory checks don't
apply.

## A note on safety

adroit's markdown writes are **format-preserving**: status changes rewrite only
the `## Status` region, body edits rewrite only the body, and an unchanged
round-trip is byte-identical. Even so, since these are your real, version-tracked
files, your git history is the backstop — review `git diff` before committing, as
you would for any change.
