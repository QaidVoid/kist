//! Indexer search: query prompt and results overlay.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Clear, Paragraph, Row, Table, TableState};

use crate::app::App;
use crate::format::{format_size, truncate_end};
use crate::ui::{centered_rect, theme};

/// Gap ratatui inserts between table columns.
const COLUMN_SPACING: u16 = 1;

/// Fixed (non-Name) column widths: Size, Seeds, Leech, Source.
const FIXED_COLS: [u16; 4] = [9, 6, 6, 6];

/// Render the search query prompt as a centered single-line input.
pub fn render_input(frame: &mut Frame, area: Rect, app: &App) {
    let popup = centered_rect(50, 3, area);
    frame.render_widget(Clear, popup);

    let block = theme::block().title(theme::title(" Search torrents ".to_string()));
    let inner = block.inner(popup);

    let paragraph = Paragraph::new(app.input.as_str())
        .style(Style::new().fg(theme::ACCENT))
        .block(block);
    frame.render_widget(paragraph, popup);

    let cur_char = app.input[..app.cursor].chars().count() as u16;
    frame.set_cursor_position((inner.x + cur_char, inner.y));
}

/// Render the search results as a large centered table overlay.
pub fn render_results(frame: &mut Frame, area: Rect, app: &App) {
    let height = area.height.saturating_sub(4).clamp(8, area.height.max(1));
    let popup = centered_rect(90, height, area);
    frame.render_widget(Clear, popup);

    let title_budget = (popup.width as usize).saturating_sub(20).max(8);
    let title = format!(
        " Search: {} ",
        truncate_end(&app.search_query, title_budget)
    );
    let block = theme::block().title(theme::title(title));

    if app.search_loading {
        let lines = vec![
            Line::raw(""),
            Line::from(Span::styled(
                " Searching\u{2026}",
                Style::new().fg(theme::DIM),
            )),
        ];
        frame.render_widget(Paragraph::new(lines).block(block), popup);
        return;
    }

    if app.search_results.is_empty() {
        let lines = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled(" No results. Press ", theme::header_style()),
                Span::styled("f", theme::key_style()),
                Span::styled(" to search again.", theme::header_style()),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines).block(block), popup);
        return;
    }

    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from(Line::raw("Size").alignment(Alignment::Right)),
        Cell::from(Line::raw("Seeds").alignment(Alignment::Right)),
        Cell::from(Line::raw("Leech").alignment(Alignment::Right)),
        Cell::from("Source"),
    ])
    .style(theme::header_style());

    let fixed: u16 = FIXED_COLS.iter().map(|w| w + COLUMN_SPACING).sum();
    let name_budget = (popup.width.saturating_sub(2 + fixed) as usize).max(1);

    let rows: Vec<Row> = app
        .search_results
        .iter()
        .map(|r| {
            Row::new(vec![
                Cell::from(truncate_end(&r.title, name_budget)),
                Cell::from(Line::raw(format_size(r.size)).alignment(Alignment::Right)),
                Cell::from(
                    Line::styled(r.seeders.to_string(), Style::new().fg(theme::OK))
                        .alignment(Alignment::Right),
                ),
                Cell::from(Line::raw(r.leechers.to_string()).alignment(Alignment::Right)),
                Cell::from(Span::styled(r.source, Style::new().fg(theme::DIM))),
            ])
        })
        .collect();

    let mut widths = vec![Constraint::Min(1)];
    widths.extend(FIXED_COLS.iter().map(|w| Constraint::Length(*w)));

    let mut state = TableState::default().with_selected(Some(app.search_selected));
    let table = Table::new(rows, widths)
        .column_spacing(COLUMN_SPACING)
        .header(header)
        .row_highlight_style(theme::selection_style())
        .block(block);
    frame.render_stateful_widget(table, popup, &mut state);
}
