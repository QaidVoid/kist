#![allow(dead_code)] // consumed by engine/UI in later task groups

//! Plain, framework-agnostic view models shared between engine and UI.
//!
//! Nothing in this module depends on librqbit. The engine is responsible for
//! translating librqbit types into the values defined here, so the UI stays
//! free of engine concerns and easy to reason about.

/// Coarse torrent state mirroring librqbit's `TorrentStatsState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub finished: bool,
    pub down_speed: u64,
    pub up_speed: u64,
    pub peers: usize,
    pub state: RowState,
    pub error: Option<String>,
}

impl TorrentRow {
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
