//! The private staging area inside `packages_dir`: received transfers waiting to be used and
//! the per-execution extraction directories.
//!
//! The directory is wiped at every start, so a restart never trusts a half-received or
//! half-extracted file. Everything here is bookkeeping around paths built from a `Uuid`,
//! never from request text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use domovoi_core::package::PackageFailureCode;
use domovoi_core::ports::Clock;
use domovoi_proto::messages::TransferKind;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

use super::error::PackageError;
use super::layout::{PRIVATE_DIR_MODE, STAGING_DIR, make_dir};

/// How long a stored transfer waits to be used before it is deleted.
pub const STAGED_TTL: SignedDuration = SignedDuration::from_mins(15);

/// A stored transfer.
#[derive(Debug, Clone, Copy)]
struct Staged {
    kind: TransferKind,
    size: u64,
    stored_at: Timestamp,
    /// Taken by an install: exempt from the unused-transfer TTL until it is deleted.
    claimed: bool,
}

/// A transfer taken for use by an install. The file stays until [`Staging::delete_claimed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedTransfer {
    /// The stored bytes.
    pub path: PathBuf,
    /// The transfer id.
    pub id: Uuid,
}

/// The staging area. Shared as `Arc<Staging>` between the transfer receiver and installs.
pub struct Staging {
    root: PathBuf,
    clock: Arc<dyn Clock>,
    staged: Mutex<BTreeMap<Uuid, Staged>>,
}

impl std::fmt::Debug for Staging {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Staging").field("root", &self.root).finish()
    }
}

impl Staging {
    /// Wipes and recreates `<packages_dir>/.staging` (0700) and `.staging/transfers`.
    ///
    /// # Errors
    /// A `storage` failure when the area cannot be recreated.
    pub fn open(packages_dir: &Path, clock: Arc<dyn Clock>) -> Result<Arc<Self>, PackageError> {
        let root = packages_dir.join(STAGING_DIR);
        match std::fs::remove_dir_all(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PackageError::io("clearing the staging area", &error)),
        }
        make_dir(&root, PRIVATE_DIR_MODE)?;
        make_dir(&root.join("transfers"), PRIVATE_DIR_MODE)?;
        Ok(Arc::new(Self {
            root,
            clock,
            staged: Mutex::new(BTreeMap::new()),
        }))
    }

    fn table(&self) -> std::sync::MutexGuard<'_, BTreeMap<Uuid, Staged>> {
        self.staged.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The directory holding transfer files.
    #[must_use]
    pub fn transfers_dir(&self) -> PathBuf {
        self.root.join("transfers")
    }

    /// Where bytes of transfer `id` are written while they arrive.
    #[must_use]
    pub fn part_path(&self, id: Uuid) -> PathBuf {
        self.transfers_dir().join(format!("{id}.part"))
    }

    /// Where a finished transfer `id` is kept.
    #[must_use]
    pub fn bin_path(&self, id: Uuid) -> PathBuf {
        self.transfers_dir().join(format!("{id}.bin"))
    }

    /// Records that `<id>.bin` now holds `size` bytes of `kind`, stored at the clock's now.
    pub fn register_staged(&self, id: Uuid, kind: TransferKind, size: u64) {
        let stored_at = self.clock.now();
        self.table().insert(
            id,
            Staged {
                kind,
                size,
                stored_at,
                claimed: false,
            },
        );
    }

    /// How many stored transfers wait to be used.
    #[must_use]
    pub fn staged_count(&self) -> usize {
        self.table().len()
    }

    /// The total bytes of stored transfers.
    #[must_use]
    pub fn staged_bytes(&self) -> u64 {
        self.table()
            .values()
            .fold(0_u64, |sum, staged| sum.saturating_add(staged.size))
    }

    /// Whether `id` is a stored transfer.
    #[must_use]
    pub fn is_staged(&self, id: Uuid) -> bool {
        self.table().contains_key(&id)
    }

    /// Takes stored transfer `id` of `kind` for use. The check and the hand-over happen under
    /// one lock, so the expiry sweep can never delete a transfer an install is reading: a
    /// claimed transfer is exempt from the TTL until [`Staging::delete_claimed`].
    ///
    /// # Errors
    /// `transfer_unknown` when it is not stored or is already claimed, `transfer_expired` when
    /// it waited longer than the TTL (it is deleted), `transfer_kind_mismatch` when it holds
    /// the other kind.
    pub fn claim(&self, id: Uuid, kind: TransferKind) -> Result<ClaimedTransfer, PackageError> {
        let now = self.clock.now();
        let mut table = self.table();
        let Some(staged) = table.get_mut(&id) else {
            return Err(PackageError::new(
                PackageFailureCode::TransferUnknown,
                format!("transfer {id} is not stored on this agent"),
            ));
        };
        if staged.claimed {
            return Err(PackageError::new(
                PackageFailureCode::TransferUnknown,
                format!("transfer {id} is already in use"),
            ));
        }
        if expired(staged, now) {
            table.remove(&id);
            drop(table);
            let _ = std::fs::remove_file(self.bin_path(id));
            return Err(PackageError::new(
                PackageFailureCode::TransferExpired,
                format!("transfer {id} waited too long and was deleted"),
            ));
        }
        if staged.kind != kind {
            return Err(PackageError::new(
                PackageFailureCode::TransferKindMismatch,
                format!("transfer {id} holds a different kind of data"),
            ));
        }
        staged.claimed = true;
        Ok(ClaimedTransfer {
            path: self.bin_path(id),
            id,
        })
    }

    /// Deletes a claimed transfer's file and forgets it.
    pub fn delete_claimed(&self, claimed: &ClaimedTransfer) {
        self.forget(claimed.id);
    }

    /// Deletes stored transfer `id`'s file (best effort) and forgets it.
    pub fn forget(&self, id: Uuid) {
        self.table().remove(&id);
        let _ = std::fs::remove_file(self.bin_path(id));
    }

    /// Deletes every unclaimed stored transfer unused for longer than the TTL; returns their ids.
    pub fn expire_staged(&self, now: Timestamp) -> Vec<Uuid> {
        let old: Vec<Uuid> = self
            .table()
            .iter()
            .filter(|(_, staged)| expired(staged, now))
            .map(|(id, _)| *id)
            .collect();
        for id in &old {
            self.forget(*id);
        }
        old
    }

    /// The extraction directory of execution `exec`: `.staging/<exec>/tree`.
    #[must_use]
    pub fn exec_tree(&self, exec: Uuid) -> PathBuf {
        self.exec_dir(exec).join("tree")
    }

    /// The execution's own staging directory `.staging/<exec>`.
    #[must_use]
    pub fn exec_dir(&self, exec: Uuid) -> PathBuf {
        self.root.join(exec.to_string())
    }

    /// Creates `.staging/<exec>` (0700); the extraction creates `tree` itself.
    ///
    /// # Errors
    /// A `storage` failure.
    pub fn make_exec_dir(&self, exec: Uuid) -> Result<(), PackageError> {
        make_dir(&self.exec_dir(exec), PRIVATE_DIR_MODE)
    }

    /// Removes `.staging/<exec>` and everything in it (best effort).
    pub fn remove_exec_dir(&self, exec: Uuid) {
        let _ = std::fs::remove_dir_all(self.exec_dir(exec));
    }
}

fn expired(staged: &Staged, now: Timestamp) -> bool {
    !staged.claimed && now.duration_since(staged.stored_at) >= STAGED_TTL
}
