//! Terminal UI rendering.
//!
//! [`render`] is the single entry point: given a frame and the [`App`], it lays
//! out the header, torrent list, status line, and footer, then draws any active
//! overlay (add bar or help). Rendering is pure given the app state.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{App, Mode};
use crate::format::{display_width, format_speed, truncate_end};
use crate::model::RowState;

pub mod add_bar;
pub mod add_options;
pub mod confirm;
pub mod detail;
pub mod filter_bar;
pub mod help;
pub mod limits_bar;
pub mod list;
pub mod search;
pub mod theme;

/// Smallest terminal the normal layout supports (columns, rows).
const MIN_SIZE: (u16, u16) = (40, 10);

/// Fraction of the main area given to the list when the detail pane is open.
const DETAIL_LIST_PERCENT: u16 = 40;
/// Minimum list height (header row plus a few torrents) in detail mode.
const DETAIL_LIST_MIN: u16 = 5;

/// Render the whole application.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < MIN_SIZE.0 || area.height < MIN_SIZE.1 {
        render_too_small(frame, area);
        return;
    }

    let [header, main, status, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas::<4>(area);

    render_header(frame, header, app);
    if let Mode::Detail { .. } = app.mode {
        // Proportional split: compressed list on top, detail pane below.
        let list_height = (main.height * DETAIL_LIST_PERCENT / 100).max(DETAIL_LIST_MIN);
        let [list_area, detail_area] =
            Layout::vertical([Constraint::Length(list_height), Constraint::Min(0)])
                .areas::<2>(main);
        list::render(frame, list_area, app);
        detail::render(frame, detail_area, app);
    } else {
        list::render(frame, main, app);
    }
    render_status(frame, status, app);
    render_footer(frame, footer, app);

    match app.mode {
        Mode::AddBar => add_bar::render(
            frame,
            area,
            app,
            " Add torrent (magnet / .torrent path / URL) ",
        ),
        Mode::Filter => filter_bar::render(frame, area, app),
        Mode::Limits => limits_bar::render(frame, area, app),
        Mode::Help => help::render(frame, area),
        Mode::ConfirmRemove { .. } => confirm::render(frame, area, app),
        Mode::SearchInput => search::render_input(frame, area, app),
        Mode::SearchResults => search::render_results(frame, area, app),
        Mode::AddOptionsSource => add_bar::render(frame, area, app, " Add with options: source "),
        Mode::AddOptions => add_options::render_form(frame, area, app),
        Mode::AddOptionsFolder => {
            add_bar::render(frame, area, app, " Output folder (blank = default) ")
        }
        Mode::AddOptionsFiles => add_options::render_files(frame, area, app),
        Mode::Detail { .. } | Mode::List => {}
    }
}

/// Centered notice shown when the terminal is below the minimum size.
fn render_too_small(frame: &mut Frame, area: Rect) {
    let message = format!("Terminal too small (min {}x{})", MIN_SIZE.0, MIN_SIZE.1);
    let y = area.y + area.height / 2;
    let line_area = Rect::new(
        area.x,
        y.min(area.bottom().saturating_sub(1)),
        area.width,
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::raw(message)).alignment(Alignment::Center),
        line_area,
    );
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let stats = &app.snapshot.aggregate;

    // Segments in display order; trailing segments are dropped when the
    // terminal is too narrow rather than letting the line wrap.
    let mut segments: Vec<Vec<Span>> = vec![
        vec![Span::raw(" "), theme::title("kist".to_string())],
        vec![
            Span::styled(
                format!(
                    "{} torrent{}  ",
                    stats.count,
                    if stats.count == 1 { "" } else { "s" }
                ),
                Style::new().fg(theme::DIM),
            ),
            Span::styled(
                theme::state_glyph(RowState::Live, false),
                Style::new().fg(theme::ACCENT),
            ),
            Span::raw(format!(" {}  ", stats.downloading)),
            Span::styled(
                theme::state_glyph(RowState::Live, true),
                Style::new().fg(theme::OK),
            ),
            Span::raw(format!(" {}  ", stats.seeding)),
            Span::styled(
                theme::state_glyph(RowState::Paused, false),
                Style::new().fg(theme::DIM),
            ),
            Span::raw(format!(" {}", stats.paused)),
        ],
        vec![
            Span::styled(theme::GLYPH_DOWN, Style::new().fg(theme::ACCENT)),
            Span::raw(format!(
                " {}{}  ",
                format_speed(stats.total_down),
                cap_suffix(app.down_limit)
            )),
            Span::styled(theme::GLYPH_UP, Style::new().fg(theme::OK)),
            Span::raw(format!(
                " {}{}",
                format_speed(stats.total_up),
                cap_suffix(app.up_limit)
            )),
        ],
        vec![
            Span::styled("sort: ", Style::new().fg(theme::DIM)),
            Span::raw(format!("{} {}", app.sort_key.label(), app.sort_dir.glyph())),
        ],
    ];
    if let Some(filter) = &app.filter {
        segments.push(vec![
            Span::styled("filter: ", Style::new().fg(theme::DIM)),
            Span::styled(filter.clone(), Style::new().fg(theme::WARN)),
        ]);
    }

    let budget = area.width.saturating_sub(2) as usize;
    let separator = "   ";
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    for (i, segment) in segments.into_iter().enumerate() {
        let seg_width: usize = segment.iter().map(|s| display_width(&s.content)).sum();
        let sep_width = if i == 0 { 0 } else { separator.len() };
        if used + sep_width + seg_width > budget {
            break;
        }
        if i > 0 {
            spans.push(Span::raw(separator));
        }
        spans.extend(segment);
        used += sep_width + seg_width;
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(theme::block()),
        area,
    );
}

fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let budget = area.width.saturating_sub(1) as usize;
    let line = if let Some(message) = &app.status {
        let style = if app.status_is_error {
            Style::new().fg(theme::ERROR)
        } else {
            Style::new().fg(theme::OK)
        };
        Line::from(Span::styled(
            format!(" {}", truncate_end(message, budget)),
            style,
        ))
    } else {
        match app.visible_rows().get(app.selected).copied() {
            Some(row) if row.state == RowState::Error => {
                let msg = row
                    .error
                    .clone()
                    .unwrap_or_else(|| "torrent error".to_string());
                Line::from(Span::styled(
                    format!(" {}", truncate_end(&msg, budget)),
                    Style::new().fg(theme::ERROR),
                ))
            }
            Some(row) => {
                let hash = short_hash(&row.infohash);
                let name_budget = budget.saturating_sub(hash.len() + 2);
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(hash, Style::new().fg(theme::DIM)),
                    Span::raw(format!("  {}", truncate_end(&row.name, name_budget))),
                ])
            }
            // A selected pending add: show what was dispatched.
            None => match app.selected_pending() {
                Some(pending) => Line::from(Span::styled(
                    format!(" {}", truncate_end(&pending.source, budget)),
                    Style::new().fg(theme::DIM),
                )),
                None => Line::raw(" "),
            },
        }
    };
    frame.render_widget(line, area);
}

/// A `≤ cap` suffix for the header speed segment, empty when uncapped.
fn cap_suffix(limit: Option<u32>) -> String {
    match limit {
        Some(bps) => format!(" \u{2264}{}", format_speed(bps as u64)),
        None => String::new(),
    }
}

/// Shorten a hex infohash to `abcd1234…ef567890`.
fn short_hash(hash: &str) -> String {
    if hash.len() <= 17 {
        return hash.to_string();
    }
    format!("{}\u{2026}{}", &hash[..8], &hash[hash.len() - 8..])
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let hints: &[(&str, &str)] = match app.mode {
        Mode::AddBar => &[("enter", "add"), ("esc", "cancel")],
        Mode::Filter => &[("enter", "apply"), ("esc", "cancel"), ("blank", "clears")],
        Mode::SearchInput => &[("enter", "search"), ("esc", "cancel")],
        Mode::SearchResults => &[
            ("enter", "download"),
            ("j/k", "move"),
            ("f", "new search"),
            ("esc", "close"),
        ],
        Mode::Help => &[("esc/?", "close help")],
        Mode::Limits => &[("tab", "field"), ("enter", "apply"), ("esc", "cancel")],
        Mode::ConfirmRemove { .. } => &[
            ("f/y", "forget"),
            ("D", "delete files"),
            ("n/esc", "cancel"),
        ],
        Mode::AddOptionsSource => &[("enter", "next"), ("esc", "cancel")],
        Mode::AddOptions => &[
            ("p", "paused"),
            ("o", "folder"),
            ("f", "files"),
            ("enter", "add"),
            ("esc", "cancel"),
        ],
        Mode::AddOptionsFolder => &[("enter", "set"), ("esc", "back")],
        Mode::AddOptionsFiles => &[("space", "toggle"), ("j/k", "move"), ("enter/esc", "back")],
        Mode::Detail { .. } => &[
            ("tab", "cycle"),
            ("j/k", "move"),
            ("space", "file"),
            ("^d/^u", "scroll"),
            ("i/esc", "close"),
        ],
        Mode::List => &[
            ("a/A", "add"),
            ("f", "search"),
            ("j/k", "move"),
            ("i", "details"),
            ("p", "pause"),
            ("r", "resume"),
            ("d", "remove"),
            ("/", "filter"),
            ("L", "limits"),
            ("s", "sort"),
            ("?", "help"),
            ("q", "quit"),
        ],
    };

    let budget = area.width.saturating_sub(1) as usize;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    let mut used = 1;
    for (i, (key, label)) in hints.iter().enumerate() {
        let sep = if i == 0 { 0 } else { 2 };
        let width = key.len() + 1 + label.len();
        if used + sep + width > budget {
            break;
        }
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key.to_string(), theme::key_style()));
        spans.push(Span::styled(
            format!(" {label}"),
            Style::new().fg(theme::DIM),
        ));
        used += sep + width;
    }
    frame.render_widget(Line::from(spans), area);
}

/// Center a popup of `percent_x`% width and `height` rows within `area`,
/// clamped to the frame.
pub(super) fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let width = (area.width.saturating_mul(percent_x) / 100).max(1);
    let height = height.min(area.height).max(1);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + area.height.saturating_sub(height) / 3;
    Rect::new(x, y, width, height)
}
