//! A loopback BitTorrent peer backed by an HTTP web seed.
//!
//! librqbit accepts incoming peer connections and routes them to a torrent by
//! the infohash in the handshake, so a web seed can be presented to it as an
//! ordinary peer. The bridge dials the session's own listen port, claims to
//! have every piece, unchokes, and answers each `request` with data fetched
//! over HTTP.
//!
//! The bridge only ever seeds. It never sends `interested` and ignores anything
//! the session offers, so it cannot consume upload bandwidth from the session.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use librqbit::ByteBuf;
use librqbit_core::Id20;
use librqbit_core::peer_id::generate_peer_id;
use librqbit_peer_protocol::{Handshake, Message, Piece};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;

use crate::model::WebSeedState;
use crate::webseed::SeedStatus;
use crate::webseed::fetch::{FetchError, Fetcher};

/// Azureus-style client prefix for the bridge's peer id. It must differ from
/// the session's own id or the session rejects the connection as a self-connect.
const PEER_ID_PREFIX: &[u8; 8] = b"-KIws01-";

/// Serialized keep-alive message (a bare zero length prefix).
const KEEP_ALIVE: [u8; 4] = [0, 0, 0, 0];

/// Wire size of a BitTorrent v1 handshake.
const HANDSHAKE_LEN: usize = 68;

/// Bytes a message needs on the wire beyond its variable-length payload: the
/// length prefix, the message id, and a `piece` message's index and offset.
const MESSAGE_OVERHEAD: usize = 13;

/// How often to send a keep-alive. librqbit drops a peer that is silent for
/// longer than its read timeout, which defaults to ten seconds.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(5);

/// Largest block the bridge will serve. Real clients ask for 16 KiB; anything
/// far above that is a malformed request.
const MAX_REQUEST_LEN: u32 = 128 * 1024;

/// Longest frame accepted from the session, a bound on the read buffer.
const MAX_FRAME_LEN: usize = 1024 * 1024;

/// First delay before reconnecting to the session; doubles per failure.
const RECONNECT_BASE: Duration = Duration::from_secs(1);

/// Longest delay between reconnection attempts.
const RECONNECT_MAX: Duration = Duration::from_secs(30);

/// Everything the bridge needs to present itself as a peer for one torrent.
#[derive(Debug, Clone)]
pub struct BridgeParams {
    /// Address of the session's own listener for incoming peer connections.
    pub listen_addr: SocketAddr,
    /// Infohash of the torrent to attach to.
    pub info_hash: Id20,
    /// The session's peer id, so the bridge can avoid colliding with it.
    pub session_peer_id: Id20,
    /// Length of a non-final piece.
    pub piece_length: u32,
    /// Number of pieces in the torrent.
    pub total_pieces: u32,
    /// Size of the piece bitfield in bytes, as the session expects it.
    pub bitfield_bytes: usize,
    /// Maximum concurrent HTTP fetches.
    pub concurrency: usize,
}

/// Why a bridge connection ended.
enum BridgeError {
    /// The web seed is unusable. Give up on it.
    Seed(String),
    /// The connection to the session failed. Reconnect later.
    Link(String),
}

/// Run a web seed bridge until the seed fails or the task is aborted.
///
/// Connection failures are retried with backoff, since they usually mean the
/// torrent is briefly not live. A failure of the web seed itself is terminal:
/// the bridge cannot retract its bitfield, so staying connected while refusing
/// requests would only make the session wait out request timeouts.
pub async fn run(params: BridgeParams, fetcher: Arc<Fetcher>, status: Arc<SeedStatus>) {
    let mut delay = RECONNECT_BASE;
    loop {
        status.set_state(WebSeedState::Connecting);
        let outcome = serve(&params, &fetcher, &status).await;
        status.set_local_port(0);
        match outcome {
            Ok(()) => delay = RECONNECT_BASE,
            Err(BridgeError::Seed(reason)) => {
                status.fail(reason);
                return;
            }
            // Losing the connection is routine: it is what a torrent that is
            // not live yet looks like from here. Keep the reason visible so a
            // seed stuck connecting still says why.
            Err(BridgeError::Link(reason)) => status.note_error(reason),
        }
        status.set_state(WebSeedState::Connecting);
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(RECONNECT_MAX);
    }
}

/// Connect to the session and serve piece requests until the connection ends.
async fn serve(
    params: &BridgeParams,
    fetcher: &Arc<Fetcher>,
    status: &Arc<SeedStatus>,
) -> Result<(), BridgeError> {
    let mut stream = TcpStream::connect(params.listen_addr)
        .await
        .map_err(|e| BridgeError::Link(format!("connect: {e}")))?;
    let _ = stream.set_nodelay(true);
    if let Ok(addr) = stream.local_addr() {
        status.set_local_port(addr.port());
    }

    let (mut read, mut write) = stream.split();
    let mut frames = Framer::default();

    handshake(params, &mut read, &mut write, &mut frames).await?;
    send_greeting(params, &mut write).await?;
    // Connected and unchoked, so the seed is available whether or not the
    // session happens to be requesting from it right now.
    status.clear_error();
    status.set_state(WebSeedState::Active);

    // Requests the session is still waiting on. A request it cancels is removed
    // here, and serving a piece it no longer expects makes it drop the peer.
    let pending: Arc<Mutex<HashSet<BlockKey>>> = Arc::default();
    let limiter = Arc::new(Semaphore::new(params.concurrency.max(1)));
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(64);
    let mut tasks: JoinSet<Result<(), String>> = JoinSet::new();
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);

    loop {
        // Drain everything already buffered before waiting for more.
        while let Some(frame) = frames.take_frame().map_err(BridgeError::Link)? {
            let message = Message::deserialize(&frame, &[])
                .map_err(|e| BridgeError::Link(format!("bad message: {e:?}")))?
                .0;
            match message {
                Message::Request(request) => {
                    if request.length > MAX_REQUEST_LEN {
                        return Err(BridgeError::Link(format!(
                            "session asked for {} bytes in one block",
                            request.length
                        )));
                    }
                    let key = (request.index, request.begin, request.length);
                    pending.lock().unwrap().insert(key);
                    tasks.spawn(serve_block(
                        key,
                        offset_of(params, request.index, request.begin),
                        limiter.clone(),
                        fetcher.clone(),
                        status.clone(),
                        pending.clone(),
                        out_tx.clone(),
                    ));
                }
                Message::Cancel(request) => {
                    pending
                        .lock()
                        .unwrap()
                        .remove(&(request.index, request.begin, request.length));
                }
                // The bridge only seeds, so everything the session tells us
                // about its own progress or interest is irrelevant.
                _ => {}
            }
        }

        tokio::select! {
            read = frames.fill(&mut read) => {
                match read {
                    Ok(0) => return Err(BridgeError::Link("session closed the connection".into())),
                    Ok(_) => {}
                    Err(e) => return Err(BridgeError::Link(format!("read: {e}"))),
                }
            }
            Some(message) = out_rx.recv() => {
                write.write_all(&message).await
                    .map_err(|e| BridgeError::Link(format!("write: {e}")))?;
            }
            Some(finished) = tasks.join_next(), if !tasks.is_empty() => {
                match finished {
                    Ok(Ok(())) => {}
                    Ok(Err(reason)) => return Err(BridgeError::Seed(reason)),
                    Err(e) if e.is_panic() => {
                        return Err(BridgeError::Seed(format!("bridge task panicked: {e}")));
                    }
                    Err(_) => {}
                }
            }
            _ = keep_alive.tick() => {
                write.write_all(&KEEP_ALIVE).await
                    .map_err(|e| BridgeError::Link(format!("keep-alive: {e}")))?;
            }
        }
    }
}

/// Exchange handshakes with the session and confirm it routed us to the right
/// torrent.
async fn handshake(
    params: &BridgeParams,
    read: &mut (impl tokio::io::AsyncRead + Unpin),
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
    frames: &mut Framer,
) -> Result<(), BridgeError> {
    let mut peer_id = generate_peer_id(PEER_ID_PREFIX);
    while peer_id == params.session_peer_id {
        peer_id = generate_peer_id(PEER_ID_PREFIX);
    }

    let mut ours = Handshake::new(params.info_hash, peer_id);
    // Opting out of BEP 10 means the session never sends an extended handshake,
    // so the bridge does not have to implement ut_metadata or ut_pex.
    ours.reserved = 0;
    let mut buf = [0u8; HANDSHAKE_LEN];
    let len = ours.serialize_unchecked_len(&mut buf);
    write
        .write_all(&buf[..len])
        .await
        .map_err(|e| BridgeError::Link(format!("write handshake: {e}")))?;

    loop {
        match Handshake::deserialize(frames.buffered()) {
            Ok((theirs, size)) => {
                if theirs.info_hash != params.info_hash {
                    return Err(BridgeError::Link(
                        "session sent a different infohash".into(),
                    ));
                }
                frames.consume(size);
                return Ok(());
            }
            Err(_) => {
                let n = frames
                    .fill(read)
                    .await
                    .map_err(|e| BridgeError::Link(format!("read handshake: {e}")))?;
                if n == 0 {
                    return Err(BridgeError::Link("session closed during handshake".into()));
                }
            }
        }
    }
}

/// Claim every piece and unchoke, so the session starts requesting immediately.
async fn send_greeting(
    params: &BridgeParams,
    write: &mut (impl tokio::io::AsyncWrite + Unpin),
) -> Result<(), BridgeError> {
    let mut bits = vec![0xFFu8; params.bitfield_bytes];
    // Spare bits past the last piece must be zero.
    let spare = params.bitfield_bytes * 8 - params.total_pieces as usize;
    if spare > 0
        && let Some(last) = bits.last_mut()
    {
        *last = 0xFFu8 << spare;
    }

    let mut out = Vec::new();
    for message in [Message::Bitfield(ByteBuf(&bits)), Message::Unchoke] {
        out.extend_from_slice(&serialize(&message, bits.len())?);
    }
    write
        .write_all(&out)
        .await
        .map_err(|e| BridgeError::Link(format!("write bitfield: {e}")))
}

/// Serialize one message into a fresh buffer.
///
/// librqbit serializes into a caller-sized slice, so `payload` is the size of
/// whatever variable-length body the message carries.
fn serialize(message: &Message<'_>, payload: usize) -> Result<Vec<u8>, BridgeError> {
    let mut buf = vec![0u8; MESSAGE_OVERHEAD + payload];
    let len = message
        .serialize(&mut buf, &Default::default)
        .map_err(|e| BridgeError::Link(format!("serialize: {e}")))?;
    buf.truncate(len);
    Ok(buf)
}

/// A block the session asked for, as `(piece, offset in piece, length)`.
type BlockKey = (u32, u32, u32);

/// Fetch one block over HTTP and queue it for sending, unless the session
/// cancelled it in the meantime.
async fn serve_block(
    key: BlockKey,
    offset: u64,
    limiter: Arc<Semaphore>,
    fetcher: Arc<Fetcher>,
    status: Arc<SeedStatus>,
    pending: Arc<Mutex<HashSet<BlockKey>>>,
    out: mpsc::Sender<Vec<u8>>,
) -> Result<(), String> {
    let (index, begin, length) = key;
    let _permit = limiter.acquire().await.map_err(|e| e.to_string())?;
    let block = match fetcher.read(offset, u64::from(length)).await {
        Ok(block) => block,
        // The fetcher already retried transient failures, so anything surfacing
        // here means the seed is done.
        Err(FetchError::Hard(reason) | FetchError::Transient(reason)) => return Err(reason),
    };

    if !pending.lock().unwrap().remove(&key) {
        return Ok(());
    }

    let message = Message::Piece(Piece::from_data(index, begin, &block));
    let buf = serialize(&message, block.len()).map_err(|e| match e {
        BridgeError::Seed(reason) | BridgeError::Link(reason) => reason,
    })?;
    status.add_served(block.len() as u64);
    let _ = out.send(buf).await;
    Ok(())
}

/// Torrent byte offset of a block within a piece.
fn offset_of(params: &BridgeParams, piece: u32, begin: u32) -> u64 {
    u64::from(piece) * u64::from(params.piece_length) + u64::from(begin)
}

/// Length-prefixed message framing over a byte stream.
///
/// Buffered bytes live outside the read future so [`Framer::fill`] stays
/// cancel-safe and can be used directly in a `select!`.
#[derive(Default)]
struct Framer {
    buf: Vec<u8>,
}

impl Framer {
    /// Bytes received but not yet consumed.
    fn buffered(&self) -> &[u8] {
        &self.buf
    }

    /// Drop the first `n` buffered bytes.
    fn consume(&mut self, n: usize) {
        self.buf.drain(..n);
    }

    /// Read whatever is available, appending it to the buffer. Returns the
    /// number of bytes read, where zero means end of stream.
    async fn fill(
        &mut self,
        read: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> std::io::Result<usize> {
        let mut chunk = [0u8; 8192];
        let n = read.read(&mut chunk).await?;
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(n)
    }

    /// Take one complete length-prefixed frame, if the buffer holds one.
    fn take_frame(&mut self) -> Result<Option<Vec<u8>>, String> {
        let Some(prefix) = self.buf.get(..4) else {
            return Ok(None);
        };
        let len = u32::from_be_bytes(prefix.try_into().expect("four bytes")) as usize;
        if len > MAX_FRAME_LEN {
            return Err(format!("session sent a {len} byte frame"));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        Ok(Some(self.buf.drain(..4 + len).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(piece_length: u32, total_pieces: u32) -> BridgeParams {
        BridgeParams {
            listen_addr: (std::net::Ipv4Addr::LOCALHOST, 1).into(),
            info_hash: Id20::new([0u8; 20]),
            session_peer_id: Id20::new([1u8; 20]),
            piece_length,
            total_pieces,
            bitfield_bytes: (total_pieces as usize).div_ceil(8),
            concurrency: 1,
        }
    }

    #[test]
    fn block_offsets_are_absolute() {
        let p = params(1024, 4);
        assert_eq!(offset_of(&p, 0, 0), 0);
        assert_eq!(offset_of(&p, 0, 512), 512);
        assert_eq!(offset_of(&p, 3, 16), 3 * 1024 + 16);
    }

    #[test]
    fn framer_yields_whole_frames_only() {
        let mut framer = Framer::default();
        framer.buf.extend_from_slice(&[0, 0, 0, 2, 9]);
        assert_eq!(framer.take_frame().unwrap(), None);
        framer.buf.push(7);
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 2, 9, 7]));
        assert_eq!(framer.take_frame().unwrap(), None);
    }

    #[test]
    fn framer_handles_keep_alives_and_back_to_back_frames() {
        let mut framer = Framer::default();
        framer
            .buf
            .extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0]);
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 0]));
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 1, 1]));
        assert_eq!(framer.take_frame().unwrap(), Some(vec![0, 0, 0, 0]));
        assert_eq!(framer.take_frame().unwrap(), None);
    }

    #[test]
    fn framer_rejects_absurd_frames() {
        let mut framer = Framer::default();
        framer.buf.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert!(framer.take_frame().is_err());
    }

    #[test]
    fn bitfield_spare_bits_are_zeroed() {
        // 12 pieces occupy two bytes, so the low four bits must be clear.
        let p = params(1024, 12);
        let mut bits = vec![0xFFu8; p.bitfield_bytes];
        let spare = p.bitfield_bytes * 8 - p.total_pieces as usize;
        if spare > 0 {
            *bits.last_mut().unwrap() = 0xFFu8 << spare;
        }
        assert_eq!(bits, vec![0xFF, 0xF0]);
    }

    #[test]
    fn bitfield_is_all_ones_when_pieces_fill_the_bytes() {
        let p = params(1024, 16);
        assert_eq!(p.bitfield_bytes, 2);
        let spare = p.bitfield_bytes * 8 - p.total_pieces as usize;
        assert_eq!(spare, 0);
    }
}
