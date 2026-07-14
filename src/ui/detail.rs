//! Torrent detail pane, shown below a compressed list.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::{App, DetailTab, Mode};
use crate::format::{
    format_duration, format_percent, format_ratio, format_size, format_speed, truncate_end,
    truncate_middle,
};
use crate::model::DetailSnapshot;
use crate::ui::theme;

/// Sparkline glyphs from empty to full (U+2581..U+2588).
const SPARK: [&str; 8] = [
    "\u{2581}", "\u{2582}", "\u{2583}", "\u{2584}", "\u{2585}", "\u{2586}", "\u{2587}", "\u{2588}",
];

/// Fixed width of the key column in key/value lines.
const KEY_WIDTH: usize = 10;

/// Render the detail pane for the torrent currently in detail mode.
///
/// The tab indicator stays pinned to the top while the tab body scrolls; the
/// stored scroll offset is clamped to the content here so key handling can
/// over-shoot freely.
pub fn render(frame: &mut Frame, area: Rect, app: &mut App) {
    let Mode::Detail { id } = app.mode else {
        return;
    };

    let title_name = app
        .detail
        .as_ref()
        .map(|d| d.name.clone())
        .unwrap_or_else(|| format!("torrent {id}"));
    let title_budget = (area.width as usize).saturating_sub(14);
    let block = theme::block().title(theme::title(format!(
        " Details: {} ",
        truncate_end(&title_name, title_budget)
    )));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [tab_area, body_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas::<2>(inner);
    frame.render_widget(Paragraph::new(tab_line(app.detail_tab)), tab_area);

    // Reserve the rightmost column for the scrollbar.
    let text_area = Rect {
        width: body_area.width.saturating_sub(1),
        ..body_area
    };
    let width = text_area.width as usize;

    let lines = match app.detail.as_ref() {
        Some(d) => detail_lines(d, app, width),
        None => vec![Line::raw(" (no data yet)")],
    };

    app.detail_page = body_area.height.max(1);
    let max_scroll = (lines.len() as u16).saturating_sub(body_area.height);
    app.detail_scroll = app.detail_scroll.min(max_scroll);

    let total = lines.len();
    frame.render_widget(
        Paragraph::new(lines).scroll((app.detail_scroll, 0)),
        text_area,
    );

    if total > body_area.height as usize {
        let mut sb_state =
            ScrollbarState::new(max_scroll as usize).position(app.detail_scroll as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            body_area,
            &mut sb_state,
        );
    }
}

/// Build the tab indicator line, highlighting the active tab.
fn tab_line(tab: DetailTab) -> Line<'static> {
    let mut spans = Vec::new();
    let tabs = [
        DetailTab::Overview,
        DetailTab::Files,
        DetailTab::Peers,
        DetailTab::Trackers,
    ];
    for (i, t) in tabs.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let label = format!(" {} ", t.label());
        if t == tab {
            spans.push(Span::styled(
                label,
                Style::new().fg(ratatui::style::Color::Black).bg(theme::ACCENT),
            ));
        } else {
            spans.push(Span::styled(label, Style::new().fg(theme::DIM)));
        }
    }
    Line::from(spans)
}

/// Content lines for the active tab.
fn detail_lines(d: &DetailSnapshot, app: &App, width: usize) -> Vec<Line<'static>> {
    match app.detail_tab {
        DetailTab::Overview => overview_lines(d, app, width),
        DetailTab::Files => file_lines(d, width),
        DetailTab::Peers => peer_lines(d, app, width),
        DetailTab::Trackers => tracker_lines(d, width),
    }
}

fn overview_lines(d: &DetailSnapshot, app: &App, width: usize) -> Vec<Line<'static>> {
    let frac = if d.total_bytes == 0 {
        0.0
    } else {
        d.progress_bytes as f64 / d.total_bytes as f64
    };
    let mut lines = vec![
        kv_line(
            "State",
            format!(
                "{} {}",
                theme::state_glyph(d.state, d.finished),
                theme::state_label(d.state, d.finished)
            ),
        ),
        kv_line(
            "Progress",
            format!(
                "{}  ({} / {})",
                format_percent(frac),
                format_size(d.progress_bytes),
                format_size(d.total_bytes)
            ),
        ),
        kv_line(
            "ETA",
            match d.eta {
                Some(eta) => format_duration(eta),
                None => theme::NONE.to_string(),
            },
        ),
        kv_line(
            "Speed",
            format!(
                "{} {}  {} {}",
                theme::GLYPH_DOWN,
                format_speed(d.down_speed),
                theme::GLYPH_UP,
                format_speed(d.up_speed)
            ),
        ),
        kv_line(
            "Uploaded",
            format!(
                "{}  ratio {}",
                format_size(d.uploaded_bytes),
                format_ratio(d.ratio())
            ),
        ),
        kv_line("Peers", d.peers.to_string()),
        kv_line("Info hash", d.infohash.clone()),
    ];

    let value_width = width.saturating_sub(KEY_WIDTH + 2);
    if let Some(pieces) = &d.pieces
        && !pieces.is_empty()
        && value_width >= 8
    {
        let have = pieces.iter().filter(|b| **b).count();
        lines.push(Line::raw(""));
        lines.push(kv_line("Pieces", format!("{have} / {}", pieces.len())));
        lines.push(Line::from(vec![
            Span::raw(format!(" {:<KEY_WIDTH$} ", "")),
            Span::styled(piece_map(pieces, value_width), Style::new().fg(theme::ACCENT)),
        ]));
    }

    if value_width >= 8 {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<KEY_WIDTH$} ", "Down"), Style::new().fg(theme::WARN)),
            Span::styled(
                sparkline(&app.detail_down_history, value_width),
                Style::new().fg(theme::ACCENT),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<KEY_WIDTH$} ", "Up"), Style::new().fg(theme::WARN)),
            Span::styled(
                sparkline(&app.detail_up_history, value_width),
                Style::new().fg(theme::OK),
            ),
        ]));
    }

    lines
}

/// One-line piece map: each cell aggregates a range of pieces and renders a
/// density glyph for the completed fraction of that range.
fn piece_map(pieces: &[bool], width: usize) -> String {
    let cells = width.min(pieces.len()).max(1);
    let mut out = String::with_capacity(cells * 3);
    for i in 0..cells {
        let start = i * pieces.len() / cells;
        let end = ((i + 1) * pieces.len() / cells).max(start + 1);
        let range = &pieces[start..end];
        let have = range.iter().filter(|b| **b).count();
        let frac = have as f64 / range.len() as f64;
        out.push_str(if frac >= 1.0 {
            "\u{2588}" // █
        } else if frac >= 0.5 {
            "\u{2593}" // ▓
        } else if frac > 0.0 {
            "\u{2592}" // ▒
        } else {
            "\u{2591}" // ░
        });
    }
    out
}

/// Right-aligned sparkline of the most recent samples, scaled to their max.
fn sparkline(history: &std::collections::VecDeque<u64>, width: usize) -> String {
    let samples: Vec<u64> = history
        .iter()
        .copied()
        .rev()
        .take(width)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let max = samples.iter().copied().max().unwrap_or(0);
    let mut out = String::new();
    for _ in samples.len()..width {
        out.push(' ');
    }
    for s in samples {
        let idx = if max == 0 {
            0
        } else {
            ((s as f64 / max as f64) * (SPARK.len() - 1) as f64).round() as usize
        };
        out.push_str(SPARK[idx.min(SPARK.len() - 1)]);
    }
    out
}

/// Lines for the files tab: one row per file with progress, size, and path.
fn file_lines(d: &DetailSnapshot, width: usize) -> Vec<Line<'static>> {
    if d.files.is_empty() {
        return vec![Line::from(Span::styled(
            " (no file metadata yet)",
            Style::new().fg(theme::DIM),
        ))];
    }
    let path_budget = width.saturating_sub(1 + 6 + 2 + 9 + 2).max(8);
    d.files
        .iter()
        .map(|f| {
            Line::from(vec![
                Span::styled(
                    format!(" {:>6}", format_percent(f.frac())),
                    Style::new().fg(theme::OK),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("{:>9}", format_size(f.size)),
                    Style::new().fg(theme::DIM),
                ),
                Span::raw("  "),
                Span::raw(truncate_middle(&f.name, path_budget)),
            ])
        })
        .collect()
}

/// Lines for the peers tab: address, state, downloaded total, derived speed.
fn peer_lines(d: &DetailSnapshot, app: &App, width: usize) -> Vec<Line<'static>> {
    if d.peer_rows.is_empty() {
        return vec![Line::from(Span::styled(
            " No peers connected.",
            Style::new().fg(theme::DIM),
        ))];
    }
    let addr_budget = width.saturating_sub(1 + 2 + 10 + 2 + 10 + 2 + 10).clamp(9, 21);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {:<addr_budget$}  {:<10}  {:>10}  {:>10}",
            "Address", "State", "Down", "Speed"
        ),
        theme::header_style(),
    ))];
    lines.extend(d.peer_rows.iter().map(|p| {
        let speed = match app.peer_speeds.get(&p.addr) {
            Some(bps) => format_speed(*bps),
            None => theme::NONE.to_string(),
        };
        Line::raw(format!(
            " {:<addr_budget$}  {:<10}  {:>10}  {:>10}",
            truncate_end(&p.addr, addr_budget),
            truncate_end(&p.state, 10),
            format_size(p.fetched_bytes),
            speed
        ))
    }));
    lines
}

/// Lines for the trackers tab: one announce URL per line.
fn tracker_lines(d: &DetailSnapshot, width: usize) -> Vec<Line<'static>> {
    if d.trackers.is_empty() {
        return vec![Line::from(Span::styled(
            " No trackers (DHT/PEX only).",
            Style::new().fg(theme::DIM),
        ))];
    }
    d.trackers
        .iter()
        .map(|t| Line::raw(format!(" {}", truncate_end(t, width.saturating_sub(1)))))
        .collect()
}

/// A labelled key/value line with a themed key and a plain value.
fn kv_line(key: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:<KEY_WIDTH$} ", key), Style::new().fg(theme::WARN)),
        Span::raw(value),
    ])
}
