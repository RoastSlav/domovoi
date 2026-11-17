//! Action requests: a proposal to run a write or destructive skill, and its status.

use std::borrow::Cow;
use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};

use super::approval::{ApprovalProvenance, HumanApproval, PlanHash};
use super::authorization::AuthorizationKind;
use super::epoch::JournalEpoch;
use super::plan::{ImplHash, PlanContract, PlanError, PlanTarget};
use super::stored::{ApprovalEvidence, ApprovalEvidenceError, StoredActionError};
use crate::host::KeyFingerprint;
use crate::id::{ActionId, ExecutionId, HostId, IncidentId, UserId};
use crate::incident::Incident;
use crate::skill::{Risk, SkillName, SkillSummary, ValidatedArgs};

/// Which plan-hash family an action uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanFamily {
    /// The legacy family, kept for an action created before v2 existed.
    V1,
    /// The current family, over the contract, implementation, target, arguments and execution.
    V2,
}

impl PlanFamily {
    /// The stored and API name, `v1` or `v2`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::V1 => "v1",
            Self::V2 => "v2",
        }
    }
}

/// The approval hash and the family it belongs to, selected in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalPlan {
    hash: PlanHash,
    family: PlanFamily,
}

impl ApprovalPlan {
    /// The hash a human approves and the dispatch guard compares.
    #[must_use]
    pub const fn hash(self) -> PlanHash {
        self.hash
    }

    /// Which family the hash belongs to.
    #[must_use]
    pub const fn family(self) -> PlanFamily {
        self.family
    }
}

/// Where an action is in its life. See `ARCHITECTURE.md` section 5.6.
///
/// [`ActionStatus::OutcomeUnknown`] and [`ActionStatus::NotRun`] are terminal: neither is
/// retried, re-dispatched or turned into a success or failure.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionStatus {
    /// Waiting for a human.
    Proposed,
    /// A human approved it.
    Approved,
    /// A human denied it.
    Denied,
    /// Nobody decided before the approval window closed.
    Expired,
    /// Sent to the agent.
    Dispatched {
        /// The agent journal epoch the hub saw when it sent this run; absent for a run
        /// sent before the epoch was recorded, or when it could not be read.
        #[serde(default)]
        agent_epoch: Option<JournalEpoch>,
    },
    /// Re-validation at dispatch failed.
    Rejected {
        /// Why.
        reason: String,
    },
    /// The skill exited successfully.
    Succeeded,
    /// The skill failed.
    Failed {
        /// Why.
        reason: String,
    },
    /// Verification passed.
    Verified,
    /// Verification failed.
    VerifyFailed,
    /// The run may have happened and its result cannot be established. Terminal.
    OutcomeUnknown {
        /// Why.
        reason: String,
    },
    /// Positive evidence that the run never reached the agent. Terminal.
    NotRun {
        /// Why.
        reason: String,
    },
}

impl ActionStatus {
    /// Every status name, as [`ActionStatus::name`] gives them.
    pub const NAMES: [&'static str; 12] = [
        "proposed",
        "approved",
        "denied",
        "expired",
        "dispatched",
        "rejected",
        "succeeded",
        "failed",
        "outcome_unknown",
        "not_run",
        "verified",
        "verify_failed",
    ];

    /// The status without its details, as stored and filtered, for example `proposed` or
    /// `verify_failed`.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Expired => "expired",
            Self::Dispatched { .. } => "dispatched",
            Self::Rejected { .. } => "rejected",
            Self::Succeeded => "succeeded",
            Self::Failed { .. } => "failed",
            Self::OutcomeUnknown { .. } => "outcome_unknown",
            Self::NotRun { .. } => "not_run",
            Self::Verified => "verified",
            Self::VerifyFailed => "verify_failed",
        }
    }
}

/// Who did something. Only `Human` can approve, and only through a [`HumanApproval`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Principal {
    /// An authenticated user.
    Human(UserId),
    /// A Domovoi component, for example `triage`.
    System(Cow<'static, str>),
}

/// Every field of a stored [`ActionRequest`], for the store to load it back.
///
/// An approval can only be supplied as a [`HumanApproval`], so loading cannot approve
/// anything that was not approved before it was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAction {
    /// Unique id.
    pub id: ActionId,
    /// The incident the action is for.
    pub incident: IncidentId,
    /// Where the skill runs.
    pub host: HostId,
    /// Which skill runs.
    pub skill: SkillName,
    /// The skill's risk at proposal time.
    pub risk: Risk,
    /// Validated, normalised arguments.
    pub args: BTreeMap<String, String>,
    /// The plan the approval binds to, in the legacy v1 family.
    pub plan_hash: PlanHash,
    /// The v2 plan identity; `None` for an action created before v2 existed.
    pub plan_hash_v2: Option<PlanHash>,
    /// The exact hash the approval carried, once an approval was given.
    pub approved_plan_hash: Option<PlanHash>,
    /// Where the approval was made, once one was given.
    pub provenance: Option<ApprovalProvenance>,
    /// Which authorization case approved it, once one did.
    pub authorization_kind: Option<AuthorizationKind>,
    /// Status when stored.
    pub status: ActionStatus,
    /// Who proposed it.
    pub proposed_by: Principal,
    /// The approval, if one was given.
    pub approved_by: Option<HumanApproval>,
    /// When the approval window closes.
    pub expires_at: Timestamp,
    /// When it was proposed.
    pub created_at: Timestamp,
    /// When it last changed.
    pub updated_at: Timestamp,
    /// Hash of the chat button token, if one was issued.
    pub chat_token_hash: Option<[u8; 32]>,
}

/// A request to run a skill with specific arguments on a specific host.
///
/// Status changes only through the transition methods (`approve`, `deny`, `expire`,
/// `dispatch`, `complete`, `verified`); there is no way to approve without a
/// [`HumanApproval`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ActionRequest {
    pub(super) id: ActionId,
    pub(super) incident: IncidentId,
    pub(super) host: HostId,
    pub(super) skill: SkillName,
    pub(super) risk: Risk,
    pub(super) args: BTreeMap<String, String>,
    pub(super) plan_hash: PlanHash,
    pub(super) plan_hash_v2: Option<PlanHash>,
    pub(super) approved_plan_hash: Option<PlanHash>,
    pub(super) provenance: Option<ApprovalProvenance>,
    pub(super) authorization_kind: Option<AuthorizationKind>,
    pub(super) status: ActionStatus,
    pub(super) proposed_by: Principal,
    pub(super) approved_by: Option<HumanApproval>,
    pub(super) expires_at: Timestamp,
    pub(super) created_at: Timestamp,
    pub(super) updated_at: Timestamp,
    pub(super) chat_token_hash: Option<[u8; 32]>,
}

impl ActionRequest {
    /// Proposes running `skill` with already validated `args` for `incident`.
    ///
    /// The v2 target is derived from `incident` and `host_key`, so its host, check and
    /// optional target always belong to the linked incident; a caller cannot supply a target
    /// from another one. `host_key` is the host's enrolled agent key; `None` means the host
    /// has no enrolled key, which refuses with [`PlanError::NoEnrolledKey`] rather than
    /// hashing a placeholder. The v1 hash is still computed and kept for the audit line and
    /// the agent's local check.
    ///
    /// The approval window is `ttl` from `now`. If that overflows the timestamp range the
    /// request expires immediately, which fails closed.
    pub fn propose(
        incident: &Incident,
        skill: &SkillSummary,
        args: &ValidatedArgs,
        host_key: Option<KeyFingerprint>,
        proposed_by: Principal,
        ttl: SignedDuration,
        now: Timestamp,
    ) -> Result<Self, PlanError> {
        let host_key = host_key.ok_or(PlanError::NoEnrolledKey)?;
        let target = PlanTarget::of_incident(incident, host_key);
        let host = target.host();
        let id = ActionId::new();
        let plan_hash_v2 = Self::v2_hash_for(id, skill, args, &target)?;
        Ok(Self {
            id,
            incident: incident.id(),
            host,
            skill: skill.name.clone(),
            risk: skill.risk,
            args: args.to_map(),
            plan_hash: PlanHash::compute(&skill.name, skill.manifest_hash, host, args),
            plan_hash_v2: Some(plan_hash_v2),
            approved_plan_hash: None,
            provenance: None,
            authorization_kind: None,
            status: ActionStatus::Proposed,
            proposed_by,
            approved_by: None,
            expires_at: now.checked_add(ttl).unwrap_or(now),
            created_at: now,
            updated_at: now,
            chat_token_hash: None,
        })
    }

    /// Loads a request the store persisted, refusing one whose approval fields contradict its
    /// status or plan.
    ///
    /// A store maps [`StoredActionError`] onto its own corruption error; nothing is repaired
    /// here. This is the only way to turn a [`StoredAction`] into a usable action.
    pub fn try_from_stored(stored: StoredAction) -> Result<Self, StoredActionError> {
        super::stored::validate(&stored)?;
        Ok(Self {
            id: stored.id,
            incident: stored.incident,
            host: stored.host,
            skill: stored.skill,
            risk: stored.risk,
            args: stored.args,
            plan_hash: stored.plan_hash,
            plan_hash_v2: stored.plan_hash_v2,
            approved_plan_hash: stored.approved_plan_hash,
            provenance: stored.provenance,
            authorization_kind: stored.authorization_kind,
            status: stored.status,
            proposed_by: stored.proposed_by,
            approved_by: stored.approved_by,
            expires_at: stored.expires_at,
            created_at: stored.created_at,
            updated_at: stored.updated_at,
            chat_token_hash: stored.chat_token_hash,
        })
    }

    /// The v2 approval hash a proposal with these inputs would have with `id` as its
    /// execution id.
    ///
    /// The execution id is one of v2's five inputs, so a caller asking whether an existing
    /// proposal is still the same plan must pass that action's own id, never a freshly
    /// generated one. The canonical encoding stays in [`PlanHash::compute_v2`].
    pub fn v2_hash_for(
        id: ActionId,
        skill: &SkillSummary,
        args: &ValidatedArgs,
        target: &PlanTarget,
    ) -> Result<PlanHash, PlanError> {
        let contract = PlanContract::skill(&skill.name, 1)?;
        let implementation = ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest);
        Ok(PlanHash::compute_v2(
            &contract,
            implementation,
            target,
            args,
            ExecutionId::from_action(id),
        ))
    }

    /// Checks that the recorded approval evidence covers exactly this action's plan.
    ///
    /// A legacy v1 approval may have no displayed hash and no provenance; a current approval
    /// has both. Either partial form is refused.
    pub fn check_approval_evidence(&self) -> Result<(), ApprovalEvidenceError> {
        let plan = self.approval_plan();
        ApprovalEvidence {
            family: plan.family(),
            plan_hash: plan.hash(),
            approved_plan_hash: self.approved_plan_hash,
            provenance: self.provenance.as_ref(),
        }
        .check()
    }

    /// Stores the hash of the single-use token carried by chat approval buttons.
    #[must_use]
    pub fn with_chat_token_hash(mut self, hash: [u8; 32]) -> Self {
        self.chat_token_hash = Some(hash);
        self
    }

    /// Forgets the chat token hash once the token was used, so it works only once.
    pub fn clear_chat_token_hash(&mut self) {
        self.chat_token_hash = None;
    }

    /// Unique id.
    #[must_use]
    pub fn id(&self) -> ActionId {
        self.id
    }

    /// The incident this action is for.
    #[must_use]
    pub fn incident(&self) -> IncidentId {
        self.incident
    }

    /// Where the skill runs.
    #[must_use]
    pub fn host(&self) -> HostId {
        self.host
    }

    /// Which skill runs.
    #[must_use]
    pub fn skill(&self) -> &SkillName {
        &self.skill
    }

    /// The skill's risk at proposal time.
    #[must_use]
    pub fn risk(&self) -> Risk {
        self.risk
    }

    /// Validated, normalised arguments.
    #[must_use]
    pub fn args(&self) -> &BTreeMap<String, String> {
        &self.args
    }

    /// The plan the approval binds to in the legacy v1 family.
    #[must_use]
    pub fn plan_hash(&self) -> PlanHash {
        self.plan_hash
    }

    /// The v2 plan identity, when this action was proposed after v2 existed.
    #[must_use]
    pub fn plan_hash_v2(&self) -> Option<PlanHash> {
        self.plan_hash_v2
    }

    /// The hash of the action's own family: v2 when present, else the legacy v1 hash.
    ///
    /// Use [`ActionRequest::approval_plan`] when the family is also needed; this is the
    /// single source of truth for both.
    #[must_use]
    pub fn plan_hash_for_approval(&self) -> PlanHash {
        self.approval_plan().hash()
    }

    /// The approval hash and its family, selected in one place.
    #[must_use]
    pub fn approval_plan(&self) -> ApprovalPlan {
        match self.plan_hash_v2 {
            Some(hash) => ApprovalPlan {
                hash,
                family: PlanFamily::V2,
            },
            None => ApprovalPlan {
                hash: self.plan_hash,
                family: PlanFamily::V1,
            },
        }
    }

    /// The exact hash the approval carried, once one was given.
    #[must_use]
    pub fn approved_plan_hash(&self) -> Option<PlanHash> {
        self.approved_plan_hash
    }

    /// Where the approval was made, once one was given.
    #[must_use]
    pub fn provenance(&self) -> Option<&ApprovalProvenance> {
        self.provenance.as_ref()
    }

    /// Which authorization case approved it, once one did.
    #[must_use]
    pub fn authorization_kind(&self) -> Option<AuthorizationKind> {
        self.authorization_kind
    }

    /// Current status.
    #[must_use]
    pub fn status(&self) -> &ActionStatus {
        &self.status
    }

    /// Who proposed it.
    #[must_use]
    pub fn proposed_by(&self) -> &Principal {
        &self.proposed_by
    }

    /// The approval, once given.
    #[must_use]
    pub fn approved_by(&self) -> Option<&HumanApproval> {
        self.approved_by.as_ref()
    }

    /// When the approval window closes.
    #[must_use]
    pub fn expires_at(&self) -> Timestamp {
        self.expires_at
    }

    /// When it was proposed.
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    /// When it last changed.
    #[must_use]
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }

    /// Hash of the chat button token, if one was issued.
    #[must_use]
    pub fn chat_token_hash(&self) -> Option<&[u8; 32]> {
        self.chat_token_hash.as_ref()
    }
}
