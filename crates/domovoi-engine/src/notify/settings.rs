//! Notification settings: time zone, quiet hours, reminder and folded-reply intervals.

use jiff::SignedDuration;
use jiff::civil::Time;
use jiff::tz::TimeZone;

/// Hours during which only critical alerts go out; the rest wait. May span midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietHours {
    /// Local time quiet hours start, inclusive.
    pub start: Time,
    /// Local time they end, exclusive.
    pub end: Time,
}

/// How the notification policy behaves.
#[derive(Debug, Clone, PartialEq)]
pub struct NotifySettings {
    /// The time zone quiet hours and the digest are in.
    pub tz: TimeZone,
    /// Quiet hours, if any.
    pub quiet_hours: Option<QuietHours>,
    /// How often an open warning is mentioned again.
    pub remind_warning: SignedDuration,
    /// How often an open critical incident is mentioned again.
    pub remind_critical: SignedDuration,
    /// At most one "again" reply per incident this often; repeats in between are counted.
    pub fold_every: SignedDuration,
}

impl NotifySettings {
    /// Defaults in `tz`: no quiet hours, reminders every 24 h (warning) and 4 h (critical),
    /// one folded reply per 30 minutes.
    #[must_use]
    pub fn new(tz: TimeZone) -> Self {
        Self {
            tz,
            quiet_hours: None,
            remind_warning: SignedDuration::from_hours(24),
            remind_critical: SignedDuration::from_hours(4),
            fold_every: SignedDuration::from_mins(30),
        }
    }
}
