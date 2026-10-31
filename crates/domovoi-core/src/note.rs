//! Runbook notes, searchable by triage.

use jiff::Timestamp;

use crate::id::{HostId, NoteId};

/// Markdown runbook text for one host, or global when `host` is `None`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Note {
    /// Unique id.
    pub id: NoteId,
    /// The host it is about, or `None` for a global note.
    pub host: Option<HostId>,
    /// Title.
    pub title: String,
    /// Body in Markdown.
    pub body_markdown: String,
    /// Free-form tags for search.
    pub tags: Vec<String>,
    /// When it last changed.
    pub updated_at: Timestamp,
}
