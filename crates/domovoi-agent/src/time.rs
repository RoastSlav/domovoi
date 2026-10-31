//! Waiting through the `Clock` port, so tests drive time with a fake clock.

use std::future::Future;

use domovoi_core::ports::Clock;
use jiff::SignedDuration;

/// Runs `fut` until it finishes or `limit` passes on `clock`; `None` on timeout.
pub async fn within<F: Future>(
    clock: &dyn Clock,
    limit: SignedDuration,
    fut: F,
) -> Option<F::Output> {
    let deadline = clock.now().saturating_add(limit).unwrap_or(clock.now());
    tokio::select! {
        out = fut => Some(out),
        () = clock.sleep_until(deadline) => None,
    }
}

/// Sleeps for `by` on `clock`.
pub async fn sleep(clock: &dyn Clock, by: SignedDuration) {
    let until = clock.now().saturating_add(by).unwrap_or(clock.now());
    clock.sleep_until(until).await;
}
