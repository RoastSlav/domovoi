//! The groups people sort incidents into: what each one asks of them.

use super::state::{Incident, IncidentStatus};
use crate::verdict::Verdict;

/// What an incident asks of the operator, from its status and triage's verdict. Every
/// incident is in exactly one group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IncidentGroup {
    /// Open, and triage did not judge it expected or only worth watching. An open incident
    /// triage has not looked at is here too.
    NeedsAction,
    /// Open, and triage said to keep an eye on it.
    Monitor,
    /// Open, and triage or a known condition says it is intended.
    Expected,
    /// Not confirmed yet.
    Pending,
    /// Acknowledged or muted by a person.
    Handled,
    /// The condition cleared.
    Resolved,
}

impl IncidentGroup {
    /// Every group, most urgent first.
    pub const ALL: [Self; 6] = [
        Self::NeedsAction,
        Self::Monitor,
        Self::Pending,
        Self::Expected,
        Self::Handled,
        Self::Resolved,
    ];

    /// The group's name as filters and the API spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::NeedsAction => "needs_action",
            Self::Monitor => "monitor",
            Self::Expected => "expected",
            Self::Pending => "pending",
            Self::Handled => "handled",
            Self::Resolved => "resolved",
        }
    }

    /// The group called `name`, if there is one.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|group| group.name() == name)
    }

    /// The group an incident with `status` and `verdict` is in.
    #[must_use]
    pub const fn of(status: IncidentStatus, verdict: Option<Verdict>) -> Self {
        match (status, verdict) {
            (IncidentStatus::Resolved, _) => Self::Resolved,
            (IncidentStatus::Pending, _) => Self::Pending,
            (IncidentStatus::Acked | IncidentStatus::Muted { .. }, _) => Self::Handled,
            (IncidentStatus::Open, Some(Verdict::Monitor)) => Self::Monitor,
            (IncidentStatus::Open, Some(Verdict::Expected)) => Self::Expected,
            (IncidentStatus::Open, _) => Self::NeedsAction,
        }
    }
}

impl Incident {
    /// What this incident asks of the operator now.
    #[must_use]
    pub const fn group(&self) -> IncidentGroup {
        IncidentGroup::of(self.status, self.verdict)
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;

    use super::*;

    #[test]
    fn an_open_incident_is_grouped_by_its_verdict() {
        let open = |verdict| IncidentGroup::of(IncidentStatus::Open, verdict);
        assert_eq!(open(None), IncidentGroup::NeedsAction);
        assert_eq!(
            open(Some(Verdict::ActionNeeded)),
            IncidentGroup::NeedsAction
        );
        assert_eq!(open(Some(Verdict::Fixed)), IncidentGroup::NeedsAction);
        assert_eq!(open(Some(Verdict::Monitor)), IncidentGroup::Monitor);
        assert_eq!(open(Some(Verdict::Expected)), IncidentGroup::Expected);
    }

    #[test]
    fn a_status_a_person_or_time_set_wins_over_the_verdict() {
        let verdict = Some(Verdict::ActionNeeded);
        let muted = IncidentStatus::Muted {
            until: Timestamp::UNIX_EPOCH,
        };
        assert_eq!(
            IncidentGroup::of(IncidentStatus::Pending, verdict),
            IncidentGroup::Pending
        );
        assert_eq!(
            IncidentGroup::of(IncidentStatus::Acked, verdict),
            IncidentGroup::Handled
        );
        assert_eq!(IncidentGroup::of(muted, verdict), IncidentGroup::Handled);
        assert_eq!(
            IncidentGroup::of(IncidentStatus::Resolved, verdict),
            IncidentGroup::Resolved
        );
    }

    #[test]
    fn names_round_trip() {
        for group in IncidentGroup::ALL {
            assert_eq!(IncidentGroup::parse(group.name()), Some(group));
        }
        assert_eq!(IncidentGroup::parse("open"), None);
    }
}
