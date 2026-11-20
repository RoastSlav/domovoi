//! Operation commands every transport shares: approve and deny an Execution step, and
//! cancel, pause, resume and answer a question of an operation. The operation writer is the
//! only thing that changes an operation; this layer decides who may ask and what is sent.

mod controls;
mod limits;
mod steps;
mod summary;

use std::sync::Arc;

use domovoi_core::ports::{Clock, OperationRepo, Redactor};
use domovoi_core::{ActionId, ExecutionId, IncidentId, OperationId, Risk, StepId};
use domovoi_engine::operations::{OperationError, OperationHandle};

use crate::agent_server::AgentAdmin;

pub use limits::{
    ANSWER_MAX, CANCEL_REASON_MAX, DEFAULT_CANCEL_REASON, EXTEND_MAX_COUNT, EXTEND_MAX_ENTRIES,
    EXTEND_MAX_MEASURED,
};
pub use steps::risk_of_skill;
pub use summary::SummaryCommandError;

/// A limit to raise when resuming: the new value of one budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetExtension {
    /// The budget's snake_case name, for example `executions` or `wall_time`.
    pub kind: String,
    /// The new limit: seconds for `wall_time`, micro-units for `cost`, a count otherwise.
    pub limit: u64,
}

/// Why an operation command was refused.
#[derive(Debug)]
pub enum OperationCommandError {
    /// This hub runs no operation service.
    NotEnabled,
    /// The input is not acceptable; the text is shown to the person.
    Invalid(String),
    /// The skill's name must be typed to approve it.
    TypedConfirmationRequired,
    /// Chat tried to approve a destructive or unknown-risk skill; only the web may.
    WebOnly,
    /// The operation is in a state that cannot take the command.
    Conflict(String),
    /// No such operation or step.
    NotFound,
    /// What the operation writer answered.
    Operation(OperationError),
}

/// What a chat button needs to know about the step behind an action id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRef {
    /// The operation.
    pub operation: OperationId,
    /// The Execution step.
    pub step: StepId,
    /// The incidents the operation is about.
    pub incidents: Vec<IncidentId>,
    /// The skill's name, or the contract's id when it is not a skill.
    pub skill: String,
    /// The risk stored with the execution; `None` when unknown.
    pub risk: Option<Risk>,
}

/// The operation commands.
#[derive(Clone)]
pub struct OperationCommands {
    pub(super) handle: Option<OperationHandle>,
    pub(super) repo: Arc<dyn OperationRepo>,
    pub(super) agents: Arc<dyn AgentAdmin>,
    pub(super) redactor: Arc<dyn Redactor>,
    pub(super) clock: Arc<dyn Clock>,
}

impl OperationCommands {
    /// Whether this hub runs an operation service.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.handle.is_some()
    }

    fn handle(&self) -> Result<&OperationHandle, OperationCommandError> {
        self.handle
            .as_ref()
            .ok_or(OperationCommandError::NotEnabled)
    }

    /// The step whose execution id equals `action`, or `None` for a legacy action. Reads
    /// the stored operations whether or not the service runs.
    pub async fn find_by_action(
        &self,
        action: ActionId,
    ) -> Result<Option<StepRef>, OperationCommandError> {
        let store = |e: domovoi_core::ports::StoreError| {
            OperationCommandError::Operation(OperationError::Store(e.to_string()))
        };
        let Some(row) = self
            .repo
            .execution(ExecutionId::from_action(action))
            .await
            .map_err(store)?
        else {
            return Ok(None);
        };
        let record = row.value;
        let snapshot = self.repo.load(record.operation).await.map_err(store)?;
        Ok(Some(StepRef {
            operation: record.operation,
            step: record.step,
            incidents: snapshot.map(|s| s.incidents).unwrap_or_default(),
            skill: record
                .contract
                .skill_name()
                .map_or_else(|| record.contract.to_string(), |name| name.to_string()),
            risk: record.risk(),
        }))
    }
}
