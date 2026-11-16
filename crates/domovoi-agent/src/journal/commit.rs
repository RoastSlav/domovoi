//! The journal's durable commit boundary: how many bytes of the append-only file are
//! acknowledged.
//!
//! The boundary file is written before the journal exists and replaced only after the bytes
//! it names have been synced. Bytes past it were never acknowledged, so startup truncates
//! them instead of adopting a complete frame that merely happens to parse.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::JOURNAL_FILE;
use crate::error::{JournalError, StateError};
use crate::fs_atomic::{DurableFaults, DurableWriteError};

/// The durable committed-length boundary, replaced atomically after every acknowledged
/// append.
pub(super) const COMMIT_FILE: &str = "action-journal.commit";

#[derive(Debug, Serialize, Deserialize)]
struct CommitPoint {
    committed_len: u64,
}

/// Whether a commit boundary already exists.
pub(super) fn exists(dir: &Path) -> bool {
    dir.join(COMMIT_FILE).exists()
}

/// Reads the committed length. A missing, unreadable or malformed boundary fails closed:
/// without it there is no durable statement about which bytes were acknowledged.
pub(super) fn load(dir: &Path) -> Result<u64, JournalError> {
    let path = dir.join(COMMIT_FILE);
    let Some(bytes) = crate::fs_atomic::read_optional(&path)? else {
        return Err(JournalError::Corrupt(format!(
            "{COMMIT_FILE} is missing; the journal has no durable commit boundary"
        )));
    };
    let point: CommitPoint = serde_json::from_slice(&bytes)
        .map_err(|err| JournalError::Corrupt(format!("{COMMIT_FILE}: {err}")))?;
    Ok(point.committed_len)
}

/// Durably records `len` as the committed journal length.
///
/// The temporary file is synced, renamed, and the parent directory is synced, so a success
/// result is the commit point for every append.
pub(super) fn write(dir: &Path, len: u64, faults: DurableFaults) -> Result<(), DurableWriteError> {
    let path = dir.join(COMMIT_FILE);
    let bytes = serde_json::to_vec(&CommitPoint { committed_len: len })
        .map_err(|err| DurableWriteError::BeforeCommit(StateError::new(&path, err)))?;
    crate::fs_atomic::write_atomic_durable_with(&path, &bytes, 0o600, faults)
}

/// Stages of a boundary reconciliation a test can make fail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ReconcileFaults {
    /// Fail syncing the boundary file.
    pub(super) file_sync: bool,
    /// Fail syncing the parent directory.
    pub(super) dir_sync: bool,
}

/// Confirms a visible boundary, returning its durable length.
///
/// Reads the visible value, syncs the boundary file and its directory entry, reloads, and
/// requires the confirmed value to be exactly `expected`. Syncing is the confirmation: a
/// successful read of a visible value is never treated as proof of durability.
pub(super) fn reconcile(dir: &Path, expected: u64) -> Result<u64, JournalError> {
    reconcile_with(dir, expected, ReconcileFaults::default())
}

/// [`reconcile`] with test-injectable failures.
pub(super) fn reconcile_with(
    dir: &Path,
    expected: u64,
    faults: ReconcileFaults,
) -> Result<u64, JournalError> {
    let path = dir.join(COMMIT_FILE);
    let visible = load(dir)?;
    if faults.file_sync {
        return Err(JournalError::State(StateError::new(
            &path,
            "injected commit-boundary file sync failure",
        )));
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| file.sync_all())
        .map_err(|err| StateError::new(&path, err))?;
    if faults.dir_sync {
        return Err(JournalError::State(StateError::new(
            dir,
            "injected commit-boundary directory sync failure",
        )));
    }
    crate::fs_atomic::sync_dir(dir).map_err(|err| StateError::new(dir, err))?;
    let confirmed = load(dir)?;
    if confirmed != expected {
        return Err(JournalError::Corrupt(format!(
            "the commit boundary names {confirmed} bytes, not the intended {expected} \
             (it was {visible} before the sync)"
        )));
    }
    Ok(confirmed)
}

/// Truncates the journal to `len` and syncs the truncation.
pub(super) fn truncate_journal(dir: &Path, len: u64) -> Result<(), JournalError> {
    let path = dir.join(JOURNAL_FILE);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|err| StateError::new(&path, err))?;
    file.set_len(len)
        .and_then(|()| file.sync_all())
        .map_err(|err| StateError::new(&path, err))?;
    Ok(())
}
