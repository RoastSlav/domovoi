//! Compaction: retention, the sticky marker, and crash-safe rewriting.

use std::collections::BTreeSet;
use std::io::Write as _;

use uuid::Uuid;

use super::JOURNAL_FILE;
use super::Journal;
use super::commit;
use super::record::{self, Record};
use super::store::{Fence, JournalState, load_records, write_meta_locked};
use crate::error::{JournalError, StateError};
use crate::fs_atomic::{DurableFaults, DurableWriteError};

/// Whether a compaction's rename is known to be durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RewriteOutcome {
    Confirmed,
    Indeterminate,
}

pub(super) fn rewrite_locked(
    state: &mut JournalState,
    records: &[Record],
) -> Result<RewriteOutcome, JournalError> {
    let mut bytes = Vec::new();
    for record in records {
        bytes.extend_from_slice(&record::frame(record)?);
    }
    let path = state.dir.join(JOURNAL_FILE);
    let tmp = state.dir.join(format!("{JOURNAL_FILE}.tmp"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(|err| StateError::new(&tmp, err))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|err| StateError::new(&tmp, err))?;
    drop(file);
    std::fs::rename(&tmp, &path).map_err(|err| StateError::new(&path, err))?;
    // The new file is visible but not yet the committed one. Replace the durable boundary
    // before anything treats the rewrite as done; the boundary write also syncs the
    // directory, so the rename and the boundary become durable together.
    let len = bytes.len() as u64;
    let mut faults = DurableFaults::default();
    if std::mem::take(&mut state.rewrite_dir_sync_fault) {
        faults.dir_sync = true;
        state.confirm_fails = true;
    }
    let outcome = match commit::write(&state.dir, len, faults) {
        Ok(()) => RewriteOutcome::Confirmed,
        Err(DurableWriteError::BeforeCommit(err)) => {
            tracing::warn!(%err, "journal compaction's commit boundary write failed");
            RewriteOutcome::Indeterminate
        }
        Err(DurableWriteError::Indeterminate(err)) => {
            let confirms = !std::mem::take(&mut state.confirm_fails)
                && commit::confirm(&state.dir, len).is_ok();
            if confirms {
                RewriteOutcome::Confirmed
            } else {
                tracing::warn!(%err, "journal compaction's boundary durability is unknown");
                RewriteOutcome::Indeterminate
            }
        }
    };
    // Adopt exactly what the file holds. A boundary that still names the old, longer file is
    // repaired from the complete shorter file; anything else fails closed.
    match commit::load(&state.dir)
        .and_then(|committed| load_records(&state.dir, &state.meta, committed))
    {
        Ok((kept, valid_len, fence)) => {
            state.records = kept;
            state.valid_len = valid_len;
            state.fenced = fence;
        }
        Err(err) => state.fenced = Some(Fence::corrupt(err.to_string())),
    }
    Ok(outcome)
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
        let mut kept = vec![state.records[0].clone()];
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
