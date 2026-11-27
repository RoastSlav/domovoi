//! What the agent logs about its link to the hub: unreachable hub, connected, dropped.
//!
//! One outage is one story: the first failed attempt is logged at once with the hub address,
//! the attempt number and when the next try comes; further failed attempts are counted and
//! summarised every [`REPEAT_EVERY`]; the line that says the link is back says how long it
//! was gone.

use domovoi_core::log_text::{human_duration, without_urls};
use domovoi_proto::messages::{SessionId, Welcome};
use jiff::{SignedDuration, Timestamp};

use crate::error::LinkError;

/// How often a hub that stays unreachable is reported again.
pub const REPEAT_EVERY: SignedDuration = SignedDuration::from_mins(5);

/// The state of the current outage and of the open session.
#[derive(Debug, Default)]
pub struct Outage {
    /// When the link was last lost or first failed; `None` while connected or before the
    /// first connection attempt.
    since: Option<Timestamp>,
    /// Failed attempts since the last connection.
    attempts: u32,
    /// Failed attempts not yet reported in a line.
    held: u32,
    /// When a failure was last reported.
    last_logged: Option<Timestamp>,
    /// The open session, with when it started.
    session: Option<(SessionId, Timestamp)>,
}

/// An error as one line of plain text: no links, no trailing punctuation.
fn plain(err: &LinkError) -> String {
    without_urls(&err.to_string())
}

impl Outage {
    /// Logs a failed attempt to reach the hub at `hub`; `delay` is the wait before the next
    /// one. Repeats inside [`REPEAT_EVERY`] are counted, not logged.
    pub fn log_unreachable(
        &mut self,
        hub: &str,
        err: &LinkError,
        now: Timestamp,
        delay: SignedDuration,
    ) {
        self.attempts += 1;
        self.since.get_or_insert(now);
        let due = self
            .last_logged
            .is_none_or(|at| now.duration_since(at) >= REPEAT_EVERY);
        if !due {
            self.held += 1;
            return;
        }
        let next = now.checked_add(delay).unwrap_or(now);
        tracing::warn!(
            hub = %hub,
            attempt = self.attempts,
            next_retry_in_secs = delay.as_secs(),
            next_retry_at = %next,
            suppressed = self.held,
            reason = %plain(err),
            "could not reach the hub"
        );
        self.held = 0;
        self.last_logged = Some(now);
    }

    /// Logs that the link is up and resets the outage.
    pub fn log_connected(&mut self, hub: &str, welcome: &Welcome, now: Timestamp) {
        let offline = self.since.map(|since| {
            let secs = now.duration_since(since).as_secs();
            human_duration(u64::try_from(secs).unwrap_or(0))
        });
        tracing::info!(
            session = %welcome.session_id,
            hub = %hub,
            hub_version = %welcome.hub_version,
            agent_version = env!("CARGO_PKG_VERSION"),
            failed_attempts = self.attempts,
            offline_for = %offline.as_deref().unwrap_or("-"),
            "connected to the hub"
        );
        self.session = Some((welcome.session_id, now));
        self.since = None;
        self.attempts = 0;
        self.held = 0;
        self.last_logged = None;
    }

    /// Logs that the open session ended with `err`: which session, how long it lasted, and
    /// why in plain words.
    pub fn log_dropped(&mut self, err: &LinkError, now: Timestamp) {
        let (session, uptime) = match self.session.take() {
            Some((id, started)) => {
                let secs = now.duration_since(started).as_secs();
                (
                    id.to_string(),
                    human_duration(u64::try_from(secs).unwrap_or(0)),
                )
            }
            None => ("-".to_owned(), "-".to_owned()),
        };
        self.since = Some(now);
        let reason = plain(err);
        // A hub that closed the connection (a restart, an upgrade) is not a fault.
        if matches!(err, LinkError::Lost(text) if text.contains("closed") || text.contains("close_notify"))
        {
            tracing::info!(%session, %uptime, %reason, "the hub link closed; reconnecting");
        } else {
            tracing::warn!(%session, %uptime, %reason, "the hub link dropped; reconnecting");
        }
    }
}

#[cfg(test)]
mod tests {
    use domovoi_testkit::log_capture::LogCapture;

    use super::*;

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(1_800_000_000 + secs).unwrap()
    }

    fn refused() -> LinkError {
        LinkError::Connect("Connection refused (os error 111)".to_owned())
    }

    #[test]
    fn an_unreachable_hub_names_the_address_the_attempt_and_the_next_try() {
        let (logs, _guard) = LogCapture::start();
        let mut outage = Outage::default();
        outage.log_unreachable(
            "wss://hub.example:7421/",
            &refused(),
            at(0),
            SignedDuration::from_secs(4),
        );
        let line = logs.lines_with("could not reach the hub").remove(0);
        for field in [
            "WARN",
            "hub=wss://hub.example:7421/",
            "attempt=1",
            "next_retry_in_secs=4",
            "next_retry_at=",
            "suppressed=0",
            "Connection refused",
        ] {
            assert!(line.contains(field), "lacks {field}: {line}");
        }
    }

    #[test]
    fn repeated_failures_are_summarised_not_repeated() {
        let (logs, _guard) = LogCapture::start();
        let mut outage = Outage::default();
        let delay = SignedDuration::from_secs(60);
        for n in 0..13 {
            outage.log_unreachable("wss://h/", &refused(), at(n * 60), delay);
        }
        let lines = logs.lines_with("could not reach the hub");
        assert_eq!(
            lines.len(),
            3,
            "first, then one per five minutes: {lines:?}"
        );
        assert!(
            lines[1].contains("attempt=6") && lines[1].contains("suppressed=4"),
            "{}",
            lines[1]
        );
    }

    #[test]
    fn connecting_says_how_long_the_hub_was_unreachable_and_prints_no_debug_wrapper() {
        let (logs, _guard) = LogCapture::start();
        let mut outage = Outage::default();
        outage.log_unreachable("wss://h/", &refused(), at(0), SignedDuration::from_secs(1));
        let welcome = Welcome {
            session_id: SessionId::new(),
            hub_version: "0.0.10".to_owned(),
            config: Default::default(),
        };
        outage.log_connected("wss://h/", &welcome, at(125));
        let line = logs.lines_with("connected to the hub").remove(0);
        assert!(
            line.contains(&format!("session={}", welcome.session_id)),
            "{line}"
        );
        assert!(!line.contains("SessionId("), "{line}");
        for field in [
            "hub=wss://h/",
            "hub_version=0.0.10",
            "agent_version=",
            "offline_for=2m05s",
            "failed_attempts=1",
        ] {
            assert!(line.contains(field), "lacks {field}: {line}");
        }
        outage.log_dropped(
            &LinkError::Lost("the hub went silent".to_owned()),
            at(125 + 3 * 3600 + 12 * 60),
        );
        let line = logs.lines_with("the hub link dropped").remove(0);
        assert!(
            line.contains(&format!("session={}", welcome.session_id)),
            "{line}"
        );
        assert!(line.contains("uptime=3h12m"), "{line}");
    }
}
