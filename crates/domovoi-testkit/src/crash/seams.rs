//! Test-only seams in the crash wrapper.
//!
//! A regression test for commit ordering has to hold one commit open at exactly the points
//! where two commits could otherwise interleave, and make a snapshot fail on demand. These
//! seams exist only for the unit tests next to this module; nothing here is compiled into the
//! testkit library, and nothing here changes what the wrapper does when it is not armed.

use std::sync::Arc;

use domovoi_core::ports::StoreError;

use super::{CrashingStore, SweepStore};
use crate::lock;

/// A pause a test can arm, wait for and release.
#[derive(Default)]
pub(crate) struct Hold {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl Hold {
    /// Tells the test that a commit is paused here, then waits for it to be let go.
    pub(super) async fn pause(&self) {
        self.entered.notify_one();
        self.release.notified().await;
    }

    /// Waits until the commit holding this pause has reached it.
    pub(crate) async fn entered(&self) {
        self.entered.notified().await;
    }

    /// Lets the paused commit continue.
    pub(crate) fn release(&self) {
        self.release.notify_one();
    }
}

impl<S: SweepStore> CrashingStore<S> {
    /// Pauses the next counted commit after it is counted and before it is applied, so a test
    /// can try to start another commit in that window.
    pub(crate) fn hold_before_apply(&self) -> Arc<Hold> {
        let hold = Arc::new(Hold::default());
        *lock(&self.apply_hold) = Some(hold.clone());
        hold
    }

    /// Pauses the next recorded commit between its durable write and its snapshot, so a test
    /// can attempt another commit in exactly the window a real one would race.
    pub(crate) fn hold_next_snapshot(&self) -> Arc<Hold> {
        let hold = Arc::new(Hold::default());
        *lock(&self.hold) = Some(hold.clone());
        hold
    }

    /// Makes the next recorded commit's snapshot fail with `error`, the way a store read
    /// failure would.
    pub(crate) fn fail_next_snapshot(&self, error: StoreError) {
        *lock(&self.snapshot_failure) = Some(error);
    }
}
