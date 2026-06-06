# Interactive TUI

Run `adroit` with no subcommand to launch the interactive terminal interface:

```sh
adroit
```

The TUI is a keyboard-driven (and mouse-aware), two-pane interface for browsing
and managing your decision log: a top status bar breadcrumbs the current view
(`adroit › <filter> › "<search>" · N ADRs · sort:… · <theme>`), the left pane
lists ADRs (filter by status, search, sort) and the right pane shows a preview —
rendered as GitHub-Flavored Markdown (press `m` to toggle raw source), or an
in-terminal editor when you press `i`. A two-line footer shows the active prompt
or a status message (errors in red) over context-aware key hints. Every read goes
through the shared query layer and every write goes through the same `Store` path
the CLI uses, so the two surfaces never diverge.

Press `?` at any time for an in-app keybinding cheat-sheet; any key dismisses it.

## List & preview

| Key            | Action                                            |
| -------------- | ------------------------------------------------- |
| `j` / `k`      | Move selection down / up (also `↓` / `↑`)         |
| `g` / `G`      | Jump to first / last ADR                          |
| `Enter`        | Focus the preview pane for scrolling              |
| `/`            | Search (title + body, case-insensitive)           |
| `:`            | Open the fuzzy **command palette**                |
| `Ctrl-P`       | **Go to ADR** (fuzzy finder — jump the selection) |
| `f`            | Cycle the status filter (All → each status → All) |
| `o`            | Cycle the sort order                              |
| `n`            | Create a new ADR (prompts for a title)            |
| `s`            | Change the selected ADR's status                  |
| `S`            | Supersede an older ADR (fuzzy-pick it) with the selected one |
| `i`            | **Edit the selected ADR's body in the TUI**       |
| `e`            | Open the selected ADR in `$EDITOR`                |
| `m`            | Toggle the preview between rendered and raw markdown |
| `?`            | Toggle the keybinding help overlay                |
| `r`            | Refresh from disk                                 |
| `q` / `Esc`    | Quit                                              |

The mouse wheel moves the list selection while the list is focused.

### Command palette

Press `:` to open a fuzzy command palette — the discoverable, searchable index
of everything the TUI can do (in the spirit of VS Code / Claude Code). Type to
filter by name (fuzzy, case-insensitive), `↑` / `↓` (or `Ctrl-P` / `Ctrl-N`) to
move, `Enter` to run, `Esc` to cancel. Every command shows its direct keybinding
on the right, so the palette doubles as a way to learn the shortcuts. It includes
the theme switchers (`Theme: gruvbox` / `warm` / `default`) which otherwise have
no key.

### Fuzzy ADR pickers

Two actions open a fuzzy ADR finder instead of asking you to type an identifier:

- **`Ctrl-P` — Go to ADR.** Fuzzy-match by `ADR-NNNN` + title and `Enter` to jump
  the list selection straight to it. Handy in a large log.
- **`S` — Supersede.** Picks the *older* ADR that the currently-selected one
  supersedes, fuzzy-matched the same way (the selected ADR is excluded — it can't
  supersede itself). No more remembering numbers.

Both share the palette's controls: type to filter, `↑`/`↓` (or `Ctrl-P`/`Ctrl-N`)
to move, `Enter` to choose, `Esc` to cancel. The supersede pick still writes
through the same `Store::supersede` path the CLI uses.

### Scrolling the preview

Press `Enter` to focus the preview pane; a scrollbar appears in the right gutter
whenever the body overflows. The mouse wheel scrolls the focused preview.

| Key                  | Action                              |
| -------------------- | ----------------------------------- |
| `j` / `k` (`↓` / `↑`) | Scroll one line down / up           |
| `PageDown` / `PageUp` | Scroll one viewport down / up       |
| `Ctrl-D` / `Ctrl-U`   | Scroll one viewport down / up (vim) |
| `g` / `Home`          | Jump to the top                     |
| `G` / `End`           | Jump to the bottom                  |
| `m`                   | Toggle rendered / raw markdown      |
| `Enter` / `Esc`       | Return to the list                  |

## Markdown rendering & themes

The preview pane renders the ADR body as **GitHub-Flavored Markdown** —
headings, bold/italic, strikethrough, inline code, fenced code blocks, block
quotes, lists, task lists, links, horizontal rules, and tables. Press `m` to
toggle between the rendered view and the raw markdown source (the in-TUI editor,
`i`, always shows raw source — you need it to edit). Fenced code blocks are
**syntax-highlighted** (via [syntect](https://github.com/trishume/syntect)) when
they carry a language tag (e.g. ` ```rust `); the highlight theme tracks the TUI
theme. Untagged blocks render as plain monospace text.

The theme drives the **whole** interface — markdown body, border accents,
selection marker, breadcrumb, and footer — not just the preview. Three themes are
available, selected with `--theme`, the `ADROIT_THEME` environment variable, or
the `tui_theme` config key:

| Theme | Description |
| ----- | ----------- |
| `gruvbox` | True-color gruvbox, matching the house mdBook/doxygen theme. **The default.** |
| `warm` | A single warm-orange accent over warm neutrals (Claude-Code-inspired). |
| `default` | 16-color ANSI palette — respects your terminal's own colors. |

```sh
adroit --theme warm               # this session
# or in ~/.config/adroit/config.yaml:  tui_theme: warm
# or in a .env file:                   ADROIT_THEME=warm
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
