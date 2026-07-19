//! Torrent list/table rendering with width-adaptive columns.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};

use crate::app::{App, PendingAdd};
use crate::format::{format_duration, format_ratio, format_size, format_speed, truncate_end};
use crate::model::{RowState, TorrentRow};
use crate::ui::theme;

/// Space the percent label occupies after the progress bar: ` 100.0%`.
const PERCENT_LABEL_WIDTH: usize = 7;
/// Minimum usable width of the flexible Name column.
const NAME_MIN_WIDTH: u16 = 24;
/// Gap ratatui inserts between table columns.
const COLUMN_SPACING: u16 = 1;

/// The optional (non-Name) columns in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColId {
    Size,
    Progress,
    Eta,
    Down,
    Up,
    Peers,
    Ratio,
}

struct Col {
    id: ColId,
    header: &'static str,
    width: u16,
    align: Alignment,
    /// Higher survives longer when the terminal narrows.
    priority: u8,
}

/// Display order. Drop order (narrowing) is ascending priority:
/// Ratio, Peers, ETA, Up, Size, Down, Progress.
const COLS: [Col; 7] = [
    Col {
        id: ColId::Size,
        header: "Size",
        width: 9,
        align: Alignment::Right,
        priority: 4,
    },
    Col {
        id: ColId::Progress,
        header: "Progress",
        width: 24,
        align: Alignment::Left,
        priority: 6,
    },
    Col {
        id: ColId::Eta,
        header: "ETA",
        width: 7,
        align: Alignment::Right,
        priority: 2,
    },
    Col {
        id: ColId::Down,
        header: "Down",
        width: 11,
        align: Alignment::Right,
        priority: 5,
    },
    Col {
        id: ColId::Up,
        header: "Up",
        width: 11,
        align: Alignment::Right,
        priority: 3,
    },
    Col {
        id: ColId::Peers,
        header: "Peers",
        width: 5,
        align: Alignment::Right,
        priority: 1,
    },
    Col {
        id: ColId::Ratio,
        header: "Ratio",
        width: 5,
        align: Alignment::Right,
        priority: 0,
    },
];

/// Pick the columns that fit `width`, dropping the lowest priority first.
/// Returns the kept columns (display order) and the resulting Name width.
fn fit_columns(width: u16) -> (Vec<&'static Col>, u16) {
    let mut kept: Vec<&Col> = COLS.iter().collect();
    loop {
        let fixed: u16 = kept.iter().map(|c| c.width + COLUMN_SPACING).sum();
        let name_width = width.saturating_sub(fixed);
        if name_width >= NAME_MIN_WIDTH || kept.is_empty() {
            return (kept, name_width.max(1));
        }
        let min_priority = kept.iter().map(|c| c.priority).min().unwrap();
        kept.retain(|c| c.priority != min_priority);
    }
}

/// Render the torrent list (or its empty state).
///
/// Adds still resolving metadata are appended as placeholder rows so a
/// dispatched add is visible before the engine registers the torrent; they can
/// be selected and cancelled with `d`.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let block = theme::block().title(theme::title(" Torrents ".to_string()));

    if app.snapshot.rows.is_empty() && app.pending_adds.is_empty() {
        let lines = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled(" No torrents yet. Press ", theme::header_style()),
                Span::styled("a", theme::key_style()),
                Span::styled(" to add one.", theme::header_style()),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }

    let visible = app.visible_rows();
    if visible.is_empty() && app.pending_adds.is_empty() {
        let lines = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled(
                    " No torrents match the filter. Press ",
                    theme::header_style(),
                ),
                Span::styled("/", theme::key_style()),
                Span::styled(" to clear it.", theme::header_style()),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }

    // Interior width: area minus the two border columns.
    let (cols, name_width) = fit_columns(area.width.saturating_sub(2));

    let mut header_cells = vec![Cell::from("Name")];
    header_cells.extend(
        cols.iter()
            .map(|c| Cell::from(Line::raw(c.header).alignment(c.align))),
    );
    let header = Row::new(header_cells).style(theme::header_style());

    let mut widths = vec![Constraint::Min(NAME_MIN_WIDTH)];
    widths.extend(cols.iter().map(|c| Constraint::Length(c.width)));

    let mut rows: Vec<Row> = visible
        .iter()
        .map(|row| row_for(row, &cols, name_width))
        .collect();
    rows.extend(
        app.pending_adds
            .iter()
            .map(|p| pending_row(p, &cols, name_width)),
    );

    let mut state = TableState::default().with_selected(Some(app.selected));
    let table = Table::new(rows, widths)
        .column_spacing(COLUMN_SPACING)
        .header(header)
        .row_highlight_style(theme::selection_style())
        .block(block);

    frame.render_stateful_widget(table, area, &mut state);
}

fn row_for(row: &TorrentRow, cols: &[&Col], name_width: u16) -> Row<'static> {
    // Name is prefixed with the state glyph so state never relies on color alone.
    let glyph = theme::state_glyph(row.state, row.finished);
    let name_budget = (name_width as usize).saturating_sub(2);
    let name = format!("{glyph} {}", truncate_end(&row.name, name_budget));

    let mut cells = vec![Cell::from(name)];
    for col in cols {
        let text = match col.id {
            ColId::Size => format_size(row.total_bytes),
            ColId::Progress => progress_cell(row, col.width as usize),
            ColId::Eta => match row.eta {
                Some(eta) => format_duration(eta),
                None => theme::NONE.to_string(),
            },
            ColId::Down => format_speed(row.down_speed),
            ColId::Up => format_speed(row.up_speed),
            ColId::Peers => row.peers.to_string(),
            ColId::Ratio => format_ratio(row.ratio()),
        };
        cells.push(Cell::from(Line::raw(text).alignment(col.align)));
    }
    Row::new(cells).style(theme::state_style(row.state, row.finished))
}

/// Placeholder row for an add whose metadata is still resolving.
fn pending_row(pending: &PendingAdd, cols: &[&Col], name_width: u16) -> Row<'static> {
    let glyph = theme::state_glyph(RowState::Initializing, false);
    let name_budget = (name_width as usize).saturating_sub(2);
    let name = format!("{glyph} {}", truncate_end(&pending.name, name_budget));

    let mut cells = vec![Cell::from(name)];
    for col in cols {
        let text = match col.id {
            ColId::Progress => format!(
                "resolving\u{2026} {}",
                format_duration(pending.started.elapsed())
            ),
            _ => theme::NONE.to_string(),
        };
        cells.push(Cell::from(Line::raw(text).alignment(col.align)));
    }
    Row::new(cells).style(theme::state_style(RowState::Initializing, false))
}

/// Progress bar sized to the cell width, followed by the percent label.
fn progress_cell(row: &TorrentRow, cell_width: usize) -> String {
    let bar_width = cell_width.saturating_sub(PERCENT_LABEL_WIDTH).max(1);
    let bar = progress_bar(row.progress_frac(), bar_width);
    format!("{bar} {:>5.1}%", row.progress_pct())
}

/// Build a textual progress bar like `████░░░░░░` of the given width.
fn progress_bar(frac: f64, width: usize) -> String {
    let frac = frac.clamp(0.0, 1.0);
    let filled = (frac * width as f64).round() as usize;
    let filled = filled.min(width);
    format!(
        "{}{}",
        theme::BAR_FILLED.repeat(filled),
        theme::BAR_EMPTY.repeat(width - filled)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_terminal_keeps_all_columns() {
        let (cols, name) = fit_columns(120);
        assert_eq!(cols.len(), COLS.len());
        assert!(name >= NAME_MIN_WIDTH);
    }

    #[test]
    fn narrow_terminal_drops_lowest_priority_first() {
        let (cols, _) = fit_columns(70);
        let ids: Vec<ColId> = cols.iter().map(|c| c.id).collect();
        // Ratio and Peers go before ETA/Up/Size/Down/Progress.
        assert!(!ids.contains(&ColId::Ratio));
        assert!(!ids.contains(&ColId::Peers));
        assert!(ids.contains(&ColId::Progress));
    }

    #[test]
    fn tiny_width_keeps_name_only() {
        let (cols, name) = fit_columns(20);
        assert!(cols.is_empty());
        assert!(name >= 1);
    }
}
