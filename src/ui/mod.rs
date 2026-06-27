//! Terminal UI rendering.
//!
//! [`render`] is the single entry point: given a frame and the [`App`], it lays
//! out the header, torrent list, status line, and footer, then draws any active
//! overlay (add bar or help). Rendering is pure given the app state.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, Mode};
use crate::format::format_speed;
pub mod add_bar;
pub mod confirm;
pub mod detail;
pub mod filter_bar;
pub mod help;
pub mod list;

/// Render the whole application.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [header, main, status, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas::<4>(area);

    render_header(frame, header, app);
    if let Mode::Detail { .. } = app.mode {
        // Compressed list on top, detail pane below.
        let [list_area, detail_area] =
            Layout::vertical([Constraint::Length(9), Constraint::Min(0)]).areas::<2>(main);
        list::render(frame, list_area, app);
        detail::render(frame, detail_area, app);
    } else {
        list::render(frame, main, app);
    }
    render_status(frame, status, app);
    render_footer(frame, footer, app);

    match app.mode {
        Mode::AddBar => add_bar::render(frame, area, app),
        Mode::Filter => filter_bar::render(frame, area, app),
        Mode::Help => help::render(frame, area),
        Mode::ConfirmRemove { .. } => confirm::render(frame, area, app),
        Mode::Detail { .. } | Mode::List => {}
    }
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let stats = &app.snapshot.aggregate;
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            "kist",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  {} torrents  {} active · {} seeding · {} paused",
            stats.count, stats.downloading, stats.seeding, stats.paused
        )),
        Span::raw(format!(
            "  \u{2193} {}  \u{2191} {}",
            format_speed(stats.total_down),
            format_speed(stats.total_up)
        )),
        Span::raw(format!(
            "  sort:{}{}",
            app.sort_key.label(),
            app.sort_dir.glyph()
        )),
    ];
    if let Some(filter) = &app.filter {
        spans.push(Span::raw("  filter:"));
        spans.push(Span::styled(filter, Style::new().fg(Color::Yellow)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(Block::bordered()),
        area,
    );
}

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    use crate::model::RowState;
    let line = if let Some(message) = &app.status {
        let style = if app.status_is_error {
            Style::new().fg(Color::Red)
        } else {
            Style::new().fg(Color::Green)
        };
        Line::from(Span::styled(format!(" {message}"), style))
    } else {
        match app.visible_rows().get(app.selected).copied() {
            Some(row) if row.state == RowState::Error => {
                let msg = row
                    .error
                    .clone()
                    .unwrap_or_else(|| "torrent error".to_string());
                Line::from(Span::styled(format!(" {msg}"), Style::new().fg(Color::Red)))
            }
            Some(row) => Line::from(vec![
                Span::raw(" "),
                Span::styled(row.infohash.clone(), Style::new().fg(Color::DarkGray)),
                Span::raw(format!("  {}", row.name)),
            ]),
            None => Line::raw(" "),
        }
    };
    frame.render_widget(line, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let hints = match app.mode {
        Mode::AddBar => "enter: add  esc: cancel",
        Mode::Filter => "enter: apply  esc: cancel  (blank clears)",
        Mode::Help => "esc / ?: close help",
        Mode::ConfirmRemove { .. } => "y: remove  n / esc: cancel",
        Mode::Detail { .. } => "tab:cycle  j/k:move  ^d/^u:scroll  i/esc:close  q:quit",
        Mode::List => {
            "a:add  j/k:move  i:details  p:pause  r:resume  d:remove  /:filter  s:sort  ?:help  q:quit"
        }
    };
    frame.render_widget(Line::raw(format!(" {hints}")), area);
}

/// Center a popup of `percent_x`% width and `height` rows within `area`.
pub(super) fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let width = area.width.saturating_mul(percent_x) / 100;
    let width = width.max(1);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + area.height.saturating_sub(height) / 3;
    Rect::new(x, y, width, height)
}
