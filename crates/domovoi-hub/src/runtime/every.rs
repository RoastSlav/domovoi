//! Running a job on a fixed period of clock time.

use std::sync::Arc;

use domovoi_core::ports::Clock;
use jiff::SignedDuration;
use tokio_util::sync::CancellationToken;

/// Runs `job` now, then again `period` after each run *started*, until `shutdown`.
///
/// Scheduling from the start, not the end, matters when the clock moves while the job
/// runs: a slow run, or a test setting the fake clock forward. The next run is then due
/// at once, instead of `period` after the new time, so nothing waits a whole period late.
pub(super) async fn every<F, Fut>(
    clock: Arc<dyn Clock>,
    period: SignedDuration,
    shutdown: CancellationToken,
    mut job: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    loop {
        let started = clock.now();
        job().await;
        let next = started.checked_add(period).unwrap_or(started);
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = clock.sleep_until(next) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use domovoi_core::ports::Clock;
    use domovoi_testkit::FakeClock;
    use jiff::{SignedDuration, Timestamp};
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::every;

    #[tokio::test]
    async fn a_clock_jump_during_a_run_makes_the_next_run_due_at_once() {
        let start: Timestamp = "2026-10-31T22:30:00Z".parse().unwrap();
        let clock = Arc::new(FakeClock::new(start));
        let shutdown = CancellationToken::new();
        let runs = Arc::new(AtomicU32::new(0));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let job = {
            let clock = clock.clone();
            let runs = runs.clone();
            move || {
                let clock = clock.clone();
                let runs = runs.clone();
                let tx = tx.clone();
                async move {
                    // The first run sees the clock jump forward while it works.
                    if runs.fetch_add(1, Ordering::SeqCst) == 0 {
                        clock.advance(SignedDuration::from_hours(9));
                    }
                    let _ = tx.send(clock.now());
                }
            }
        };
        let dyn_clock: Arc<dyn Clock> = clock.clone();
        let task = tokio::spawn(every(
            dyn_clock,
            SignedDuration::from_mins(1),
            shutdown.clone(),
            job,
        ));
        let guard = std::time::Duration::from_secs(10);
        let first = tokio::time::timeout(guard, rx.recv())
            .await
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(guard, rx.recv())
            .await
            .expect("the second run must not wait for another minute of clock time")
            .unwrap();
        assert_eq!(
            first, second,
            "it ran again at once, without the clock moving"
        );
        assert_eq!(
            runs.load(Ordering::SeqCst),
            2,
            "then it waits a full minute"
        );
        shutdown.cancel();
        task.await.unwrap();
    }
}
