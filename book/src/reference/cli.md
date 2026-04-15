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

List all ADR files in the directory.

```sh
adroit list
```

### `adroit` (no command)

Launch the interactive TUI.

## Configuration

adroit reads configuration from `~/.config/adroit/config.yaml` (XDG on Linux, platform-appropriate elsewhere).

```yaml
dir: ~/projects/myapp/docs/adr
```

| Field | Type | Description |
|---|---|---|
| `dir` | path | Default ADR directory. Relative paths resolve from CWD. |
