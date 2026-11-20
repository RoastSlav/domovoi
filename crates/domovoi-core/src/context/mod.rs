//! Read-only context items with provenance and status, and the queries that fetch them.
//!
//! An item carries evidence only. There is no field that can express permission, and an item
//! that came from a past operation says so every time it is rendered.

mod item;
mod query;

pub use item::{
    ContextItem, ContextItemId, ContextLayer, ContextSourceId, ItemStatus, Origin, Provenance,
};
pub use query::{
    ContextBatch, ContextGap, ContextQuery, ContextScope, ContextSelector, MAX_ITEM_BYTES,
    MAX_LIMIT, MIN_ITEM_BYTES,
};

#[cfg(test)]
mod tests;
