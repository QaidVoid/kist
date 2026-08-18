//! Command palette: a searchable list of everything kist can do.
//!
//! It doubles as the discovery path for commands that have no key, which is
//! what lets the footer truncate safely on a narrow terminal.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::App;
use crate::format::truncate_end;
use crate::ui::{centered_rect, theme};

/// Rows of commands shown before the list starts scrolling.
const MAX_ROWS: usize = 10;

/// Render the palette: the query, the matching commands, and their keys.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let entries = app.palette_entries();
    let selected = app.palette_selected.min(entries.len().saturating_sub(1));

    // Scroll the window rather than the whole list, so the highlight stays put.
    let visible = entries.len().min(MAX_ROWS);
    let first = selected.saturating_sub(visible.saturating_sub(1));

    let height = (visible as u16 + 4).min(area.height);
    let popup = centered_rect(60, height, area);
    frame.render_widget(Clear, popup);

    let block = theme::overlay_block("Run a command");
    let inner = block.inner(popup);
    let width = inner.width as usize;

    let mut lines = vec![Line::from(vec![
        Span::styled(" \u{203a} ", theme::accent()),
        Span::styled(app.input.clone(), theme::primary()),
    ])];

    if entries.is_empty() {
        lines.push(theme::validation("no command matches"));
    } else {
        for (i, spec) in entries.iter().enumerate().skip(first).take(visible) {
            let key = spec.key.unwrap_or("");
            let label_budget = width.saturating_sub(10);
            let label = truncate_end(spec.label, label_budget);
            let style = match i == selected {
                true => theme::selection_style(),
                false => theme::secondary(),
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" {:<label_budget$} ", label), style),
                Span::styled(format!("{key:>5} "), theme::muted()),
            ]));
        }
    }
    lines.push(theme::dismiss_hint("esc", "cancel"));

    frame.render_widget(Paragraph::new(lines).block(block), popup);

    // Put the terminal cursor after the typed query.
    let cursor = app.input[..app.cursor].chars().count() as u16;
    frame.set_cursor_position((inner.x + 3 + cursor, inner.y));
}
