//! A fact: one thing Domovoi knows about a subject, with where it came from and how fresh it is.

use std::fmt;
use std::str::FromStr;

use jiff::Timestamp;
use serde_json::Value;

use super::predicate::Predicate;
use super::subject::SubjectRef;
use crate::actor::Actor;
use crate::id::FactRecordId;

macro_rules! snake_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            /// The stable name used in storage and on the wire.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text,)+ }
            }

            /// Reads [`Self::as_str`] back; `None` for anything else.
            #[must_use]
            pub fn parse(text: &str) -> Option<Self> {
                match text { $($text => Some(Self::$variant),)+ _ => None }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::parse(&text).ok_or_else(|| serde::de::Error::custom("unknown value"))
            }
        }
    };
}

pub(super) use snake_enum;

snake_enum!(
    /// Where a fact came from.
    FactSource {
        /// The owner said so.
        User => "user",
        /// An agent or the hub observed it.
        Discovered => "discovered",
        /// An integration reported it.
        Integration => "integration",
        /// A model or an operation derived it.
        Inferred => "inferred",
        /// It was imported from earlier data.
        Imported => "imported",
    }
);

snake_enum!(
    /// Where a fact is in its life.
    KnowledgeStatus {
        /// Believed and fresh.
        Current => "current",
        /// Past its freshness period.
        Stale => "stale",
        /// Contradicted by a live fact from another source.
        Disputed => "disputed",
        /// Replaced by a newer fact.
        Superseded => "superseded",
        /// Withdrawn.
        Retracted => "retracted",
    }
);

impl KnowledgeStatus {
    /// Current, stale and disputed facts are live: they can be read and corrected.
    #[must_use]
    pub const fn is_live(self) -> bool {
        matches!(self, Self::Current | Self::Stale | Self::Disputed)
    }
}

snake_enum!(
    /// What an evidence reference points at.
    EvidenceKind {
        /// A note.
        Note => "note",
        /// An incident.
        Incident => "incident",
        /// A signal.
        Signal => "signal",
        /// An action.
        Action => "action",
        /// An operation.
        Operation => "operation",
        /// An operation step.
        Step => "step",
    }
);

/// A pointer from a fact to what it was derived from. Checked for shape only; the target is
/// not looked up.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EvidenceRef {
    /// What kind of record is referred to.
    pub kind: EvidenceKind,
    /// The record's id.
    pub id: uuid::Uuid,
}

impl EvidenceRef {
    /// A reference to a record.
    #[must_use]
    pub fn new(kind: EvidenceKind, id: uuid::Uuid) -> Self {
        Self { kind, id }
    }
}

/// Why evidence text was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the evidence reference is not valid")]
pub struct EvidenceError;

impl fmt::Display for EvidenceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.as_str(), self.id.hyphenated())
    }
}

impl FromStr for EvidenceRef {
    type Err = EvidenceError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (kind, id) = text.split_once(':').ok_or(EvidenceError)?;
        if text.len() > 64 || id.len() != 36 || id.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(EvidenceError);
        }
        Ok(Self {
            kind: EvidenceKind::parse(kind).ok_or(EvidenceError)?,
            id: id.parse().map_err(|_| EvidenceError)?,
        })
    }
}

impl serde::Serialize for EvidenceRef {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for EvidenceRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// One stored fact.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    /// The fact's id.
    pub id: FactRecordId,
    /// What it is about.
    pub subject: SubjectRef,
    /// What it says.
    pub predicate: Predicate,
    /// The canonical value; `None` only after the retention purge.
    pub value: Option<Value>,
    /// Where it came from.
    pub source: FactSource,
    /// The stored status; see [`Fact::effective_status`] for what readers should use.
    pub status: KnowledgeStatus,
    /// When it was last seen to hold.
    pub observed_at: Timestamp,
    /// After this instant the fact no longer holds.
    pub valid_until: Option<Timestamp>,
    /// When the owner or a later observation confirmed it.
    pub last_verified_at: Option<Timestamp>,
    /// The fact this one replaced.
    pub supersedes: Option<FactRecordId>,
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
    /// When its content was cleared by the retention purge.
    pub content_purged_at: Option<Timestamp>,
}

impl Fact {
    /// The instant the fact stops being fresh: the earlier of `valid_until` and the
    /// predicate's staleness period after the last verification or observation.
    #[must_use]
    pub fn fresh_until(&self) -> Option<Timestamp> {
        let base = self.last_verified_at.unwrap_or(self.observed_at);
        let stale = base.checked_add(self.predicate.staleness()).ok();
        match (stale, self.valid_until) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The status a reader should show at `now`: a stored `Current` fact past its freshness
    /// is `Stale`; every other stored status is returned unchanged.
    #[must_use]
    pub fn effective_status(&self, now: Timestamp) -> KnowledgeStatus {
        if self.status == KnowledgeStatus::Current
            && self.fresh_until().is_some_and(|until| now >= until)
        {
            KnowledgeStatus::Stale
        } else {
            self.status
        }
    }

    /// Whether a safety decision may rely on the fact: safety-critical, owner-sourced,
    /// effectively current and with its content intact.
    #[must_use]
    pub fn usable_for_safety(&self, now: Timestamp) -> bool {
        self.predicate.is_safety_critical()
            && self.source == FactSource::User
            && self.effective_status(now) == KnowledgeStatus::Current
            && self.value.is_some()
    }
}

/// Which actors may write which source.
#[must_use]
pub fn source_allows_actor(source: FactSource, actor: &Actor) -> bool {
    match (source, actor) {
        (_, Actor::Policy(_)) => false,
        (FactSource::User, actor) => matches!(actor, Actor::User(_)),
        (FactSource::Discovered, actor) => matches!(actor, Actor::System(_) | Actor::Agent(_)),
        (FactSource::Integration | FactSource::Inferred, actor) => {
            matches!(actor, Actor::System(_))
        }
        (FactSource::Imported, actor) => matches!(actor, Actor::System(_) | Actor::User(_)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::id::{HostId, UserId};

    fn ts(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).unwrap()
    }

    fn fact(predicate: Predicate, source: FactSource, value: Value) -> Fact {
        Fact {
            id: FactRecordId::new(),
            subject: SubjectRef::Host(HostId::new()),
            predicate,
            value: Some(value),
            source,
            status: KnowledgeStatus::Current,
            observed_at: ts(0),
            valid_until: None,
            last_verified_at: None,
            supersedes: None,
            evidence: None,
            actor: Actor::User(UserId::new()),
            created_at: ts(0),
            retracted_at: None,
            retracted_by: None,
            content_purged_at: None,
        }
    }

    const DAY: i64 = 86_400;

    #[test]
    fn effective_status_turns_stale_after_the_staleness_period() {
        let mut f = fact(Predicate::Role, FactSource::User, json!("x"));
        assert_eq!(f.effective_status(ts(179 * DAY)), KnowledgeStatus::Current);
        assert_eq!(f.effective_status(ts(180 * DAY)), KnowledgeStatus::Stale);
        f.last_verified_at = Some(ts(100 * DAY));
        assert_eq!(f.effective_status(ts(180 * DAY)), KnowledgeStatus::Current);
        f.status = KnowledgeStatus::Disputed;
        assert_eq!(f.effective_status(ts(900 * DAY)), KnowledgeStatus::Disputed);
    }

    #[test]
    fn valid_until_shortens_freshness() {
        let mut f = fact(Predicate::Role, FactSource::User, json!("x"));
        f.valid_until = Some(ts(10 * DAY));
        assert_eq!(f.fresh_until(), Some(ts(10 * DAY)));
        assert_eq!(f.effective_status(ts(10 * DAY)), KnowledgeStatus::Stale);
    }

    #[test]
    fn usable_for_safety_needs_user_source_current_and_safety_critical() {
        let path = json!({"cidr": "10.0.0.0/8"});
        let mut f = fact(Predicate::ManagementPath, FactSource::User, path.clone());
        assert!(f.usable_for_safety(ts(DAY)));
        assert!(!f.usable_for_safety(ts(31 * DAY)));
        f.source = FactSource::Discovered;
        assert!(!f.usable_for_safety(ts(DAY)));
        f.source = FactSource::Inferred;
        assert!(!f.usable_for_safety(ts(DAY)));
        let mut cidr = fact(Predicate::TrustedCidr, FactSource::User, path);
        assert!(cidr.usable_for_safety(ts(DAY)));
        cidr.value = None;
        assert!(!cidr.usable_for_safety(ts(DAY)));
        let role = fact(Predicate::Role, FactSource::User, json!("x"));
        assert!(!role.usable_for_safety(ts(DAY)));
    }

    #[test]
    fn source_allows_actor_table() {
        let user = Actor::User(UserId::new());
        let system = Actor::System("knowledge".into());
        let agent = Actor::Agent(HostId::new());
        let policy = Actor::Policy("r".into());
        use FactSource::*;
        let table = [
            (User, [true, false, false, false]),
            (Discovered, [false, true, true, false]),
            (Integration, [false, true, false, false]),
            (Inferred, [false, true, false, false]),
            (Imported, [true, true, false, false]),
        ];
        for (source, expected) in table {
            let got = [&user, &system, &agent, &policy].map(|a| source_allows_actor(source, a));
            assert_eq!(got, expected, "{source:?}");
        }
    }

    #[test]
    fn evidence_ref_round_trips_and_is_shape_checked() {
        let id = uuid::Uuid::now_v7();
        let r = EvidenceRef::new(EvidenceKind::Note, id);
        assert_eq!(r.to_string().parse::<EvidenceRef>().unwrap(), r);
        assert!(format!("thing:{id}").parse::<EvidenceRef>().is_err());
        assert!("note:short".parse::<EvidenceRef>().is_err());
    }
}
