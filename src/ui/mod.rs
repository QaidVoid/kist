//! Terminal UI rendering.
//!
//! [`render`] is the single entry point: given a frame and the [`App`], it lays
//! out the header, torrent list, status line, and footer, then draws any active
//! overlay (add bar or help). Rendering is pure given the app state.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Mode};
use crate::format::{display_width, format_speed, truncate_end};
use crate::model::RowState;

pub mod add_bar;
pub mod add_options;
pub mod confirm;
pub mod detail;
pub mod filter_bar;
#[cfg(test)]
pub mod harness;
pub mod help;
#[cfg(test)]
mod layout_tests;
pub mod limits_bar;
pub mod list;
pub mod palette;
pub mod search;
pub mod theme;

/// Smallest terminal the normal layout supports (columns, rows).
pub(crate) const MIN_SIZE: (u16, u16) = (40, 10);

/// How much horizontal room the UI has to work with.
///
/// Every region that rearranges on width resolves it here, so a resize moves
/// them all on the same render instead of each module picking its own cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Width {
    /// Room for one column of content and little else.
    Compact,
    /// The ordinary case: the full table, single-column detail.
    Medium,
    /// Enough room to lay content out side by side.
    Wide,
}

/// Narrowest terminal considered [`Width::Medium`].
pub const BREAKPOINT_MEDIUM: u16 = 72;
/// Narrowest terminal considered [`Width::Wide`].
pub const BREAKPOINT_WIDE: u16 = 100;

impl Width {
    /// Classify a terminal width.
    pub fn of(width: u16) -> Self {
        match width {
            w if w >= BREAKPOINT_WIDE => Width::Wide,
            w if w >= BREAKPOINT_MEDIUM => Width::Medium,
            _ => Width::Compact,
        }
    }
}

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

    let [header, main, status] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas::<3>(area);

    render_header(frame, header, app);
    if app.detail_target_id().is_some() {
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

    match app.mode {
        Mode::AddBar => add_bar::render(frame, area, app, "Add torrent"),
        Mode::Filter => filter_bar::render(frame, area, app),
        Mode::Limits => limits_bar::render(frame, area, app),
        Mode::Help => help::render(frame, area),
        Mode::ConfirmRemove { .. } => confirm::render(frame, area, app),
        Mode::SearchInput => search::render_input(frame, area, app),
        Mode::SearchResults => search::render_results(frame, area, app),
        Mode::AddOptionsSource => add_bar::render(frame, area, app, "Add with options"),
        Mode::AddOptions => add_options::render_form(frame, area, app),
        Mode::AddOptionsFolder => add_bar::render(frame, area, app, "Output folder"),
        Mode::AddOptionsFiles => add_options::render_files(frame, area, app),
        Mode::WebSeedPrompt { .. } => add_bar::render(frame, area, app, "Add web seed"),
        Mode::Palette => palette::render(frame, area, app),
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

/// Draw the header: one line of summary plus a separating rule.
///
/// Counts and transfer rates are kept apart and never share a glyph. The state
/// arrows belong to the rows, so here the counts are spelled out and the arrows
/// mean only "download rate" and "upload rate".
fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let stats = &app.snapshot.aggregate;
    let rule = Block::new()
        .borders(Borders::BOTTOM)
        .border_style(theme::muted());
    let line_area = rule.inner(area);
    frame.render_widget(rule, area);

    // Segments in priority order; trailing ones are dropped when the terminal
    // is too narrow rather than letting the line wrap.
    let mut segments: Vec<Vec<Span>> = vec![
        vec![theme::title("kist".to_string())],
        vec![Span::styled(
            format!(
                "{} torrent{}",
                stats.count,
                if stats.count == 1 { "" } else { "s" }
            ),
            theme::secondary(),
        )],
        vec![
            Span::styled(theme::GLYPH_DOWN, theme::accent()),
            Span::styled(
                format!(
                    " {}{}  ",
                    format_speed(stats.total_down),
                    cap_suffix(app.down_limit)
                ),
                theme::secondary(),
            ),
            Span::styled(theme::GLYPH_UP, theme::success()),
            Span::styled(
                format!(
                    " {}{}",
                    format_speed(stats.total_up),
                    cap_suffix(app.up_limit)
                ),
                theme::secondary(),
            ),
        ],
    ];

    // A filter or a pending bulk action changes what every other number means,
    // so they outrank the per-state breakdown when room is short.
    if let Some(filter) = &app.filter {
        segments.insert(
            2,
            vec![
                Span::styled("filter ", theme::muted()),
                Span::styled(filter.clone(), theme::warning()),
            ],
        );
    }
    if app.marked_count() > 0 {
        segments.insert(
            2,
            vec![Span::styled(
                format!("{} marked", app.marked_count()),
                theme::marked_style(),
            )],
        );
    }
    if let Some(counts) = state_counts(stats) {
        segments.push(vec![Span::styled(counts, theme::muted())]);
    }
    segments.push(vec![
        Span::styled("sort ", theme::muted()),
        Span::styled(
            format!("{} {}", app.sort_key.label(), app.sort_dir.glyph()),
            theme::secondary(),
        ),
    ]);

    let budget = line_area.width.saturating_sub(2) as usize;
    let separator = "   ";
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    let mut used = 1;
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

    frame.render_widget(Paragraph::new(Line::from(spans)), line_area);
}

/// Spell out the non-zero per-state counts, or `None` when there is nothing to
/// say. Words rather than glyphs, so counts cannot be misread as rates.
fn state_counts(stats: &crate::model::AggregateStats) -> Option<String> {
    let parts: Vec<String> = [
        (stats.downloading, "downloading"),
        (stats.seeding, "seeding"),
        (stats.paused, "paused"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, label)| format!("{n} {label}"))
    .collect();
    match parts.is_empty() {
        true => None,
        false => Some(parts.join(", ")),
    }
}

/// Draw the one bottom row.
///
/// It carries whatever is most worth saying right now, in priority order: a
/// transient message, then a problem with the selected torrent, then the
/// selected torrent's full name when the list truncated it, and otherwise the
/// key hints. The name matters because the Name column truncates, so this is
/// the only place a long name can be read in full.
fn render_status(frame: &mut Frame, area: Rect, app: &App) {
    let budget = area.width.saturating_sub(1) as usize;

    // While an overlay is open its own keys are the only ones that apply, so
    // the row belongs to it rather than to whatever is selected behind it.
    if !matches!(app.mode, Mode::List | Mode::Detail { .. }) {
        render_hints(frame, area, app);
        return;
    }

    if let Some(message) = &app.status {
        let style = match app.status_is_error {
            true => theme::danger(),
            false => theme::success(),
        };
        let line = Line::from(Span::styled(
            format!(" {}", truncate_end(message, budget)),
            style,
        ));
        frame.render_widget(line, area);
        return;
    }

    let selected = app.visible_rows().get(app.selected).copied();
    if let Some(row) = selected
        && row.state == RowState::Error
    {
        let message = row
            .error
            .clone()
            .unwrap_or_else(|| "torrent error".to_string());
        let line = Line::from(vec![
            Span::styled(
                format!(" {} ", theme::state_glyph(row.state, false)),
                theme::danger(),
            ),
            Span::styled(
                truncate_end(&message, budget.saturating_sub(3)),
                theme::danger(),
            ),
        ]);
        frame.render_widget(line, area);
        return;
    }

    // A pending add has no row yet, so show what was dispatched.
    if selected.is_none()
        && let Some(pending) = app.selected_pending()
    {
        let line = Line::from(Span::styled(
            format!(" {}", truncate_end(&pending.source, budget)),
            theme::muted(),
        ));
        frame.render_widget(line, area);
        return;
    }

    // Reading the whole name beats reading the hints, but help must stay
    // reachable, so it is pinned to the right rather than dropped.
    if let Some(row) = selected
        && display_width(&row.name) > list::name_budget(area.width)
    {
        const PINNED: &str = "? help  q quit";
        let name_budget = budget.saturating_sub(PINNED.len() + 3);
        let name = truncate_end(&row.name, name_budget);
        let pad = budget
            .saturating_sub(display_width(&name) + PINNED.len() + 1)
            .max(1);
        frame.render_widget(
            Line::from(vec![
                Span::styled(format!(" {name}"), theme::secondary()),
                Span::raw(" ".repeat(pad)),
                Span::styled("?", theme::key_style()),
                Span::styled(" help  ", theme::muted()),
                Span::styled("q", theme::key_style()),
                Span::styled(" quit", theme::muted()),
            ]),
            area,
        );
        return;
    }

    render_hints(frame, area, app);
}

/// A `≤ cap` suffix for the header speed segment, empty when uncapped.
fn cap_suffix(limit: Option<u32>) -> String {
    match limit {
        Some(bps) => format!(" \u{2264}{}", format_speed(bps as u64)),
        None => String::new(),
    }
}

/// Draw the key hints for the current mode.
fn render_hints(frame: &mut Frame, area: Rect, app: &App) {
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
        Mode::WebSeedPrompt { .. } => &[("enter", "attach"), ("esc", "cancel")],
        Mode::Palette => &[("enter", "run"), ("up/down", "move"), ("esc", "cancel")],
        Mode::Detail { .. } if app.detail_tab == crate::app::DetailTab::Sources => &[
            ("tab", "cycle"),
            ("j/k", "move"),
            ("w", "attach"),
            ("d", "detach"),
            ("i/esc", "close"),
        ],
        Mode::Detail { .. } => &[
            ("tab", "cycle"),
            ("j/k", "move"),
            ("space", "file"),
            ("w", "web seed"),
            ("^d/^u", "scroll"),
            ("i/esc", "close"),
        ],
        // Generated from the command table so the footer cannot drift from
        // what the keys actually do.
        Mode::List => &list_hints(app),
    };

    // Reserve room for the "more exist" indicator so truncation is never
    // silent: dropping hints without saying so is how a user ends up unable to
    // find help or quit.
    let budget = area.width.saturating_sub(1) as usize;
    let indicator_width = display_width(theme::GLYPH_MORE) + 2;
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    let mut used = 1;
    let mut shown = 0;
    for (i, (key, label)) in hints.iter().enumerate() {
        let sep = if i == 0 { 0 } else { 2 };
        let width = key.len() + 1 + label.len();
        // Every hint but the last must leave room for the indicator.
        let reserve = if i + 1 == hints.len() {
            0
        } else {
            indicator_width
        };
        if used + sep + width + reserve > budget {
            break;
        }
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key.to_string(), theme::key_style()));
        spans.push(Span::styled(format!(" {label}"), theme::muted()));
        used += sep + width;
        shown += 1;
    }
    if shown < hints.len() {
        spans.push(Span::styled(
            format!("  {}", theme::GLYPH_MORE),
            theme::muted(),
        ));
    }
    frame.render_widget(Line::from(spans), area);
}

/// Footer hints for list mode, essentials first.
///
/// Movement is not a command in the table, so it is prepended here; everything
/// else comes from the table, ordered so the essential hints survive
/// truncation on a narrow terminal.
fn list_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let has_torrent = !app.snapshot.rows.is_empty();
    let mut hints = vec![("j/k", "move")];
    let mut rest = Vec::new();
    for spec in crate::commands::COMMANDS {
        let Some(key) = spec.key else { continue };
        if spec.needs_torrent && !has_torrent {
            continue;
        }
        match spec.essential {
            true => hints.push((key, spec.short)),
            false => rest.push((key, spec.short)),
        }
    }
    hints.extend(rest);
    hints
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
