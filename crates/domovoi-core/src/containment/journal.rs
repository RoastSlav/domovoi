//! The helper's write-ahead journal: record types and the pure line reader.
//!
//! One JSON line per record, written and synced before the `nft` call it describes. A torn
//! final line is ignored and reported; a corrupt line anywhere else makes the journal
//! unreadable and refuses all containment.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// What a journalled intent was about to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalOp {
    /// Step one.
    Apply,
    /// Step two.
    Confirm,
    /// A removal.
    Rollback,
}

/// Why the table was created again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecreateReason {
    /// The layout changed on an empty table.
    LayoutChanged,
    /// The table vanished and was re-created empty.
    TableLost,
    /// The owner ran the maintenance command.
    Maintenance,
}

/// One journal record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rec", rename_all = "snake_case")]
pub enum JournalRecord {
    /// Written before the `nft` call of `op`.
    Intent {
        /// What is about to run.
        op: JournalOp,
        /// The block id, the apply execution id.
        block: String,
        /// The canonical source.
        source: String,
        /// The set the element goes in.
        set: String,
        /// The TTL after `confirm`.
        ttl_s: u32,
        /// When.
        at: Timestamp,
    },
    /// The element is in the set.
    Applied {
        /// The block.
        block: String,
        /// When.
        at: Timestamp,
    },
    /// The element was already there; nothing changed.
    Noop {
        /// The block.
        block: String,
    },
    /// The step failed with a token.
    Failed {
        /// The block.
        block: String,
        /// The wire token.
        token: String,
    },
    /// The element carries its full TTL.
    Confirmed {
        /// The block.
        block: String,
        /// When the element expires.
        deadline: Timestamp,
    },
    /// Written before the rollback batch.
    RollbackIntent {
        /// The block.
        block: String,
        /// When.
        at: Timestamp,
    },
    /// The element is gone because we removed it.
    RolledBack {
        /// The block.
        block: String,
        /// When.
        at: Timestamp,
    },
    /// The table vanished with this block in it.
    Lost {
        /// The block.
        block: String,
        /// When.
        at: Timestamp,
    },
    /// The table was created again.
    TableRecreated {
        /// Why.
        reason: RecreateReason,
        /// When.
        at: Timestamp,
    },
    /// A set member with no journal entry.
    Orphan {
        /// The element text.
        element: String,
    },
}

impl JournalRecord {
    /// The line to append, without the newline.
    ///
    /// # Errors
    /// A serialisation failure, which the record types do not produce.
    pub fn to_line(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// The block the record belongs to, when it has one.
    #[must_use]
    pub fn block(&self) -> Option<&str> {
        match self {
            Self::Intent { block, .. }
            | Self::Applied { block, .. }
            | Self::Noop { block }
            | Self::Failed { block, .. }
            | Self::Confirmed { block, .. }
            | Self::RollbackIntent { block, .. }
            | Self::RolledBack { block, .. }
            | Self::Lost { block, .. } => Some(block),
            Self::TableRecreated { .. } | Self::Orphan { .. } => None,
        }
    }
}

/// A journal that cannot be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("journal line {line} is corrupt")]
pub struct JournalCorrupt {
    /// The 1-based line number.
    pub line: usize,
}

/// What reading a journal gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalRead {
    /// The records, in order.
    pub records: Vec<JournalRecord>,
    /// Whether the final line was torn and ignored.
    pub torn_final_line: bool,
}

/// Reads journal text. Blank lines are skipped.
///
/// # Errors
/// [`JournalCorrupt`] for a bad line that is not the last one.
pub fn read_journal(text: &str) -> Result<JournalRead, JournalCorrupt> {
    let lines: Vec<(usize, &str)> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index + 1, line))
        .collect();
    let mut records = Vec::new();
    let mut torn_final_line = false;
    for (position, (number, line)) in lines.iter().enumerate() {
        match serde_json::from_str::<JournalRecord>(line) {
            Ok(record) => records.push(record),
            Err(_) if position + 1 == lines.len() && !text.ends_with('\n') => {
                torn_final_line = true;
            }
            Err(_) => return Err(JournalCorrupt { line: *number }),
        }
    }
    Ok(JournalRead {
        records,
        torn_final_line,
    })
}
