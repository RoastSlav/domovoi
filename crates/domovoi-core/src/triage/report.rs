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
/// Most by-hand steps in one report.
pub const MAX_MANUAL_STEPS: usize = 5;
/// Longest one by-hand step, in characters.
pub const MANUAL_STEP_MAX_CHARS: usize = 200;

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
    /// Steps a person does by hand, such as checking a cable; at most [`MAX_MANUAL_STEPS`].
    #[serde(default)]
    pub manual_steps: Vec<String>,
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
        if self.manual_steps.len() > MAX_MANUAL_STEPS {
            return Err(ValidationError::new(
                "manual_steps",
                format!("must have at most {MAX_MANUAL_STEPS} items"),
            ));
        }
        if self
            .manual_steps
            .iter()
            .any(|step| step.chars().count() > MANUAL_STEP_MAX_CHARS)
        {
            return Err(ValidationError::new(
                "manual_steps",
                format!("each must be at most {MANUAL_STEP_MAX_CHARS} characters"),
            ));
        }
        Ok(())
    }
}

/// Share of words two texts have in common, from 0.0 to 1.0 (Jaccard on lowercase words).
fn overlap(a: &str, b: &str) -> f64 {
    let words = |t: &str| -> std::collections::BTreeSet<String> {
        t.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2)
            .map(str::to_lowercase)
            .collect()
    };
    let (a, b) = (words(a), words(b));
    let union = a.union(&b).count();
    if union == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let ratio = a.intersection(&b).count() as f64 / union as f64;
    ratio
}

impl TriageReport {
    /// Names of fields that say what an earlier field already said, or that break the field's
    /// job (a shell command in `next_step`). A soft check: the report is still kept, the
    /// caller shows the repeated field once.
    #[must_use]
    pub fn field_problems(&self) -> Vec<&'static str> {
        let mut found = Vec::new();
        if overlap(&self.cause, &self.summary) >= 0.7 {
            found.push("cause repeats summary");
        }
        if overlap(&self.next_step, &self.summary) >= 0.7
            || overlap(&self.next_step, &self.cause) >= 0.7
        {
            found.push("next_step repeats the problem");
        }
        if has_shell_command(&self.next_step) {
            found.push("next_step is a shell command");
        }
        found
    }
}

/// Programs whose next word, when it is one of [`COMMAND_VERBS`], makes a sentence a command.
const COMMAND_PROGRAMS: [&str; 6] = ["apt", "apt-get", "dnf", "pacman", "systemctl", "docker"];
const COMMAND_VERBS: [&str; 14] = [
    "install", "remove", "purge", "restart", "start", "stop", "enable", "disable", "run", "rm",
    "upgrade", "update", "-S", "-Syu",
];

/// Whether `text` holds a shell command: a line that starts with `$` or `sudo `, a backtick, or
/// a package or service program followed by a verb ("apt install nginx then systemctl restart").
fn has_shell_command(text: &str) -> bool {
    if text.contains('`')
        || text.lines().any(|line| {
            line.trim_start().starts_with("sudo ") || line.trim_start().starts_with('$')
        })
    {
        return true;
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    words.windows(2).any(|pair| {
        COMMAND_PROGRAMS.contains(&pair[0].trim_matches(|c: char| !c.is_alphanumeric() && c != '-'))
            && COMMAND_VERBS.contains(&pair[1].trim_matches(|c: char| c == ',' || c == '.'))
    })
}

/// Limits for one triage run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TriageBudget {
    /// Most tool calls the model may make.
    pub max_tool_calls: u16,
    /// Wall-clock limit in seconds.
    pub max_seconds: u32,
    /// Most output tokens one model turn may ask for. A reasoning model counts its
    /// reasoning against this, so a turn that runs out is retried with more.
    pub max_output_tokens: u32,
    /// Most output tokens the whole run may ask for. Every request is charged before it is
    /// sent, including each growth retry and the report repair, so growing an allowance
    /// cannot run without bound.
    pub max_total_output_tokens: u32,
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
