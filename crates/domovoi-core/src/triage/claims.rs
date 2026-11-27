//! Claims a model may not make about a security event on its own word.
//!
//! A `security.*` check records something that happened. Calling it harmless, blaming
//! Domovoi or its agent, or telling a person to loosen authentication would wave it away, and
//! only a person decides that (a known condition, or a dismissal). A model's text that says so
//! is refused, whatever verdict it carries.

use crate::verdict::Verdict;

use super::report::TriageReport;

/// Opens `details` when a model's text called a security event benign, blamed Domovoi or
/// advised weaker authentication with nothing recorded to support it.
pub const NOT_ESTABLISHED: &str = "Not established: the model's report called this security \
event harmless, blamed Domovoi or its agent, or advised weaker authentication, but no known \
condition and no action Domovoi recorded supports that. Only a person can decide it.";

/// Opens `details` when the engine itself found a recorded Domovoi action that explains the
/// security event. The engine writes this line and nothing else may: it clears the line from
/// model text before it adds its own, so a report cannot claim a record it was not given.
pub const RECORDED_ACTION: &str = "Recorded Domovoi action: ";

/// Phrases that call an event harmless or expected.
const BENIGN: &[&str] = &[
    "benign",
    "harmless",
    "false positive",
    "no action required",
    "no action needed",
    "no action is required",
    "no action is needed",
    "nothing to worry",
    "not a concern",
    "no concern",
    "no security concern",
    "not a security",
    "not malicious",
    "not suspicious",
    "not an attack",
    "safe to ignore",
    "can be ignored",
    "can safely ignore",
    "routine",
    "normal behavior",
    "normal behaviour",
    "normal activity",
    "normal operation",
    "expected behavior",
    "expected behaviour",
    "expected activity",
    "expected failure",
    "expected authentication",
    "expected error",
    "is expected",
    "are expected",
    "was expected",
    "as expected",
    "likely a typo",
    "typo",
    "mistyped",
    "legitimate",
    "self-test",
];

/// Phrases that put an event on Domovoi or its agent.
const ATTRIBUTION: &[&str] = &[
    "domovoi",
    "the agent",
    "agent operations",
    "agent's",
    "automated operation",
    "automated check",
    "automated process",
    "scheduled check",
    "health check",
    "monitoring check",
];

/// Phrases that advise weaker authentication.
const LOOSENING: &[&str] = &[
    "nopasswd",
    "passwordless",
    "without a password",
    "without password",
    "without authentication",
    "disable authentication",
    "disable password",
    "sudoers",
    "permitrootlogin yes",
    "passwordauthentication yes",
];

/// Whether `text` says one of `phrases`, ignoring case. A phrase starting a word is not found
/// inside a longer word, so "unexpected" does not say "expected".
fn says(text: &str, phrases: &[&str]) -> bool {
    let text = text.to_lowercase();
    phrases.iter().any(|phrase| {
        text.match_indices(phrase).any(|(at, _)| {
            text[..at]
                .chars()
                .next_back()
                .is_none_or(|before| !before.is_alphanumeric())
        })
    })
}

impl TriageReport {
    /// The text a person reads as the model's conclusion: the headline, summary, cause, next
    /// step and manual steps. `details` is reasoning and is not read.
    fn conclusion(&self) -> impl Iterator<Item = &str> {
        [&self.headline, &self.summary, &self.cause, &self.next_step]
            .into_iter()
            .chain(&self.manual_steps)
            .map(String::as_str)
    }

    /// Whether the engine found a recorded Domovoi action for this report (see
    /// [`TriageReport::record_action`]).
    #[must_use]
    pub fn has_recorded_action(&self) -> bool {
        self.details.starts_with(RECORDED_ACTION)
    }

    /// Whether the conclusion puts the event on Domovoi, its agent or its automated checks.
    #[must_use]
    pub fn attributes_to_domovoi(&self) -> bool {
        self.conclusion().any(|text| says(text, ATTRIBUTION))
    }

    /// Removes a record line the model wrote itself. Called before [`Self::record_action`]
    /// and before any guard, so the line is the engine's or absent.
    pub fn clear_recorded_action(&mut self) {
        if let Some(rest) = self.details.strip_prefix(RECORDED_ACTION) {
            self.details = format!("The model's details: {rest}");
        }
    }

    /// Notes the recorded Domovoi action `line` that the engine found for the event: `details`
    /// opens with it, and when the report puts the event on Domovoi, `cause` becomes the
    /// record, with the model's own cause kept in `details` as replaced. The model's word is
    /// not the evidence: the record is.
    pub fn record_action(&mut self, line: &str) {
        self.clear_recorded_action();
        if self.attributes_to_domovoi() {
            let replaced = std::mem::replace(&mut self.cause, format!("Recorded: {line}"));
            self.details = format!(
                "{RECORDED_ACTION}{line}\n\nThe model's own cause, replaced by the record: \
                 {replaced}\n\n{}",
                self.details
            );
        } else {
            self.details = format!("{RECORDED_ACTION}{line}\n\n{}", self.details);
        }
    }

    /// Whether the conclusion calls the event harmless, advises weaker authentication or
    /// blames Domovoi. A recorded action lets the report blame Domovoi, and nothing else.
    #[must_use]
    pub fn claims_without_evidence(&self) -> bool {
        let recorded = self.has_recorded_action();
        self.conclusion().any(|text| {
            says(text, BENIGN) || (!recorded && says(text, ATTRIBUTION)) || says(text, LOOSENING)
        })
    }

    /// Withholds the report when its text makes a claim only a person may make; the verdict
    /// becomes `ACTION_NEEDED` if it was lower. Returns whether it changed anything.
    pub(super) fn refuse_claims(&mut self, title: &str) -> bool {
        if !self.claims_without_evidence() {
            return false;
        }
        self.withhold(Verdict::ActionNeeded, NOT_ESTABLISHED, title);
        true
    }
}

/// Whether a question to a person about a security event asserts a benign cause, blames
/// Domovoi (unless `recorded`: the engine found a record of an action) or proposes weaker
/// authentication. The same phrases as for a report.
#[must_use]
pub fn question_asserts_a_claim(question: &str, recorded: bool) -> bool {
    says(question, BENIGN)
        || (!recorded && says(question, ATTRIBUTION))
        || says(question, LOOSENING)
}

/// The question that replaces a refused one: neutral, written by code, with no cause in it.
#[must_use]
pub fn neutral_question(title: &str) -> String {
    format!(
        "A security event was recorded: {title}. Was this you, or someone you know? If so, \
         what were you doing, and from where? If you do not know, say so."
    )
}
