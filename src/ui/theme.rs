//! Central visual theme: semantic roles, glyphs, and shared chrome.
//!
//! Render modules choose a role by *meaning* rather than picking a colour for
//! appearance, so emphasis stays consistent across views and a palette change
//! is a one-file edit. The palette stays within the 16 terminal colours, which
//! keeps kist legible on the many terminals whose palettes are customised.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType};

use crate::model::RowState;

/// Primary accent: titles, active tab, selection, input prompts.
pub const ACCENT: Color = Color::Cyan;
/// Ordinary content, the most prominent text level.
pub const TEXT: Color = Color::Reset;
/// Supporting values that are read after the primary content.
pub const TEXT_SECONDARY: Color = Color::Gray;
/// Chrome and metadata: read only when looked for.
pub const TEXT_MUTED: Color = Color::DarkGray;
/// Success and seeding.
pub const OK: Color = Color::Green;
/// Errors and destructive actions.
pub const ERROR: Color = Color::Red;
/// Warnings and attention (filter text, confirm dialogs).
pub const WARN: Color = Color::Yellow;

/// Downward arrow used for download rates.
pub const GLYPH_DOWN: &str = "\u{2193}";
/// Upward arrow used for upload rates.
pub const GLYPH_UP: &str = "\u{2191}";
/// Filled progress-bar cell.
pub const BAR_FILLED: &str = "\u{2588}";
/// Empty progress-bar cell.
pub const BAR_EMPTY: &str = "\u{2591}";
/// Value shown when a number is unknown or not applicable.
pub const NONE: &str = "\u{2014}";
/// Marker shown on a row selected for a bulk action.
pub const GLYPH_MARK: &str = "\u{25CF}";
/// Trailing indicator that more commands exist than the footer can show.
pub const GLYPH_MORE: &str = "\u{2026}";

/// The most prominent text level: names and primary values.
pub fn primary() -> Style {
    Style::new().fg(TEXT)
}

/// Supporting values: progress, rates, anything read second.
pub fn secondary() -> Style {
    Style::new().fg(TEXT_SECONDARY)
}

/// Chrome, hints, and metadata that should recede.
pub fn muted() -> Style {
    Style::new().fg(TEXT_MUTED)
}

/// Primary text with weight, for the one thing that matters most in a region.
pub fn emphasis() -> Style {
    Style::new().fg(TEXT).add_modifier(Modifier::BOLD)
}

/// The accent role, for titles and active elements.
pub fn accent() -> Style {
    Style::new().fg(ACCENT)
}

/// Attention without alarm: filters, warnings, reversible destructive choices.
pub fn warning() -> Style {
    Style::new().fg(WARN)
}

/// Failure, and irreversible destructive choices.
pub fn danger() -> Style {
    Style::new().fg(ERROR)
}

/// Success and completion.
pub fn success() -> Style {
    Style::new().fg(OK)
}

/// A bordered block using the theme border type, drawn as chrome so it stays
/// subordinate to the content inside it.
pub fn block() -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(muted())
}

/// A styled block title span.
pub fn title(text: String) -> Span<'static> {
    Span::styled(text, accent().add_modifier(Modifier::BOLD))
}

/// Style for table/section headers, which label content rather than being it.
pub fn header_style() -> Style {
    muted()
}

/// Style for the row under the cursor.
pub fn selection_style() -> Style {
    Style::new().bg(Color::DarkGray).fg(Color::White)
}

/// Style for a row marked for a bulk action but not under the cursor.
pub fn marked_style() -> Style {
    accent().add_modifier(Modifier::BOLD)
}

/// Style for key tokens in hints and help.
pub fn key_style() -> Style {
    accent().add_modifier(Modifier::BOLD)
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
        RowState::Live if finished => success(),
        RowState::Live => primary(),
        RowState::Paused => muted(),
        RowState::Error => danger(),
        RowState::Initializing => accent(),
    }
}

/// Framing shared by every modal overlay.
///
/// One presentation for all of them, so a user learns the shape once.
pub fn overlay_block(heading: &str) -> Block<'static> {
    block().title(title(format!(" {heading} ")))
}

/// Framing for an overlay that offers an irreversible action.
pub fn danger_overlay_block(heading: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(warning())
        .title(Span::styled(
            format!(" {heading} "),
            warning().add_modifier(Modifier::BOLD),
        ))
}

/// An action the user can take, shown as its key followed by what it does.
///
/// Actions are affordances rather than prose so the key is always adjacent to
/// its effect, and so a column of them lines up.
pub fn action(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!(" {key:>5} "), key_style()),
        Span::styled(label.to_string(), secondary()),
    ]
}

/// An action that destroys data, marked apart from its neighbours.
pub fn danger_action(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!(" {key:>5} "), danger().add_modifier(Modifier::BOLD)),
        Span::styled(label.to_string(), danger()),
    ]
}

/// The line telling the user how to leave an overlay.
pub fn dismiss_hint(keys: &str, what: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {keys:>5} "), muted().add_modifier(Modifier::BOLD)),
        Span::styled(what.to_string(), muted()),
    ])
}

/// A validation complaint shown inside a prompt while the user types.
pub fn validation(message: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {message}"), warning()))
}
