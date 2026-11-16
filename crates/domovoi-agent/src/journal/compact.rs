//! Compaction: retention, the sticky marker, and the crash-safe rewrite transaction.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;

use uuid::Uuid;

use super::JOURNAL_FILE;
use super::Journal;
use super::commit;
use super::compaction::{self, CompactionIntent};
use super::record::{self, Record};
use super::store::{Fence, JournalState, write_meta_locked};
use crate::error::JournalError;
use crate::fs_atomic::{DurableFaults, DurableWriteError};

/// Whether a compaction's rename is known to be durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RewriteOutcome {
    Confirmed,
    Indeterminate,
}

/// Stages of the compaction transaction a test can make fail.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RewriteFaults {
    /// Fail the intent's temporary-file write or sync.
    pub(super) intent_temp_sync: bool,
    /// Fail the intent rename.
    pub(super) intent_rename: bool,
    /// Fail the intent's parent-directory sync.
    pub(super) intent_dir_sync: bool,
    /// Fail writing the compacted temporary file.
    pub(super) temp_write: bool,
    /// Fail syncing the compacted temporary file.
    pub(super) temp_sync: bool,
    /// Fail renaming the compacted file over the journal.
    pub(super) rename: bool,
    /// Fail the journal's parent-directory sync.
    pub(super) journal_dir_sync: bool,
    /// Fail the recovery sync after a failed journal directory sync.
    pub(super) journal_dir_recovery_fails: bool,
    /// Fail the boundary's temporary-file write or sync.
    pub(super) boundary_temp_sync: bool,
    /// Fail the boundary rename.
    pub(super) boundary_rename: bool,
    /// Fail the boundary's parent-directory sync.
    pub(super) boundary_dir_sync: bool,
    /// Fail the confirmation of the boundary.
    pub(super) boundary_reconcile_fails: bool,
    /// Leave the intent in place.
    pub(super) intent_remove: bool,
    /// Remove the intent but skip the confirming directory sync.
    pub(super) intent_remove_dir_sync: bool,
}

/// Rewrites the journal with `records` through the durable compaction transaction.
///
/// Ordering: durable intent (old and new identity) -> synced temporary file -> rename ->
/// journal directory sync -> boundary write -> boundary confirmation -> intent removal ->
/// directory sync. The compacted name is durable before the boundary may name it, and the
/// transaction is removed only after the new committed state is confirmed.
pub(super) fn rewrite_locked(
    state: &mut JournalState,
    records: &[Record],
) -> Result<RewriteOutcome, JournalError> {
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(&record::frame(record)?);
    }
    let journal = state.dir.join(JOURNAL_FILE);
    let Some(old) = crate::fs_atomic::read_optional(&journal)? else {
        state.fenced = Some(Fence::corrupt("the journal disappeared before compaction"));
        return Ok(RewriteOutcome::Indeterminate);
    };
    if old.len() as u64 != state.valid_len {
        state.fenced = Some(Fence::corrupt(
            "the journal length changed before compaction",
        ));
        return Ok(RewriteOutcome::Indeterminate);
    }
    let faults = std::mem::take(&mut state.rewrite_faults);
    let intent = CompactionIntent {
        format: compaction::INTENT_FORMAT,
        transaction: Uuid::now_v7(),
        epoch: state.meta.epoch,
        old_len: old.len() as u64,
        old_digest: compaction::digest(&old),
        new_len: bytes.len() as u64,
        new_digest: compaction::digest(&bytes),
    };
    // 1. The intent is durable before the original journal may be replaced.
    let intent_faults = DurableFaults {
        temp_sync: faults.intent_temp_sync,
        rename: faults.intent_rename,
        dir_sync: faults.intent_dir_sync,
    };
    match compaction::write(&state.dir, &intent, intent_faults) {
        Ok(()) => {}
        Err(DurableWriteError::BeforeCommit(_)) => {
            // The rename never happened, so no intent name exists to protect. A leftover
            // temporary file is harmless and is removed here best-effort.
            let _ = std::fs::remove_file(state.dir.join(compaction::INTENT_TEMP_FILE));
            return Ok(RewriteOutcome::Indeterminate);
        }
        Err(DurableWriteError::Indeterminate(_)) => {
            // The intent name may be visible. If its cleanup is not durably confirmed, an
            // append would change the journal identity the intent describes.
            if clean_intent(state, faults).durable() {
                return Ok(RewriteOutcome::Indeterminate);
            }
            state.fenced = Some(Fence::append(
                "a compaction intent may survive its failed write",
            ));
            return Ok(RewriteOutcome::Indeterminate);
        }
    }
    // 2. Build and sync the compacted file.
    if write_temp(&state.dir, &bytes, faults).is_err() {
        if !abort_transaction(state, faults).durable() {
            state.fenced = Some(Fence::append(
                "an aborted compaction's intent removal is not durably confirmed",
            ));
        }
        return Ok(RewriteOutcome::Indeterminate);
    }
    // 3. Replace the journal name.
    if faults.rename || std::fs::rename(compaction::temp_path(&state.dir), &journal).is_err() {
        if !abort_transaction(state, faults).durable() {
            state.fenced = Some(Fence::append(
                "an aborted compaction's intent removal is not durably confirmed",
            ));
        }
        return Ok(RewriteOutcome::Indeterminate);
    }
    // 4. The new name must be durable before the boundary may name it.
    let synced = if faults.journal_dir_sync {
        Err(std::io::Error::other("injected directory sync failure"))
    } else {
        crate::fs_atomic::sync_dir(&state.dir)
    };
    if synced.is_err()
        && (faults.journal_dir_recovery_fails || crate::fs_atomic::sync_dir(&state.dir).is_err())
    {
        fence_compaction(state, records, bytes.len() as u64);
        return Ok(RewriteOutcome::Indeterminate);
    }
    // 5. The boundary names the compacted length, and 6. the naming is confirmed.
    let boundary_faults = DurableFaults {
        temp_sync: faults.boundary_temp_sync,
        rename: faults.boundary_rename,
        dir_sync: faults.boundary_dir_sync,
    };
    let durable = match commit::write(&state.dir, bytes.len() as u64, boundary_faults) {
        Ok(()) => true,
        Err(DurableWriteError::BeforeCommit(_)) => {
            // Nothing was named yet; make one durable attempt to establish it.
            commit::write(&state.dir, bytes.len() as u64, DurableFaults::default()).is_ok()
        }
        Err(DurableWriteError::Indeterminate(_)) => {
            // The rename may be visible but its durability is unconfirmed. Confirm it.
            !faults.boundary_reconcile_fails
                && commit::reconcile(&state.dir, bytes.len() as u64).is_ok()
        }
    };
    let confirmed = durable
        && !faults.boundary_reconcile_fails
        && commit::reconcile(&state.dir, bytes.len() as u64).is_ok();
    if !confirmed {
        fence_compaction(state, records, bytes.len() as u64);
        return Ok(RewriteOutcome::Indeterminate);
    }
    // 7. The new state is durable; the intent may be removed only if that removal is itself
    // durable. An intent that might survive a crash would no longer describe the journal
    // once an append changed it, so an unconfirmed cleanup fences mutations until startup
    // resolves the transaction.
    let cleanup = clean_intent(state, faults);
    state.records = records.to_vec();
    state.valid_len = bytes.len() as u64;
    if cleanup.durable() {
        Ok(RewriteOutcome::Confirmed)
    } else {
        state.fenced = Some(Fence::append(
            "a completed compaction's intent removal is not durably confirmed",
        ));
        Ok(RewriteOutcome::Indeterminate)
    }
}

/// How a compaction intent's cleanup ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntentCleanup {
    /// Removed, and its directory entry synced: nothing can survive a crash.
    Durable,
    /// The removal failed; the intent remains.
    RemoveFailed,
    /// The file is gone from this view, but the directory entry was not synced.
    Unsynced,
    /// An injected failure left the intent in place.
    Injected,
}

impl IntentCleanup {
    /// Whether no intent can survive a crash.
    fn durable(self) -> bool {
        matches!(self, Self::Durable)
    }
}

/// Removes the compaction intent and proves the removal is durable.
///
/// A missing file in this view is not proof: only a synced parent directory is. Callers must
/// keep the journal read-only whenever this is not [`IntentCleanup::Durable`], because an
/// append would change the journal identity a surviving intent still names.
fn clean_intent(state: &JournalState, faults: RewriteFaults) -> IntentCleanup {
    let path = state.dir.join(compaction::INTENT_FILE);
    if faults.intent_remove {
        tracing::warn!("a compaction intent was left in place");
        return IntentCleanup::Injected;
    }
    if faults.intent_remove_dir_sync {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                tracing::warn!(%err, "the compaction intent could not be removed");
                return IntentCleanup::RemoveFailed;
            }
        }
        tracing::warn!("the compaction intent's removal was not synced");
        return IntentCleanup::Unsynced;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(%err, "the compaction intent could not be removed");
            return IntentCleanup::RemoveFailed;
        }
    }
    match crate::fs_atomic::sync_dir(&state.dir) {
        Ok(()) => IntentCleanup::Durable,
        Err(err) => {
            tracing::warn!(%err, "the compaction intent removal could not be synced");
            IntentCleanup::Unsynced
        }
    }
}

fn write_temp(dir: &Path, bytes: &[u8], faults: RewriteFaults) -> std::io::Result<()> {
    if faults.temp_write {
        return Err(std::io::Error::other(
            "injected compacted temporary-file write failure",
        ));
    }
    let path = compaction::temp_path(dir);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    file.write_all(bytes)?;
    if faults.temp_sync {
        return Err(std::io::Error::other(
            "injected compacted temporary-file sync failure",
        ));
    }
    file.sync_all()
}

/// The original journal is untouched: discard the temporary file and the intent.
///
/// Returns whether the intent's removal is durable. When it is not, the caller fences and
/// leaves the old records in memory, because the intent may survive and startup must abort it.
fn abort_transaction(state: &JournalState, faults: RewriteFaults) -> IntentCleanup {
    let _ = std::fs::remove_file(compaction::temp_path(&state.dir));
    let cleanup = clean_intent(state, faults);
    if !cleanup.durable() {
        tracing::warn!("an aborted compaction's intent removal is not durably confirmed");
    }
    cleanup
}

/// The rename happened but its durability was not confirmed. Mutations stay unavailable and
/// reads show the retained records until startup resolves the transaction.
fn fence_compaction(state: &mut JournalState, records: &[Record], len: u64) {
    state.records = records.to_vec();
    state.valid_len = len;
    state.fenced = Some(Fence::append(
        "a compaction's durability could not be confirmed",
    ));
}

pub(super) fn kept_executions(
    records: &[Record],
    keep: usize,
    now_mono_ns: u64,
    boot_id: &str,
    boot_first_load_mono_ns: u64,
) -> BTreeSet<Uuid> {
    let mut accepted = BTreeSet::new();
    let mut resolved = BTreeSet::new();
    let mut finished: Vec<Uuid> = Vec::new();
    let mut limiter_active = BTreeSet::new();
    for record in records {
        match record {
            Record::Accepted {
                execution_id,
                mutating: true,
                boot_id: entry_boot,
                mono_ns,
                ..
            } => {
                accepted.insert(*execution_id);
                if super::limiter::active_until(
                    entry_boot,
                    *mono_ns,
                    boot_id,
                    boot_first_load_mono_ns,
                ) > now_mono_ns
                {
                    limiter_active.insert(*execution_id);
                }
            }
            Record::Accepted { execution_id, .. } => {
                accepted.insert(*execution_id);
            }
            Record::Finished { execution_id, .. } => {
                resolved.insert(*execution_id);
                if !finished.contains(execution_id) {
                    finished.push(*execution_id);
                }
            }
            Record::Abandoned { execution_id, .. } => {
                resolved.insert(*execution_id);
            }
            _ => {}
        }
    }
    // Every unresolved acceptance and every acceptance still inside the mutation window is
    // kept; so are the newest `keep` resolved ones.
    let mut kept: BTreeSet<Uuid> = accepted.difference(&resolved).copied().collect();
    kept.extend(limiter_active);
    for id in finished.into_iter().rev().take(keep) {
        kept.insert(id);
    }
    kept
}

impl Journal {
    /// Prunes resolved history, first making the sticky marker durable.
    ///
    /// Unresolved executions and every mutating acceptance still inside the rate-limit
    /// window are always kept. Returns whether records were actually removed.
    pub fn prune(
        &self,
        keep: usize,
        now_mono_ns: u64,
        boot_id: &str,
    ) -> Result<bool, JournalError> {
        let mut state = self.lock()?;
        if let Some(fence) = &state.fenced {
            return Err(JournalError::Corrupt(fence.reason.clone()));
        }
        let keep_set = kept_executions(
            &state.records,
            keep,
            now_mono_ns,
            boot_id,
            state.meta.boot.first_load_mono_ns,
        );
        let mut kept: Vec<Record> = vec![state.records[0].clone()];
        for record in &state.records[1..] {
            if record
                .execution_id()
                .is_some_and(|id| keep_set.contains(&id))
            {
                kept.push(record.clone());
            }
        }
        // Decide before touching the marker: a no-op prune must not set `pruned`.
        if kept.len() == state.records.len() {
            return Ok(false);
        }
        if !state.meta.pruned {
            let mut next = state.meta.clone();
            next.pruned = true;
            if !write_meta_locked(&mut state, &next)? {
                return Ok(false);
            }
        }
        match rewrite_locked(&mut state, &kept)? {
            RewriteOutcome::Confirmed => Ok(true),
            RewriteOutcome::Indeterminate => Ok(false),
        }
    }
}
