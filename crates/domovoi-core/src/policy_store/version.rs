//! Policy version headers, stored rules and acknowledgements.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::actor::Actor;
use crate::id::PolicyVersionId;

/// Whether a policy version can still change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyStatus {
    /// The one editable rule set.
    Draft,
    /// An immutable, numbered version.
    Published,
}

impl PolicyStatus {
    /// The stored name: `"draft"` or `"published"`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
        }
    }

    /// Reads [`PolicyStatus::as_str`] back; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            _ => None,
        }
    }
}

/// One policy version's header. Rules are read separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyVersionRecord {
    /// The version's id.
    pub id: PolicyVersionId,
    /// Draft or published.
    pub status: PolicyStatus,
    /// `Some(1..)` exactly when published; never reused.
    pub number: Option<u32>,
    /// Starts at 1 and grows with every draft save; frozen at publication.
    pub revision: u64,
    /// The active version number when the draft was first created.
    pub based_on: Option<u32>,
    /// 64 lowercase hex characters: the hash of the stored rule bodies.
    pub rules_hash: String,
    /// How many rules the version holds.
    pub rule_count: u32,
    /// How many of them are locked.
    pub locked_count: u32,
    /// How many of them are shadow rules.
    pub shadow_count: u32,
    /// When the draft was first created.
    pub created_at: Timestamp,
    /// Who created it.
    pub created_by: Actor,
    /// When it last changed.
    pub updated_at: Timestamp,
    /// Who changed it last.
    pub updated_by: Actor,
    /// When it was published.
    pub published_at: Option<Timestamp>,
    /// Who published it.
    pub published_by: Option<Actor>,
}

/// One rule as stored: projections for filtering plus the opaque envelope text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRule {
    /// The rule's id, unique within a version.
    pub rule_id: String,
    /// Position in the version, from 0.
    pub ordinal: u32,
    /// Whether the rule is switched on.
    pub enabled: bool,
    /// Whether the rule is a shadow rule.
    pub shadow: bool,
    /// Whether the rule is a locked deny.
    pub locked: bool,
    /// The versioned envelope text holding the rule.
    pub body: String,
}

/// The owner's acknowledgement of one blocking publication finding, stored with the version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckRecord {
    /// The finding's stable key.
    pub finding_key: String,
    /// The finding's kind.
    pub kind: String,
    /// The first rule involved.
    pub first_rule: String,
    /// The second rule involved.
    pub second_rule: String,
    /// How the evaluator resolves the conflict, if it does.
    pub resolution: Option<String>,
    /// The finding's note at acknowledgement time.
    pub note: String,
    /// Who acknowledged.
    pub acknowledged_by: Actor,
    /// When.
    pub acknowledged_at: Timestamp,
}
