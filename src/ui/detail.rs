//! Torrent detail pane, shown below a compressed list.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};

use crate::app::{App, DetailTab, Mode};
use crate::format::{format_percent, format_ratio, format_size, format_speed};
use crate::model::DetailSnapshot;

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
    let title = Span::styled(
        format!(" Details: {title_name} "),
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    );
    let block = Block::bordered().title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [tab_area, body_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas::<2>(inner);
    frame.render_widget(Paragraph::new(tab_line(app.detail_tab)), tab_area);

    let lines = match app.detail.as_ref() {
        Some(d) => detail_lines(d, app.detail_tab),
        None => vec![Line::raw(" (no data yet)")],
    };

    app.detail_page = body_area.height;
    let max_scroll = (lines.len() as u16).saturating_sub(body_area.height);
    app.detail_scroll = app.detail_scroll.min(max_scroll);

    frame.render_widget(
        Paragraph::new(lines).scroll((app.detail_scroll, 0)),
        body_area,
    );
}

/// Build the tab indicator line, highlighting the active tab.
fn tab_line(tab: DetailTab) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, t) in [DetailTab::Overview, DetailTab::Files, DetailTab::Peers]
        .into_iter()
        .enumerate()
    {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let label = format!(" {} ", t.label());
        if t == tab {
            spans.push(Span::styled(
                label,
                Style::new().fg(Color::Black).bg(Color::Cyan),
            ));
        } else {
            spans.push(Span::raw(label));
        }
    }
    Line::from(spans)
}

/// Content lines for the active tab.
fn detail_lines(d: &DetailSnapshot, tab: DetailTab) -> Vec<Line<'static>> {
    match tab {
        DetailTab::Overview => overview_lines(d),
        DetailTab::Files => file_lines(d),
        DetailTab::Peers => peer_lines(d),
    }
}

fn overview_lines(d: &DetailSnapshot) -> Vec<Line<'static>> {
    let frac = if d.total_bytes == 0 {
        0.0
    } else {
        d.progress_bytes as f64 / d.total_bytes as f64
    };
    vec![
        kv_line("State", d.state.label().to_string()),
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
            "Speed",
            format!(
                "\u{2193} {}  \u{2191} {}",
                format_speed(d.down_speed),
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
        kv_line("Finished", d.finished.to_string()),
        kv_line("Info hash", d.infohash.clone()),
    ]
}

/// Lines for the files tab: one row per file with progress and size.
fn file_lines(d: &DetailSnapshot) -> Vec<Line<'static>> {
    if d.files.is_empty() {
        return vec![Line::raw(" (no file metadata yet)")];
    }
    d.files
        .iter()
        .map(|f| {
            Line::from(vec![
                Span::styled(format_percent(f.frac()), Style::new().fg(Color::Green)),
                Span::raw("  "),
                Span::styled(format_size(f.size), Style::new().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::raw(f.name.clone()),
            ])
        })
        .collect()
}

fn peer_lines(d: &DetailSnapshot) -> Vec<Line<'static>> {
    vec![
        kv_line("Connected", d.peers.to_string()),
        Line::from(vec![Span::styled(
            " Per-peer detail is not exposed by librqbit.",
            Style::new().fg(Color::DarkGray),
        )]),
    ]
}

/// A labelled key/value line with a yellow key and a plain value.
fn kv_line(key: &str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:<10}", key), Style::new().fg(Color::Yellow)),
        Span::raw(value),
    ])
}
