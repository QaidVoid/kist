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

    let popup = centered_rect(64, 6, area);
    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .title(Span::styled(
            " Confirm removal ",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ))
        .style(Style::new().fg(Color::Yellow));
    let inner = block.inner(popup);

    // Truncate the name so the question line fits the dialog.
    let prefix = "Remove ";
    let suffix = "?";
    let budget = inner.width as usize;
    let fixed = prefix.chars().count() + suffix.chars().count();
    let max_name = budget.saturating_sub(fixed).max(1);
    let display_name = truncate(&name, max_name);

    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw(prefix.to_string()),
            Span::styled(
                display_name,
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(suffix.to_string()),
        ]),
        Line::from(vec![Span::styled(
            " Files are kept on disk.",
            Style::new().fg(Color::DarkGray),
        )]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                " y",
                Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" confirm    "),
            Span::styled(
                "n",
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" / "),
            Span::styled(
                "esc",
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" cancel"),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

/// Truncate `s` to `max` display characters, appending an ellipsis if cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{kept}\u{2026}")
}
