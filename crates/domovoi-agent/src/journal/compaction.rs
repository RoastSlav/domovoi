//! The durable compaction transaction: an intent that proves which shorter journal is
//! legitimate, and its idempotent startup recovery.
//!
//! The intent is durably written before the journal is replaced and binds both identities:
//! the old committed length and digest, and the expected compacted length and digest, plus
//! the epoch and a transaction id. Recovery adopts a state only when the visible journal
//! matches one side exactly and the boundary can be durably established.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use super::JOURNAL_FILE;
use super::commit;
use super::meta::Metadata;
use super::record;
use crate::error::{JournalError, StateError};
use crate::fs_atomic::{DurableFaults, DurableWriteError};

/// The durable compaction transaction record.
pub(super) const INTENT_FILE: &str = "action-journal.compact";
/// The compaction schema this build writes and accepts.
pub(super) const INTENT_FORMAT: u32 = 1;
/// The temporary compacted journal.
pub(super) const TEMP_FILE: &str = "action-journal.bin.tmp";

/// Every temporary name a reset must not leave behind.
pub(super) const TEMP_FILES: [&str; 4] = [
    TEMP_FILE,
    ".action-journal.json.tmp",
    ".action-journal.commit.tmp",
    ".action-journal.compact.tmp",
];

/// Both sides of one compaction, bound to the epoch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CompactionIntent {
    /// The intent schema version.
    pub(super) format: u32,
    /// A fresh identity for this transaction, so a stale record is recognisable.
    pub(super) transaction: Uuid,
    /// The journal epoch this compaction belongs to.
    pub(super) epoch: Uuid,
    /// The committed length before compaction.
    pub(super) old_len: u64,
    /// The SHA-256 of the complete journal before compaction, as lowercase hex.
    pub(super) old_digest: String,
    /// The expected compacted length.
    pub(super) new_len: u64,
    /// The SHA-256 of the complete compacted journal, as lowercase hex.
    pub(super) new_digest: String,
}

/// The SHA-256 of a complete file, as lowercase hex.
pub(super) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(super) fn exists(dir: &Path) -> bool {
    dir.join(INTENT_FILE).exists()
}

/// The intent, or any temporary file, is state a fresh initialization must not adopt.
pub(super) fn leftovers_exist(dir: &Path) -> bool {
    exists(dir) || TEMP_FILES.iter().any(|name| dir.join(name).exists())
}

/// Reads the intent, refusing an unexpected format or a nil identity.
pub(super) fn read(dir: &Path) -> Result<Option<CompactionIntent>, JournalError> {
    let path = dir.join(INTENT_FILE);
    let Some(bytes) = crate::fs_atomic::read_optional(&path)? else {
        return Ok(None);
    };
    let intent: CompactionIntent = serde_json::from_slice(&bytes)
        .map_err(|err| JournalError::Corrupt(format!("{INTENT_FILE}: {err}")))?;
    if intent.format != INTENT_FORMAT {
        return Err(JournalError::Corrupt(format!(
            "{INTENT_FILE}: unsupported format {}",
            intent.format
        )));
    }
    if intent.transaction.is_nil() || intent.epoch.is_nil() {
        return Err(JournalError::Corrupt(format!(
            "{INTENT_FILE}: a nil transaction or epoch"
        )));
    }
    if intent.old_digest.len() != 64 || intent.new_digest.len() != 64 {
        return Err(JournalError::Corrupt(format!(
            "{INTENT_FILE}: a digest is not 32 bytes of hex"
        )));
    }
    Ok(Some(intent))
}

/// Durably writes the intent before the original journal may be replaced.
pub(super) fn write(
    dir: &Path,
    intent: &CompactionIntent,
    faults: DurableFaults,
) -> Result<(), DurableWriteError> {
    let path = dir.join(INTENT_FILE);
    let bytes = serde_json::to_vec(intent)
        .map_err(|err| DurableWriteError::BeforeCommit(StateError::new(&path, err)))?;
    crate::fs_atomic::write_atomic_durable_with(&path, &bytes, 0o600, faults)
}

/// Removes the intent and syncs the directory so the removal is durable.
pub(super) fn remove(dir: &Path) -> Result<(), JournalError> {
    let path = dir.join(INTENT_FILE);
    match std::fs::remove_file(&path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            return Err(JournalError::State(StateError::new(&path, err)));
        }
        _ => {}
    }
    crate::fs_atomic::sync_dir(dir).map_err(|err| StateError::new(dir, err))?;
    Ok(())
}

pub(super) fn temp_path(dir: &Path) -> PathBuf {
    dir.join(TEMP_FILE)
}

fn sync_journal(dir: &Path) -> Result<(), JournalError> {
    let path = dir.join(JOURNAL_FILE);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| file.sync_all())
        .map_err(|err| StateError::new(&path, err))?;
    Ok(())
}

/// Resolves an interrupted compaction before startup applies the ordinary boundary rules.
///
/// Returns the length the commit boundary must durably name. On any mismatch the intent is
/// kept and an error is returned, so the journal never opens with a guessed state.
pub(super) fn recover(dir: &Path, meta: &Metadata, committed: u64) -> Result<u64, JournalError> {
    let Some(intent) = read(dir)? else {
        return Ok(committed);
    };
    if intent.epoch != meta.epoch {
        return Err(JournalError::Corrupt(format!(
            "{INTENT_FILE} belongs to another epoch; keeping it"
        )));
    }
    let path = dir.join(JOURNAL_FILE);
    let Some(bytes) = crate::fs_atomic::read_optional(&path)? else {
        return Err(JournalError::Corrupt(format!(
            "{INTENT_FILE} exists but the journal is missing"
        )));
    };
    let actual = bytes.len() as u64;
    let actual_digest = digest(&bytes);
    let is_new = actual == intent.new_len && actual_digest == intent.new_digest;
    let is_old = actual == intent.old_len && actual_digest == intent.old_digest;

    if is_new {
        // A compacted file is present: prove it is complete and lifecycle-valid, make its
        // name and contents durable, then establish the boundary that names it.
        let parsed = record::parse(&bytes, meta.epoch).map_err(|err| {
            JournalError::Corrupt(format!("the compacted journal does not validate: {err}"))
        })?;
        if parsed.valid_len != actual {
            return Err(JournalError::Corrupt(
                "the compacted journal does not end on a record boundary".to_owned(),
            ));
        }
        sync_journal(dir)?;
        crate::fs_atomic::sync_dir(dir).map_err(|err| StateError::new(dir, err))?;
        let target = match committed {
            c if c == intent.new_len => c,
            c if c == intent.old_len => {
                commit::write(dir, intent.new_len, DurableFaults::default())
                    .map_err(|err| StateError::new(dir.join(commit::COMMIT_FILE), err))?;
                intent.new_len
            }
            other => {
                return Err(JournalError::Corrupt(format!(
                    "{INTENT_FILE} names lengths {} -> {}, but the boundary is {other}",
                    intent.old_len, intent.new_len
                )));
            }
        };
        commit::reconcile(dir, target)?;
        remove(dir)?;
        return Ok(target);
    }
    if is_old {
        // The original committed journal is still on disk: abort the transaction rather than
        // discarding committed records. The boundary must still name the old length.
        if committed != intent.old_len {
            return Err(JournalError::Corrupt(format!(
                "{INTENT_FILE} names {} bytes, but the boundary is {committed}",
                intent.old_len
            )));
        }
        commit::reconcile(dir, intent.old_len)?;
        let _ = std::fs::remove_file(temp_path(dir));
        remove(dir)?;
        return Ok(intent.old_len);
    }
    Err(JournalError::Corrupt(format!(
        "{INTENT_FILE} matches neither the compacted {new} bytes nor the original {old} bytes, \
         but the journal is {actual}",
        new = intent.new_len,
        old = intent.old_len
    )))
}
