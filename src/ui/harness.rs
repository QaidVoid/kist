//! Test-only rendering harness.
//!
//! Draws the whole UI into an in-memory buffer at a fixed terminal size so
//! layout can be asserted without a real terminal. Every layout bug found so
//! far was invisible to unit tests and only showed up on screen; this is what
//! makes those bugs testable.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

use crate::app::App;

/// Smallest terminal the UI claims to support, from [`crate::ui::MIN_SIZE`].
pub const MIN: (u16, u16) = super::MIN_SIZE;

/// A narrow but ordinary terminal.
pub const NARROW: (u16, u16) = (80, 24);

/// A roomy terminal.
pub const WIDE: (u16, u16) = (120, 40);

/// Render `app` at `width` x `height` and return the screen as plain text,
/// one line per row with trailing blanks trimmed.
pub fn render_to_text(app: &mut App, (width, height): (u16, u16)) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| super::render(frame, app))
        .expect("draw");

    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            let line: String = (0..width)
                .map(|x| {
                    buffer
                        .cell((x, y))
                        .map(|cell| cell.symbol())
                        .unwrap_or(" ")
                        .to_string()
                })
                .collect();
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every terminal size the layout assertions cover.
pub fn sizes() -> [(u16, u16); 3] {
    [MIN, NARROW, WIDE]
}

/// Render `app` and return each row's cell foreground colours.
///
/// Text-only assertions cannot see colour, which is where a redesign is most
/// likely to quietly drop meaning: a state that stops being green still reads
/// correctly as text while telling the user nothing at a glance.
pub fn render_colors(app: &mut App, (width, height): (u16, u16)) -> Vec<Vec<Color>> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| super::render(frame, app))
        .expect("draw");

    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    buffer
                        .cell((x, y))
                        .map(|cell| cell.fg)
                        .unwrap_or(Color::Reset)
                })
                .collect()
        })
        .collect()
}

/// The foreground colour of the text `needle` itself, wherever it appears.
///
/// Sharper than looking at a whole row: a row "contains" the state colour as
/// soon as its one-character glyph is coloured, which says nothing about
/// whether the row reads as that state at a glance.
pub fn color_of_text(app: &mut App, size: (u16, u16), needle: &str) -> Color {
    let text = render_to_text(app, size);
    let (row, col) = text
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find(needle).map(|x| (y, line[..x].chars().count())))
        .unwrap_or_else(|| panic!("no text {needle:?} in:\n{text}"));
    render_colors(app, size)[row][col]
}

/// Assert that nothing rendered at `size` exceeds the terminal width.
///
/// Wide characters occupy more than one column, so this measures display width
/// rather than counting `char`s.
pub fn assert_within_width(app: &mut App, size: (u16, u16), what: &str) {
    let (width, _) = size;
    let text = render_to_text(app, size);
    for (row, line) in text.lines().enumerate() {
        let used = crate::format::display_width(line);
        assert!(
            used <= width as usize,
            "{what} at {width} columns: row {row} is {used} columns wide\n{line}"
        );
    }
}
