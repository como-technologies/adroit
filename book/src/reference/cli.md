# CLI Reference

## Global options

| Flag | Default | Description |
|---|---|---|
| `--dir <PATH>` | `docs/adr` | Path to the ADR directory |
| `--version` | | Print version information |
| `--help` | | Print help |

## Commands

### `adroit init`

Initialize an ADR directory. Creates the directory if it doesn't exist.

```sh
adroit init
adroit --dir decisions init
```

### `adroit new <TITLE>`

Create a new ADR with the given title.

```sh
adroit new "Use PostgreSQL for primary datastore"
```

### `adroit list`

List all ADR files in the directory.

```sh
adroit list
```

### `adroit` (no command)

Launch the interactive TUI.
