//! Remove-confirmation modal overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::{App, Mode};
use crate::ui::centered_rect;

/// Render a centered confirmation dialog naming the torrent being removed.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Mode::ConfirmRemove { id } = app.mode else {
        return;
    };
    let name = app
        .snapshot
        .rows
        .iter()
        .find(|row| row.id == id)
        .map(|row| row.name.clone())
        .unwrap_or_else(|| format!("torrent {id}"));

    let popup = centered_rect(60, 3, area);
    frame.render_widget(Clear, popup);

    let line = Line::from(vec![
        Span::raw(" Remove "),
        Span::styled(
            name,
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::raw("?  (files are kept)"),
    ]);
    let hint = Line::raw(" y: confirm    n / esc: cancel");

    let block = Block::bordered().title(" Confirm removal ");
    let paragraph = Paragraph::new(vec![Line::raw(""), line, hint]).block(block);
    frame.render_widget(paragraph, popup);
}
