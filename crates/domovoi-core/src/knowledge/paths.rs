//! Management paths: how the owner reaches a host, and whether that has been confirmed.

use jiff::Timestamp;

use super::fact::{Fact, FactSource, KnowledgeStatus};
use crate::actor::Actor;
use crate::id::{FactRecordId, HostId};

/// Opaque proof that a path is covered. It is stored and compared for equality only, never
/// parsed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageEvidence(String);

impl CoverageEvidence {
    /// Wraps evidence text of 1 to 2048 characters.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        (1..=2048)
            .contains(&text.chars().count())
            .then_some(Self(text))
    }

    /// The stored text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The owner's confirmation that a management path is covered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathConfirmation {
    /// The management-path fact this confirms. A confirmation belongs to one fact id.
    pub fact: FactRecordId,
    /// The host the path belongs to.
    pub host: HostId,
    /// The owner who confirmed it.
    pub confirmed_by: Actor,
    /// When it was confirmed.
    pub confirmed_at: Timestamp,
    /// What showed the path covered.
    pub evidence: CoverageEvidence,
}

/// What a coverage check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageVerdict {
    /// A verified, fresh attestation shows the path covered.
    Covered {
        /// The proof to store with the confirmation.
        evidence: CoverageEvidence,
    },
    /// The attestation shows the path is not covered.
    Uncovered,
    /// Coverage cannot be established right now.
    Unverifiable {
        /// Why not.
        reason: UnverifiableReason,
    },
}

/// Why coverage could not be verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnverifiableReason {
    /// No attestor is wired in.
    NotAvailable,
    /// The helper's key is not pinned.
    KeyNotPinned,
    /// The helper presented a different key than the pinned one.
    KeyMismatch,
    /// No attestation was received.
    AttestationMissing,
    /// The attestation is too old.
    AttestationStale,
    /// The attestation's signature does not verify.
    SignatureInvalid,
    /// The report does not list everything it must.
    ReportIncomplete,
    /// The attestation's generation went backwards.
    GenerationRegressed,
}

impl UnverifiableReason {
    /// A short plain description for the owner.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::NotAvailable => "the helper's coverage cannot be verified yet",
            Self::KeyNotPinned => "the helper's key is not pinned",
            Self::KeyMismatch => "the helper's key does not match the pinned key",
            Self::AttestationMissing => "the helper has not sent an attestation",
            Self::AttestationStale => "the helper's attestation is too old",
            Self::SignatureInvalid => "the helper's attestation is not validly signed",
            Self::ReportIncomplete => "the helper's report is incomplete",
            Self::GenerationRegressed => "the helper's attestation went backwards",
        }
    }
}

/// Where a management path stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathState {
    /// Not owner-sourced: a suggestion awaiting acceptance.
    Candidate,
    /// Owner-sourced but never confirmed.
    Unconfirmed,
    /// Owner-sourced, fresh and confirmed.
    Confirmed,
    /// Past its freshness period.
    Stale,
}

impl PathState {
    /// The stable name used on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Unconfirmed => "unconfirmed",
            Self::Confirmed => "confirmed",
            Self::Stale => "stale",
        }
    }
}

/// The state of a live management-path fact: candidate when its source is not the owner,
/// stale when its effective status is stale, unconfirmed when it has no confirmation,
/// otherwise confirmed.
#[must_use]
pub fn path_state(
    fact: &Fact,
    confirmation: Option<&PathConfirmation>,
    now: Timestamp,
) -> PathState {
    if fact.source != FactSource::User {
        PathState::Candidate
    } else if fact.effective_status(now) == KnowledgeStatus::Stale {
        PathState::Stale
    } else if confirmation.is_none_or(|c| c.fact != fact.id) {
        PathState::Unconfirmed
    } else {
        PathState::Confirmed
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::id::UserId;
    use crate::knowledge::{Predicate, SubjectRef};

    #[test]
    fn path_state_table() {
        let host = HostId::new();
        let user = Actor::User(UserId::new());
        let mut fact = Fact {
            id: FactRecordId::new(),
            subject: SubjectRef::Host(host),
            predicate: Predicate::ManagementPath,
            value: Some(json!({"cidr": "10.0.0.0/8"})),
            source: FactSource::Discovered,
            status: KnowledgeStatus::Current,
            observed_at: Timestamp::from_second(0).unwrap(),
            valid_until: None,
            last_verified_at: None,
            supersedes: None,
            evidence: None,
            actor: user.clone(),
            created_at: Timestamp::from_second(0).unwrap(),
            retracted_at: None,
            retracted_by: None,
            content_purged_at: None,
        };
        let now = Timestamp::from_second(86_400).unwrap();
        let confirmation = PathConfirmation {
            fact: fact.id,
            host,
            confirmed_by: user,
            confirmed_at: now,
            evidence: CoverageEvidence::new("e").unwrap(),
        };
        assert_eq!(
            path_state(&fact, Some(&confirmation), now),
            PathState::Candidate
        );
        fact.source = FactSource::User;
        assert_eq!(path_state(&fact, None, now), PathState::Unconfirmed);
        assert_eq!(
            path_state(&fact, Some(&confirmation), now),
            PathState::Confirmed
        );
        fact.status = KnowledgeStatus::Stale;
        assert_eq!(
            path_state(&fact, Some(&confirmation), now),
            PathState::Stale
        );
    }
}
