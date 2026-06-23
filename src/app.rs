//! Terminal application state machine.
//!
//! [`App`] holds the UI state (current mode, selection, input buffer, latest
//! snapshot and status) and translates input events into engine [`Command`]s
//! via an [`Action`]. Rendering is pure given an `App`, so this module has no
//! dependency on ratatui.

use std::time::{Duration, Instant};

use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};

use crate::engine::Command;
use crate::model::{RowState, Snapshot};

/// How long a transient status/error message stays on screen before clearing.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// The current top-level UI mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the torrent list.
    List,
    /// The add-torrent input prompt is open.
    AddBar,
    /// The help overlay is open.
    Help,
}

/// A merged event fed to [`App::handle`] by the runtime.
#[derive(Debug)]
pub enum Event {
    /// A raw terminal input event (key, mouse, resize).
    Input(CrosstermEvent),
    /// A periodic refresh tick.
    Tick,
}

/// What the runtime should do after [`App`] handles an event.
#[derive(Debug, Default)]
pub struct Action {
    /// Engine commands to send.
    pub commands: Vec<Command>,
    /// Whether the application should exit.
    pub quit: bool,
}

impl Action {
    fn none() -> Self {
        Self::default()
    }

    fn cmd(command: Command) -> Self {
        Self {
            commands: vec![command],
            quit: false,
        }
    }
}

/// The terminal application state.
pub struct App {
    /// Active UI mode.
    pub mode: Mode,
    /// Index of the selected row in the list.
    pub selected: usize,
    /// Current contents of the add-torrent input bar.
    pub input: String,
    /// Byte offset of the editing cursor within [`App::input`].
    pub cursor: usize,
    /// Top visible visual line of the add bar (persisted for edge-scrolling).
    pub view_top: usize,
    /// Wrap width last used to render the add bar, so cursor movement matches
    /// the displayed layout.
    pub wrap_width: usize,
    /// Latest snapshot received from the engine.
    pub snapshot: Snapshot,
    /// Latest status/error message to display, if any.
    pub status: Option<String>,
    /// Whether the current status is an error (for coloring).
    pub status_is_error: bool,
    /// When the current status was set, for auto-dismissal.
    status_at: Option<Instant>,
}

impl App {
    /// Create a new app starting in list mode with no torrents.
    pub fn new() -> Self {
        Self {
            mode: Mode::List,
            selected: 0,
            input: String::new(),
            cursor: 0,
            view_top: 0,
            wrap_width: 1,
            snapshot: Snapshot::default(),
            status: None,
            status_is_error: false,
            status_at: None,
        }
    }

    /// Replace the snapshot, keeping the selection in range.
    pub fn update_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot;
        let len = self.snapshot.rows.len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    /// Set the latest status message.
    pub fn set_status(&mut self, message: String, is_error: bool) {
        self.status = Some(message);
        self.status_is_error = is_error;
        self.status_at = Some(Instant::now());
    }

    /// Clear any transient status message.
    pub fn clear_status(&mut self) {
        self.status = None;
        self.status_is_error = false;
        self.status_at = None;
    }

    /// Clear the status if it has been visible longer than [`STATUS_TIMEOUT`].
    pub fn expire_status(&mut self) {
        if self
            .status_at
            .is_some_and(|at| at.elapsed() >= STATUS_TIMEOUT)
        {
            self.clear_status();
        }
    }

    /// Handle a merged event, returning the action to take.
    pub fn handle(&mut self, event: Event) -> Action {
        match event {
            Event::Tick => Action::none(),
            // Any key clears a transient status, then is processed normally.
            Event::Input(CrosstermEvent::Key(key)) => {
                if self.status.is_some() {
                    self.clear_status();
                }
                self.handle_key(key)
            }
            Event::Input(CrosstermEvent::Paste(text)) => {
                // A bracketed paste: insert the whole string at the cursor.
                if self.mode == Mode::AddBar {
                    self.insert_str(&text);
                }
                Action::none()
            }
            Event::Input(_) => Action::none(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Action {
        if let Some(action) = global_key(key) {
            return action;
        }
        match self.mode {
            Mode::List => self.handle_list_key(key),
            Mode::AddBar => self.handle_add_key(key),
            Mode::Help => self.handle_help_key(key),
        }
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Action {
                quit: true,
                ..Action::none()
            },
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                Action::none()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                Action::none()
            }
            KeyCode::Char('a') => {
                self.input.clear();
                self.mode = Mode::AddBar;
                Action::none()
            }
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                Action::none()
            }
            KeyCode::Char('p') | KeyCode::Char(' ') => self.cmd_for_selected(Command::Pause),
            KeyCode::Char('r') => self.cmd_for_selected(Command::Resume),
            KeyCode::Char('d') | KeyCode::Delete => self.cmd_for_selected(Command::Remove),
            KeyCode::Enter => self.toggle_pause_resume(),
            _ => Action::none(),
        }
    }

    fn handle_add_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.clear_input();
                self.mode = Mode::List;
                Action::none()
            }
            KeyCode::Enter => {
                let source = self.input.trim().to_string();
                self.clear_input();
                self.mode = Mode::List;
                if source.is_empty() {
                    Action::none()
                } else {
                    Action::cmd(Command::Add(source))
                }
            }
            KeyCode::Backspace => {
                self.backspace();
                Action::none()
            }
            KeyCode::Left => {
                self.move_left();
                Action::none()
            }
            KeyCode::Right => {
                self.move_right();
                Action::none()
            }
            KeyCode::Up => {
                self.move_line(-1);
                Action::none()
            }
            KeyCode::Down => {
                self.move_line(1);
                Action::none()
            }
            KeyCode::Home => {
                self.cursor = 0;
                Action::none()
            }
            KeyCode::End => {
                self.cursor = self.input.len();
                Action::none()
            }
            KeyCode::Char(c) => {
                // Insert printable characters, including uppercase (Shift+char).
                // Skip only control/alt combinations; Ctrl+C quits globally.
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    self.insert_char(c);
                }
                Action::none()
            }
            _ => Action::none(),
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => {
                self.mode = Mode::List;
                Action::none()
            }
            _ => Action::none(),
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.snapshot.rows.is_empty() {
            self.selected = 0;
            return;
        }
        let max = self.snapshot.rows.len() - 1;
        let next = self.selected as i32 + delta;
        self.selected = next.clamp(0, max as i32) as usize;
    }

    /// Clear the add bar and reset the cursor and scroll to the start.
    fn clear_input(&mut self) {
        self.input.clear();
        self.cursor = 0;
        self.view_top = 0;
    }

    /// Insert a character at the cursor and advance past it.
    fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert a string at the cursor and advance past it.
    ///
    /// Newlines are stripped so the input stays a single logical line (the
    /// prompt wraps it for display instead).
    fn insert_str(&mut self, s: &str) {
        let filtered: String = s.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
        self.input.insert_str(self.cursor, &filtered);
        self.cursor += filtered.len();
    }

    /// Delete the character immediately before the cursor.
    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.prev_char_start();
        self.input.replace_range(prev..self.cursor, "");
        self.cursor = prev;
    }

    /// Move the cursor one character to the left.
    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_start();
        }
    }

    /// Move the cursor one character to the right.
    fn move_right(&mut self) {
        if self.cursor < self.input.len() {
            let adv = self.input[self.cursor..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(0);
            self.cursor += adv;
        }
    }

    /// Move the cursor across wrapped visual lines by `delta` (-1 up, +1 down),
    /// preserving the column within the line and clamping to the text bounds.
    fn move_line(&mut self, delta: i32) {
        let width = self.wrap_width.max(1);
        let total = self.input.chars().count();
        let cur = self.cursor_char_index();
        let line = (cur / width) as i32;
        let col = cur % width;
        let target_line = line + delta;
        if target_line < 0 {
            self.cursor = 0;
            return;
        }
        let target = (target_line as usize) * width + col;
        if target >= total {
            self.cursor = self.input.len();
        } else {
            self.cursor = self.char_to_byte(target);
        }
    }

    /// Char index of the cursor within the input.
    fn cursor_char_index(&self) -> usize {
        self.input[..self.cursor].chars().count()
    }

    /// Byte offset of the `idx`-th char, clamped to the input length.
    fn char_to_byte(&self, idx: usize) -> usize {
        self.input
            .char_indices()
            .nth(idx)
            .map(|(i, _)| i)
            .unwrap_or_else(|| self.input.len())
    }

    /// Byte index of the start of the character immediately before the cursor.
    fn prev_char_start(&self) -> usize {
        self.input[..self.cursor]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn cmd_for_selected(&self, make: fn(usize) -> Command) -> Action {
        match self.selected_id() {
            Some(id) => Action::cmd(make(id)),
            None => Action::none(),
        }
    }

    fn toggle_pause_resume(&self) -> Action {
        match self.snapshot.rows.get(self.selected) {
            Some(row) if row.state == RowState::Paused => Action::cmd(Command::Resume(row.id)),
            Some(row) => Action::cmd(Command::Pause(row.id)),
            None => Action::none(),
        }
    }

    fn selected_id(&self) -> Option<usize> {
        self.snapshot.rows.get(self.selected).map(|row| row.id)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Ctrl+C quits from any mode.
fn global_key(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action {
            quit: true,
            ..Action::none()
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(input: &str, cursor_char: usize, width: usize) -> App {
        let mut a = App::new();
        a.input = input.to_string();
        a.cursor = a
            .input
            .char_indices()
            .nth(cursor_char)
            .map(|(i, _)| i)
            .unwrap_or_else(|| a.input.len());
        a.wrap_width = width;
        a
    }

    #[test]
    fn up_down_cross_wrapped_lines() {
        // "abcdef" wrapped at 3 -> lines "abc" / "def". Cursor on 'e' (char 4).
        let mut a = app_with("abcdef", 4, 3);
        a.move_line(-1); // up -> line 0, col 1 -> 'b'
        assert_eq!(a.cursor_char_index(), 1);
        a.move_line(1); // down -> line 1, col 1 -> 'e'
        assert_eq!(a.cursor_char_index(), 4);
        a.move_line(1); // down past the last line -> end
        assert_eq!(a.cursor, a.input.len());
        a.move_line(-1); // up from end (line 2, col 0) -> line 1, col 0 -> 'd'
        assert_eq!(a.cursor_char_index(), 3);
    }

    #[test]
    fn up_at_top_goes_to_start() {
        let mut a = app_with("abcdef", 1, 3);
        a.move_line(-1);
        assert_eq!(a.cursor, 0);
    }

    #[test]
    fn insert_str_strips_newlines() {
        let mut a = App::new();
        a.input = "ab".to_string();
        a.cursor = 2;
        a.insert_str("c\nd\r\ne");
        assert_eq!(a.input, "abcde");
        assert_eq!(a.cursor, 5);
    }
}
