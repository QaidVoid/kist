//! Terminal application state machine.
//!
//! [`App`] holds the UI state (current mode, selection, input buffer, latest
//! snapshot and status) and translates input events into engine [`Command`]s
//! via an [`Action`]. Rendering is pure given an `App`, so this module has no
//! dependency on ratatui.

use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};

use crate::engine::Command;
use crate::model::{RowState, Snapshot};

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
    /// Latest snapshot received from the engine.
    pub snapshot: Snapshot,
    /// Latest status/error message to display, if any.
    pub status: Option<String>,
    /// Whether the current status is an error (for coloring).
    pub status_is_error: bool,
}

impl App {
    /// Create a new app starting in list mode with no torrents.
    pub fn new() -> Self {
        Self {
            mode: Mode::List,
            selected: 0,
            input: String::new(),
            snapshot: Snapshot::default(),
            status: None,
            status_is_error: false,
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
    }

    /// Handle a merged event, returning the action to take.
    pub fn handle(&mut self, event: Event) -> Action {
        match event {
            Event::Tick => Action::none(),
            Event::Input(CrosstermEvent::Key(key)) => self.handle_key(key),
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
                self.input.clear();
                self.mode = Mode::List;
                Action::none()
            }
            KeyCode::Enter => {
                let source = self.input.trim().to_string();
                self.input.clear();
                self.mode = Mode::List;
                if source.is_empty() {
                    Action::none()
                } else {
                    Action::cmd(Command::Add(source))
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                Action::none()
            }
            KeyCode::Char(c) => {
                if key.modifiers.is_empty() {
                    self.input.push(c);
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
