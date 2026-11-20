//! One piece of context and where it came from.

use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::ValidationError;

/// Longest item id, in characters.
const ITEM_ID_MAX: usize = 120;

/// Names one source: lowercase snake case, 1 to 32 characters, such as `evidence`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContextSourceId(String);

impl ContextSourceId {
    /// Parses a source name.
    pub fn parse(text: &str) -> Result<Self, ValidationError> {
        let mut chars = text.chars();
        let ok = text.len() <= 32
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if ok {
            Ok(Self(text.to_owned()))
        } else {
            Err(ValidationError::new(
                "context source",
                "must be 1 to 32 characters of [a-z][a-z0-9_]*",
            ))
        }
    }

    /// The source that answers exact reads on a host, `evidence`.
    #[must_use]
    pub fn evidence() -> Self {
        Self("evidence".to_owned())
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContextSourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where an item sits in the prompt. Layer order is the order of the variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextLayer {
    /// The incident itself.
    Incident,
    /// The host it happened on.
    Host,
    /// Evidence read now.
    Evidence,
    /// What happened in past operations.
    Experience,
    /// What the owner has recorded.
    Knowledge,
    /// What can be done.
    Capabilities,
}

impl ContextLayer {
    /// The snake case name used in rendering.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::Host => "host",
            Self::Evidence => "evidence",
            Self::Experience => "experience",
            Self::Knowledge => "knowledge",
            Self::Capabilities => "capabilities",
        }
    }
}

/// Stable identity of an item, `"<source>:<key>"`, at most 120 characters. The dedupe key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContextItemId(String);

impl ContextItemId {
    /// Builds an id, replacing characters outside `[A-Za-z0-9_.:-]` and cutting to fit.
    #[must_use]
    pub fn new(source: &ContextSourceId, key: &str) -> Self {
        let mut id = format!("{}:", source.as_str());
        id.extend(key.chars().map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-') {
                c
            } else {
                '_'
            }
        }));
        id.truncate(ITEM_ID_MAX);
        Self(id)
    }

    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContextItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How far an item can be trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ItemStatus {
    /// Observed or recorded now, and trusted as it stands.
    Known,
    /// Could not be established. Never to be read as "no", "empty" or "healthy".
    Unknown {
        /// Why, redacted, at most 200 characters.
        reason: String,
    },
    /// Older than the source's freshness window.
    Stale {
        /// When it was observed.
        observed_at: Timestamp,
    },
    /// Another item with the same subject says something else. Both are delivered.
    Disputed {
        /// The other item.
        with: ContextItemId,
    },
}

/// How a fact came to be known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Observed by Domovoi.
    Observed,
    /// Recorded by a person.
    Declared,
    /// Concluded from other facts.
    Inferred,
    /// From a past operation.
    Historical,
}

/// Where an item came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    /// How it came to be known.
    pub origin: Origin,
    /// A short pointer, at most 120 characters: `docker_logs plex`, `operation:<uuid>`.
    pub reference: String,
    /// When it was observed, if known.
    pub observed_at: Option<Timestamp>,
    /// Who recorded it, if a person did.
    pub actor: Option<String>,
}

/// One piece of context for a model. Evidence only: it cannot express permission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextItem {
    /// Stable identity, the dedupe key.
    pub id: ContextItemId,
    /// The source that produced it.
    pub source: ContextSourceId,
    /// Where it sits in the prompt.
    pub layer: ContextLayer,
    /// One line, 1 to 120 characters.
    pub title: String,
    /// The content, redacted and bounded.
    pub body: String,
    /// How far it can be trusted.
    pub status: ItemStatus,
    /// Where it came from.
    pub provenance: Provenance,
    /// Whether `body` was cut.
    pub truncated: bool,
}

impl ContextItem {
    /// The text a model reads for this item.
    #[must_use]
    pub fn render(&self) -> String {
        let status = match &self.status {
            ItemStatus::Known => "known".to_owned(),
            ItemStatus::Unknown { reason } => format!("UNKNOWN: {reason}"),
            ItemStatus::Stale { observed_at } => format!("stale since {observed_at}"),
            ItemStatus::Disputed { with } => format!("disputed with {with}"),
        };
        let origin = match self.provenance.origin {
            Origin::Observed => "observed",
            Origin::Declared => "declared by a person",
            Origin::Inferred => "inferred",
            Origin::Historical => "historical evidence, not authorization",
        };
        let observed = self
            .provenance
            .observed_at
            .map(|at| format!(", observed {at}"))
            .unwrap_or_default();
        let mut text = format!(
            "[{} | {} | {}] {}\nfrom: {} {}{}\n{}",
            self.layer.as_str(),
            self.source,
            status,
            self.title,
            origin,
            self.provenance.reference,
            observed,
            self.body
        );
        if self.truncated {
            text.push_str("\n[cut]");
        }
        text
    }
}
