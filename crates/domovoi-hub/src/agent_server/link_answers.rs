//! How the agent's journal answers become the engine's [`ActionAnswers`].

use domovoi_core::ContractId;
use domovoi_core::ExecutionId;
use domovoi_core::JournalEpoch;
use domovoi_core::capability::{CatalogImplementation, ContractVersion};
use domovoi_core::ports::{ActionAnswer, ActionAnswerEntry, ActionAnswers, UnknownReason};
use domovoi_proto::messages::{
    ActionState, ActionStatusAnswers, CapabilityOutcome, CatalogEntry, OutcomeUnknownReason,
};

/// The engine's view of the journal's answers, mapped against what the answering session
/// promised.
///
/// `not_found` is positive evidence only when the agent records every action execution in
/// its journal. An older agent, or one that still serves an unjournaled execution path,
/// can run a request and still have no record of it, so its `not_found` is reported as an
/// unknowable outcome instead.
pub(super) fn answers_from(
    reply: ActionStatusAnswers,
    authoritative_absence: bool,
) -> ActionAnswers {
    ActionAnswers {
        epoch: JournalEpoch::from_uuid(reply.epoch),
        answers: reply
            .answers
            .into_iter()
            .map(|answer| ActionAnswerEntry {
                execution: ExecutionId::from_uuid(answer.execution_id),
                answer: answer_from(answer.state, authoritative_absence),
            })
            .collect(),
    }
}

/// What one journal state means to the engine. A finished run keeps whether it succeeded and
/// the agent's message; the output itself is not needed to settle an action.
fn answer_from(state: ActionState, authoritative_absence: bool) -> ActionAnswer {
    match state {
        ActionState::Finished { result } => match result.outcome {
            CapabilityOutcome::Succeeded { .. } => ActionAnswer::Finished {
                succeeded: true,
                detail: None,
            },
            // Keep the failure class as it came off the wire, so a code this build does not
            // know is reported rather than flattened into a generic failure.
            CapabilityOutcome::Failed { code, message } => ActionAnswer::Finished {
                succeeded: false,
                detail: Some(format!("{}: {message}", code.raw())),
            },
        },
        ActionState::Running => ActionAnswer::Running,
        ActionState::NotFound if authoritative_absence => ActionAnswer::NotFound,
        ActionState::NotFound => {
            tracing::warn!(
                "the agent has no record of a run but does not promise that every execution is journaled; \
                 its absence is not proof the run never happened",
            );
            ActionAnswer::Unknown {
                reason: UnknownReason::JournalLost,
            }
        }
        ActionState::OutcomeUnknown { reason } => ActionAnswer::Unknown {
            reason: unknown_reason(&reason),
        },
    }
}

/// Why the journal could not establish an outcome. A reason this build does not know is
/// reported as a lost record: the run may have happened, so nothing is assumed.
fn unknown_reason(reason: &domovoi_proto::Open<OutcomeUnknownReason>) -> UnknownReason {
    match reason.known() {
        Some(OutcomeUnknownReason::NoStartRecord) => UnknownReason::NoStartRecord,
        Some(OutcomeUnknownReason::AgentRestarted) => UnknownReason::AgentRestarted,
        Some(OutcomeUnknownReason::JournalLost) => UnknownReason::JournalLost,
        None => {
            tracing::warn!(
                reason = reason.raw(),
                "the agent reported an unknown outcome reason; treating the record as lost",
            );
            UnknownReason::JournalLost
        }
    }
}

/// The largest page the wire and the registry ask for.
pub(super) const MAX_CATALOG_PAGE: u32 = 100;

/// One wire entry as the core type, or why its id or version is unreadable.
///
/// The descriptor is ignored on purpose: it is opaque at the wire layer, and deriving a
/// contract from it would let an agent define its own authority (RW-07).
pub(super) fn implementation_of(entry: CatalogEntry) -> Result<CatalogImplementation, String> {
    let contract = ContractId::parse(&entry.contract)
        .map_err(|err| format!("{:?}: {}", entry.contract, err))?;
    let version = ContractVersion::new(entry.version)
        .map_err(|err| format!("{}: {}", entry.contract, err))?;
    Ok(CatalogImplementation { contract, version })
}

/// A reply of a kind the request cannot have had.
pub(super) fn unexpected(message: &domovoi_proto::Message) -> domovoi_core::ports::LinkError {
    domovoi_core::ports::LinkError::Rejected(format!("the agent answered with {}", message.kind()))
}
