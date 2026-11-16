//! Record appends and the durable commit boundary that makes them acknowledged.
//!
//! An append is acknowledged only after its frame is synced *and* the durable committed
//! length has been replaced. The boundary always predates the bytes it does not name, so a
//! crash between any two steps leaves a boundary that startup can trust.

use std::io::Write as _;
use std::path::Path;

use super::JOURNAL_FILE;
use super::commit;
use super::record::{self, Record};
use super::store::{AppendFault, Fence, JournalState};
use crate::error::{JournalError, StateError};

/// Which stage of a real append failed. Production error handling is driven by this, never
/// by the injected fault.
enum AppendFailure {
    /// The journal could not be opened; no byte was written.
    Open(StateError),
    /// The write stopped partway.
    Write(StateError),
    /// A complete frame is visible but its sync did not confirm.
    Sync(StateError),
    /// The frame was written and synced, then the outcome was reported uncertain.
    Uncertain(StateError),
}

pub(super) fn append_locked(state: &mut JournalState, record: &Record) -> Result<(), JournalError> {
    if let Some(fence) = &state.fenced {
        return Err(JournalError::Corrupt(fence.reason.clone()));
    }
    let fault = std::mem::take(&mut state.append_fault);
    let frame = record::frame(record)?;
    let path = state.dir.join(JOURNAL_FILE);
    let previous = state.valid_len;
    let target = previous + frame.len() as u64;

    let uncertain = match write_frame(
        &path,
        &frame,
        fault,
        &mut state.recovery_sync_fails,
        &mut state.truncate_fails,
        &mut state.truncate_sync_fails,
    ) {
        Ok(()) => None,
        Err(AppendFailure::Open(err)) => return Err(JournalError::State(err)),
        Err(AppendFailure::Write(err)) => {
            // Nothing complete was appended: return to the last committed boundary.
            repair_or_fence(state);
            return Err(JournalError::State(err));
        }
        Err(AppendFailure::Sync(err)) => {
            // A complete frame may be visible but unsynced. One recovery sync may confirm it;
            // only then may the commit boundary name it.
            let recovery_fails = std::mem::take(&mut state.recovery_sync_fails);
            if recovery_fails || !recovery_sync(&path) || !frame_is_last(state, record) {
                repair_or_fence(state);
                return Err(JournalError::State(err));
            }
            None
        }
        Err(AppendFailure::Uncertain(err)) => Some(err),
    };

    // The frame is on disk and its durability is confirmed. The boundary replacement is the
    // commit point; until it returns, nothing may acknowledge the record.
    let commit_faults = std::mem::take(&mut state.commit_faults);
    match commit::write(&state.dir, target, commit_faults) {
        Ok(()) => {
            adopt(state, record, target);
            match uncertain {
                Some(err) => Err(JournalError::State(err)),
                None => Ok(()),
            }
        }
        Err(crate::fs_atomic::DurableWriteError::BeforeCommit(err)) => {
            // The previous boundary is unchanged and durable, and it predates this frame.
            // Startup truncates anything past it, so the frame can never become authoritative.
            repair_or_fence(state);
            Err(JournalError::State(err))
        }
        Err(crate::fs_atomic::DurableWriteError::Indeterminate(err)) => {
            // The rename is visible but its directory entry may not be durable. Confirm the
            // exact boundary value and its parent directory before acknowledging.
            let confirm_fails = std::mem::take(&mut state.confirm_fails);
            if !confirm_fails && commit::reconcile(&state.dir, target).is_ok() {
                adopt(state, record, target);
                match uncertain {
                    Some(err) => Err(JournalError::State(err)),
                    None => Ok(()),
                }
            } else {
                // Roll the boundary back to the last acknowledged length so this frame can
                // never be adopted. Only if that durable write also fails must the process
                // keep the fence: a restart then sees the old or the new boundary, and the
                // frame was synced before either was named.
                if commit::write(
                    &state.dir,
                    previous,
                    crate::fs_atomic::DurableFaults::default(),
                )
                .is_err()
                {
                    state.fenced = Some(Fence::append(
                        "an append's commit boundary could not be confirmed",
                    ));
                } else {
                    repair_or_fence(state);
                }
                Err(JournalError::State(err))
            }
        }
    }
}

fn adopt(state: &mut JournalState, record: &Record, target: u64) {
    state.valid_len = target;
    state.records.push(record.clone());
}

/// Truncates to the last committed boundary, or fences when that cannot be confirmed.
///
/// The durable boundary already names the previous length, so even a failed truncation is
/// repaired at the next startup; the fence only prevents this process from appending behind
/// an unconfirmed tail.
fn repair_or_fence(state: &mut JournalState) {
    let previous = state.valid_len;
    let path = state.dir.join(JOURNAL_FILE);
    let fail_setlen = std::mem::take(&mut state.truncate_fails);
    let fail_sync = std::mem::take(&mut state.truncate_sync_fails);
    if truncate_and_sync(&path, previous, fail_setlen, fail_sync).is_ok() {
        return;
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
    truncate_sync_fails: &mut bool,
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
        AppendFault::RecoverySyncTruncateSyncFail => {
            file.write_all(frame)
                .map_err(|err| AppendFailure::Write(StateError::new(path, err)))?;
            *recovery_fails = true;
            *truncate_sync_fails = true;
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

fn recovery_sync(path: &Path) -> bool {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all())
        .is_ok()
}

fn truncate_and_sync(
    path: &Path,
    len: u64,
    fail_setlen: bool,
    fail_sync: bool,
) -> std::io::Result<()> {
    if fail_setlen {
        return Err(std::io::Error::other("injected truncation failure"));
    }
    let file = std::fs::OpenOptions::new().write(true).open(path)?;
    file.set_len(len)?;
    if fail_sync {
        return Err(std::io::Error::other("injected truncation sync failure"));
    }
    file.sync_all()
}

/// Re-reads the whole file and requires `expected` to be the last complete record.
///
/// The caller may adopt only when the frame is the final complete frame and the file's
/// committed prefix validates.
fn frame_is_last(state: &JournalState, expected: &Record) -> bool {
    let path = state.dir.join(JOURNAL_FILE);
    let Ok(Some(bytes)) = crate::fs_atomic::read_optional(&path) else {
        return false;
    };
    let Ok(parsed) = record::parse(&bytes, state.meta.epoch) else {
        return false;
    };
    parsed.valid_len == bytes.len() as u64 && parsed.records.last() == Some(expected)
}
