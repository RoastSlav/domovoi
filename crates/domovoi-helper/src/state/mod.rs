//! The helper's state directory: creation, the lock and atomic writes.
//!
//! Everything here is an ordinary-file operation under `/var/lib/domovoi-helper`. The lock file
//! serialises helper instances, and the atomic write takes proof that the lock is held as an
//! argument so "inside the lock" is a fact of the types.

mod atomic;
mod init;
mod lock;

pub use atomic::{StateWriteError, WriteFaults, write_atomic_locked};
pub use init::{InitError, InitOutcome, StateLayout, init};
pub use lock::{LOCK_FILE, LockError, LockGuard, StateLock};
