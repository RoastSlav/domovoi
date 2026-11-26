//! Test support for scripting what the fake link reports about a session.

use super::FakeAgentLink;

impl FakeAgentLink {
    /// How many times the executor asked for a session snapshot. Test support: it proves a
    /// negative dispatch test actually reached the dispatch path.
    #[must_use]
    pub fn snapshot_calls(&self) -> usize {
        self.snapshot_calls
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
