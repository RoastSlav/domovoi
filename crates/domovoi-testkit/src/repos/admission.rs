//! The in-memory hourly model-run window.

use async_trait::async_trait;
use domovoi_core::ports::{AdmissionOutcome, AdmissionPurpose, AdmissionRepo, StoreError};
use jiff::{SignedDuration, Timestamp};

use super::InMemoryStore;

/// The starts inside the window, oldest first.
fn inside(state: &super::State, since_ms: i64) -> Vec<Timestamp> {
    let mut starts: Vec<Timestamp> = state
        .admissions
        .iter()
        .map(|(_, at)| *at)
        .filter(|at| at.as_millisecond() > since_ms)
        .collect();
    starts.sort();
    starts
}

#[async_trait]
impl AdmissionRepo for InMemoryStore {
    async fn admit(
        &self,
        purpose: AdmissionPurpose,
        room: u32,
        window: SignedDuration,
        now: Timestamp,
    ) -> Result<AdmissionOutcome, StoreError> {
        let mut state = self.begin()?;
        let window_ms = i64::try_from(window.as_millis()).unwrap_or(i64::MAX);
        let now_ms = now.as_millisecond();
        let since = now_ms.saturating_sub(window_ms);
        let prune_before = now_ms.saturating_sub(window_ms.saturating_mul(2));
        state
            .admissions
            .retain(|(_, at)| at.as_millisecond() >= prune_before);
        let starts = inside(&state, since);
        if starts.len() >= usize::try_from(room).unwrap_or(usize::MAX) {
            return Ok(AdmissionOutcome::Full { starts });
        }
        state.admissions.push((purpose, now));
        Ok(AdmissionOutcome::Admitted)
    }

    async fn starts_since(&self, since: Timestamp) -> Result<Vec<Timestamp>, StoreError> {
        let state = self.begin()?;
        Ok(inside(&state, since.as_millisecond()))
    }
}
