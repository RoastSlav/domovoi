//! What triage produces: a structured report, proposals and the outcome of a run.

use std::collections::BTreeMap;

use crate::error::ValidationError;
use crate::id::IncidentId;
use crate::skill::SkillName;
use crate::verdict::Verdict;

/// Longest headline, in characters.
pub const HEADLINE_MAX_CHARS: usize = 120;
/// Longest next step, in characters.
pub const NEXT_STEP_MAX_CHARS: usize = 300;
/// Most evidence references in one report.
pub const MAX_EVIDENCE: usize = 6;
/// Most action proposals in one report.
pub const MAX_PROPOSALS: usize = 3;

/// One piece of evidence the model cites.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    /// Short name, for example `restarts`.
    pub label: String,
    /// The observed value.
    pub value: String,
    /// Where it came from, for example `docker_logs plex`.
    pub source: String,
}

/// A suggestion to run a skill. It is never executed directly: the engine validates it
/// against the manifest and it waits for a human approval (INV-9).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionProposal {
    /// The skill to run.
    pub skill: SkillName,
    /// Arguments, validated against the manifest before anything is proposed to a human.
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    /// Why the model suggests it.
    pub reason: String,
}

/// The JSON object the model must return as its final turn.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TriageReport {
    /// One line for the card, at most [`HEADLINE_MAX_CHARS`] characters.
    pub headline: String,
    /// A few sentences on what is happening.
    pub summary: String,
    /// The most likely cause.
    pub cause: String,
    /// Supporting evidence, at most [`MAX_EVIDENCE`] items.
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    /// The overall judgement.
    pub verdict: Verdict,
    /// What a human should do next, at most [`NEXT_STEP_MAX_CHARS`] characters.
    pub next_step: String,
    /// Skills worth running, at most [`MAX_PROPOSALS`].
    #[serde(default)]
    pub proposals: Vec<ActionProposal>,
    /// Full reasoning, posted in the incident thread.
    #[serde(default)]
    pub details: String,
}

impl TriageReport {
    /// Checks the length and count limits.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let headline = self.headline.chars().count();
        if headline == 0 || headline > HEADLINE_MAX_CHARS {
            return Err(ValidationError::new(
                "headline",
                format!("must be 1 to {HEADLINE_MAX_CHARS} characters"),
            ));
        }
        if self.next_step.chars().count() > NEXT_STEP_MAX_CHARS {
            return Err(ValidationError::new(
                "next_step",
                format!("must be at most {NEXT_STEP_MAX_CHARS} characters"),
            ));
        }
        if self.evidence.len() > MAX_EVIDENCE {
            return Err(ValidationError::new(
                "evidence",
                format!("must have at most {MAX_EVIDENCE} items"),
            ));
        }
        if self.proposals.len() > MAX_PROPOSALS {
            return Err(ValidationError::new(
                "proposals",
                format!("must have at most {MAX_PROPOSALS} items"),
            ));
        }
        Ok(())
    }
}

/// Limits for one triage run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TriageBudget {
    /// Most tool calls the model may make.
    pub max_tool_calls: u16,
    /// Wall-clock limit in seconds.
    pub max_seconds: u32,
    /// Most output tokens across the run.
    pub max_output_tokens: u32,
}

/// How a triage run ended.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum TriageOutcome {
    /// The model produced a valid report.
    Report(TriageReport),
    /// A previous report for an equivalent incident was reused without a model call.
    Cached {
        /// The reused report.
        report: TriageReport,
        /// The incident it came from.
        from: IncidentId,
    },
    /// Triage did not run, for example because a budget was exhausted.
    Skipped {
        /// Why.
        reason: String,
    },
    /// Triage ran and failed; the incident is posted as a raw card.
    Failed {
        /// Why.
        reason: String,
    },
}
