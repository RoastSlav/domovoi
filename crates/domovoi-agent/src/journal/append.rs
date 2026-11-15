//! Record appends and their confirmed-durability reconciliation.

use std::io::Write as _;
use std::path::Path;

use super::JOURNAL_FILE;
use super::record::{self, Record};
use super::store::{AppendFault, Fence, JournalState};
use crate::error::{JournalError, StateError};

pub(super) fn append_locked(state: &mut JournalState, record: &Record) -> Result<(), JournalError> {
    if let Some(fence) = &state.fenced {
        return Err(JournalError::Corrupt(fence.reason.clone()));
    }
    let fault = std::mem::take(&mut state.append_fault);
    let frame = record::frame(record)?;
    let path = state.dir.join(JOURNAL_FILE);
    let previous = state.valid_len;
    match write_frame(&path, &frame, fault) {
        Ok(()) => {
            state.valid_len += frame.len() as u64;
            state.records.push(record.clone());
            Ok(())
        }
        Err(err) => match fault {
            AppendFault::Open => {
                // Nothing reached the file; the previous state is authoritative.
                Err(JournalError::State(err))
            }
            AppendFault::Partial | AppendFault::PartialTruncate => {
                // Only the tail we just wrote is incomplete. Return to the validated
                // boundary and sync that truncation before another append.
                let repaired =
                    fault == AppendFault::Partial && truncate_and_sync(&path, previous).is_ok();
                if !repaired {
                    state.fenced = Some(Fence::append("could not truncate a partial append frame"));
                }
                Err(JournalError::State(err))
            }
            AppendFault::Sync | AppendFault::RecoverySync => {
                // The complete frame may be visible but unsynced. Try one recovery sync;
                // only its success makes the frame authoritative.
                let recovered = fault == AppendFault::Sync
                    && recovery_sync(&path).is_ok()
                    && validate_and_adopt(state);
                if recovered {
                    Ok(())
                } else {
                    // Never leave an unconfirmed finish frame for a later restart to adopt.
                    let _ = truncate_and_sync(&path, previous);
                    state.fenced = Some(Fence::append(
                        "an append's durability could not be confirmed",
                    ));
                    Err(JournalError::State(err))
                }
            }
            AppendFault::Uncertain => {
                // The sync succeeded, then the outcome was reported uncertain. Adopt the
                // durable record but do not acknowledge it; a retry is a duplicate.
                if validate_and_adopt(state) {
                    Err(JournalError::State(err))
                } else {
                    state.fenced = Some(Fence::corrupt(
                        "an uncertain append left an invalid journal",
                    ));
                    Err(JournalError::State(err))
                }
            }
            AppendFault::None => unreachable!("a successful append never reaches this arm"),
        },
    }
}

fn write_frame(path: &Path, frame: &[u8], fault: AppendFault) -> Result<(), StateError> {
    if fault == AppendFault::Open {
        return Err(StateError::new(path, "injected append open failure"));
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|err| StateError::new(path, err))?;
    match fault {
        AppendFault::Partial | AppendFault::PartialTruncate => {
            let half = frame.len() / 2;
            file.write_all(&frame[..half])
                .map_err(|err| StateError::new(path, err))?;
            Err(StateError::new(path, "injected partial append failure"))
        }
        AppendFault::Sync | AppendFault::RecoverySync => {
            file.write_all(frame)
                .map_err(|err| StateError::new(path, err))?;
            Err(StateError::new(path, "injected append sync failure"))
        }
        AppendFault::Uncertain => {
            file.write_all(frame)
                .and_then(|()| file.sync_all())
                .map_err(|err| StateError::new(path, err))?;
            Err(StateError::new(path, "injected uncertain append outcome"))
        }
        AppendFault::Open | AppendFault::None => file
            .write_all(frame)
            .and_then(|()| file.sync_all())
            .map_err(|err| StateError::new(path, err)),
    }
}

fn recovery_sync(path: &Path) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .sync_all()
}

fn truncate_and_sync(path: &Path, len: u64) -> std::io::Result<()> {
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_len(len)?;
    file.sync_all()
}

/// Re-reads the whole file, validates it and adopts it. Returns whether it was valid.
fn validate_and_adopt(state: &mut JournalState) -> bool {
    let path = state.dir.join(JOURNAL_FILE);
    let Ok(Some(bytes)) = crate::fs_atomic::read_optional(&path) else {
        return false;
    };
    let Ok(parsed) = record::parse(&bytes, state.meta.epoch) else {
        return false;
    };
    if parsed.valid_len < bytes.len() as u64 && truncate_and_sync(&path, parsed.valid_len).is_err()
    {
        return false;
    }
    state.records = parsed.records;
    state.valid_len = parsed.valid_len;
    true
}
