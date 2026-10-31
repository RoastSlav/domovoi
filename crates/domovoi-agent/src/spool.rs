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
        let bytes = serde_json::to_vec(&self.meta).map_err(|e| StateError::new(&path, e))?;
        write_atomic(&path, &bytes, MODE)
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

    /// Adds a batch of `signals` and returns its sequence number. It is on disk on return.
    pub fn push(&mut self, signals: Vec<Signal>) -> Result<u64, StateError> {
        let path = self.path();
        let batch = Batch {
            batch_seq: self.meta.next_seq,
            signals,
        };
        self.meta.next_seq += 1;
        let text = line(&batch, &path)?;
        self.bytes += text.len();
        self.batches.push_back((batch.clone(), text.len()));
        // A fresh file is created through `rewrite`, so it gets the spool's mode.
        if self.shrink()? || !path.exists() {
            self.rewrite()?;
        } else {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|e| StateError::new(&path, e))?;
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_data())
                .map_err(|e| StateError::new(&path, e))?;
        }
        self.save_meta()?;
        Ok(batch.batch_seq)
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
