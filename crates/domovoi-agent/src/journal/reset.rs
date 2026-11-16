//! Explicit journal initialization and reset, including resumable archival.

use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::lock::{self, JournalLock};
use super::meta::Metadata;
use super::store::{RESET_INTENT_FILE, create_epoch};
use super::{JOURNAL_FILE, Journal, META_FILE};
use crate::error::{JournalError, StateError};

#[derive(Debug, Serialize, Deserialize)]
struct ResetIntent {
    /// `archiving` before the new epoch exists, `created` after it does.
    stage: String,
    generation: String,
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

fn write_reset_intent(dir: &Path, intent: &ResetIntent) -> Result<(), JournalError> {
    let path = dir.join(RESET_INTENT_FILE);
    let bytes = serde_json::to_vec(intent)
        .map_err(|err| JournalError::Corrupt(format!("{RESET_INTENT_FILE}: {err}")))?;
    crate::fs_atomic::write_atomic_durable(&path, &bytes, 0o600)
        .map_err(|err| StateError::new(&path, err))?;
    Ok(())
}

pub(super) fn reset_locked(
    dir: &Path,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
) -> Result<Uuid, JournalError> {
    let journal = dir.join(JOURNAL_FILE);
    let meta = dir.join(META_FILE);
    let intent = read_reset_intent(dir)?;

    // A reset that created its new epoch but could not remove the intent is finished, not
    // restarted: do not archive or rotate the epoch it just created.
    if let Some(existing) = &intent
        && existing.stage == "created"
        && let Some(epoch) = existing.epoch
        && Metadata::load(dir)
            .ok()
            .flatten()
            .is_some_and(|meta| meta.epoch == epoch)
    {
        remove_reset_intent(dir)?;
        return Ok(epoch);
    }

    let any = journal.exists() || meta.exists();
    if !any && intent.is_none() {
        return Err(JournalError::State(StateError::new(
            dir,
            "there is no journal to reset; use `journal init` for a new installation",
        )));
    }
    if any {
        let base = Metadata::load(dir)
            .ok()
            .flatten()
            .map(|meta| meta.epoch.to_string())
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        write_reset_intent(
            dir,
            &ResetIntent {
                stage: "archiving".to_owned(),
                generation: base.clone(),
                epoch: None,
            },
        )?;
        archive_existing(dir, &base)?;
    }
    let epoch = create_epoch(dir, now, boot_id, mono_ns)?;
    write_reset_intent(
        dir,
        &ResetIntent {
            stage: "created".to_owned(),
            generation: intent
                .as_ref()
                .map(|intent| intent.generation.clone())
                .unwrap_or_else(|| epoch.to_string()),
            epoch: Some(epoch),
        },
    )?;
    remove_reset_intent(dir)?;
    Ok(epoch)
}

fn remove_reset_intent(dir: &Path) -> Result<(), JournalError> {
    let path = dir.join(RESET_INTENT_FILE);
    match std::fs::remove_file(&path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            return Err(JournalError::State(StateError::new(&path, err)));
        }
        _ => {}
    }
    crate::fs_atomic::sync_dir(dir)
        .map_err(|err| JournalError::State(StateError::new(dir, err)))?;
    Ok(())
}

/// Hard-links each existing journal file to a unique archive name, then removes the source.
///
/// Hard links give no-overwrite semantics: a collision fails rather than replacing the
/// archive, and every source is removed only after every archive link exists.
fn archive_existing(dir: &Path, base: &str) -> Result<(), JournalError> {
    let mut generation = base.to_owned();
    for _ in 0..10 {
        let mut created: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
        let mut collision = false;
        for name in [JOURNAL_FILE, META_FILE] {
            let from = dir.join(name);
            if !from.exists() {
                continue;
            }
            let to = dir.join(format!("{name}.{generation}"));
            match std::fs::hard_link(&from, &to) {
                Ok(()) => created.push((from, to)),
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    collision = true;
                    break;
                }
                Err(err) => {
                    for (_, link) in &created {
                        let _ = std::fs::remove_file(link);
                    }
                    return Err(JournalError::State(StateError::new(&from, err)));
                }
            }
        }
        if collision {
            for (_, link) in &created {
                let _ = std::fs::remove_file(link);
            }
            generation = format!("{base}-{}", Uuid::now_v7());
            continue;
        }
        for (from, _) in &created {
            std::fs::remove_file(from).map_err(|err| StateError::new(from, err))?;
        }
        return Ok(());
    }
    Err(JournalError::State(StateError::new(
        dir,
        "could not choose a unique archive name",
    )))
}

impl Journal {
    /// Creates the journal for the first time, refusing if either journal file exists.
    pub fn init(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        std::fs::create_dir_all(dir).map_err(|err| StateError::new(dir, err))?;
        let lock = lock::acquire(&dir.join(lock::LOCK_FILE))?;
        Self::init_with_lock(dir, lock, now, boot_id, mono_ns)
    }

    /// Creates the journal without the OS lock. Test-only; production uses [`Journal::init`].
    #[cfg(test)]
    pub(crate) fn init_unlocked(
        dir: &Path,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        Self::init_with_lock(dir, lock::unlocked(), now, boot_id, mono_ns)
    }

    fn init_with_lock(
        dir: &Path,
        lock: JournalLock,
        now: Timestamp,
        boot_id: &str,
        mono_ns: u64,
    ) -> Result<Uuid, JournalError> {
        let _lock = lock;
        if dir.join(JOURNAL_FILE).exists()
            || dir.join(META_FILE).exists()
            || dir.join(RESET_INTENT_FILE).exists()
        {
            return Err(JournalError::AlreadyExists);
        }
        create_epoch(dir, now, boot_id, mono_ns)
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
