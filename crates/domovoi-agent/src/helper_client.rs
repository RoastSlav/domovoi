//! The agent's side of the helper socket: one request per connection, answered from the
//! helper's closed set. This is the only agent module that names the helper socket or builds
//! a helper request.

use std::path::PathBuf;
use std::sync::Arc;

use domovoi_core::attest::Hash32;
use domovoi_core::helper::{
    FrameError, HELPER_PROTOCOL, HELPER_SOCKET, HelperGroup, HelperRequest, HelperRequestFrame,
    HelperResponse, HelperResponseFrame, Nonce, SignedAttestation, UnsignedToken, encode_frame,
};
use domovoi_core::identity::RefusalToken;
use domovoi_core::ports::Clock;
use domovoi_proto::PublicKey;
use domovoi_proto::messages::{HelperIdentity, HelperStatus};
use jiff::SignedDuration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::link::STATUS_PROBE_INTERVAL;
use crate::link::handshake::AgentStatusSource;
use crate::skill_runner::RunnerRoute;
use crate::time::{sleep, within};

/// How long one exchange with the helper may take, measured on the injected clock.
const EXCHANGE_DEADLINE: SignedDuration = SignedDuration::from_secs(10);

/// What the helper reported about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthInfo {
    /// The helper protocol number it speaks.
    pub protocol: u16,
    /// The build that answered.
    pub build: String,
    /// The serving process's id.
    pub pid: u32,
}

/// The operation groups the helper serves and what the platform allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupsInfo {
    /// Groups the helper will serve.
    pub enabled: Vec<HelperGroup>,
    /// Groups named in its configuration but ignored.
    pub ignored: Vec<HelperGroup>,
}

/// Why a helper exchange failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HelperClientError {
    /// The socket path does not exist: no helper is installed.
    #[error("the helper socket does not exist")]
    Absent,
    /// Connecting, writing or reading failed, or the deadline passed.
    #[error("the helper is unreachable: {0}")]
    Unreachable(String),
    /// The answer was not a well-formed frame.
    #[error("the helper sent a malformed frame: {0}")]
    Frame(String),
    /// The helper speaks another protocol number.
    #[error("the helper speaks protocol {0}")]
    Protocol(u16),
    /// The helper refused the request.
    #[error("the helper refused the request: {0}")]
    Refused(RefusalToken),
    /// The helper's start check failed.
    #[error("the helper refused to start: {0}")]
    StartRefused(RefusalToken),
    /// The helper answered something other than what was asked.
    #[error("the helper answered an unexpected message")]
    Unexpected,
    /// The helper could not sign an attestation and said why. The token carries no signature.
    #[error("the helper could not attest: {0}")]
    Unsigned(UnsignedToken),
    /// The helper does not serve the operation: its group is switched off.
    #[error("the helper does not serve this operation")]
    Unavailable,
}

impl HelperClientError {
    /// The `error` code the agent relays to the hub for this failure.
    ///
    /// A token the helper itself gave (a refusal, a start refusal or an unsigned answer) is
    /// relayed as it is; everything else is `helper_unavailable`.
    #[must_use]
    pub fn relay_code(&self) -> &'static str {
        match self {
            Self::Refused(token) | Self::StartRefused(token) => token.as_str(),
            Self::Unsigned(token) => token.as_str(),
            Self::Absent
            | Self::Unreachable(_)
            | Self::Frame(_)
            | Self::Protocol(_)
            | Self::Unexpected
            | Self::Unavailable => HELPER_UNAVAILABLE,
        }
    }
}

/// The relayed code for a helper that cannot be asked or does not serve the operation.
pub const HELPER_UNAVAILABLE: &str = "helper_unavailable";

/// What the request handler holds of the helper socket: it can ask for a signed safety report and
/// nothing else, and it is the only part of the handler that reaches the socket.
#[derive(Debug, Clone)]
pub struct HelperRelay {
    client: HelperClient,
}

impl HelperRelay {
    /// A relay for the socket at `socket`.
    #[must_use]
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            client: HelperClient::new(socket),
        }
    }

    /// A relay for the socket the helper unit listens on.
    #[must_use]
    pub fn default_path() -> Self {
        Self {
            client: HelperClient::default_path(),
        }
    }

    /// The client behind the relay, for the status probe.
    #[must_use]
    pub(crate) fn client(&self) -> &HelperClient {
        &self.client
    }

    /// Asks the helper for a signed safety report bound to `nonce`, returned exactly as the helper
    /// wrote it.
    pub async fn attest(
        &self,
        nonce: Nonce,
        clock: &dyn Clock,
    ) -> Result<SignedAttestation, HelperClientError> {
        self.client.attest(nonce, clock).await
    }
}

/// A client for the helper's local socket.
#[derive(Debug, Clone)]
pub struct HelperClient {
    socket: PathBuf,
}

impl HelperClient {
    /// A client for the socket at `socket`.
    #[must_use]
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    /// A client for the socket the helper unit listens on.
    #[must_use]
    pub fn default_path() -> Self {
        Self::new(HELPER_SOCKET)
    }

    /// Asks whether the helper is alive.
    pub async fn health(&self, clock: &dyn Clock) -> Result<HealthInfo, HelperClientError> {
        match self.exchange(HelperRequest::Health, clock).await? {
            HelperResponse::Health {
                protocol,
                build,
                pid,
            } => Ok(HealthInfo {
                protocol,
                build,
                pid,
            }),
            other => Err(refusal(other)),
        }
    }

    /// Asks which operation groups the helper serves.
    pub async fn groups(&self, clock: &dyn Clock) -> Result<GroupsInfo, HelperClientError> {
        match self.exchange(HelperRequest::Groups, clock).await? {
            HelperResponse::Groups {
                enabled, ignored, ..
            } => Ok(GroupsInfo { enabled, ignored }),
            other => Err(refusal(other)),
        }
    }

    /// Asks the helper for a signed safety report bound to `nonce`.
    ///
    /// The report and the signature are returned exactly as the helper wrote them.
    pub async fn attest(
        &self,
        nonce: Nonce,
        clock: &dyn Clock,
    ) -> Result<SignedAttestation, HelperClientError> {
        match self
            .exchange(HelperRequest::SafetyAttest { nonce }, clock)
            .await?
        {
            HelperResponse::SafetyAttested { attestation } => Ok(attestation),
            HelperResponse::SafetyUnsigned { token } => Err(HelperClientError::Unsigned(token)),
            HelperResponse::Unavailable { .. } => Err(HelperClientError::Unavailable),
            other => Err(refusal(other)),
        }
    }

    /// Asks the helper for its public attestation key.
    pub async fn public_key(&self, clock: &dyn Clock) -> Result<[u8; 32], HelperClientError> {
        match self.exchange(HelperRequest::SafetyPublicKey, clock).await? {
            HelperResponse::SafetyPublicKey { public_key } => public_key
                .parse::<Hash32>()
                .map(|hash| *hash.as_bytes())
                .map_err(|_| HelperClientError::Unexpected),
            HelperResponse::SafetyUnsigned { token } => Err(HelperClientError::Unsigned(token)),
            HelperResponse::Unavailable { .. } => Err(HelperClientError::Unavailable),
            other => Err(refusal(other)),
        }
    }

    async fn exchange(
        &self,
        request: HelperRequest,
        clock: &dyn Clock,
    ) -> Result<HelperResponse, HelperClientError> {
        let frame = HelperRequestFrame {
            v: HELPER_PROTOCOL,
            id: Uuid::now_v7(),
            request,
        };
        let bytes = encode_frame(&frame).map_err(frame_error)?;
        let answer = within(clock, EXCHANGE_DEADLINE, self.round_trip(&bytes))
            .await
            .ok_or_else(|| HelperClientError::Unreachable("timed out".to_owned()))??;
        if answer.v != HELPER_PROTOCOL {
            return Err(HelperClientError::Protocol(answer.v));
        }
        Ok(answer.response)
    }

    #[cfg(unix)]
    async fn round_trip(&self, request: &[u8]) -> Result<HelperResponseFrame, HelperClientError> {
        use domovoi_core::helper::{MAX_RESPONSE_BYTES, decode_response, frame_len};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let io_error = |err: std::io::Error| HelperClientError::Unreachable(err.to_string());
        let mut stream = tokio::net::UnixStream::connect(&self.socket)
            .await
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => HelperClientError::Absent,
                _ => io_error(err),
            })?;
        stream.write_all(request).await.map_err(io_error)?;
        let mut header = [0_u8; 4];
        stream.read_exact(&mut header).await.map_err(io_error)?;
        let len = frame_len(header, MAX_RESPONSE_BYTES).map_err(frame_error)?;
        let mut body = vec![0_u8; len];
        stream.read_exact(&mut body).await.map_err(io_error)?;
        decode_response(&body).map_err(frame_error)
    }

    #[cfg(not(unix))]
    async fn round_trip(&self, _request: &[u8]) -> Result<HelperResponseFrame, HelperClientError> {
        let _ = &self.socket;
        Err(HelperClientError::Absent)
    }
}

fn frame_error(err: FrameError) -> HelperClientError {
    HelperClientError::Frame(err.to_string())
}

fn refusal(response: HelperResponse) -> HelperClientError {
    match response {
        HelperResponse::Refused { token } => HelperClientError::Refused(token),
        HelperResponse::StartRefused { token, .. } => HelperClientError::StartRefused(token),
        _ => HelperClientError::Unexpected,
    }
}

/// What to report to the hub about the helper right now, from a fresh `health` and `groups`
/// exchange. A missing socket is `Absent`; any other failure is an `Error` carrying the
/// helper's token, or `unreachable`, `protocol` or `frame` when it never answered properly.
pub async fn probe_status(client: &HelperClient, clock: &dyn Clock) -> HelperStatus {
    let health = match client.health(clock).await {
        Ok(health) => health,
        Err(err) => return failure(&err),
    };
    if health.protocol != HELPER_PROTOCOL {
        return error_status("protocol");
    }
    match client.groups(clock).await {
        Ok(groups) => HelperStatus::Ok {
            ops: groups
                .enabled
                .iter()
                .map(|group| group_name(*group))
                .collect(),
        },
        Err(err) => failure(&err),
    }
}

fn group_name(group: HelperGroup) -> String {
    group.as_str().to_owned()
}

fn failure(err: &HelperClientError) -> HelperStatus {
    match err {
        HelperClientError::Absent => HelperStatus::Absent,
        HelperClientError::Refused(token) | HelperClientError::StartRefused(token) => {
            error_status(token.as_str())
        }
        HelperClientError::Protocol(_) => error_status("protocol"),
        HelperClientError::Frame(_) | HelperClientError::Unexpected => error_status("frame"),
        HelperClientError::Unreachable(_) => error_status("unreachable"),
        HelperClientError::Unsigned(token) => error_status(token.as_str()),
        HelperClientError::Unavailable => error_status("unavailable"),
    }
}

/// The helper's public key as the agent announces it, when the helper answers.
///
/// Only the public key: the agent sends no fingerprint, and the hub computes its own.
pub async fn probe_identity(client: &HelperClient, clock: &dyn Clock) -> Option<HelperIdentity> {
    client
        .public_key(clock)
        .await
        .ok()
        .map(|key| HelperIdentity {
            public_key: PublicKey(key),
        })
}

fn error_status(reason: &str) -> HelperStatus {
    HelperStatus::Error {
        reason: reason.to_owned(),
    }
}

/// Probes the helper every [`STATUS_PROBE_INTERVAL`] on `clock` and writes the answer into
/// `status` until `stop` is cancelled, so a helper that starts after the agent turns `Absent`
/// into `Ok` within one interval. Waits first: the caller probes once at start itself.
pub async fn keep_status(
    client: HelperClient,
    clock: Arc<dyn Clock>,
    status: AgentStatusSource,
    stop: CancellationToken,
) {
    keep_status_with(client, None, clock, status, stop).await;
}

/// [`keep_status`] that also probes the skill runner in the same loop: every interval, and at
/// once after the route reports a transport failure. The runner's answer goes into the route's
/// shared state and into `status`, next to the helper's, keeping every other field.
pub async fn keep_status_with(
    client: HelperClient,
    runner: Option<Arc<RunnerRoute>>,
    clock: Arc<dyn Clock>,
    status: AgentStatusSource,
    stop: CancellationToken,
) {
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            () = sleep(&*clock, STATUS_PROBE_INTERVAL) => {}
            () = async {
                match &runner {
                    Some(route) => route.state().woken().await,
                    None => std::future::pending().await,
                }
            } => {}
        }
        let helper_status = probe_status(&client, &*clock).await;
        let helper_identity = match helper_status {
            HelperStatus::Ok { .. } => probe_identity(&client, &*clock).await,
            _ => None,
        };
        let runner_status = match &runner {
            Some(route) => Some(route.refresh().await),
            None => None,
        };
        if let Some(mut current) = status.current() {
            let mut changed = current.helper_status != helper_status
                || current.helper_identity != helper_identity;
            current.helper_status = helper_status;
            current.helper_identity = helper_identity;
            if let Some(runner_status) = runner_status
                && current.skill_runner_status != runner_status
            {
                current.skill_runner_status = runner_status;
                changed = true;
            }
            if changed {
                status.set(current);
            }
        }
    }
}
