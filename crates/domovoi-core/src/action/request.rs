//! Action requests: a proposal to run a write or destructive skill, and its status.

use std::borrow::Cow;
use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};

use super::approval::{HumanApproval, PlanHash};
use crate::id::{ActionId, HostId, IncidentId, UserId};
use crate::skill::{Risk, SkillName, SkillSummary, ValidatedArgs};

/// Where an action is in its life. See `ARCHITECTURE.md` section 5.6.
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
    Dispatched,
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
    /// The plan the approval binds to.
    pub plan_hash: PlanHash,
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
    pub(super) status: ActionStatus,
    pub(super) proposed_by: Principal,
    pub(super) approved_by: Option<HumanApproval>,
    pub(super) expires_at: Timestamp,
    pub(super) created_at: Timestamp,
    pub(super) updated_at: Timestamp,
    pub(super) chat_token_hash: Option<[u8; 32]>,
}

impl ActionRequest {
    /// Proposes running `skill` with already validated `args` on `host`.
    ///
    /// The approval window is `ttl` from `now`. If that overflows the timestamp range the
    /// request expires immediately, which fails closed.
    #[must_use]
    pub fn propose(
        incident: IncidentId,
        host: HostId,
        skill: &SkillSummary,
        args: &ValidatedArgs,
        proposed_by: Principal,
        ttl: SignedDuration,
        now: Timestamp,
    ) -> Self {
        Self {
            id: ActionId::new(),
            incident,
            host,
            skill: skill.name.clone(),
            risk: skill.risk,
            args: args.to_map(),
            plan_hash: PlanHash::compute(&skill.name, skill.manifest_hash, host, args),
            status: ActionStatus::Proposed,
            proposed_by,
            approved_by: None,
            expires_at: now.checked_add(ttl).unwrap_or(now),
            created_at: now,
            updated_at: now,
            chat_token_hash: None,
        }
    }

    /// Loads a request the store persisted, unchanged.
    #[must_use]
    pub fn from_stored(stored: StoredAction) -> Self {
        Self {
            id: stored.id,
            incident: stored.incident,
            host: stored.host,
            skill: stored.skill,
            risk: stored.risk,
            args: stored.args,
            plan_hash: stored.plan_hash,
            status: stored.status,
            proposed_by: stored.proposed_by,
            approved_by: stored.approved_by,
            expires_at: stored.expires_at,
            created_at: stored.created_at,
            updated_at: stored.updated_at,
            chat_token_hash: stored.chat_token_hash,
        }
    }

    /// Stores the hash of the single-use token carried by chat approval buttons.
    #[must_use]
    pub fn with_chat_token_hash(mut self, hash: [u8; 32]) -> Self {
        self.chat_token_hash = Some(hash);
        self
    }

    /// Replaces the generated id, for rehydrating stored requests and fixed test ids.
    #[must_use]
    pub fn with_id(mut self, id: ActionId) -> Self {
        self.id = id;
        self
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

    /// The plan the approval binds to.
    #[must_use]
    pub fn plan_hash(&self) -> PlanHash {
        self.plan_hash
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
