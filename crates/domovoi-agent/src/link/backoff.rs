//! Reconnect delays: exponential with jitter, reset after a stable connection.

use jiff::SignedDuration;

/// The first delay after a failure.
pub const INITIAL: SignedDuration = SignedDuration::from_secs(1);
/// The longest delay.
pub const MAX: SignedDuration = SignedDuration::from_secs(60);
/// A connection that lasted this long resets the delay to [`INITIAL`].
pub const STABLE_AFTER: SignedDuration = SignedDuration::from_secs(60);
/// The jitter factor stays within `1 ± JITTER`.
pub const JITTER: f64 = 0.2;

/// The reconnect delay sequence: 1 s doubling to 60 s, each scaled by a jitter factor.
pub struct Backoff {
    base: SignedDuration,
    jitter: Box<dyn FnMut() -> f64 + Send + Sync>,
}

/// A jitter source drawing uniformly from `[1 - JITTER, 1 + JITTER]`.
#[must_use]
pub fn random_jitter() -> Box<dyn FnMut() -> f64 + Send + Sync> {
    Box::new(|| 1.0 - JITTER + 2.0 * JITTER * rand::random::<f64>())
}

impl Backoff {
    /// A sequence whose jitter factors come from `jitter`; values outside
    /// `1 ± JITTER` are clamped.
    #[must_use]
    pub fn new(jitter: Box<dyn FnMut() -> f64 + Send + Sync>) -> Self {
        Self {
            base: INITIAL,
            jitter,
        }
    }

    /// The next delay; each call doubles the base up to [`MAX`].
    pub fn next_delay(&mut self) -> SignedDuration {
        let factor = (self.jitter)().clamp(1.0 - JITTER, 1.0 + JITTER);
        let delay = self.base.mul_f64(factor);
        self.base = self.base.saturating_mul(2).min(MAX);
        delay
    }

    /// Starts again from [`INITIAL`], after a stable connection.
    pub fn reset(&mut self) {
        self.base = INITIAL;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_double_to_the_cap_and_reset() {
        let mut backoff = Backoff::new(Box::new(|| 1.0));
        let secs: Vec<i64> = (0..8).map(|_| backoff.next_delay().as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 16, 32, 60, 60]);
        backoff.reset();
        assert_eq!(backoff.next_delay(), INITIAL);
    }

    #[test]
    fn jitter_is_clamped_to_twenty_percent() {
        let mut high = Backoff::new(Box::new(|| 5.0));
        assert_eq!(high.next_delay(), SignedDuration::from_millis(1200));
        let mut random = Backoff::new(random_jitter());
        for _ in 0..100 {
            let ms = random.next_delay().as_millis();
            assert!(ms >= 800, "{ms}");
            random.reset();
        }
    }
}
