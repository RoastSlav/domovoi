//! The connection's peer: who it is, and the length-prefixed request it sends.

use std::future::Future;

use domovoi_core::helper::frame_len;
use domovoi_core::identity::{AGENT_ACCOUNT, RefusalToken};
use domovoi_core::ports::{Clock, HostInspect};
use jiff::SignedDuration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use uuid::Uuid;

/// A connected stream whose peer's uid the kernel vouches for.
pub trait Connection: AsyncRead + AsyncWrite + Unpin + Send {
    /// The uid of the process on the other end (`SO_PEERCRED`).
    fn peer_uid(&self) -> std::io::Result<u32>;

    /// Reads and discards what the peer has already sent, without waiting for more.
    ///
    /// Closing a socket that still holds unread bytes resets the connection, which can destroy an
    /// answer the peer has not read yet.
    fn discard_unread(&mut self);
}

impl Connection for tokio::net::UnixStream {
    fn peer_uid(&self) -> std::io::Result<u32> {
        self.peer_cred().map(|cred| cred.uid())
    }

    fn discard_unread(&mut self) {
        use std::io::Read as _;
        use std::os::fd::AsFd as _;
        // The descriptor is non-blocking, so a read with nothing queued returns `WouldBlock`.
        let Ok(owned) = self.as_fd().try_clone_to_owned() else {
            return;
        };
        let mut file = std::fs::File::from(owned);
        let mut scratch = [0_u8; 4096];
        // The cap bounds the work on a peer that keeps sending.
        for _ in 0..64 {
            match file.read(&mut scratch) {
                Ok(n) if n > 0 => {}
                _ => return,
            }
        }
    }
}

/// Why the peer check did not pass.
#[derive(Debug)]
pub(super) enum PeerFault {
    /// The peer is not the agent (or the agent cannot be resolved, so nobody can be).
    NotAgent(u32),
    /// The kernel could not say who the peer is.
    Io(std::io::Error),
}

/// Accepts only the uid of `domovoi-agent`, looked up by name now. Root is never accepted.
pub(super) fn authorize<C: Connection>(
    conn: &C,
    inspect: &dyn HostInspect,
) -> Result<(), PeerFault> {
    let uid = conn.peer_uid().map_err(PeerFault::Io)?;
    let agent = match inspect.user(AGENT_ACCOUNT) {
        Ok(Some(account)) => account.uid,
        Ok(None) | Err(_) => return Err(PeerFault::NotAgent(uid)),
    };
    if uid == agent && uid != 0 {
        Ok(())
    } else {
        Err(PeerFault::NotAgent(uid))
    }
}

/// The result of reading one request.
pub(super) enum ReadOutcome {
    /// A complete body.
    Body(Vec<u8>),
    /// The deadline passed first.
    TimedOut,
    /// The peer closed before a header arrived.
    Closed,
    /// The header or body is not acceptable; answer with this token.
    Refused(RefusalToken),
    /// An unexpected I/O error.
    Failed(std::io::Error),
}

/// Reads the 4-byte length, checks it against the cap, then reads that many bytes, all within
/// `limit` on `clock`.
pub(super) async fn read_request<R: AsyncRead + Unpin>(
    reader: &mut R,
    clock: &dyn Clock,
    limit: SignedDuration,
) -> ReadOutcome {
    within(clock, limit, read_frame(reader))
        .await
        .unwrap_or(ReadOutcome::TimedOut)
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> ReadOutcome {
    let mut header = [0_u8; 4];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return ReadOutcome::Closed,
        Err(e) => return ReadOutcome::Failed(e),
    }
    let len = match frame_len(header) {
        Ok(len) => len,
        Err(err) => return ReadOutcome::Refused(err.token()),
    };
    let mut body = vec![0_u8; len];
    match reader.read_exact(&mut body).await {
        Ok(_) => ReadOutcome::Body(body),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            ReadOutcome::Refused(RefusalToken::FrameMalformed)
        }
        Err(e) => ReadOutcome::Failed(e),
    }
}

/// The `id` a request carries, or the nil id when the body does not have a readable one, so an
/// answer can still be correlated when the rest of the request is refused.
pub(super) fn request_id(body: &[u8]) -> Uuid {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("id")
                .and_then(|id| id.as_str().map(str::to_owned))
        })
        .and_then(|text| Uuid::parse_str(&text).ok())
        .unwrap_or_else(Uuid::nil)
}

/// Runs `fut` until it finishes or `limit` passes on `clock`; `None` on timeout.
async fn within<F: Future>(clock: &dyn Clock, limit: SignedDuration, fut: F) -> Option<F::Output> {
    let now = clock.now();
    let deadline = now.saturating_add(limit).unwrap_or(now);
    tokio::select! {
        out = fut => Some(out),
        () = clock.sleep_until(deadline) => None,
    }
}
