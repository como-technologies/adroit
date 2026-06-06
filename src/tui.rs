//! Interactive ratatui TUI surface for browsing and triaging ADRs.
//!
//! The TUI is split into two layers:
//!
//! * [`TuiState`] — pure, terminal-free state and transitions. All list
//!   filtering, search narrowing, selection movement, mode switching and the
//!   mapping from a key/mode to an [`Action`] intent lives here and is unit
//!   tested without a real terminal.
//! * The render + event loop ([`driver`]) — the thin terminal layer that wires
//!   crossterm + ratatui to [`TuiState`] and executes [`Action`]s.
//!
//! Reads ALWAYS go through [`crate::query`]; writes ALWAYS go through
//! [`Store`]. No file I/O, status rewriting or state derivation is duplicated
//! here — the CLI and TUI share the exact same engine.
//!
//! The whole module is gated behind the `tui` Cargo feature, so a
//! `--no-default-features` build of the core lib + CLI pulls in no ratatui /
//! crossterm and never references the terminal.

use std::io::IsTerminal;

use anyhow::{Context, Result};

use crate::adr::{Adr, Number, Status};
use crate::config::{self, Config, MarkdownTheme};
use crate::format::Format;
use crate::query::{self, Filter, Sort};
use crate::store::{Store, StoreError, StoreOptions};
use crate::view::{AdrDetail, AdrSummary};
use the_other_tui_markdown::{RendererBuilder, Theme as MdTheme, into_text_with_renderer};

/// A pure, terminal-free multi-line plain-text editor buffer.
///
/// Holds the editable body as a `Vec<String>` of lines plus a cursor
/// (`row`, `col`) measured in **characters** (not bytes), so multi-byte UTF-8
/// content behaves. It implements the minimal correct editing surface required
/// by Step 3 — insert/delete characters, newlines, backspace, arrow movement,
/// and Home/End — and nothing more (no undo/redo, selection, or clipboard).
///
/// It is deliberately free of any ratatui / crossterm / [`Store`] types so it
/// can be unit-tested in isolation, mirroring how [`TuiState`] stays pure.
///
/// Invariants: `lines` is never empty (an empty buffer is `[""]`); the cursor
/// always points at a valid position (`row < lines.len()`, `col <=
/// chars_in_line(row)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorBuffer {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

impl Default for EditorBuffer {
    fn default() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
        }
    }
}

impl EditorBuffer {
    /// Build an empty buffer (a single empty line).
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a buffer from text, splitting on `\n`. A trailing newline is
    /// dropped so it does not create a spurious empty final line; round-tripping
    /// via [`to_string`](Self::to_string) is therefore stable. `\r` is stripped
    /// so CRLF input edits cleanly (writes normalize to `\n`).
    ///
    /// Named `from_str` for symmetry with `to_string`; it is infallible and
    /// takes a `&str`, so it deliberately does not implement [`std::str::FromStr`].
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        let normalized = text.replace("\r\n", "\n");
        let trimmed = normalized.strip_suffix('\n').unwrap_or(&normalized);
        let lines: Vec<String> = if trimmed.is_empty() {
            vec![String::new()]
        } else {
            trimmed.split('\n').map(|s| s.to_string()).collect()
        };
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    /// Render the buffer back to a single `\n`-joined string (no trailing
    /// newline — the [`Store`] write path adds exactly one).
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        self.lines.join("\n")
    }

    /// The buffer's lines, for rendering.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Cursor row (0-based line index).
    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    /// Cursor column (0-based, in characters).
    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    /// Number of characters in the current cursor line.
    fn cur_len(&self) -> usize {
        self.lines[self.cursor_row].chars().count()
    }

    /// Byte offset of character index `col` within `line`.
    fn byte_idx(line: &str, col: usize) -> usize {
        line.char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len())
    }

    /// Insert a single character at the cursor, advancing the cursor.
    pub fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.cursor_row];
        let at = Self::byte_idx(line, self.cursor_col);
        line.insert(at, c);
        self.cursor_col += 1;
    }

    /// Split the current line at the cursor, moving the tail to a new line and
    /// placing the cursor at the start of it.
    pub fn insert_newline(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        let at = Self::byte_idx(line, self.cursor_col);
        let tail = line.split_off(at);
        self.lines.insert(self.cursor_row + 1, tail);
        self.cursor_row += 1;
        self.cursor_col = 0;
    }

    /// Delete the character before the cursor. At the start of a line (col 0)
    /// this joins the line onto the end of the previous line. A no-op at the
    /// very start of the buffer.
    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            let remove_at = Self::byte_idx(line, self.cursor_col - 1);
            line.remove(remove_at);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            // Join this line onto the previous one.
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.cur_len();
            self.lines[self.cursor_row].push_str(&current);
        }
    }

    /// Move the cursor left one character, wrapping to the end of the previous
    /// line at a line start.
    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.cur_len();
        }
    }

    /// Move the cursor right one character, wrapping to the start of the next
    /// line at a line end.
    pub fn move_right(&mut self) {
        if self.cursor_col < self.cur_len() {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    /// Move the cursor up one line, clamping the column to the new line length.
    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.cursor_col.min(self.cur_len());
        }
    }

    /// Move the cursor down one line, clamping the column to the new line length.
    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = self.cursor_col.min(self.cur_len());
        }
    }

    /// Move the cursor to the start of the current line.
    pub fn home(&mut self) {
        self.cursor_col = 0;
    }

    /// Move the cursor to the end of the current line.
    pub fn end(&mut self) {
        self.cursor_col = self.cur_len();
    }
}

/// What the user is currently doing — drives key handling and rendering.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    /// Browsing the list (default).
    #[default]
    List,
    /// Typing a free-text search query.
    Search { input: String },
    /// Typing a title for a new ADR.
    NewTitle { input: String },
    /// Picking a target status for the selected ADR.
    PickStatus { index: usize },
    /// Typing the identifier of the OLD ADR the selected one supersedes.
    Supersede { input: String },
    /// Scrolling / focused on the preview pane.
    Preview,
    /// Editing the selected ADR's markdown body in the right pane.
    ///
    /// `address` is the scheme token of the ADR being edited, `dirty` tracks
    /// whether the buffer has diverged from disk (drives the "modified"
    /// indicator + Esc confirm), and `confirm_discard` is set once the user has
    /// pressed Esc on a dirty buffer and we are awaiting a y/n (or second Esc)
    /// decision.
    Edit {
        address: String,
        dirty: bool,
        confirm_discard: bool,
    },
}

/// An intent produced by a key press that the driver must execute.
///
/// Intents that touch the filesystem map directly onto [`Store`] / editor
/// calls, keeping the state layer pure and unit-testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Nothing to do.
    None,
    /// Quit the application.
    Quit,
    /// Re-run the query and refresh the preview.
    Refresh,
    /// Create a new ADR with the given title via the [`Store`] write path.
    Create(String),
    /// Change the selected ADR's status via [`Store::set_status_ref`]. The
    /// `String` is the ADR's scheme addressing token (number/slug/uuid).
    SetStatus(String, Status),
    /// Supersede `old` with the selected ADR (`new`) via [`Store::supersede`].
    /// Both are scheme addressing tokens.
    Supersede { new: String, old: String },
    /// Open the given ADR (by addressing token) in `$EDITOR`.
    Edit(String),
    /// Persist the edited body of an ADR via [`Store::set_body_ref`], then reload
    /// so the preview reflects it. `address` is the ADR's scheme token.
    SaveBody { address: String, body: String },
}

/// Status filter cycled with `f`: `All` plus each [`Status`], in lifecycle order.
const STATUS_CYCLE: [Option<Status>; 6] = [
    None,
    Some(Status::Proposed),
    Some(Status::Accepted),
    Some(Status::Rejected),
    Some(Status::Deprecated),
    Some(Status::Superseded),
];

/// The statuses offered by the status picker, in display order.
pub const STATUSES: [Status; 5] = [
    Status::Proposed,
    Status::Accepted,
    Status::Rejected,
    Status::Deprecated,
    Status::Superseded,
];

/// Pure TUI state: the visible rows, selection, filters and current mode.
///
/// `rows` is always the already-queried, presentation-ready view for the
/// active filter + search; the driver refreshes it via [`TuiState::set_rows`]
/// using [`crate::query`]. The state itself performs no I/O.
#[derive(Debug, Clone, Default)]
pub struct TuiState {
    rows: Vec<AdrSummary>,
    selected: usize,
    status_filter: Option<Status>,
    search: Option<String>,
    sort: Sort,
    mode: Mode,
    preview: Option<AdrDetail>,
    preview_scroll: u16,
    message: Option<String>,
    /// The in-TUI body editor buffer, present only while in [`Mode::Edit`].
    editor: Option<EditorBuffer>,
    /// Vertical scroll offset (top visible line) of the editor pane.
    edit_scroll: usize,
    /// Visible line height of the editor pane (driver-supplied each frame).
    edit_viewport: usize,
    /// Markdown color theme for the rendered preview.
    md_theme: MarkdownTheme,
    /// When true, the preview shows raw markdown source instead of rendered.
    preview_raw: bool,
    /// When true, the keybinding help overlay is shown over everything.
    show_help: bool,
}

impl TuiState {
    /// Build an empty state with default filter/sort.
    pub fn new() -> Self {
        Self::default()
    }

    /// The [`Filter`] describing what the driver should query for.
    pub fn filter(&self) -> Filter {
        Filter {
            status: self.status_filter,
            sort: self.sort,
        }
    }

    /// The active free-text search needle, if any.
    pub fn search(&self) -> Option<&str> {
        self.search.as_deref()
    }

    /// Replace the visible rows (already filtered/sorted by the driver's query),
    /// clamping the selection into range.
    pub fn set_rows(&mut self, rows: Vec<AdrSummary>) {
        self.rows = rows;
        if self.selected >= self.rows.len() {
            self.selected = self.rows.len().saturating_sub(1);
        }
    }

    /// The currently visible rows.
    pub fn visible_rows(&self) -> &[AdrSummary] {
        &self.rows
    }

    /// The index of the selected row, if any rows exist.
    pub fn selected_index(&self) -> Option<usize> {
        (!self.rows.is_empty()).then_some(self.selected)
    }

    /// The selected summary, if any.
    pub fn selected(&self) -> Option<&AdrSummary> {
        self.rows.get(self.selected)
    }

    /// The selected ADR number, if any (rows without a number are skipped).
    pub fn selected_number(&self) -> Option<u32> {
        self.selected().and_then(|s| s.number)
    }

    /// The selected ADR's scheme addressing token (number/slug/uuid), if any —
    /// the scheme-agnostic handle the write actions use.
    pub fn selected_address(&self) -> Option<String> {
        self.selected().map(|s| s.address.clone())
    }

    /// Current mode.
    pub fn mode(&self) -> &Mode {
        &self.mode
    }

    /// The active status filter (`None` == all).
    pub fn status_filter(&self) -> Option<Status> {
        self.status_filter
    }

    /// The active sort order.
    pub fn sort(&self) -> Sort {
        self.sort
    }

    /// The preview detail, if loaded.
    pub fn preview(&self) -> Option<&AdrDetail> {
        self.preview.as_ref()
    }

    /// The preview vertical scroll offset.
    pub fn preview_scroll(&self) -> u16 {
        self.preview_scroll
    }

    /// The markdown theme used for the rendered preview.
    pub fn md_theme(&self) -> MarkdownTheme {
        self.md_theme
    }

    /// Set the markdown theme (from resolved config) for the rendered preview.
    pub fn set_md_theme(&mut self, theme: MarkdownTheme) {
        self.md_theme = theme;
    }

    /// Whether the preview currently shows raw markdown source.
    pub fn preview_raw(&self) -> bool {
        self.preview_raw
    }

    /// Toggle the preview between rendered markdown and raw source.
    pub fn toggle_preview_raw(&mut self) {
        self.preview_raw = !self.preview_raw;
        self.preview_scroll = 0;
    }

    /// Whether the keybinding help overlay is currently shown.
    pub fn show_help(&self) -> bool {
        self.show_help
    }

    /// Toggle the help overlay.
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Dismiss the help overlay (any key closes it).
    pub fn close_help(&mut self) {
        self.show_help = false;
    }

    /// A transient status-bar message, if any.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Set a transient status-bar message.
    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
    }

    /// Store the detail for the selected row (driver-loaded via `query::detail`).
    pub fn set_preview(&mut self, detail: Option<AdrDetail>) {
        self.preview = detail;
        self.preview_scroll = 0;
    }

    // --- selection movement -------------------------------------------------

    /// Move selection down one row.
    pub fn select_next(&mut self) {
        if !self.rows.is_empty() && self.selected + 1 < self.rows.len() {
            self.selected += 1;
        }
    }

    /// Move selection up one row.
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Select the first row.
    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    /// Select the last row.
    pub fn select_last(&mut self) {
        self.selected = self.rows.len().saturating_sub(1);
    }

    // --- filtering / search -------------------------------------------------

    /// Cycle the status filter: All -> Proposed -> ... -> Superseded -> All.
    pub fn cycle_status_filter(&mut self) {
        let pos = STATUS_CYCLE
            .iter()
            .position(|s| *s == self.status_filter)
            .unwrap_or(0);
        self.status_filter = STATUS_CYCLE[(pos + 1) % STATUS_CYCLE.len()];
        self.selected = 0;
    }

    /// Set the status filter directly.
    pub fn apply_filter(&mut self, status: Option<Status>) {
        self.status_filter = status;
        self.selected = 0;
    }

    /// Set (or clear) the free-text search needle. An empty needle clears it.
    pub fn set_search(&mut self, needle: Option<String>) {
        self.search = needle.filter(|s| !s.is_empty());
        self.selected = 0;
    }

    /// Cycle the sort order: NumberAsc -> NumberDesc -> CreatedDesc -> TitleAsc.
    pub fn cycle_sort(&mut self) {
        self.sort = match self.sort {
            Sort::NumberAsc => Sort::NumberDesc,
            Sort::NumberDesc => Sort::CreatedDesc,
            Sort::CreatedDesc => Sort::TitleAsc,
            Sort::TitleAsc => Sort::NumberAsc,
        };
    }

    // --- preview scroll -----------------------------------------------------

    /// Scroll the preview down one line.
    pub fn preview_scroll_down(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_add(1);
    }

    /// Scroll the preview up one line.
    pub fn preview_scroll_up(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_sub(1);
    }

    // --- mode transitions ---------------------------------------------------

    /// Enter search-input mode, seeding with any current needle.
    pub fn begin_search(&mut self) {
        self.mode = Mode::Search {
            input: self.search.clone().unwrap_or_default(),
        };
    }

    /// Enter new-ADR title-input mode.
    pub fn begin_new(&mut self) {
        self.mode = Mode::NewTitle {
            input: String::new(),
        };
    }

    /// Enter the status picker for the selected ADR (no-op if no selection).
    pub fn begin_pick_status(&mut self) {
        if self.selected_address().is_some() {
            self.mode = Mode::PickStatus { index: 0 };
        }
    }

    /// Enter supersede-input mode for the selected ADR (no-op if no selection).
    pub fn begin_supersede(&mut self) {
        if self.selected_address().is_some() {
            self.mode = Mode::Supersede {
                input: String::new(),
            };
        }
    }

    /// Focus the preview pane for scrolling.
    pub fn focus_preview(&mut self) {
        self.mode = Mode::Preview;
    }

    /// Return to list mode, discarding any in-progress input.
    pub fn back_to_list(&mut self) {
        self.mode = Mode::List;
    }

    /// Append a character to the active text-input mode.
    pub fn push_char(&mut self, c: char) {
        match &mut self.mode {
            // Supersede now takes a scheme identifier (number, slug, or uuid),
            // so it accepts the same free text as the other input modes.
            Mode::Search { input } | Mode::NewTitle { input } | Mode::Supersede { input } => {
                input.push(c)
            }
            _ => {}
        }
    }

    /// Remove the last character from the active text-input mode.
    pub fn pop_char(&mut self) {
        match &mut self.mode {
            Mode::Search { input } | Mode::NewTitle { input } | Mode::Supersede { input } => {
                input.pop();
            }
            _ => {}
        }
    }

    /// Move the status-picker cursor down.
    pub fn picker_next(&mut self) {
        if let Mode::PickStatus { index } = &mut self.mode
            && *index + 1 < STATUSES.len()
        {
            *index += 1;
        }
    }

    /// Move the status-picker cursor up.
    pub fn picker_prev(&mut self) {
        if let Mode::PickStatus { index } = &mut self.mode {
            *index = index.saturating_sub(1);
        }
    }

    /// Confirm the current input/picker, returning the [`Action`] to perform
    /// and resetting to list mode.
    pub fn confirm(&mut self) -> Action {
        let action = match &self.mode {
            Mode::Search { input } => {
                self.set_search(Some(input.clone()));
                Action::Refresh
            }
            Mode::NewTitle { input } => {
                let title = input.trim().to_string();
                if title.is_empty() {
                    Action::None
                } else {
                    Action::Create(title)
                }
            }
            Mode::PickStatus { index } => match self.selected_address() {
                Some(addr) => Action::SetStatus(addr, STATUSES[*index]),
                None => Action::None,
            },
            Mode::Supersede { input } => {
                let old = input.trim();
                match self.selected_address() {
                    Some(new) if !old.is_empty() => Action::Supersede {
                        new,
                        old: old.to_string(),
                    },
                    _ => Action::None,
                }
            }
            _ => Action::None,
        };
        self.mode = Mode::List;
        action
    }

    // --- body editor --------------------------------------------------------

    /// The active editor buffer, if in edit mode.
    pub fn editor(&self) -> Option<&EditorBuffer> {
        self.editor.as_ref()
    }

    /// The editor pane's top visible line.
    pub fn edit_scroll(&self) -> usize {
        self.edit_scroll
    }

    /// True while editing with unsaved changes.
    pub fn is_dirty(&self) -> bool {
        matches!(self.mode, Mode::Edit { dirty: true, .. })
    }

    /// Enter body-edit mode for the selected ADR, seeding the buffer from the
    /// loaded preview body. No-op if there is no selection or no preview loaded.
    pub fn begin_edit(&mut self) {
        let Some(address) = self.selected_address() else {
            return;
        };
        let Some(detail) = &self.preview else {
            return;
        };
        self.editor = Some(EditorBuffer::from_str(&detail.body));
        self.edit_scroll = 0;
        self.mode = Mode::Edit {
            address,
            dirty: false,
            confirm_discard: false,
        };
    }

    /// Leave edit mode, dropping the buffer and returning to the list.
    fn exit_edit(&mut self) {
        self.editor = None;
        self.edit_scroll = 0;
        self.mode = Mode::List;
    }

    /// Mark the buffer dirty and clear any pending discard confirmation
    /// (called after every mutating edit keystroke).
    fn mark_dirty(&mut self) {
        if let Mode::Edit {
            dirty,
            confirm_discard,
            ..
        } = &mut self.mode
        {
            *dirty = true;
            *confirm_discard = false;
        }
    }

    /// Apply a mutating edit op to the buffer (if in edit mode), keeping the
    /// cursor visible and flagging the buffer dirty.
    fn edit_mutate(&mut self, op: impl FnOnce(&mut EditorBuffer)) {
        let Some(buf) = self.editor.as_mut() else {
            return;
        };
        op(buf);
        // `buf`'s borrow ends here; now safe to touch other `self` state.
        self.mark_dirty();
        self.keep_cursor_visible();
    }

    /// Apply a non-mutating cursor movement to the buffer (if in edit mode),
    /// keeping the cursor visible without flagging dirty.
    fn edit_move(&mut self, op: impl FnOnce(&mut EditorBuffer)) {
        let Some(buf) = self.editor.as_mut() else {
            return;
        };
        op(buf);
        self.keep_cursor_visible();
    }

    /// Insert a character into the edit buffer.
    pub fn edit_insert_char(&mut self, c: char) {
        self.edit_mutate(|b| b.insert_char(c));
    }

    /// Insert a newline into the edit buffer.
    pub fn edit_newline(&mut self) {
        self.edit_mutate(|b| b.insert_newline());
    }

    /// Backspace in the edit buffer.
    pub fn edit_backspace(&mut self) {
        self.edit_mutate(|b| b.backspace());
    }

    /// Move the edit cursor left.
    pub fn edit_left(&mut self) {
        self.edit_move(|b| b.move_left());
    }

    /// Move the edit cursor right.
    pub fn edit_right(&mut self) {
        self.edit_move(|b| b.move_right());
    }

    /// Move the edit cursor up.
    pub fn edit_up(&mut self) {
        self.edit_move(|b| b.move_up());
    }

    /// Move the edit cursor down.
    pub fn edit_down(&mut self) {
        self.edit_move(|b| b.move_down());
    }

    /// Move the edit cursor to the start of its line.
    pub fn edit_home(&mut self) {
        self.edit_move(|b| b.home());
    }

    /// Move the edit cursor to the end of its line.
    pub fn edit_end(&mut self) {
        self.edit_move(|b| b.end());
    }

    /// Move the edit cursor to the very end of the buffer (last line, last col).
    pub fn edit_down_to_end(&mut self) {
        self.edit_move(|b| {
            while b.cursor_row() + 1 < b.lines().len() {
                b.move_down();
            }
            b.end();
        });
    }

    /// The visible height (in lines) of the editor pane, set by the driver each
    /// frame so [`keep_cursor_visible`](Self::keep_cursor_visible) can scroll.
    /// Defaults conservatively when never set.
    fn keep_cursor_visible(&mut self) {
        // Use a stored viewport height when available; the driver updates it via
        // `set_edit_viewport`. We keep the cursor row within
        // [edit_scroll, edit_scroll + height).
        let row = self.editor.as_ref().map(|b| b.cursor_row()).unwrap_or(0);
        let height = self.edit_viewport.max(1);
        if row < self.edit_scroll {
            self.edit_scroll = row;
        } else if row >= self.edit_scroll + height {
            self.edit_scroll = row + 1 - height;
        }
    }

    /// Record the editor pane's visible line height (driver-supplied each frame).
    pub fn set_edit_viewport(&mut self, height: usize) {
        self.edit_viewport = height.max(1);
        self.keep_cursor_visible();
    }

    /// Produce the [`Action::SaveBody`] for the current edit buffer and clear
    /// the dirty flag (the driver applies the save). No-op outside edit mode.
    pub fn save_edit(&mut self) -> Action {
        let Mode::Edit { address, .. } = &self.mode else {
            return Action::None;
        };
        let address = address.clone();
        let Some(buf) = &self.editor else {
            return Action::None;
        };
        let body = buf.to_string();
        if let Mode::Edit {
            dirty,
            confirm_discard,
            ..
        } = &mut self.mode
        {
            *dirty = false;
            *confirm_discard = false;
        }
        Action::SaveBody { address, body }
    }

    /// Handle Esc in edit mode: cancel immediately if clean, otherwise arm a
    /// discard confirmation. Returns `true` if the editor was exited.
    pub fn request_cancel_edit(&mut self) -> bool {
        match &mut self.mode {
            Mode::Edit { dirty: false, .. } => {
                self.exit_edit();
                true
            }
            Mode::Edit {
                confirm_discard, ..
            } => {
                if *confirm_discard {
                    self.exit_edit();
                    true
                } else {
                    *confirm_discard = true;
                    false
                }
            }
            _ => false,
        }
    }

    /// Confirm discarding unsaved edits (the `y` answer / second Esc).
    pub fn confirm_discard_edit(&mut self) {
        if matches!(self.mode, Mode::Edit { .. }) {
            self.exit_edit();
        }
    }

    /// Cancel a pending discard confirmation (the `n` answer), staying in edit
    /// mode with the buffer intact.
    pub fn cancel_discard_edit(&mut self) {
        if let Mode::Edit {
            confirm_discard, ..
        } = &mut self.mode
        {
            *confirm_discard = false;
        }
    }

    /// True while awaiting a discard y/n decision.
    pub fn awaiting_discard_confirm(&self) -> bool {
        matches!(
            self.mode,
            Mode::Edit {
                confirm_discard: true,
                ..
            }
        )
    }
}

/// Open the store the TUI operates on, at the already-resolved ADR `dir`.
///
/// This is the seam the binary and the tests share: `main.rs` resolves the dir
/// from `--dir`/config exactly once and hands it here, so the TUI never
/// re-resolves with `None` (which previously ignored `--dir`). The store options
/// (format/layout/status dirs) still come from `config`, via the one shared
/// [`StoreOptions::from_config`] mapping.
pub fn open_store(config: &Config, dir: &std::path::Path) -> Result<Store, StoreError> {
    Store::open_or_create_with(dir, StoreOptions::from_config(config))
}

/// Launch the interactive TUI against the resolved ADR `dir`.
///
/// `dir` is the directory already resolved by the binary from `--dir`/config, so
/// `adroit --dir X` and the no-subcommand TUI open the same store (mirrors how
/// `serve` is threaded the resolved dir).
///
/// In a non-interactive context (stdin is not a TTY — CI, pipes, the
/// integration tests) this prints a short hint and returns instead of trying
/// to seize a real terminal, so tests never hang.
pub fn run(config: &Config, dir: &std::path::Path) -> Result<()> {
    if !std::io::stdin().is_terminal() {
        println!(
            "adroit TUI requires an interactive terminal. \
             Run `adroit` in a TTY, or use the CLI subcommands (try `adroit --help`)."
        );
        return Ok(());
    }
    let store = open_store(config, dir)?;
    driver::run(config, &store)
}

/// Load the rows for the current filter into `state`, then refresh the preview.
///
/// Search is title+body (`query::search`); otherwise plain summaries. The
/// status filter applies in both cases. Reads only — never writes.
fn reload(state: &mut TuiState, store: &Store) -> Result<(), query::QueryError> {
    let filter = state.filter();
    let rows = match state.search() {
        Some(needle) => {
            let mut rows = query::search(store, needle)?;
            if let Some(status) = filter.status {
                rows.retain(|r| r.status == status);
            }
            sort_in_place(&mut rows, filter.sort);
            rows
        }
        None => query::summaries(store, &filter)?,
    };
    state.set_rows(rows);
    refresh_preview(state, store)
}

/// Apply the active sort to search results (which `query::search` returns in
/// number-ascending order) so search and list share one ordering.
fn sort_in_place(rows: &mut [AdrSummary], sort: Sort) {
    match sort {
        Sort::NumberAsc => rows.sort_by_key(|a| a.number),
        Sort::NumberDesc => rows.sort_by_key(|a| std::cmp::Reverse(a.number)),
        // `created` is `Option<String>` (not `Copy`); reverse via the comparator
        // to avoid cloning the key per element.
        Sort::CreatedDesc => rows.sort_by(|a, b| b.created.cmp(&a.created)),
        Sort::TitleAsc => rows.sort_by_key(|a| a.title.to_lowercase()),
    }
}

/// Load detail for the currently selected row into the preview pane.
fn refresh_preview(state: &mut TuiState, store: &Store) -> Result<(), query::QueryError> {
    match state.selected_number() {
        Some(num) => state.set_preview(Some(query::detail(store, num)?)),
        None => state.set_preview(None),
    }
    Ok(())
}

/// Create a new ADR through the shared [`Store`] write path, mirroring the
/// CLI's `new` (template-rendered markdown body, default status).
fn create_adr(store: &Store, cfg: &Config, title: &str) -> Result<Adr> {
    let mut adr = Adr::new(title)?;
    adr.status = cfg.default_status;
    let r = store.next_ref(title, adr.id.uuid())?;
    crate::store::apply_ref_pub(&mut adr, &r);

    if store.options().format == Format::Markdown {
        let name = &cfg.default_template;
        let text = crate::template::resolve(name, cfg.templates_dir.as_deref(), store.root())
            .with_context(|| format!("could not resolve template '{name}'"))?;
        let date = adr.created.to_string();
        let date = date.get(..10).unwrap_or(&date);
        adr.body = crate::template::render(&text, cfg.naming, &r, title, cfg.default_status, date);
    }
    store.write(&mut adr)?;
    Ok(adr)
}

/// Execute an [`Action`] against the shared [`Store`], then reload state.
///
/// Returns `Ok(true)` when the app should quit. `Action::Edit` is a no-op here
/// — the driver handles editor spawning (it needs to suspend the terminal);
/// this keeps `apply_action` headless and directly unit-testable against a
/// tempdir-backed `Store`.
/// Resolve a TUI addressing token into an [`AdrRef`] under the configured scheme.
fn resolve_addr(cfg: &Config, addr: &str) -> Option<crate::naming::AdrRef> {
    cfg.naming.parse_ref(addr)
}

fn apply_action(state: &mut TuiState, store: &Store, cfg: &Config, action: Action) -> Result<bool> {
    match action {
        Action::None | Action::Edit(_) => {}
        Action::Quit => return Ok(true),
        Action::Refresh => reload(state, store)?,
        Action::Create(title) => match create_adr(store, cfg, &title) {
            Ok(adr) => {
                let n = adr.number.map(Number::get).unwrap_or(0);
                state.set_message(format!("Created ADR {n:04}: {}", adr.title));
                reload(state, store)?;
            }
            Err(e) => state.set_message(format!("create failed: {e}")),
        },
        Action::SetStatus(addr, status) => match resolve_addr(cfg, &addr) {
            Some(r) => match store.set_status_ref(&r, status) {
                Ok(_) => {
                    state.set_message(format!("{} -> {status}", cfg.naming.display(&r)));
                    reload(state, store)?;
                }
                Err(e) => state.set_message(format!("status change failed: {e}")),
            },
            None => state.set_message(format!("invalid ADR id '{addr}'")),
        },
        Action::Supersede { new, old } => {
            match (resolve_addr(cfg, &new), resolve_addr(cfg, &old)) {
                (Some(new_r), Some(old_r)) => match store.supersede(&new_r, &old_r) {
                    Ok(_) => {
                        state.set_message(format!(
                            "{} superseded by {}",
                            cfg.naming.display(&old_r),
                            cfg.naming.display(&new_r)
                        ));
                        reload(state, store)?;
                    }
                    Err(e) => state.set_message(format!("supersede failed: {e}")),
                },
                _ => state.set_message("invalid ADR id".to_string()),
            }
        }
        Action::SaveBody { address, body } => match resolve_addr(cfg, &address) {
            Some(r) => match store.set_body_ref(&r, &body) {
                Ok(_) => {
                    state.set_message(format!("Saved {}", cfg.naming.display(&r)));
                    // Refresh the preview so it reflects the saved body.
                    refresh_preview(state, store)?;
                }
                Err(e) => state.set_message(format!("save failed: {e}")),
            },
            None => state.set_message(format!("invalid ADR id '{address}'")),
        },
    }
    Ok(false)
}

mod driver {
    use super::*;
    use crossterm::{
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
        execute,
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    };
    use ratatui::{
        prelude::*,
        widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    };
    use std::io::{Stdout, stdout};
    use std::time::Duration;

    type Term = Terminal<CrosstermBackend<Stdout>>;

    pub fn run(config: &Config, store: &Store) -> Result<()> {
        let mut state = TuiState::new();
        // Apply the configured theme (`--theme` / `ADROIT_THEME` / config) — this
        // drives both the chrome palette and the markdown preview.
        state.set_md_theme(config.tui_theme);
        reload(&mut state, store)?;

        let mut terminal = setup()?;
        let res = event_loop(&mut terminal, &mut state, store, config);
        teardown(&mut terminal)?;
        res
    }

    fn setup() -> Result<Term> {
        enable_raw_mode()?;
        let mut out = stdout();
        execute!(out, EnterAlternateScreen)?;
        Ok(Terminal::new(CrosstermBackend::new(out))?)
    }

    fn teardown(terminal: &mut Term) -> Result<()> {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        Ok(())
    }

    fn event_loop(
        terminal: &mut Term,
        state: &mut TuiState,
        store: &Store,
        config: &Config,
    ) -> Result<()> {
        loop {
            terminal.draw(|f| ui(f, state))?;
            // (state is reborrowed mutably by `ui` for viewport bookkeeping)
            if !event::poll(Duration::from_millis(200))? {
                continue;
            }
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                let action = handle_key(state, key);
                // The editor needs the terminal suspended; everything else goes
                // through the shared, headless `apply_action`.
                if let Action::Edit(addr) = action {
                    run_editor(terminal, state, store, config, &addr)?;
                    continue;
                }
                if apply_action(state, store, config, action)? {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Suspend the TUI, run `$EDITOR` on the ADR, then resume and reload.
    /// Reuses the binary's editor resolution (`config::resolve_editor`).
    fn run_editor(
        terminal: &mut Term,
        state: &mut TuiState,
        store: &Store,
        config: &Config,
        addr: &str,
    ) -> Result<()> {
        let Some(r) = resolve_addr(config, addr) else {
            state.set_message(format!("invalid ADR id '{addr}'"));
            return Ok(());
        };
        let path = match store.find_path_by_ref(&r) {
            Ok(p) => p,
            Err(e) => {
                state.set_message(format!("{} not found: {e}", config.naming.display(&r)));
                return Ok(());
            }
        };
        let label = config.naming.display(&r);
        teardown(terminal)?;
        // Resolve the editor the same way the CLI does (VISUAL/EDITOR > config
        // > auto-detect). `resolve_editor` may mutate config (caching a choice),
        // so work on a clone.
        let mut cfg: Config = config.clone();
        let result = match config::resolve_editor(&mut cfg) {
            Ok(Some(cmd)) => spawn_editor(&cmd, &path),
            Ok(None) => edit::edit_file(&path).context("editor failed"),
            Err(e) => Err(anyhow::anyhow!(e)),
        };
        *terminal = setup()?;
        terminal.clear()?;
        match result {
            Ok(()) => state.set_message(format!("edited {label}")),
            Err(e) => state.set_message(format!("editor failed: {e}")),
        }
        reload(state, store)?;
        Ok(())
    }

    /// Spawn an explicit editor command (may include flags, e.g. `code --wait`).
    fn spawn_editor(cmd: &str, path: &std::path::Path) -> Result<()> {
        let mut parts = cmd.split_whitespace();
        let bin = parts.next().context("editor command is empty")?;
        let exit = std::process::Command::new(bin)
            .args(parts)
            .arg(path)
            .status()
            .with_context(|| format!("failed to launch editor '{cmd}'"))?;
        if !exit.success() {
            anyhow::bail!("editor exited with {exit}");
        }
        Ok(())
    }

    /// Map a key press (in the current mode) to an [`Action`], mutating state
    /// for navigation/input that has no filesystem effect.
    fn handle_key(state: &mut TuiState, key: KeyEvent) -> Action {
        // The help overlay swallows the next key (any key dismisses it).
        if state.show_help() {
            state.close_help();
            return Action::None;
        }
        match state.mode().clone() {
            Mode::List => handle_list_key(state, key),
            Mode::Preview => handle_preview_key(state, key),
            Mode::PickStatus { .. } => handle_picker_key(state, key),
            Mode::Edit { .. } => handle_edit_key(state, key),
            Mode::Search { .. } | Mode::NewTitle { .. } | Mode::Supersede { .. } => {
                handle_input_key(state, key)
            }
        }
    }

    fn handle_list_key(state: &mut TuiState, key: KeyEvent) -> Action {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                state.select_next();
                Action::Refresh
            }
            KeyCode::Char('k') | KeyCode::Up => {
                state.select_prev();
                Action::Refresh
            }
            KeyCode::Char('g') => {
                state.select_first();
                Action::Refresh
            }
            KeyCode::Char('G') => {
                state.select_last();
                Action::Refresh
            }
            KeyCode::Enter => {
                state.focus_preview();
                Action::None
            }
            KeyCode::Char('/') => {
                state.begin_search();
                Action::None
            }
            KeyCode::Char('f') => {
                state.cycle_status_filter();
                Action::Refresh
            }
            KeyCode::Char('o') => {
                state.cycle_sort();
                Action::Refresh
            }
            KeyCode::Char('n') => {
                state.begin_new();
                Action::None
            }
            KeyCode::Char('s') if !shift => {
                state.begin_pick_status();
                Action::None
            }
            KeyCode::Char('S') => {
                state.begin_supersede();
                Action::None
            }
            KeyCode::Char('e') => match state.selected_address() {
                Some(addr) => Action::Edit(addr),
                None => Action::None,
            },
            KeyCode::Char('i') => {
                state.begin_edit();
                Action::None
            }
            KeyCode::Char('m') => {
                state.toggle_preview_raw();
                Action::None
            }
            KeyCode::Char('?') => {
                state.toggle_help();
                Action::None
            }
            KeyCode::Char('r') => Action::Refresh,
            _ => Action::None,
        }
    }

    fn handle_preview_key(state: &mut TuiState, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Esc | KeyCode::Enter => {
                state.back_to_list();
                Action::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                state.preview_scroll_down();
                Action::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                state.preview_scroll_up();
                Action::None
            }
            KeyCode::Char('m') => {
                state.toggle_preview_raw();
                Action::None
            }
            _ => Action::None,
        }
    }

    /// Edit-mode keys. While awaiting a discard confirmation only y/n/Esc are
    /// meaningful; otherwise this is a full plain-text editor with Ctrl-S save.
    fn handle_edit_key(state: &mut TuiState, key: KeyEvent) -> Action {
        // Discard confirmation prompt takes priority.
        if state.awaiting_discard_confirm() {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Esc => {
                    state.confirm_discard_edit();
                    return Action::Refresh;
                }
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    state.cancel_discard_edit();
                    return Action::None;
                }
                _ => return Action::None,
            }
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Ctrl-S saves through the Store write path.
            KeyCode::Char('s') if ctrl => state.save_edit(),
            KeyCode::Esc => {
                // Clean -> exit immediately (refresh restores list view);
                // dirty -> arms the discard confirmation.
                if state.request_cancel_edit() {
                    Action::Refresh
                } else {
                    Action::None
                }
            }
            KeyCode::Enter => {
                state.edit_newline();
                Action::None
            }
            KeyCode::Backspace => {
                state.edit_backspace();
                Action::None
            }
            KeyCode::Left => {
                state.edit_left();
                Action::None
            }
            KeyCode::Right => {
                state.edit_right();
                Action::None
            }
            KeyCode::Up => {
                state.edit_up();
                Action::None
            }
            KeyCode::Down => {
                state.edit_down();
                Action::None
            }
            KeyCode::Home => {
                state.edit_home();
                Action::None
            }
            KeyCode::End => {
                state.edit_end();
                Action::None
            }
            KeyCode::Tab => {
                // Insert spaces for a tab (keeps the buffer plain + predictable).
                for _ in 0..4 {
                    state.edit_insert_char(' ');
                }
                Action::None
            }
            // Plain typed characters (ignore other control chords).
            KeyCode::Char(c) if !ctrl => {
                state.edit_insert_char(c);
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_picker_key(state: &mut TuiState, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                state.back_to_list();
                Action::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                state.picker_next();
                Action::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                state.picker_prev();
                Action::None
            }
            KeyCode::Enter => state.confirm(),
            _ => Action::None,
        }
    }

    fn handle_input_key(state: &mut TuiState, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                state.back_to_list();
                Action::None
            }
            KeyCode::Enter => state.confirm(),
            KeyCode::Backspace => {
                state.pop_char();
                Action::None
            }
            KeyCode::Char(c) => {
                state.push_char(c);
                Action::None
            }
            _ => Action::None,
        }
    }

    // --- rendering ----------------------------------------------------------

    /// Render a frame. Takes `&mut TuiState` because the editor pane reports its
    /// visible height back into the state (so cursor-follow scrolling knows the
    /// viewport); only that bookkeeping field is mutated.
    fn ui(f: &mut Frame, state: &mut TuiState) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(f.area());

        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[0]);

        render_list(f, state, panes[0]);
        if matches!(state.mode(), Mode::Edit { .. }) {
            render_editor(f, state, panes[1]);
        } else {
            render_preview(f, state, panes[1]);
        }
        render_footer(f, state, chunks[1]);

        if let Mode::PickStatus { .. } = state.mode() {
            render_status_picker(f, state, chunks[0]);
        }
        if state.show_help() {
            render_help(f, state, f.area());
        }
    }

    /// The `?` keybinding cheat-sheet, a centered overlay grouped by task.
    fn render_help(f: &mut Frame, state: &TuiState, area: Rect) {
        let c = chrome(state.md_theme());
        let sect = |s: &str| {
            Line::from(Span::styled(
                s.to_string(),
                Style::default().fg(c.title).add_modifier(Modifier::BOLD),
            ))
        };
        let row = |k: &str, d: &str| {
            Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{k:<9}"),
                    Style::default().fg(c.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(d.to_string(), Style::default().fg(c.muted)),
            ])
        };
        let lines = vec![
            sect("Navigate"),
            row("j / k", "move selection (or ↑ / ↓)"),
            row("g / G", "first / last"),
            row("Enter", "focus the preview pane"),
            Line::from(""),
            sect("Find"),
            row("/", "search title + body"),
            row("f", "cycle status filter"),
            row("o", "cycle sort order"),
            row("r", "refresh"),
            Line::from(""),
            sect("Author"),
            row("n", "new ADR"),
            row("s", "set status"),
            row("S", "supersede (this supersedes …)"),
            row("i", "edit body in-terminal"),
            row("e", "open in $EDITOR"),
            Line::from(""),
            sect("Preview"),
            row("j / k", "scroll"),
            row("m", "toggle rendered / raw"),
            row("Esc", "back to list"),
            Line::from(""),
            sect("General"),
            row("?", "toggle this help"),
            row("q", "quit"),
        ];
        let height = lines.len() as u16 + 2;
        let popup = centered(46, height.min(area.height), area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(c.accent))
            .title(Span::styled(
                " Keybindings — any key to close ",
                Style::default().fg(c.title).add_modifier(Modifier::BOLD),
            ));
        f.render_widget(Clear, popup);
        f.render_widget(Paragraph::new(lines).block(block), popup);
    }

    /// Render the in-TUI body editor in the right pane and place the terminal
    /// cursor. Reports the inner height back to `state` for scroll-follow.
    fn render_editor(f: &mut Frame, state: &mut TuiState, area: Rect) {
        let title = match state.mode() {
            Mode::Edit { address, dirty, .. } => {
                let flag = if *dirty { " [modified]" } else { "" };
                format!(" Edit {address}{flag} ")
            }
            _ => " Edit ".to_string(),
        };
        let c = chrome(state.md_theme());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(c.accent))
            .title(Span::styled(title, Style::default().fg(c.title)));
        let inner = block.inner(area);
        f.render_widget(block, area);

        // The inner text area height drives cursor-follow scrolling.
        let height = inner.height as usize;
        state.set_edit_viewport(height);

        let Some(buf) = state.editor() else {
            return;
        };
        let top = state.edit_scroll();
        let visible: Vec<Line> = buf
            .lines()
            .iter()
            .skip(top)
            .take(height)
            .map(|l| Line::from(l.clone()))
            .collect();
        let para = Paragraph::new(visible);
        f.render_widget(para, inner);

        // Place the hardware cursor (clamped to the visible region). Column is
        // measured in characters; convert to a display column conservatively
        // (1:1 — adequate for ASCII/markdown bodies).
        let cursor_row = buf.cursor_row();
        if cursor_row >= top && cursor_row < top + height {
            let rel_row = (cursor_row - top) as u16;
            let col = buf.cursor_col() as u16;
            let max_col = inner.width.saturating_sub(1);
            f.set_cursor_position((inner.x + col.min(max_col), inner.y + rel_row));
        }
    }

    fn render_list(f: &mut Frame, state: &TuiState, area: Rect) {
        let filter_label = match state.status_filter() {
            Some(s) => s.to_string(),
            None => "All".to_string(),
        };
        let title = format!(
            " ADRs [{}/{}]  filter:{}  sort:{} ",
            state.selected_index().map(|i| i + 1).unwrap_or(0),
            state.visible_rows().len(),
            filter_label,
            sort_label(state.sort()),
        );
        let c = chrome(state.md_theme());
        let items: Vec<ListItem> = state
            .visible_rows()
            .iter()
            .map(|s| {
                let num = Span::styled(
                    format!("{:<5}", s.number_display),
                    Style::default().fg(c.muted),
                );
                let status = Span::styled(
                    format!("{:<11}", s.status),
                    Style::default().fg(status_color(s.status)),
                );
                let line = Line::from(vec![num, status, Span::raw(s.title.clone())]);
                ListItem::new(line)
            })
            .collect();

        let mut list_state = ListState::default();
        list_state.select(state.selected_index());

        // The list is the primary pane: accent border unless the preview/editor
        // has focus.
        let focused = matches!(state.mode(), Mode::List);
        let border = if focused { c.accent } else { c.border };
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border))
                    .title(Span::styled(title, Style::default().fg(c.title))),
            )
            .highlight_style(
                Style::default()
                    .bg(c.selection_bg)
                    .fg(c.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        f.render_stateful_widget(list, area, &mut list_state);
    }

    /// The gruvbox (true-color) markdown theme. Starts from the crate default
    /// and overrides the styled elements, so any field we don't set keeps a
    /// sane fallback.
    fn gruvbox_theme() -> MdTheme {
        let fg = Color::Rgb(235, 219, 178); // ebdbb2
        let gray = Color::Rgb(146, 131, 116); // 928374
        let orange = Color::Rgb(254, 128, 25); // fe8019
        let yellow = Color::Rgb(250, 189, 47); // fabd2f
        let green = Color::Rgb(184, 187, 38); // b8bb26
        let aqua = Color::Rgb(142, 192, 124); // 8ec07c
        let blue = Color::Rgb(131, 165, 152); // 83a598
        let bold = Modifier::BOLD;
        MdTheme {
            h1: Style::new().fg(orange).add_modifier(bold),
            h2: Style::new().fg(yellow).add_modifier(bold),
            h3: Style::new().fg(green).add_modifier(bold),
            h4: Style::new().fg(aqua).add_modifier(bold),
            h5: Style::new().fg(aqua).add_modifier(bold),
            h6: Style::new().fg(aqua).add_modifier(bold),
            strong: Style::new().fg(fg).add_modifier(bold),
            emphasis: Style::new().fg(fg).add_modifier(Modifier::ITALIC),
            strikethrough: Style::new().fg(gray).add_modifier(Modifier::CROSSED_OUT),
            inline_code: Style::new().fg(green),
            code_block: Style::new().fg(aqua),
            block_quote: Style::new().fg(gray).add_modifier(Modifier::ITALIC),
            link: Style::new().fg(blue).add_modifier(Modifier::UNDERLINED),
            list_marker: Style::new().fg(orange),
            table_header: Style::new().fg(yellow).add_modifier(bold),
            rule: Style::new().fg(gray),
            ..MdTheme::default()
        }
    }

    /// The warm, Claude-Code-style markdown theme: one orange accent on warm
    /// neutrals, headings in amber/orange.
    fn warm_theme() -> MdTheme {
        let fg = Color::Rgb(212, 190, 152); // d4be98 warm parchment
        let muted = Color::Rgb(124, 111, 100); // 7c6f64
        let accent = Color::Rgb(254, 128, 25); // fe8019 — the one accent
        let amber = Color::Rgb(232, 167, 78); // e8a74e
        let soft = Color::Rgb(216, 166, 87); // d8a657
        let bold = Modifier::BOLD;
        MdTheme {
            h1: Style::new().fg(accent).add_modifier(bold),
            h2: Style::new().fg(amber).add_modifier(bold),
            h3: Style::new().fg(soft).add_modifier(bold),
            h4: Style::new().fg(soft).add_modifier(bold),
            h5: Style::new().fg(soft).add_modifier(bold),
            h6: Style::new().fg(soft).add_modifier(bold),
            strong: Style::new().fg(fg).add_modifier(bold),
            emphasis: Style::new().fg(fg).add_modifier(Modifier::ITALIC),
            strikethrough: Style::new().fg(muted).add_modifier(Modifier::CROSSED_OUT),
            inline_code: Style::new().fg(amber),
            code_block: Style::new().fg(amber),
            block_quote: Style::new().fg(muted).add_modifier(Modifier::ITALIC),
            link: Style::new().fg(accent).add_modifier(Modifier::UNDERLINED),
            list_marker: Style::new().fg(accent),
            table_header: Style::new().fg(amber).add_modifier(bold),
            rule: Style::new().fg(muted),
            ..MdTheme::default()
        }
    }

    /// The TUI chrome palette (borders, selection, titles, hints) for a theme.
    /// Centralizes every chrome color so the whole UI re-skins from one place.
    pub(super) struct Chrome {
        /// The single accent (focused border, active key hints, selection text).
        pub accent: Color,
        /// Muted text — inactive hints, the footer, secondary metadata.
        pub muted: Color,
        /// Unfocused pane border.
        pub border: Color,
        /// Selected-row background.
        pub selection_bg: Color,
        /// Pane / section titles.
        pub title: Color,
    }

    /// Resolve the chrome palette for a theme. `Default` uses ANSI named colors
    /// (respects the terminal); gruvbox/warm use true-color.
    pub(super) fn chrome(theme: MarkdownTheme) -> Chrome {
        match theme {
            MarkdownTheme::Gruvbox => Chrome {
                accent: Color::Rgb(254, 128, 25),     // fe8019 orange
                muted: Color::Rgb(146, 131, 116),     // 928374
                border: Color::Rgb(102, 92, 84),      // 665c54
                selection_bg: Color::Rgb(60, 56, 54), // 3c3836
                title: Color::Rgb(250, 189, 47),      // fabd2f
            },
            MarkdownTheme::Warm => Chrome {
                accent: Color::Rgb(254, 128, 25),     // fe8019
                muted: Color::Rgb(124, 111, 100),     // 7c6f64
                border: Color::Rgb(80, 73, 69),       // 504945
                selection_bg: Color::Rgb(60, 56, 54), // 3c3836
                title: Color::Rgb(232, 167, 78),      // e8a74e amber
            },
            MarkdownTheme::Default => Chrome {
                accent: Color::Cyan,
                muted: Color::DarkGray,
                border: Color::DarkGray,
                selection_bg: Color::Blue,
                title: Color::Cyan,
            },
        }
    }

    /// Render an ADR body to a themed ratatui `Text` (GitHub-Flavored Markdown).
    ///
    /// The crate's default heading renderer keeps the literal `#`/`##` prefix,
    /// which makes the preview read like raw source. We override it to drop the
    /// hashes and carry heading hierarchy through the theme's per-level styling
    /// (bold/underline/color) instead. Bullets (`•`) and block-quote glyphs
    /// (`▌`) already render nicely, so they keep the crate defaults.
    pub(super) fn render_markdown_body(body: &str, theme: MarkdownTheme) -> Text<'static> {
        let md_theme = match theme {
            MarkdownTheme::Default => MdTheme::default(),
            MarkdownTheme::Gruvbox => gruvbox_theme(),
            MarkdownTheme::Warm => warm_theme(),
        };
        let renderer = RendererBuilder::new()
            .with_theme(md_theme)
            // Drop the `# ` prefix: the spans are already styled per heading
            // level, so emit them as-is (one line, no literal hashes).
            .with_heading(|_level, spans| vec![Line::from(spans)])
            .build();
        into_text_with_renderer(body, &renderer)
    }

    /// Trim an RFC 3339 timestamp to its `YYYY-MM-DD` date for the header.
    fn ymd(iso: &str) -> &str {
        iso.get(..10).unwrap_or(iso)
    }

    fn render_preview(f: &mut Frame, state: &TuiState, area: Rect) {
        let header = match state.preview() {
            Some(d) => {
                let s = &d.summary;
                let created = s.created.as_deref().map(ymd).unwrap_or("unknown");
                // Git-derived status transitions after the initial proposal.
                let mut milestones = String::new();
                for e in d.history.iter().skip(1) {
                    milestones.push_str(&format!("\n{}: {}", e.label, ymd(&e.date)));
                }
                let updated = match &d.last_modified {
                    Some(lm) => format!("\nUpdated: {}", ymd(lm)),
                    None => String::new(),
                };
                let superseded = match &s.superseded_by {
                    Some(r) => format!("\nSuperseded by: {r}"),
                    None => String::new(),
                };
                Some((
                    format!(
                        "{}: {}\nStatus:  {}\nCreated: {created}{milestones}{updated}{superseded}\n\n",
                        s.reference, s.title, s.status,
                    ),
                    d,
                ))
            }
            None => None,
        };
        let para = match header {
            Some((header, d)) if state.preview_raw() => {
                Paragraph::new(format!("{header}{}", d.body))
            }
            Some((header, d)) => {
                // Styled metadata header, then the themed Markdown body.
                let mut text = Text::raw(header);
                let rendered = render_markdown_body(&d.body, state.md_theme());
                text.lines.extend(rendered.lines);
                Paragraph::new(text)
            }
            None => Paragraph::new("No ADR selected."),
        };
        let c = chrome(state.md_theme());
        let focused = matches!(state.mode(), Mode::Preview);
        let border = if focused { c.accent } else { c.border };
        let title = if state.preview_raw() {
            " Preview (raw) "
        } else {
            " Preview "
        };
        let para = para
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(border))
                    .title(Span::styled(title, Style::default().fg(c.title))),
            )
            .wrap(Wrap { trim: false })
            .scroll((state.preview_scroll(), 0));
        f.render_widget(para, area);
    }

    fn render_footer(f: &mut Frame, state: &TuiState, area: Rect) {
        let help = match state.mode() {
            Mode::List => {
                "j/k move  g/G top/bottom  Enter preview  / search  f filter  o sort  \
                 n new  s status  S supersede  i edit-body  e $EDITOR  r refresh  ? help  q quit"
            }
            Mode::Preview => "j/k scroll  g/G top/bottom  Enter/Esc back  ? help  q quit",
            Mode::Search { .. } => "type to search  Enter apply  Esc cancel",
            Mode::NewTitle { .. } => "type title  Enter create  Esc cancel",
            Mode::PickStatus { .. } => "j/k pick  Enter apply  Esc cancel",
            Mode::Supersede { .. } => "type OLD adr id  Enter supersede  Esc cancel",
            Mode::Edit {
                confirm_discard: true,
                ..
            } => "discard unsaved edits?  y/Esc discard  n keep editing",
            Mode::Edit { .. } => {
                "type to edit  Enter newline  arrows/Home/End move  \
                 Ctrl-S save  Esc cancel"
            }
        };
        let prompt = match state.mode() {
            Mode::Search { input } => Some(format!("search: {input}")),
            Mode::NewTitle { input } => Some(format!("new title: {input}")),
            Mode::Supersede { input } => Some(format!("this supersedes {input}")),
            Mode::Edit {
                confirm_discard: true,
                ..
            } => Some("Unsaved changes — discard? (y/n)".to_string()),
            _ => state.message().map(|m| m.to_string()),
        };
        // Line 1: the active prompt or a transient message, in the accent color;
        // line 2: the context-aware key hints, muted.
        let c = chrome(state.md_theme());
        let lines = vec![
            Line::from(Span::styled(
                prompt.unwrap_or_default(),
                Style::default().fg(c.accent),
            )),
            Line::from(Span::styled(help, Style::default().fg(c.muted))),
        ];
        f.render_widget(Paragraph::new(lines), area);
    }

    fn render_status_picker(f: &mut Frame, state: &TuiState, area: Rect) {
        let Mode::PickStatus { index } = state.mode() else {
            return;
        };
        let popup = centered(40, STATUSES.len() as u16 + 2, area);
        let items: Vec<ListItem> = STATUSES
            .iter()
            .map(|s| ListItem::new(s.to_string()))
            .collect();
        let mut list_state = ListState::default();
        list_state.select(Some(*index));
        let c = chrome(state.md_theme());
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(c.accent))
                    .title(Span::styled(
                        " Set status (Enter) ",
                        Style::default().fg(c.title),
                    )),
            )
            .highlight_style(
                Style::default()
                    .bg(c.selection_bg)
                    .fg(c.accent)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        f.render_widget(Clear, popup);
        f.render_stateful_widget(list, popup, &mut list_state);
    }

    fn centered(width: u16, height: u16, area: Rect) -> Rect {
        let x = area.x + area.width.saturating_sub(width) / 2;
        let y = area.y + area.height.saturating_sub(height) / 2;
        Rect {
            x,
            y,
            width: width.min(area.width),
            height: height.min(area.height),
        }
    }

    fn sort_label(sort: Sort) -> &'static str {
        match sort {
            Sort::NumberAsc => "num",
            Sort::NumberDesc => "num-desc",
            Sort::CreatedDesc => "created",
            Sort::TitleAsc => "title",
        }
    }

    fn status_color(status: Status) -> Color {
        match status {
            Status::Proposed => Color::Yellow,
            Status::Accepted => Color::Green,
            Status::Rejected => Color::Red,
            Status::Deprecated => Color::Magenta,
            Status::Superseded => Color::DarkGray,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Store, StoreOptions};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn summary(number: u32, status: Status, title: &str) -> AdrSummary {
        AdrSummary {
            number: Some(number),
            number_display: format!("{number:04}"),
            reference: format!("ADR-{number:04}"),
            address: number.to_string(),
            title: title.to_string(),
            status,
            created: Some(format!("2024-01-{number:02}T00:00:00Z")),
            supersedes: Vec::new(),
            superseded_by: None,
            review_due: false,
            forge_data: None,
        }
    }

    fn sample_rows() -> Vec<AdrSummary> {
        vec![
            summary(1, Status::Accepted, "Use Rust"),
            summary(2, Status::Proposed, "Adopt ratatui"),
            summary(3, Status::Rejected, "Use Java"),
        ]
    }

    /// A markdown / by_status store over a fresh tempdir, plus a Config wired to
    /// match it (so `create_adr` resolves the built-in `madr` template).
    fn setup_store() -> (TempDir, Store, Config) {
        let dir = TempDir::new().unwrap();
        let store = Store::open_or_create_with(dir.path(), StoreOptions::default()).unwrap();
        let config = Config::default();
        (dir, store, config)
    }

    /// Mirror the integration helper: write a markdown ADR into its status dir.
    fn write_md(store: &Store, status: Status, number: u32, title: &str, body: &str) -> PathBuf {
        let dir = store.status_dir(status);
        std::fs::create_dir_all(&dir).unwrap();
        let slug: String = title.to_lowercase().replace(' ', "-");
        let p = dir.join(format!("{number:04}-{slug}.md"));
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn open_store_uses_the_resolved_dir() {
        // The TUI must open the store at the dir threaded in from the CLI
        // (`--dir`/config), NOT re-resolve to the XDG default. Assert the seam:
        // `open_store(cfg, dir)` opens exactly `dir`.
        let dir = TempDir::new().unwrap();
        let cfg = Config::default();
        let store = open_store(&cfg, dir.path()).unwrap();
        assert_eq!(store.root(), dir.path());
        // An ADR written under that dir is found there, proving it is live.
        write_md(
            &store,
            Status::Proposed,
            1,
            "Scoped",
            "# ADR-0001: Scoped\n\n## Status\n\nProposed\n",
        );
        assert!(dir.path().join("proposed/0001-scoped.md").exists());
    }

    // --- pure state: selection movement -------------------------------------

    #[test]
    fn select_next_and_prev_stay_in_bounds() {
        let mut s = TuiState::new();
        s.set_rows(sample_rows());
        assert_eq!(s.selected_index(), Some(0));
        s.select_next();
        assert_eq!(s.selected_index(), Some(1));
        s.select_next();
        s.select_next(); // clamp at last
        assert_eq!(s.selected_index(), Some(2));
        s.select_prev();
        assert_eq!(s.selected_index(), Some(1));
        s.select_first();
        assert_eq!(s.selected_index(), Some(0));
        s.select_last();
        assert_eq!(s.selected_index(), Some(2));
    }

    #[test]
    fn empty_rows_have_no_selection() {
        let mut s = TuiState::new();
        s.set_rows(vec![]);
        assert_eq!(s.selected_index(), None);
        assert_eq!(s.selected_number(), None);
        s.select_next();
        s.select_prev();
        assert_eq!(s.selected_index(), None);
    }

    #[test]
    fn set_rows_clamps_selection() {
        let mut s = TuiState::new();
        s.set_rows(sample_rows());
        s.select_last(); // index 2
        s.set_rows(vec![summary(1, Status::Proposed, "Only one")]);
        assert_eq!(s.selected_index(), Some(0));
    }

    // --- pure state: filtering / search -------------------------------------

    #[test]
    fn cycle_status_filter_walks_all_then_wraps() {
        let mut s = TuiState::new();
        assert_eq!(s.status_filter(), None);
        s.cycle_status_filter();
        assert_eq!(s.status_filter(), Some(Status::Proposed));
        s.cycle_status_filter();
        assert_eq!(s.status_filter(), Some(Status::Accepted));
        for _ in 0..4 {
            s.cycle_status_filter();
        }
        assert_eq!(s.status_filter(), None); // wrapped back to All
    }

    #[test]
    fn filter_resets_selection_and_builds_filter() {
        let mut s = TuiState::new();
        s.set_rows(sample_rows());
        s.select_last();
        s.apply_filter(Some(Status::Proposed));
        assert_eq!(s.selected_index(), Some(0));
        let f = s.filter();
        assert_eq!(f.status, Some(Status::Proposed));
        assert_eq!(f.sort, Sort::NumberAsc);
    }

    #[test]
    fn search_narrows_via_filter_contract() {
        let mut s = TuiState::new();
        s.set_search(Some("ratatui".to_string()));
        assert_eq!(s.search(), Some("ratatui"));
        s.set_search(Some(String::new())); // empty clears
        assert_eq!(s.search(), None);
    }

    #[test]
    fn cycle_sort_rotates() {
        let mut s = TuiState::new();
        assert_eq!(s.sort(), Sort::NumberAsc);
        s.cycle_sort();
        assert_eq!(s.sort(), Sort::NumberDesc);
        s.cycle_sort();
        assert_eq!(s.sort(), Sort::CreatedDesc);
        s.cycle_sort();
        assert_eq!(s.sort(), Sort::TitleAsc);
        s.cycle_sort();
        assert_eq!(s.sort(), Sort::NumberAsc);
    }

    // --- pure state: input modes -> Action intents --------------------------

    #[test]
    fn search_confirm_returns_refresh_and_sets_needle() {
        let mut s = TuiState::new();
        s.begin_search();
        for c in "rust".chars() {
            s.push_char(c);
        }
        s.pop_char(); // -> "rus"
        assert_eq!(s.confirm(), Action::Refresh);
        assert_eq!(s.search(), Some("rus"));
        assert_eq!(*s.mode(), Mode::List);
    }

    #[test]
    fn new_title_confirm_returns_create() {
        let mut s = TuiState::new();
        s.begin_new();
        for c in "My ADR".chars() {
            s.push_char(c);
        }
        assert_eq!(s.confirm(), Action::Create("My ADR".to_string()));
    }

    #[test]
    fn empty_new_title_is_noop() {
        let mut s = TuiState::new();
        s.begin_new();
        assert_eq!(s.confirm(), Action::None);
    }

    #[test]
    fn pick_status_maps_to_set_status_for_selected() {
        let mut s = TuiState::new();
        s.set_rows(sample_rows());
        s.select_next(); // ADR 2
        s.begin_pick_status();
        s.picker_next(); // Proposed -> Accepted
        assert_eq!(
            s.confirm(),
            Action::SetStatus("2".to_string(), Status::Accepted)
        );
    }

    #[test]
    fn pick_status_noop_without_selection() {
        let mut s = TuiState::new();
        s.set_rows(vec![]);
        s.begin_pick_status();
        // begin is a no-op with no selection; mode stays List.
        assert_eq!(*s.mode(), Mode::List);
    }

    #[test]
    fn supersede_confirm_maps_old_to_selected_new() {
        let mut s = TuiState::new();
        s.set_rows(sample_rows());
        s.select_last(); // ADR 3 is the NEW one
        s.begin_supersede();
        s.push_char('1');
        assert_eq!(
            s.confirm(),
            Action::Supersede {
                new: "3".to_string(),
                old: "1".to_string()
            }
        );
    }

    #[test]
    fn esc_cancels_input_without_action() {
        let mut s = TuiState::new();
        s.begin_new();
        s.push_char('x');
        s.back_to_list();
        assert_eq!(*s.mode(), Mode::List);
    }

    #[test]
    fn preview_scroll_saturates() {
        let mut s = TuiState::new();
        assert_eq!(s.preview_scroll(), 0);
        s.preview_scroll_up(); // can't go negative
        assert_eq!(s.preview_scroll(), 0);
        s.preview_scroll_down();
        s.preview_scroll_down();
        assert_eq!(s.preview_scroll(), 2);
        s.preview_scroll_up();
        assert_eq!(s.preview_scroll(), 1);
    }

    #[test]
    fn preview_defaults_to_rendered_and_toggles_raw() {
        let mut s = TuiState::new();
        assert!(!s.preview_raw()); // rendered markdown by default
        s.toggle_preview_raw();
        assert!(s.preview_raw());
        s.toggle_preview_raw();
        assert!(!s.preview_raw());
    }

    #[test]
    fn toggling_raw_resets_preview_scroll() {
        let mut s = TuiState::new();
        s.preview_scroll_down();
        s.preview_scroll_down();
        assert_eq!(s.preview_scroll(), 2);
        s.toggle_preview_raw();
        assert_eq!(s.preview_scroll(), 0);
    }

    #[test]
    fn md_theme_defaults_to_gruvbox_and_is_settable() {
        let mut s = TuiState::new();
        assert_eq!(s.md_theme(), MarkdownTheme::Gruvbox);
        s.set_md_theme(MarkdownTheme::Warm);
        assert_eq!(s.md_theme(), MarkdownTheme::Warm);
        s.set_md_theme(MarkdownTheme::Default);
        assert_eq!(s.md_theme(), MarkdownTheme::Default);
    }

    #[test]
    fn help_overlay_toggles_and_any_key_closes_it() {
        let mut s = TuiState::new();
        assert!(!s.show_help());
        s.toggle_help();
        assert!(s.show_help());
        s.close_help();
        assert!(!s.show_help());
    }

    #[test]
    fn every_theme_yields_a_chrome_and_markdown_palette() {
        for t in [
            MarkdownTheme::Gruvbox,
            MarkdownTheme::Warm,
            MarkdownTheme::Default,
        ] {
            let _ = driver::chrome(t);
            let _ = driver::render_markdown_body("# H\n\nbody", t);
        }
    }

    /// The preview renderer must actually RENDER markdown, not pass it through:
    /// inline emphasis/code markers AND heading `#` prefixes are stripped
    /// (styling carries the meaning), while the text content survives. Guards
    /// against the preview silently reading like raw source.
    #[test]
    fn render_markdown_body_strips_inline_markers() {
        let md = "# Title\n\nSome **bold** and `code` here.\n";
        let text = driver::render_markdown_body(md, MarkdownTheme::Default);
        let rendered: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        // Content preserved.
        assert!(rendered.contains("bold"), "bold text missing: {rendered:?}");
        assert!(rendered.contains("code"), "code text missing: {rendered:?}");
        assert!(
            rendered.contains("Title"),
            "heading text missing: {rendered:?}"
        );
        // Inline emphasis / code markers stripped (proves it's rendered).
        assert!(
            !rendered.contains("**"),
            "bold markers not stripped: {rendered:?}"
        );
        assert!(
            !rendered.contains('`'),
            "code markers not stripped: {rendered:?}"
        );
        // Heading `#` prefix dropped (the bug: it read like raw source). The
        // heading text survives; only the literal hashes are gone.
        assert!(
            !rendered.contains('#'),
            "heading hashes not stripped (preview reads as raw markdown): {rendered:?}"
        );
    }

    // --- action handlers against a real Store / tempdir ---------------------

    #[test]
    fn apply_create_action_writes_via_store_and_reloads() {
        let (_d, store, cfg) = setup_store();
        let mut s = TuiState::new();
        reload(&mut s, &store).unwrap();
        assert_eq!(s.visible_rows().len(), 0);

        let quit = apply_action(&mut s, &store, &cfg, Action::Create("First".to_string())).unwrap();
        assert!(!quit);
        assert_eq!(s.visible_rows().len(), 1);
        assert_eq!(s.selected().unwrap().title, "First");
        assert_eq!(s.selected().unwrap().status, Status::Proposed);
    }

    #[test]
    fn apply_set_status_action_moves_through_store() {
        let (_d, store, cfg) = setup_store();
        let adr = create_adr(&store, &cfg, "Decide").unwrap();
        let num = adr.number.unwrap().get();
        let mut s = TuiState::new();
        reload(&mut s, &store).unwrap();

        let quit = apply_action(
            &mut s,
            &store,
            &cfg,
            Action::SetStatus(num.to_string(), Status::Accepted),
        )
        .unwrap();
        assert!(!quit);
        assert_eq!(s.selected().unwrap().status, Status::Accepted);
        // Confirm it persisted through the store, not just in memory.
        assert_eq!(
            query::detail(&store, num).unwrap().summary.status,
            Status::Accepted
        );
    }

    #[test]
    fn apply_supersede_action_marks_old_superseded() {
        let (_d, store, cfg) = setup_store();
        let old = create_adr(&store, &cfg, "Old")
            .unwrap()
            .number
            .unwrap()
            .get();
        let new = create_adr(&store, &cfg, "New")
            .unwrap()
            .number
            .unwrap()
            .get();
        let mut s = TuiState::new();
        reload(&mut s, &store).unwrap();

        apply_action(
            &mut s,
            &store,
            &cfg,
            Action::Supersede {
                new: new.to_string(),
                old: old.to_string(),
            },
        )
        .unwrap();
        assert_eq!(
            query::detail(&store, old).unwrap().summary.status,
            Status::Superseded
        );
    }

    #[test]
    fn quit_action_signals_exit() {
        let (_d, store, cfg) = setup_store();
        let mut s = TuiState::new();
        assert!(apply_action(&mut s, &store, &cfg, Action::Quit).unwrap());
    }

    // --- EditorBuffer: pure, terminal-free editing --------------------------

    #[test]
    fn editor_from_str_and_to_string_round_trip() {
        let text = "line one\nline two\nline three";
        let buf = EditorBuffer::from_str(text);
        assert_eq!(buf.lines().len(), 3);
        assert_eq!(buf.to_string(), text);
    }

    #[test]
    fn editor_from_str_drops_single_trailing_newline() {
        // A trailing newline must not create a spurious empty final line, so
        // round-tripping a typical file body is stable.
        let buf = EditorBuffer::from_str("a\nb\n");
        assert_eq!(buf.lines(), &["a".to_string(), "b".to_string()]);
        assert_eq!(buf.to_string(), "a\nb");
    }

    #[test]
    fn editor_from_str_normalizes_crlf() {
        let buf = EditorBuffer::from_str("a\r\nb\r\n");
        assert_eq!(buf.to_string(), "a\nb");
    }

    #[test]
    fn editor_empty_is_single_blank_line() {
        let buf = EditorBuffer::from_str("");
        assert_eq!(buf.lines(), &[String::new()]);
        assert_eq!(buf.to_string(), "");
        let buf2 = EditorBuffer::new();
        assert_eq!(buf2, EditorBuffer::default());
    }

    #[test]
    fn editor_insert_char_advances_cursor() {
        let mut buf = EditorBuffer::new();
        for c in "hi".chars() {
            buf.insert_char(c);
        }
        assert_eq!(buf.to_string(), "hi");
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (0, 2));
    }

    #[test]
    fn editor_insert_char_in_middle() {
        let mut buf = EditorBuffer::from_str("ac");
        buf.move_right(); // after 'a'
        buf.insert_char('b');
        assert_eq!(buf.to_string(), "abc");
        assert_eq!(buf.cursor_col(), 2);
    }

    #[test]
    fn editor_insert_char_handles_unicode() {
        let mut buf = EditorBuffer::from_str("aé");
        buf.end(); // col 2 (2 chars), byte len 3
        buf.insert_char('z');
        assert_eq!(buf.to_string(), "aéz");
        assert_eq!(buf.cursor_col(), 3);
        // Backspacing the multi-byte char before it works on char boundaries.
        buf.move_left(); // before 'z'
        buf.backspace(); // removes 'é'
        assert_eq!(buf.to_string(), "az");
    }

    #[test]
    fn editor_newline_splits_line_at_cursor() {
        let mut buf = EditorBuffer::from_str("hello world");
        for _ in 0..5 {
            buf.move_right();
        }
        buf.insert_newline();
        assert_eq!(buf.lines(), &["hello".to_string(), " world".to_string()]);
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (1, 0));
    }

    #[test]
    fn editor_backspace_within_line() {
        let mut buf = EditorBuffer::from_str("abc");
        buf.end();
        buf.backspace();
        assert_eq!(buf.to_string(), "ab");
        assert_eq!(buf.cursor_col(), 2);
    }

    #[test]
    fn editor_backspace_at_line_start_joins_previous_line() {
        let mut buf = EditorBuffer::from_str("foo\nbar");
        buf.move_down(); // row 1
        buf.home(); // col 0 of "bar"
        buf.backspace(); // join onto "foo"
        assert_eq!(buf.to_string(), "foobar");
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (0, 3));
    }

    #[test]
    fn editor_backspace_at_buffer_start_is_noop() {
        let mut buf = EditorBuffer::from_str("x");
        buf.home();
        buf.backspace();
        assert_eq!(buf.to_string(), "x");
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (0, 0));
    }

    #[test]
    fn editor_cursor_movement_and_wrapping() {
        let mut buf = EditorBuffer::from_str("ab\ncd");
        // move_right wraps line1-end -> line2-start.
        buf.end(); // (0,2)
        buf.move_right(); // -> (1,0)
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (1, 0));
        // move_left wraps line2-start -> line1-end.
        buf.move_left(); // -> (0,2)
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (0, 2));
        // up/down clamp column to the destination line length.
        let mut b2 = EditorBuffer::from_str("longline\nx");
        b2.end(); // (0,8)
        b2.move_down(); // clamp to (1,1)
        assert_eq!((b2.cursor_row(), b2.cursor_col()), (1, 1));
        b2.move_up(); // back up; column clamps to <= 8 (stays 1)
        assert_eq!((b2.cursor_row(), b2.cursor_col()), (0, 1));
    }

    #[test]
    fn editor_movement_clamps_at_edges() {
        let mut buf = EditorBuffer::from_str("a\nb");
        buf.move_up(); // already top -> no-op
        assert_eq!(buf.cursor_row(), 0);
        buf.move_left(); // already at start -> no-op
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (0, 0));
        buf.move_down();
        buf.move_down(); // already bottom -> no-op
        assert_eq!(buf.cursor_row(), 1);
        buf.end();
        buf.move_right(); // already at very end -> no-op
        assert_eq!((buf.cursor_row(), buf.cursor_col()), (1, 1));
    }

    #[test]
    fn editor_home_and_end() {
        let mut buf = EditorBuffer::from_str("hello");
        buf.end();
        assert_eq!(buf.cursor_col(), 5);
        buf.home();
        assert_eq!(buf.cursor_col(), 0);
    }

    // --- edit mode wiring through TuiState ----------------------------------

    fn state_with_one_adr(store: &Store, cfg: &Config, title: &str) -> (TuiState, u32) {
        let num = create_adr(store, cfg, title).unwrap().number.unwrap().get();
        let mut s = TuiState::new();
        reload(&mut s, store).unwrap();
        (s, num)
    }

    #[test]
    fn begin_edit_seeds_buffer_from_preview_body() {
        let (_d, store, cfg) = setup_store();
        let (mut s, _num) = state_with_one_adr(&store, &cfg, "Editable");
        let body = s.preview().unwrap().body.clone();
        s.begin_edit();
        assert!(matches!(s.mode(), Mode::Edit { dirty: false, .. }));
        assert_eq!(s.editor().unwrap().to_string(), body);
        assert!(!s.is_dirty());
    }

    #[test]
    fn editing_marks_dirty_and_save_action_clears_it() {
        let (_d, store, cfg) = setup_store();
        let (mut s, num) = state_with_one_adr(&store, &cfg, "Editable");
        s.begin_edit();
        s.edit_down_to_end();
        s.edit_newline();
        s.edit_insert_char('Z');
        assert!(s.is_dirty());

        let action = s.save_edit();
        match action {
            Action::SaveBody {
                ref address,
                ref body,
            } => {
                assert_eq!(address, &num.to_string());
                assert!(body.ends_with('Z'));
            }
            other => panic!("expected SaveBody, got {other:?}"),
        }
        assert!(!s.is_dirty());
    }

    #[test]
    fn save_body_action_persists_through_store_preserving_structure() {
        let (_d, store, cfg) = setup_store();
        let (mut s, num) = state_with_one_adr(&store, &cfg, "Persist Me");
        let before = query::detail(&store, num).unwrap().body;
        s.begin_edit();
        // Append a paragraph at the very end of the document body.
        s.edit_down_to_end();
        s.edit_insert_char('!');
        let action = s.save_edit();

        let quit = apply_action(&mut s, &store, &cfg, action).unwrap();
        assert!(!quit);

        let after = query::detail(&store, num).unwrap().body;
        assert_ne!(before, after);
        assert!(after.ends_with('!'));
        // The H1 / Status scaffolding from the template survives unchanged.
        assert!(after.contains(&format!("ADR-{num:04}")) || after.contains("## Status"));
    }

    #[test]
    fn esc_on_clean_buffer_exits_immediately() {
        let (_d, store, cfg) = setup_store();
        let (mut s, _num) = state_with_one_adr(&store, &cfg, "Clean");
        s.begin_edit();
        assert!(s.request_cancel_edit()); // clean -> true (exited)
        assert_eq!(*s.mode(), Mode::List);
        assert!(s.editor().is_none());
    }

    #[test]
    fn esc_on_dirty_buffer_requires_confirm() {
        let (_d, store, cfg) = setup_store();
        let (mut s, _num) = state_with_one_adr(&store, &cfg, "Dirty");
        s.begin_edit();
        s.edit_insert_char('x');
        assert!(!s.request_cancel_edit()); // dirty -> arm confirmation
        assert!(s.awaiting_discard_confirm());
        assert!(matches!(s.mode(), Mode::Edit { .. }));
        // 'n' cancels the prompt, staying in edit mode with the buffer intact.
        s.cancel_discard_edit();
        assert!(!s.awaiting_discard_confirm());
        assert!(matches!(s.mode(), Mode::Edit { .. }));
        // Re-arm and confirm discard.
        assert!(!s.request_cancel_edit());
        s.confirm_discard_edit();
        assert_eq!(*s.mode(), Mode::List);
        assert!(s.editor().is_none());
    }

    #[test]
    fn edit_scroll_follows_cursor_within_viewport() {
        let mut s = TuiState::new();
        // Seed a long buffer directly via begin path is awkward; build state.
        s.set_preview(Some(AdrDetail {
            summary: summary(1, Status::Proposed, "Long"),
            body: (0..20)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
            body_html: None,
            related: Vec::new(),
            history: Vec::new(),
            last_modified: None,
        }));
        s.set_rows(vec![summary(1, Status::Proposed, "Long")]);
        s.begin_edit();
        s.set_edit_viewport(5);
        assert_eq!(s.edit_scroll(), 0);
        for _ in 0..10 {
            s.edit_down();
        }
        // Cursor at row 10, viewport 5 -> top scrolled so row 10 visible.
        assert!(s.edit_scroll() > 0);
        assert!(s.editor().unwrap().cursor_row() >= s.edit_scroll());
        assert!(s.editor().unwrap().cursor_row() < s.edit_scroll() + 5);
    }

    #[test]
    fn reload_applies_status_filter_with_search() {
        let (_d, store, cfg) = setup_store();
        let a = create_adr(&store, &cfg, "Alpha keyword")
            .unwrap()
            .number
            .unwrap()
            .get();
        create_adr(&store, &cfg, "Beta keyword").unwrap();
        store.set_status(Number::new(a), Status::Accepted).unwrap();

        let mut s = TuiState::new();
        s.set_search(Some("keyword".to_string()));
        reload(&mut s, &store).unwrap();
        assert_eq!(s.visible_rows().len(), 2);

        s.apply_filter(Some(Status::Accepted));
        reload(&mut s, &store).unwrap();
        assert_eq!(s.visible_rows().len(), 1);
        assert_eq!(s.selected().unwrap().number, Some(a));
    }
}
