//! The phases of one block, and the only file that hands a containment request to an agent.
//!
//! `execute_containment` is a sibling of the unit restart's dispatch, not a second framework:
//! it takes a step that carries a person's approval or a stored permit, runs `apply`, then the
//! hub's own `confirm` inside the window, and takes the failure paths of ADR 0038. Every phase
//! change is committed before the request that causes it is sent, and nothing is retried after a
//! hand-off.

use domovoi_core::action::AuthorizationKind;
use domovoi_core::capability::containment::BLOCK_SOURCE_ID;
use domovoi_core::containment::{ContainDispatch, ContainOutputs, Net, Token, parse_message};
use domovoi_core::ports::{
    AuditDraft, BlockPhase, BlockRow, ContainmentWrite, LinkError, StateWrite, Transaction,
    phase_journal,
};
use domovoi_core::{AgentSessionId, HostId, JournalEpoch, PlanHash, StepOutcome};
use jiff::{SignedDuration, Timestamp};

use super::input::Assessment;
use super::{ContainDeps, PHASE_TIMEOUT, audit};
use crate::operations::{AuthorizedStep, ExecuteError};

/// The answers an agent gives before it accepts a request, which prove nothing reached the kernel.
const REFUSED_FRAMES: [&str; 10] = [
    "ceiling_exceeded",
    "rate_limited",
    "allow_risk_denied",
    "authorization_expired",
    "plan_hash_mismatch",
    "contract_unknown",
    "contract_version_unsupported",
    "journal_corrupt",
    "policy_requires_verified_safety",
    "bad_request",
];
/// The longest reason kept for an answer nobody recognised.
pub(super) const UNRECOGNISED_MAX: usize = 500;

/// An Execution step of `firewall.block_source` that a person approved or a permit authorized,
/// as the one thing `execute_containment` may run. Built once, by the dispatch handler.
#[derive(Debug)]
pub struct AuthorizedContainment {
    step: AuthorizedStep,
}

impl AuthorizedContainment {
    /// The one constructor: it takes a step that already carries an authorization and refuses
    /// any other contract.
    pub(crate) fn of(step: AuthorizedStep) -> Result<Self, ExecuteError> {
        if step.execution().contract.as_str() != BLOCK_SOURCE_ID {
            return Err(ExecuteError::Refused(
                "the step is not a containment step".to_owned(),
            ));
        }
        Ok(Self { step })
    }

    /// A step for a test that drives the phases without the approval machinery in front.
    #[cfg(test)]
    pub(crate) fn for_test(execution: domovoi_core::ExecutionRecord) -> Self {
        Self {
            step: AuthorizedStep::for_test(execution),
        }
    }

    /// The execution as it will be run.
    #[must_use]
    pub fn execution(&self) -> &domovoi_core::ExecutionRecord {
        self.step.execution()
    }
}

/// What the dispatch handler worked out, fresh, before the writer committed the dispatch.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The network to block.
    pub net: Net,
    /// The TTL the block has once confirmed.
    pub ttl: SignedDuration,
    /// The one plan hash of all three phases.
    pub plan_hash: PlanHash,
    /// How the helper's safety set stands.
    pub assessment: Assessment,
    /// Who authorized.
    pub authorization: AuthorizationKind,
    /// When the authorization stops being valid.
    pub expires_at: Timestamp,
}

/// How a block's run ends for its step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainEnd {
    /// The step ends with this outcome, reason and output.
    Settled {
        /// The step's outcome.
        outcome: StepOutcome,
        /// Why, for anything but a clean success.
        reason: Option<String>,
        /// What to show the person.
        output: Option<String>,
    },
    /// Nothing proves either result: the step stays dispatched and a reconcile settles it.
    Unknown,
}

pub(super) fn settled(
    outcome: StepOutcome,
    reason: Option<String>,
    output: Option<String>,
) -> ContainEnd {
    ContainEnd::Settled {
        outcome,
        reason,
        output,
    }
}

/// What came back for one phase.
pub(super) enum Answer {
    /// The agent said it succeeded.
    Done(ContainOutputs),
    /// The agent refused or failed, as `"<token>: <detail>"`.
    Rejected(String),
    /// Nothing usable came back after the hand-off.
    Unknown,
    /// The request never entered the session's queue.
    NotHandedOver,
}

/// Whether a rejection proves nothing reached the kernel.
pub(super) fn is_refusal(message: &str) -> bool {
    use Token::{
        BlockMismatch, ContainmentBlocked, DrillsNotPassed, ExclusionOverlap, ExclusionsIncomplete,
        HelperUnavailable, InvalidTarget, JournalCorrupt, MalformedSafety, MappedForm,
        MaxActiveReached, MutationLimit, NoManagementCidrs, PrefixTooWide,
        PrefixUnsupportedPlatform, SafetyChanged, SafetyUnknown, TableDrift, TtlAboveMaximum,
        TtlTooShort, UnknownBlock,
    };
    let head = message.split(": ").next().unwrap_or_default();
    if REFUSED_FRAMES.contains(&head) {
        return true;
    }
    matches!(
        parse_message(message).map(|(token, _)| token),
        Some(
            MalformedSafety
                | SafetyChanged
                | SafetyUnknown
                | ContainmentBlocked
                | PrefixUnsupportedPlatform
                | InvalidTarget
                | MappedForm
                | PrefixTooWide
                | ExclusionOverlap
                | ExclusionsIncomplete
                | NoManagementCidrs
                | TtlTooShort
                | TtlAboveMaximum
                | MaxActiveReached
                | MutationLimit
                | UnknownBlock
                | BlockMismatch
                | TableDrift
                | JournalCorrupt
                | DrillsNotPassed
                | HelperUnavailable
        )
    )
}

pub(super) fn token_of(message: &str) -> Option<Token> {
    parse_message(message).map(|(token, _)| token)
}

pub(super) fn token_text(message: &str) -> &str {
    message.split(": ").next().unwrap_or(message)
}

/// The assurance as stored on the block row.
fn assurance_json(assessment: &Assessment) -> String {
    use domovoi_core::action::PlanAssurance;
    match &assessment.assurance {
        PlanAssurance::Verified(identity) => serde_json::json!({
            "kind": "verified",
            "identity": identity.canonical(),
        }),
        PlanAssurance::Unverified(reason) => serde_json::json!({
            "kind": "unverified",
            "reason": reason.token(),
            "unprotected_paths": assessment
                .unprotected
                .iter()
                .map(|cidr| format!("{}/{}", cidr.addr, cidr.prefix))
                .collect::<Vec<_>>(),
        }),
    }
    .to_string()
}

/// Runs one authorized block: `apply`, the hub's `confirm` inside the window, and the failure
/// paths. The dispatch of the step is already committed; `session` and `epoch` are the ones it
/// was committed against.
pub async fn execute_containment(
    deps: &ContainDeps,
    authorized: &AuthorizedContainment,
    prepared: &Prepared,
    session: AgentSessionId,
    epoch: JournalEpoch,
) -> ContainEnd {
    let row = authorized.execution();
    let now = deps.clock.now();
    let block = BlockRow {
        block_id: row.execution,
        host_id: row.host,
        operation_id: row.operation,
        step_id: row.step,
        source: prepared.net.canonical(),
        ttl_s: u32::try_from(prepared.ttl.as_secs()).unwrap_or(u32::MAX),
        plan_hash: prepared.plan_hash.to_string(),
        phase: BlockPhase::Applying,
        authorization_kind: match prepared.authorization {
            AuthorizationKind::Human => "human".to_owned(),
            AuthorizationKind::Policy => "policy".to_owned(),
        },
        assurance: assurance_json(&prepared.assessment),
        confirm_execution: None,
        rollback_execution: None,
        applied_at: None,
        deadline: None,
        existing_flows: None,
        bridge_gap: None,
        failure_token: None,
        failure_message: None,
        created_at: now,
        updated_at: now,
        version: 1,
    };
    // The block row, with its `applying` journal and audit rows, is committed before the frame.
    let mut audits: Vec<AuditDraft> = vec![audit::draft(
        audit::REQUESTED,
        &block,
        audit::system(),
        now,
        None,
    )];
    if !prepared.assessment.is_verified() && prepared.authorization == AuthorizationKind::Human {
        audits.push(audit::draft(
            audit::UNVERIFIED_APPROVED,
            &block,
            audit::system(),
            now,
            prepared.assessment.reason().map(|r| r.token()),
        ));
    }
    audits.push(audit::draft(
        audit::APPLYING,
        &block,
        audit::system(),
        now,
        None,
    ));
    let mut tx = Transaction::new()
        .write(StateWrite::Containment(ContainmentWrite::Create(
            block.clone(),
        )))
        .journal(phase_journal(
            &block,
            BlockPhase::Queued,
            BlockPhase::Applying,
            None,
            audit::system(),
            now,
        ));
    for draft in audits {
        tx = tx.audit(draft);
    }
    if let Err(err) = deps.uow.commit(tx).await {
        tracing::warn!(%err, "could not record a containment block; nothing was sent");
        return settled(
            StepOutcome::NotRun,
            Some("hub_not_handed_off".to_owned()),
            None,
        );
    }
    let mut run = super::run::Run {
        deps,
        host: row.host,
        block,
        net: prepared.net,
        ttl: prepared.ttl,
        plan: prepared.plan_hash,
        authorization: prepared.authorization,
        expires_at: prepared.expires_at,
        expected: prepared.assessment.expected.clone(),
    };
    run.run(session, epoch).await
}

/// Hands one request to the host's session and waits for its answer: the one place a containment
/// request reaches the relay.
pub(super) async fn send_raw(
    deps: &ContainDeps,
    host: HostId,
    request: ContainDispatch,
    session: AgentSessionId,
    epoch: JournalEpoch,
) -> Answer {
    let now = deps.clock.now();
    let deadline = now
        .checked_add(PHASE_TIMEOUT)
        .and_then(|t| t.checked_add(deps.result_grace))
        .unwrap_or(now);
    let relay = &deps.caps.relay;
    let handoff = match relay
        .dispatch_phase(host, request, session, epoch, deadline)
        .await
    {
        Ok(handoff) => handoff,
        Err(err) => {
            tracing::warn!(%host, %err, "the hub never handed a containment request to the agent");
            return Answer::NotHandedOver;
        }
    };
    match relay.phase_result(host, handoff, deadline).await {
        Ok(outputs) => Answer::Done(outputs),
        Err(LinkError::Rejected(message)) => Answer::Rejected(message),
        Err(_) => Answer::Unknown,
    }
}
