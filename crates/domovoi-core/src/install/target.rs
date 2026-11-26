//! One host's attempt within an install job, and the approval it runs under.

use std::collections::BTreeMap;

use jiff::Timestamp;
use uuid::Uuid;

use super::failure::TargetFailure;
use super::status::TargetStatus;
use crate::action::{ApprovalProvenance, HumanApproval, JournalEpoch, PlanHash};
use crate::id::{AgentSessionId, ExecutionId, HostId, InstallJobId, InstallTargetId};

/// The human approval a target runs under, as stored.
///
/// It is always a human approval: a package job has no policy or model path. The store
/// rebuilds the [`HumanApproval`] with `HumanApproval::restore`; nothing else makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAuthorization {
    approval: HumanApproval,
    provenance: ApprovalProvenance,
}

impl StoredAuthorization {
    /// Pairs an approval with where it was made. A job creator passes what the approving
    /// surface produced; the store passes what it loaded.
    #[must_use]
    pub fn new(approval: HumanApproval, provenance: ApprovalProvenance) -> Self {
        Self {
            approval,
            provenance,
        }
    }

    /// The approval: who, through which channel, and when.
    #[must_use]
    pub fn approval(&self) -> &HumanApproval {
        &self.approval
    }

    /// Where the approval was made: the session reference and the client.
    #[must_use]
    pub fn provenance(&self) -> &ApprovalProvenance {
        &self.provenance
    }
}

/// One host's attempt within a job. A retry is a new target with the next `attempt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    /// Unique id.
    pub id: InstallTargetId,
    /// The job it belongs to.
    pub job: InstallJobId,
    /// The host it runs on.
    pub host: HostId,
    /// 1 for the first attempt, then one more per retry.
    pub attempt: u32,
    /// The idempotency key of this attempt's request to the agent; fresh for every attempt.
    pub execution: ExecutionId,
    /// Where it stands.
    pub status: TargetStatus,
    /// The per-host plan hash the owner approved.
    pub plan_hash: PlanHash,
    /// The approval this attempt runs under.
    pub authorization: StoredAuthorization,
    /// When the approval stops being usable for starting.
    pub expires_at: Timestamp,
    /// The transfer that carried the archive, once relayed.
    pub artifact_transfer: Option<Uuid>,
    /// The transfer that carried the signature file, once relayed.
    pub signature_transfer: Option<Uuid>,
    /// The agent session the request was handed to.
    pub dispatched_session: Option<AgentSessionId>,
    /// The journal epoch of that session.
    pub dispatched_epoch: Option<JournalEpoch>,
    /// Why it did not succeed, when it did not.
    pub failure: Option<TargetFailure>,
    /// What the agent reported on success.
    pub outputs: Option<BTreeMap<String, String>>,
    /// Whether `outputs` came from the agent's own answer (false after a journal-only finish).
    pub outputs_confirmed: bool,
    /// When it was queued.
    pub queued_at: Timestamp,
    /// When the work started.
    pub started_at: Option<Timestamp>,
    /// When it reached a terminal status.
    pub finished_at: Option<Timestamp>,
    /// Row version; every write bumps it.
    pub version: u64,
}
