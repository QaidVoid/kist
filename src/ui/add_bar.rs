//! Add-torrent prompt overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::App;
use crate::ui::centered_rect;

/// Render the add-torrent input prompt as a centered popup.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(70, 3, area);
    frame.render_widget(Clear, popup);

    // A trailing cursor block indicates where typed text appears.
    let content = format!("{}\u{2588}", app.input);
    let paragraph = Paragraph::new(content)
        .block(Block::bordered().title(" Add torrent (magnet / .torrent path / URL) "))
        .style(Style::new().fg(Color::Cyan));
    frame.render_widget(paragraph, popup);
}
