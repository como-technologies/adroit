# Quick Start

## Create your first ADR

No setup needed — adroit auto-creates the ADR directory on first use:

```sh
adroit new "Use PostgreSQL for primary datastore"
```

This creates `~/.local/share/adroit/0001-use-postgresql-for-primary-datastore.md` with YAML frontmatter and a standard template.

Use `--dir` to choose a different location:

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
adroit status 1 accepted
```

## Edit a decision

```sh
adroit edit 1
```

## Launch the TUI

Run `adroit` with no subcommand to open the interactive interface:

```sh
adroit
```
