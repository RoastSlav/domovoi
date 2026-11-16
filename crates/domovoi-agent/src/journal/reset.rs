//! Explicit journal initialization and reset, including resumable, ordered archival.
//!
//! Archive ordering: create every link, verify it points at the source, sync the directory,
//! only then remove the source names, and sync again. No source name is deleted before its
//! archive name is durable.

use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::JOURNAL_FILE;
use super::Journal;
use super::archive::{archive_existing, choose_generation, finish_epoch, present_sources};
use super::commit;
use super::lock::{self, JournalLock};
use super::meta::{META_FILE, Metadata};
use super::store::{RESET_INTENT_FILE, create_epoch_with};
use crate::error::{JournalError, StateError};
use crate::fs_atomic::DurableFaults;

const ARCHIVING: &str = "archiving";
const ARCHIVED: &str = "archived";
const CREATED: &str = "created";

/// Stages of a reset a test can make fail. Never set in production.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ResetFaults {
    pub(super) intent_write: bool,
    pub(super) link_first: bool,
    pub(super) link_second: bool,
    pub(super) verify: bool,
    pub(super) link_dir_sync: bool,
    pub(super) remove_first: bool,
    pub(super) remove_second: bool,
    pub(super) remove_dir_sync: bool,
    pub(super) create_epoch: bool,
    pub(super) epoch_commit: DurableFaults,
    pub(super) intent_remove: bool,
    pub(super) intent_dir_sync: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResetIntent {
    /// One of `archiving`, `archived`, `created`.
    stage: String,
    /// The archive generation chosen when the reset started.
    generation: String,
    /// The exact source names present when the reset started.
    #[serde(default)]
    sources: Vec<String>,
    /// The epoch this reset creates, chosen before anything is archived.
    #[serde(default)]
    epoch: Option<Uuid>,
}

fn read_reset_intent(dir: &Path) -> Result<Option<ResetIntent>, JournalError> {
    let path = dir.join(RESET_INTENT_FILE);
    let Some(bytes) = crate::fs_atomic::read_optional(&path)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|err| JournalError::Corrupt(format!("{RESET_INTENT_FILE}: {err}")))
}

fn write_reset_intent(
    dir: &Path,
    intent: &ResetIntent,
    faults: ResetFaults,
) -> Result<(), JournalError> {
    let path = dir.join(RESET_INTENT_FILE);
    if faults.intent_write {
        return Err(JournalError::State(StateError::new(
            &path,
            "injected reset intent write failure",
        )));
    }
    let bytes = serde_json::to_vec(intent)
        .map_err(|err| JournalError::Corrupt(format!("{RESET_INTENT_FILE}: {err}")))?;
    crate::fs_atomic::write_atomic_durable(&path, &bytes, 0o600)
        .map_err(|err| StateError::new(&path, err))?;
    Ok(())
}

fn remove_reset_intent(dir: &Path, faults: ResetFaults) -> Result<(), JournalError> {
    let path = dir.join(RESET_INTENT_FILE);
    if faults.intent_remove {
        return Err(JournalError::State(StateError::new(
            &path,
            "injected reset intent removal failure",
        )));
    }
    match std::fs::remove_file(&path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            return Err(JournalError::State(StateError::new(&path, err)));
        }
        _ => {}
    }
    if faults.intent_dir_sync {
        return Err(JournalError::State(StateError::new(
            dir,
            "injected reset intent directory sync failure",
        )));
    }
    crate::fs_atomic::sync_dir(dir).map_err(|err| StateError::new(dir, err))?;
    Ok(())
}
fn finish_reset(
    dir: &Path,
    mut intent: ResetIntent,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
    faults: ResetFaults,
) -> Result<Uuid, JournalError> {
    let epoch = intent.epoch.ok_or_else(|| {
        JournalError::Corrupt(format!(
            "{RESET_INTENT_FILE} does not name the epoch to create"
        ))
    })?;
    if intent.stage == ARCHIVING {
        archive_existing(dir, &intent.generation, &intent.sources, faults)?;
        intent.stage = ARCHIVED.to_owned();
        write_reset_intent(dir, &intent, faults)?;
    }
    if intent.stage == ARCHIVED {
        finish_epoch(dir, epoch, now, boot_id, mono_ns, faults)?;
        intent.stage = CREATED.to_owned();
        write_reset_intent(dir, &intent, faults)?;
    }
    if intent.stage != CREATED {
        return Err(JournalError::Corrupt(format!(
            "{RESET_INTENT_FILE} names an unknown stage"
        )));
    }
    // Never remove the intent unless the epoch it names is the one on disk.
    match Metadata::load(dir)? {
        Some(meta) if meta.epoch == epoch => {}
        Some(meta) => {
            return Err(JournalError::Corrupt(format!(
                "the reset intent names epoch {epoch}, but the metadata names {}",
                meta.epoch
            )));
        }
        None => {
            return Err(JournalError::Corrupt(
                "the created epoch's metadata is missing".to_owned(),
            ));
        }
    }
    remove_reset_intent(dir, faults)?;
    Ok(epoch)
}

pub(super) fn reset_locked(
    dir: &Path,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
) -> Result<Uuid, JournalError> {
    reset_locked_with(dir, now, boot_id, mono_ns, ResetFaults::default())
}

pub(super) fn reset_locked_with(
    dir: &Path,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
    faults: ResetFaults,
) -> Result<Uuid, JournalError> {
    if let Some(intent) = read_reset_intent(dir)? {
        return finish_reset(dir, intent, now, boot_id, mono_ns, faults);
    }
    let sources = present_sources(dir)?;
    if sources.is_empty() {
        return Err(JournalError::State(StateError::new(
            dir,
            "there is no journal to reset; use `journal init` for a new installation",
        )));
    }
    let base = Metadata::load(dir)
        .ok()
        .flatten()
        .map(|meta| meta.epoch.to_string())
        .unwrap_or_else(|| Uuid::now_v7().to_string());
    let generation = choose_generation(dir, &base, &sources)?;
    let intent = ResetIntent {
        stage: ARCHIVING.to_owned(),
        generation,
        sources,
        epoch: Some(Uuid::now_v7()),
    };
    write_reset_intent(dir, &intent, faults)?;
    finish_reset(dir, intent, now, boot_id, mono_ns, faults)
}

impl Journal {
    /// Creates the journal for the first time, refusing if any journal file exists.
    pub fn init(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        std::fs::create_dir_all(dir).map_err(|err| StateError::new(dir, err))?;
        let lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        Self::init_with_lock(dir, lock, now, boot_id, mono_ns, DurableFaults::default())
    }

    /// Creates the journal without the OS lock. Test-only; production uses [`Journal::init`].
    #[cfg(test)]
    pub(crate) fn init_unlocked(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        Self::init_with_lock(
            dir,
            lock::unlocked(),
            now,
            boot_id,
            mono_ns,
            DurableFaults::default(),
        )
    }

    /// Creates the journal with injected commit-boundary failures. Test-only.
    #[cfg(test)]
    pub(super) fn init_unlocked_with_faults(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
        faults: DurableFaults,
    ) -> Result<Uuid, JournalError> {
        Self::init_with_lock(dir, lock::unlocked(), now, boot_id, mono_ns, faults)
    }

    fn init_with_lock(
        dir: &Path,
        lock: JournalLock,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
        faults: DurableFaults,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock;
        if dir.join(JOURNAL_FILE).exists()
            || dir.join(META_FILE).exists()
            || commit::exists(dir)
            || dir.join(RESET_INTENT_FILE).exists()
        {
            return Err(JournalError::AlreadyExists);
        }
        create_epoch_with(dir, Uuid::now_v7(), now, boot_id, mono_ns, faults)
    }

    /// Archives the existing pair and starts a new epoch, without the OS lock. Test-only.
    #[cfg(test)]
    pub(crate) fn reset_unlocked(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock::unlocked();
        reset_locked(dir, now, boot_id, mono_ns)
    }

    /// Resets with injected archive failures. Test-only.
    #[cfg(test)]
    pub(super) fn reset_unlocked_with_faults(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
        faults: ResetFaults,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock::unlocked();
        reset_locked_with(dir, now, boot_id, mono_ns, faults)
    }

    /// Archives the existing pair and starts a new epoch.
    pub fn reset(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        reset_locked(dir, now, boot_id, mono_ns)
    }
}
