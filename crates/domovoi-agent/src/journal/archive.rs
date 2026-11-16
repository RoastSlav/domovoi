//! Ordered, idempotent archival of the journal files during a reset.
//!
//! Every source name is link-verified and its directory entry synced before any source is
//! removed, and the removals are synced before a new epoch is created.
//!
//! Archives are hard links in the same directory as their source, so they never cross a
//! filesystem and no copy of the journal contents is made.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use uuid::Uuid;

use super::JOURNAL_FILE;
use super::commit;
use super::compaction;
use super::meta::{META_FILE, Metadata};
use super::record::{self, Record};
use super::reset::ResetFaults;
use super::store::create_epoch_with;
use crate::error::{JournalError, StateError};
use crate::fs_atomic::DurableFaults;

fn exists(path: &Path) -> Result<bool, JournalError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(JournalError::State(StateError::new(path, err))),
    }
}

/// Recognises every file an interrupted reset or compaction can leave behind, so reset
/// archives or removes it instead of letting it affect the new epoch.
pub(super) fn present_sources(dir: &Path) -> Result<Vec<String>, JournalError> {
    let mut sources = Vec::new();
    for name in [
        JOURNAL_FILE,
        META_FILE,
        commit::COMMIT_FILE,
        compaction::INTENT_FILE,
    ] {
        if exists(&dir.join(name))? {
            sources.push(name.to_owned());
        }
    }
    for name in compaction::TEMP_FILES {
        if exists(&dir.join(name))? {
            sources.push(name.to_owned());
        }
    }
    Ok(sources)
}

/// Picks a generation whose archive names do not collide with an existing archive.
pub(super) fn choose_generation(
    dir: &Path,
    base: &str,
    sources: &[String],
) -> Result<String, JournalError> {
    let mut generation = base.to_owned();
    for _ in 0..10 {
        let mut collision = false;
        for name in sources {
            if exists(&dir.join(format!("{name}.{generation}")))? {
                collision = true;
                break;
            }
        }
        if !collision {
            return Ok(generation);
        }
        generation = format!("{base}-{}", Uuid::now_v7());
    }
    Err(JournalError::State(StateError::new(
        dir,
        "could not choose a unique archive name",
    )))
}

/// Verifies that an archive name refers to the same file as its source.
fn verify_link(from: &Path, to: &Path) -> Result<(), JournalError> {
    let source = std::fs::metadata(from).map_err(|err| StateError::new(from, err))?;
    let archive = std::fs::metadata(to).map_err(|err| StateError::new(to, err))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if source.dev() != archive.dev() || source.ino() != archive.ino() {
            return Err(JournalError::Corrupt(format!(
                "the archive {} is not the journal file {}",
                to.display(),
                from.display()
            )));
        }
    }
    #[cfg(not(unix))]
    {
        // The agent is a Linux program; elsewhere only the size can be compared.
        if source.len() != archive.len() {
            return Err(JournalError::Corrupt(format!(
                "the archive {} does not match the journal file {}",
                to.display(),
                from.display()
            )));
        }
    }
    Ok(())
}

/// Creates the archive links, makes them durable, then removes the source names.
///
/// Idempotent at every phase: a link that already exists is verified, a source that is
/// already gone is accepted, and every directory sync is repeated on a retry.
pub(super) fn archive_existing(
    dir: &Path,
    generation: &str,
    sources: &[String],
    faults: ResetFaults,
) -> Result<(), JournalError> {
    let mut links: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (index, name) in sources.iter().enumerate() {
        let from = dir.join(name);
        let to = dir.join(format!("{name}.{generation}"));
        match (exists(&from)?, exists(&to)?) {
            (true, true) => {
                if faults.verify {
                    return Err(JournalError::State(StateError::new(
                        &to,
                        "injected archive verification failure",
                    )));
                }
                verify_link(&from, &to)?;
            }
            (true, false) => {
                let injected = match index {
                    0 => faults.link_first,
                    1 => faults.link_second,
                    _ => false,
                };
                if injected {
                    return Err(JournalError::State(StateError::new(
                        &from,
                        "injected archive link failure",
                    )));
                }
                std::fs::hard_link(&from, &to).map_err(|err| StateError::new(&from, err))?;
                if faults.verify {
                    return Err(JournalError::State(StateError::new(
                        &to,
                        "injected archive verification failure",
                    )));
                }
                verify_link(&from, &to)?;
            }
            (false, true) => {}
            (false, false) => {
                return Err(JournalError::Corrupt(format!(
                    "the reset intent lists {name}, but neither its source nor its archive exists"
                )));
            }
        }
        links.push((from, to));
    }
    // The archive names must be durable before any source name goes away.
    if faults.link_dir_sync {
        return Err(JournalError::State(StateError::new(
            dir,
            "injected archive directory sync failure",
        )));
    }
    crate::fs_atomic::sync_dir(dir).map_err(|err| StateError::new(dir, err))?;
    for (index, (from, _)) in links.iter().enumerate() {
        let injected = match index {
            0 => faults.remove_first,
            1 => faults.remove_second,
            _ => false,
        };
        if injected {
            return Err(JournalError::State(StateError::new(
                from,
                "injected source removal failure",
            )));
        }
        match std::fs::remove_file(from) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                return Err(JournalError::State(StateError::new(from, err)));
            }
            _ => {}
        }
    }
    if faults.remove_dir_sync {
        return Err(JournalError::State(StateError::new(
            dir,
            "injected source-removal directory sync failure",
        )));
    }
    crate::fs_atomic::sync_dir(dir).map_err(|err| StateError::new(dir, err))?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<(), JournalError> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(JournalError::State(StateError::new(path, err)))
        }
        _ => Ok(()),
    }
}

/// Creates the chosen epoch, or finishes one whose creation was interrupted.
pub(super) fn finish_epoch(
    dir: &Path,
    epoch: Uuid,
    now: Timestamp,
    boot_id: &str,
    mono_ns: u64,
    faults: ResetFaults,
) -> Result<(), JournalError> {
    if faults.create_epoch {
        return Err(JournalError::State(StateError::new(
            dir,
            "injected epoch creation failure",
        )));
    }
    for name in compaction::TEMP_FILES {
        match std::fs::remove_file(dir.join(name)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                return Err(JournalError::State(StateError::new(dir.join(name), err)));
            }
            _ => {}
        }
    }
    let journal = dir.join(JOURNAL_FILE);
    if !exists(&journal)? {
        create_epoch_with(dir, epoch, now, boot_id, mono_ns, faults.epoch_commit)
            .map_err(|err| StateError::new(&journal, err))?;
        return Ok(());
    }
    let bytes = std::fs::read(&journal).map_err(|err| StateError::new(&journal, err))?;
    if bytes.is_empty() {
        // A crash between file creation and the first byte leaves nothing to preserve.
        std::fs::remove_file(&journal).map_err(|err| StateError::new(&journal, err))?;
        remove_if_exists(&dir.join(commit::COMMIT_FILE))?;
        create_epoch_with(dir, epoch, now, boot_id, mono_ns, faults.epoch_commit)
            .map_err(|err| StateError::new(&journal, err))?;
        return Ok(());
    }
    let parsed = record::parse(&bytes, epoch).map_err(|err| {
        JournalError::Corrupt(format!("the interrupted new epoch does not parse: {err}"))
    })?;
    let ours = parsed.valid_len == bytes.len() as u64
        && matches!(
            parsed.records.first(),
            Some(Record::JournalCreated { epoch: created, .. }) if *created == epoch
        );
    if !ours {
        return Err(JournalError::Corrupt(
            "the partially created journal is not the epoch this reset started".to_owned(),
        ));
    }
    if let Err(err) = commit::write(dir, parsed.valid_len, faults.epoch_commit) {
        return Err(JournalError::State(StateError::new(
            dir.join(commit::COMMIT_FILE),
            err,
        )));
    }
    // The boundary must be durably confirmed before the epoch is treated as created.
    commit::reconcile(dir, parsed.valid_len)?;
    if Metadata::load(dir)?.is_none() {
        Metadata::new(epoch, boot_id, mono_ns)
            .write(dir, DurableFaults::default())
            .map_err(|err| StateError::new(dir.join(META_FILE), err))?;
    }
    Ok(())
}
