//! Ranged HTTP fetching for one web seed.
//!
//! The session requests 16 KiB blocks, which would be a pathological number of
//! HTTP requests. Reads are therefore served out of aligned windows: a block
//! triggers a ranged GET for the whole window containing it, and the window is
//! cached so neighbouring blocks are answered from memory.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use reqwest::StatusCode;
use reqwest::header::{CONTENT_RANGE, RANGE};
use tokio::sync::Mutex;

use crate::model::WebSeedState;
use crate::webseed::SeedStatus;
use crate::webseed::mapping::FileMap;

/// Smallest window fetched in one request; larger piece lengths win.
const MIN_WINDOW: u64 = 1024 * 1024;

/// How long a single ranged request may take before it counts as transient
/// failure.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Transient failures tolerated for one read before the seed is declared dead.
const MAX_ATTEMPTS: u32 = 5;

/// First backoff delay; doubles per attempt.
const BACKOFF_BASE: Duration = Duration::from_millis(500);

/// Longest backoff delay between retries.
const BACKOFF_MAX: Duration = Duration::from_secs(16);

/// Why a fetch failed.
#[derive(Debug)]
pub enum FetchError {
    /// Worth retrying: timeouts, connection errors, `5xx`, short bodies.
    Transient(String),
    /// Not worth retrying: the URL is wrong, or the server does not do ranges.
    Hard(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Transient(m) | FetchError::Hard(m) => f.write_str(m),
        }
    }
}

/// A window of a file held in memory.
struct CachedWindow {
    file: usize,
    start: u64,
    data: Bytes,
}

/// Identifies a window as `(file index, offset in file)`.
type WindowKey = (usize, u64);

/// Fetches torrent byte ranges from one web seed over HTTP.
pub struct Fetcher {
    client: reqwest::Client,
    map: FileMap,
    status: Arc<SeedStatus>,
    window: u64,
    capacity: usize,
    cache: Mutex<VecDeque<CachedWindow>>,
    /// One gate per window being fetched, so concurrent workers that need the
    /// same window wait for one request instead of each issuing their own.
    inflight: Mutex<HashMap<WindowKey, Arc<Mutex<()>>>>,
}

impl Fetcher {
    /// Create a fetcher for one seed. `piece_length` sets the window floor and
    /// `capacity` caps how many windows are cached, bounding memory at
    /// `capacity * window`.
    pub fn new(map: FileMap, status: Arc<SeedStatus>, piece_length: u32, capacity: usize) -> Self {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            client,
            map,
            status,
            window: MIN_WINDOW.max(u64::from(piece_length)),
            capacity: capacity.max(1),
            cache: Mutex::new(VecDeque::new()),
            inflight: Mutex::new(HashMap::new()),
        }
    }

    /// Read `len` bytes at torrent offset `offset`, retrying transient failures
    /// with exponential backoff.
    ///
    /// The seed's state follows the outcome: backing off while retrying, active
    /// once a read succeeds.
    pub async fn read(&self, offset: u64, len: u64) -> Result<Vec<u8>, FetchError> {
        let mut backoff = BACKOFF_BASE;
        let mut last = String::new();
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                self.status.set_state(WebSeedState::BackingOff);
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
            match self.read_once(offset, len).await {
                Ok(data) => {
                    self.status.set_state(WebSeedState::Active);
                    return Ok(data);
                }
                Err(FetchError::Hard(m)) => return Err(FetchError::Hard(m)),
                Err(FetchError::Transient(m)) => last = m,
            }
        }
        Err(FetchError::Hard(format!(
            "gave up after {MAX_ATTEMPTS} attempts: {last}"
        )))
    }

    /// One attempt at reading a torrent byte range, assembling it from windows.
    async fn read_once(&self, offset: u64, len: u64) -> Result<Vec<u8>, FetchError> {
        let ranges = self.map.ranges(offset, len);
        let covered: u64 = ranges.iter().map(|r| r.len).sum();
        if covered != len {
            return Err(FetchError::Hard(format!(
                "requested {len} bytes at {offset}, but the torrent only covers {covered}"
            )));
        }

        let mut out = Vec::with_capacity(len as usize);
        for range in ranges {
            let end = range.offset + range.len;
            let mut pos = range.offset;
            while pos < end {
                let start = pos - pos % self.window;
                let data = self.window(range.file, range.url, start).await?;
                let inner = (pos - start) as usize;
                let available = data.len().saturating_sub(inner);
                if available == 0 {
                    return Err(FetchError::Hard(format!(
                        "{} is shorter than the torrent says",
                        range.url
                    )));
                }
                let take = available.min((end - pos) as usize);
                out.extend_from_slice(&data[inner..inner + take]);
                pos += take as u64;
            }
        }
        Ok(out)
    }

    /// The window of `file` starting at `start`, from cache or over HTTP.
    ///
    /// Concurrent callers for the same window are collapsed into one request.
    /// Without that, every worker misses the cache at the same moment and
    /// re-downloads the same bytes, multiplying traffic by the concurrency.
    async fn window(&self, file: usize, url: &str, start: u64) -> Result<Bytes, FetchError> {
        if let Some(hit) = self.cached(file, start).await {
            return Ok(hit);
        }
        let key = (file, start);
        let gate = self.gate(key).await;
        let result = {
            let _guard = gate.lock().await;
            // Whoever held the gate before us may have filled the cache.
            match self.cached(file, start).await {
                Some(hit) => Ok(hit),
                None => {
                    let fetched = self.fetch_window(file, url, start).await;
                    if let Ok(data) = &fetched {
                        self.store(file, start, data.clone()).await;
                    }
                    fetched
                }
            }
        };
        self.release(key, &gate).await;
        result
    }

    /// Fetch one window over HTTP, clamped to the end of the file.
    async fn fetch_window(&self, file: usize, url: &str, start: u64) -> Result<Bytes, FetchError> {
        let file_len = self
            .map
            .file(file)
            .map(|f| f.len)
            .ok_or_else(|| FetchError::Hard(format!("no file at index {file}")))?;
        let len = self.window.min(file_len.saturating_sub(start));
        if len == 0 {
            return Err(FetchError::Hard(format!(
                "window at {start} is past the end of {url}"
            )));
        }
        self.fetch_range(url, start, len).await
    }

    /// The gate for one window, creating it if this is the first request.
    async fn gate(&self, key: WindowKey) -> Arc<Mutex<()>> {
        let mut inflight = self.inflight.lock().await;
        inflight.entry(key).or_default().clone()
    }

    /// Drop a window's gate once nobody else is waiting on it.
    async fn release(&self, key: WindowKey, gate: &Arc<Mutex<()>>) {
        let mut inflight = self.inflight.lock().await;
        // Two references means the map's and ours, so no one else is waiting.
        // Insertion also takes this lock, so a new waiter cannot slip in here.
        if Arc::strong_count(gate) == 2 {
            inflight.remove(&key);
        }
    }

    /// Look a window up in the cache, promoting it to most-recently-used.
    async fn cached(&self, file: usize, start: u64) -> Option<Bytes> {
        let mut cache = self.cache.lock().await;
        let index = cache
            .iter()
            .position(|w| w.file == file && w.start == start)?;
        let window = cache.remove(index)?;
        let data = window.data.clone();
        cache.push_front(window);
        Some(data)
    }

    /// Insert a window, evicting the least-recently-used one when full.
    async fn store(&self, file: usize, start: u64, data: Bytes) {
        let mut cache = self.cache.lock().await;
        cache.push_front(CachedWindow { file, start, data });
        while cache.len() > self.capacity {
            cache.pop_back();
        }
    }

    /// Issue one ranged GET and validate that the server actually honored it.
    async fn fetch_range(&self, url: &str, start: u64, len: u64) -> Result<Bytes, FetchError> {
        let end = start + len - 1;
        let response = self
            .client
            .get(url)
            .header(RANGE, format!("bytes={start}-{end}"))
            .send()
            .await
            .map_err(|e| FetchError::Transient(format!("{url}: {e}")))?;

        let status = response.status();
        match status {
            StatusCode::PARTIAL_CONTENT => {}
            // A 200 means the server ignored the Range header and is sending
            // the whole entity. Reading it as if it were the requested range
            // would silently serve wrong bytes at every offset, so refuse.
            StatusCode::OK => {
                return Err(FetchError::Hard(format!(
                    "{url}: server ignored the Range header"
                )));
            }
            s if s.is_server_error() => {
                return Err(FetchError::Transient(format!("{url}: {s}")));
            }
            s => return Err(FetchError::Hard(format!("{url}: {s}"))),
        }

        // A 206 for a different range than we asked for is just as wrong as a
        // 200, and cheaper to catch here than via a hash failure per piece.
        if let Some(range) = response.headers().get(CONTENT_RANGE)
            && let Some(got) = range.to_str().ok().and_then(parse_content_range_start)
            && got != start
        {
            return Err(FetchError::Hard(format!(
                "{url}: asked for byte {start} but got byte {got}"
            )));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| FetchError::Transient(format!("{url}: {e}")))?;
        if body.len() as u64 != len {
            return Err(FetchError::Transient(format!(
                "{url}: asked for {len} bytes, got {}",
                body.len()
            )));
        }
        Ok(body)
    }
}

/// Extract the first byte position from a `Content-Range: bytes 0-99/200`
/// header, ignoring anything that does not parse.
fn parse_content_range_start(value: &str) -> Option<u64> {
    let spec = value.trim().strip_prefix("bytes ")?;
    let (start, _) = spec.split_once('-')?;
    start.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_range_start() {
        assert_eq!(parse_content_range_start("bytes 0-99/200"), Some(0));
        assert_eq!(
            parse_content_range_start("bytes 1024-2047/4096"),
            Some(1024)
        );
        assert_eq!(parse_content_range_start("bytes */200"), None);
        assert_eq!(parse_content_range_start("items 0-99/200"), None);
        assert_eq!(parse_content_range_start(""), None);
    }
}
