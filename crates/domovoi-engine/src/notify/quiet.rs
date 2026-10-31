//! Quiet-hours arithmetic in the configured time zone, across midnight and DST changes.

use jiff::Timestamp;
use jiff::civil::Time;

use super::settings::{NotifySettings, QuietHours};

fn inside(time: Time, quiet: &QuietHours) -> bool {
    if quiet.start <= quiet.end {
        time >= quiet.start && time < quiet.end
    } else {
        // Spans midnight, such as 22:00 to 07:00.
        time >= quiet.start || time < quiet.end
    }
}

/// Whether `now` falls in quiet hours.
#[must_use]
pub fn in_quiet_hours(now: Timestamp, settings: &NotifySettings) -> bool {
    let Some(quiet) = &settings.quiet_hours else {
        return false;
    };
    inside(now.to_zoned(settings.tz.clone()).time(), quiet)
}

/// When the quiet hours `now` falls in end, or `None` outside quiet hours.
///
/// The end is a local time on the right day. When a DST change makes that local time
/// ambiguous the earlier instant is used; when it does not exist, the instant after the
/// gap.
#[must_use]
pub fn quiet_ends_at(now: Timestamp, settings: &NotifySettings) -> Option<Timestamp> {
    let quiet = settings.quiet_hours.as_ref()?;
    let local = now.to_zoned(settings.tz.clone());
    if !inside(local.time(), quiet) {
        return None;
    }
    let mut day = local.date();
    if quiet.start > quiet.end && local.time() >= quiet.start {
        day = day.tomorrow().ok()?;
    }
    let end = day
        .to_datetime(quiet.end)
        .to_zoned(settings.tz.clone())
        .ok()?;
    Some(end.timestamp())
}
