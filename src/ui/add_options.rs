//! Add-with-options overlays: the options form and the file-selection list.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::app::App;
use crate::format::{format_size, truncate_end, truncate_middle};
use crate::ui::{centered_rect, theme};

/// Render the add-options form (paused, output folder, file preview).
pub fn render_form(frame: &mut Frame, area: Rect, app: &App) {
    let Some(state) = &app.add_options else {
        return;
    };
    let popup = centered_rect(70, 11, area);
    frame.render_widget(Clear, popup);
    let block = theme::block().title(theme::title(" Add with options ".to_string()));
    let inner = block.inner(popup);
    let budget = inner.width.saturating_sub(2) as usize;

    let paused = if state.paused { "on" } else { "off" };
    let folder = if state.output_folder.trim().is_empty() {
        "(session default)".to_string()
    } else {
        truncate_middle(&state.output_folder, budget.saturating_sub(10))
    };
    let files = if state.preview_loading {
        "resolving\u{2026}".to_string()
    } else if state.files.is_empty() {
        "not previewed".to_string()
    } else {
        let selected = state.files.iter().filter(|f| f.included).count();
        format!("{selected} / {} selected", state.files.len())
    };

    let field = |k: &str, label: &str, value: String| {
        Line::from(vec![
            Span::styled(format!(" {k}  "), theme::key_style()),
            Span::styled(format!("{label:<8}"), Style::new().fg(theme::WARN)),
            Span::raw(value),
        ])
    };

    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("source  ", Style::new().fg(theme::DIM)),
            Span::raw(truncate_end(&state.source, budget.saturating_sub(9))),
        ]),
        Line::raw(""),
        field("p", "paused", paused.to_string()),
        field("o", "folder", folder),
        field("f", "files", files),
    ];

    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

/// Render the file-selection list for the add-options preview.
pub fn render_files(frame: &mut Frame, area: Rect, app: &App) {
    let Some(state) = &app.add_options else {
        return;
    };
    let popup = centered_rect(76, 20, area);
    frame.render_widget(Clear, popup);
    let block = theme::block().title(theme::title(" Select files (space toggles) ".to_string()));
    let inner = block.inner(popup);
    let rows = inner.height as usize;
    let budget = inner.width.saturating_sub(1) as usize;

    // Window the list so the selected row stays visible.
    let top = state.file_selected.saturating_sub(rows.saturating_sub(1));
    let lines: Vec<Line> = state
        .files
        .iter()
        .enumerate()
        .skip(top)
        .take(rows)
        .map(|(i, f)| {
            let mark = if f.included { "[x]" } else { "[ ]" };
            let size = format_size(f.size);
            let name_budget = budget.saturating_sub(mark.len() + 1 + size.len() + 2 + 1);
            let text = format!(
                " {mark} {:>9}  {}",
                size,
                truncate_middle(&f.name, name_budget)
            );
            let mut style = if f.included {
                Style::new().fg(theme::OK)
            } else {
                Style::new().fg(theme::DIM)
            };
            if i == state.file_selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            Line::from(Span::styled(text, style))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines).block(block), popup);
}
