# Managing ADRs

## Creating a new ADR

```sh
adroit new "Title of the decision"
```

Each ADR is automatically assigned the next sequential number and written as a Markdown file. The ADR directory is created automatically if it doesn't exist.

## Listing ADRs

```sh
adroit list
```

Lists all ADRs as a table with their number, status, creation date, and title, sorted by number.

## Viewing an ADR

```sh
adroit show 1
```

Displays the ADR's metadata (number, title, status, creation date, ID) and body.

## Updating status

```sh
adroit status 1 accepted
```

Changes the lifecycle status of an ADR. Status names are case-insensitive. See [ADR lifecycle](#adr-lifecycle) below for valid statuses.

## Editing an ADR

```sh
adroit edit 1
```

Opens the ADR file in your editor. adroit auto-detects your editor from `$VISUAL`, `$EDITOR`, or by scanning your PATH. If no editor is found, you'll be prompted to choose one — the choice is saved to your config file for next time.

You can also set your editor explicitly in `~/.config/adroit/config.yaml`:

```yaml
editor: code --wait
```

## ADR lifecycle

Every ADR moves through a lifecycle:

| Status | Meaning |
|---|---|
| **Proposed** | Under discussion, not yet decided |
| **Accepted** | Decision has been made and is in effect |
| **Deprecated** | No longer relevant but kept for history |
| **Superseded** | Replaced by a newer decision |

New ADRs start with status **Proposed**.

## Custom ADR directory

By default, adroit stores ADRs in `~/.local/share/adroit/` (XDG data directory). You can override this with:

- The `--dir` CLI flag (highest priority, relative to CWD)
- The `dir` field in `~/.config/adroit/config.yaml` (relative to the XDG data directory)
- The built-in default: `~/.local/share/adroit/`

Use `--dir` for one-off commands:

```sh
adroit --dir ./docs/adr list
```

Or set `dir` in your config for a persistent default:

```yaml
# ~/.config/adroit/config.yaml
dir: my-project          # → ~/.local/share/adroit/my-project/
dir: ~/work/decisions    # → /home/you/work/decisions/
dir: /opt/company/adrs   # → /opt/company/adrs/
```
