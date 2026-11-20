//! The current value of a finished operation's summary.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::id::{HostId, OperationId};
use crate::signal::{CheckKey, TargetKey};

use super::field::{SummaryField, SummaryRevision, SummarySource};

/// How a finished operation ended: the terminal status names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOutcome {
    /// The problem was solved.
    Succeeded,
    /// It was not solved.
    Failed,
    /// It was handed to a person.
    Escalated,
    /// A person stopped it.
    Cancelled,
}

impl ResolutionOutcome {
    /// The stored snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Escalated => "escalated",
            Self::Cancelled => "cancelled",
        }
    }

    /// Reads [`ResolutionOutcome::as_str`] back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::Succeeded,
            Self::Failed,
            Self::Escalated,
            Self::Cancelled,
        ]
        .into_iter()
        .find(|outcome| outcome.as_str() == text)
    }
}

/// One subject an operation was pinned to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummarySubject {
    /// The host.
    pub host: HostId,
    /// The check.
    pub check: CheckKey,
    /// The target, `null` when the check has none.
    pub target: Option<TargetKey>,
}

/// A capability run that was actually handed to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutedContract {
    /// The contract id.
    pub contract: String,
    /// The host it ran on.
    pub host: HostId,
    /// The target, if any.
    pub target: Option<String>,
    /// A step outcome name: `succeeded`, `failed`, `outcome_unknown` or `running`.
    pub outcome: String,
}

/// A narrative field with its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NarrativeField {
    /// The text.
    pub text: String,
    /// Who wrote it: a model or a person.
    pub source: SummarySource,
    /// The actor in string form.
    pub actor: String,
    /// The revision shown.
    pub revision: u32,
    /// When it was written.
    pub at: Timestamp,
}

/// The current value of every field of one finished operation.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionSummary {
    /// The operation.
    pub operation: OperationId,
    /// What the operation was for.
    pub goal: String,
    /// When it ended.
    pub ended_at: Timestamp,
    /// How it ended.
    pub outcome: ResolutionOutcome,
    /// Whether a fix was verified.
    pub verified: bool,
    /// What it was pinned to.
    pub subjects: Vec<SummarySubject>,
    /// What was run.
    pub contracts: Vec<ExecutedContract>,
    /// Every host involved.
    pub hosts: Vec<HostId>,
    /// What was wrong.
    pub problem: Option<NarrativeField>,
    /// Why.
    pub cause: Option<NarrativeField>,
    /// What fixed it or should be done.
    pub fix: Option<NarrativeField>,
}

impl ResolutionSummary {
    /// Folds the newest revision of each field into a summary. `Ok(None)` when there is no
    /// `outcome` revision yet; `Err` names a stored value that does not have its field's shape.
    pub fn fold(
        goal: &str,
        ended_at: Timestamp,
        current: &[SummaryRevision],
    ) -> Result<Option<Self>, String> {
        fn get<T: serde::de::DeserializeOwned>(
            current: &[SummaryRevision],
            field: SummaryField,
        ) -> Result<Option<T>, String> {
            current
                .iter()
                .find(|r| r.field == field)
                .map(|r| serde_json::from_value(r.value.clone()))
                .transpose()
                .map_err(|e| format!("{}: {e}", field.as_str()))
        }
        fn narrative(current: &[SummaryRevision], field: SummaryField) -> Option<NarrativeField> {
            let revision = current.iter().find(|r| r.field == field)?;
            Some(NarrativeField {
                text: revision.value.as_str()?.to_owned(),
                source: revision.source,
                actor: revision.actor.clone(),
                revision: revision.revision,
                at: revision.created_at,
            })
        }
        let Some(outcome) = get::<ResolutionOutcome>(current, SummaryField::Outcome)? else {
            return Ok(None);
        };
        let Some(first) = current.first() else {
            return Ok(None);
        };
        Ok(Some(Self {
            operation: first.operation,
            goal: goal.to_owned(),
            ended_at,
            outcome,
            verified: get(current, SummaryField::Verified)?.unwrap_or(false),
            subjects: get(current, SummaryField::Subjects)?.unwrap_or_default(),
            contracts: get(current, SummaryField::ContractsExecuted)?.unwrap_or_default(),
            hosts: get(current, SummaryField::Hosts)?.unwrap_or_default(),
            problem: narrative(current, SummaryField::Problem),
            cause: narrative(current, SummaryField::Cause),
            fix: narrative(current, SummaryField::Fix),
        }))
    }
}

/// Outcome and verified flag of a finished operation, for list rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolutionBrief {
    /// How it ended.
    pub outcome: ResolutionOutcome,
    /// Whether a fix was verified.
    pub verified: bool,
    /// When it ended.
    pub ended_at: Timestamp,
}

/// The words of the subjects that the full-text index holds, such as
/// `container.restart_loop plex`.
#[must_use]
pub fn subjects_text(subjects: &[SummarySubject]) -> String {
    subjects
        .iter()
        .map(|s| match &s.target {
            Some(target) => format!("{} {target}", s.check),
            None => s.check.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The words of the executed contracts that the full-text index holds, such as
/// `skill.restart_container plex`.
#[must_use]
pub fn contracts_text(contracts: &[ExecutedContract]) -> String {
    contracts
        .iter()
        .map(|c| match &c.target {
            Some(target) => format!("{} {target}", c.contract),
            None => c.contract.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
