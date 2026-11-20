//! Asking a context source a question.

use jiff::Timestamp;

use super::item::{ContextItem, ContextSourceId};
use crate::id::{HostId, IncidentId, OperationId};
use crate::triage::ReadToolCall;

/// Most items one query may ask for.
pub const MAX_LIMIT: u16 = 50;
/// Smallest allowed item body bound.
pub const MIN_ITEM_BYTES: u32 = 256;
/// Largest allowed item body bound.
pub const MAX_ITEM_BYTES: u32 = 8192;

/// What a query is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextScope {
    /// One host.
    Host(HostId),
    /// One incident on a host.
    Incident {
        /// The host.
        host: HostId,
        /// The incident.
        incident: IncidentId,
    },
    /// One operation on a host.
    Operation {
        /// The host.
        host: HostId,
        /// The operation.
        operation: OperationId,
    },
}

/// What to fetch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextSelector {
    /// What the source holds about the scope right now, newest first, within the limit.
    Overview,
    /// Free-text search inside the scope; 1 to 200 characters, redacted by the caller.
    Search {
        /// The search text.
        text: String,
    },
    /// One exact read.
    Read {
        /// The read to perform.
        call: ReadToolCall,
    },
}

/// One question to a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextQuery {
    /// What it is about.
    pub scope: ContextScope,
    /// What to fetch.
    pub selector: ContextSelector,
    /// Most items to return, 1 to [`MAX_LIMIT`].
    pub limit: u16,
    /// Longest item body, [`MIN_ITEM_BYTES`] to [`MAX_ITEM_BYTES`].
    pub max_item_bytes: u32,
    /// Nothing runs after this instant.
    pub deadline: Timestamp,
}

/// A question a source could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextGap {
    /// The source that could not answer.
    pub source: ContextSourceId,
    /// Why, redacted, at most 200 characters.
    pub reason: String,
}

/// What a source answered: items and gaps. No item and no gap means a known empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContextBatch {
    /// The items, at most the query's limit.
    pub items: Vec<ContextItem>,
    /// What could not be answered.
    pub gaps: Vec<ContextGap>,
}
