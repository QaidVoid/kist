//! librqbit session wrapper and command handling.
//!
//! The [`Engine`] owns the librqbit [`Session`] and exposes thin async methods
//! plus a synchronous [`Engine::snapshot`] for the UI. [`spawn`] runs the engine
//! on a background task, taking commands on a channel and publishing snapshots
//! (and status messages) back to the UI.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, SessionPersistenceConfig, TorrentStats,
    TorrentStatsState,
};
use tokio::sync::{mpsc, watch};
use tokio::task::AbortHandle;

use crate::config::Config;
use crate::error;
use crate::model::{
    DetailFile, DetailSnapshot, PeerRow, PreviewFile, RowState, Snapshot, TorrentRow, WebSeedRow,
    WebSeedState,
};
use crate::search::{self, SearchOutcome};
use crate::webseed::bridge::BridgeParams;
use crate::webseed::fetch::Fetcher;
use crate::webseed::mapping::{FileMap, TorrentFile};
use crate::webseed::state::SeedStore;
use crate::webseed::{self, SeedStatus};

/// Owns the librqbit session and translates it into plain view models.
pub struct Engine {
    session: Arc<Session>,
    /// librqbit API wrapper; the only public route to some data (piece haves).
    api: Api,
    /// Web seed settings resolved from the config.
    web_seeds: WebSeedSettings,
    /// Attached web seeds and their bridge tasks, keyed by infohash. Infohash
    /// rather than torrent id, because ids are not stable across restarts.
    seeds: Mutex<HashMap<String, Vec<SeedEntry>>>,
    /// Non-fatal problems found while starting up, published once the UI is up.
    startup_warnings: Vec<String>,
}

/// Web seed settings resolved from the config once at startup.
struct WebSeedSettings {
    enabled: bool,
    concurrency: usize,
    state_path: Option<PathBuf>,
}

/// One web seed attached to a torrent, with its bridge task if it is running.
struct SeedEntry {
    url: String,
    status: Arc<SeedStatus>,
    task: Option<AbortHandle>,
}

impl SeedEntry {
    fn new(url: String) -> Self {
        Self {
            url,
            status: Arc::new(SeedStatus::default()),
            task: None,
        }
    }
}

impl Engine {
    /// Create and start a librqbit session from the given [`Config`].
    ///
    /// Returns an error if the session cannot initialize (for example an
    /// invalid or unwritable download directory).
    pub async fn new(config: &Config) -> Result<Self> {
        let (listen_addr, listen_warning) = resolve_listen_addr(config.listen_port_range());
        let opts = SessionOptions {
            dht: config.enable_dht.then(DhtSessionConfig::default),
            listen: Some(ListenerOptions {
                listen_addr,
                ..Default::default()
            }),
            // Persist the torrent list so it survives restarts, in a kist-owned
            // folder (falling back to librqbit's default if the dir is unknown).
            persistence: if config.enable_session_persistence {
                Some(SessionPersistenceConfig::Json {
                    folder: crate::config::persistence_directory().ok(),
                })
            } else {
                None
            },
            ratelimits: LimitsConfig {
                download_bps: config.download_limit_bps().and_then(NonZeroU32::new),
                upload_bps: config.upload_limit_bps().and_then(NonZeroU32::new),
            },
            ..Default::default()
        };
        let session = Session::new_with_opts(config.download_directory.clone(), opts)
            .await
            .context("failed to initialize torrent session")?;
        let api = Api::new(session.clone(), None);

        let web_seeds = WebSeedSettings {
            enabled: config.enable_web_seeds,
            concurrency: config.web_seed_concurrency.clamp(1, 16),
            state_path: crate::config::web_seed_state_file().ok(),
        };
        let (seeds, mut startup_warnings) = restore_web_seeds(&session, &web_seeds);
        startup_warnings.extend(listen_warning);

        Ok(Self {
            session,
            api,
            web_seeds,
            seeds: Mutex::new(seeds),
            startup_warnings,
        })
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

    /// Add a torrent with explicit options: start paused, an alternate output
    /// folder, and an explicit set of files to download.
    pub async fn add_with_options(
        &self,
        source: String,
        paused: bool,
        output_folder: Option<String>,
        only_files: Option<Vec<usize>>,
    ) -> Result<()> {
        let add = AddTorrent::from_cli_argument(&source)
            .with_context(|| format!("invalid torrent source: {source:?}"))?;
        let opts = AddTorrentOptions {
            paused,
            output_folder,
            only_files,
            ..Default::default()
        };
        self.session
            .add_torrent(add, Some(opts))
            .await
            .with_context(|| format!("failed to add torrent: {source}"))?;
        Ok(())
    }

    /// List the files of a torrent source without adding or downloading it.
    pub async fn preview(&self, source: &str) -> Result<Vec<PreviewFile>> {
        let add = AddTorrent::from_cli_argument(source)
            .with_context(|| format!("invalid torrent source: {source:?}"))?;
        let opts = AddTorrentOptions {
            list_only: true,
            ..Default::default()
        };
        let response = self
            .session
            .add_torrent(add, Some(opts))
            .await
            .with_context(|| format!("failed to read torrent: {source}"))?;
        let AddTorrentResponse::ListOnly(list) = response else {
            return Ok(Vec::new());
        };
        let mut files = Vec::new();
        for details in list.info.iter_file_details() {
            files.push(PreviewFile {
                name: details.filename.to_string(),
                size: details.len,
            });
        }
        Ok(files)
    }

    /// Set the global download/upload rate limits live (`None` = unlimited).
    pub fn set_limits(&self, down: Option<u32>, up: Option<u32>) {
        self.session
            .ratelimits
            .set_download_bps(down.and_then(NonZeroU32::new));
        self.session
            .ratelimits
            .set_upload_bps(up.and_then(NonZeroU32::new));
    }

    /// Update which files of a torrent are downloaded.
    pub async fn set_files(&self, id: usize, included: &HashSet<usize>) -> Result<()> {
        self.api
            .api_torrent_action_update_only_files(TorrentIdOrHash::Id(id), included)
            .await
            .map(|_| ())
            .with_context(|| format!("failed to update files for torrent {id}"))
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

    /// Forget the torrent with the given id and delete its downloaded files.
    pub async fn remove_with_data(&self, id: usize) -> Result<()> {
        self.session
            .delete(TorrentIdOrHash::Id(id), true)
            .await
            .with_context(|| format!("failed to delete torrent {id}"))
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
        let (down_speed, up_speed, peers) = live_speeds(&stats);
        let file_progress = stats.file_progress.clone();

        let live = handle.live();
        let eta = live
            .as_ref()
            .and_then(|l| l.down_speed_estimator().time_remaining());
        // Bridge peers are kist's own web seeds, not swarm members, so the peer
        // list labels them instead of passing them off as real peers.
        let bridge_ports = self.web_seed_ports(&infohash);
        let web_seeds = self.web_seed_rows(&infohash);
        let peer_rows = live
            .map(|l| {
                let snapshot = l.per_peer_stats_snapshot(Default::default());
                let mut rows: Vec<PeerRow> = snapshot
                    .peers
                    .into_iter()
                    .map(|(addr, p)| PeerRow {
                        web_seed: is_bridge_addr(&addr, &bridge_ports),
                        addr,
                        state: p.state.to_string(),
                        fetched_bytes: p.counters.fetched_bytes,
                    })
                    .collect();
                rows.sort_by(|a, b| a.addr.cmp(&b.addr));
                rows
            })
            .unwrap_or_default();

        let mut trackers: Vec<String> = handle
            .shared()
            .trackers
            .iter()
            .map(|u| u.to_string())
            .collect();
        trackers.sort();

        // The bitfield is byte-aligned, so it carries spare bits past the last
        // piece that the piece map must not show.
        let pieces = self
            .api
            .api_dump_haves(TorrentIdOrHash::Id(id))
            .ok()
            .map(|(have, total)| have.iter().map(|bit| *bit).take(total as usize).collect());

        // `only_files == None` means every file is included.
        let only_files = handle.only_files();
        let files = handle
            .with_metadata(|m| {
                m.file_infos
                    .iter()
                    .enumerate()
                    .map(|(i, fi)| DetailFile {
                        name: fi.relative_filename.to_string_lossy().to_string(),
                        size: fi.len,
                        have: file_progress.get(i).copied().unwrap_or(0).min(fi.len),
                        included: only_files.as_ref().is_none_or(|sel| sel.contains(&i)),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Some(DetailSnapshot {
            infohash,
            state: to_row_state(stats.state),
            total_bytes: stats.total_bytes,
            progress_bytes: stats.progress_bytes,
            uploaded_bytes: stats.uploaded_bytes,
            down_speed,
            up_speed,
            eta,
            finished: stats.finished,
            peers,
            files,
            peer_rows,
            trackers,
            web_seeds,
            pieces,
        })
    }

    fn find_handle(&self, id: usize) -> Result<Arc<ManagedTorrent>> {
        self.session
            .get(TorrentIdOrHash::Id(id))
            .with_context(|| format!("torrent {id} not found"))
    }

    /// Non-fatal startup problems, for the UI to show once it is running.
    pub fn startup_warnings(&self) -> &[String] {
        &self.startup_warnings
    }

    /// Attach an HTTP source to a torrent as a web seed.
    ///
    /// The bridge starts on the next reconcile, so a torrent whose metadata has
    /// not resolved yet simply picks the seed up once it does.
    pub fn attach_web_seed(&self, id: usize, url: String) -> Result<String> {
        if !self.web_seeds.enabled {
            bail!("web seeds are disabled");
        }
        webseed::validate_url(&url).map_err(|reason| anyhow!("{reason}"))?;
        let infohash = self.find_handle(id)?.shared().info_hash.as_string();

        let mut seeds = self.seeds.lock().unwrap();
        let entries = seeds.entry(infohash).or_default();
        if entries.iter().any(|entry| entry.url == url) {
            return Ok("web seed already attached".to_string());
        }
        entries.push(SeedEntry::new(url));
        let saved = self.persist(&seeds);
        drop(seeds);

        self.reconcile_web_seeds();
        Ok(with_save_result("attached web seed", saved))
    }

    /// Detach a web seed, stopping its bridge.
    pub fn detach_web_seed(&self, id: usize, url: &str) -> Result<String> {
        let infohash = self.find_handle(id)?.shared().info_hash.as_string();

        let mut seeds = self.seeds.lock().unwrap();
        let entries = seeds
            .get_mut(&infohash)
            .with_context(|| format!("torrent {id} has no web seeds"))?;
        let index = entries
            .iter()
            .position(|entry| entry.url == url)
            .context("web seed not attached")?;
        if let Some(task) = entries.remove(index).task {
            task.abort();
        }
        if entries.is_empty() {
            seeds.remove(&infohash);
        }
        let saved = self.persist(&seeds);
        Ok(with_save_result("detached web seed", saved))
    }

    /// Start and stop bridges so they match the attached seeds and the state of
    /// their torrents.
    ///
    /// Run on the refresh tick rather than driven by events, because librqbit
    /// does not notify on pause, resume, or metadata resolution.
    pub fn reconcile_web_seeds(&self) {
        if !self.web_seeds.enabled {
            return;
        }
        let Some(listen) = self.session.listen_addr() else {
            return;
        };
        let torrents: HashMap<String, Arc<ManagedTorrent>> = self.session.with_torrents(|list| {
            list.map(|(_, handle)| (handle.shared().info_hash.as_string(), handle.clone()))
                .collect()
        });

        let mut seeds = self.seeds.lock().unwrap();
        for (infohash, entries) in seeds.iter_mut() {
            // A bridge is only useful while the torrent is live and still
            // wants data: the session refuses connections for anything that is
            // not live, and a completed torrent has nothing left to request.
            let handle = torrents
                .get(infohash)
                .filter(|handle| handle.live().is_some() && !handle.stats().finished);
            for entry in entries.iter_mut() {
                // A finished task means the bridge gave up on the seed.
                if entry.task.as_ref().is_some_and(AbortHandle::is_finished) {
                    entry.task = None;
                }
                let failed = entry.status.state() == WebSeedState::Failed;
                match (&entry.task, handle) {
                    (None, Some(handle)) if !failed => {
                        entry.task = self.start_bridge(handle, loopback_target(listen), entry);
                    }
                    (Some(task), None) => {
                        task.abort();
                        entry.task = None;
                    }
                    _ => {}
                }
                // A seed with no reason to run reads as idle rather than
                // forever "connecting".
                if handle.is_none() && !failed {
                    entry.status.park();
                }
            }
        }
    }

    /// Spawn a bridge for one seed, or `None` if the torrent's metadata has not
    /// resolved yet.
    fn start_bridge(
        &self,
        handle: &Arc<ManagedTorrent>,
        listen_addr: SocketAddr,
        entry: &SeedEntry,
    ) -> Option<AbortHandle> {
        let concurrency = self.web_seeds.concurrency;
        let (map, params) = bridge_setup(handle, listen_addr, &entry.url, concurrency)?;
        let status = entry.status.clone();
        let fetcher = Arc::new(Fetcher::new(
            map,
            status.clone(),
            params.piece_length,
            concurrency,
        ));
        Some(tokio::spawn(webseed::bridge::run(params, fetcher, status)).abort_handle())
    }

    /// Web seed rows for a torrent's detail pane.
    fn web_seed_rows(&self, infohash: &str) -> Vec<WebSeedRow> {
        let seeds = self.seeds.lock().unwrap();
        seeds
            .get(infohash)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| WebSeedRow {
                        url: entry.url.clone(),
                        state: entry.status.state(),
                        served_bytes: entry.status.served_bytes(),
                        error: entry.status.error(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Loopback ports the torrent's bridges are connected from.
    fn web_seed_ports(&self, infohash: &str) -> HashSet<u16> {
        let seeds = self.seeds.lock().unwrap();
        seeds
            .get(infohash)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| e.status.local_port())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Write the attached seeds to disk.
    fn persist(&self, seeds: &HashMap<String, Vec<SeedEntry>>) -> Result<()> {
        let Some(path) = &self.web_seeds.state_path else {
            return Ok(());
        };
        let mut store = SeedStore::default();
        for (infohash, entries) in seeds {
            for entry in entries {
                store.insert(infohash, entry.url.clone());
            }
        }
        webseed::state::save(path, &store)
    }
}

/// Address families tried when binding the peer listener, dual-stack first so
/// both IPv4 and IPv6 peers can connect.
const LISTEN_FAMILIES: [IpAddr; 2] = [
    IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
];

/// Pick the address to listen on for incoming peer connections.
///
/// librqbit binds a single address and fails the whole session if it is taken,
/// where it used to walk a port range itself. kist keeps its configured range
/// meaningful by probing it here, and falls back to an OS-assigned port so a
/// port clash costs the preferred port rather than preventing startup.
fn resolve_listen_addr(ports: std::ops::Range<u16>) -> (SocketAddr, Option<String>) {
    for ip in LISTEN_FAMILIES {
        if let Some(addr) = ports
            .clone()
            .map(|port| SocketAddr::new(ip, port))
            .find(bindable)
        {
            return (addr, None);
        }
    }
    let warning = format!(
        "ports {}-{} are unavailable, letting the OS choose the peer port",
        ports.start,
        ports.end.saturating_sub(1)
    );
    for ip in LISTEN_FAMILIES {
        let any = SocketAddr::new(ip, 0);
        if bindable(&any) {
            return (any, Some(warning));
        }
    }
    // Nothing binds at all; hand back the configured port so librqbit reports
    // the real reason rather than kist guessing at it.
    (
        SocketAddr::new(LISTEN_FAMILIES[0], ports.start),
        Some(warning),
    )
}

/// Whether `addr` can be bound right now. The probe socket is closed
/// immediately, so librqbit binds it moments later.
fn bindable(addr: &SocketAddr) -> bool {
    std::net::TcpListener::bind(addr).is_ok()
}

/// Where a bridge should dial to reach the session's own peer listener.
///
/// An unspecified bind address is not connectable, so it becomes loopback;
/// anything else is already the address the session is reachable on.
pub(crate) fn loopback_target(listen: SocketAddr) -> SocketAddr {
    if !listen.ip().is_unspecified() {
        return listen;
    }
    let ip = match listen.ip() {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
    };
    SocketAddr::new(ip, listen.port())
}

/// Build the file map and bridge parameters for one web seed on a torrent.
///
/// Returns `None` while the torrent's metadata is still resolving, since the
/// file layout is what the mapping is built from.
pub(crate) fn bridge_setup(
    handle: &ManagedTorrent,
    listen_addr: SocketAddr,
    url: &str,
    concurrency: usize,
) -> Option<(FileMap, BridgeParams)> {
    let shared = handle.shared();
    let info_hash = shared.info_hash;
    handle
        .with_metadata(|metadata| {
            let name = metadata
                .info
                .name()
                .map(|name| name.into_owned())
                .unwrap_or_else(|| info_hash.as_string());
            let files: Vec<TorrentFile> = metadata
                .file_infos
                .iter()
                .map(|file| TorrentFile {
                    path: file
                        .relative_filename
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect(),
                    offset: file.offset_in_torrent,
                    len: file.len,
                })
                .collect();
            let lengths = metadata.lengths();
            let map = FileMap::new(url, &name, metadata.info.info().files.is_some(), &files);
            let params = BridgeParams {
                listen_addr,
                info_hash,
                session_peer_id: shared.peer_id,
                piece_length: lengths.default_piece_length(),
                total_pieces: lengths.total_pieces(),
                bitfield_bytes: lengths.piece_bitfield_bytes(),
                concurrency,
            };
            (map, params)
        })
        .ok()
}

/// Load persisted web seeds, dropping any whose torrent is no longer present.
///
/// A state file that cannot be read is treated as empty: losing the seed list
/// is recoverable, refusing to start is not.
fn restore_web_seeds(
    session: &Session,
    settings: &WebSeedSettings,
) -> (HashMap<String, Vec<SeedEntry>>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut seeds = HashMap::new();
    if !settings.enabled {
        return (seeds, warnings);
    }
    if session.listen_addr().is_none() {
        warnings.push("web seeds need an incoming peer port, but none was bound".to_string());
    }
    let Some(path) = &settings.state_path else {
        return (seeds, warnings);
    };

    let (mut store, error) = webseed::state::load(path);
    if let Some(error) = error {
        warnings.push(error);
    }
    let known: HashSet<String> = session.with_torrents(|list| {
        list.map(|(_, handle)| handle.shared().info_hash.as_string())
            .collect()
    });
    store.retain_known(&|infohash| known.contains(infohash));
    for infohash in store.infohashes() {
        let entries = store
            .urls(infohash)
            .iter()
            .map(|url| SeedEntry::new(url.clone()))
            .collect();
        seeds.insert(infohash.clone(), entries);
    }
    (seeds, warnings)
}

/// Whether a peer address belongs to one of this torrent's bridges.
fn is_bridge_addr(addr: &str, ports: &HashSet<u16>) -> bool {
    if ports.is_empty() {
        return false;
    }
    let Some((host, port)) = addr.rsplit_once(':') else {
        return false;
    };
    if !matches!(host, "127.0.0.1" | "[::1]" | "::1" | "localhost") {
        return false;
    }
    port.parse().is_ok_and(|port| ports.contains(&port))
}

/// Append a note when the seed list could not be written.
fn with_save_result(message: &str, saved: Result<()>) -> String {
    match saved {
        Ok(()) => message.to_string(),
        Err(e) => format!("{message} (not saved: {e})"),
    }
}

/// Extract download/upload speeds and the live peer count from torrent stats.
fn live_speeds(stats: &TorrentStats) -> (u64, u64, usize) {
    match &stats.live {
        Some(live) => (
            mbps_to_bytes(live.download_speed.mbps),
            mbps_to_bytes(live.upload_speed.mbps),
            live.snapshot.peer_stats.live as usize,
        ),
        None => (0, 0, 0),
    }
}

/// Map a librqbit managed torrent into a plain [`TorrentRow`].
fn to_row(id: usize, handle: &ManagedTorrent) -> TorrentRow {
    let stats = handle.stats();
    let name = handle
        .name()
        .unwrap_or_else(|| handle.shared().info_hash.as_string());
    let (down_speed, up_speed, peers) = live_speeds(&stats);
    let eta = handle
        .live()
        .and_then(|l| l.down_speed_estimator().time_remaining());

    TorrentRow {
        id,
        name,
        total_bytes: stats.total_bytes,
        progress_bytes: stats.progress_bytes,
        uploaded_bytes: stats.uploaded_bytes,
        finished: stats.finished,
        down_speed,
        up_speed,
        eta,
        peers,
        state: to_row_state(stats.state),
        error: stats.error,
    }
}

fn to_row_state(state: TorrentStatsState) -> RowState {
    match state {
        // The paused flag here is the user's intent for once initializing
        // finishes; while it runs the torrent really is initializing.
        TorrentStatsState::Initializing { .. } => RowState::Initializing,
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
    /// Add a torrent with explicit options.
    AddWithOptions {
        /// Source string (magnet, path, or URL).
        source: String,
        /// Start the torrent paused.
        paused: bool,
        /// Alternate output folder, or `None` for the session default.
        output_folder: Option<String>,
        /// Explicit file indices to download, or `None` for all files.
        only_files: Option<Vec<usize>>,
    },
    /// List a source's files without adding it, for the add-options preview.
    PreviewAdd(String),
    /// Abort an in-flight add (e.g. a magnet stuck resolving metadata),
    /// identified by its source string.
    CancelAdd(String),
    /// Pause a torrent by id.
    Pause(usize),
    /// Resume a torrent by id.
    Resume(usize),
    /// Forget a torrent by id (files are kept).
    Remove(usize),
    /// Forget a torrent by id and delete its downloaded files.
    RemoveWithData(usize),
    /// Set which files of a torrent are downloaded.
    SetFiles {
        /// Torrent id.
        id: usize,
        /// File indices to keep downloading.
        included: HashSet<usize>,
    },
    /// Set the global download/upload rate limits (`None` = unlimited).
    SetLimits {
        /// Download cap in bytes per second.
        down: Option<u32>,
        /// Upload cap in bytes per second.
        up: Option<u32>,
    },
    /// Attach an HTTP source to a torrent as a web seed.
    AttachWebSeed {
        /// Torrent id.
        id: usize,
        /// HTTP or HTTPS URL of the source.
        url: String,
    },
    /// Detach a web seed from a torrent.
    DetachWebSeed {
        /// Torrent id.
        id: usize,
        /// URL to detach.
        url: String,
    },
    /// Begin publishing detail snapshots for the given torrent id.
    FetchDetail(usize),
    /// Stop publishing detail snapshots.
    StopDetail,
    /// Search public indexers with the given query.
    Search(String),
    /// Shut the engine task down.
    Quit,
}

/// A finished add-options file preview published back to the UI.
pub struct PreviewOutcome {
    /// The source string the preview was requested for, for correlation.
    pub source: String,
    /// The listed files, empty on failure.
    pub files: Vec<PreviewFile>,
    /// Error message when the preview failed.
    pub error: Option<String>,
}

/// A discrete status notification published by the engine for the UI.
pub struct EngineStatus {
    /// Human-readable message, already formatted for display.
    pub message: String,
    /// Whether this originated from an error.
    pub is_error: bool,
    /// Source of a completed [`Command::Add`] (success or failure), so the UI
    /// can clear its pending-add marker.
    pub finished_add: Option<String>,
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
    /// Search outcomes, one per [`Command::Search`].
    pub search: mpsc::UnboundedReceiver<SearchOutcome>,
    /// Add-options file previews, one per [`Command::PreviewAdd`].
    pub preview: mpsc::UnboundedReceiver<PreviewOutcome>,
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
    let (search_tx, search_rx) = mpsc::unbounded_channel::<SearchOutcome>();
    let (preview_tx, preview_rx) = mpsc::unbounded_channel::<PreviewOutcome>();

    for warning in engine.startup_warnings() {
        let _ = status_tx.send(EngineStatus {
            message: warning.clone(),
            is_error: true,
            finished_add: None,
        });
    }

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(refresh);
        // Torrent id currently shown in the detail pane, if any.
        let mut detail_id: Option<usize> = None;
        // Abort handles for in-flight adds, keyed by source, so a stuck add
        // (e.g. an unresolvable magnet) can be cancelled.
        let mut add_tasks: HashMap<String, AbortHandle> = HashMap::new();
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
                                        finished_add: None,
                                    });
                                }
                            }
                        }
                        Command::StopDetail => {
                            detail_id = None;
                            let _ = detail_tx.send(None);
                        }
                        // Setting limits is instant and live; no task needed.
                        Command::SetLimits { down, up } => engine.set_limits(down, up),
                        // Search runs against public indexers, not the session,
                        // so it gets its own task and result channel.
                        Command::Search(query) => {
                            let search_tx = search_tx.clone();
                            tokio::spawn(async move {
                                let _ = search_tx.send(search::search(query).await);
                            });
                        }
                        // A preview resolves metadata over the network, so it
                        // runs in its own task like search.
                        Command::PreviewAdd(source) => {
                            let engine = engine.clone();
                            let preview_tx = preview_tx.clone();
                            tokio::spawn(async move {
                                let outcome = match engine.preview(&source).await {
                                    Ok(files) => PreviewOutcome {
                                        source,
                                        files,
                                        error: None,
                                    },
                                    Err(e) => PreviewOutcome {
                                        source,
                                        files: Vec::new(),
                                        error: Some(error::to_status_line(&e)),
                                    },
                                };
                                let _ = preview_tx.send(outcome);
                            });
                        }
                        // Adds keep an abort handle so they can be cancelled
                        // while metadata is still resolving.
                        Command::Add(source) => {
                            let engine = engine.clone();
                            let snapshot_tx = snapshot_tx.clone();
                            let status_tx = status_tx.clone();
                            let cmd = Command::Add(source.clone());
                            let task = tokio::spawn(async move {
                                if let Some(status) = handle_command(&engine, cmd).await {
                                    let _ = status_tx.send(status);
                                }
                                let _ = snapshot_tx.send(engine.snapshot());
                            });
                            add_tasks.insert(source, task.abort_handle());
                        }
                        // Like Add, but with explicit options; also cancellable
                        // while metadata resolves.
                        Command::AddWithOptions {
                            source,
                            paused,
                            output_folder,
                            only_files,
                        } => {
                            let engine = engine.clone();
                            let snapshot_tx = snapshot_tx.clone();
                            let status_tx = status_tx.clone();
                            let key = source.clone();
                            let task = tokio::spawn(async move {
                                let result = engine
                                    .add_with_options(source.clone(), paused, output_folder, only_files)
                                    .await;
                                let status = match result {
                                    Ok(_) => EngineStatus {
                                        message: "added torrent".to_string(),
                                        is_error: false,
                                        finished_add: Some(source),
                                    },
                                    Err(e) => EngineStatus {
                                        message: error::to_status_line(&e),
                                        is_error: true,
                                        finished_add: Some(source),
                                    },
                                };
                                let _ = status_tx.send(status);
                                let _ = snapshot_tx.send(engine.snapshot());
                            });
                            add_tasks.insert(key, task.abort_handle());
                        }
                        Command::CancelAdd(source) => {
                            // A finished task means the real outcome is already
                            // on the status channel; stay silent then.
                            if let Some(handle) = add_tasks.remove(&source)
                                && !handle.is_finished()
                            {
                                handle.abort();
                                let _ = status_tx.send(EngineStatus {
                                    message: "cancelled add".to_string(),
                                    is_error: false,
                                    finished_add: Some(source),
                                });
                            }
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
                    add_tasks.retain(|_, handle| !handle.is_finished());
                    engine.reconcile_web_seeds();
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
        search: search_rx,
        preview: preview_rx,
    }
}

/// Apply a single action command, returning a status message if one should be shown.
async fn handle_command(engine: &Engine, cmd: Command) -> Option<EngineStatus> {
    let mut finished_add = None;
    let result: Result<String> = match cmd {
        Command::Add(source) => {
            let result = engine
                .add(source.clone())
                .await
                .map(|_| "added torrent".to_string());
            finished_add = Some(source);
            result
        }
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
        Command::RemoveWithData(id) => engine
            .remove_with_data(id)
            .await
            .map(|_| format!("deleted torrent {id} and its files")),
        Command::SetFiles { id, included } => engine
            .set_files(id, &included)
            .await
            .map(|_| format!("updated files for torrent {id}")),
        Command::AttachWebSeed { id, url } => engine.attach_web_seed(id, url),
        Command::DetachWebSeed { id, url } => engine.detach_web_seed(id, &url),
        // These are handled by the spawn loop, not here.
        Command::AddWithOptions { .. }
        | Command::PreviewAdd(_)
        | Command::SetLimits { .. }
        | Command::FetchDetail(_)
        | Command::StopDetail
        | Command::Search(_)
        | Command::CancelAdd(_)
        | Command::Quit => {
            return None;
        }
    };
    match result {
        Ok(message) => Some(EngineStatus {
            message,
            is_error: false,
            finished_add,
        }),
        Err(e) => Some(EngineStatus {
            message: error::to_status_line(&e),
            is_error: true,
            finished_add,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use librqbit::Magnet;

    #[test]
    fn listen_addr_uses_the_configured_range_when_free() {
        // Ask for a range starting at a port the OS just handed out and freed,
        // which is as close to "known free" as a test can get.
        let probe = std::net::TcpListener::bind("[::]:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let (addr, warning) = resolve_listen_addr(port..port + 4);
        assert!(
            (port..port + 4).contains(&addr.port()),
            "expected a port from the range, got {addr}"
        );
        assert!(warning.is_none(), "a usable range must not warn");
    }

    #[test]
    fn listen_addr_falls_back_when_the_range_is_taken() {
        // A one-port range around a port we hold, so the whole range is taken.
        // The listener is dual-stack, so it blocks both families kist tries.
        let held = std::net::TcpListener::bind("[::]:0").unwrap();
        let port = held.local_addr().unwrap().port();
        let range = port..port + 1;

        let (addr, warning) = resolve_listen_addr(range.clone());
        assert!(
            !range.contains(&addr.port()),
            "a taken range must not be reused"
        );
        assert!(
            warning.is_some_and(|w| w.contains("unavailable")),
            "falling back to another port must be reported"
        );
        drop(held);
    }

    #[test]
    fn bridges_dial_loopback_for_unspecified_listeners() {
        let v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 6881);
        assert_eq!(
            loopback_target(v6),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 6881)
        );

        let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 6881);
        assert_eq!(
            loopback_target(v4),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 6881)
        );

        // A listener bound to a real address is already reachable there.
        let specific = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)), 6881);
        assert_eq!(loopback_target(specific), specific);
    }

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
