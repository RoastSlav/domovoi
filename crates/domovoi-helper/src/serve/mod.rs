//! Serving one connection: peer check, one framed request, one framed answer.

mod dispatch;
mod peer;

use std::sync::Arc;

use domovoi_config::{ConfigIssue, HelperConfig};
use domovoi_core::helper::{HELPER_PROTOCOL, HelperResponse, HelperResponseFrame, encode_frame};
use domovoi_core::identity::{HostDirs, RefusalToken};
use domovoi_core::ports::{Clock, DockerReadApi, HostInspect, ProcessExecutor};
use jiff::SignedDuration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

pub use dispatch::{read_platform_record, unavailable_reason};
pub use peer::Connection;

use crate::start_check::run_start_checks;

/// How long a peer may take to send its request, measured on the injected clock.
pub const DEFAULT_READ_DEADLINE: SignedDuration = SignedDuration::from_secs(10);

/// Everything a connection needs to be served; nothing in it outlives the connection.
pub struct ServeContext {
    /// `helper.toml` as the command loaded it, or the issues that stopped it loading.
    pub config: Result<HelperConfig, Vec<ConfigIssue>>,
    /// The read-only view of the host used by the peer check, the start checks and the record read.
    pub inspect: Arc<dyn HostInspect>,
    /// The clock the read deadline runs on.
    pub clock: Arc<dyn Clock>,
    /// The directories of the layout.
    pub dirs: HostDirs,
    /// How long the peer may take to send its request.
    pub read_deadline: SignedDuration,
    /// The build string `health` reports.
    pub build: String,
    /// Runs `ip` for `safety.attest`. Without one every route read fails and the attestation is
    /// a signed incomplete report.
    pub executor: Option<Arc<dyn ProcessExecutor>>,
    /// The read-only view of the container engine for `docker.read`. Without one every Docker
    /// request is answered `engine_absent`.
    pub docker: Option<Arc<dyn DockerReadApi>>,
}

impl ServeContext {
    /// A context with the default read deadline.
    #[must_use]
    pub fn new(
        config: Result<HelperConfig, Vec<ConfigIssue>>,
        inspect: Arc<dyn HostInspect>,
        clock: Arc<dyn Clock>,
        dirs: HostDirs,
        build: impl Into<String>,
    ) -> Self {
        Self {
            config,
            inspect,
            clock,
            dirs,
            read_deadline: DEFAULT_READ_DEADLINE,
            build: build.into(),
            executor: None,
            docker: None,
        }
    }

    /// The same context with `api` answering the Docker reads.
    #[must_use]
    pub fn with_docker_read(mut self, api: Arc<dyn DockerReadApi>) -> Self {
        self.docker = Some(api);
        self
    }

    /// The same context with `executor` running the route reads of `safety.attest`.
    #[must_use]
    pub fn with_process_executor(mut self, executor: Arc<dyn ProcessExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }
}

/// How serving a connection ended. Only `Failed` makes the process exit non-zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServeOutcome {
    /// An answer, including a refusal, was written.
    Answered,
    /// The peer sent nothing before the deadline; the connection was closed without an answer.
    TimedOut,
    /// The peer closed the connection before a complete request, or before reading the answer.
    PeerClosed,
    /// A defect or an unexpected I/O error; the process exits non-zero.
    Failed(String),
}

/// Serves one request on `conn` and returns.
///
/// Order: the peer's uid must be that of `domovoi-agent`, resolved now (root is not accepted);
/// the length header is read and checked against the cap before any body byte, all under the
/// read deadline on the injected clock; the start checks run and a failure answers every request;
/// the body is decoded; the closed set of operations is dispatched; one frame is written and the
/// stream is shut down. Never panics.
pub async fn serve_connection<C: Connection>(mut conn: C, ctx: &ServeContext) -> ServeOutcome {
    match peer::authorize(&conn, ctx.inspect.as_ref()) {
        Ok(()) => {}
        Err(peer::PeerFault::NotAgent(uid)) => {
            tracing::warn!(
                peer_uid = uid,
                "refused a connection from a uid that is not the agent"
            );
            return refuse_unread(&mut conn, refused(RefusalToken::PeerNotAgent)).await;
        }
        Err(peer::PeerFault::Io(err)) => return ServeOutcome::Failed(err.to_string()),
    }
    let body = match peer::read_request(&mut conn, ctx.clock.as_ref(), ctx.read_deadline).await {
        peer::ReadOutcome::Body(body) => body,
        peer::ReadOutcome::TimedOut => return ServeOutcome::TimedOut,
        peer::ReadOutcome::Closed => return ServeOutcome::PeerClosed,
        peer::ReadOutcome::Refused(token) => {
            return refuse_unread(&mut conn, refused(token)).await;
        }
        peer::ReadOutcome::Failed(err) => return ServeOutcome::Failed(err.to_string()),
    };
    let id = peer::request_id(&body);
    if let Some(response) = start_refusal(ctx) {
        return reply(&mut conn, id, response).await;
    }
    let response = match domovoi_core::helper::decode_request(&body) {
        Ok(frame) => dispatch::dispatch(&frame.request, ctx, conn.peer_uid().ok()).await,
        Err(token) => refused(token),
    };
    reply(&mut conn, id, response).await
}

/// Answers a request that was not (fully) read, then discards what the peer already sent.
async fn refuse_unread<C: Connection>(conn: &mut C, response: HelperResponse) -> ServeOutcome {
    let outcome = reply(conn, Uuid::nil(), response).await;
    conn.discard_unread();
    outcome
}

fn refused(token: RefusalToken) -> HelperResponse {
    HelperResponse::Refused { token }
}

/// The answer for a failed start check or an unusable configuration, `None` when all is well.
fn start_refusal(ctx: &ServeContext) -> Option<HelperResponse> {
    if let Err(refusal) = run_start_checks(ctx.inspect.as_ref(), &ctx.dirs) {
        for fault in &refusal.faults {
            tracing::error!(
                path = %fault.path.display(),
                why = ?fault.why,
                found = ?fault.found,
                expected = ?fault.expected,
                "start check failed"
            );
        }
        tracing::error!(token = %refusal.token, "the helper refuses to serve");
        return Some(HelperResponse::StartRefused {
            token: refusal.token,
            faults: refusal.faults,
        });
    }
    if let Err(issues) = &ctx.config {
        for issue in issues {
            tracing::error!(key = %issue.path, "helper.toml: {}", issue.message);
        }
        return Some(HelperResponse::StartRefused {
            token: RefusalToken::HelperConfigInvalid,
            faults: Vec::new(),
        });
    }
    None
}

async fn reply<W: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut W,
    id: Uuid,
    response: HelperResponse,
) -> ServeOutcome {
    let frame = HelperResponseFrame {
        v: HELPER_PROTOCOL,
        id,
        response,
    };
    let bytes = match encode_frame(&frame) {
        Ok(bytes) => bytes,
        Err(err) => return ServeOutcome::Failed(err.to_string()),
    };
    let written = async {
        conn.write_all(&bytes).await?;
        conn.flush().await?;
        conn.shutdown().await
    }
    .await;
    match written {
        Ok(()) => ServeOutcome::Answered,
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => ServeOutcome::PeerClosed,
        Err(err) if err.kind() == std::io::ErrorKind::NotConnected => ServeOutcome::Answered,
        Err(err) => ServeOutcome::Failed(err.to_string()),
    }
}
