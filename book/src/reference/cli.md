# CLI Reference

## Global options

| Flag | Default | Description |
|---|---|---|
| `--dir <PATH>` | `~/.local/share/adroit/` | Path to the ADR directory (overrides config) |
| `--version` | | Print version information |
| `--help` | | Print help |

The `--dir` flag takes precedence over the config file. If omitted, adroit checks `~/.config/adroit/config.yaml` for a `dir` field, then falls back to `~/.local/share/adroit/`.

## Commands

### `adroit new <TITLE>`

Create a new ADR with the given title. The ADR directory is created automatically if it doesn't exist.

```sh
adroit new "Use PostgreSQL for primary datastore"
```

### `adroit list`

List all ADRs as a table showing number, status, creation date, and title.

```sh
adroit list
```

Example output:

```
#     Status      Created     Title
0001  Proposed    2026-04-15  Use PostgreSQL for primary datastore
0002  Accepted    2026-04-14  Adopt ADR process
```

### `adroit show <NUMBER>`

Display a single ADR by its sequential number, including metadata and body.

```sh
adroit show 1
```

Example output:

```
ADR 0001: Use PostgreSQL for primary datastore
Status:  Proposed
Created: 2026-04-15T10:30:00Z
ID:      550e8400-e29b-41d4-a716-446655440000

## Context
We need a database.
```

### `adroit status <NUMBER> <STATUS>`

Update the lifecycle status of an ADR. Status names are case-insensitive.

Valid statuses: `proposed`, `accepted`, `deprecated`, `superseded`.

```sh
adroit status 1 accepted
```

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

### `adroit` (no command)

Launch the interactive TUI.

## Configuration

adroit stores configuration in `~/.config/adroit/config.yaml` (XDG on Linux, platform-appropriate elsewhere). The file is created automatically on first run with your detected editor.

```yaml
editor: vim
```

| Field | Type | Description |
|---|---|---|
| `dir` | path | ADR directory. Supports `~` and `$ENV_VAR` expansion. |
| `editor` | string | Preferred editor command. Include flags if needed (e.g. `code --wait`). |

You can edit this file at any time to change your defaults. Set `$VISUAL` or `$EDITOR` to override the editor for a single session.

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
