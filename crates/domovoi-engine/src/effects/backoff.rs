//! How long a retry waits: exponential from a small base, capped so it never grows without
//! bound.

use jiff::SignedDuration;

/// The wait after the first failed attempt.
pub const BASE: SignedDuration = SignedDuration::from_secs(30);
/// The longest a retry ever waits.
pub const CAP: SignedDuration = SignedDuration::from_hours(1);

/// Delay before attempt number `attempt + 1`, after attempt `attempt` (1-based) failed:
/// `min(BASE * 2^(attempt-1), CAP)`.
#[must_use]
pub fn backoff(attempt: u32) -> SignedDuration {
    let doublings = attempt.saturating_sub(1).min(16);
    let seconds = BASE
        .as_secs()
        .saturating_mul(1i64.checked_shl(doublings).unwrap_or(i64::MAX));
    let capped = seconds.min(CAP.as_secs());
    SignedDuration::from_secs(capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_the_cap() {
        let table = [
            (1, 30),
            (2, 60),
            (3, 120),
            (4, 240),
            (5, 480),
            (12, 3600),
            (40, 3600),
        ];
        for (attempt, secs) in table {
            assert_eq!(
                backoff(attempt),
                SignedDuration::from_secs(secs),
                "attempt {attempt}"
            );
        }
    }

    #[test]
    fn backoff_does_not_overflow_for_huge_attempts() {
        assert_eq!(backoff(u32::MAX), CAP);
        assert_eq!(backoff(0), BASE);
    }
}
