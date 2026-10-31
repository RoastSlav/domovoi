//! Time.

use async_trait::async_trait;
use jiff::Timestamp;

/// The current time and a way to wait for a later one.
///
/// All time-dependent logic takes a `Clock` so tests can drive time with a fake.
#[async_trait]
pub trait Clock: Send + Sync {
    /// The current instant (UTC).
    fn now(&self) -> Timestamp;

    /// Completes once [`now`](Self::now) is at or after `t`. Completes at once when `t`
    /// is not in the future. Cancel-safe: dropping the future has no effect.
    async fn sleep_until(&self, t: Timestamp);
}
