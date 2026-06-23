//! Add-torrent prompt overlay, rendered as a wrapping textbox.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;

/// Render the add-torrent prompt as a fixed-height centered textbox.
///
/// The input is a single logical line; it is hard-wrapped at the box width into
/// visual lines for readability. The view scrolls vertically only when the
/// cursor would leave the box (edge-scrolling), so the cursor can move freely
/// within the visible area. Magnet links are ASCII, so one character occupies
/// one column and char-based wrapping is sufficient.
pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let popup = centered_rect(70, 5, area);
    frame.render_widget(Clear, popup);

    let block = Block::bordered().title(" Add torrent (magnet / .torrent path / URL) ");
    let inner = block.inner(popup);
    let width = (inner.width as usize).max(1);
    let visible_rows = (inner.height as usize).max(1);
    // Remember the wrap width so Up/Down movement matches the rendered layout.
    app.wrap_width = width;

    let chars: Vec<char> = app.input.chars().collect();
    let total = chars.len();
    let cur_char = app.input[..app.cursor].chars().count();
    let cursor_line = cur_char / width;
    let cursor_col = cur_char % width;

    let content_lines = if total == 0 { 1 } else { total.div_ceil(width) };
    // Ensure the cursor's line is always a real rendered line.
    let lines = content_lines.max(cursor_line + 1);

    // Edge-scroll: only move the view when the cursor would leave it.
    if cursor_line < app.view_top {
        app.view_top = cursor_line;
    } else if cursor_line >= app.view_top + visible_rows {
        app.view_top = cursor_line - visible_rows + 1;
    }
    let view_top = app.view_top;

    let visible: String = (0..visible_rows)
        .filter_map(|r| {
            let line = view_top + r;
            if line < lines {
                let start = line * width;
                let end = (start + width).min(total);
                Some(chars[start..end].iter().collect::<String>())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let paragraph = Paragraph::new(visible)
        .style(Style::new().fg(Color::Cyan))
        .block(block);
    frame.render_widget(paragraph, popup);

    // Place the terminal cursor where the next character will be typed.
    let row = cursor_line - view_top;
    frame.set_cursor_position((inner.x + cursor_col as u16, inner.y + row as u16));
}
