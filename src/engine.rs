#![allow(dead_code)] // public API consumed by main in a later task group

//! librqbit session wrapper and command handling.
//!
//! The [`Engine`] owns the librqbit [`Session`] and exposes thin async methods
//! plus a synchronous [`Engine::snapshot`] for the UI. [`spawn`] runs the engine
//! on a background task, taking commands on a channel and publishing snapshots
//! (and status messages) back to the UI.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, ManagedTorrent, Session, SessionOptions, TorrentStatsState};
use tokio::sync::{mpsc, watch};

use crate::config::Config;
use crate::error;
use crate::model::{RowState, Snapshot, TorrentRow};

/// Owns the librqbit session and translates it into plain view models.
pub struct Engine {
    session: Arc<Session>,
}

impl Engine {
    /// Create and start a librqbit session from the given [`Config`].
    ///
    /// Returns an error if the session cannot initialize (for example an
    /// invalid or unwritable download directory).
    pub async fn new(config: &Config) -> Result<Self> {
        let opts = SessionOptions {
            disable_dht: !config.enable_dht,
            listen_port_range: Some(config.listen_port_range()),
            ..Default::default()
        };
        let session = Session::new_with_opts(config.download_directory.clone(), opts)
            .await
            .context("failed to initialize torrent session")?;
        Ok(Self { session })
    }

    /// Add a torrent from a magnet link, `.torrent` file path, or URL.
    pub async fn add(&self, source: String) -> Result<()> {
        let add = AddTorrent::from_cli_argument(&source)
            .with_context(|| format!("invalid torrent source: {source:?}"))?;
        self.session
            .add_torrent(add, None)
            .await
            .context("failed to add torrent")?;
        Ok(())
    }

    /// Pause the torrent with the given id.
    pub async fn pause(&self, id: usize) -> Result<()> {
        let handle = self.find_handle(id)?;
        self.session
            .pause(&handle)
            .await
            .with_context(|| format!("failed to pause torrent {id}"))
    }

    /// Resume the torrent with the given id.
    pub async fn resume(&self, id: usize) -> Result<()> {
        let handle = self.find_handle(id)?;
        self.session
            .unpause(&handle)
            .await
            .with_context(|| format!("failed to resume torrent {id}"))
    }

    /// Forget the torrent with the given id, keeping any downloaded files.
    pub async fn remove(&self, id: usize) -> Result<()> {
        self.session
            .delete(TorrentIdOrHash::Id(id), false)
            .await
            .with_context(|| format!("failed to remove torrent {id}"))
    }

    /// Build a consistent snapshot of all torrents without performing I/O.
    pub fn snapshot(&self) -> Snapshot {
        let rows: Vec<TorrentRow> = self
            .session
            .with_torrents(|torrents| torrents.map(|(id, handle)| to_row(id, handle)).collect());
        Snapshot::from_rows(rows)
    }

    fn find_handle(&self, id: usize) -> Result<Arc<ManagedTorrent>> {
        self.session
            .get(TorrentIdOrHash::Id(id))
            .with_context(|| format!("torrent {id} not found"))
    }
}

/// Map a librqbit managed torrent into a plain [`TorrentRow`].
fn to_row(id: usize, handle: &ManagedTorrent) -> TorrentRow {
    let stats = handle.stats();
    let infohash = handle.shared().info_hash.as_string();
    let name = handle.name().unwrap_or_else(|| infohash.clone());

    let (down_speed, up_speed, peers) = match &stats.live {
        Some(live) => (
            mbps_to_bytes(live.download_speed.mbps),
            mbps_to_bytes(live.upload_speed.mbps),
            live.snapshot.peer_stats.live,
        ),
        None => (0, 0, 0),
    };

    TorrentRow {
        id,
        name,
        infohash,
        total_bytes: stats.total_bytes,
        progress_bytes: stats.progress_bytes,
        finished: stats.finished,
        down_speed,
        up_speed,
        peers,
        state: to_row_state(stats.state),
        error: stats.error,
    }
}

fn to_row_state(state: TorrentStatsState) -> RowState {
    match state {
        TorrentStatsState::Initializing => RowState::Initializing,
        TorrentStatsState::Live => RowState::Live,
        TorrentStatsState::Paused => RowState::Paused,
        TorrentStatsState::Error => RowState::Error,
    }
}

/// Convert librqbit's MiB/s speed into bytes per second.
fn mbps_to_bytes(mbps: f64) -> u64 {
    (mbps * 1024.0 * 1024.0) as u64
}

/// Commands the UI sends to the engine.
#[derive(Debug)]
pub enum Command {
    /// Add a torrent from the given source string.
    Add(String),
    /// Pause a torrent by id.
    Pause(usize),
    /// Resume a torrent by id.
    Resume(usize),
    /// Forget a torrent by id (files are kept).
    Remove(usize),
    /// Shut the engine task down.
    Quit,
}

/// A discrete status notification published by the engine for the UI.
pub struct EngineStatus {
    /// Human-readable message, already formatted for display.
    pub message: String,
    /// Whether this originated from an error.
    pub is_error: bool,
}

/// Connection back to the UI from a spawned engine task.
pub struct EngineLink {
    /// Send commands to the engine.
    pub commands: mpsc::Sender<Command>,
    /// Latest snapshot (always readable; coalesces rapid updates).
    pub snapshots: watch::Receiver<Snapshot>,
    /// Discrete status/error messages from the engine.
    pub status: mpsc::UnboundedReceiver<EngineStatus>,
}

/// Spawn the engine task.
///
/// The task consumes [`Command`]s, applies them, and publishes a fresh
/// [`Snapshot`] after each change as well as on a fixed `refresh` tick. Status
/// messages (success or failure) are emitted on the status channel.
pub fn spawn(engine: Arc<Engine>, refresh: Duration) -> EngineLink {
    let (command_tx, mut command_rx) = mpsc::channel::<Command>(32);
    let (snapshot_tx, snapshot_rx) = watch::channel(engine.snapshot());
    let (status_tx, status_rx) = mpsc::unbounded_channel::<EngineStatus>();

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(refresh);
        loop {
            tokio::select! {
                biased;
                cmd = command_rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    if matches!(cmd, Command::Quit) {
                        break;
                    }
                    if let Some(status) = handle_command(&engine, cmd).await {
                        let _ = status_tx.send(status);
                    }
                    let _ = snapshot_tx.send(engine.snapshot());
                }
                _ = ticker.tick() => {
                    let _ = snapshot_tx.send(engine.snapshot());
                }
            }
        }
    });

    EngineLink {
        commands: command_tx,
        snapshots: snapshot_rx,
        status: status_rx,
    }
}

/// Apply a single command, returning a status message if one should be shown.
async fn handle_command(engine: &Engine, cmd: Command) -> Option<EngineStatus> {
    let result: Result<String> = match cmd {
        Command::Add(source) => engine
            .add(source)
            .await
            .map(|_| "added torrent".to_string()),
        Command::Pause(id) => engine
            .pause(id)
            .await
            .map(|_| format!("paused torrent {id}")),
        Command::Resume(id) => engine
            .resume(id)
            .await
            .map(|_| format!("resumed torrent {id}")),
        Command::Remove(id) => engine
            .remove(id)
            .await
            .map(|_| format!("removed torrent {id}")),
        Command::Quit => return None,
    };
    match result {
        Ok(message) => Some(EngineStatus {
            message,
            is_error: false,
        }),
        Err(e) => Some(EngineStatus {
            message: error::to_status_line(&e),
            is_error: true,
        }),
    }
}
