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
use librqbit::{
    AddTorrent, ManagedTorrent, Session, SessionOptions, TorrentStats, TorrentStatsState,
};
use tokio::sync::{mpsc, watch};

use crate::config::Config;
use crate::error;
use crate::model::{DetailFile, DetailSnapshot, RowState, Snapshot, TorrentRow};

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
            .with_context(|| format!("failed to add torrent: {source}"))?;
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

    /// Build a detail snapshot for one torrent, or `None` if it is gone.
    ///
    /// Per-file progress is paired defensively with file metadata so a metadata
    /// state change can never panic.
    pub fn detail(&self, id: usize) -> Option<DetailSnapshot> {
        let handle = self.session.get(TorrentIdOrHash::Id(id))?;
        let stats = handle.stats();
        let infohash = handle.shared().info_hash.as_string();
        let name = handle.name().unwrap_or_else(|| infohash.clone());
        let (down_speed, up_speed, peers) = live_speeds(&stats);
        let file_progress = stats.file_progress.clone();

        let files = handle
            .with_metadata(|m| {
                m.file_infos
                    .iter()
                    .enumerate()
                    .map(|(i, fi)| DetailFile {
                        name: fi.relative_filename.to_string_lossy().to_string(),
                        size: fi.len,
                        have: file_progress.get(i).copied().unwrap_or(0).min(fi.len),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Some(DetailSnapshot {
            name,
            infohash,
            state: to_row_state(stats.state),
            total_bytes: stats.total_bytes,
            progress_bytes: stats.progress_bytes,
            uploaded_bytes: stats.uploaded_bytes,
            down_speed,
            up_speed,
            finished: stats.finished,
            peers,
            files,
        })
    }

    fn find_handle(&self, id: usize) -> Result<Arc<ManagedTorrent>> {
        self.session
            .get(TorrentIdOrHash::Id(id))
            .with_context(|| format!("torrent {id} not found"))
    }
}

/// Extract download/upload speeds and the live peer count from torrent stats.
fn live_speeds(stats: &TorrentStats) -> (u64, u64, usize) {
    match &stats.live {
        Some(live) => (
            mbps_to_bytes(live.download_speed.mbps),
            mbps_to_bytes(live.upload_speed.mbps),
            live.snapshot.peer_stats.live,
        ),
        None => (0, 0, 0),
    }
}

/// Map a librqbit managed torrent into a plain [`TorrentRow`].
fn to_row(id: usize, handle: &ManagedTorrent) -> TorrentRow {
    let stats = handle.stats();
    let infohash = handle.shared().info_hash.as_string();
    let name = handle.name().unwrap_or_else(|| infohash.clone());
    let (down_speed, up_speed, peers) = live_speeds(&stats);

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
#[derive(Debug, Clone)]
pub enum Command {
    /// Add a torrent from the given source string.
    Add(String),
    /// Pause a torrent by id.
    Pause(usize),
    /// Resume a torrent by id.
    Resume(usize),
    /// Forget a torrent by id (files are kept).
    Remove(usize),
    /// Begin publishing detail snapshots for the given torrent id.
    FetchDetail(usize),
    /// Stop publishing detail snapshots.
    StopDetail,
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
    /// Latest per-torrent detail snapshot, or `None` when not in detail mode.
    pub detail: watch::Receiver<Option<DetailSnapshot>>,
    /// Discrete status/error messages from the engine.
    pub status: mpsc::UnboundedReceiver<EngineStatus>,
}

/// Spawn the engine task.
///
/// The task consumes [`Command`]s, applies them, and publishes a fresh
/// [`Snapshot`] after each change as well as on a fixed `refresh` tick. Status
/// messages (success or failure) are emitted on the status channel. While a
/// [`Command::FetchDetail`] id is active, a [`DetailSnapshot`] is republished
/// on each tick.
pub fn spawn(engine: Arc<Engine>, refresh: Duration) -> EngineLink {
    let (command_tx, mut command_rx) = mpsc::channel::<Command>(32);
    let (snapshot_tx, snapshot_rx) = watch::channel(engine.snapshot());
    let (detail_tx, detail_rx) = watch::channel::<Option<DetailSnapshot>>(None);
    let (status_tx, status_rx) = mpsc::unbounded_channel::<EngineStatus>();

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(refresh);
        // Torrent id currently shown in the detail pane, if any.
        let mut detail_id: Option<usize> = None;
        loop {
            tokio::select! {
                biased;
                cmd = command_rx.recv() => {
                    let Some(cmd) = cmd else { break };
                    match cmd {
                        Command::Quit => break,
                        // Cheap flag-setting commands are handled inline.
                        Command::FetchDetail(id) => {
                            detail_id = Some(id);
                            match engine.detail(id) {
                                Some(d) => { let _ = detail_tx.send(Some(d)); }
                                None => {
                                    detail_id = None;
                                    let _ = detail_tx.send(None);
                                    let _ = status_tx.send(EngineStatus {
                                        message: format!("torrent {id} not found"),
                                        is_error: true,
                                    });
                                }
                            }
                        }
                        Command::StopDetail => {
                            detail_id = None;
                            let _ = detail_tx.send(None);
                        }
                        // Action commands run in their own task so a slow
                        // operation (e.g. resolving magnet metadata over the
                        // network) does not block snapshots or other commands.
                        other => {
                            let engine = engine.clone();
                            let snapshot_tx = snapshot_tx.clone();
                            let status_tx = status_tx.clone();
                            tokio::spawn(async move {
                                if let Some(status) = handle_command(&engine, other).await {
                                    let _ = status_tx.send(status);
                                }
                                let _ = snapshot_tx.send(engine.snapshot());
                            });
                        }
                    }
                }
                _ = ticker.tick() => {
                    let _ = snapshot_tx.send(engine.snapshot());
                    if let Some(id) = detail_id {
                        match engine.detail(id) {
                            Some(d) => { let _ = detail_tx.send(Some(d)); }
                            None => {
                                detail_id = None;
                                let _ = detail_tx.send(None);
                            }
                        }
                    }
                }
            }
        }
    });

    EngineLink {
        commands: command_tx,
        snapshots: snapshot_rx,
        detail: detail_rx,
        status: status_rx,
    }
}

/// Apply a single action command, returning a status message if one should be shown.
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
        // Detail and quit are handled by the spawn loop, not here.
        Command::FetchDetail(_) | Command::StopDetail | Command::Quit => return None,
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

#[cfg(test)]
mod tests {
    use librqbit::Magnet;

    #[test]
    fn supported_magnet_formats() {
        // A realistic BTv1 magnet with & query params parses fine.
        let v1 = "magnet:?xt=urn:btih:cab507494d02ebb1178b38f2e9d7be299c86b862&dn=ubuntu&tr=udp://example.org:1337";
        assert!(Magnet::parse(v1).is_ok(), "BTv1 40-hex magnet should parse");

        // Uppercase hex is accepted too.
        let upper = "magnet:?xt=urn:btih:CAB507494D02EBB1178B38F2E9D7BE299C86B862";
        assert!(
            Magnet::parse(upper).is_ok(),
            "uppercase BTv1 magnet should parse"
        );

        // A BTv2 multihash magnet (urn:btmh:1220...) also parses.
        let v2 = "magnet:?xt=urn:btmh:1220caf1e1c30e81cb361b9ee167c4aa64228a7fa4fa9f6105232b28ad099f3a302e";
        assert!(
            Magnet::parse(v2).is_ok(),
            "BTv2 multihash magnet should parse"
        );

        // A 64-hex value placed under urn:btih: (a BTv2 hash in a btih field) is rejected.
        let bad =
            "magnet:?xt=urn:btih:caf1e1c30e81cb361b9ee167c4aa64228a7fa4fa9f6105232b28ad099f3a302e";
        let err = match Magnet::parse(bad) {
            Ok(_) => panic!("expected the 64-hex-under-btih magnet to be rejected"),
            Err(e) => e.to_string(),
        };
        assert!(
            err.contains("length 40 or 32"),
            "expected length error, got: {err}"
        );
    }
}
