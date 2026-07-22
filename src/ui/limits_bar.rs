//! Global rate-limits form overlay with separate download/upload fields.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, LimitField};
use crate::ui::{centered_rect, theme};

/// Width reserved for the field's editable box.
const FIELD_WIDTH: usize = 12;

/// Render the rate-limits form: a Download and an Upload field.
///
/// `tab` and the arrow keys switch fields; a blank field means unlimited. The
/// terminal cursor is placed in the focused field.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(48, 8, area);
    frame.render_widget(Clear, popup);

    let block = theme::block().title(theme::title(" Rate limits ".to_string()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let form = &app.limits_form;
    let down_row = inner.y + 1;
    let up_row = inner.y + 2;

    let field_line = |label: &str, value: &str, focused: bool| {
        let box_style = if focused {
            Style::new().fg(theme::WARN).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme::DIM)
        };
        let shown = if value.is_empty() && !focused {
            "unlimited".to_string()
        } else {
            format!("{value:<FIELD_WIDTH$}")
        };
        Line::from(vec![
            Span::styled(format!("  {label:<10}"), Style::new().fg(theme::WARN)),
            Span::styled(format!("[{shown}]"), box_style),
        ])
    };

    let lines = vec![
        Line::raw(""),
        field_line("Download", &form.down, form.field == LimitField::Down),
        field_line("Upload", &form.up, form.field == LimitField::Up),
        Line::raw(""),
        Line::from(Span::styled(
            "  blank = unlimited \u{b7} units K/M/G",
            Style::new().fg(theme::DIM),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), inner);

    // Place the cursor at the end of the focused field's text.
    let (row, value) = match form.field {
        LimitField::Down => (down_row, &form.down),
        LimitField::Up => (up_row, &form.up),
    };
    // "  " + 10-wide label + "[" before the text.
    let col = inner.x + 2 + 10 + 1 + value.chars().count() as u16;
    frame.set_cursor_position((col, row));
}
