//! The signal spool: every batch is on disk before it is sent, until the hub acknowledges it.
//!
//! - `spool.jsonl` holds one `{batch_seq, signals}` batch per line, oldest first.
//! - `spool.json` holds the next sequence number and the dropped-signal count, so
//!   `batch_seq` keeps rising across restarts even when the spool is empty.
//! - When the spool is full ([`MAX_BATCHES`] or [`MAX_BYTES`]), the oldest firing signals
//!   with info severity go first, then whole oldest batches. Resolved signals are kept
//!   ahead of whole batches, since losing one would leave an incident open.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};

use domovoi_core::{Severity, Signal, SignalStatus};
use domovoi_proto::MAX_FRAME_BYTES;
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
// Leave room for the protocol envelope around a `Signals` payload.
const MAX_BATCH_BYTES: usize = MAX_FRAME_BYTES - 1024;

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
    repair_needed: bool,
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
            Some(bytes) => serde_json::from_slice(&bytes).map_err(|err| {
                StateError::new(
                    &meta_path,
                    format!(
                        "spool metadata is unreadable; refusing to reuse acknowledged batch sequences: {err}"
                    ),
                )
            })?,
            None => Meta::default(),
        };
        let mut batches = VecDeque::new();
        let mut bytes: usize = 0;
        let mut needs_rewrite = false;
        let mut next_batch_seq = None;
        if let Some(raw) = read_optional(&path)? {
            for text in String::from_utf8_lossy(&raw).lines() {
                match serde_json::from_str::<Batch>(text) {
                    Ok(mut batch) => {
                        let size = text.len().saturating_add(1);
                        let original_seq = batch.batch_seq;
                        let seq = next_batch_seq
                            .map_or(batch.batch_seq, |next| batch.batch_seq.max(next));
                        if size > MAX_BATCH_BYTES {
                            let (chunks, next) =
                                encode_chunks(batch.signals, seq, MAX_BATCH_BYTES, &path)?;
                            for (chunk, encoded) in chunks {
                                let chunk_size = encoded.len();
                                bytes = bytes.saturating_add(chunk_size);
                                batches.push_back((chunk, chunk_size));
                            }
                            next_batch_seq = Some(next);
                            meta.next_seq = meta.next_seq.max(next);
                            needs_rewrite = true;
                            continue;
                        }
                        batch.batch_seq = seq;
                        if seq != original_seq {
                            needs_rewrite = true;
                        }
                        bytes = bytes.saturating_add(size);
                        batches.push_back((batch, size));
                        next_batch_seq = Some(seq.saturating_add(1));
                        meta.next_seq = meta.next_seq.max(seq.saturating_add(1));
                    }
                    Err(err) => {
                        tracing::warn!(%err, "skipping an unreadable spool line");
                        needs_rewrite = true;
                    }
                }
            }
        }
        let mut spool = Self {
            dir: state_dir.to_owned(),
            batches,
            bytes,
            meta,
            max_batches: MAX_BATCHES,
            max_bytes: MAX_BYTES,
            repair_needed: false,
        };
        let changed = spool.shrink()?;
        if changed || needs_rewrite {
            spool.rewrite()?;
            spool.save_meta()?;
        }
        Ok(spool)
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
        save_meta(&self.dir, &self.meta)
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
            self.meta.dropped = self
                .meta
                .dropped
                .saturating_add((before - batch.signals.len()) as u64);
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
                self.meta.dropped = self.meta.dropped.saturating_add(batch.signals.len() as u64);
                self.bytes -= size;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Adds `signals` and returns the first sequence number. Data is on disk on return.
    pub fn push(&mut self, signals: Vec<Signal>) -> Result<u64, StateError> {
        let path = self.path();
        let first_seq = self.meta.next_seq;
        let chunk_limit = self.max_bytes.min(MAX_BATCH_BYTES);
        let (encoded, next_seq) = encode_chunks(signals, first_seq, chunk_limit, &path)?;
        let mut proposed = Self {
            dir: self.dir.clone(),
            batches: self.batches.clone(),
            bytes: self.bytes,
            meta: self.meta,
            max_batches: self.max_batches,
            max_bytes: self.max_bytes,
            repair_needed: self.repair_needed,
        };
        let mut appended = String::new();
        for (batch, text) in encoded {
            proposed.bytes = proposed.bytes.saturating_add(text.len());
            appended.push_str(&text);
            proposed.batches.push_back((batch, text.len()));
        }
        proposed.meta.next_seq = next_seq;
        let changed = proposed.shrink()?;

        // Persist the high-water mark before writing batches. If the batch write then fails,
        // skipping sequence numbers is safe; reusing one after an acknowledged batch is not.
        let mut sequence_meta = self.meta;
        sequence_meta.next_seq = next_seq;
        save_meta(&self.dir, &sequence_meta)?;
        self.meta.next_seq = next_seq;

        // A fresh file is created through `rewrite`, so it gets the spool's mode.
        if changed || self.repair_needed || !path.exists() {
            proposed.rewrite()?;
        } else {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| StateError::new(&path, e))?;
            if let Err(err) = file
                .write_all(appended.as_bytes())
                .and_then(|()| file.sync_data())
            {
                let primary = StateError::new(&path, err);
                if let Err(rewrite_err) = self.rewrite() {
                    tracing::error!(%rewrite_err, "could not remove a partial spool append");
                    self.repair_needed = true;
                }
                return Err(primary);
            }
        }

        self.batches = proposed.batches;
        self.bytes = proposed.bytes;
        self.meta.dropped = proposed.meta.dropped;
        self.repair_needed = false;
        if let Err(err) = self.save_meta() {
            // The sequence high-water mark was already saved. Only the dropped-signal counter
            // can be stale, and the signal batch itself is durable.
            tracing::warn!(%err, "signals were spooled but the dropped-signal count was not saved");
        }
        Ok(first_seq)
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

fn save_meta(dir: &Path, meta: &Meta) -> Result<(), StateError> {
    let path = dir.join(SPOOL_META_FILE);
    let bytes = serde_json::to_vec(meta).map_err(|e| StateError::new(&path, e))?;
    write_atomic(&path, &bytes, MODE)
}

fn encode_chunks(
    signals: Vec<Signal>,
    first_seq: u64,
    max_bytes: usize,
    path: &Path,
) -> Result<(Vec<(Batch, String)>, u64), StateError> {
    let mut encoded = Vec::new();
    let mut current = Vec::new();
    let mut current_payload_bytes = 0usize;
    let mut next_seq = first_seq;
    for signal in signals {
        let signal_text =
            serde_json::to_string(&signal).map_err(|err| StateError::new(path, err))?;
        let empty_line = line(
            &Batch {
                batch_seq: next_seq,
                signals: Vec::new(),
            },
            path,
        )?;
        let comma = usize::from(!current.is_empty());
        let candidate_size = empty_line
            .len()
            .saturating_add(current_payload_bytes)
            .saturating_add(comma)
            .saturating_add(signal_text.len());
        if candidate_size <= max_bytes {
            current_payload_bytes = current_payload_bytes
                .saturating_add(comma)
                .saturating_add(signal_text.len());
            current.push(signal);
            continue;
        }
        if current.is_empty() {
            return Err(StateError::new(
                path,
                format!("one signal exceeds the {max_bytes} byte spool limit"),
            ));
        }
        let batch = Batch {
            batch_seq: next_seq,
            signals: current,
        };
        let text = line(&batch, path)?;
        encoded.push((batch, text));
        next_seq = next_seq
            .checked_add(1)
            .ok_or_else(|| StateError::new(path, "spool batch sequence number is exhausted"))?;
        let empty_line = line(
            &Batch {
                batch_seq: next_seq,
                signals: Vec::new(),
            },
            path,
        )?;
        if empty_line.len().saturating_add(signal_text.len()) > max_bytes {
            return Err(StateError::new(
                path,
                format!("one signal exceeds the {max_bytes} byte spool limit"),
            ));
        }
        current = vec![signal];
        current_payload_bytes = signal_text.len();
    }
    if !current.is_empty() || encoded.is_empty() {
        let batch = Batch {
            batch_seq: next_seq,
            signals: current,
        };
        let text = line(&batch, path)?;
        if text.len() > max_bytes {
            return Err(StateError::new(
                path,
                format!("one signal exceeds the {max_bytes} byte spool limit"),
            ));
        }
        encoded.push((batch, text));
        next_seq = next_seq
            .checked_add(1)
            .ok_or_else(|| StateError::new(path, "spool batch sequence number is exhausted"))?;
    }
    Ok((encoded, next_seq))
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
        let result = self.spool.lock().await.push(signals);
        self.notify.notify_one();
        result
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
