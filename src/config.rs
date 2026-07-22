//! User configuration loading and persistence.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};

/// Application name used to resolve per-OS config directories.
const APP_NAME: &str = "kist";

/// User-tunable settings, persisted as TOML.
///
/// `#[serde(default)]` lets a partial config file work: any missing field
/// falls back to its [`Default`] value instead of failing to parse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where downloaded torrents are written by default.
    pub download_directory: PathBuf,
    /// Inclusive port range for incoming peer connections, as `[start, end]`.
    pub listen_ports: [u16; 2],
    /// Whether to enable the DHT (needed for magnet links).
    pub enable_dht: bool,
    /// Whether to persist the torrent list across restarts.
    pub enable_session_persistence: bool,
    /// UI refresh interval in milliseconds.
    pub refresh_interval_ms: u64,
    /// Global download speed cap as a human size (e.g. `"2M"`); absent means
    /// unlimited.
    pub download_limit: Option<String>,
    /// Global upload speed cap as a human size (e.g. `"512K"`); absent means
    /// unlimited.
    pub upload_limit: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            download_directory: default_download_dir(),
            listen_ports: [6881, 6889],
            enable_dht: true,
            enable_session_persistence: true,
            refresh_interval_ms: 250,
            download_limit: None,
            upload_limit: None,
        }
    }
}

impl Config {
    /// The UI refresh interval clamped to a sane minimum, as a [`Duration`].
    pub fn refresh_interval(&self) -> Duration {
        Duration::from_millis(self.refresh_interval_ms.max(50))
    }

    /// Build the librqbit listen port range as a half-open `start..end`,
    /// normalized so `end >= start` and the upper bound is inclusive-friendly.
    pub fn listen_port_range(&self) -> std::ops::Range<u16> {
        let start = self.listen_ports[0];
        let end = self.listen_ports[1].max(start);
        start..end.saturating_add(1)
    }

    /// Configured global download cap in bytes per second, if any. An
    /// unparseable value is treated as unlimited.
    pub fn download_limit_bps(&self) -> Option<u32> {
        self.download_limit
            .as_deref()
            .and_then(crate::format::parse_rate)
    }

    /// Configured global upload cap in bytes per second, if any. An unparseable
    /// value is treated as unlimited.
    pub fn upload_limit_bps(&self) -> Option<u32> {
        self.upload_limit
            .as_deref()
            .and_then(crate::format::parse_rate)
    }

    /// Apply command-line overrides on top of the loaded config.
    pub fn apply_overrides(&mut self, cli: &Cli) {
        if let Some(dir) = &cli.download_dir {
            self.download_directory = dir.clone();
        }
    }
}

/// Command-line interface parsed by clap.
#[derive(Debug, Parser)]
#[command(version, about = "A simple terminal torrent client.")]
pub struct Cli {
    /// Torrent to add on startup (magnet link, .torrent path, or URL).
    pub torrent: Option<String>,

    /// Override the download directory for this run.
    #[arg(long, value_name = "DIR")]
    pub download_dir: Option<PathBuf>,

    /// Path to an alternate config file.
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
}

impl Cli {
    /// Resolve the config file path: an explicit `--config`, else the default.
    pub fn config_path(&self) -> Result<PathBuf> {
        match &self.config {
            Some(path) => Ok(path.clone()),
            None => default_config_path(),
        }
    }
}

fn default_download_dir() -> PathBuf {
    UserDirs::new()
        .and_then(|d| d.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Resolve the default config file path under the OS config directory.
pub fn default_config_path() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().join("config.toml"))
}

/// The kist-owned folder for session persistence (under the OS data directory).
pub fn persistence_directory() -> Result<PathBuf> {
    Ok(project_dirs()?.data_dir().join("session"))
}

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", APP_NAME).context("could not determine OS directories")
}

/// Load config from `path`, never aborting on a missing or invalid file.
///
/// - Missing file: returns defaults and writes a default `config.toml` for the
///   user to edit.
/// - Invalid file: warns and returns defaults.
pub fn load_or_init(path: &Path) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(contents) => match toml::from_str::<Config>(&contents) {
            Ok(config) => Ok(config),
            Err(e) => {
                warn_fallback(path, &format!("failed to parse config: {e}"));
                Ok(Config::default())
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let config = Config::default();
            if let Err(e) = write_config(path, &config) {
                warn_fallback(path, &format!("could not write default config: {e}"));
            }
            Ok(config)
        }
        Err(e) => {
            warn_fallback(path, &format!("could not read config: {e}"));
            Ok(Config::default())
        }
    }
}

/// Serialize `config` as pretty TOML to `path`, creating parent dirs as needed.
fn write_config(path: &Path, config: &Config) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let serialized = toml::to_string_pretty(config).context("failed to serialize config")?;
    std::fs::write(path, serialized)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn warn_fallback(path: &Path, reason: &str) {
    eprintln!("warning: {reason} (at {}); using defaults", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A unique temp file path for a test (caller cleans it up).
    fn unique_tmp_path() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut path = std::env::temp_dir();
        path.push(format!("kist-config-test-{}-{n}.toml", std::process::id()));
        path
    }

    #[test]
    fn missing_file_creates_default_and_uses_defaults() {
        let path = unique_tmp_path();
        let _ = std::fs::remove_file(&path);

        let config = load_or_init(&path).expect("missing file should not error");
        assert_eq!(
            config.refresh_interval_ms,
            Config::default().refresh_interval_ms
        );
        assert!(path.exists(), "default config file should be written");

        // The written file must round-trip back to the same values.
        let reloaded = load_or_init(&path).expect("reload should parse written file");
        assert_eq!(reloaded.listen_ports, config.listen_ports);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_file_falls_back_gracefully() {
        let path = unique_tmp_path();
        std::fs::write(&path, "this is := not valid toml !!! [\n").unwrap();

        let config = load_or_init(&path).expect("invalid file should not abort");
        assert_eq!(config.enable_dht, Config::default().enable_dht);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn partial_file_fills_missing_fields_with_defaults() {
        let path = unique_tmp_path();
        std::fs::write(&path, "enable_dht = false\n").unwrap();

        let config = load_or_init(&path).expect("partial file should parse");
        assert!(!config.enable_dht, "explicit value must be honored");
        assert_eq!(
            config.refresh_interval_ms,
            Config::default().refresh_interval_ms
        );

        let _ = std::fs::remove_file(&path);
    }
}
