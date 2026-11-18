//! The gates a test can hold one fake link's answers with.
//!
//! [`FakeAgentLink`]: super::FakeAgentLink

use std::sync::Arc;

/// A held availability check: the gate that keeps it pending, and a notifier that fires when
/// the check has been entered.
pub(super) struct AvailabilityHold {
    pub(super) gate: Arc<tokio::sync::Semaphore>,
    pub(super) entered: Arc<tokio::sync::Notify>,
}

impl AvailabilityHold {
    /// A hold over `gate`, with the notifier a waiting test listens on.
    pub(super) fn new(gate: Arc<tokio::sync::Semaphore>) -> Self {
        Self {
            gate,
            entered: Arc::new(tokio::sync::Notify::new()),
        }
    }
}
