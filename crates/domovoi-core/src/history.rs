//! A bounded series of a tracked numeric value, such as reallocated sectors over time.

use std::collections::VecDeque;

use jiff::{SignedDuration, Timestamp};

/// Maximum points kept; the oldest are dropped first.
pub const HISTORY_CAPACITY: usize = 64;

const SECONDS_PER_DAY: f64 = 86_400.0;

/// The most recent [`HISTORY_CAPACITY`] `(time, value)` points.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct EntityHistory(VecDeque<(Timestamp, f64)>);

impl EntityHistory {
    /// Creates an empty history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a point, dropping the oldest when full.
    pub fn push(&mut self, at: Timestamp, value: f64) {
        if self.0.len() == HISTORY_CAPACITY {
            self.0.pop_front();
        }
        self.0.push_back((at, value));
    }

    /// The most recent point.
    #[must_use]
    pub fn latest(&self) -> Option<(Timestamp, f64)> {
        self.0.back().copied()
    }

    /// Number of points held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no points are held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates points from oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = &(Timestamp, f64)> {
        self.0.iter()
    }

    /// Change in value across the points within `window` before `now`.
    ///
    /// Returns the latest minus the earliest point in the window, or `None` with fewer than two.
    #[must_use]
    pub fn delta_over(&self, window: SignedDuration, now: Timestamp) -> Option<f64> {
        let start = now.checked_sub(window).ok()?;
        let mut in_window = self.0.iter().filter(|(t, _)| *t >= start && *t <= now);
        let first = in_window.next()?;
        let last = in_window.next_back()?;
        Some(last.1 - first.1)
    }

    /// Least-squares slope, in value per day, over points in the last 30 days.
    ///
    /// Returns `None` with fewer than two points or when all points share one timestamp.
    #[must_use]
    pub fn rate_per_day(&self, now: Timestamp) -> Option<f64> {
        let start = now.checked_sub(SignedDuration::from_hours(30 * 24)).ok()?;
        let points: Vec<(f64, f64)> = self
            .0
            .iter()
            .filter(|(t, _)| *t >= start && *t <= now)
            .map(|(t, v)| (t.as_second() as f64 / SECONDS_PER_DAY, *v))
            .collect();
        if points.len() < 2 {
            return None;
        }
        let n = points.len() as f64;
        let mean_x = points.iter().map(|p| p.0).sum::<f64>() / n;
        let mean_y = points.iter().map(|p| p.1).sum::<f64>() / n;
        let sxx: f64 = points.iter().map(|p| (p.0 - mean_x).powi(2)).sum();
        if sxx == 0.0 {
            return None;
        }
        let sxy: f64 = points.iter().map(|p| (p.0 - mean_x) * (p.1 - mean_y)).sum();
        Some(sxy / sxx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: i64) -> Timestamp {
        Timestamp::from_second(day * 86_400).unwrap()
    }

    #[test]
    fn entity_history_is_bounded_and_computes_rate() {
        let mut h = EntityHistory::new();
        for i in 0..100 {
            h.push(at(i), i as f64 * 3.0);
        }
        assert_eq!(h.len(), HISTORY_CAPACITY);
        assert_eq!(h.latest(), Some((at(99), 297.0)));
        let rate = h.rate_per_day(at(99)).unwrap();
        assert!((rate - 3.0).abs() < 1e-9, "rate was {rate}");
    }

    #[test]
    fn rate_needs_two_points() {
        let mut h = EntityHistory::new();
        assert_eq!(h.rate_per_day(at(1)), None);
        h.push(at(1), 5.0);
        assert_eq!(h.rate_per_day(at(1)), None);
    }

    #[test]
    fn delta_over_uses_points_inside_window() {
        let mut h = EntityHistory::new();
        h.push(at(0), 1.0);
        h.push(at(8), 4.0);
        h.push(at(10), 9.0);
        let d = h.delta_over(SignedDuration::from_hours(3 * 24), at(10));
        assert_eq!(d, Some(5.0));
        assert_eq!(h.delta_over(SignedDuration::from_hours(24), at(10)), None);
    }
}
