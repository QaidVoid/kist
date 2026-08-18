//! Plain, framework-agnostic view models shared between engine and UI.
//!
//! Nothing in this module depends on librqbit. The engine is responsible for
//! translating librqbit types into the values defined here, so the UI stays
//! free of engine concerns and easy to reason about.

use std::time::Duration;

/// Coarse torrent state mirroring librqbit's `TorrentStatsState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowState {
    Initializing,
    Live,
    Paused,
    Error,
}

impl RowState {
    /// Lowercase human-readable label for display.
    pub fn label(self) -> &'static str {
        match self {
            RowState::Initializing => "initializing",
            RowState::Live => "live",
            RowState::Paused => "paused",
            RowState::Error => "error",
        }
    }
}

/// A single torrent's live state, cheap to clone and free of librqbit types.
#[derive(Debug, Clone)]
pub struct TorrentRow {
    pub id: usize,
    pub name: String,
    pub infohash: String,
    pub total_bytes: u64,
    pub progress_bytes: u64,
    pub uploaded_bytes: u64,
    pub finished: bool,
    pub down_speed: u64,
    pub up_speed: u64,
    /// Estimated time remaining, when the engine can compute one.
    pub eta: Option<Duration>,
    pub peers: usize,
    pub state: RowState,
    pub error: Option<String>,
}

impl TorrentRow {
    /// Share ratio `uploaded / downloaded` (may exceed 1.0).
    pub fn ratio(&self) -> f64 {
        if self.progress_bytes == 0 {
            0.0
        } else {
            self.uploaded_bytes as f64 / self.progress_bytes as f64
        }
    }

    /// Progress as a fraction in `0.0..=1.0`.
    pub fn progress_frac(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            ((self.progress_bytes as f64) / (self.total_bytes as f64)).clamp(0.0, 1.0)
        }
    }

    /// Progress as a percentage in `0.0..=100.0`.
    pub fn progress_pct(&self) -> f64 {
        self.progress_frac() * 100.0
    }
}

/// Aggregate totals for the whole session, shown in the header.
#[derive(Debug, Clone, Default)]
pub struct AggregateStats {
    pub total_down: u64,
    pub total_up: u64,
    pub count: usize,
    pub downloading: usize,
    pub seeding: usize,
    pub paused: usize,
}

/// A consistent snapshot of all rows plus aggregate totals.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub rows: Vec<TorrentRow>,
    pub aggregate: AggregateStats,
}

impl Snapshot {
    /// Build a snapshot from rows, computing aggregate totals in the process.
    pub fn from_rows(rows: Vec<TorrentRow>) -> Self {
        let mut aggregate = AggregateStats {
            count: rows.len(),
            ..Default::default()
        };
        for r in &rows {
            aggregate.total_down = aggregate.total_down.saturating_add(r.down_speed);
            aggregate.total_up = aggregate.total_up.saturating_add(r.up_speed);
            match r.state {
                RowState::Live => {
                    if r.finished {
                        aggregate.seeding += 1;
                    } else {
                        aggregate.downloading += 1;
                    }
                }
                RowState::Paused => aggregate.paused += 1,
                _ => {}
            }
        }
        Self { rows, aggregate }
    }
}

/// One file within a torrent's detail view.
#[derive(Debug, Clone, Default)]
pub struct DetailFile {
    /// Path of the file relative to the download root.
    pub name: String,
    /// Total size in bytes.
    pub size: u64,
    /// Bytes confirmed downloaded.
    pub have: u64,
    /// Whether this file is selected for download.
    pub included: bool,
}

impl DetailFile {
    /// Downloaded fraction in `0.0..=1.0`.
    pub fn frac(&self) -> f64 {
        if self.size == 0 {
            0.0
        } else {
            ((self.have as f64) / (self.size as f64)).clamp(0.0, 1.0)
        }
    }
}

/// One file listed by an add-time preview, before the torrent is added.
#[derive(Debug, Clone)]
pub struct PreviewFile {
    /// Path of the file relative to the download root.
    pub name: String,
    /// Total size in bytes.
    pub size: u64,
}

/// One connected peer in a torrent's detail view.
#[derive(Debug, Clone)]
pub struct PeerRow {
    /// Peer socket address as a string.
    pub addr: String,
    /// librqbit peer state name (e.g. `live`).
    pub state: String,
    /// Total payload bytes fetched from this peer.
    pub fetched_bytes: u64,
    /// Whether this "peer" is one of kist's own web seed bridges rather than a
    /// member of the swarm.
    pub web_seed: bool,
}

/// State of an attached web seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSeedState {
    /// Attached, but the torrent does not need it: paused, finished, or gone.
    Idle,
    /// The torrent needs it, but it is not connected yet.
    Connecting,
    /// Connected and available to serve.
    Active,
    /// Retrying after a transient HTTP failure.
    BackingOff,
    /// Permanently failed; no longer serving.
    Failed,
}

impl WebSeedState {
    /// Lowercase human-readable label for display.
    pub fn label(self) -> &'static str {
        match self {
            WebSeedState::Idle => "idle",
            WebSeedState::Connecting => "connecting",
            WebSeedState::Active => "active",
            WebSeedState::BackingOff => "backing off",
            WebSeedState::Failed => "failed",
        }
    }
}

/// One web seed attached to a torrent, in the detail view.
#[derive(Debug, Clone)]
pub struct WebSeedRow {
    /// The HTTP source URL as the user entered it.
    pub url: String,
    pub state: WebSeedState,
    /// Payload bytes this seed has served to the session.
    pub served_bytes: u64,
    /// Why the seed failed, when it has.
    pub error: Option<String>,
}

/// A detailed view of a single torrent, fetched on demand for the detail pane.
///
/// This is independent of the lightweight list [`Snapshot`] so reading detail
/// data does not increase the cost of the regular list refresh.
#[derive(Debug, Clone)]
pub struct DetailSnapshot {
    pub name: String,
    pub infohash: String,
    pub state: RowState,
    pub total_bytes: u64,
    pub progress_bytes: u64,
    pub uploaded_bytes: u64,
    pub down_speed: u64,
    pub up_speed: u64,
    /// Estimated time remaining, when the engine can compute one.
    pub eta: Option<Duration>,
    pub finished: bool,
    /// Connected (live) peer count.
    pub peers: usize,
    pub files: Vec<DetailFile>,
    /// Connected peers, sorted by address.
    pub peer_rows: Vec<PeerRow>,
    /// Tracker announce URLs, sorted.
    pub trackers: Vec<String>,
    /// Attached web seeds, in the order they were attached.
    pub web_seeds: Vec<WebSeedRow>,
    /// Per-piece have flags, when available.
    pub pieces: Option<Vec<bool>>,
}

impl DetailSnapshot {
    /// Share ratio `uploaded / downloaded` as a fraction (may exceed 1.0).
    pub fn ratio(&self) -> f64 {
        if self.progress_bytes == 0 {
            0.0
        } else {
            self.uploaded_bytes as f64 / self.progress_bytes as f64
        }
    }
}
