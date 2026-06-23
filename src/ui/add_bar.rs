//! Add-torrent prompt overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;

/// Render the add-torrent input prompt as a centered popup with a real cursor.
///
/// Only the window of input around the cursor that fits the box is rendered, so
/// long magnet links stay reachable. Magnet links are ASCII, so one character
/// occupies one column and char-based windowing is sufficient.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(70, 3, area);
    frame.render_widget(Clear, popup);

    let block = Block::bordered().title(" Add torrent (magnet / .torrent path / URL) ");
    let inner = block.inner(popup);
    // Reserve one trailing column so the cursor always sits inside the box.
    let width = (inner.width as usize).saturating_sub(1).max(1);

    let chars: Vec<char> = app.input.chars().collect();
    let cur_char = app.input[..app.cursor].chars().count();
    let total = chars.len();
    let start = cur_char.min(total.saturating_sub(width));
    let end = (start + width).min(total);
    let visible: String = chars[start..end].iter().collect();

    let paragraph = Paragraph::new(visible)
        .style(Style::new().fg(Color::Cyan))
        .block(block);
    frame.render_widget(paragraph, popup);

    // Place the terminal cursor where the next character will be typed.
    let col = (cur_char - start) as u16;
    frame.set_cursor_position((inner.x + col, inner.y));
}
