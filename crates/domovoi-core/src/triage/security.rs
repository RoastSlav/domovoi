//! What a security event may be told about itself.
//!
//! A `security.*` check records something that happened. Only a person decides that it was
//! expected: with a known condition, or by dismissing the incident. A model's report never
//! does, a report written for another incident never does, and the text of a report whose
//! verdict was refused is not shown as the answer.

use crate::id::IncidentId;
use crate::incident::IncidentStatus;
use crate::ports::IncidentTimelineEntry;
use crate::signal::CheckKey;
use crate::verdict::Verdict;

use super::claims::NOT_ESTABLISHED;
use super::coverage::SEEN_UNTIL_KEY;
use super::report::{HEADLINE_MAX_CHARS, TriageOutcome, TriageReport};

/// Opens `details` when a model called a security event expected.
pub const NOT_DECIDED: &str = "Needs a person: the model called this security event expected, \
but only a person can decide that (a known condition or an action Domovoi recorded); an earlier \
incident that looks alike is not evidence.";

/// Opens `details` when a model closed a security event without a tool result to show for it.
pub const NOT_VERIFIED: &str = "Not verified: the model called this event expected or fixed, \
but no tool result in its report ties it to a recorded action, so it needs a person.";

/// Opens `details` when a report written for another incident was attached to a security event.
pub const NOT_REUSED: &str = "Needs a person: this report was written for another security \
event and reused. It is not evidence about this one.";

const NEEDS_PERSON: &str = "Needs a person: ";
const UNVERIFIED: &str = "Not verified: ";

/// Whether `check` records security events.
#[must_use]
pub fn is_security_check(check: &CheckKey) -> bool {
    check.as_str().starts_with("security.")
}

impl TriageReport {
    /// Refuses the report's judgement: sets `verdict`, rewrites every field that states a
    /// conclusion so it agrees with it, and keeps the model's own text in `details`, marked
    /// as not accepted. `why` opens `details`. `title` is the incident's title.
    pub fn withhold(&mut self, verdict: Verdict, why: &str, title: &str) {
        let mut rejected = format!(
            "The model's own text, not accepted:\nheadline: {}\nsummary: {}\ncause: {}\n\
             next step: {}",
            self.headline, self.summary, self.cause, self.next_step
        );
        for step in &self.manual_steps {
            rejected.push_str("\nby hand: ");
            rejected.push_str(step);
        }
        // A report refused before keeps one copy of the reason.
        let earlier = [NOT_DECIDED, NOT_VERIFIED, NOT_REUSED, NOT_ESTABLISHED]
            .iter()
            .fold(self.details.as_str(), |text, marker| {
                text.strip_prefix(marker).unwrap_or(text)
            })
            .trim()
            .to_owned();
        self.details = format!("{why}\n\n{rejected}");
        if !earlier.is_empty() {
            self.details.push_str("\n\n");
            self.details.push_str(&earlier);
        }
        let prefix = if verdict == Verdict::ActionNeeded {
            NEEDS_PERSON
        } else {
            UNVERIFIED
        };
        self.verdict = verdict;
        self.headline = format!("{prefix}{title}")
            .chars()
            .take(HEADLINE_MAX_CHARS)
            .collect();
        self.summary = format!("A security event was recorded: {title}. {why}");
        self.cause = "Not established.".to_owned();
        self.next_step = "Check who did this and from where. If it was you, dismiss the \
                          incident; if not, change the affected credentials and close the way in."
            .to_owned();
        self.manual_steps.clear();
        // Evidence about other incidents says nothing about this one.
        self.evidence.retain(|item| {
            !["search_experience", "incident_history", "notes_search"]
                .iter()
                .any(|tool| item.source.contains(tool))
        });
    }

    /// Whether the headline already is the one [`TriageReport::withhold`] writes.
    fn is_withheld(&self) -> bool {
        self.headline.starts_with(NEEDS_PERSON) || self.headline.starts_with(UNVERIFIED)
    }

    /// Applies the security rule to a stored or copied report of `check`: `EXPECTED` becomes
    /// `ACTION_NEEDED`, text that calls the event harmless, blames Domovoi or advises weaker
    /// authentication is replaced by "Not established" (verdict at least `ACTION_NEEDED`),
    /// and a report capped earlier whose text still says otherwise is rewritten. Other checks are left alone. Returns whether it changed anything.
    pub fn cap_security(&mut self, check: &CheckKey, title: &str) -> bool {
        if !is_security_check(check) {
            return false;
        }
        if self.verdict == Verdict::Expected {
            self.withhold(Verdict::ActionNeeded, NOT_DECIDED, title);
            return true;
        }
        if self.is_withheld() {
            return false;
        }
        for why in [NOT_DECIDED, NOT_VERIFIED] {
            if self.details.starts_with(why) {
                self.withhold(self.verdict, why, title);
                return true;
            }
        }
        self.refuse_claims(title)
    }
}

impl TriageOutcome {
    /// [`TriageReport::cap_security`] for an outcome. A reused report of a security check is
    /// refused whatever its verdict. Returns whether it changed anything.
    pub fn cap_security(&mut self, check: &CheckKey, title: &str) -> bool {
        if !is_security_check(check) {
            return false;
        }
        match self {
            Self::Report(report) => report.cap_security(check, title),
            Self::Cached { report, .. } => {
                if report.details.starts_with(NOT_REUSED) {
                    return false;
                }
                report.withhold(Verdict::ActionNeeded, NOT_REUSED, title);
                report.proposals.clear();
                report.evidence.clear();
                true
            }
            Self::Skipped { .. } | Self::Failed { .. } => false,
        }
    }
}

/// The timeline summary of a reused report.
#[must_use]
pub fn cached_summary(headline: &str, from: IncidentId) -> String {
    format!("{headline} (reused from {from})")
}

/// Rewrites the stored triage entries of a security incident as they may be shown. The
/// stored rows are not changed.
pub fn show_timeline(check: &CheckKey, title: &str, entries: &mut [IncidentTimelineEntry]) {
    if !is_security_check(check) {
        return;
    }
    for entry in entries {
        if !matches!(entry.kind.as_str(), "triage.report" | "triage.cached") {
            continue;
        }
        let Ok(mut outcome) = serde_json::from_value::<TriageOutcome>(entry.detail.clone()) else {
            continue;
        };
        if !outcome.cap_security(check, title) {
            continue;
        }
        entry.summary = match &outcome {
            TriageOutcome::Report(report) => report.headline.clone(),
            TriageOutcome::Cached { report, from } => cached_summary(&report.headline, *from),
            TriageOutcome::Skipped { reason } | TriageOutcome::Failed { reason } => reason.clone(),
        };
        let seen = entry.detail.get(SEEN_UNTIL_KEY).cloned();
        entry.detail = serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null);
        if let (Some(seen), Some(detail)) = (seen, entry.detail.as_object_mut()) {
            detail.insert(SEEN_UNTIL_KEY.to_owned(), seen);
        }
    }
}

/// The entries that speak about the incident as it stands. A reopened security incident is a
/// new event: what was said before it reopened is history, not its answer. Other incidents
/// keep their whole timeline.
#[must_use]
pub fn current_episode<'a>(
    check: &CheckKey,
    entries: &'a [IncidentTimelineEntry],
) -> &'a [IncidentTimelineEntry] {
    if !is_security_check(check) {
        return entries;
    }
    let reopened = entries.iter().rposition(|entry| {
        entry.kind == "status.changed"
            && serde_json::from_value::<IncidentStatus>(entry.detail["from"].clone())
                .is_ok_and(|from| from == IncidentStatus::Resolved)
    });
    reopened.map_or(entries, |index| &entries[index..])
}

#[cfg(test)]
#[path = "security_tests.rs"]
mod tests;
