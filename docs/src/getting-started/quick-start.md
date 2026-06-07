# Quick Start

adroit defaults to the **markdown / by-status** profile: an ADR's status is the
directory it lives in (`proposed/`, `accepted/`, …), and its number and title
come from the `# ADR-NNNN: Title` heading — no YAML frontmatter. See
[ADR Format](../reference/adr-format.md) for the full details and the
alternative frontmatter profile.

## Create your first ADR

No setup needed — adroit auto-creates the ADR directory on first use:

```sh
adroit new "Use PostgreSQL for primary datastore"
```

This assigns the next sequential number, scaffolds the file from the `madr`
template, writes it into `proposed/0001-use-postgresql-for-primary-datastore.md`,
and opens it in your editor. Use `--no-edit` to skip the editor.

Use `--dir` to choose a different location (for a real team repo, point it at
your ADR directory — see [Using adroit with Your Repo](../usage/your-repo.md)):

```sh
adroit --dir decisions new "Use PostgreSQL for primary datastore"
```

## List decisions

```sh
adroit list
```

## View a decision

```sh
adroit show 1
```

## Accept a decision

```sh
adroit set-status 1 accepted
```

In by-status mode this moves the file from `proposed/` to `accepted/` and
rewrites its `## Status` section — the rest of the file is left byte-identical.
(Reading the status back is `adroit status 1`, which prints `accepted`.)

## Edit a decision

```sh
adroit edit 1
```

## Launch the TUI

Run `adroit` with no subcommand to open the interactive interface (browse,
triage, in-terminal editing):

```sh
adroit
```

Press `?` for the keybinding cheat-sheet, `:` for the fuzzy command palette (every
action by name), and `Enter` to focus the preview. See
[Interactive TUI](../usage/tui.md) for the full keymap, themes, the vi editor, and
the in-TUI AI assists.

## Next steps

- Walk a decision end-to-end — local-only, AI-assisted, or forge — in
  [The ADR Workflow](../usage/workflow.md#worked-workflows).
- Point adroit at a real team repo: [Using adroit with Your Repo](../usage/your-repo.md).
