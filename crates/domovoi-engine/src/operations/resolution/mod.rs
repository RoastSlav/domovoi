//! Resolution summaries of finished operations: what Domovoi fills in, what the model
//! drafted from its own report, and what the owner edited or confirmed.
//!
//! Every revision is written by the operation writer. The pure parts are [`plan_draft`] and
//! [`plan_edit`]; the effect handler, the start-up backfill and the writer commands around
//! them do the I/O.

mod backfill;
mod handler;
mod narrative;
mod plan;

use domovoi_core::experience::{SummaryField, SummaryRevision};

pub(super) use backfill::queue_missing;
pub(super) use handler::DraftResolutionHandler;
pub(crate) use plan::{EditPlan, PlannedRevision, plan_edit};

use super::error::OperationError;

/// The longest text of a narrative field, in characters.
pub const MAX_TEXT_CHARS: usize = domovoi_core::experience::MAX_NARRATIVE_CHARS;

/// An owner's edit of one narrative field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSummaryField {
    /// The field to change; only the narrative ones accept an edit.
    pub field: SummaryField,
    /// The new text.
    pub value: String,
    /// The field's newest revision when the person opened it, 0 when it had none.
    pub expected_revision: u32,
}

/// What an edit did.
#[derive(Debug, Clone, PartialEq)]
pub struct EditedField {
    /// The revision that now holds the text: the new one, or the existing one when nothing
    /// changed.
    pub revision: SummaryRevision,
    /// Whether the text was already held by a revision the owner wrote, so nothing was added.
    pub unchanged: bool,
}

/// Why a summary edit was refused. Nothing is committed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SummaryError {
    /// The field name is not one of the summary fields.
    #[error("that is not a summary field")]
    UnknownField,
    /// Domovoi fills this field itself.
    #[error("this field is filled by Domovoi and cannot be edited")]
    CodeOwned(SummaryField),
    /// The operation is still open.
    #[error("the operation has not finished")]
    NotTerminal,
    /// The field has a newer revision than the one the person saw.
    #[error("the summary changed since you opened it")]
    Stale {
        /// The revision the person saw.
        expected: u32,
        /// The newest revision.
        actual: u32,
    },
    /// The text is empty.
    #[error("write something: an empty value is not accepted")]
    Empty,
    /// The text is too long.
    #[error("the text is longer than {max} characters")]
    TooLong {
        /// The limit.
        max: usize,
    },
    /// The text holds a control character other than a newline.
    #[error("the text contains characters that are not allowed")]
    BadText,
}

/// Why a summary edit failed: the edit was refused, or the operation service was.
#[derive(Debug, thiserror::Error)]
pub enum EditSummaryError {
    /// The edit was refused.
    #[error("{0}")]
    Summary(#[from] SummaryError),
    /// The operation service failed or the operation is unknown.
    #[error("{0}")]
    Operation(#[from] OperationError),
}
