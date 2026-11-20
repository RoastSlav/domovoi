//! Editing the narrative of a finished operation's summary.
//!
//! The operation writer is the only thing that writes a summary revision; this layer decides
//! who is asking and what is sent. Reading summaries stays a plain read.

use domovoi_core::OperationId;
use domovoi_core::experience::SummaryField;
use domovoi_engine::operations::{EditSummaryError, EditSummaryField, EditedField};

use super::OperationCommands;
use crate::services::Caller;

/// Why a summary edit was refused.
#[derive(Debug, thiserror::Error)]
pub enum SummaryCommandError {
    /// This hub runs no operation service.
    #[error("orchestration is not enabled")]
    NotEnabled,
    /// The edit was refused, or the operation service failed.
    #[error("{0}")]
    Edit(#[from] EditSummaryError),
}

impl OperationCommands {
    /// Edits or confirms a narrative field of `op`'s summary. Writing the text a model drafted
    /// confirms it; code-owned fields refuse an edit. `expected_revision` is the field's newest
    /// revision when the person opened it, 0 when it had none.
    pub async fn edit_summary_field(
        &self,
        caller: &Caller,
        op: OperationId,
        field: SummaryField,
        value: &str,
        expected_revision: u32,
    ) -> Result<EditedField, SummaryCommandError> {
        let handle = self
            .handle
            .as_ref()
            .ok_or(SummaryCommandError::NotEnabled)?;
        let edit = EditSummaryField {
            field,
            value: value.to_owned(),
            expected_revision,
        };
        Ok(handle.edit_summary_field(op, edit, caller.actor()).await?)
    }
}
