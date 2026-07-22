//! Help overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::ui::{centered_rect, theme};

/// Render the keybindings help popup.
pub fn render(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(56, 24, area);
    frame.render_widget(Clear, popup);

    let key = |k: &str| Span::styled(format!(" {:<8}", k), theme::key_style());
    let desc = |d: &str| Span::raw(d.to_string());

    let lines = vec![
        Line::raw(""),
        Line::from(vec![key("a"), desc("add a torrent")]),
        Line::from(vec![
            key("A"),
            desc("add with options (paused/folder/files)"),
        ]),
        Line::from(vec![key("f"), desc("search indexers (enter downloads)")]),
        Line::from(vec![key("j / k"), desc("move down / up")]),
        Line::from(vec![key("i"), desc("open / close torrent details")]),
        Line::from(vec![
            key("tab"),
            desc("cycle detail tab (overview/files/peers/trackers)"),
        ]),
        Line::from(vec![
            key("spc"),
            desc("in files tab: include / exclude file"),
        ]),
        Line::from(vec![
            key("^d/^u"),
            desc("scroll detail content (also pgdn/pgup)"),
        ]),
        Line::from(vec![key("p / spc"), desc("pause selected")]),
        Line::from(vec![key("r"), desc("resume selected")]),
        Line::from(vec![key("enter"), desc("toggle pause / resume")]),
        Line::from(vec![
            key("d"),
            desc("remove (confirms) / cancel pending add"),
        ]),
        Line::from(vec![key("f / D"), desc("forget / delete-with-files")]),
        Line::from(vec![key("/"), desc("filter by name (blank clears)")]),
        Line::from(vec![key("L"), desc("set global rate limits")]),
        Line::from(vec![key("s / S"), desc("cycle sort / reverse direction")]),
        Line::from(vec![key("?"), desc("toggle this help")]),
        Line::from(vec![key("q"), desc("quit")]),
        Line::from(vec![key("ctrl+c"), desc("quit")]),
        Line::raw(""),
        Line::from(vec![Span::styled(
            " esc cancels prompts and closes details",
            Style::new().fg(theme::DIM),
        )]),
        Line::from(vec![Span::styled(
            " narrow terminals hide low-priority columns",
            Style::new().fg(theme::DIM),
        )]),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(theme::block().title(theme::title(" kist keybindings ".to_string()))),
        popup,
    );
}
