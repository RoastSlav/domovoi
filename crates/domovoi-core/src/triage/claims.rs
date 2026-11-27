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

    /// Whether the conclusion calls the event harmless, blames Domovoi or advises weaker
    /// authentication.
    #[must_use]
    pub fn claims_without_evidence(&self) -> bool {
        self.conclusion()
            .any(|text| says(text, BENIGN) || says(text, ATTRIBUTION) || says(text, LOOSENING))
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
