//! kist: a simple terminal torrent client built on librqbit.

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use crossterm::event;

use crate::app::{Action, App, Event};
use crate::config::Cli;
use crate::engine::{Command, EngineLink};
use crate::error::Result;

mod app;
mod commands;
mod config;
mod engine;
mod error;
mod format;
mod model;
mod search;
mod ui;
mod webseed;

/// Build the panic hook: restore the terminal, then delegate to `previous`.
/// Restore runs before the delegation so the report prints into the normal
/// buffer; it is best effort because the hook also fires for panics with no
/// alternate screen active, where a failed restore is harmless.
fn panic_hook(
    previous: Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>,
) -> Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync> {
    Box::new(move |info| {
        let _ = ratatui::try_restore();
        previous(info)
    })
}

fn main() -> Result<()> {
    std::panic::set_hook(panic_hook(std::panic::take_hook()));

    let cli = Cli::parse();
    let config_path = cli.config_path()?;
    let mut config = config::load_or_init(&config_path)?;
    config.apply_overrides(&cli);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let engine = rt.block_on(engine::Engine::new(&config))?;
    let engine = Arc::new(engine);

    let _enter = rt.enter();
    let mut link = engine::spawn(engine, config.refresh_interval());

    // Pre-queue a torrent passed on the command line, if any.
    let initial = cli
        .torrent
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_string);
    if let Some(source) = &initial {
        let _ = link.commands.try_send(Command::Add(source.clone()));
    }

    let refresh = config.refresh_interval();
    let limits = (config.download_limit_bps(), config.upload_limit_bps());
    let web_seeds = config.enable_web_seeds;
    let mut terminal =
        ratatui::try_init().map_err(|e| anyhow::anyhow!("failed to initialize terminal: {e}"))?;
    let ui_result = run_ui(
        &mut terminal,
        &mut link,
        refresh,
        initial,
        limits,
        web_seeds,
    );
    let _ = ratatui::try_restore();
    ui_result?;

    // Ask the engine to shut down; the channel also closing will stop it.
    let _ = link.commands.try_send(Command::Quit);

    Ok(())
}

/// Run the terminal UI event loop.
///
/// Returns an error only for terminal I/O failures; engine/add failures are
/// surfaced as non-fatal status messages.
fn run_ui(
    terminal: &mut ratatui::DefaultTerminal,
    link: &mut EngineLink,
    refresh: Duration,
    initial: Option<String>,
    limits: (Option<u32>, Option<u32>),
    web_seeds: bool,
) -> std::io::Result<()> {
    let mut app = App::new();
    app.update_snapshot(link.snapshots.borrow().clone());
    app.down_limit = limits.0;
    app.up_limit = limits.1;
    app.web_seeds_enabled = web_seeds;
    if let Some(source) = &initial {
        app.push_pending_add(source);
    }

    loop {
        app.expire_status();
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        // Block up to `refresh` for input; a timeout becomes a refresh Tick.
        let event = if event::poll(refresh)? {
            Event::Input(event::read()?)
        } else {
            Event::Tick
        };
        let action = app.handle(event);
        apply_action(&action, &link.commands);
        if action.quit {
            break;
        }

        // Drain engine status messages.
        let mut batch = Vec::new();
        while let Ok(status) = link.status.try_recv() {
            batch.push(status);
        }
        apply_status_batch(&mut app, &batch);

        // Drain finished searches.
        while let Ok(outcome) = link.search.try_recv() {
            app.set_search_outcome(outcome);
        }

        // Drain finished add-options previews.
        while let Ok(outcome) = link.preview.try_recv() {
            app.set_preview_outcome(outcome);
        }

        // Apply a fresh snapshot if the engine published one.
        if link.snapshots.has_changed().unwrap_or(false) {
            let snapshot = link.snapshots.borrow_and_update();
            app.update_snapshot(snapshot.clone());
        }

        // Apply the latest detail snapshot when in detail mode.
        if link.detail.has_changed().unwrap_or(false) {
            let detail = link.detail.borrow_and_update().clone();
            app.set_detail(detail);
        }
    }

    Ok(())
}

/// Send the commands produced by an [`Action`] to the engine.
fn apply_action(action: &Action, commands: &tokio::sync::mpsc::Sender<Command>) {
    for command in &action.commands {
        let _ = commands.try_send(command.clone());
    }
}

/// The message a batch of engine statuses collapses into: the last error if
/// one arrived, otherwise the last message.
fn last_error_or_last(batch: &[engine::EngineStatus]) -> Option<&engine::EngineStatus> {
    batch
        .iter()
        .rev()
        .find(|s| s.is_error)
        .or_else(|| batch.last())
}

/// Apply a drained batch of engine statuses: every pending add finishes, and
/// the line shows the batch's last error if there was one, else its last
/// message, so a success cannot mask an error from the same frame.
fn apply_status_batch(app: &mut App, batch: &[engine::EngineStatus]) {
    for status in batch {
        if let Some(source) = &status.finished_add {
            app.finish_pending_add(source);
        }
    }
    if let Some(status) = last_error_or_last(batch) {
        app.set_status(status.message.clone(), status.is_error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineStatus;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn status(message: &str, is_error: bool) -> EngineStatus {
        EngineStatus {
            message: message.to_string(),
            is_error,
            finished_add: None,
        }
    }

    #[test]
    fn an_error_survives_later_successes_in_a_batch() {
        let batch = vec![
            status("added one", false),
            status("torrent gone", true),
            status("added two", false),
        ];
        assert_eq!(last_error_or_last(&batch).unwrap().message, "torrent gone");

        // Without an error the last message wins, and an empty batch shows
        // nothing at all.
        assert_eq!(
            last_error_or_last(&[status("added one", false)])
                .unwrap()
                .message,
            "added one"
        );
        assert!(last_error_or_last(&[]).is_none());
    }

    #[test]
    fn a_batch_finishes_adds_and_shows_the_last_error() {
        let mut a = App::new();
        a.push_pending_add("magnet:?xt=urn:btih:abc");
        let batch = vec![
            status("added one", false),
            EngineStatus {
                message: "torrent gone".to_string(),
                is_error: true,
                finished_add: Some("magnet:?xt=urn:btih:abc".to_string()),
            },
            status("added two", false),
        ];

        apply_status_batch(&mut a, &batch);

        assert!(a.pending_adds.is_empty(), "every finished_add must apply");
        assert_eq!(a.status.as_deref(), Some("torrent gone"));
        assert!(a.status_is_error);
    }

    #[test]
    fn panic_hook_restores_and_delegates() {
        let called = Arc::new(AtomicBool::new(false));
        let previous_called = called.clone();
        let previous = Box::new(move |_info: &std::panic::PanicHookInfo<'_>| {
            previous_called.store(true, Ordering::SeqCst);
        }) as Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync>;

        std::panic::set_hook(panic_hook(previous));

        // With no alternate screen active the restore attempt is a harmless
        // error, and the previous hook must still run.
        let result = std::panic::catch_unwind(|| panic!("boom"));
        assert!(result.is_err());
        assert!(called.load(Ordering::SeqCst), "previous hook must run");
    }
}
