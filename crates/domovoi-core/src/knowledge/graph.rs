//! Relations between subjects and the entities the owner invents.

use jiff::{SignedDuration, Timestamp};

use super::fact::{EvidenceRef, FactSource, KnowledgeStatus, snake_enum};
use super::subject::SubjectRef;
use crate::actor::Actor;
use crate::id::{EntityId, RelationId};

snake_enum!(
    /// What an edge between two subjects means.
    RelationKind {
        /// `from` needs `to` to work.
        DependsOn => "depends_on",
        /// `from` runs on `to`.
        RunsOn => "runs_on",
        /// Traffic for `from` is routed to `to`.
        RoutesTo => "routes_to",
        /// `from` keeps its data on `to`.
        StoresOn => "stores_on",
        /// `from` is backed up to `to`.
        BacksUpTo => "backs_up_to",
        /// `from` is managed from `to`.
        ManagedFrom => "managed_from",
    }
);

/// How long a relation stays fresh after it was last observed.
pub const RELATION_STALENESS: SignedDuration = SignedDuration::from_secs(180 * 24 * 3600);

/// One edge between two subjects.
#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    /// The relation's id.
    pub id: RelationId,
    /// The source end; `None` only after the content purge.
    pub from: Option<SubjectRef>,
    /// What the edge means.
    pub kind: RelationKind,
    /// The target end; `None` only after the content purge.
    pub to: Option<SubjectRef>,
    /// Where it came from.
    pub source: FactSource,
    /// The stored status: only current, stale or retracted are ever stored.
    pub status: KnowledgeStatus,
    /// When it was last seen to hold.
    pub observed_at: Timestamp,
    /// What it was derived from.
    pub evidence: Option<EvidenceRef>,
    /// Who wrote it.
    pub actor: Actor,
    /// When it was written.
    pub created_at: Timestamp,
    /// When it was retracted.
    pub retracted_at: Option<Timestamp>,
    /// Who retracted it.
    pub retracted_by: Option<Actor>,
    /// When its endpoints were cleared by the retention purge.
    pub content_purged_at: Option<Timestamp>,
}

impl Relation {
    /// The status a reader should show at `now`: a stored `Current` relation older than
    /// [`RELATION_STALENESS`] is `Stale`.
    #[must_use]
    pub fn effective_status(&self, now: Timestamp) -> KnowledgeStatus {
        let stale = self
            .observed_at
            .checked_add(RELATION_STALENESS)
            .is_ok_and(|until| now >= until);
        if self.status == KnowledgeStatus::Current && stale {
            KnowledgeStatus::Stale
        } else {
            self.status
        }
    }
}

/// A subject the owner invented, such as a VPN or a NAS pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    /// The entity's id.
    pub id: EntityId,
    /// A lowercase slug of 1 to 32 characters from `[a-z0-9_-]`.
    pub kind: String,
    /// A trimmed name of 1 to 80 characters, unique ignoring case.
    pub name: String,
    /// Always the owner who created it.
    pub created_by: Actor,
    /// When it was created.
    pub created_at: Timestamp,
}

/// Whether `kind` is a valid entity kind slug.
#[must_use]
pub fn valid_entity_kind(kind: &str) -> bool {
    (1..=32).contains(&kind.len())
        && kind
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
}

/// A trimmed entity name of 1 to 80 characters, or `None`.
#[must_use]
pub fn clean_entity_name(name: &str) -> Option<String> {
    let name = name.trim();
    (1..=80)
        .contains(&name.chars().count())
        .then(|| name.to_owned())
}
