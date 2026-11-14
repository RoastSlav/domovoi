//! The signal spool: every batch is on disk before it is sent, until the hub acknowledges it.
//!
//! - `spool.jsonl` holds one `{batch_seq, signals}` batch per line, oldest first.
//! - `spool.json` holds the next sequence number and the dropped-signal count, so
//!   `batch_seq` keeps rising across restarts even when the spool is empty.
//! - When the spool is full ([`MAX_BATCHES`] or [`MAX_BYTES`]), the oldest firing signals
//!   with info severity go first, then whole oldest batches. Resolved signals are kept
//!   ahead of whole batches, since losing one would leave an incident open.
//! - A push that fails leaves no trace of its batch: the file is put back the way it was and
//!   the batch's sequence number is reused. If the file cannot be put back, the failure is
//!   logged; that batch may then be sent once after a restart, never by this process.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};

use domovoi_core::{Severity, Signal, SignalStatus};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, MutexGuard, Notify};

use crate::error::StateError;
use crate::fs_atomic::{read_optional, write_atomic};

/// The spool file under the state directory.
pub const SPOOL_FILE: &str = "spool.jsonl";
/// The spool's sequence and counters under the state directory.
pub const SPOOL_META_FILE: &str = "spool.json";
/// Most batches the spool keeps.
pub const MAX_BATCHES: usize = 10_000;
/// Most bytes the spool keeps.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
const MODE: u32 = 0o600;
/// The reason a test-injected write failure reports. It is never raised in a normal build.
const FAULT: &str = "injected write failure";

/// The stages of a spool write a test can make fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Opening the spool file for append.
    Open,
    /// After part of the batch line reached the file.
    Append,
    /// After the whole batch line reached the file, before it was flushed.
    Sync,
    /// Saving `spool.json`.
    SaveMeta,
    /// Undoing a failed write, by truncation or by putting the previous file back.
    Restore,
}

/// Stages a test wants to fail. They are consumed as they are reached, so the next write works.
#[cfg(test)]
#[derive(Debug, Default)]
struct Faults {
    queued: std::cell::Cell<[Option<Stage>; 2]>,
}

#[cfg(test)]
impl Faults {
    /// Fails `stages`, at most two, in the order they are reached.
    fn set(&self, stages: &[Stage]) {
        let mut queued = [None; 2];
        for (slot, stage) in queued.iter_mut().zip(stages) {
            *slot = Some(*stage);
        }
        self.queued.set(queued);
    }

    /// Whether `stage` is queued, forgetting it so it fails only once.
    fn take(&self, stage: Stage) -> bool {
        let mut queued = self.queued.get();
        let Some(index) = queued.iter().position(|queued| *queued == Some(stage)) else {
            return false;
        };
        queued[index] = None;
        self.queued.set(queued);
        true
    }
}

/// One batch of signals as sent in `signals`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Batch {
    /// Its sequence number, rising by one per batch for the agent's lifetime.
    pub batch_seq: u64,
    /// Its signals.
    pub signals: Vec<Signal>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct Meta {
    next_seq: u64,
    dropped: u64,
}

/// Signal batches waiting for the hub's acknowledgement.
#[derive(Debug)]
pub struct Spool {
    dir: PathBuf,
    batches: VecDeque<(Batch, usize)>,
    bytes: usize,
    meta: Meta,
    max_batches: usize,
    max_bytes: usize,
    /// Write stages a test injects failures into; never set in a normal build.
    #[cfg(test)]
    faults: Faults,
}

fn line(batch: &Batch, path: &Path) -> Result<String, StateError> {
    let mut text = serde_json::to_string(batch).map_err(|e| StateError::new(path, e))?;
    text.push('\n');
    Ok(text)
}

fn droppable(signal: &Signal) -> bool {
    signal.status() == SignalStatus::Firing && signal.severity() == Severity::Info
}

impl Spool {
    /// Opens the spool in `state_dir`, reading what an earlier run left. Unreadable lines are
    /// skipped.
    pub fn open(state_dir: &Path) -> Result<Self, StateError> {
        let path = state_dir.join(SPOOL_FILE);
        let meta_path = state_dir.join(SPOOL_META_FILE);
        let mut meta: Meta = match read_optional(&meta_path)? {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            None => Meta::default(),
        };
        let mut batches = VecDeque::new();
        let mut bytes = 0;
        if let Some(raw) = read_optional(&path)? {
            for text in String::from_utf8_lossy(&raw).lines() {
                match serde_json::from_str::<Batch>(text) {
                    Ok(batch) => {
                        meta.next_seq = meta.next_seq.max(batch.batch_seq + 1);
                        bytes += text.len() + 1;
                        batches.push_back((batch, text.len() + 1));
                    }
                    Err(err) => tracing::warn!(%err, "skipping an unreadable spool line"),
                }
            }
        }
        Ok(Self {
            dir: state_dir.to_owned(),
            batches,
            bytes,
            meta,
            max_batches: MAX_BATCHES,
            max_bytes: MAX_BYTES,
            #[cfg(test)]
            faults: Faults::default(),
        })
    }

    /// The same spool with other caps, for tests and small hosts.
    #[must_use]
    pub fn with_limits(mut self, max_batches: usize, max_bytes: usize) -> Self {
        self.max_batches = max_batches.max(1);
        self.max_bytes = max_bytes;
        self
    }

    fn path(&self) -> PathBuf {
        self.dir.join(SPOOL_FILE)
    }

    fn save_meta(&self) -> Result<(), StateError> {
        let path = self.dir.join(SPOOL_META_FILE);
        if self.fail_here(Stage::SaveMeta) {
            return Err(StateError::new(&path, FAULT));
        }
        let bytes = serde_json::to_vec(&self.meta).map_err(|e| StateError::new(&path, e))?;
        write_atomic(&path, &bytes, MODE)
    }

    /// Whether a test asked `stage` to fail, consuming it so the next write succeeds.
    #[cfg(test)]
    fn fail_here(&self, stage: Stage) -> bool {
        self.faults.take(stage)
    }

    /// Failures are only injected in tests.
    #[cfg(not(test))]
    fn fail_here(&self, _stage: Stage) -> bool {
        false
    }

    /// Fails `stages` on the next writes. Only built for tests.
    #[cfg(test)]
    fn inject(&self, stages: &[Stage]) {
        self.faults.set(stages);
    }

    fn rewrite(&self) -> Result<(), StateError> {
        let path = self.path();
        let mut text = String::with_capacity(self.bytes);
        for (batch, _) in &self.batches {
            text.push_str(&line(batch, &path)?);
        }
        write_atomic(&path, text.as_bytes(), MODE)
    }

    fn over(&self) -> bool {
        self.batches.len() > self.max_batches || self.bytes > self.max_bytes
    }

    /// Drops signals until the spool fits; returns whether anything was dropped.
    fn shrink(&mut self) -> Result<bool, StateError> {
        let path = self.path();
        let mut changed = false;
        while self.over() {
            let oldest_info = self
                .batches
                .iter()
                .position(|(b, _)| b.signals.iter().any(droppable));
            let Some(index) = oldest_info else { break };
            let (batch, size) = &mut self.batches[index];
            let before = batch.signals.len();
            batch.signals.retain(|s| !droppable(s));
            self.meta.dropped += (before - batch.signals.len()) as u64;
            let new_size = if batch.signals.is_empty() {
                0
            } else {
                line(batch, &path)?.len()
            };
            self.bytes = self.bytes - *size + new_size;
            *size = new_size;
            if new_size == 0 {
                self.batches.remove(index);
            }
            changed = true;
        }
        while self.over() && self.batches.len() > 1 {
            if let Some((batch, size)) = self.batches.pop_front() {
                self.meta.dropped += batch.signals.len() as u64;
                self.bytes -= size;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Adds a batch of `signals` and returns its sequence number.
    ///
    /// A successful push leaves the batch on disk. A failed one leaves no batch in memory, gives
    /// its sequence number back for the next push, and puts the spool file back the way it was,
    /// so a reopening never sees it. When that undo itself fails, the failure is logged: the
    /// live spool still never sends the batch, but a restart may read it and send it once.
    pub fn push(&mut self, signals: Vec<Signal>) -> Result<u64, StateError> {
        let path = self.path();
        let seq = self.meta.next_seq;
        let batch = Batch {
            batch_seq: seq,
            signals,
        };
        let text = line(&batch, &path)?;
        self.meta.next_seq += 1;
        self.bytes += text.len();
        self.batches.push_back((batch, text.len()));
        if let Err(err) = self.write(&path, &text) {
            self.rollback(seq);
            return Err(err);
        }
        Ok(seq)
    }

    /// Forgets the batch a failed push added and gives its sequence number back. Batches
    /// `shrink` dropped during the same push are not restored in memory: they are still on disk,
    /// or were lost with a rewrite the failed push could not undo, and a restart decides.
    fn rollback(&mut self, seq: u64) {
        if self
            .batches
            .back()
            .is_some_and(|(last, _)| last.batch_seq == seq)
            && let Some((_, size)) = self.batches.pop_back()
        {
            self.bytes -= size;
        }
        self.meta.next_seq = seq;
    }

    /// Writes the batch just added and its metadata, or puts the file back when that fails.
    fn write(&mut self, path: &Path, text: &str) -> Result<(), StateError> {
        // Over its limits `shrink` changes the batches, and a new file has to be created whole,
        // so both go through `write_rewritten`; everything else appends one line.
        if self.shrink()? || !path.exists() {
            self.write_rewritten(path)
        } else {
            self.write_appended(path, text)
        }
    }

    /// Appends one line, then saves the metadata. Once anything may have reached the file, every
    /// failure truncates it back to its length before the append.
    fn write_appended(&self, path: &Path, text: &str) -> Result<(), StateError> {
        let before = std::fs::metadata(path)
            .map_err(|e| StateError::new(path, e))?
            .len();
        // Opening the file writes nothing, so a failure up to here needs no undo.
        if self.fail_here(Stage::Open) {
            return Err(StateError::new(path, FAULT));
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| StateError::new(path, e))?;
        let filled = self.fill(&mut file, path, text);
        drop(file);
        if let Err(err) = filled {
            self.truncate_to(path, before);
            return Err(err);
        }
        if let Err(err) = self.save_meta() {
            self.truncate_to(path, before);
            return Err(err);
        }
        Ok(())
    }

    /// Writes the batch line and flushes it. A failure may leave part of the line in the file.
    fn fill(&self, file: &mut std::fs::File, path: &Path, text: &str) -> Result<(), StateError> {
        if self.fail_here(Stage::Append) {
            let half = text.len() / 2;
            let _ = file.write_all(&text.as_bytes()[..half]);
            return Err(StateError::new(path, FAULT));
        }
        file.write_all(text.as_bytes())
            .map_err(|e| StateError::new(path, e))?;
        if self.fail_here(Stage::Sync) {
            return Err(StateError::new(path, FAULT));
        }
        file.sync_data().map_err(|e| StateError::new(path, e))
    }

    /// Replaces the whole file with the batches in memory, then saves the metadata. A metadata
    /// failure puts the previous file back, including batches `shrink` dropped, so a failed push
    /// leaves nothing new behind.
    fn write_rewritten(&self, path: &Path) -> Result<(), StateError> {
        let existed = path.exists();
        let previous = if existed {
            std::fs::read(path).ok()
        } else {
            None
        };
        self.rewrite()?;
        if let Err(err) = self.save_meta() {
            self.restore(path, previous.as_deref(), existed);
            return Err(err);
        }
        Ok(())
    }

    /// Truncates the spool file back to `len`. A failure is logged: the batch is no longer in
    /// memory, but a restart may read it from disk and send it once.
    fn truncate_to(&self, path: &Path, len: u64) {
        let undone = if self.fail_here(Stage::Restore) {
            Err(StateError::new(path, FAULT))
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|file| file.set_len(len).and_then(|()| file.sync_data()))
                .map_err(|e| StateError::new(path, e))
        };
        if let Err(err) = undone {
            tracing::error!(
                %err,
                path = %path.display(),
                "could not undo a failed spool write; its batch may be resent after a restart"
            );
        }
    }

    /// Puts `previous` back after a rewrite whose metadata could not be saved, or removes the
    /// file when the push created it. A failure is logged for the same reason as in
    /// [`Self::truncate_to`].
    fn restore(&self, path: &Path, previous: Option<&[u8]>, existed: bool) {
        let restored = if self.fail_here(Stage::Restore) {
            Err(StateError::new(path, FAULT))
        } else {
            match (previous, existed) {
                (Some(bytes), _) => write_atomic(path, bytes, MODE),
                (None, false) => match std::fs::remove_file(path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        Err(StateError::new(path, e))
                    }
                    _ => Ok(()),
                },
                // The file was there but could not be read back, so there is nothing to put
                // back; the new batch stays on disk and is sent once more after a restart.
                (None, true) => Err(StateError::new(path, "could not be read back")),
            }
        };
        if let Err(err) = restored {
            tracing::error!(
                %err,
                path = %path.display(),
                "could not restore the spool file after a failed write; its batch may be resent after a restart"
            );
        }
    }

    /// Forgets every batch up to and including `up_to`.
    pub fn ack(&mut self, up_to: u64) -> Result<(), StateError> {
        let before = self.batches.len();
        while self
            .batches
            .front()
            .is_some_and(|(b, _)| b.batch_seq <= up_to)
        {
            if let Some((_, size)) = self.batches.pop_front() {
                self.bytes -= size;
            }
        }
        if self.batches.len() == before {
            return Ok(());
        }
        self.rewrite()
    }

    /// The waiting batches with a sequence number above `after`, oldest first.
    #[must_use]
    pub fn pending_after(&self, after: Option<u64>) -> Vec<Batch> {
        self.batches
            .iter()
            .filter(|(b, _)| after.is_none_or(|seq| b.batch_seq > seq))
            .map(|(b, _)| b.clone())
            .collect()
    }

    /// How many batches are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.batches.len()
    }

    /// Whether no batch is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// How many signals were dropped because the spool was full, over the agent's lifetime.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.meta.dropped
    }
}

/// The spool shared by the scheduler, which fills it, and the link, which drains it.
#[derive(Debug)]
pub struct SharedSpool {
    spool: Mutex<Spool>,
    notify: Notify,
}

impl SharedSpool {
    /// Shares `spool`.
    #[must_use]
    pub fn new(spool: Spool) -> Self {
        Self {
            spool: Mutex::new(spool),
            notify: Notify::new(),
        }
    }

    /// Spools a batch and wakes the link to send it.
    pub async fn push(&self, signals: Vec<Signal>) -> Result<u64, StateError> {
        let seq = self.spool.lock().await.push(signals)?;
        self.notify.notify_one();
        Ok(seq)
    }

    /// Locks the spool.
    pub async fn lock(&self) -> MutexGuard<'_, Spool> {
        self.spool.lock().await
    }

    /// Waits until a batch was pushed since the last wake-up.
    pub async fn pushed(&self) {
        self.notify.notified().await;
    }
}

#[cfg(test)]
mod tests;
