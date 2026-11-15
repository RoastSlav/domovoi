//! The authentication handshake: `hello`, `challenge`, signed response, `welcome`.

use std::sync::{Arc, Mutex};

use domovoi_core::SkillSummary;
use domovoi_proto::messages::{
    AgentSettings, AgentStatus, ChallengeResponse, CheckInfo, Hello, HelperIdentity, HelperStatus,
    HostFacts, RejectReason, SkillRunnerStatus, Welcome,
};
use domovoi_proto::{Message, PROTOCOL_VERSION, Sha256Bytes, Transport, challenge_message};
use jiff::SignedDuration;
use uuid::Uuid;

use super::frame;
use crate::error::LinkError;
use crate::identity::Identity;

/// Values the agent adds to `hello` beyond facts and catalogues.
#[derive(Debug, Clone)]
pub struct HelloExtensions {
    /// Protocol features the agent implements; empty until one exists.
    pub features: Vec<String>,
    /// The action journal's epoch.
    pub journal_epoch: Uuid,
    /// The capability catalogue's hash; `None` until it exists.
    pub catalog_hash: Option<Sha256Bytes>,
    /// The unattended ceiling's hash; `None` until it exists.
    pub ceiling_hash: Option<Sha256Bytes>,
    /// A short description of the unattended ceiling; `None` until it exists.
    pub ceiling_summary: Option<String>,
    /// What the skill runner reports about itself.
    pub skill_runner_status: SkillRunnerStatus,
    /// What the helper reports about itself.
    pub helper_status: HelperStatus,
    /// Whether the agent's effective uid is 0.
    pub runs_as_root: bool,
    /// The helper's public key when one is present.
    pub helper_identity: Option<HelperIdentity>,
}

impl Default for HelloExtensions {
    fn default() -> Self {
        Self {
            features: Vec::new(),
            journal_epoch: Uuid::nil(),
            catalog_hash: None,
            ceiling_hash: None,
            ceiling_summary: None,
            skill_runner_status: SkillRunnerStatus::Absent,
            helper_status: HelperStatus::Absent,
            runs_as_root: false,
            helper_identity: None,
        }
    }
}

impl HelloExtensions {
    /// The runtime status these extensions describe.
    #[must_use]
    pub fn status(&self) -> AgentStatus {
        AgentStatus {
            helper_status: self.helper_status.clone(),
            helper_identity: self.helper_identity.clone(),
            skill_runner_status: self.skill_runner_status.clone(),
            runs_as_root: self.runs_as_root,
            ceiling_hash: self.ceiling_hash,
            ceiling_summary: self.ceiling_summary.clone(),
        }
    }
}

/// The values an agent reports in `agent_status`, shared so a probe can change them.
#[derive(Clone)]
pub struct AgentStatusSource {
    current: Arc<Mutex<AgentStatus>>,
}

impl AgentStatusSource {
    /// A source that always reports `status` until changed.
    #[must_use]
    pub fn new(status: AgentStatus) -> Self {
        Self {
            current: Arc::new(Mutex::new(status)),
        }
    }

    /// The values to report now, or `None` when the lock is poisoned and the true values
    /// are unknown. Never invents a status.
    #[must_use]
    pub fn current(&self) -> Option<AgentStatus> {
        self.current.lock().ok().map(|status| status.clone())
    }

    /// Poisons the status mutex so a test can prove no status is fabricated. Test-only.
    #[cfg(test)]
    pub fn poison(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.current.lock().expect("the status lock is healthy");
            panic!("poison the status lock");
        }));
    }

    /// Replaces the reported values; the next probe sends them if they differ.
    pub fn set(&self, status: AgentStatus) {
        if let Ok(mut current) = self.current.lock() {
            *current = status;
        }
    }
}

impl Default for AgentStatusSource {
    fn default() -> Self {
        Self::new(absent_status())
    }
}

fn absent_status() -> AgentStatus {
    AgentStatus {
        helper_status: HelperStatus::Absent,
        helper_identity: None,
        skill_runner_status: SkillRunnerStatus::Absent,
        runs_as_root: false,
        ceiling_hash: None,
        ceiling_summary: None,
    }
}

/// How long each handshake step may take.
pub const STEP_TIMEOUT: SignedDuration = SignedDuration::from_secs(30);

/// What the agent announces in `hello`.
pub struct Announce<'a> {
    /// Host facts.
    pub facts: HostFacts,
    /// Summaries of the skills in the local catalogue.
    pub skills: &'a [SkillSummary],
    /// Every check with its local settings.
    pub checks: &'a [CheckInfo],
    /// What was found out about the container runtime.
    pub docker: Option<&'a domovoi_proto::messages::DockerReport>,
    /// The rest of the 1.2 `hello` extensions.
    pub extensions: &'a HelloExtensions,
}

/// Runs the handshake on a fresh connection and returns the hub's `welcome`.
///
/// The challenge signature covers `session_hint` (the TLS channel binding), so it cannot be
/// replayed on another connection.
pub async fn handshake(
    transport: &mut dyn Transport,
    identity: &Identity,
    session_hint: &str,
    announce: Announce<'_>,
) -> Result<Welcome, LinkError> {
    if announce.extensions.journal_epoch.is_nil() {
        // A running protocol 1.2 agent always owns a journal with a real epoch. The
        // `Default` value exists for tests and decoding, never for an outbound handshake.
        return Err(LinkError::Protocol(
            "the agent has no journal epoch to announce".to_owned(),
        ));
    }
    let release = env!("CARGO_PKG_VERSION");
    let hello = Hello {
        agent_id: identity.agent_id,
        protocol: PROTOCOL_VERSION,
        agent_version: release.to_owned(),
        facts: Box::new(announce.facts),
        capabilities: Vec::new(),
        features: announce.extensions.features.clone(),
        journal_epoch: announce.extensions.journal_epoch,
        catalog_hash: announce.extensions.catalog_hash,
        ceiling_hash: announce.extensions.ceiling_hash,
        ceiling_summary: announce.extensions.ceiling_summary.clone(),
        skill_runner_status: announce.extensions.skill_runner_status.clone(),
        helper_status: announce.extensions.helper_status.clone(),
        runs_as_root: announce.extensions.runs_as_root,
        helper_identity: announce.extensions.helper_identity.clone(),
        skills: announce.skills.to_vec(),
        checks: announce.checks.to_vec(),
        docker: announce.docker.cloned(),
    };
    frame::send(transport, &Message::Hello(hello), None).await?;
    let challenge = match frame::expect(transport).await?.message {
        Message::Challenge(challenge) => challenge,
        other => return Err(refused(other, release)),
    };
    let signed = challenge_message(&challenge.nonce, identity.agent_id, session_hint);
    let response = ChallengeResponse {
        signature: identity.sign(&signed),
    };
    frame::send(transport, &Message::ChallengeResponse(response), None).await?;
    match frame::expect(transport).await?.message {
        Message::Welcome(welcome) => Ok(welcome),
        other => Err(refused(other, release)),
    }
}

fn refused(message: Message, release: &str) -> LinkError {
    match message {
        Message::Reject(reject) => match reject.reason {
            RejectReason::Revoked => LinkError::Revoked("revoked"),
            RejectReason::UnknownAgent => LinkError::Revoked("unknown agent"),
            RejectReason::Incompatible { hub } => LinkError::Incompatible {
                hub,
                agent: PROTOCOL_VERSION,
            },
            RejectReason::AgentTooOld { minimum } => LinkError::AgentTooOld {
                agent_version: release.to_owned(),
                minimum,
            },
            other => LinkError::Protocol(format!("rejected: {other:?}")),
        },
        other => LinkError::Protocol(format!("unexpected {} during handshake", other.kind())),
    }
}

/// The heartbeat interval from the hub's settings; zero means the default of 10 s.
#[must_use]
pub fn heartbeat(settings: &AgentSettings) -> SignedDuration {
    match settings.heartbeat_seconds {
        0 => SignedDuration::from_secs(10),
        secs => SignedDuration::from_secs(i64::from(secs)),
    }
}

#[cfg(test)]
mod tests {
    use super::AgentStatusSource;

    #[test]
    fn a_poisoned_status_source_never_fabricates_a_status() {
        let source = AgentStatusSource::default();
        assert!(source.current().is_some());
        source.poison();
        assert!(
            source.current().is_none(),
            "an unreadable status is unknown, not absent"
        );
    }
}
