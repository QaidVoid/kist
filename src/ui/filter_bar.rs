//! Filter-entry prompt overlay (single line).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Clear, Paragraph};

use crate::app::App;
use crate::ui::{centered_rect, theme};

/// Render the list filter entry as a centered single-line input with a cursor.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(50, 3, area);
    frame.render_widget(Clear, popup);

    let block = theme::block().title(theme::title(" Filter (substring, blank to clear) ".to_string()));
    let inner = block.inner(popup);

    let paragraph = Paragraph::new(app.input.as_str())
        .style(Style::new().fg(theme::WARN))
        .block(block);
    frame.render_widget(paragraph, popup);

    let cur_char = app.input[..app.cursor].chars().count() as u16;
    frame.set_cursor_position((inner.x + cur_char, inner.y));
}
