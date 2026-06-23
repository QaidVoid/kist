//! Torrent list/table rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState};

use crate::app::App;
use crate::format::{format_size, format_speed};
use crate::model::TorrentRow;

/// Render the torrent list (or its empty state).
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    if app.snapshot.rows.is_empty() {
        let para = Paragraph::new(Line::raw(" No torrents. Press 'a' to add one."))
            .block(Block::bordered().title("Torrents"));
        frame.render_widget(para, area);
        return;
    }

    let rows: Vec<Row> = app.snapshot.rows.iter().map(row_for).collect();
    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Size"),
        Cell::from("Progress"),
        Cell::from("Down"),
        Cell::from("Up"),
        Cell::from("Peers"),
        Cell::from("State"),
    ])
    .style(Style::new().fg(Color::Yellow));

    let widths = [
        Constraint::Min(24),
        Constraint::Length(9),
        Constraint::Length(18),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(6),
        Constraint::Length(12),
    ];

    let mut state = TableState::default().with_selected(Some(app.selected));
    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::new().bg(Color::DarkGray).fg(Color::White))
        .block(Block::bordered().title("Torrents"));

    frame.render_stateful_widget(table, area, &mut state);
}

fn row_for(row: &TorrentRow) -> Row<'_> {
    let bar = progress_bar(row.progress_frac(), 10);
    Row::new(vec![
        Cell::from(row.name.clone()),
        Cell::from(format_size(row.total_bytes)),
        Cell::from(format!("{bar} {:>5.1}%", row.progress_pct())),
        Cell::from(format_speed(row.down_speed)),
        Cell::from(format_speed(row.up_speed)),
        Cell::from(row.peers.to_string()),
        Cell::from(row.state.label()),
    ])
    .style(style_for_state(row))
}

fn style_for_state(row: &TorrentRow) -> Style {
    use crate::model::RowState;
    match row.state {
        RowState::Live if row.finished => Style::new().fg(Color::Green),
        RowState::Live => Style::new().fg(Color::Reset),
        RowState::Paused => Style::new().fg(Color::DarkGray),
        RowState::Error => Style::new().fg(Color::Red),
        RowState::Initializing => Style::new().fg(Color::Cyan),
    }
}

/// Build a fixed-width textual progress bar like `████░░░░░░`.
fn progress_bar(frac: f64, width: usize) -> String {
    let frac = frac.clamp(0.0, 1.0);
    let filled = (frac * width as f64).round() as usize;
    let filled = filled.min(width);
    let bar = "\u{2588}".repeat(filled);
    let rest = "\u{2591}".repeat(width - filled);
    format!("{bar}{rest}")
}
