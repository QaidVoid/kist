//! Help overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::ui::centered_rect;

/// Render the keybindings help popup.
pub fn render(frame: &mut Frame, area: Rect) {
    let popup = centered_rect(54, 14, area);
    frame.render_widget(Clear, popup);

    let title = Line::from(vec![Span::styled(
        " kist keybindings ",
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )]);

    let lines = vec![
        Line::raw(""),
        Line::raw(" a        add a torrent"),
        Line::raw(" j / k     move down / up"),
        Line::raw(" p / space pause selected"),
        Line::raw(" r        resume selected"),
        Line::raw(" enter    toggle pause / resume"),
        Line::raw(" d        remove (keep files)"),
        Line::raw(" ?        toggle this help"),
        Line::raw(" q        quit"),
        Line::raw(" ctrl+c   quit"),
        Line::raw(""),
        Line::raw(" esc cancels the current prompt or quits"),
    ];

    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(title)),
        popup,
    );
}
