//! The fields of a resolution summary and who may write each.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::id::OperationId;

/// One field of a resolution summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryField {
    /// How the operation ended.
    Outcome,
    /// Whether a fix was verified.
    Verified,
    /// What the operation was pinned to.
    Subjects,
    /// The capability contracts that were actually run.
    ContractsExecuted,
    /// Every host the operation touched.
    Hosts,
    /// What was wrong, in the investigator's words.
    Problem,
    /// Why it was wrong.
    Cause,
    /// What fixed it, or what should be done.
    Fix,
}

impl SummaryField {
    /// Every field, in the order readers list them.
    pub const ALL: [Self; 8] = [
        Self::Outcome,
        Self::Verified,
        Self::Subjects,
        Self::ContractsExecuted,
        Self::Hosts,
        Self::Problem,
        Self::Cause,
        Self::Fix,
    ];

    /// The stored snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Outcome => "outcome",
            Self::Verified => "verified",
            Self::Subjects => "subjects",
            Self::ContractsExecuted => "contracts_executed",
            Self::Hosts => "hosts",
            Self::Problem => "problem",
            Self::Cause => "cause",
            Self::Fix => "fix",
        }
    }

    /// Reads [`SummaryField::as_str`] back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.as_str() == text)
    }

    /// Whether only Domovoi's own code fills this field.
    #[must_use]
    pub const fn is_code_owned(self) -> bool {
        matches!(
            self,
            Self::Outcome | Self::Verified | Self::Subjects | Self::ContractsExecuted | Self::Hosts
        )
    }

    /// Whether the field is free text a model drafts and a person may edit.
    #[must_use]
    pub const fn is_narrative(self) -> bool {
        !self.is_code_owned()
    }
}

/// Who wrote a revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummarySource {
    /// Domovoi's own code, from the operation's typed steps.
    Code,
    /// A model, from its own committed report.
    Model,
    /// A person.
    User,
}

impl SummarySource {
    /// The stored snake_case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Model => "model",
            Self::User => "user",
        }
    }

    /// Reads [`SummarySource::as_str`] back.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        [Self::Code, Self::Model, Self::User]
            .into_iter()
            .find(|source| source.as_str() == text)
    }

    /// Whether this source may write `field`: code writes code-owned fields, a model or a
    /// person writes narrative ones.
    #[must_use]
    pub const fn may_write(self, field: SummaryField) -> bool {
        match self {
            Self::Code => field.is_code_owned(),
            Self::Model | Self::User => field.is_narrative(),
        }
    }
}

/// One immutable revision of one field.
///
/// `value` is canonical JSON: `outcome` a string, `verified` a bool, `subjects`,
/// `contracts_executed` and `hosts` arrays, a narrative field a string of 1 to 1000
/// characters. Text is already redacted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryRevision {
    /// The operation the summary belongs to.
    pub operation: OperationId,
    /// The field.
    pub field: SummaryField,
    /// 1-based and dense per (operation, field).
    pub revision: u32,
    /// The value.
    pub value: serde_json::Value,
    /// Who wrote it.
    pub source: SummarySource,
    /// The actor in string form, for example `system:operations`.
    pub actor: String,
    /// When it was written.
    pub created_at: Timestamp,
}
