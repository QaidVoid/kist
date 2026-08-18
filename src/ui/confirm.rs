//! Remove-confirmation modal overlay.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::{App, Mode};
use crate::format::{format_percent, format_size, truncate_end};
use crate::ui::theme;

/// Widest the dialog grows before the torrent name is truncated.
const MAX_WIDTH: u16 = 60;
/// Narrowest dialog that still reads as a sentence.
const MIN_WIDTH: u16 = 34;

/// Render the removal confirmation.
///
/// The two outcomes are affordances in a column so their keys line up, and the
/// destructive one is marked apart. Everything here has to survive the minimum
/// supported terminal: an option the user cannot see is an option they cannot
/// weigh, and the one that would be pushed off-screen is the dangerous one.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Mode::ConfirmRemove { id } = app.mode else {
        return;
    };
    let row = app.snapshot.rows.iter().find(|row| row.id == id);
    let count = app.bulk_target_count(id);

    let width = MAX_WIDTH.clamp(MIN_WIDTH.min(area.width), area.width.saturating_sub(2));
    let inner_width = width.saturating_sub(4) as usize;

    let heading = match count > 1 {
        true => format!("Remove {count} torrents?"),
        false => "Remove torrent?".to_string(),
    };

    let mut lines: Vec<Line> = Vec::new();
    match (count > 1, row) {
        (true, _) => lines.push(Line::from(Span::styled(
            format!(" {count} torrents are marked"),
            theme::secondary(),
        ))),
        (false, Some(r)) => {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", theme::state_glyph(r.state, r.finished)),
                    theme::state_style(r.state, r.finished),
                ),
                Span::styled(
                    truncate_end(&r.name, inner_width.saturating_sub(3)),
                    theme::emphasis(),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                format!(
                    "   {} \u{b7} {} downloaded",
                    format_size(r.total_bytes),
                    format_percent(r.progress_frac())
                ),
                theme::muted(),
            )));
        }
        (false, None) => lines.push(Line::from(Span::styled(
            " (no longer in the list)",
            theme::muted(),
        ))),
    }

    lines.push(Line::from(theme::action("f", "forget, keep the files")));
    lines.push(Line::from(theme::danger_action(
        "D",
        "delete the files from disk",
    )));
    lines.push(theme::dismiss_hint("esc", "cancel"));

    // Trim the least important line first rather than letting the bottom of
    // the dialog fall off the screen.
    let max_inner = area.height.saturating_sub(2) as usize;
    while lines.len() > max_inner && lines.len() > 3 {
        lines.remove(1);
    }

    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = centered(width, height, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(theme::danger_overlay_block(&heading)),
        popup,
    );
}

/// Center a popup within `area`, clamped to the frame.
fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 3;
    Rect::new(x, y, width, height)
}
