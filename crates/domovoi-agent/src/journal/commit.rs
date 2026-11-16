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

/// Confirms a visible but indeterminate boundary, returning its durable length.
///
/// A rename that returned an error may still be visible. Only after the boundary file and
/// its directory entry are synced, and the visible value is exactly `expected`, is the
/// boundary durable.
pub(super) fn confirm(dir: &Path, expected: u64) -> Result<u64, JournalError> {
    let path = dir.join(COMMIT_FILE);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| file.sync_all())
        .and_then(|()| crate::fs_atomic::sync_dir(dir))
        .map_err(|err| JournalError::State(StateError::new(&path, err)))?;
    let visible = load(dir)?;
    if visible != expected {
        return Err(JournalError::Corrupt(format!(
            "the commit boundary names {visible} bytes, not the intended {expected}"
        )));
    }
    Ok(visible)
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
