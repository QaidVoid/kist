//! Central visual theme: colors, glyphs, and border style.
//!
//! Every render module takes its styles and glyphs from here so the UI stays
//! visually consistent and a palette change is a one-file edit.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType};

use crate::model::RowState;

/// Primary accent: titles, active tab, selection, input prompts.
pub const ACCENT: Color = Color::Cyan;
/// De-emphasized text: hints, secondary values, inactive content.
pub const DIM: Color = Color::DarkGray;
/// Success and seeding.
pub const OK: Color = Color::Green;
/// Errors and destructive actions.
pub const ERROR: Color = Color::Red;
/// Warnings and attention (filter text, confirm dialogs).
pub const WARN: Color = Color::Yellow;

/// Downward arrow used for download speeds.
pub const GLYPH_DOWN: &str = "\u{2193}";
/// Upward arrow used for upload speeds.
pub const GLYPH_UP: &str = "\u{2191}";
/// Filled progress-bar cell.
pub const BAR_FILLED: &str = "\u{2588}";
/// Empty progress-bar cell.
pub const BAR_EMPTY: &str = "\u{2591}";
/// Value shown when a number is unknown or not applicable.
pub const NONE: &str = "\u{2014}";

/// A bordered block using the theme border type.
pub fn block() -> Block<'static> {
    Block::bordered().border_type(BorderType::Rounded)
}

/// A styled block title span.
pub fn title(text: String) -> Span<'static> {
    Span::styled(text, Style::new().fg(ACCENT).add_modifier(Modifier::BOLD))
}

/// Style for table/section headers.
pub fn header_style() -> Style {
    Style::new().fg(DIM).add_modifier(Modifier::BOLD)
}

/// Style for the selected list row.
pub fn selection_style() -> Style {
    Style::new().bg(Color::DarkGray).fg(Color::White)
}

/// Style for key tokens in hints and help.
pub fn key_style() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Glyph identifying a torrent state (seeding is finished + live).
pub fn state_glyph(state: RowState, finished: bool) -> &'static str {
    match state {
        RowState::Live if finished => GLYPH_UP,
        RowState::Live => GLYPH_DOWN,
        // U+2016 double vertical line: pause without emoji risk.
        RowState::Paused => "\u{2016}",
        // U+25CC dotted circle: checking/initializing.
        RowState::Initializing => "\u{25CC}",
        // U+2717 ballot x.
        RowState::Error => "\u{2717}",
    }
}

/// Short human label for a torrent state (seeding is finished + live).
pub fn state_label(state: RowState, finished: bool) -> &'static str {
    match state {
        RowState::Live if finished => "seeding",
        _ => state.label(),
    }
}

/// Row/text style for a torrent state (seeding is finished + live).
pub fn state_style(state: RowState, finished: bool) -> Style {
    match state {
        RowState::Live if finished => Style::new().fg(OK),
        RowState::Live => Style::new().fg(Color::Reset),
        RowState::Paused => Style::new().fg(DIM),
        RowState::Error => Style::new().fg(ERROR),
        RowState::Initializing => Style::new().fg(ACCENT),
    }
}
