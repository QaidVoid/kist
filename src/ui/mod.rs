#![allow(dead_code)] // entry `render` consumed by main in a later task group

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
use crate::model::AggregateStats;

pub mod add_bar;
pub mod help;
pub mod list;

/// Render the whole application.
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let [header, main, status, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas::<4>(area);

    render_header(frame, header, &app.snapshot.aggregate);
    list::render(frame, main, app);
    render_status(frame, status, app);
    render_footer(frame, footer, app);

    match app.mode {
        Mode::AddBar => add_bar::render(frame, area, app),
        Mode::Help => help::render(frame, area),
        Mode::List => {}
    }
}

fn render_header(frame: &mut Frame, area: Rect, stats: &AggregateStats) {
    let line = Line::from(vec![
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
            fmt_speed(stats.total_down),
            fmt_speed(stats.total_up)
        )),
    ]);
    frame.render_widget(Paragraph::new(line).block(Block::bordered()), area);
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
        match app.snapshot.rows.get(app.selected) {
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
        Mode::Help => "esc / ?: close help",
        Mode::List => "a:add  j/k:move  p:pause  r:resume  d:remove  ?:help  q:quit",
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

/// Format a byte count with binary units, e.g. `1.4 GiB`.
pub(super) fn fmt_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Format a bytes-per-second rate, e.g. `1.4 MiB/s`.
pub(super) fn fmt_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", fmt_bytes(bytes_per_sec))
}
