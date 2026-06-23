#![allow(dead_code)] // consumed by engine/main in later task groups

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
    /// UI refresh interval in milliseconds.
    pub refresh_interval_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            download_directory: default_download_dir(),
            listen_ports: [6881, 6889],
            enable_dht: true,
            refresh_interval_ms: 250,
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
    let dirs =
        ProjectDirs::from("", "", APP_NAME).context("could not determine OS config directory")?;
    Ok(dirs.config_dir().join("config.toml"))
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
