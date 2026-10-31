//! A clock that only moves when a test moves it.

use async_trait::async_trait;
use domovoi_core::ports::Clock;
use jiff::{SignedDuration, Timestamp};
use tokio::sync::watch;

/// A [`Clock`] that starts at a fixed instant and never reads real time.
///
/// [`advance`](Self::advance) and [`set`](Self::set) wake every pending
/// [`sleep_until`](Clock::sleep_until) whose target has been reached.
#[derive(Debug)]
pub struct FakeClock {
    now: watch::Sender<Timestamp>,
}

impl FakeClock {
    /// A clock reading `start`.
    #[must_use]
    pub fn new(start: Timestamp) -> Self {
        Self {
            now: watch::Sender::new(start),
        }
    }

    /// Moves the clock forward by `by`. Negative or overflowing durations leave it unchanged.
    pub fn advance(&self, by: SignedDuration) {
        let current = *self.now.borrow();
        if by.is_negative() {
            return;
        }
        if let Ok(next) = current.checked_add(by) {
            self.now.send_replace(next);
        }
    }

    /// Moves the clock to `t`, which must not be earlier than now; earlier instants are ignored.
    pub fn set(&self, t: Timestamp) {
        self.now.send_if_modified(|now| {
            let later = t > *now;
            if later {
                *now = t;
            }
            later
        });
    }
}

#[async_trait]
impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *self.now.borrow()
    }

    async fn sleep_until(&self, t: Timestamp) {
        let mut rx = self.now.subscribe();
        // The sender lives as long as `self`, so `wait_for` only fails if the clock is gone.
        let _ = rx.wait_for(|now| *now >= t).await;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[tokio::test]
    async fn fake_clock_wakes_sleepers_only_when_advanced() {
        let start = Timestamp::constant(1_700_000_000, 0);
        let clock = Arc::new(FakeClock::new(start));
        let target = start.checked_add(SignedDuration::from_secs(60)).unwrap();

        let sleeper = {
            let clock = Arc::clone(&clock);
            tokio::spawn(async move { clock.sleep_until(target).await })
        };
        tokio::task::yield_now().await;
        assert!(!sleeper.is_finished(), "woke without the clock moving");

        clock.advance(SignedDuration::from_secs(59));
        tokio::task::yield_now().await;
        assert!(!sleeper.is_finished(), "woke before the target");

        clock.advance(SignedDuration::from_secs(1));
        sleeper.await.unwrap();
        assert_eq!(clock.now(), target);
    }

    #[tokio::test]
    async fn past_targets_complete_at_once_and_time_never_goes_back() {
        let start = Timestamp::constant(1_700_000_000, 0);
        let clock = FakeClock::new(start);
        clock.sleep_until(start).await;
        clock.advance(SignedDuration::from_secs(-5));
        clock.set(Timestamp::constant(1_600_000_000, 0));
        assert_eq!(clock.now(), start);
    }
}
