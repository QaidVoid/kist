//! Remove-confirmation modal overlay.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Mode};
use crate::format::{display_width, format_size, truncate_end};
use crate::ui::theme;

/// Widest the dialog content may grow before the name is truncated.
const MAX_CONTENT_WIDTH: usize = 70;
/// Narrowest dialog that still fits the action buttons comfortably.
const MIN_CONTENT_WIDTH: usize = 40;

/// Render a centered confirmation dialog describing the torrent being removed.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Mode::ConfirmRemove { id } = app.mode else {
        return;
    };
    let row = app.snapshot.rows.iter().find(|row| row.id == id);
    let name = row
        .map(|r| r.name.clone())
        .unwrap_or_else(|| format!("torrent {id}"));

    // Fit the dialog to its content, clamped to the frame.
    let max_content = (area.width.saturating_sub(6) as usize).min(MAX_CONTENT_WIDTH);
    let name_display = truncate_end(&name, max_content.saturating_sub(5));
    let content_width = display_width(&name_display)
        .saturating_add(5)
        .clamp(MIN_CONTENT_WIDTH.min(max_content), max_content);

    let popup = centered_fixed(content_width as u16 + 2, 13, area);
    frame.render_widget(Clear, popup);

    let block = theme::block()
        .title(Span::styled(
            " Confirm removal ",
            Style::new().fg(theme::WARN).add_modifier(Modifier::BOLD),
        ))
        .border_style(Style::new().fg(theme::WARN));

    let glyph_span = match row {
        Some(r) => Span::styled(
            format!("  {} ", theme::state_glyph(r.state, r.finished)),
            theme::state_style(r.state, r.finished),
        ),
        None => Span::raw("  "),
    };
    let stats = match row {
        Some(r) => format!(
            "     {} \u{b7} {:.1}% downloaded",
            format_size(r.total_bytes),
            r.progress_pct()
        ),
        None => "     (no longer in the list)".to_string(),
    };

    let lines = vec![
        Line::raw(""),
        Line::from(Span::styled(
            " Remove this torrent?",
            Style::new().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::from(vec![
            glyph_span,
            Span::styled(
                name_display,
                Style::new().fg(theme::ERROR).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(stats, Style::new().fg(theme::DIM))),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                " f Forget ",
                Style::new()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  keeps files", Style::new().fg(theme::DIM)),
        ])
        .alignment(Alignment::Center),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                " D Delete ",
                Style::new()
                    .bg(theme::ERROR)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  deletes files from disk", Style::new().fg(theme::ERROR)),
        ])
        .alignment(Alignment::Center),
        Line::raw(""),
        Line::from(Span::styled("n / esc  cancel", Style::new().fg(theme::DIM)))
            .alignment(Alignment::Center),
    ];

    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

/// Center a fixed-size popup within `area`, clamped to the frame.
fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 3;
    Rect::new(x, y, width, height)
}
