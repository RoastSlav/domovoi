//! Test-only fault injection for the journal's durability paths.
//!
//! Production never calls these; they make each filesystem stage deterministically fail so
//! the recovery behaviour, not just the returned error, can be asserted.

use super::Journal;
#[cfg(test)]
use super::compact::RewriteFaults;
use super::store::AppendFault;
use crate::fs_atomic::DurableFaults;

impl Journal {
    /// Injects the next metadata write failures.
    #[doc(hidden)]
    pub fn inject_metadata_faults(&self, faults: DurableFaults) {
        if let Ok(mut state) = self.state.lock() {
            state.meta_faults = faults;
        }
    }

    /// Injects the next append failure.
    #[doc(hidden)]
    pub fn inject_append_fault(&self, fault: AppendFault) {
        if let Ok(mut state) = self.state.lock() {
            state.append_fault = fault;
        }
    }

    /// Injects the next commit-boundary write failures.
    #[doc(hidden)]
    pub fn inject_commit_faults(&self, faults: DurableFaults) {
        if let Ok(mut state) = self.state.lock() {
            state.commit_faults = faults;
        }
    }

    /// Makes the recovery confirmation of an indeterminate commit boundary fail.
    #[doc(hidden)]
    pub fn inject_commit_confirm_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.confirm_fails = true;
        }
    }

    /// Makes the next append's repair truncation fail.
    #[doc(hidden)]
    pub fn inject_truncate_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.truncate_fails = true;
        }
    }

    /// Injects the next compaction failure. Test-only.
    #[cfg(test)]
    pub(super) fn inject_rewrite_faults(&self, faults: RewriteFaults) {
        if let Ok(mut state) = self.state.lock() {
            state.rewrite_faults = faults;
        }
    }

    /// Makes the next compaction fail its directory sync and its recovery sync. Test-only.
    #[cfg(test)]
    pub(super) fn inject_rewrite_dir_sync_failure(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.rewrite_faults = RewriteFaults {
                journal_dir_sync: true,
                journal_dir_recovery_fails: true,
                ..RewriteFaults::default()
            };
        }
    }

    /// Poisons the state mutex. Test-only.
    #[cfg(test)]
    pub(crate) fn poison_state_for_test(&self) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = self.state.lock().expect("the journal lock is healthy");
            panic!("poison the journal lock");
        }));
    }
}
