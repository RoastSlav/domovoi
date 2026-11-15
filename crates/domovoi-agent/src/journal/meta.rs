//! `action-journal.json`: the epoch, the sticky pruning marker and the current boot.

use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::JournalError;
use crate::fs_atomic::{
    DurableFaults, DurableWriteError, read_optional, write_atomic_durable_with,
};

/// The journal's metadata file.
pub const META_FILE: &str = "action-journal.json";
/// The metadata schema this build writes and accepts.
pub const META_FORMAT: u32 = 1;

/// The boot-load state written once per boot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootState {
    /// `/proc/sys/kernel/random/boot_id` as it was at first load.
    pub boot_id: String,
    /// `/proc/uptime` in nanoseconds at the moment this agent first loaded the journal.
    pub first_load_mono_ns: u64,
}

/// The single source of truth for the epoch, the pruning marker and the boot state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// The metadata schema version.
    pub format: u32,
    /// The journal epoch.
    pub epoch: Uuid,
    /// Sticky: once true, absent ids are `outcome_unknown` until a local reset.
    pub pruned: bool,
    /// The current boot's first journal load.
    pub boot: BootState,
}

impl Metadata {
    /// A fresh metadata object for a new epoch.
    #[must_use]
    pub fn new(epoch: Uuid, boot_id: impl Into<String>, first_load_mono_ns: u64) -> Self {
        Self {
            format: META_FORMAT,
            epoch,
            pruned: false,
            boot: BootState {
                boot_id: boot_id.into(),
                first_load_mono_ns,
            },
        }
    }

    /// Reads `dir/action-journal.json`, or `None` when it does not exist.
    pub fn load(dir: &Path) -> Result<Option<Self>, JournalError> {
        let path = dir.join(META_FILE);
        let Some(bytes) = read_optional(&path)? else {
            return Ok(None);
        };
        let meta: Self = serde_json::from_slice(&bytes)
            .map_err(|err| JournalError::Corrupt(format!("{META_FILE}: {err}")))?;
        if meta.format != META_FORMAT {
            return Err(JournalError::UnsupportedFormat(meta.format));
        }
        Ok(Some(meta))
    }

    /// Writes this complete object with the durable procedure.
    pub fn write(&self, dir: &Path, faults: DurableFaults) -> Result<(), DurableWriteError> {
        let path = dir.join(META_FILE);
        let bytes = serde_json::to_vec(self).map_err(|err| {
            DurableWriteError::BeforeCommit(crate::error::StateError::new(&path, err))
        })?;
        write_atomic_durable_with(&path, &bytes, 0o600, faults)
    }
}
