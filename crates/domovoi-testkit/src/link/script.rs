//! Test support for scripting what the fake link reports about a session.

use domovoi_core::ports::AgentSnapshot;

use super::FakeAgentLink;
use crate::lock;

impl FakeAgentLink {
    /// Answers the next `snapshot` calls from `answers`, first in first out, then falls back
    /// to the live state. It lets a test show a session changing between the catalogue triage
    /// read and the moment proposals are stored.
    pub fn script_snapshots(&self, answers: impl IntoIterator<Item = Option<AgentSnapshot>>) {
        let mut scripted = lock(&self.scripted_snapshots);
        scripted.clear();
        scripted.extend(answers);
    }

    /// How many times the executor asked for a session snapshot. Test support: it proves a
    /// negative dispatch test actually reached the dispatch path.
    #[must_use]
    pub fn snapshot_calls(&self) -> usize {
        self.snapshot_calls
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
