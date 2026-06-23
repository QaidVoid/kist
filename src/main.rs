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
mod config;
mod engine;
mod error;
mod format;
mod model;
mod ui;

fn main() -> Result<()> {
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
    if let Some(source) = cli
        .torrent
        .as_deref()
        .map(str::trim)
        .filter(|source| !source.is_empty())
    {
        let _ = link.commands.try_send(Command::Add(source.to_string()));
    }

    let refresh = config.refresh_interval();
    let mut terminal =
        ratatui::try_init().map_err(|e| anyhow::anyhow!("failed to initialize terminal: {e}"))?;
    let ui_result = run_ui(&mut terminal, &mut link, refresh);
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
) -> std::io::Result<()> {
    let mut app = App::new();
    app.update_snapshot(link.snapshots.borrow().clone());

    loop {
        app.expire_status();
        terminal.draw(|frame| ui::render(frame, &app))?;

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
        while let Ok(status) = link.status.try_recv() {
            app.set_status(status.message, status.is_error);
        }

        // Apply a fresh snapshot if the engine published one.
        if link.snapshots.has_changed().unwrap_or(false) {
            let snapshot = link.snapshots.borrow_and_update();
            app.update_snapshot(snapshot.clone());
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
