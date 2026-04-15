# Quick Start

## Initialize

Create an ADR directory in your project:

```sh
adroit init
```

This creates `docs/adr/` by default. Use `--dir` to choose a different location:

```sh
adroit init --dir decisions
```

## Create your first ADR

```sh
adroit new "Use PostgreSQL for primary datastore"
```

This creates a numbered Markdown file like `0001-use-postgresql-for-primary-datastore.md` with a standard template.

## List decisions

```sh
adroit list
```

## Launch the TUI

Run `adroit` with no subcommand to open the interactive interface:

```sh
adroit
```
