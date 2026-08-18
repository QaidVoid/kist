//! Persistence for attached web seeds.
//!
//! Web seeds are runtime state rather than user configuration, so they live in
//! a JSON file in the data directory beside the session store instead of in
//! `config.toml`, which would otherwise be rewritten on every attach.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Attached web seed URLs, keyed by torrent infohash.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SeedStore {
    seeds: BTreeMap<String, Vec<String>>,
}

impl SeedStore {
    /// URLs attached to `infohash`.
    pub fn urls(&self, infohash: &str) -> &[String] {
        self.seeds.get(infohash).map_or(&[], Vec::as_slice)
    }

    /// Every infohash with at least one attached seed.
    pub fn infohashes(&self) -> impl Iterator<Item = &String> {
        self.seeds.keys()
    }

    /// Attach `url` to `infohash`, reporting whether it was new.
    pub fn insert(&mut self, infohash: &str, url: String) -> bool {
        let urls = self.seeds.entry(infohash.to_string()).or_default();
        if urls.iter().any(|u| u == &url) {
            return false;
        }
        urls.push(url);
        true
    }

    /// Drop entries for torrents that are no longer in the session.
    pub fn retain_known(&mut self, known: &dyn Fn(&str) -> bool) {
        self.seeds.retain(|infohash, _| known(infohash));
    }
}

/// Load the store from `path`.
///
/// A missing file is an empty store. A file that cannot be read or parsed is
/// also treated as empty, with the reason returned so the caller can surface it
/// rather than failing startup over recoverable state.
pub fn load(path: &Path) -> (SeedStore, Option<String>) {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (SeedStore::default(), None),
        Err(e) => {
            return (
                SeedStore::default(),
                Some(format!("could not read web seeds: {e}")),
            );
        }
    };
    match serde_json::from_str(&contents) {
        Ok(store) => (store, None),
        Err(e) => (
            SeedStore::default(),
            Some(format!("could not parse web seeds: {e}")),
        ),
    }
}

/// Write the store to `path`, creating parent directories as needed.
pub fn save(path: &Path, store: &SeedStore) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(store)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_path() -> std::path::PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!("kist-seeds-test-{}-{n}.json", std::process::id()));
        path
    }

    #[test]
    fn insert_is_idempotent() {
        let mut store = SeedStore::default();
        assert!(store.insert("abc", "https://e.com/a".into()));
        assert!(!store.insert("abc", "https://e.com/a".into()));
        assert!(store.insert("abc", "https://e.com/b".into()));
        assert_eq!(store.urls("abc").len(), 2);
        assert!(store.urls("other").is_empty());
    }

    #[test]
    fn retain_known_drops_stale_torrents() {
        let mut store = SeedStore::default();
        store.insert("keep", "https://e.com/a".into());
        store.insert("drop", "https://e.com/b".into());
        store.retain_known(&|hash| hash == "keep");
        assert_eq!(store.infohashes().count(), 1);
        assert_eq!(store.urls("keep").len(), 1);
    }

    #[test]
    fn round_trips_through_a_file() {
        let path = tmp_path();
        let mut store = SeedStore::default();
        store.insert("abc", "https://e.com/a".into());
        save(&path, &store).unwrap();

        let (loaded, error) = load(&path);
        assert!(error.is_none());
        assert_eq!(loaded.urls("abc"), ["https://e.com/a"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_empty_without_an_error() {
        let (store, error) = load(&tmp_path());
        assert!(error.is_none());
        assert_eq!(store.infohashes().count(), 0);
    }

    #[test]
    fn corrupt_file_is_empty_with_an_error() {
        let path = tmp_path();
        std::fs::write(&path, "{not json").unwrap();
        let (store, error) = load(&path);
        assert!(error.is_some(), "a corrupt file must be reported");
        assert_eq!(store.infohashes().count(), 0);
        let _ = std::fs::remove_file(&path);
    }
}
