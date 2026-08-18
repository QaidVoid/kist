//! Help overlay, generated from the command table.
//!
//! Deriving it means a key can never be listed here and bound to something
//! else, and a new command cannot be forgotten.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::commands::{self, Group};
use crate::ui::theme;

/// Render the keybindings help popup.
pub fn render(frame: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    for group in Group::all() {
        let entries: Vec<_> = commands::in_group(group, true)
            .into_iter()
            .filter(|spec| spec.key.is_some())
            .collect();
        if entries.is_empty() {
            continue;
        }
        lines.push(Line::from(Span::styled(
            format!(" {}", group.label()),
            theme::header_style(),
        )));
        for spec in entries {
            let key = spec.key.unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!(" {key:>6}  "), theme::key_style()),
                Span::styled(spec.label.to_string(), theme::secondary()),
            ]));
        }
    }
    lines.push(Line::from(Span::styled(
        " Press : for every command, including those with no key.",
        theme::muted(),
    )));
    lines.push(theme::dismiss_hint("esc", "close"));

    // Trim from the top rather than letting the bottom fall off the screen:
    // the dismissal hint is the one line that must always survive.
    let max_inner = area.height.saturating_sub(2) as usize;
    while lines.len() > max_inner && lines.len() > 1 {
        lines.remove(0);
    }

    // Sized to the content: a fixed percentage leaves labels truncated on a
    // narrow terminal, where the room is most needed.
    let widest = lines
        .iter()
        .map(|line| line.width() + 2)
        .max()
        .unwrap_or(20) as u16;
    let width = widest.min(area.width.saturating_sub(2));
    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = centered_fixed(width, height, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(theme::overlay_block("Keys")),
        popup,
    );
}

/// Center a fixed-size popup within `area`, clamped to the frame.
fn centered_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 3;
    Rect::new(x, y, width, height)
}
