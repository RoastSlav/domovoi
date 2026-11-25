//! The agent-side cache, single flight and rate limit of helper reads. The helper keeps no
//! state between requests (one process per connection), so all of it lives here, and every
//! decision reads time from the injected clock.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};

use domovoi_core::helper::{HelperRequest, HelperResponse};
use domovoi_core::ports::Clock;
use jiff::{SignedDuration, Timestamp};
use tokio::sync::watch;

use super::reads::ReadClientError;

/// Requests one client may send in one [`RATE_WINDOW`].
const RATE_LIMIT: usize = 20;
/// The sliding window of the local rate limit.
const RATE_WINDOW: SignedDuration = SignedDuration::from_secs(10);

/// What one finished exchange gave, shared with the callers that waited for it.
type Outcome = Result<HelperResponse, ReadClientError>;

#[derive(Debug)]
struct Entry {
    expires: Timestamp,
    response: HelperResponse,
}

#[derive(Debug, Default)]
struct State {
    entries: HashMap<String, Entry>,
    flights: HashMap<String, watch::Receiver<Option<Outcome>>>,
    sent: VecDeque<Timestamp>,
}

/// Caches the answers of cheap reads for a few seconds, coalesces identical concurrent
/// requests into one exchange and refuses the 21st request in 10 seconds without sending it.
/// Errors are never cached.
#[derive(Debug, Default)]
pub struct ReadCache {
    state: Mutex<State>,
}

/// The cache key and lifetime of `request`, or `None` when evidence must be fresh.
fn policy(request: &HelperRequest) -> Option<(String, SignedDuration)> {
    let (key, secs) = match request {
        HelperRequest::DockerProbe => ("docker_probe".to_owned(), 30),
        HelperRequest::DockerList => ("docker_list".to_owned(), 5),
        HelperRequest::NetListeners => ("net_listeners".to_owned(), 5),
        HelperRequest::SmartScan => ("smart_scan".to_owned(), 300),
        HelperRequest::SmartReport { device } => (format!("smart_report:{device}"), 60),
        _ => return None,
    };
    Some((key, SignedDuration::from_secs(secs)))
}

/// Whether `response` is an answer worth keeping; failures and refusals never are.
fn cacheable(response: &HelperResponse) -> bool {
    matches!(
        response,
        HelperResponse::DockerEngine { .. }
            | HelperResponse::DockerContainers { .. }
            | HelperResponse::NetListeners { .. }
            | HelperResponse::SmartScan { .. }
            | HelperResponse::SmartReport { .. }
    )
}

/// What this call does about `key`.
enum Step {
    Hit(HelperResponse),
    Wait(watch::Receiver<Option<Outcome>>),
    Lead(watch::Sender<Option<Outcome>>),
}

/// Removes a flight when its leader finishes or is dropped, so followers never wait on a
/// leader that is gone.
struct Flight<'a> {
    cache: &'a ReadCache,
    key: String,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.cache.lock().flights.remove(&self.key);
    }
}

impl ReadCache {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Counts one request against the window, or says the window is full.
    fn admit(state: &mut State, now: Timestamp) -> Result<(), ReadClientError> {
        let cutoff = now - RATE_WINDOW;
        while state.sent.front().is_some_and(|at| *at <= cutoff) {
            state.sent.pop_front();
        }
        if state.sent.len() >= RATE_LIMIT {
            return Err(ReadClientError::RateLimited);
        }
        state.sent.push_back(now);
        Ok(())
    }

    /// Answers `request` from the cache, from the call another caller already has in flight,
    /// or by calling `send` once.
    pub(super) async fn fetch<F, Fut>(
        &self,
        request: &HelperRequest,
        clock: &dyn Clock,
        send: F,
    ) -> Outcome
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Outcome>,
    {
        let Some((key, ttl)) = policy(request) else {
            Self::admit(&mut self.lock(), clock.now())?;
            return send().await;
        };
        let mut send = Some(send);
        loop {
            let step = {
                let mut state = self.lock();
                let now = clock.now();
                if let Some(entry) = state.entries.get(&key).filter(|e| now < e.expires) {
                    Step::Hit(entry.response.clone())
                } else if let Some(flight) = state.flights.get(&key) {
                    Step::Wait(flight.clone())
                } else {
                    Self::admit(&mut state, now)?;
                    let (tx, rx) = watch::channel(None);
                    state.flights.insert(key.clone(), rx);
                    Step::Lead(tx)
                }
            };
            match step {
                Step::Hit(response) => return Ok(response),
                Step::Wait(mut flight) => {
                    if let Ok(done) = flight.wait_for(Option::is_some).await
                        && let Some(outcome) = done.clone()
                    {
                        return outcome;
                    }
                    // The leader was dropped before it answered: try again.
                }
                Step::Lead(tx) => {
                    let guard = Flight { cache: self, key };
                    let Some(send) = send.take() else {
                        return Err(ReadClientError::Protocol);
                    };
                    let outcome = send().await;
                    if let Ok(response) = &outcome
                        && cacheable(response)
                    {
                        let now = clock.now();
                        let expires = now.saturating_add(ttl).unwrap_or(now);
                        let mut state = self.lock();
                        state.entries.retain(|_, e| now < e.expires);
                        state.entries.insert(
                            guard.key.clone(),
                            Entry {
                                expires,
                                response: response.clone(),
                            },
                        );
                    }
                    tx.send_replace(Some(outcome.clone()));
                    return outcome;
                }
            }
        }
    }
}
