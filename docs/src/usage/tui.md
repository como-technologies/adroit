# Interactive TUI

Run `adroit` with no subcommand to launch the interactive terminal interface:

```sh
adroit
```

The TUI is a keyboard-driven, two-pane interface for browsing and managing your
decision log: the left pane lists ADRs (filter by status, search, sort) and the
right pane shows a preview — rendered as GitHub-Flavored Markdown (press `m` to
toggle raw source), or an in-terminal editor when you press `i`. Every read goes
through the shared query layer and every write goes through the same `Store` path
the CLI uses, so the two surfaces never diverge.

## List & preview

| Key            | Action                                            |
| -------------- | ------------------------------------------------- |
| `j` / `k`      | Move selection down / up (also `↓` / `↑`)         |
| `g` / `G`      | Jump to first / last ADR                          |
| `Enter`        | Focus the preview pane for scrolling              |
| `/`            | Search (title + body, case-insensitive)           |
| `f`            | Cycle the status filter (All → each status → All) |
| `o`            | Cycle the sort order                              |
| `n`            | Create a new ADR (prompts for a title)            |
| `s`            | Change the selected ADR's status                  |
| `S`            | Supersede an older ADR with the selected one      |
| `i`            | **Edit the selected ADR's body in the TUI**       |
| `e`            | Open the selected ADR in `$EDITOR`                |
| `m`            | Toggle the preview between rendered and raw markdown |
| `r`            | Refresh from disk                                 |
| `q` / `Esc`    | Quit                                              |

In the preview pane, `j` / `k` scroll, `m` toggles rendered/raw, and
`Enter` / `Esc` return to the list.

## Markdown rendering & themes

The preview pane renders the ADR body as **GitHub-Flavored Markdown** —
headings, bold/italic, strikethrough, inline code, fenced code blocks, block
quotes, lists, task lists, links, horizontal rules, and tables. Press `m` to
toggle between the rendered view and the raw markdown source (the in-TUI editor,
`i`, always shows raw source — you need it to edit). Code blocks are styled but
not yet syntax-highlighted.

Two themes are available, selected with `--theme`, the `ADROIT_THEME`
environment variable, or the `tui_theme` config key:

| Theme | Description |
| ----- | ----------- |
| `default` | 16-color ANSI palette — respects your terminal's colors. The default. |
| `gruvbox` | True-color gruvbox, matching the house mdBook/doxygen theme. |

```sh
adroit --theme gruvbox            # this session
# or in ~/.config/adroit/config.yaml:  tui_theme: gruvbox
# or in a .env file:                   ADROIT_THEME=gruvbox
```

## Editing a body in the TUI

Press `i` on the selected ADR to load its markdown body into an editable buffer
shown in the right pane, with a visible cursor. This never leaves the terminal —
`e` remains the escape hatch to your full external `$EDITOR`.

| Key                 | Action                                               |
| ------------------- | ---------------------------------------------------- |
| (type)              | Insert characters                                    |
| `Enter`             | Insert a newline (split the line at the cursor)      |
| `Backspace`         | Delete back one char; at line start, join the lines  |
| `←` `→` `↑` `↓`     | Move the cursor (wraps at line ends; clamps columns) |
| `Home` / `End`      | Jump to the start / end of the line                  |
| `Tab`               | Insert four spaces                                   |
| `Ctrl-S`            | **Save** the body and refresh the preview            |
| `Esc`               | Cancel (see below)                                   |

While editing, the pane title shows the ADR number and a `[modified]` indicator
once the buffer has unsaved changes. The editor is a plain-text editor — there
is no undo/redo, selection, clipboard, or syntax highlighting; it is a focused,
correct multi-line editing surface for the common "tweak the prose" case.

### Saving and format preservation

`Ctrl-S` writes the edited body back through `Store::set_body`, which reads the
ADR, replaces **only** the body, and re-serializes via the existing format
profile. The frontmatter / `## Status` section / `> State:` banner / status
directory are all left untouched — saving a body never changes an ADR's status
or moves its file, and saving an unedited buffer is byte-identical to the
original. After a save the preview refreshes to reflect the new body.

### Cancelling

`Esc` cancels edit mode. If the buffer has **no** unsaved changes it returns to
the list immediately. If there **are** unsaved changes, adroit asks you to
confirm: press `y` (or `Esc` again) to discard your edits, or `n` to keep
editing. This guards against losing work to an accidental keystroke.
