//! The execution row of an Execution step: what was approved and how far it got.

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::action::{
    ApprovalChannel, ApprovalProvenance, Authorization, AuthorizationKind, HumanApproval,
    JournalEpoch, PlanBinding, PlanHash, PolicyPermit,
};
use crate::capability::ContractId;
use crate::id::{ExecutionId, HostId, OperationId, StepId};
use crate::signal::{CheckKey, TargetKey};
use crate::skill::Risk;

use super::intent::ExecutionIntent;
use super::step::StepOutcome;

/// Why an authorization was refused. The record is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ExecutionError {
    /// A person already approved this execution.
    #[error("this execution was already approved")]
    AlreadyAuthorized,
    /// The approval window has closed.
    #[error("the approval window has closed")]
    Expired,
    /// The approval named a different plan than the one stored.
    #[error("the plan changed since it was shown")]
    PlanChanged,
    /// Only a person can authorize this kind of execution (a package change).
    #[error("only a person can approve this")]
    NotYetSupported,
    /// A policy permit may authorize only a write, or a block that carries a verified safety
    /// identity; this step is neither.
    #[error("a policy permit can authorize only a write or a verified block")]
    PermitNotAllowed,
    /// The stored risk is not read or write, so only a web approval with the typed skill name
    /// is allowed.
    #[error("approve this in the web UI and type the skill's name")]
    NeedsTypedConfirmation,
}

/// How far an execution got, derived from its row and its step's outcome. Never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionState {
    /// Proposed, with no approval step yet.
    Proposed,
    /// Waiting for a person to approve.
    AwaitingApproval,
    /// Approved and not yet sent.
    Authorized,
    /// Sent to the agent; no result yet.
    Dispatched,
    /// Refused before it ran.
    Refused,
    /// Never ran.
    NotRun,
    /// Could not run.
    Unavailable,
    /// Ran and worked.
    Succeeded,
    /// Ran and did not work.
    Failed,
    /// May have run; nobody knows.
    OutcomeUnknown,
    /// Skipped when the operation ended.
    Skipped,
    /// Cancelled with the operation.
    Cancelled,
}

impl ExecutionState {
    /// The snake_case name used in journal payloads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Authorized => "authorized",
            Self::Dispatched => "dispatched",
            Self::Refused => "refused",
            Self::NotRun => "not_run",
            Self::Unavailable => "unavailable",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }
}

/// A person's approval of one execution, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionApproval {
    approval: HumanApproval,
    displayed: PlanHash,
    provenance: ApprovalProvenance,
}

impl ExecutionApproval {
    /// Rebuilds an approval the store persisted. Call only in `crates/domovoi-store/src/`
    /// when loading an execution; it records no new decision.
    #[must_use]
    pub fn restore(
        approval: HumanApproval,
        displayed: PlanHash,
        provenance: ApprovalProvenance,
    ) -> Self {
        Self {
            approval,
            displayed,
            provenance,
        }
    }

    /// Who approved, where and when.
    #[must_use]
    pub fn approval(&self) -> &HumanApproval {
        &self.approval
    }

    /// The plan hash the approving surface displayed.
    #[must_use]
    pub fn displayed(&self) -> PlanHash {
        self.displayed
    }

    /// Where the approval was made.
    #[must_use]
    pub fn provenance(&self) -> &ApprovalProvenance {
        &self.provenance
    }
}

/// The policy permit that authorized one execution, with the decision document it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPermit {
    permit: PolicyPermit,
    decision: String,
}

impl StoredPermit {
    /// Pairs a permit with the text of the enforced decision document.
    #[must_use]
    pub fn new(permit: PolicyPermit, decision: String) -> Self {
        Self { permit, decision }
    }

    /// The permit.
    #[must_use]
    pub fn permit(&self) -> &PolicyPermit {
        &self.permit
    }

    /// The decision id the permit packs.
    #[must_use]
    pub fn decision_id(&self) -> &str {
        self.permit.decision_id()
    }

    /// The enforced decision document, as text.
    #[must_use]
    pub fn decision(&self) -> &str {
        &self.decision
    }
}

/// Everything a store needs to give back to rebuild an [`ExecutionRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionParts {
    /// The operation.
    pub operation: OperationId,
    /// The Execution step.
    pub step: StepId,
    /// The host.
    pub host: HostId,
    /// The contract.
    pub contract: ContractId,
    /// The contract version.
    pub contract_version: u32,
    /// The check.
    pub check: CheckKey,
    /// The target.
    pub target: Option<TargetKey>,
    /// The arguments.
    pub args: BTreeMap<String, String>,
    /// The v2 plan hash.
    pub plan_hash: PlanHash,
    /// When the approval window closes.
    pub expires_at: Timestamp,
    /// The skill's risk when the step was proposed; `None` when unknown.
    pub risk: Option<Risk>,
    /// The hash of the outstanding chat token, if any.
    pub chat_token_hash: Option<[u8; 32]>,
    /// The stored approval.
    pub approval: Option<ExecutionApproval>,
    /// The stored policy permit; at most one of `approval` and `permit` is set.
    pub permit: Option<StoredPermit>,
    /// When the hub began waiting for the agent.
    pub waiting_since: Option<Timestamp>,
    /// The journal epoch and time of dispatch.
    pub dispatched: Option<(JournalEpoch, Timestamp)>,
    /// When the execution settled.
    pub settled_at: Option<Timestamp>,
    /// When the row was created.
    pub created_at: Timestamp,
    /// When the row last changed.
    pub updated_at: Timestamp,
}

/// The execution row of one Execution step. Its id is the step's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRecord {
    /// The idempotency key of the run: always [`ExecutionId::from_step`] of `step`.
    pub execution: ExecutionId,
    /// The operation.
    pub operation: OperationId,
    /// The Execution step.
    pub step: StepId,
    /// The host to run on.
    pub host: HostId,
    /// The contract to run.
    pub contract: ContractId,
    /// The contract version the plan was made against.
    pub contract_version: u32,
    /// The check the run is about.
    pub check: CheckKey,
    /// The target the run is about.
    pub target: Option<TargetKey>,
    /// The arguments.
    pub args: BTreeMap<String, String>,
    /// The v2 plan hash a person approves and dispatch recomputes.
    pub plan_hash: PlanHash,
    /// When the approval window closes.
    pub expires_at: Timestamp,
    risk: Option<Risk>,
    // Boxed to keep the record (and the write enum that carries it) small.
    chat_token_hash: Option<Box<[u8; 32]>>,
    approval: Option<ExecutionApproval>,
    permit: Option<Box<StoredPermit>>,
    /// When the hub began waiting for the agent to come back.
    pub waiting_since: Option<Timestamp>,
    /// The agent journal epoch and time the run was handed off at.
    pub dispatched: Option<(JournalEpoch, Timestamp)>,
    /// When the result was recorded.
    pub settled_at: Option<Timestamp>,
    /// When the row was created.
    pub created_at: Timestamp,
    /// When the row last changed.
    pub updated_at: Timestamp,
}

/// The one contract whose destructive-risk steps a policy permit may authorize.
const PERMIT_DESTRUCTIVE_CONTRACT: &str = "firewall.block_source";

impl ExecutionRecord {
    /// A proposed execution of `intent` as `step` of `operation`, not yet approved.
    #[must_use]
    pub fn propose(
        intent: &ExecutionIntent,
        step: StepId,
        operation: OperationId,
        plan_hash: PlanHash,
        expires_at: Timestamp,
        risk: Risk,
        now: Timestamp,
    ) -> Self {
        Self {
            execution: ExecutionId::from_step(step),
            operation,
            step,
            host: intent.host,
            contract: intent.contract.clone(),
            contract_version: intent.contract_version,
            check: intent.check.clone(),
            target: intent.target.clone(),
            args: intent.args.clone(),
            plan_hash,
            expires_at,
            risk: Some(risk),
            chat_token_hash: None,
            approval: None,
            permit: None,
            waiting_since: None,
            dispatched: None,
            settled_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Records a person's approval, or a policy permit.
    ///
    /// A person's approval is refused, in this order: a channel other than a typed web
    /// confirmation when the stored risk is not read or write; a binding that does not name
    /// the stored plan hash; a second authorization; an expired window.
    ///
    /// A permit (see [`Self::authorize_by_permit`]) is refused in the order its method says.
    /// On any refusal nothing changes.
    pub fn authorize(
        &mut self,
        authorization: Authorization,
        provenance: ApprovalProvenance,
        binding: &PlanBinding,
        now: Timestamp,
    ) -> Result<(), ExecutionError> {
        let Some(human) = authorization.human_approval() else {
            // The writer passes the decision document through `authorize_by_permit`; this
            // path keeps the same checks for a caller that has none to store.
            return self.authorize_by_permit(&authorization, String::new(), now);
        };
        let typed = matches!(
            human.channel(),
            ApprovalChannel::Web {
                typed_confirmation: true
            }
        );
        if !matches!(self.risk, Some(Risk::Read | Risk::Write)) && !typed {
            return Err(ExecutionError::NeedsTypedConfirmation);
        }
        if !binding.matches(&self.plan_hash) {
            return Err(ExecutionError::PlanChanged);
        }
        if self.is_authorized() {
            return Err(ExecutionError::AlreadyAuthorized);
        }
        if now >= self.expires_at {
            return Err(ExecutionError::Expired);
        }
        let displayed = match binding {
            PlanBinding::Full(hash) => *hash,
            PlanBinding::Prefix(_) => self.plan_hash,
        };
        self.approval = Some(ExecutionApproval {
            approval: human.clone(),
            displayed,
            provenance,
        });
        self.updated_at = now;
        Ok(())
    }

    /// Records a policy permit as the authorization, with the decision document it came from.
    ///
    /// Refuses, in this order: a stored risk that is not `write` (the one exception is a
    /// `firewall.block_source` step, which carries `destructive` so a person's approval needs the
    /// typed confirmation, and which a permit may authorize only when it carries a verified
    /// safety identity); a permit that names
    /// another plan hash; a second authorization of either kind; a permit or window that has
    /// closed. On any refusal nothing changes.
    pub fn authorize_by_permit(
        &mut self,
        authorization: &Authorization,
        decision: String,
        now: Timestamp,
    ) -> Result<(), ExecutionError> {
        let Some(permit) = authorization.policy_permit() else {
            return Err(ExecutionError::NotYetSupported);
        };
        if !self.permit_may_authorize(permit.safety_identity().is_some()) {
            return Err(ExecutionError::PermitNotAllowed);
        }
        if permit.plan_hash() != self.plan_hash {
            return Err(ExecutionError::PlanChanged);
        }
        if self.is_authorized() {
            return Err(ExecutionError::AlreadyAuthorized);
        }
        if now >= permit.expires_at() || now >= self.expires_at {
            return Err(ExecutionError::Expired);
        }
        self.permit = Some(Box::new(StoredPermit::new(permit.clone(), decision)));
        self.updated_at = now;
        Ok(())
    }

    /// Whether a permit may authorize this step: any write, or a block of a network source when
    /// the permit names the verified safety identity it was issued against. Every other risk,
    /// destructive included, stays a person's decision.
    fn permit_may_authorize(&self, names_safety_identity: bool) -> bool {
        match self.risk {
            Some(Risk::Write) => true,
            Some(Risk::Destructive) => {
                self.contract.as_str() == PERMIT_DESTRUCTIVE_CONTRACT && names_safety_identity
            }
            _ => false,
        }
    }

    /// Whether a person or a policy permit has authorized this execution.
    #[must_use]
    pub fn is_authorized(&self) -> bool {
        self.approval.is_some() || self.permit.is_some()
    }

    /// Which kind of authorization the record holds, if any.
    #[must_use]
    pub fn authorization_kind(&self) -> Option<AuthorizationKind> {
        match (&self.approval, &self.permit) {
            (Some(_), _) => Some(AuthorizationKind::Human),
            (None, Some(_)) => Some(AuthorizationKind::Policy),
            (None, None) => None,
        }
    }

    /// The stored policy permit, once one authorized this execution.
    #[must_use]
    pub fn permit(&self) -> Option<&StoredPermit> {
        self.permit.as_deref()
    }

    /// The risk of the skill when the step was proposed; `None` when unknown (rows written
    /// before risk was stored). Unknown is treated like destructive wherever it matters.
    #[must_use]
    pub fn risk(&self) -> Option<Risk> {
        self.risk
    }

    /// The hash of the single-use token a chat approval must carry, while one is outstanding.
    #[must_use]
    pub fn chat_token_hash(&self) -> Option<&[u8; 32]> {
        self.chat_token_hash.as_deref()
    }

    /// The record with `hash` as its outstanding chat token.
    #[must_use]
    pub fn with_chat_token_hash(mut self, hash: [u8; 32]) -> Self {
        self.chat_token_hash = Some(Box::new(hash));
        self
    }

    /// Forgets the outstanding chat token.
    pub fn clear_chat_token_hash(&mut self) {
        self.chat_token_hash = None;
    }

    /// The stored approval, once a person gave one.
    #[must_use]
    pub fn approval(&self) -> Option<&ExecutionApproval> {
        self.approval.as_ref()
    }

    /// The state derived from the step's outcome and, while pending, the approval step's.
    #[must_use]
    pub fn state(&self, step: StepOutcome, approval_step: Option<StepOutcome>) -> ExecutionState {
        match step {
            StepOutcome::Succeeded => ExecutionState::Succeeded,
            StepOutcome::Failed => ExecutionState::Failed,
            StepOutcome::Refused => ExecutionState::Refused,
            StepOutcome::NotRun => ExecutionState::NotRun,
            StepOutcome::Unavailable => ExecutionState::Unavailable,
            StepOutcome::OutcomeUnknown => ExecutionState::OutcomeUnknown,
            StepOutcome::Skipped => ExecutionState::Skipped,
            StepOutcome::Cancelled => ExecutionState::Cancelled,
            StepOutcome::Running => ExecutionState::Dispatched,
            StepOutcome::Pending if self.is_authorized() => ExecutionState::Authorized,
            StepOutcome::Pending if approval_step == Some(StepOutcome::Pending) => {
                ExecutionState::AwaitingApproval
            }
            StepOutcome::Pending => ExecutionState::Proposed,
        }
    }

    /// Rebuilds a record from stored parts. Call only in `crates/domovoi-store/src/`.
    #[must_use]
    pub fn restore(parts: ExecutionParts) -> Self {
        Self {
            execution: ExecutionId::from_step(parts.step),
            operation: parts.operation,
            step: parts.step,
            host: parts.host,
            contract: parts.contract,
            contract_version: parts.contract_version,
            check: parts.check,
            target: parts.target,
            args: parts.args,
            plan_hash: parts.plan_hash,
            expires_at: parts.expires_at,
            risk: parts.risk,
            chat_token_hash: parts.chat_token_hash.map(Box::new),
            approval: parts.approval,
            permit: parts.permit.map(Box::new),
            waiting_since: parts.waiting_since,
            dispatched: parts.dispatched,
            settled_at: parts.settled_at,
            created_at: parts.created_at,
            updated_at: parts.updated_at,
        }
    }
}
