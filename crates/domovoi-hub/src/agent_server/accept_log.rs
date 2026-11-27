//! What the agent-port accept loop does when `accept` fails: back off, and log once per
//! interval with a count instead of once per failure.
//!
//! A hub that has run out of file descriptors fails `accept` at once, every time. Retrying
//! with no pause burns a core and writes the same line hundreds of thousands of times, which
//! also hides every other line.

use std::time::{Duration, Instant};

use domovoi_core::log_gate::{Gate, RateLimitedLog};

use super::conn_gate::{ConnLimits, Refused};

/// The pause after the first failed accept.
const FIRST_PAUSE: Duration = Duration::from_millis(100);
/// The longest pause between accepts while they keep failing.
const LONGEST_PAUSE: Duration = Duration::from_secs(1);
/// How often a failing accept loop writes a line.
const LINE_EVERY: Duration = Duration::from_secs(10);

/// Waits `pause` on `clock`; returns true when `stop` fired first and the loop should end.
pub(crate) async fn wait(
    clock: &dyn domovoi_core::ports::Clock,
    stop: &tokio_util::sync::CancellationToken,
    pause: Duration,
) -> bool {
    let now = clock.now();
    let until = jiff::SignedDuration::try_from(pause)
        .ok()
        .and_then(|pause| now.checked_add(pause).ok())
        .unwrap_or(now);
    tokio::select! {
        () = stop.cancelled() => true,
        () = clock.sleep_until(until) => false,
    }
}

/// Accepts agents on `listener` until the server shuts down, pausing and logging sparingly
/// while `accept` keeps failing.
pub(crate) async fn serve(
    server: &super::AgentServer,
    listener: tokio::net::TcpListener,
) -> Result<(), super::AgentServerError> {
    let inner = &server.inner;
    let mut errors = AcceptErrors::new();
    let refusals = Refusals::new();
    loop {
        let accepted = tokio::select! {
            () = inner.shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => accepted,
        };
        match accepted {
            Ok((tcp, peer)) => {
                errors.succeeded();
                match inner.preauth.try_acquire(peer.ip()) {
                    Ok(permit) => {
                        let server = server.clone();
                        tokio::spawn(async move {
                            super::conn::accept(&server.inner, tcp, peer, permit).await;
                        });
                    }
                    Err(refused) => {
                        // Closing at once costs the sender nothing it had not already spent,
                        // and keeps the descriptor free for a real agent.
                        drop(tcp);
                        refusals.refused(peer.ip(), &refused, inner.preauth.limits());
                    }
                }
            }
            Err(err) => {
                let pause = errors.failed(&err, Instant::now());
                if wait(&*inner.deps.clock, &inner.shutdown, pause).await {
                    return Ok(());
                }
            }
        }
    }
}

/// Logs refused connections, one line per interval with the count held back.
pub(crate) struct Refusals {
    gate: RateLimitedLog,
}

impl Refusals {
    /// A fresh log: nothing refused yet.
    pub(crate) fn new() -> Self {
        Self {
            gate: RateLimitedLog::new(LINE_EVERY),
        }
    }

    /// Records one refused connection from `peer`; writes a line when it is the first of the
    /// interval.
    pub(crate) fn refused(&self, peer: std::net::IpAddr, why: &Refused, limits: ConnLimits) {
        if let Gate::Log { suppressed } = self.gate.hit_now("preauth") {
            tracing::warn!(
                %peer,
                pending_from_peer = why.from_source,
                pending_total = why.in_total,
                limit_per_peer = limits.per_source,
                limit_total = limits.total,
                suppressed,
                "refused an agent connection: too many connections have not finished the handshake{}; idle or half-open sockets are dropped after 5 seconds, so this clears by itself (it is only a problem if real agents are refused: check what is connecting to the agent port)",
                if why.source_full { " from this address" } else { "" },
            );
        }
    }
}

/// The state of a run of failed accepts.
pub(crate) struct AcceptErrors {
    gate: RateLimitedLog,
    pause: Duration,
}

impl AcceptErrors {
    /// A fresh tracker: no failures yet.
    pub(crate) fn new() -> Self {
        Self {
            gate: RateLimitedLog::new(LINE_EVERY),
            pause: FIRST_PAUSE,
        }
    }

    /// Records a failed accept at `now`, logs it when it is the first of the run or an
    /// interval has passed (with the number held back), and returns how long to wait before
    /// trying again.
    pub(crate) fn failed(&mut self, err: &std::io::Error, now: Instant) -> Duration {
        if let Gate::Log { suppressed } = self.gate.hit("accept", now) {
            tracing::warn!(
                %err,
                suppressed,
                retry_in_ms = u64::try_from(self.pause.as_millis()).unwrap_or(u64::MAX),
                "could not accept an agent connection; agents cannot connect while this lasts (too many open files is the usual cause: raise the open-file limit)"
            );
        }
        let wait = self.pause;
        self.pause = (self.pause * 2).min(LONGEST_PAUSE);
        wait
    }

    /// Records a good accept: the next failure starts a new run.
    pub(crate) fn succeeded(&mut self) {
        self.pause = FIRST_PAUSE;
    }
}

#[cfg(test)]
mod tests {
    use domovoi_testkit::log_capture::LogCapture;

    use super::*;

    fn emfile() -> std::io::Error {
        std::io::Error::from_raw_os_error(24)
    }

    #[test]
    fn a_flood_of_failed_accepts_writes_one_line_and_backs_off() {
        let (logs, _guard) = LogCapture::start();
        let mut errors = AcceptErrors::new();
        let t0 = Instant::now();
        let mut last = Duration::ZERO;
        for _ in 0..5000 {
            last = errors.failed(&emfile(), t0);
        }
        assert_eq!(
            logs.lines_with("could not accept an agent connection")
                .len(),
            1,
            "one line for 5000 failures"
        );
        assert_eq!(last, LONGEST_PAUSE, "the pause grows to its cap");
        errors.failed(&emfile(), t0 + Duration::from_secs(11));
        let lines = logs.lines_with("could not accept an agent connection");
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("suppressed=4999"), "{}", lines[1]);
        errors.succeeded();
        assert_eq!(
            errors.failed(&emfile(), t0 + Duration::from_secs(12)),
            FIRST_PAUSE
        );
    }
}
