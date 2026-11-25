//! The runner side of the agent-to-runner protocol: serve exactly one connection.
//!
//! The binary adopts the one connection systemd hands the process and gives it to
//! [`serve_connection`] together with ports for the host ([`HostInspect`]) and the isolation
//! probe ([`ProbeCollector`]). Every defined outcome (a result, a refusal, a timeout, a
//! cancel, a vanished peer) returns `Ok`, so the process exits 0 and leaves no failed unit.

mod connection;
mod peer;
mod roots;
mod run;
mod start_checks;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use domovoi_core::isolation::IsolationReport;
use domovoi_core::ports::HostInspect;
use domovoi_core::skill_runner::FrameError;
use tokio::net::UnixStream;

pub use peer::{RequestKind, peer_allowed};

/// How long a peer gets to send its request frame.
pub const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(10);

/// Collects the runner's observations of its own filesystem view for a `Probe` request.
///
/// Implemented by the binary over the helper crate's collector, because the runner library
/// may not depend on that crate.
pub trait ProbeCollector: Send + Sync {
    /// The observations, with `helper_pid` supplied by a root caller for the helper-visibility point.
    fn collect(&self, helper_pid: Option<u32>) -> IsolationReport;
}

/// Everything one served connection needs besides the stream.
pub struct ServeContext {
    /// The skills directory the unit was rendered with.
    pub skills_root: PathBuf,
    /// The packages directory the unit was rendered with.
    pub packages_root: PathBuf,
    /// Read-only window onto accounts and `/proc`, for the start checks and the peer rule.
    pub inspect: Arc<dyn HostInspect>,
    /// Produces the report a `Probe` request returns.
    pub probe: Arc<dyn ProbeCollector>,
    /// How long the request frame may take to arrive.
    pub request_timeout: Duration,
}

impl ServeContext {
    /// A context with the default request timeout ([`REQUEST_READ_TIMEOUT`]).
    #[must_use]
    pub fn new(
        skills_root: PathBuf,
        packages_root: PathBuf,
        inspect: Arc<dyn HostInspect>,
        probe: Arc<dyn ProbeCollector>,
    ) -> Self {
        Self {
            skills_root,
            packages_root,
            inspect,
            probe,
            request_timeout: REQUEST_READ_TIMEOUT,
        }
    }
}

/// A defect while serving; nothing a peer can cause.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// A reply could not be encoded, even as a refusal.
    #[error("cannot encode a reply: {0}")]
    Encode(FrameError),
    /// The probe collector panicked or was cancelled.
    #[error("the probe collector failed: {0}")]
    Probe(String),
}

/// Serves one request on `stream` and returns when the reply is written, the run is cancelled
/// or the peer is gone.
///
/// The peer uid comes from `SO_PEERCRED`; a stream whose peer cannot be read is refused as
/// `peer_not_allowed`.
///
/// # Errors
/// Only a defect ([`ServeError`]); every outcome a peer can cause is `Ok`.
pub async fn serve_connection(stream: UnixStream, ctx: ServeContext) -> Result<(), ServeError> {
    let peer = stream.peer_cred().ok().map(|cred| cred.uid());
    serve_connection_with_peer(stream, peer, ctx).await
}

/// [`serve_connection`] with the peer uid given, which tests use because `SO_PEERCRED` cannot be
/// faked on a socket pair.
///
/// # Errors
/// Only a defect ([`ServeError`]).
pub async fn serve_connection_with_peer(
    stream: UnixStream,
    peer_uid: Option<u32>,
    ctx: ServeContext,
) -> Result<(), ServeError> {
    connection::serve(stream, peer_uid, ctx).await
}
