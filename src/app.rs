//! Terminal application state machine.
//!
//! [`App`] holds the UI state (current mode, selection, input buffer, latest
//! snapshot and status) and translates input events into engine [`Command`]s
//! via an [`Action`]. Rendering is pure given an `App`, so this module has no
//! dependency on ratatui.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyModifiers};

use crate::engine::Command;
use crate::model::{DetailSnapshot, PeerRow, RowState, Snapshot, TorrentRow};
use crate::search::{SearchOutcome, SearchResult};

/// How long a transient status/error message stays on screen before clearing.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// Number of recent speed samples kept for the detail sparklines.
const SPEED_HISTORY_LEN: usize = 60;

/// Column the torrent list is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    State,
    Progress,
    Speed,
}

impl SortKey {
    /// Lowercase label for display in the chrome.
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::State => "state",
            SortKey::Progress => "progress",
            SortKey::Speed => "speed",
        }
    }

    /// Next sort key in the cycle.
    pub fn next(self) -> Self {
        match self {
            SortKey::Name => SortKey::State,
            SortKey::State => SortKey::Progress,
            SortKey::Progress => SortKey::Speed,
            SortKey::Speed => SortKey::Name,
        }
    }
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    /// Arrow glyph for display in the chrome.
    pub fn glyph(self) -> &'static str {
        match self {
            SortDir::Asc => "\u{2191}",
            SortDir::Desc => "\u{2193}",
        }
    }
}

/// Active tab of the torrent detail pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    #[default]
    Overview,
    Files,
    Peers,
    Trackers,
}

impl DetailTab {
    /// Next tab in the cycle.
    pub fn next(self) -> Self {
        match self {
            DetailTab::Overview => DetailTab::Files,
            DetailTab::Files => DetailTab::Peers,
            DetailTab::Peers => DetailTab::Trackers,
            DetailTab::Trackers => DetailTab::Overview,
        }
    }

    /// Lowercase label.
    pub fn label(self) -> &'static str {
        match self {
            DetailTab::Overview => "overview",
            DetailTab::Files => "files",
            DetailTab::Peers => "peers",
            DetailTab::Trackers => "trackers",
        }
    }
}

/// The current top-level UI mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Browsing the torrent list.
    List,
    /// The add-torrent input prompt is open.
    AddBar,
    /// The help overlay is open.
    Help,
    /// A confirmation dialog is open for removing the torrent with this id.
    ConfirmRemove { id: usize },
    /// The list filter entry prompt is open.
    Filter,
    /// The detail pane is open for the torrent with this id.
    Detail { id: usize },
    /// The search query prompt is open.
    SearchInput,
    /// The search results overlay is open.
    SearchResults,
}

/// An add that was dispatched to the engine but has not completed yet, e.g. a
/// magnet still resolving metadata. Shown in the list until the engine reports
/// the outcome.
pub struct PendingAdd {
    /// The source string the add was dispatched with, for correlation.
    pub source: String,
    /// Human-friendly display name.
    pub name: String,
    /// When the add was dispatched, for the elapsed indicator.
    pub started: Instant,
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
    /// Column the list is sorted by.
    pub sort_key: SortKey,
    /// Sort direction.
    pub sort_dir: SortDir,
    /// Active name filter (case-insensitive substring), if any.
    pub filter: Option<String>,
    /// Latest detail snapshot for the pane, when in detail mode.
    pub detail: Option<DetailSnapshot>,
    /// Active tab of the detail pane.
    pub detail_tab: DetailTab,
    /// Vertical scroll offset, in lines, of the detail pane content.
    pub detail_scroll: u16,
    /// Height of the detail content viewport from the last render, used to size
    /// page scrolling.
    pub detail_page: u16,
    /// Recent download-speed samples for the detail torrent (oldest first).
    pub detail_down_history: VecDeque<u64>,
    /// Recent upload-speed samples for the detail torrent (oldest first).
    pub detail_up_history: VecDeque<u64>,
    /// Torrent id the history and peer-speed buffers belong to.
    history_id: Option<usize>,
    /// Smoothed per-peer download speeds (bytes/s), keyed by peer address.
    pub peer_speeds: HashMap<String, u64>,
    /// Last-seen fetched counter and timestamp per peer, for speed derivation.
    peer_samples: HashMap<String, (u64, Instant)>,
    /// Adds dispatched to the engine that have not completed yet.
    pub pending_adds: Vec<PendingAdd>,
    /// Results of the last indexer search, sorted by seeders.
    pub search_results: Vec<SearchResult>,
    /// Index of the selected row in the search results.
    pub search_selected: usize,
    /// Whether a search is in flight (results not yet received).
    pub search_loading: bool,
    /// Query the results (or in-flight search) belong to.
    pub search_query: String,
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
            sort_key: SortKey::Name,
            sort_dir: SortDir::Asc,
            filter: None,
            detail: None,
            detail_tab: DetailTab::default(),
            detail_scroll: 0,
            detail_page: 0,
            detail_down_history: VecDeque::new(),
            detail_up_history: VecDeque::new(),
            history_id: None,
            peer_speeds: HashMap::new(),
            peer_samples: HashMap::new(),
            pending_adds: Vec::new(),
            search_results: Vec::new(),
            search_selected: 0,
            search_loading: false,
            search_query: String::new(),
            status: None,
            status_is_error: false,
            status_at: None,
        }
    }

    /// Replace the snapshot, keeping the selection in range.
    pub fn update_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot;
        self.clamp_selection();
    }

    /// The torrents currently shown: filtered by name and sorted by the active
    /// key/direction. Computed view-side so sorting and filtering stay
    /// synchronous and never touch the engine.
    pub fn visible_rows(&self) -> Vec<&TorrentRow> {
        let mut rows: Vec<&TorrentRow> = self.snapshot.rows.iter().collect();
        if let Some(filter) = &self.filter {
            let needle = filter.to_lowercase();
            if !needle.is_empty() {
                rows.retain(|r| r.name.to_lowercase().contains(&needle));
            }
        }
        let dir = self.sort_dir;
        rows.sort_by(|a, b| {
            let primary = match self.sort_key {
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::State => a.state.cmp(&b.state),
                SortKey::Progress => a.progress_frac().total_cmp(&b.progress_frac()),
                SortKey::Speed => a.down_speed.cmp(&b.down_speed),
            };
            // Name is the stable tiebreaker so equal keys don't flicker.
            let ordered = primary.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            if dir == SortDir::Desc {
                ordered.reverse()
            } else {
                ordered
            }
        });
        rows
    }

    /// Number of selectable list rows: visible torrents plus pending adds.
    fn list_len(&self) -> usize {
        self.visible_rows().len() + self.pending_adds.len()
    }

    /// Keep `selected` within the bounds of the visible list.
    fn clamp_selection(&mut self) {
        let len = self.list_len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    /// After a sort/filter change, try to keep the same torrent selected.
    fn reselect(&mut self, prev_id: Option<usize>) {
        if let Some(id) = prev_id
            && let Some(i) = self.visible_rows().iter().position(|r| r.id == id)
        {
            self.selected = i;
        } else {
            self.clamp_selection();
        }
    }

    /// Cycle the sort key (or toggle direction), keeping the selection stable.
    fn cycle_sort(&mut self, toggle_dir: bool) {
        let prev = self.selected_id();
        if toggle_dir {
            self.sort_dir = match self.sort_dir {
                SortDir::Asc => SortDir::Desc,
                SortDir::Desc => SortDir::Asc,
            };
        } else {
            self.sort_key = self.sort_key.next();
        }
        self.reselect(prev);
    }

    /// Replace the detail snapshot for the pane, feeding the speed history and
    /// per-peer speed buffers (which reset when the target torrent changes).
    pub fn set_detail(&mut self, detail: Option<DetailSnapshot>) {
        let id = match self.mode {
            Mode::Detail { id } => Some(id),
            _ => None,
        };
        if id != self.history_id {
            self.detail_down_history.clear();
            self.detail_up_history.clear();
            self.peer_speeds.clear();
            self.peer_samples.clear();
            self.history_id = id;
        }
        if let Some(d) = &detail
            && id.is_some()
        {
            push_capped(&mut self.detail_down_history, d.down_speed);
            push_capped(&mut self.detail_up_history, d.up_speed);
            self.update_peer_speeds(&d.peer_rows);
        }
        self.detail = detail;
    }

    /// Derive per-peer download speeds from fetched-bytes deltas, lightly
    /// smoothed with an EMA. Peers seen for the first time get no speed yet.
    fn update_peer_speeds(&mut self, peers: &[PeerRow]) {
        let now = Instant::now();
        let mut samples = HashMap::with_capacity(peers.len());
        for p in peers {
            if let Some((last_bytes, last_at)) = self.peer_samples.get(&p.addr) {
                let dt = now.duration_since(*last_at).as_secs_f64();
                if dt > 0.0 {
                    let inst = (p.fetched_bytes.saturating_sub(*last_bytes) as f64 / dt) as u64;
                    let smoothed = match self.peer_speeds.get(&p.addr) {
                        Some(prev) => (*prev as f64 * 0.6 + inst as f64 * 0.4) as u64,
                        None => inst,
                    };
                    self.peer_speeds.insert(p.addr.clone(), smoothed);
                }
            }
            samples.insert(p.addr.clone(), (p.fetched_bytes, now));
        }
        self.peer_speeds
            .retain(|addr, _| samples.contains_key(addr));
        self.peer_samples = samples;
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
                if matches!(self.mode, Mode::AddBar | Mode::SearchInput) {
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
            Mode::Filter => self.handle_filter_key(key),
            Mode::ConfirmRemove { .. } => self.handle_confirm_key(key),
            Mode::Detail { .. } => self.handle_detail_key(key),
            Mode::SearchInput => self.handle_search_input_key(key),
            Mode::SearchResults => self.handle_search_results_key(key),
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
                self.clear_input();
                self.mode = Mode::AddBar;
                Action::none()
            }
            KeyCode::Char('/') => {
                if self.filter.is_some() {
                    // Toggle the filter off.
                    let prev = self.selected_id();
                    self.filter = None;
                    self.reselect(prev);
                    Action::none()
                } else {
                    self.clear_input();
                    self.mode = Mode::Filter;
                    Action::none()
                }
            }
            KeyCode::Char('f') => {
                self.clear_input();
                self.mode = Mode::SearchInput;
                Action::none()
            }
            KeyCode::Char('s') => {
                self.cycle_sort(false);
                Action::none()
            }
            KeyCode::Char('S') => {
                self.cycle_sort(true);
                Action::none()
            }
            KeyCode::Char('i') => match self.selected_id() {
                Some(id) => {
                    self.detail_tab = DetailTab::Overview;
                    self.detail_scroll = 0;
                    self.mode = Mode::Detail { id };
                    Action::cmd(Command::FetchDetail(id))
                }
                None => Action::none(),
            },
            KeyCode::Char('?') => {
                self.mode = Mode::Help;
                Action::none()
            }
            KeyCode::Char('p') | KeyCode::Char(' ') => self.cmd_for_selected(Command::Pause),
            KeyCode::Char('r') => self.cmd_for_selected(Command::Resume),
            KeyCode::Char('d') | KeyCode::Delete => {
                // Removal is destructive, so confirm first. Cancelling a
                // pending add loses nothing, so it needs no confirmation.
                if let Some(id) = self.selected_id() {
                    self.mode = Mode::ConfirmRemove { id };
                    Action::none()
                } else if let Some(pending) = self.selected_pending() {
                    Action::cmd(Command::CancelAdd(pending.source.clone()))
                } else {
                    Action::none()
                }
            }
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
                    self.push_pending_add(&source);
                    Action::cmd(Command::Add(source))
                }
            }
            _ => self.handle_text_key(key).unwrap_or_else(Action::none),
        }
    }

    /// Track a dispatched add so the list can show it until the engine reports
    /// the outcome.
    pub fn push_pending_add(&mut self, source: &str) {
        self.pending_adds.push(PendingAdd {
            name: add_display_name(source),
            source: source.to_string(),
            started: Instant::now(),
        });
    }

    /// Drop the pending marker for a completed add.
    pub fn finish_pending_add(&mut self, source: &str) {
        if let Some(i) = self.pending_adds.iter().position(|p| p.source == source) {
            self.pending_adds.remove(i);
            self.clamp_selection();
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.clear_input();
                self.mode = Mode::List;
                Action::none()
            }
            KeyCode::Enter => {
                let prev = self.selected_id();
                let text = self.input.trim().to_string();
                self.clear_input();
                self.mode = Mode::List;
                self.filter = if text.is_empty() { None } else { Some(text) };
                self.reselect(prev);
                Action::none()
            }
            _ => self.handle_text_key(key).unwrap_or_else(Action::none),
        }
    }

    fn handle_search_input_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.clear_input();
                // Fall back to existing results if there are any.
                self.mode = if self.search_results.is_empty() && !self.search_loading {
                    Mode::List
                } else {
                    Mode::SearchResults
                };
                Action::none()
            }
            KeyCode::Enter => {
                let query = self.input.trim().to_string();
                self.clear_input();
                if query.is_empty() {
                    self.mode = Mode::List;
                    return Action::none();
                }
                self.search_query = query.clone();
                self.search_results.clear();
                self.search_selected = 0;
                self.search_loading = true;
                self.mode = Mode::SearchResults;
                Action::cmd(Command::Search(query))
            }
            _ => self.handle_text_key(key).unwrap_or_else(Action::none),
        }
    }

    fn handle_search_results_key(&mut self, key: KeyEvent) -> Action {
        let last = self.search_results.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::List;
                Action::none()
            }
            KeyCode::Char('f') | KeyCode::Char('/') => {
                self.clear_input();
                self.mode = Mode::SearchInput;
                Action::none()
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.search_selected = self.search_selected.saturating_sub(1);
                Action::none()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.search_selected = (self.search_selected + 1).min(last);
                Action::none()
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.search_selected = 0;
                Action::none()
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.search_selected = last;
                Action::none()
            }
            // Add the selected result and stay, so several can be queued.
            KeyCode::Enter => {
                let magnet = self
                    .search_results
                    .get(self.search_selected)
                    .map(|hit| hit.magnet.clone());
                match magnet {
                    Some(magnet) => {
                        self.push_pending_add(&magnet);
                        Action::cmd(Command::Add(magnet))
                    }
                    None => Action::none(),
                }
            }
            _ => Action::none(),
        }
    }

    /// Apply a finished search, ignoring outcomes for superseded queries.
    pub fn set_search_outcome(&mut self, outcome: SearchOutcome) {
        if outcome.query != self.search_query {
            return;
        }
        self.search_loading = false;
        self.search_results = outcome.results;
        self.search_selected = 0;
        if !outcome.failed.is_empty() {
            self.set_status(
                format!("search failed on: {}", outcome.failed.join(", ")),
                true,
            );
        }
    }

    /// Text-editing keys shared by the add, filter, and search inputs.
    ///
    /// Returns `Some(action)` when the key was handled, else `None` so callers
    /// can fall through.
    fn handle_text_key(&mut self, key: KeyEvent) -> Option<Action> {
        let action = match key.code {
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
            _ => return None,
        };
        Some(action)
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

    fn handle_confirm_key(&mut self, key: KeyEvent) -> Action {
        let Mode::ConfirmRemove { id } = self.mode else {
            return Action::none();
        };
        // Only an explicit "yes" confirms; everything else cancels (including
        // unrecognized keys, per the default-cancel requirement).
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.mode = Mode::List;
                Action::cmd(Command::Remove(id))
            }
            _ => {
                self.mode = Mode::List;
                Action::none()
            }
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Tab => {
                self.detail_tab = self.detail_tab.next();
                self.detail_scroll = 0;
                Action::none()
            }
            KeyCode::Char('i') | KeyCode::Esc => {
                self.mode = Mode::List;
                Action::cmd(Command::StopDetail)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                self.refocus_detail()
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                self.refocus_detail()
            }
            KeyCode::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(self.detail_page.max(1));
                Action::none()
            }
            KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(self.detail_page.max(1));
                Action::none()
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.detail_scroll = self.detail_scroll.saturating_add(self.detail_half_page());
                Action::none()
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.detail_scroll = self.detail_scroll.saturating_sub(self.detail_half_page());
                Action::none()
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.detail_scroll = 0;
                Action::none()
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.detail_scroll = u16::MAX;
                Action::none()
            }
            KeyCode::Char('q') => Action {
                quit: true,
                ..Action::none()
            },
            _ => Action::none(),
        }
    }

    /// After moving the selection in detail mode, retarget the pane at the new
    /// torrent (or leave detail mode if nothing is selected).
    fn refocus_detail(&mut self) -> Action {
        match self.selected_id() {
            Some(id) => {
                self.detail_scroll = 0;
                self.mode = Mode::Detail { id };
                Action::cmd(Command::FetchDetail(id))
            }
            None => {
                self.mode = Mode::List;
                Action::cmd(Command::StopDetail)
            }
        }
    }

    /// Half the detail viewport height, at least one line, for Ctrl+D/Ctrl+U.
    fn detail_half_page(&self) -> u16 {
        (self.detail_page / 2).max(1)
    }

    fn move_selection(&mut self, delta: i32) {
        let len = self.list_len();
        if len == 0 {
            self.selected = 0;
            return;
        }
        let max = len - 1;
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
        match self.visible_rows().get(self.selected) {
            Some(row) if row.state == RowState::Paused => Action::cmd(Command::Resume(row.id)),
            Some(row) => Action::cmd(Command::Pause(row.id)),
            None => Action::none(),
        }
    }

    fn selected_id(&self) -> Option<usize> {
        self.visible_rows().get(self.selected).map(|row| row.id)
    }

    /// The pending add under the selection, if the selection is past the real
    /// torrents (pending rows render after them).
    pub fn selected_pending(&self) -> Option<&PendingAdd> {
        self.pending_adds
            .get(self.selected.checked_sub(self.visible_rows().len())?)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// Human-friendly name for a pending add: the magnet's `dn` parameter when
/// present, otherwise the source string itself.
fn add_display_name(source: &str) -> String {
    if let Some(query) = source.strip_prefix("magnet:?") {
        for param in query.split('&') {
            if let Some(value) = param.strip_prefix("dn=") {
                let decoded = percent_decode(value);
                if !decoded.is_empty() {
                    return decoded;
                }
            }
        }
    }
    source.to_string()
}

/// Decode percent-encoding (and `+` as space), keeping invalid sequences as-is.
fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(hi), Some(lo)) => {
                    out.push(hi * 16 + lo);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Push a sample, dropping the oldest once the history is full.
fn push_capped(buf: &mut VecDeque<u64>, value: u64) {
    if buf.len() == SPEED_HISTORY_LEN {
        buf.pop_front();
    }
    buf.push_back(value);
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

    fn row(id: usize, name: &str, state: RowState, frac: f64, speed: u64) -> TorrentRow {
        TorrentRow {
            id,
            name: name.to_string(),
            infohash: format!("h{id}"),
            total_bytes: 100,
            progress_bytes: (frac * 100.0) as u64,
            uploaded_bytes: 0,
            finished: false,
            down_speed: speed,
            up_speed: 0,
            eta: None,
            peers: 0,
            state,
            error: None,
        }
    }

    #[test]
    fn sort_by_name_then_speed_and_filter() {
        let mut a = App::new();
        a.snapshot = Snapshot::from_rows(vec![
            row(0, "charlie", RowState::Live, 0.5, 10),
            row(1, "alpha", RowState::Live, 0.1, 30),
            row(2, "bravo", RowState::Paused, 0.9, 20),
        ]);

        // Default: name ascending -> alpha, bravo, charlie.
        let names: Vec<usize> = a.visible_rows().iter().map(|r| r.id).collect();
        assert_eq!(names, vec![1, 2, 0]);

        // Speed descending -> alpha(30), bravo(20), charlie(10).
        a.sort_key = SortKey::Speed;
        a.sort_dir = SortDir::Desc;
        let by_speed: Vec<usize> = a.visible_rows().iter().map(|r| r.id).collect();
        assert_eq!(by_speed, vec![1, 2, 0]);

        // Case-insensitive filter "ra" matches only "bravo".
        a.filter = Some("RA".to_string());
        let filtered: Vec<usize> = a.visible_rows().iter().map(|r| r.id).collect();
        assert_eq!(filtered, vec![2]);
    }

    #[test]
    fn add_tracks_pending_until_engine_reports() {
        let mut a = App::new();
        a.mode = Mode::AddBar;
        let source = "magnet:?xt=urn:btih:cab507494d02ebb1178b38f2e9d7be299c86b862&dn=My%20File+v2";
        a.input = source.to_string();
        a.cursor = a.input.len();

        let action = a.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(&action.commands[..], [Command::Add(s)] if s == source));
        assert_eq!(a.pending_adds.len(), 1);
        assert_eq!(a.pending_adds[0].name, "My File v2");

        // Completion for another source leaves the marker alone.
        a.finish_pending_add("magnet:?xt=urn:btih:other");
        assert_eq!(a.pending_adds.len(), 1);

        a.finish_pending_add(source);
        assert!(a.pending_adds.is_empty());
    }

    #[test]
    fn pending_add_is_selectable_and_cancellable() {
        let mut a = App::new();
        a.snapshot = Snapshot::from_rows(vec![row(0, "alpha", RowState::Live, 0.5, 10)]);
        a.push_pending_add("magnet:?xt=urn:btih:abc");

        // The selection extends past the real torrents onto the pending row.
        a.handle_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(a.selected, 1);
        assert!(a.selected_pending().is_some());

        // `d` on a pending row cancels it directly (no confirm dialog).
        let action = a.handle_key(KeyEvent::from(KeyCode::Char('d')));
        assert!(
            matches!(&action.commands[..], [Command::CancelAdd(s)] if s == "magnet:?xt=urn:btih:abc")
        );
        assert_eq!(a.mode, Mode::List);

        // The engine's cancel status clears the row and reclamps the selection.
        a.finish_pending_add("magnet:?xt=urn:btih:abc");
        assert_eq!(a.selected, 0);

        // `d` on a real torrent still asks for confirmation.
        let action = a.handle_key(KeyEvent::from(KeyCode::Char('d')));
        assert!(action.commands.is_empty());
        assert_eq!(a.mode, Mode::ConfirmRemove { id: 0 });
    }

    #[test]
    fn add_display_name_falls_back_to_source() {
        assert_eq!(
            add_display_name("/path/to/file.torrent"),
            "/path/to/file.torrent"
        );
        assert_eq!(
            add_display_name("magnet:?xt=urn:btih:abc"),
            "magnet:?xt=urn:btih:abc"
        );
        assert_eq!(
            add_display_name("magnet:?dn=a%C3%A9b&xt=urn:btih:abc"),
            "aéb"
        );
    }

    #[test]
    fn search_flow_dispatches_query_and_applies_results() {
        let mut a = App::new();
        a.handle_key(KeyEvent::from(KeyCode::Char('f')));
        assert_eq!(a.mode, Mode::SearchInput);

        for c in "ubuntu".chars() {
            a.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
        let action = a.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(&action.commands[..], [Command::Search(q)] if q == "ubuntu"));
        assert_eq!(a.mode, Mode::SearchResults);
        assert!(a.search_loading);

        // A stale outcome for another query is dropped.
        a.set_search_outcome(SearchOutcome {
            query: "other".to_string(),
            results: Vec::new(),
            failed: Vec::new(),
        });
        assert!(a.search_loading);

        let hit = SearchResult {
            title: "ubuntu-24.04.iso".to_string(),
            size: 1,
            seeders: 2,
            leechers: 3,
            source: "apibay",
            magnet: "magnet:?xt=urn:btih:x".to_string(),
        };
        a.set_search_outcome(SearchOutcome {
            query: "ubuntu".to_string(),
            results: vec![hit],
            failed: Vec::new(),
        });
        assert!(!a.search_loading);

        // Enter adds the selected magnet, tracks it, and stays in the results.
        let action = a.handle_key(KeyEvent::from(KeyCode::Enter));
        assert!(matches!(&action.commands[..], [Command::Add(m)] if m == "magnet:?xt=urn:btih:x"));
        assert_eq!(a.mode, Mode::SearchResults);
        assert_eq!(a.pending_adds.len(), 1);

        a.handle_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(a.mode, Mode::List);
    }

    #[test]
    fn sort_keeps_selected_torrent() {
        let mut a = App::new();
        a.snapshot = Snapshot::from_rows(vec![
            row(0, "charlie", RowState::Live, 0.5, 10),
            row(1, "alpha", RowState::Live, 0.1, 30),
            row(2, "bravo", RowState::Paused, 0.9, 20),
        ]);
        a.selected = 0;
        let before = a.visible_rows().get(a.selected).map(|r| r.id);
        a.cycle_sort(false);
        a.cycle_sort(true);
        let after = a.visible_rows().get(a.selected).map(|r| r.id);
        assert_eq!(before, after);
    }
}
