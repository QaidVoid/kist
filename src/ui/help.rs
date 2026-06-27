//! Help overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::ui::centered_rect;

/// Render the keybindings help popup.
pub fn render(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(56, 22, area);
    frame.render_widget(Clear, popup);

    let title = Line::from(vec![Span::styled(
        " kist keybindings ",
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )]);

    let key = |k: &str| Span::styled(format!(" {:<8}", k), Style::new().fg(Color::Yellow));
    let desc = |d: &str| Span::raw(d.to_string());

    let lines = vec![
        Line::raw(""),
        Line::from(vec![key("a"), desc("add a torrent")]),
        Line::from(vec![key("j / k"), desc("move down / up")]),
        Line::from(vec![key("i"), desc("open / close torrent details")]),
        Line::from(vec![
            key("tab"),
            desc("cycle detail tab (overview/files/peers)"),
        ]),
        Line::from(vec![
            key("^d/^u"),
            desc("scroll detail content (also pgdn/pgup)"),
        ]),
        Line::from(vec![key("g / G"), desc("detail top / bottom (also home/end)")]),
        Line::from(vec![key("p / spc"), desc("pause selected")]),
        Line::from(vec![key("r"), desc("resume selected")]),
        Line::from(vec![key("enter"), desc("toggle pause / resume")]),
        Line::from(vec![key("d"), desc("remove (asks to confirm)")]),
        Line::from(vec![key("y / n"), desc("confirm / cancel removal")]),
        Line::from(vec![key("/"), desc("filter by name (blank clears)")]),
        Line::from(vec![key("s"), desc("cycle sort column")]),
        Line::from(vec![key("S"), desc("reverse sort direction")]),
        Line::from(vec![key("?"), desc("toggle this help")]),
        Line::from(vec![key("q"), desc("quit")]),
        Line::from(vec![key("ctrl+c"), desc("quit")]),
        Line::raw(""),
        Line::raw(" esc cancels prompts and closes details"),
    ];

    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title)),
        popup,
    );
}
