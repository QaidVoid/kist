//! Filter-entry prompt overlay (single line).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;

/// Render the list filter entry as a centered single-line input with a cursor.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(50, 3, area);
    frame.render_widget(Clear, popup);

    let block = Block::bordered().title(" Filter (substring, blank to clear) ");
    let inner = block.inner(popup);

    let paragraph = Paragraph::new(app.input.as_str())
        .style(Style::new().fg(Color::Yellow))
        .block(block);
    frame.render_widget(paragraph, popup);

    let cur_char = app.input[..app.cursor].chars().count() as u16;
    frame.set_cursor_position((inner.x + cur_char, inner.y));
}
