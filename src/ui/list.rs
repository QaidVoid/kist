//! Torrent list/table rendering with width-adaptive columns.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState};

use crate::app::{App, PendingAdd};
use crate::format::{self, format_duration, format_ratio, format_size, format_speed, truncate_end};
use crate::model::{RowState, TorrentRow};
use crate::ui::theme;

/// Space the percent label occupies after the progress bar, including the
/// separating space before it.
const PERCENT_LABEL_WIDTH: usize = format::PERCENT_MAX_WIDTH + 1;
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
        width: format::SIZE_MAX_WIDTH as u16,
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
        width: format::DURATION_MAX_WIDTH as u16,
        align: Alignment::Right,
        priority: 2,
    },
    Col {
        id: ColId::Down,
        header: "Down",
        width: format::SPEED_MAX_WIDTH as u16,
        align: Alignment::Right,
        priority: 5,
    },
    Col {
        id: ColId::Up,
        header: "Up",
        width: format::SPEED_MAX_WIDTH as u16,
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
        width: format::RATIO_MAX_WIDTH as u16,
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

/// Display columns a torrent name gets at this overall list width.
///
/// The bottom row uses this to decide whether the name it is showing adds
/// anything the row does not already show.
pub fn name_budget(list_width: u16) -> usize {
    let (_, name_width) = fit_columns(list_width.saturating_sub(2));
    (name_width as usize).saturating_sub(4)
}

/// Render the torrent list (or its empty state).
///
/// Adds still resolving metadata are appended as placeholder rows so a
/// dispatched add is visible before the engine registers the torrent; they can
/// be selected and cancelled with `d`.
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let block = theme::block().title(theme::title(" Torrents ".to_string()));

    if app.snapshot.rows.is_empty() && app.pending_adds.is_empty() {
        render_empty(
            frame,
            area,
            block,
            vec![
                Span::styled("No torrents yet. Press ", theme::muted()),
                Span::styled("a", theme::key_style()),
                Span::styled(" to add one.", theme::muted()),
            ],
        );
        return;
    }

    let visible = app.visible_rows();
    if visible.is_empty() && app.pending_adds.is_empty() {
        let filter = app.filter.clone().unwrap_or_default();
        render_empty(
            frame,
            area,
            block,
            vec![
                Span::styled("Nothing matches ", theme::muted()),
                Span::styled(format!("\u{201c}{filter}\u{201d}"), theme::warning()),
                Span::styled(". Press ", theme::muted()),
                Span::styled("/", theme::key_style()),
                Span::styled(" to clear the filter.", theme::muted()),
            ],
        );
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
        .map(|row| row_for(row, &cols, name_width, app.marked.contains(&row.id)))
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

/// Draw an empty-state message centred in the list area rather than pinned to
/// the first row, so an empty list does not read as a rendering failure.
fn render_empty(frame: &mut Frame, area: Rect, block: Block<'static>, message: Vec<Span<'static>>) {
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let row = inner.y + inner.height.saturating_sub(1) / 2;
    let line_area = Rect::new(
        inner.x,
        row.min(inner.bottom().saturating_sub(1)),
        inner.width,
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(message)).alignment(Alignment::Center),
        line_area,
    );
}

fn row_for(row: &TorrentRow, cols: &[&Col], name_width: u16, marked: bool) -> Row<'static> {
    // Three emphasis levels: the name reads first, the values that change read
    // second, and the rest recedes. State is carried by a glyph as well as
    // colour so it never depends on colour alone.
    let mark = match marked {
        true => theme::GLYPH_MARK,
        false => " ",
    };
    let glyph = theme::state_glyph(row.state, row.finished);
    let name_budget = (name_width as usize).saturating_sub(4);
    // The name carries the state, because a one-character glyph is not enough
    // to read a row's state at a glance. Only an ordinary download stays at
    // the neutral primary weight; everything else says what it is.
    let name_style = match (row.state, row.finished) {
        (RowState::Error, _) => theme::danger().add_modifier(Modifier::BOLD),
        (RowState::Live, true) => theme::success(),
        (RowState::Live, false) => theme::primary(),
        (RowState::Paused, _) => theme::muted(),
        (RowState::Initializing, _) => theme::accent(),
    };
    let name = Line::from(vec![
        Span::styled(mark.to_string(), theme::marked_style()),
        Span::styled(
            format!("{glyph} "),
            theme::state_style(row.state, row.finished),
        ),
        Span::styled(truncate_end(&row.name, name_budget), name_style),
    ]);

    let mut cells = vec![Cell::from(name)];
    for col in cols {
        let (text, style) = match col.id {
            // Progress and the download rate are what a user watches.
            ColId::Progress => (
                progress_cell(row, col.width as usize),
                match row.finished {
                    true => theme::success(),
                    false => theme::secondary(),
                },
            ),
            ColId::Down => (format_speed(row.down_speed), theme::secondary()),
            ColId::Size => (format_size(row.total_bytes), theme::muted()),
            ColId::Eta => (
                match row.eta {
                    Some(eta) => format_duration(eta),
                    None => theme::NONE.to_string(),
                },
                theme::muted(),
            ),
            ColId::Up => (format_speed(row.up_speed), theme::muted()),
            ColId::Peers => (row.peers.to_string(), theme::muted()),
            ColId::Ratio => (format_ratio(row.ratio()), theme::muted()),
        };
        // Columns are sized from their formatters, but a value with no bound
        // (a peer count, say) must lose characters visibly rather than have an
        // end silently clipped off by the table.
        let text = truncate_end(&text, col.width as usize);
        cells.push(Cell::from(Line::styled(text, style).alignment(col.align)));
    }
    Row::new(cells)
}

/// Placeholder row for an add whose metadata is still resolving.
fn pending_row(pending: &PendingAdd, cols: &[&Col], name_width: u16) -> Row<'static> {
    let glyph = theme::state_glyph(RowState::Initializing, false);
    let name_budget = (name_width as usize).saturating_sub(4);
    let name = Line::from(vec![
        Span::raw(" "),
        Span::styled(format!("{glyph} "), theme::accent()),
        Span::styled(truncate_end(&pending.name, name_budget), theme::muted()),
    ]);

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
    let percent = crate::format::format_percent(row.progress_frac());
    format!("{bar} {percent:>width$}", width = format::PERCENT_MAX_WIDTH)
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
