//! The agent's side of the runner socket: one request per connection, one buffered reply.
//!
//! The write half stays open until the reply has been read, because the runner treats an
//! early close as a cancel. Dropping the future of [`RunnerClient::run`] closes the
//! connection, which is how a deadline or a shutdown cancels a running skill.

use std::path::PathBuf;
use std::sync::Arc;

use domovoi_core::isolation::IsolationReport;
use domovoi_core::ports::Clock;
use domovoi_core::skill_runner::{
    FrameError, Header, HealthReply, ProbeRequest, RUNNER_PROTOCOL, RUNNER_SOCKET_PATH, Refusal,
    RunReply, RunRequest, RunnerReply, RunnerRequest,
};
use jiff::SignedDuration;

#[cfg(unix)]
use crate::time::within;

/// How long connecting may take, measured on the injected clock.
const CONNECT_DEADLINE: SignedDuration = SignedDuration::from_secs(3);
/// How long a `health` reply may take.
const HEALTH_DEADLINE: SignedDuration = SignedDuration::from_secs(3);
/// How long a `probe` reply may take.
const PROBE_DEADLINE: SignedDuration = SignedDuration::from_secs(10);

/// Why an exchange with the runner failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The socket path does not exist: no runner is installed.
    #[error("the skill runner socket does not exist")]
    Absent,
    /// The socket exists but this account may not connect to it.
    #[error("permission denied connecting to the skill runner socket")]
    PermissionDenied,
    /// Connecting or writing failed for another reason.
    #[error("the skill runner is unreachable: {0}")]
    Connect(String),
    /// The runner closed the connection without answering.
    #[error("the skill runner closed the connection without a reply")]
    ClosedWithoutReply,
    /// A deadline passed.
    #[error("the skill runner did not answer in time")]
    TimedOut,
    /// The answer was not a well-formed frame or not the kind that was asked for.
    #[error("the skill runner sent a malformed reply: {0}")]
    Malformed(String),
    /// The runner refused the request.
    #[error("the skill runner refused the request: {}: {}", .0.code.as_str(), .0.detail)]
    Refused(Refusal),
}

/// A client for the runner's local socket.
#[derive(Clone)]
pub struct RunnerClient {
    socket: PathBuf,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for RunnerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunnerClient")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

impl RunnerClient {
    /// A client for the socket at `socket`; deadlines are measured on `clock`.
    #[must_use]
    pub fn new(socket: PathBuf, clock: Arc<dyn Clock>) -> Self {
        Self { socket, clock }
    }

    /// A client for the socket the runner unit listens on.
    #[must_use]
    pub fn default_path(clock: Arc<dyn Clock>) -> Self {
        Self::new(PathBuf::from(RUNNER_SOCKET_PATH), clock)
    }

    /// Asks for the runner's version and the roots it was started with (3 s to connect, 3 s to answer).
    pub async fn health(&self) -> Result<HealthReply, ClientError> {
        let request = RunnerRequest::Health(Header {
            protocol: RUNNER_PROTOCOL,
        });
        match self.exchange(&request, Some(HEALTH_DEADLINE)).await? {
            RunnerReply::Health(reply) => Ok(reply),
            other => Err(unexpected(other)),
        }
    }

    /// Runs one skill. The reply is awaited without a deadline here: the caller bounds it, and
    /// dropping the returned future closes the connection, which cancels the run.
    pub async fn run(&self, request: RunRequest) -> Result<RunReply, ClientError> {
        match self.exchange(&RunnerRequest::Run(request), None).await? {
            RunnerReply::Run(reply) => Ok(reply),
            other => Err(unexpected(other)),
        }
    }

    /// Asks for the runner's own filesystem observations (10 s to answer). `helper_pid` is
    /// only honoured when the caller is root.
    pub async fn probe(&self, helper_pid: Option<u32>) -> Result<IsolationReport, ClientError> {
        let request = RunnerRequest::Probe(ProbeRequest {
            protocol: RUNNER_PROTOCOL,
            helper_pid,
        });
        match self.exchange(&request, Some(PROBE_DEADLINE)).await? {
            RunnerReply::Probe(report) => Ok(*report),
            other => Err(unexpected(other)),
        }
    }

    #[cfg(unix)]
    async fn exchange(
        &self,
        request: &RunnerRequest,
        reply_deadline: Option<SignedDuration>,
    ) -> Result<RunnerReply, ClientError> {
        use domovoi_core::skill_runner::{MAX_REQUEST_BYTES, encode_frame};

        let bytes = encode_frame(request, MAX_REQUEST_BYTES).map_err(frame_error)?;
        let mut stream = within(
            &*self.clock,
            CONNECT_DEADLINE,
            tokio::net::UnixStream::connect(&self.socket),
        )
        .await
        .ok_or(ClientError::TimedOut)?
        .map_err(connect_error)?;
        let io = round_trip(&mut stream, &bytes);
        let reply = match reply_deadline {
            Some(limit) => within(&*self.clock, limit, io)
                .await
                .ok_or(ClientError::TimedOut)??,
            None => io.await?,
        };
        // The stream is dropped here, after the reply: never before it.
        drop(stream);
        Ok(reply)
    }

    #[cfg(not(unix))]
    async fn exchange(
        &self,
        _request: &RunnerRequest,
        _reply_deadline: Option<SignedDuration>,
    ) -> Result<RunnerReply, ClientError> {
        let _ = (&self.socket, &self.clock);
        Err(ClientError::Absent)
    }
}

#[cfg(unix)]
async fn round_trip(
    stream: &mut tokio::net::UnixStream,
    request: &[u8],
) -> Result<RunnerReply, ClientError> {
    use domovoi_core::skill_runner::{MAX_FRAME_BYTES, decode_payload, frame_len};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    stream
        .write_all(request)
        .await
        .map_err(|err| io_error(&err))?;
    let mut header = [0_u8; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|err| io_error(&err))?;
    let len = frame_len(header, MAX_FRAME_BYTES).map_err(frame_error)?;
    let mut body = Vec::new();
    (&mut *stream)
        .take(len as u64)
        .read_to_end(&mut body)
        .await
        .map_err(|err| io_error(&err))?;
    if body.len() != len {
        return Err(ClientError::Malformed("truncated reply".to_owned()));
    }
    decode_payload(&body).map_err(frame_error)
}

#[cfg(unix)]
fn connect_error(err: std::io::Error) -> ClientError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ClientError::Absent,
        std::io::ErrorKind::PermissionDenied => ClientError::PermissionDenied,
        _ => ClientError::Connect(err.to_string()),
    }
}

#[cfg(unix)]
fn io_error(err: &std::io::Error) -> ClientError {
    match err.kind() {
        std::io::ErrorKind::UnexpectedEof
        | std::io::ErrorKind::BrokenPipe
        | std::io::ErrorKind::ConnectionReset => ClientError::ClosedWithoutReply,
        _ => ClientError::Connect(err.to_string()),
    }
}

fn frame_error(err: FrameError) -> ClientError {
    ClientError::Malformed(err.to_string())
}

fn unexpected(reply: RunnerReply) -> ClientError {
    match reply {
        RunnerReply::Refused(refusal) => ClientError::Refused(refusal),
        _ => ClientError::Malformed("an answer of the wrong kind".to_owned()),
    }
}
