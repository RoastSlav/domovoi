//! The key lifecycle: `init` completion, `rotate-key`, `recover` and `fingerprint`.
//!
//! These are local commands for root, run by hand. Nothing in the serving path calls them, and
//! nothing here runs implicitly. A rotation or a recovery always ends under a new key: after a
//! rotation continuity is deliberately broken, and after state loss it cannot be proven. What is
//! retired is only public: the old public key, its fingerprint and a copy of the old state. The
//! old private key is never copied; the replacement of the key file frees it.

use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use domovoi_core::attest::BootId;
use domovoi_core::host::KeyFingerprint;
use jiff::Timestamp;

use super::keys::{Key, KeyFault, fingerprint_groups, key_id_of, load_key};
use super::state::StateFile;
use super::{KEY_FILE, RETIRED_DIR, STATE_FILE};
use crate::state::{
    InitError, LockError, StateLayout, StateLock, StateWriteError, WriteFaults, init,
    write_atomic_locked,
};

/// Why a lifecycle command stopped. Nothing was changed unless the variant says otherwise.
#[derive(Debug, thiserror::Error)]
pub enum LifecycleError {
    /// `init` found a key or a state file; it never replaces either.
    #[error("a key or a state file already exists; use `rotate-key` or `recover`")]
    AlreadyInitialised,
    /// `rotate-key` needs a usable key and a state file; use `recover` when one is lost.
    #[error("there is no usable key and state; use `recover`")]
    NotRotatable,
    /// `recover` found neither a key nor a state file; use `init`.
    #[error("there is no key or state to recover; use `init`")]
    NothingToRecover,
    /// `fingerprint` found no usable key.
    #[error("there is no usable key")]
    NoKey,
    /// The directory or lock facts are wrong; run `helper repair`.
    #[error("the state directory is not as expected")]
    Directory(#[from] InitError),
    /// The lock could not be taken.
    #[error("the state lock cannot be taken")]
    Lock(#[from] LockError),
    /// A file could not be made durable.
    #[error("a state file could not be written")]
    Write(#[from] StateWriteError),
    /// A system call failed.
    #[error("{path}: {source}")]
    Io {
        /// The path being created or read.
        path: PathBuf,
        /// What the system said.
        source: std::io::Error,
    },
}

/// The key a command made or looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInfo {
    /// Its fingerprint.
    pub fingerprint: KeyFingerprint,
    /// Its 16-character label.
    pub key_id: String,
}

impl KeyInfo {
    fn of(key: &Key) -> Self {
        let fingerprint = key.fingerprint();
        Self {
            key_id: key_id_of(&fingerprint),
            fingerprint,
        }
    }

    /// The fingerprint as the owner compares it: 64 hex characters in groups of four.
    #[must_use]
    pub fn grouped(&self) -> String {
        fingerprint_groups(&self.fingerprint)
    }
}

/// What a rotation or recovery did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    /// The key that was retired, when it could be read.
    pub old: Option<KeyInfo>,
    /// The new key.
    pub new: KeyInfo,
    /// The directory the public record of the old key was written to.
    pub retired: PathBuf,
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> LifecycleError {
    let path = path.to_path_buf();
    move |source| LifecycleError::Io { path, source }
}

fn sync_dir(dir: &Path) -> Result<(), LifecycleError> {
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io(dir))
}

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn private_dir(path: &Path) -> Result<(), LifecycleError> {
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(io(path))
}

/// Creates `path` exclusively with mode 0600, writes `bytes` and syncs it.
fn create_private_file(path: &Path, bytes: &[u8]) -> Result<(), LifecycleError> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
        .open(path)
        .map_err(io(path))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(io(path))?;
    file.write_all(bytes).map_err(io(path))?;
    file.sync_all().map_err(io(path))
}

/// Completes `helper init`: creates the key and the first state.
///
/// The directory and the lock are made first, as [`init`] always does. The key is created
/// exclusively (mode 0600, synced with its directory), then the state with generation 0 and zero
/// hashes under the lock. A crash between the two leaves a key without a state, which `recover`
/// resolves. It needs no valid configuration and no route observation.
///
/// # Errors
///
/// [`LifecycleError::AlreadyInitialised`] when a key or a state file exists; otherwise the first
/// failure.
pub fn init_attestation(
    layout: StateLayout,
    dir: &Path,
    boot_id: &BootId,
) -> Result<KeyInfo, LifecycleError> {
    if exists(&dir.join(KEY_FILE)) || exists(&dir.join(STATE_FILE)) {
        return Err(LifecycleError::AlreadyInitialised);
    }
    init(layout, dir)?;
    let (key, seed) = Key::generate();
    create_private_file(&dir.join(KEY_FILE), &seed)?;
    sync_dir(dir)?;
    let lock = StateLock::open(dir)?;
    let guard = lock.acquire()?;
    write_state(
        &guard,
        dir,
        &StateFile::fresh(key.fingerprint(), boot_id.clone()),
    )?;
    Ok(KeyInfo::of(&key))
}

fn write_state(
    guard: &crate::state::LockGuard<'_>,
    dir: &Path,
    state: &StateFile,
) -> Result<(), LifecycleError> {
    write_atomic_locked(
        guard,
        dir,
        STATE_FILE,
        &state.render(),
        0o600,
        WriteFaults::default(),
    )?;
    Ok(())
}

/// The retired record's directory name: UTC `YYYYMMDDTHHMMSSZ`.
fn retired_name(now: Timestamp) -> String {
    now.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// Writes what is public about the old key and a copy of the old state into
/// `retired/<timestamp>/`, durably. The old private key is not copied.
fn retire(
    dir: &Path,
    now: Timestamp,
    old: Option<&Key>,
    old_state: Option<&[u8]>,
) -> Result<PathBuf, LifecycleError> {
    let retired = dir.join(RETIRED_DIR);
    if !exists(&retired) {
        private_dir(&retired)?;
    }
    let target = retired.join(retired_name(now));
    private_dir(&target)?;
    if let Some(key) = old {
        let info = KeyInfo::of(key);
        let public = format!("{}\n", key.public_key_hex());
        create_private_file(&target.join("public.key"), public.as_bytes())?;
        let print = format!("{}\n{}\n", info.grouped(), info.key_id);
        create_private_file(&target.join("fingerprint"), print.as_bytes())?;
    }
    if let Some(bytes) = old_state {
        create_private_file(&target.join(STATE_FILE), bytes)?;
    }
    sync_dir(&target)?;
    sync_dir(&retired)?;
    sync_dir(dir)?;
    Ok(target)
}

/// Replaces the live key and state with a new pair, under the lock the caller holds.
fn replace(
    guard: &crate::state::LockGuard<'_>,
    dir: &Path,
    boot_id: &BootId,
) -> Result<Key, LifecycleError> {
    let (key, seed) = Key::generate();
    write_atomic_locked(guard, dir, KEY_FILE, &seed, 0o600, WriteFaults::default())?;
    write_state(
        guard,
        dir,
        &StateFile::fresh(key.fingerprint(), boot_id.clone()),
    )?;
    Ok(key)
}

/// Rotates the key: retires the old public record durably first, then writes the new key and a
/// new state (generation 0, zero hashes) and replaces the live files. The old private key is
/// deleted by that replacement; no copy of it exists anywhere in the tree.
///
/// A crash between the two replacements leaves the new key beside the old state, which the next
/// attestation reports as `state_key_mismatch`; `recover` resolves it.
///
/// # Errors
///
/// [`LifecycleError::NotRotatable`] without a usable key and a state file; otherwise the first
/// failure.
pub fn rotate_key(
    layout: StateLayout,
    dir: &Path,
    boot_id: &BootId,
    now: Timestamp,
) -> Result<Replaced, LifecycleError> {
    init(layout, dir)?;
    let lock = StateLock::open(dir)?;
    let guard = lock.acquire()?;
    let old = load_key(dir).map_err(|_| LifecycleError::NotRotatable)?;
    let state = std::fs::read(dir.join(STATE_FILE)).map_err(|_| LifecycleError::NotRotatable)?;
    let retired = retire(dir, now, Some(&old), Some(&state))?;
    let new = replace(&guard, dir, boot_id)?;
    Ok(Replaced {
        old: Some(KeyInfo::of(&old)),
        new: KeyInfo::of(&new),
        retired,
    })
}

/// Recovers from lost or damaged state: needs at least one of the key and the state file, retires
/// what can be read of them and always creates a new key, because continuity cannot be proven
/// after state loss.
///
/// # Errors
///
/// [`LifecycleError::NothingToRecover`] when neither file exists; otherwise the first failure.
pub fn recover(
    layout: StateLayout,
    dir: &Path,
    boot_id: &BootId,
    now: Timestamp,
) -> Result<Replaced, LifecycleError> {
    if !exists(&dir.join(KEY_FILE)) && !exists(&dir.join(STATE_FILE)) {
        return Err(LifecycleError::NothingToRecover);
    }
    init(layout, dir)?;
    let lock = StateLock::open(dir)?;
    let guard = lock.acquire()?;
    let old = load_key(dir).ok();
    let state = std::fs::read(dir.join(STATE_FILE)).ok();
    let retired = retire(dir, now, old.as_ref(), state.as_deref())?;
    let new = replace(&guard, dir, boot_id)?;
    Ok(Replaced {
        old: old.as_ref().map(KeyInfo::of),
        new: KeyInfo::of(&new),
        retired,
    })
}

/// The fingerprint and label of the key in `dir`, computed from the private key file.
///
/// # Errors
///
/// [`LifecycleError::NoKey`] when the key file is missing or is not a seed.
pub fn fingerprint(dir: &Path) -> Result<KeyInfo, LifecycleError> {
    match load_key(dir) {
        Ok(key) => Ok(KeyInfo::of(&key)),
        Err(KeyFault::Missing | KeyFault::Corrupt) => Err(LifecycleError::NoKey),
    }
}
