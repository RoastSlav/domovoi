//! Trends over facts: least-squares slopes and time-to-floor projections.

use jiff::{SignedDuration, Timestamp};

/// How far back trends look.
pub const TREND_WINDOW: SignedDuration = SignedDuration::from_hours(24 * 14);
/// Projections further out than this are not shown.
pub const MAX_PROJECTION: SignedDuration = SignedDuration::from_hours(24 * 180);

const SECS_PER_DAY: f64 = 86_400.0;

/// The least-squares slope of `points` (x, y); `None` with fewer than two distinct x.
#[must_use]
pub fn least_squares(points: &[(f64, f64)]) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len() as f64;
    let mean_x = points.iter().map(|p| p.0).sum::<f64>() / n;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
    if sxx <= f64::EPSILON {
        return None;
    }
    let sxy: f64 = points.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
    Some(sxy / sxx)
}

/// The change per day of `points` over the [`TREND_WINDOW`] before `now`.
#[must_use]
pub fn rate_per_day(points: &[(Timestamp, f64)], now: Timestamp) -> Option<f64> {
    let since = now.checked_sub(TREND_WINDOW).unwrap_or(Timestamp::MIN);
    let xy: Vec<(f64, f64)> = points
        .iter()
        .filter(|(at, v)| *at >= since && *at <= now && v.is_finite())
        .map(|(at, v)| (now.duration_since(*at).as_secs_f64() / -SECS_PER_DAY, *v))
        .collect();
    least_squares(&xy)
}

/// How long until a declining value reaches `floor` at its current rate, if that is
/// within [`MAX_PROJECTION`]. `None` when it is not declining.
#[must_use]
pub fn projection(
    points: &[(Timestamp, f64)],
    floor: f64,
    now: Timestamp,
) -> Option<SignedDuration> {
    let rate = rate_per_day(points, now)?;
    if rate >= 0.0 {
        return None;
    }
    let (_, latest) = points
        .iter()
        .filter(|(at, _)| *at <= now)
        .max_by_key(|(at, _)| *at)?;
    let days = (latest - floor) / -rate;
    if !days.is_finite() || days < 0.0 {
        return None;
    }
    let left = SignedDuration::try_from_secs_f64(days * SECS_PER_DAY).ok()?;
    (left <= MAX_PROJECTION).then_some(left)
}

/// A projection as a phrase: "full in ~12 weeks at this rate".
#[must_use]
pub fn projection_text(left: SignedDuration) -> String {
    let days = (left.as_secs_f64() / SECS_PER_DAY).round().max(0.0) as i64;
    let span = match days {
        0 => "less than a day".to_owned(),
        1 => "~1 day".to_owned(),
        2..14 => format!("~{days} days"),
        _ => format!("~{} weeks", (days as f64 / 7.0).round() as i64),
    };
    format!("full in {span} at this rate")
}
