//! The daily digest: one message per day for every host.

mod build;
mod trend;

pub use build::{DELTA_WINDOW, DigestInputs, TOLERANCE, build_digest, format_value, has_floor};
pub use trend::{
    MAX_PROJECTION, TREND_WINDOW, least_squares, projection, projection_text, rate_per_day,
};

use jiff::Timestamp;
use jiff::civil::Time;
use jiff::tz::TimeZone;

/// When the digest goes out: once a day, at the first tick after a local time.
#[derive(Debug, Clone, PartialEq)]
pub struct DigestService {
    /// Whether the digest is sent at all.
    pub enabled: bool,
    /// The local time it is due.
    pub time: Time,
    /// The time zone `time` is in.
    pub tz: TimeZone,
}

impl DigestService {
    /// Whether the digest is due at `now`, given when it was last sent: on or after the
    /// local time, and not yet today (in the configured zone).
    #[must_use]
    pub fn due(&self, now: Timestamp, last_sent: Option<Timestamp>) -> bool {
        if !self.enabled {
            return false;
        }
        let local = now.to_zoned(self.tz.clone());
        if local.time() < self.time {
            return false;
        }
        last_sent.is_none_or(|last| last.to_zoned(self.tz.clone()).date() < local.date())
    }

    /// The next moment the digest is due after `now`: today at the local time if it has
    /// not passed yet, otherwise tomorrow. `None` when the digest is disabled or the time
    /// cannot be represented.
    #[must_use]
    pub fn next_due(&self, now: Timestamp) -> Option<Timestamp> {
        if !self.enabled {
            return None;
        }
        let local = now.to_zoned(self.tz.clone());
        let mut day = local.date();
        if local.time() >= self.time {
            day = day.tomorrow().ok()?;
        }
        day.to_datetime(self.time)
            .to_zoned(self.tz.clone())
            .ok()
            .map(|at| at.timestamp())
    }
}
