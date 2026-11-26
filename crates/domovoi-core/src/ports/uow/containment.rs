//! The state writes of containment blocks, applied inside a
//! [`Transaction`](super::Transaction).

use crate::id::ExecutionId;
use crate::ports::{BlockRow, PhaseGuard, PhaseUpdate};

/// One change to a containment block. Built only by the containment driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainmentWrite {
    /// Stores a new block at version 1. A second row for the same block id is a conflict;
    /// a confirm or rollback execution id already used elsewhere is
    /// [`CommitError::Constraint`](super::CommitError::Constraint).
    Create(BlockRow),
    /// Moves a block whose stored phase and version are as guarded; a mismatch is a conflict
    /// and the caller re-reads. The store bumps `version`.
    Phase {
        /// The block.
        block: ExecutionId,
        /// What the stored block must look like.
        expected: PhaseGuard,
        /// What to change.
        update: PhaseUpdate,
    },
}
