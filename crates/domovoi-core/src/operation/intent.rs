//! What a step is for: the immutable intent of each kind of step.

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::PlanHash;
use crate::actor::Actor;
use crate::capability::ContractId;
use crate::error::ValidationError;
use crate::id::{HostId, StepId};
use crate::signal::{CheckKey, TargetKey};
use crate::triage::ReadToolCall;

use super::step::{MAX_SUMMARY_CHARS, StepKind, bounded};

/// Longest free text of an assessment, plan, question or note, in characters.
const MAX_TEXT_CHARS: usize = 2000;

/// Why a step was a retry of an earlier one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryOf {
    /// The earlier Execution step.
    pub step: StepId,
    /// What is different this time, in one plain sentence (1 to 280 characters).
    pub changed: String,
}

/// What an Execution step asks to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionIntent {
    /// The capability contract to run.
    pub contract: ContractId,
    /// The contract version the plan was made against.
    pub contract_version: u32,
    /// The host to run it on.
    pub host: HostId,
    /// The check the run is about.
    pub check: CheckKey,
    /// The target the run is about, if any.
    pub target: Option<TargetKey>,
    /// The arguments, as the capability's manifest names them.
    pub args: BTreeMap<String, String>,
    /// The earlier step this one retries, if any.
    pub retry_of: Option<RetryOf>,
}

/// What a step is for. Written once and never changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepIntent {
    /// One turn of the model.
    ReasoningTurn {
        /// What the turn was given, by reference.
        context_refs: Vec<String>,
    },
    /// A read-only request for evidence.
    EvidenceRequest {
        /// The host to ask.
        host: HostId,
        /// The read tool's name.
        tool: String,
        /// The tool's arguments, validated as a [`ReadToolCall`] when the step is stored.
        args: serde_json::Value,
    },
    /// The model's reading of the evidence.
    Assessment {
        /// What it concluded.
        text: String,
    },
    /// The model's plan.
    Plan {
        /// The plan.
        text: String,
    },
    /// A policy decision about an execution.
    PolicyDecision {
        /// The Execution step it decides on.
        execution: StepId,
    },
    /// A person's approval of an execution.
    Approval {
        /// The Execution step it approves.
        execution: StepId,
        /// The plan hash a person must name to approve.
        plan_hash: PlanHash,
        /// When the approval window closes.
        expires_at: Timestamp,
    },
    /// A run of a capability.
    Execution(ExecutionIntent),
    /// A check after an execution.
    Verification {
        /// The Execution step it checks.
        execution: StepId,
        /// How long after the execution the check runs, in seconds.
        delay_secs: u64,
    },
    /// A message to a person.
    Notification {
        /// Where it goes.
        channel: String,
        /// What it is about.
        subject: String,
    },
    /// A question to a person.
    Question {
        /// The question.
        text: String,
        /// When it stops waiting for an answer.
        expires_at: Timestamp,
    },
    /// A note.
    Note {
        /// The note.
        text: String,
        /// Who wrote it.
        author: Actor,
    },
}

impl StepIntent {
    /// The kind of step this intent belongs to.
    #[must_use]
    pub const fn kind(&self) -> StepKind {
        match self {
            Self::ReasoningTurn { .. } => StepKind::ReasoningTurn,
            Self::EvidenceRequest { .. } => StepKind::EvidenceRequest,
            Self::Assessment { .. } => StepKind::Assessment,
            Self::Plan { .. } => StepKind::Plan,
            Self::PolicyDecision { .. } => StepKind::PolicyDecision,
            Self::Approval { .. } => StepKind::Approval,
            Self::Execution(_) => StepKind::Execution,
            Self::Verification { .. } => StepKind::Verification,
            Self::Notification { .. } => StepKind::Notification,
            Self::Question { .. } => StepKind::Question,
            Self::Note { .. } => StepKind::Note,
        }
    }

    /// Checks lengths and, for an evidence request, that the tool call parses.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::EvidenceRequest { tool, args, .. } => ReadToolCall::from_json(tool, args.clone())
                .map(drop)
                .map_err(|e| ValidationError::new("evidence request", e.to_string())),
            Self::Assessment { text }
            | Self::Plan { text }
            | Self::Question { text, .. }
            | Self::Note { text, .. } => bounded("step text", text, MAX_TEXT_CHARS),
            Self::Notification { channel, subject } => {
                bounded("notification channel", channel, 64)?;
                bounded("notification subject", subject, MAX_SUMMARY_CHARS)
            }
            Self::Execution(intent) => match &intent.retry_of {
                Some(retry) => bounded("retry change", &retry.changed, MAX_SUMMARY_CHARS),
                None => Ok(()),
            },
            Self::ReasoningTurn { .. }
            | Self::PolicyDecision { .. }
            | Self::Approval { .. }
            | Self::Verification { .. } => Ok(()),
        }
    }
}
