//! The write-ahead journal on disk, and what its records say about each block.
//!
//! One JSON line per record, written and synced before the `nft` call it describes, under the
//! same `state.lock` that guards the key, the state and the mutation limiter. Authoritative
//! membership is always the JSON read of the sets; this file explains it.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use domovoi_core::containment::templates::STEP1_TTL_S;
use domovoi_core::containment::{JournalOp, JournalRecord, read_journal};
use jiff::{SignedDuration, Timestamp};
use nix::fcntl::OFlag;

use crate::state::{LockGuard, WriteFaults, write_atomic_locked};

/// The journal's file name inside the helper's state directory.
pub const JOURNAL_FILE: &str = "containment.journal";

/// How long a finished block stays in the file.
const KEEP: SignedDuration = SignedDuration::from_hours(24);

/// Why the journal cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// A line that is not the last one is corrupt; nothing repairs it automatically.
    #[error("the containment journal is corrupt")]
    Corrupt,
    /// The file could not be read or written.
    #[error("the containment journal cannot be read or written: {0}")]
    Io(#[from] std::io::Error),
}

/// Where a block stands, from its records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// An intent with no result yet.
    Intent(JournalOp),
    /// In the set with its 30 second timeout.
    Applied,
    /// In the set with its full TTL.
    Confirmed,
    /// A rollback was about to run.
    RollbackIntent,
    /// Removed by the helper.
    RolledBack,
    /// The table vanished with the block in it.
    Lost,
    /// The step failed; nothing was added.
    Failed,
    /// The element was already there; nothing changed.
    Noop,
}

/// One block as the journal tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockState {
    /// The block id.
    pub block: String,
    /// The canonical source.
    pub source: String,
    /// The set of the element.
    pub set: String,
    /// The TTL after `confirm`.
    pub ttl_s: u32,
    /// The phase.
    pub phase: Phase,
    /// When the element was added.
    pub applied_at: Option<Timestamp>,
    /// When the element expires, once confirmed.
    pub deadline: Option<Timestamp>,
    /// The time of the last record, for aging.
    pub last_at: Timestamp,
    /// Whether a `rollback_intent` or `rolled_back` follows the apply.
    pub rollback_recorded: bool,
}

impl BlockState {
    /// Whether the kernel should still hold the element at `now`.
    #[must_use]
    pub fn live(&self, now: Timestamp) -> bool {
        match self.phase {
            Phase::Applied => self
                .applied_at
                .is_some_and(|at| now < at + SignedDuration::from_secs(i64::from(STEP1_TTL_S))),
            Phase::Confirmed => self.deadline.is_some_and(|d| now < d),
            _ => false,
        }
    }

    /// When the block stopped mattering, or `None` while it is live or in doubt.
    fn finished_at(&self, now: Timestamp) -> Option<Timestamp> {
        match self.phase {
            Phase::Applied | Phase::Confirmed if self.live(now) => None,
            Phase::Applied => self
                .applied_at
                .map(|at| at + SignedDuration::from_secs(i64::from(STEP1_TTL_S))),
            Phase::Confirmed => self.deadline,
            Phase::Intent(_) | Phase::RollbackIntent => None,
            Phase::RolledBack | Phase::Lost | Phase::Failed | Phase::Noop => Some(self.last_at),
        }
    }
}

/// The journal's records and the blocks they describe.
#[derive(Debug, Clone, Default)]
pub struct Loaded {
    /// Every record in order.
    pub records: Vec<JournalRecord>,
    /// Whether the final line was torn and ignored.
    pub torn: bool,
}

/// Folds `records` into one state per block, in first-seen order of the block ids.
#[must_use]
pub fn fold(records: &[JournalRecord]) -> Vec<BlockState> {
    let mut order: Vec<String> = Vec::new();
    let mut map: BTreeMap<String, BlockState> = BTreeMap::new();
    for record in records {
        match record {
            JournalRecord::Intent {
                op,
                block,
                source,
                set,
                ttl_s,
                at,
            } => {
                let entry = map.entry(block.clone()).or_insert_with(|| {
                    order.push(block.clone());
                    BlockState {
                        block: block.clone(),
                        source: source.clone(),
                        set: set.clone(),
                        ttl_s: *ttl_s,
                        phase: Phase::Intent(*op),
                        applied_at: None,
                        deadline: None,
                        last_at: *at,
                        rollback_recorded: false,
                    }
                });
                entry.phase = Phase::Intent(*op);
                entry.last_at = *at;
            }
            JournalRecord::Applied { block, at } => touch(&mut map, block, *at, |s| {
                s.phase = Phase::Applied;
                s.applied_at = Some(*at);
            }),
            JournalRecord::Noop { block } => {
                touch_no_time(&mut map, block, |s| s.phase = Phase::Noop)
            }
            JournalRecord::Failed { block, .. } => {
                touch_no_time(&mut map, block, |s| s.phase = Phase::Failed);
            }
            JournalRecord::Confirmed { block, deadline } => {
                touch(&mut map, block, *deadline, |s| {
                    s.phase = Phase::Confirmed;
                    s.deadline = Some(*deadline);
                })
            }
            JournalRecord::RollbackIntent { block, at } => touch(&mut map, block, *at, |s| {
                s.phase = Phase::RollbackIntent;
                s.rollback_recorded = true;
            }),
            JournalRecord::RolledBack { block, at } => touch(&mut map, block, *at, |s| {
                s.phase = Phase::RolledBack;
                s.rollback_recorded = true;
            }),
            JournalRecord::Lost { block, at } => {
                touch(&mut map, block, *at, |s| s.phase = Phase::Lost)
            }
            JournalRecord::TableRecreated { .. } | JournalRecord::Orphan { .. } => {}
        }
    }
    order.into_iter().filter_map(|b| map.remove(&b)).collect()
}

fn touch(
    map: &mut BTreeMap<String, BlockState>,
    block: &str,
    at: Timestamp,
    change: impl FnOnce(&mut BlockState),
) {
    if let Some(state) = map.get_mut(block) {
        change(state);
        state.last_at = at;
    }
}

fn touch_no_time(
    map: &mut BTreeMap<String, BlockState>,
    block: &str,
    change: impl FnOnce(&mut BlockState),
) {
    if let Some(state) = map.get_mut(block) {
        change(state);
    }
}

/// The journal file.
#[derive(Debug, Clone)]
pub struct Journal {
    path: PathBuf,
    dir: PathBuf,
}

impl Journal {
    /// The journal inside the state directory `dir`.
    #[must_use]
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join(JOURNAL_FILE),
            dir: dir.to_path_buf(),
        }
    }

    /// Reads the journal. A missing file is an empty journal.
    pub fn load(&self) -> Result<Loaded, JournalError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Loaded::default()),
            Err(e) => return Err(e.into()),
        };
        let read = read_journal(&text).map_err(|_| JournalError::Corrupt)?;
        Ok(Loaded {
            records: read.records,
            torn: read.torn_final_line,
        })
    }

    /// Appends `record` and syncs it before returning, so the record exists before the `nft`
    /// call it describes. `_guard` proves the state lock is held.
    pub fn append(
        &self,
        _guard: &LockGuard<'_>,
        record: &JournalRecord,
    ) -> Result<(), JournalError> {
        let line = record
            .to_line()
            .map_err(|e| JournalError::Io(std::io::Error::other(e)))?;
        let existed = self.path.exists();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .custom_flags(OFlag::O_NOFOLLOW.bits())
            .open(&self.path)?;
        let torn_tail = existed
            && std::fs::read(&self.path)
                .map(|bytes| bytes.last().is_some_and(|b| *b != b'\n'))
                .unwrap_or(false);
        let mut bytes = Vec::with_capacity(line.len() + 2);
        if torn_tail {
            bytes.push(b'\n');
        }
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        file.write_all(&bytes)?;
        file.sync_all()?;
        if !existed {
            std::fs::File::open(&self.dir)?.sync_all()?;
        }
        Ok(())
    }

    /// Drops the records of blocks that finished more than 24 hours before `now`, and every
    /// `orphan` record (the next list derives them again). Returns whether the file changed.
    pub fn compact(
        &self,
        guard: &LockGuard<'_>,
        loaded: &Loaded,
        now: Timestamp,
    ) -> Result<bool, JournalError> {
        let old: std::collections::BTreeSet<String> = fold(&loaded.records)
            .into_iter()
            .filter(|b| {
                b.finished_at(now)
                    .is_some_and(|at| now.duration_since(at) > KEEP)
            })
            .map(|b| b.block)
            .collect();
        let keep: Vec<&JournalRecord> = loaded
            .records
            .iter()
            .filter(|r| match r {
                JournalRecord::Orphan { .. } => false,
                JournalRecord::TableRecreated { at, .. } => now.duration_since(*at) <= KEEP,
                other => other.block().is_none_or(|b| !old.contains(b)),
            })
            .collect();
        if keep.len() == loaded.records.len() && !loaded.torn {
            return Ok(false);
        }
        let mut text = String::new();
        for record in keep {
            text.push_str(
                &record
                    .to_line()
                    .map_err(|e| JournalError::Io(std::io::Error::other(e)))?,
            );
            text.push('\n');
        }
        write_atomic_locked(
            guard,
            &self.dir,
            JOURNAL_FILE,
            text.as_bytes(),
            0o600,
            WriteFaults::default(),
        )
        .map_err(|e| JournalError::Io(std::io::Error::other(e.to_string())))?;
        Ok(true)
    }
}
