//! The state writes of owner-made decisions that live outside an aggregate: known conditions
//! and plain settings. They go through a [`Transaction`](super::Transaction) so the audit row
//! of the decision commits with it or not at all.

use crate::AckRule;
use crate::id::AckRuleId;

/// A write to the known conditions or the settings store.
#[derive(Debug, Clone)]
pub enum AdminWrite {
    /// Inserts a known condition or replaces the one with the same id.
    PutAckRule(AckRule),
    /// Removes a known condition. A missing one is
    /// [`CommitError::Constraint`](super::CommitError::Constraint).
    DeleteAckRule(AckRuleId),
    /// Sets (`Some`) or removes (`None`) one settings key.
    Setting {
        /// The settings key.
        key: String,
        /// The new value, or `None` to remove the key.
        value: Option<serde_json::Value>,
    },
}
