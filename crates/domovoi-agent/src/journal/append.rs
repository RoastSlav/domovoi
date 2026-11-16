//! Record appends and their confirmed-durability reconciliation.

use std::io::Write as _;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::JOURNAL_FILE;
use super::record::{self, Record};
use super::store::{AppendFault, Fence, JournalState};
use crate::error::{JournalError, StateError};

/// A durable marker that the last append could not be repaired.
pub(super) const APPEND_MARKER_FILE: &str = "action-journal.append";

#[derive(Debug, Serialize, Deserialize)]
struct AppendMarker {
    previous_len: u64,
}

/// Which stage of a real append failed. Production error handling is driven by this, never
/// by the injected fault.
enum AppendFailure {
    Open(StateError),
    Write(StateError),
    Sync(StateError),
    Uncertain(StateError),
}

pub(super) fn append_locked(state: &mut JournalState, record: &Record) -> Result<(), JournalError> {
    if let Some(fence) = &state.fenced {
        return Err(JournalError::Corrupt(fence.reason.clone()));
    }
    let fault = std::mem::take(&mut state.append_fault);
    state.recovery_sync_fails = false;
    state.truncate_fails = false;
    let frame = record::frame(record)?;
    let path = state.dir.join(JOURNAL_FILE);
    let previous = state.valid_len;
    let failure = match write_frame(
        &path,
        &frame,
        fault,
        &mut state.recovery_sync_fails,
        &mut state.truncate_fails,
    ) {
        Ok(()) => {
            state.valid_len += frame.len() as u64;
            state.records.push(record.clone());
            return Ok(());
        }
        Err(failure) => failure,
    };
    match failure {
        AppendFailure::Open(err) => {
            // Nothing reached the file; the previous state is authoritative.
            Err(JournalError::State(err))
        }
        AppendFailure::Write(err) => {
            // The write stopped partway. Return to the validated boundary and sync it.
            repair_or_mark(state, &path, previous);
            Err(JournalError::State(err))
        }
        AppendFailure::Sync(err) => {
            // A complete frame may be visible but unsynced. One recovery sync may confirm it.
            let recovery_fails = std::mem::take(&mut state.recovery_sync_fails);
            if !recovery_fails
                && recovery_sync(&path, false).is_ok()
                && validate_and_adopt(state, record)
            {
                Ok(())
            } else {
                repair_or_mark(state, &path, previous);
                state.fenced = Some(Fence::append(
                    "an append's durability could not be confirmed",
                ));
                Err(JournalError::State(err))
            }
        }
        AppendFailure::Uncertain(err) => {
            // The sync succeeded, then the outcome was reported uncertain. Adopt the
            // durable record but do not acknowledge it; a retry is a duplicate.
            if validate_and_adopt(state, record) {
                Err(JournalError::State(err))
            } else {
                state.fenced = Some(Fence::corrupt(
                    "an uncertain append left an invalid journal",
                ));
                Err(JournalError::State(err))
            }
        }
    }
}

/// Truncates to the previous boundary, or writes a durable marker when that fails.
fn repair_or_mark(state: &mut JournalState, path: &Path, previous: u64) {
    let truncate_fails = std::mem::take(&mut state.truncate_fails);
    if truncate_and_sync(path, previous, truncate_fails).is_ok() {
        return;
    }
    if let Err(err) = write_marker(&state.dir, previous) {
        tracing::error!(%err, "could not write the durable append marker");
    }
    state.fenced = Some(Fence::append(
        "an unconfirmed append frame could not be truncated",
    ));
}

fn write_frame(
    path: &Path,
    frame: &[u8],
    fault: AppendFault,
    recovery_fails: &mut bool,
    truncate_fails: &mut bool,
) -> Result<(), AppendFailure> {
    if fault == AppendFault::Open {
        return Err(AppendFailure::Open(StateError::new(
            path,
            "injected append open failure",
        )));
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|err| AppendFailure::Open(StateError::new(path, err)))?;
    match fault {
        AppendFault::Partial => {
            let half = frame.len() / 2;
            file.write_all(&frame[..half])
                .map_err(|err| AppendFailure::Write(StateError::new(path, err)))?;
            Err(AppendFailure::Write(StateError::new(
                path,
                "injected partial append failure",
            )))
        }
        AppendFault::Sync => {
            file.write_all(frame)
                .map_err(|err| AppendFailure::Write(StateError::new(path, err)))?;
            Err(AppendFailure::Sync(StateError::new(
                path,
                "injected append sync failure",
            )))
        }
        AppendFault::RecoverySync => {
            file.write_all(frame)
                .map_err(|err| AppendFailure::Write(StateError::new(path, err)))?;
            *recovery_fails = true;
            Err(AppendFailure::Sync(StateError::new(
                path,
                "injected append sync failure",
            )))
        }
        AppendFault::RecoverySyncTruncateFail => {
            file.write_all(frame)
                .map_err(|err| AppendFailure::Write(StateError::new(path, err)))?;
            *recovery_fails = true;
            *truncate_fails = true;
            Err(AppendFailure::Sync(StateError::new(
                path,
                "injected append sync failure",
            )))
        }
        AppendFault::Uncertain => {
            file.write_all(frame)
                .and_then(|()| file.sync_all())
                .map_err(|err| AppendFailure::Sync(StateError::new(path, err)))?;
            Err(AppendFailure::Uncertain(StateError::new(
                path,
                "injected uncertain append outcome",
            )))
        }
        AppendFault::Open | AppendFault::None => {
            file.write_all(frame)
                .map_err(|err| AppendFailure::Write(StateError::new(path, err)))?;
            file.sync_all()
                .map_err(|err| AppendFailure::Sync(StateError::new(path, err)))
        }
    }
}

fn recovery_sync(path: &Path, force_fail: bool) -> std::io::Result<()> {
    if force_fail {
        return Err(std::io::Error::other("injected recovery sync failure"));
    }
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .sync_all()
}

fn truncate_and_sync(path: &Path, len: u64, force_fail: bool) -> std::io::Result<()> {
    if force_fail {
        return Err(std::io::Error::other("injected truncation failure"));
    }
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_len(len)?;
    file.sync_all()
}

/// Re-reads the whole file, validates it and adopts it only when `expected` is the last,
/// complete and valid record.
fn validate_and_adopt(state: &mut JournalState, expected: &Record) -> bool {
    let path = state.dir.join(JOURNAL_FILE);
    let Ok(Some(bytes)) = crate::fs_atomic::read_optional(&path) else {
        return false;
    };
    let Ok(parsed) = record::parse(&bytes, state.meta.epoch) else {
        return false;
    };
    if parsed.valid_len != bytes.len() as u64 {
        return false;
    }
    if parsed.records.last() != Some(expected) {
        return false;
    }
    state.records = parsed.records;
    state.valid_len = parsed.valid_len;
    true
}

fn write_marker(dir: &Path, previous_len: u64) -> Result<(), std::io::Error> {
    let path = dir.join(APPEND_MARKER_FILE);
    let bytes = serde_json::to_vec(&AppendMarker { previous_len })
        .map_err(|err| std::io::Error::other(err.to_string()))?;
    crate::fs_atomic::write_atomic_durable(&path, &bytes, 0o600)
        .map_err(|err| std::io::Error::other(err.to_string()))
}

/// Repairs a leftover append marker before a journal is opened.
///
/// The marker is written only when an unconfirmed frame could not be truncated. Startup
/// truncates to the recorded boundary, syncs it, removes the marker and syncs the
/// directory. If any step fails, startup fails closed rather than adopting the frame.
pub(super) fn recover_marker(dir: &Path) -> Result<(), JournalError> {
    let marker = dir.join(APPEND_MARKER_FILE);
    let Some(bytes) = crate::fs_atomic::read_optional(&marker)? else {
        return Ok(());
    };
    let parsed: AppendMarker = serde_json::from_slice(&bytes)
        .map_err(|err| JournalError::Corrupt(format!("{APPEND_MARKER_FILE}: {err}")))?;
    let journal = dir.join(JOURNAL_FILE);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&journal)
        .map_err(|err| JournalError::State(StateError::new(&journal, err)))?;
    let len = file
        .metadata()
        .map_err(|err| JournalError::State(StateError::new(&journal, err)))?
        .len();
    if len < parsed.previous_len {
        return Err(JournalError::Corrupt(format!(
            "the append marker names {previous} bytes, but the journal has {len}",
            previous = parsed.previous_len
        )));
    }
    file.set_len(parsed.previous_len)
        .and_then(|()| file.sync_all())
        .map_err(|err| JournalError::State(StateError::new(&journal, err)))?;
    drop(file);
    std::fs::remove_file(&marker)
        .map_err(|err| JournalError::State(StateError::new(&marker, err)))?;
    crate::fs_atomic::sync_dir(dir)
        .map_err(|err| JournalError::State(StateError::new(dir, err)))?;
    Ok(())
}
