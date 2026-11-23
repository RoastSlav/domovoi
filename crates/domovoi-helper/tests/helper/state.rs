use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use domovoi_core::identity::{FaultKind, PathId};
use domovoi_helper::state::{
    InitError, InitOutcome, LockError, LockGuard, StateLayout, StateLock, StateWriteError,
    WriteFaults, init, write_atomic_locked,
};
use nix::unistd::{Gid, Uid};

fn layout() -> StateLayout {
    StateLayout {
        root_uid: Uid::effective().as_raw(),
        root_gid: Gid::effective().as_raw(),
    }
}

fn mode_of(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o7777
}

fn initialised() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("helper");
    assert_eq!(init(layout(), &dir).expect("init"), InitOutcome::Created);
    (tmp, dir)
}

#[test]
fn helper_init_creates_directory_0700_and_refuses_existing() {
    let (_tmp, dir) = initialised();
    assert_eq!(mode_of(&dir), 0o700);
    assert_eq!(mode_of(&dir.join("state.lock")), 0o600);

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let err = init(layout(), &dir).expect_err("a 0755 directory is never accepted");
    let InitError::Faults(faults) = err else {
        panic!("expected a fault list");
    };
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].id, PathId::HelperDir);
    assert_eq!(faults[0].why, FaultKind::Mode);
    assert_eq!(
        mode_of(&dir),
        0o755,
        "init must not correct what it refuses"
    );
}

#[test]
fn helper_init_is_idempotent_on_a_correct_layout() {
    let (_tmp, dir) = initialised();
    assert_eq!(
        init(layout(), &dir).expect("second init"),
        InitOutcome::AlreadyInitialised
    );
}

#[test]
fn helper_init_refuses_a_lock_with_the_wrong_mode_and_changes_nothing() {
    let (_tmp, dir) = initialised();
    let lock = dir.join("state.lock");
    std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o644)).expect("chmod");
    let err = init(layout(), &dir).expect_err("refused");
    assert!(matches!(err, InitError::Faults(f) if f[0].id == PathId::HelperLock));
    assert_eq!(mode_of(&lock), 0o644);
}

#[test]
fn state_lock_second_holder_gets_would_block_on_try() {
    let (_tmp, dir) = initialised();
    let first = StateLock::open(&dir).expect("open");
    let second = StateLock::open(&dir).expect("open");
    let guard = first.try_acquire().expect("lock").expect("free at first");
    assert!(second.try_acquire().expect("lock").is_none());
    drop(guard);
    assert!(second.try_acquire().expect("lock").is_some());
}

#[test]
fn state_lock_refuses_taking_the_same_lock_twice_in_one_process() {
    let (_tmp, dir) = initialised();
    let lock = StateLock::open(&dir).expect("open");
    let guard = lock.acquire().expect("first");
    assert!(matches!(lock.acquire(), Err(LockError::AlreadyHeld(_))));
    assert!(matches!(lock.try_acquire(), Err(LockError::AlreadyHeld(_))));
    drop(guard);
    assert!(lock.try_acquire().expect("lock").is_some());
}

#[test]
fn state_lock_refuses_a_symlink() {
    let (tmp, dir) = initialised();
    let target = tmp.path().join("elsewhere");
    std::fs::write(&target, b"").expect("write");
    std::fs::remove_file(dir.join("state.lock")).expect("remove");
    symlink(&target, dir.join("state.lock")).expect("symlink");
    assert!(matches!(StateLock::open(&dir), Err(LockError::Symlink(_))));
}

fn locked_write(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    mode: u32,
    faults: WriteFaults,
) -> Result<(), StateWriteError> {
    let lock = StateLock::open(dir).expect("open");
    let guard = lock.acquire().expect("acquire");
    write_atomic_locked(&guard, dir, name, bytes, mode, faults)
}

#[test]
fn write_atomic_locked_writes_the_file_with_the_requested_mode() {
    let (_tmp, dir) = initialised();
    locked_write(&dir, "state", b"one", 0o600, WriteFaults::default()).expect("write");
    assert_eq!(std::fs::read(dir.join("state")).expect("read"), b"one");
    assert_eq!(mode_of(&dir.join("state")), 0o600);
    assert!(!dir.join(".state.tmp").exists());
}

#[test]
fn write_atomic_locked_leaves_old_file_on_temp_sync_failure() {
    let (_tmp, dir) = initialised();
    locked_write(&dir, "state", b"old", 0o600, WriteFaults::default()).expect("write");
    let faults = WriteFaults {
        temp_sync: true,
        ..WriteFaults::default()
    };
    let err = locked_write(&dir, "state", b"new", 0o600, faults).expect_err("fails");
    assert!(matches!(err, StateWriteError::Failed { .. }));
    assert_eq!(std::fs::read(dir.join("state")).expect("read"), b"old");
}

#[test]
fn write_atomic_locked_leaves_old_file_on_rename_failure() {
    let (_tmp, dir) = initialised();
    locked_write(&dir, "state", b"old", 0o600, WriteFaults::default()).expect("write");
    let faults = WriteFaults {
        rename: true,
        ..WriteFaults::default()
    };
    let err = locked_write(&dir, "state", b"new", 0o600, faults).expect_err("fails");
    assert!(matches!(err, StateWriteError::Failed { .. }));
    assert_eq!(std::fs::read(dir.join("state")).expect("read"), b"old");
}

#[test]
fn write_atomic_locked_reports_durability_unknown_on_dir_sync_failure() {
    let (_tmp, dir) = initialised();
    let faults = WriteFaults {
        dir_sync: true,
        ..WriteFaults::default()
    };
    let err = locked_write(&dir, "state", b"new", 0o600, faults).expect_err("reported");
    assert!(matches!(err, StateWriteError::DurabilityUnknown { .. }));
    assert_eq!(
        std::fs::read(dir.join("state")).expect("read"),
        b"new",
        "the rename had already happened"
    );
}

#[test]
fn write_atomic_locked_removes_a_leftover_temp_file() {
    let (_tmp, dir) = initialised();
    let leftover = dir.join(".state.tmp");
    std::fs::write(&leftover, b"junk from a crash").expect("write");
    std::fs::set_permissions(&leftover, std::fs::Permissions::from_mode(0o666)).expect("chmod");
    locked_write(&dir, "state", b"fresh", 0o600, WriteFaults::default()).expect("write");
    assert_eq!(std::fs::read(dir.join("state")).expect("read"), b"fresh");
    assert_eq!(mode_of(&dir.join("state")), 0o600);
    assert!(!leftover.exists());
}

#[test]
fn write_atomic_locked_rejects_a_name_that_is_not_one_component() {
    let (_tmp, dir) = initialised();
    for name in ["", "..", "a/b"] {
        let err = locked_write(&dir, name, b"x", 0o600, WriteFaults::default());
        assert!(
            matches!(err, Err(StateWriteError::Failed { .. })),
            "{name:?}"
        );
    }
}

type GuardedWrite = fn(&LockGuard<'_>, &Path, &str, &[u8], u32, WriteFaults) -> WriteResult;
type WriteResult = Result<(), StateWriteError>;

#[test]
fn write_atomic_locked_requires_the_guard() {
    // The signature takes the guard, so "inside the lock" is a fact of the types.
    let _: GuardedWrite = write_atomic_locked;
}
