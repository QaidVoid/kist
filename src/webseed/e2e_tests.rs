//! End-to-end tests for the web seed bridge.
//!
//! These run a real librqbit session and a stub HTTP server over loopback, so
//! they exercise the whole path: handshake, bitfield, piece requests, ranged
//! GETs, and the session's own hash verification.

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{CreateTorrentOptions, ListenerOptions, Session, SessionOptions, create_torrent};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::model::WebSeedState;
use crate::webseed::SeedStatus;
use crate::webseed::fetch::Fetcher;

/// How a stub server answers requests, so failure paths can be exercised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServeMode {
    /// Honor `Range` properly.
    Ranges,
    /// Ignore `Range` and return the whole file with `200 OK`.
    IgnoreRange,
    /// Answer everything with `404`.
    NotFound,
    /// Honor `Range` but return the wrong bytes.
    Corrupt,
}

/// Deterministic pseudorandom bytes, so torrents have real piece hashes without
/// depending on a random source.
fn content(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as u8
        })
        .collect()
}

/// A temp directory removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("kist-webseed-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Bytes a stub server has sent, so tests can catch a seed downloading the same
/// data more than once.
type Served = Arc<std::sync::atomic::AtomicU64>;

/// Serve `root` over HTTP on loopback, returning the base URL.
///
/// Deliberately minimal: enough of HTTP/1.1 to answer the ranged GETs the
/// fetcher issues, and nothing else.
async fn serve(root: PathBuf, mode: ServeMode) -> String {
    serve_counted(root, mode).await.0
}

async fn serve_counted(root: PathBuf, mode: ServeMode) -> (String, Served) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served: Served = Served::default();
    let counter = served.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let root = root.clone();
            let counter = counter.clone();
            tokio::spawn(async move {
                let _ = handle_request(stream, root, mode, counter).await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}/"), served)
}

async fn handle_request(
    mut stream: TcpStream,
    root: PathBuf,
    mode: ServeMode,
    served: Served,
) -> std::io::Result<()> {
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while !request.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await? == 0 {
            return Ok(());
        }
        request.push(byte[0]);
    }
    let request = String::from_utf8_lossy(&request).to_string();
    let mut lines = request.lines();
    let target = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/")
        .to_string();
    // Header names are case-insensitive, and hyper sends them lowercase.
    let range = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("range")
                .then(|| value.trim().strip_prefix("bytes="))?
        })
        .and_then(parse_range);

    let path = root.join(percent_decode(target.trim_start_matches('/')));
    let Ok(body) = std::fs::read(&path) else {
        return respond(&mut stream, 404, "Not Found", None, b"missing").await;
    };
    if mode == ServeMode::NotFound {
        return respond(&mut stream, 404, "Not Found", None, b"missing").await;
    }
    if mode == ServeMode::IgnoreRange || range.is_none() {
        return respond(&mut stream, 200, "OK", None, &body).await;
    }

    let (start, end) = range.unwrap();
    let end = end.min(body.len().saturating_sub(1));
    let mut slice = body[start..=end].to_vec();
    if mode == ServeMode::Corrupt {
        // Flip every byte, so the data is the right length but hashes wrong.
        for byte in &mut slice {
            *byte = !*byte;
        }
    }
    let header = format!("bytes {start}-{end}/{}", body.len());
    served.fetch_add(slice.len() as u64, std::sync::atomic::Ordering::Relaxed);
    respond(&mut stream, 206, "Partial Content", Some(&header), &slice).await
}

fn parse_range(spec: &str) -> Option<(usize, usize)> {
    let (start, end) = spec.trim().split_once('-')?;
    Some((start.parse().ok()?, end.parse().unwrap_or(usize::MAX)))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn respond(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    content_range: Option<&str>,
    body: &[u8],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(range) = content_range {
        head.push_str(&format!("Content-Range: {range}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}

/// Start a session listening on a loopback port chosen by the OS, so tests
/// never collide with each other or reach the network.
async fn session(download_dir: &Path) -> Arc<Session> {
    Session::new_with_opts(
        download_dir.to_path_buf(),
        SessionOptions {
            dht: None,
            persistence: None,
            listen: Some(ListenerOptions {
                listen_addr: (Ipv4Addr::LOCALHOST, 0).into(),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

/// Build a torrent from `path`, add it to a fresh session downloading into a
/// separate directory, and attach a bridge for `url`.
///
/// Returns the session, the torrent handle, and the seed status.
async fn attach(
    source: &Path,
    download_dir: &Path,
    url: &str,
    mode_status: Arc<SeedStatus>,
) -> (Arc<Session>, Arc<librqbit::ManagedTorrent>) {
    let torrent = create_torrent(
        source,
        CreateTorrentOptions {
            // Small pieces keep the fixtures small while still spanning several
            // pieces and file boundaries.
            piece_length: Some(32 * 1024),
            ..Default::default()
        },
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let bytes = torrent.as_bytes().unwrap();

    let session = session(download_dir).await;
    let handle = session
        .add_torrent(librqbit::AddTorrent::from_bytes(bytes), None)
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    handle.wait_until_initialized().await.unwrap();

    let listen_addr = crate::engine::loopback_target(session.listen_addr().unwrap());
    let (map, params) = crate::engine::bridge_setup(&handle, listen_addr, url, 4).unwrap();
    let fetcher = Arc::new(Fetcher::new(
        map,
        mode_status.clone(),
        params.piece_length,
        4,
    ));
    tokio::spawn(crate::webseed::bridge::run(params, fetcher, mode_status));
    (session, handle)
}

/// Poll until `check` passes or the timeout expires.
async fn wait_for(timeout: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    check()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn single_file_torrent_downloads_from_a_web_seed() {
    let served = TempDir::new("src-single");
    let data = content(300 * 1024, 7);
    std::fs::write(served.path().join("movie.bin"), &data).unwrap();

    let base = serve(served.path().to_path_buf(), ServeMode::Ranges).await;
    let out = TempDir::new("out-single");
    let status = Arc::new(SeedStatus::default());
    let (_session, handle) = attach(
        &served.path().join("movie.bin"),
        out.path(),
        &base,
        status.clone(),
    )
    .await;

    let finished = wait_for(Duration::from_secs(30), || handle.stats().finished).await;
    assert!(
        finished,
        "torrent did not complete from the web seed alone (seed state: {:?}, error: {:?})",
        status.state(),
        status.error()
    );

    let downloaded = std::fs::read(out.path().join("movie.bin")).unwrap();
    assert_eq!(downloaded, data, "downloaded content must match the source");
    assert!(
        status.served_bytes() > 0,
        "the web seed should have served the payload"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_file_torrent_downloads_across_file_boundaries() {
    let served = TempDir::new("src-multi");
    let root = served.path().join("album");
    std::fs::create_dir_all(root.join("disc 1")).unwrap();
    // Sizes chosen so pieces straddle both file boundaries.
    let a = content(50 * 1024, 1);
    let b = content(40 * 1024, 2);
    let c = content(60 * 1024, 3);
    std::fs::write(root.join("disc 1").join("one.bin"), &a).unwrap();
    std::fs::write(root.join("disc 1").join("two.bin"), &b).unwrap();
    std::fs::write(root.join("three.bin"), &c).unwrap();

    let base = serve(served.path().to_path_buf(), ServeMode::Ranges).await;
    let out = TempDir::new("out-multi");
    let status = Arc::new(SeedStatus::default());
    let (_session, handle) = attach(&root, out.path(), &base, status.clone()).await;

    let finished = wait_for(Duration::from_secs(30), || handle.stats().finished).await;
    assert!(
        finished,
        "multi-file torrent did not complete (seed state: {:?}, error: {:?})",
        status.state(),
        status.error()
    );

    assert_eq!(
        std::fs::read(out.path().join("album/disc 1/one.bin")).unwrap(),
        a
    );
    assert_eq!(
        std::fs::read(out.path().join("album/disc 1/two.bin")).unwrap(),
        b
    );
    assert_eq!(
        std::fs::read(out.path().join("album/three.bin")).unwrap(),
        c
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_that_ignores_range_fails_the_seed() {
    let served = TempDir::new("src-norange");
    let data = content(200 * 1024, 11);
    std::fs::write(served.path().join("movie.bin"), &data).unwrap();

    let base = serve(served.path().to_path_buf(), ServeMode::IgnoreRange).await;
    let out = TempDir::new("out-norange");
    let status = Arc::new(SeedStatus::default());
    let (_session, handle) = attach(
        &served.path().join("movie.bin"),
        out.path(),
        &base,
        status.clone(),
    )
    .await;

    let failed = wait_for(Duration::from_secs(20), || {
        status.state() == WebSeedState::Failed
    })
    .await;
    assert!(failed, "a server ignoring Range must fail the seed");
    assert!(
        status.error().is_some_and(|e| e.contains("Range")),
        "the failure should name the cause, got {:?}",
        status.error()
    );
    assert!(
        !handle.stats().finished,
        "nothing should have completed from a broken seed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_missing_file_fails_the_seed() {
    let served = TempDir::new("src-404");
    let data = content(100 * 1024, 13);
    std::fs::write(served.path().join("movie.bin"), &data).unwrap();

    let base = serve(served.path().to_path_buf(), ServeMode::NotFound).await;
    let out = TempDir::new("out-404");
    let status = Arc::new(SeedStatus::default());
    let (_session, _handle) = attach(
        &served.path().join("movie.bin"),
        out.path(),
        &base,
        status.clone(),
    )
    .await;

    let failed = wait_for(Duration::from_secs(20), || {
        status.state() == WebSeedState::Failed
    })
    .await;
    assert!(failed, "a 404 must fail the seed rather than retry forever");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_web_seed_is_not_downloaded_more_than_once() {
    let served_dir = TempDir::new("src-once");
    // Several windows worth of data, so concurrent workers overlap.
    let size = 5 * 1024 * 1024;
    let data = content(size, 31);
    std::fs::write(served_dir.path().join("movie.bin"), &data).unwrap();

    let (base, served) = serve_counted(served_dir.path().to_path_buf(), ServeMode::Ranges).await;
    let out = TempDir::new("out-once");
    let status = Arc::new(SeedStatus::default());
    let (_session, handle) = attach(
        &served_dir.path().join("movie.bin"),
        out.path(),
        &base,
        status.clone(),
    )
    .await;

    assert!(
        wait_for(Duration::from_secs(60), || handle.stats().finished).await,
        "torrent did not complete: {:?}",
        status.error()
    );

    // Concurrent workers must not each fetch the same window. Some overhead is
    // fine; fetching the whole torrent several times over is not.
    let total = served.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        total < (size as u64) * 3 / 2,
        "web seed served {total} bytes for a {size} byte torrent, so windows are being refetched"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_connected_seed_reports_active_before_it_is_asked_for_anything() {
    let served = TempDir::new("src-active");
    std::fs::write(served.path().join("movie.bin"), content(64 * 1024, 29)).unwrap();

    let base = serve(served.path().to_path_buf(), ServeMode::Ranges).await;
    let out = TempDir::new("out-active");
    let status = Arc::new(SeedStatus::default());
    let (_session, _handle) = attach(
        &served.path().join("movie.bin"),
        out.path(),
        &base,
        status.clone(),
    )
    .await;

    // Being connected and unchoked is what makes a seed available, so the state
    // must not depend on the session happening to request something.
    let active = wait_for(Duration::from_secs(20), || {
        status.state() == WebSeedState::Active
    })
    .await;
    assert!(
        active,
        "a connected seed should report active, got {:?}",
        status.state()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_torrent_resumes_downloading_from_its_web_seed() {
    let served = TempDir::new("src-pause");
    let data = content(300 * 1024, 23);
    std::fs::write(served.path().join("movie.bin"), &data).unwrap();

    let base = serve(served.path().to_path_buf(), ServeMode::Ranges).await;
    let out = TempDir::new("out-pause");
    let status = Arc::new(SeedStatus::default());
    let (session, handle) = attach(
        &served.path().join("movie.bin"),
        out.path(),
        &base,
        status.clone(),
    )
    .await;

    // A paused torrent is not live, so the session refuses the bridge's
    // connection entirely. It must keep trying rather than give up.
    session.pause(&handle).await.unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_ne!(
        status.state(),
        WebSeedState::Failed,
        "a paused torrent must not fail the seed: {:?}",
        status.error()
    );

    session.unpause(&handle).await.unwrap();
    let finished = wait_for(Duration::from_secs(60), || handle.stats().finished).await;
    assert!(
        finished,
        "the torrent should complete once resumed (seed state: {:?}, error: {:?})",
        status.state(),
        status.error()
    );
    assert_eq!(std::fs::read(out.path().join("movie.bin")).unwrap(), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn corrupt_web_seed_data_never_completes_the_torrent() {
    let served = TempDir::new("src-corrupt");
    let data = content(200 * 1024, 17);
    std::fs::write(served.path().join("movie.bin"), &data).unwrap();

    let base = serve(served.path().to_path_buf(), ServeMode::Corrupt).await;
    let out = TempDir::new("out-corrupt");
    let status = Arc::new(SeedStatus::default());
    let (_session, handle) = attach(
        &served.path().join("movie.bin"),
        out.path(),
        &base,
        status.clone(),
    )
    .await;

    // The session hashes every piece, so a seed serving well-formed but wrong
    // bytes can never complete the torrent no matter how much it serves.
    let completed = wait_for(Duration::from_secs(10), || handle.stats().finished).await;
    assert!(
        !completed,
        "corrupt web seed data must never be accepted as complete"
    );
    assert_eq!(
        handle.stats().progress_bytes,
        0,
        "no corrupt piece should have been counted as progress"
    );
}
