//! `checks.json`: each check's own state, and which targets the agent last saw firing.
//!
//! The firing memory sits beside each check's state rather than inside it, so it can never
//! collide with a key the check uses itself.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use domovoi_checks::framework::CheckState;
use domovoi_core::CheckKey;
use serde::{Deserialize, Serialize};

use crate::error::StateError;
use crate::fs_atomic::{read_optional, write_atomic};

/// The check state file under the state directory.
pub const CHECKS_FILE: &str = "checks.json";
const MODE: u32 = 0o600;

/// The target recorded for observations that have none.
pub const NO_TARGET: &str = "";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct Contents {
    #[serde(default)]
    states: BTreeMap<CheckKey, CheckState>,
    #[serde(default)]
    firing: BTreeMap<CheckKey, BTreeSet<String>>,
}

/// Check states and firing memory, shared by the scheduler and `run_check`.
#[derive(Debug)]
pub struct CheckStore {
    path: PathBuf,
    contents: Mutex<Contents>,
}

impl CheckStore {
    /// Opens `checks.json` in `state_dir`; a missing or unreadable file starts empty.
    pub fn open(state_dir: &Path) -> Result<Self, StateError> {
        let path = state_dir.join(CHECKS_FILE);
        let contents = match read_optional(&path)? {
            Some(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                tracing::warn!(%err, "check state is unreadable; starting over");
                Contents::default()
            }),
            None => Contents::default(),
        };
        Ok(Self {
            path,
            contents: Mutex::new(contents),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Contents> {
        self.contents.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A copy of `key`'s state, empty for a check that never ran.
    #[must_use]
    pub fn state(&self, key: &CheckKey) -> CheckState {
        self.lock().states.get(key).cloned().unwrap_or_default()
    }

    /// Stores `key`'s state after a run.
    pub fn put_state(&self, key: &CheckKey, state: CheckState) {
        self.lock().states.insert(key.clone(), state);
    }

    /// The targets of `key` that were firing after its last run.
    #[must_use]
    pub fn firing(&self, key: &CheckKey) -> BTreeSet<String> {
        self.lock().firing.get(key).cloned().unwrap_or_default()
    }

    /// Records the targets of `key` that are firing now.
    pub fn set_firing(&self, key: &CheckKey, targets: BTreeSet<String>) {
        let mut contents = self.lock();
        if targets.is_empty() {
            contents.firing.remove(key);
        } else {
            contents.firing.insert(key.clone(), targets);
        }
    }

    /// Writes everything to `checks.json` atomically.
    pub fn save(&self) -> Result<(), StateError> {
        let bytes =
            serde_json::to_vec(&*self.lock()).map_err(|e| StateError::new(&self.path, e))?;
        write_atomic(&self.path, &bytes, MODE)
    }
}
