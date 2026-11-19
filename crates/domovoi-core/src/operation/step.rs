//! Steps: the immutable units an operation is made of, and their derived progress.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::ValidationError;
use crate::id::{OperationId, StepId};

use super::intent::StepIntent;

/// Longest step summary or reason, in characters.
pub const MAX_SUMMARY_CHARS: usize = 280;
/// Longest stored step output, in bytes.
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024;
/// Longest phase label, in characters.
const MAX_PHASE_CHARS: usize = 40;

/// `text` cut to at most `max` characters, on a character boundary.
#[must_use]
pub fn cut_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// What a step is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// One turn of the model.
    ReasoningTurn,
    /// A read-only request for evidence from a host.
    EvidenceRequest,
    /// The model's reading of the evidence.
    Assessment,
    /// The model's plan.
    Plan,
    /// A policy decision about an execution. Nothing creates one yet.
    PolicyDecision,
    /// A person's approval of an execution.
    Approval,
    /// A run of a capability on a host.
    Execution,
    /// A check that an execution fixed the problem.
    Verification,
    /// A message to a person.
    Notification,
    /// A question to a person.
    Question,
    /// A note, such as a person's answer.
    Note,
}

impl StepKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 11] = [
        Self::ReasoningTurn,
        Self::EvidenceRequest,
        Self::Assessment,
        Self::Plan,
        Self::PolicyDecision,
        Self::Approval,
        Self::Execution,
        Self::Verification,
        Self::Notification,
        Self::Question,
        Self::Note,
    ];

    /// The stored snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReasoningTurn => "reasoning_turn",
            Self::EvidenceRequest => "evidence_request",
            Self::Assessment => "assessment",
            Self::Plan => "plan",
            Self::PolicyDecision => "policy_decision",
            Self::Approval => "approval",
            Self::Execution => "execution",
            Self::Verification => "verification",
            Self::Notification => "notification",
            Self::Question => "question",
            Self::Note => "note",
        }
    }

    /// Reads [`StepKind::as_str`] back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// How far a step got. Derived progress, never part of the step's intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    /// Not started.
    Pending,
    /// Started, not finished.
    Running,
    /// Done and it worked.
    Succeeded,
    /// Done and it did not work.
    Failed,
    /// Refused before running.
    Refused,
    /// Decided not to run, or never got to.
    NotRun,
    /// Could not be run: the host or capability is not available.
    Unavailable,
    /// It may have run; nobody knows.
    OutcomeUnknown,
    /// Skipped because the operation ended first.
    Skipped,
    /// Cancelled with the operation.
    Cancelled,
}

impl StepOutcome {
    /// Every outcome, in declaration order.
    pub const ALL: [Self; 10] = [
        Self::Pending,
        Self::Running,
        Self::Succeeded,
        Self::Failed,
        Self::Refused,
        Self::NotRun,
        Self::Unavailable,
        Self::OutcomeUnknown,
        Self::Skipped,
        Self::Cancelled,
    ];

    /// The stored snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Refused => "refused",
            Self::NotRun => "not_run",
            Self::Unavailable => "unavailable",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }

    /// Reads [`StepOutcome::as_str`] back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|outcome| outcome.as_str() == text)
    }

    /// Whether the step is over: neither pending nor running.
    #[must_use]
    pub const fn is_final(self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }
}

/// Checks `text` holds 1 to `max` characters.
pub(crate) fn bounded(field: &'static str, text: &str, max: usize) -> Result<(), ValidationError> {
    let len = text.chars().count();
    if len == 0 || len > max {
        return Err(ValidationError::new(
            field,
            format!("must be 1 to {max} characters"),
        ));
    }
    Ok(())
}

/// One immutable step of an operation.
///
/// Intent never changes after the step is stored. What happened to it afterwards is the
/// [`StepProgress`], stored beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// The step's id.
    pub id: StepId,
    /// The operation it belongs to.
    pub operation: OperationId,
    /// Its place in the operation: 0-based, dense and unique.
    pub ordinal: u32,
    /// What it is.
    pub kind: StepKind,
    /// A short label of the phase of work it belongs to (1 to 40 of `[a-z0-9_-]`).
    pub phase: String,
    /// The step it follows from, always of a lower ordinal.
    pub parent: Option<StepId>,
    /// The cycle it was created in.
    pub cycle: u32,
    /// What it is for.
    pub intent: StepIntent,
    /// When it was created.
    pub created_at: Timestamp,
}

impl Step {
    /// Checks the phase grammar, that the kind matches the intent, and the intent itself.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let phase_ok = !self.phase.is_empty()
            && self.phase.chars().count() <= MAX_PHASE_CHARS
            && self
                .phase
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'));
        if !phase_ok {
            return Err(ValidationError::new(
                "step phase",
                "must be 1 to 40 characters of a-z, 0-9, underscore and hyphen",
            ));
        }
        if self.kind != self.intent.kind() {
            return Err(ValidationError::new(
                "step kind",
                "does not match the intent",
            ));
        }
        self.intent.validate()
    }
}

/// How far a step got, derived from what happened to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepProgress {
    /// The outcome so far.
    pub outcome: StepOutcome,
    /// When it started.
    pub started_at: Option<Timestamp>,
    /// When it ended.
    pub ended_at: Option<Timestamp>,
    /// A one-line summary, at most 280 characters, already redacted.
    pub summary: Option<String>,
    /// Why it ended as it did, at most 280 characters.
    pub reason: Option<String>,
    /// What it produced, at most 16 KiB, already redacted. Never in a trace or the journal.
    pub output: Option<String>,
}

impl StepProgress {
    /// A step that has not started.
    #[must_use]
    pub const fn pending() -> Self {
        Self {
            outcome: StepOutcome::Pending,
            started_at: None,
            ended_at: None,
            summary: None,
            reason: None,
            output: None,
        }
    }

    /// Checks the text limits.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let too_long = |text: &Option<String>| {
            text.as_ref()
                .is_some_and(|t| t.chars().count() > MAX_SUMMARY_CHARS)
        };
        if too_long(&self.summary) || too_long(&self.reason) {
            return Err(ValidationError::new(
                "step progress",
                "summary and reason are at most 280 characters",
            ));
        }
        if self
            .output
            .as_ref()
            .is_some_and(|o| o.len() > MAX_OUTPUT_BYTES)
        {
            return Err(ValidationError::new(
                "step progress",
                "output is at most 16 KiB",
            ));
        }
        Ok(())
    }
}

/// A step and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    /// The immutable step.
    pub step: Step,
    /// Its progress.
    pub progress: StepProgress,
}

/// What a model turn used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepUsage {
    /// Tokens sent to the model.
    pub input_tokens: u32,
    /// Tokens the model wrote.
    pub output_tokens: u32,
    /// Cost in millionths, when the provider reports one.
    pub cost_micros: Option<u64>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::id::HostId;

    use super::*;

    #[test]
    fn step_kinds_and_outcomes_round_trip() {
        assert_eq!(StepKind::ALL.len(), 11);
        assert_eq!(StepOutcome::ALL.len(), 10);
        for kind in StepKind::ALL {
            assert_eq!(StepKind::parse(kind.as_str()), Some(kind));
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, json!(kind.as_str()));
        }
        for outcome in StepOutcome::ALL {
            assert_eq!(StepOutcome::parse(outcome.as_str()), Some(outcome));
            let json = serde_json::to_value(outcome).unwrap();
            assert_eq!(json, json!(outcome.as_str()));
        }
        assert_eq!(StepKind::parse("thought"), None);
        let finals: Vec<_> = StepOutcome::ALL.iter().filter(|o| o.is_final()).collect();
        assert_eq!(finals.len(), 8);
    }

    #[test]
    fn cut_chars_never_splits_a_character() {
        assert_eq!(cut_chars("aé😀b", 3), "aé😀");
        assert_eq!(cut_chars("ab", 5), "ab");
    }

    fn at() -> Timestamp {
        Timestamp::constant(1_700_000_000, 0)
    }

    fn step(phase: &str, intent: StepIntent) -> Result<Step, ValidationError> {
        let step = Step {
            id: StepId::new(),
            operation: OperationId::new(),
            ordinal: 0,
            kind: intent.kind(),
            phase: phase.to_owned(),
            parent: None,
            cycle: 0,
            intent,
            created_at: at(),
        };
        step.validate().map(|()| step)
    }

    #[test]
    fn step_validation_checks_phase_text_and_evidence_calls() {
        let plan = |text: &str| StepIntent::Plan {
            text: text.to_owned(),
        };
        assert!(step("diagnose", plan("restart it")).is_ok());
        assert!(step("Diagnose", plan("restart it")).is_err());
        assert!(step("", plan("restart it")).is_err());
        assert!(step("diagnose", plan("")).is_err());
        assert!(step("diagnose", plan(&"x".repeat(2001))).is_err());
        let evidence = |tool: &str| StepIntent::EvidenceRequest {
            host: HostId::new(),
            tool: tool.to_owned(),
            args: json!({}),
        };
        assert!(step("evidence", evidence("docker_ps")).is_ok());
        assert!(step("evidence", evidence("rm_rf")).is_err());
    }

    #[test]
    fn step_intent_json_is_tagged_by_kind() {
        let intent = StepIntent::Verification {
            execution: StepId::from_uuid(uuid::Uuid::from_u128(1)),
            delay_secs: 30,
        };
        let value = serde_json::to_value(&intent).unwrap();
        assert_eq!(value["kind"], "verification");
        assert_eq!(serde_json::from_value::<StepIntent>(value).unwrap(), intent);
    }

    #[test]
    fn progress_limits_are_checked() {
        assert!(StepProgress::pending().validate().is_ok());
        let long = StepProgress {
            summary: Some("x".repeat(281)),
            ..StepProgress::pending()
        };
        assert!(long.validate().is_err());
        let big = StepProgress {
            output: Some("x".repeat(MAX_OUTPUT_BYTES + 1)),
            ..StepProgress::pending()
        };
        assert!(big.validate().is_err());
    }
}
