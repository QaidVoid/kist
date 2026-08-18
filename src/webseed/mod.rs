//! Web seed (BEP 19) support.
//!
//! librqbit has no notion of an HTTP source, and its only entry point for
//! torrent data is the BitTorrent peer protocol. So a web seed is presented to
//! the session as an ordinary peer: [`bridge`] dials the session's own listen
//! port over loopback, advertises the pieces the HTTP source can serve, and
//! answers piece requests with data fetched by [`fetch`] using ranged GETs.
//!
//! Nothing here verifies piece hashes. Fetched bytes are handed to the session
//! as normal peer blocks so its existing verification applies, which means a
//! web seed serving bad data is treated exactly like a lying peer.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU16, AtomicU64, Ordering};

use crate::model::WebSeedState;

pub mod bridge;
pub mod fetch;
pub mod mapping;
pub mod state;

#[cfg(test)]
mod e2e_tests;

/// Live state of one web seed, shared between its bridge task and the UI.
///
/// The engine reads this synchronously while building detail snapshots, so it
/// is plain atomics rather than a channel.
#[derive(Debug, Default)]
pub struct SeedStatus {
    state: AtomicU8,
    served_bytes: AtomicU64,
    /// Loopback port the bridge connected from, so the peer list can label it.
    /// Zero while not connected.
    local_port: AtomicU16,
    /// Why the seed failed, once it has.
    error: Mutex<Option<String>>,
}

impl SeedStatus {
    /// Current state of the seed.
    pub fn state(&self) -> WebSeedState {
        match self.state.load(Ordering::Relaxed) {
            1 => WebSeedState::Connecting,
            2 => WebSeedState::Active,
            3 => WebSeedState::BackingOff,
            4 => WebSeedState::Failed,
            _ => WebSeedState::Idle,
        }
    }

    /// Total payload bytes this seed has served to the session.
    pub fn served_bytes(&self) -> u64 {
        self.served_bytes.load(Ordering::Relaxed)
    }

    /// Loopback port the bridge peer is connected from, if it is connected.
    pub fn local_port(&self) -> Option<u16> {
        match self.local_port.load(Ordering::Relaxed) {
            0 => None,
            port => Some(port),
        }
    }

    pub(crate) fn set_state(&self, state: WebSeedState) {
        let value = match state {
            WebSeedState::Idle => 0,
            WebSeedState::Connecting => 1,
            WebSeedState::Active => 2,
            WebSeedState::BackingOff => 3,
            WebSeedState::Failed => 4,
        };
        self.state.store(value, Ordering::Relaxed);
    }

    pub(crate) fn add_served(&self, bytes: u64) {
        self.served_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(crate) fn set_local_port(&self, port: u16) {
        self.local_port.store(port, Ordering::Relaxed);
    }

    /// The most recent problem, whether the seed recovered from it or not.
    pub fn error(&self) -> Option<String> {
        self.error.lock().ok()?.clone()
    }

    /// Record a problem the seed may still recover from, such as being unable
    /// to reach the session while a torrent is starting up.
    pub(crate) fn note_error(&self, reason: String) {
        self.set_error(Some(reason));
    }

    /// Mark the seed permanently failed with a reason.
    pub(crate) fn fail(&self, reason: String) {
        self.set_error(Some(reason));
        self.set_state(WebSeedState::Failed);
    }

    /// Forget the last recorded problem.
    pub(crate) fn clear_error(&self) {
        self.set_error(None);
    }

    /// Park the seed because its torrent does not need it.
    ///
    /// This clears the last error: losing the connection is how a torrent that
    /// paused or finished looks from the bridge, and reporting that as a
    /// problem on an otherwise healthy seed is just noise.
    pub(crate) fn park(&self) {
        self.set_error(None);
        self.set_local_port(0);
        self.set_state(WebSeedState::Idle);
    }

    fn set_error(&self, reason: Option<String>) {
        if let Ok(mut error) = self.error.lock() {
            *error = reason;
        }
    }
}

/// Whether `url` is usable as a web seed.
///
/// BEP 19 only defines HTTP and FTP sources; kist supports the HTTP family
/// only, so anything else is rejected before a bridge is ever started.
pub fn validate_url(url: &str) -> Result<(), &'static str> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "not a valid URL")?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        _ => Err("only http and https URLs are supported"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_urls() {
        assert!(validate_url("http://example.com/files/").is_ok());
        assert!(validate_url("https://example.com/a.bin").is_ok());
    }

    #[test]
    fn rejects_other_schemes_and_garbage() {
        assert!(validate_url("magnet:?xt=urn:btih:abc").is_err());
        assert!(validate_url("ftp://example.com/a.bin").is_err());
        assert!(validate_url("/tmp/a.bin").is_err());
        assert!(validate_url("").is_err());
    }

    #[test]
    fn status_round_trips_states() {
        let status = SeedStatus::default();
        assert_eq!(status.state(), WebSeedState::Idle);
        for state in [
            WebSeedState::Connecting,
            WebSeedState::Active,
            WebSeedState::BackingOff,
            WebSeedState::Failed,
            WebSeedState::Idle,
        ] {
            status.set_state(state);
            assert_eq!(status.state(), state);
        }
        status.add_served(10);
        status.add_served(5);
        assert_eq!(status.served_bytes(), 15);
        assert_eq!(status.local_port(), None);
        status.set_local_port(1234);
        assert_eq!(status.local_port(), Some(1234));
    }
}
