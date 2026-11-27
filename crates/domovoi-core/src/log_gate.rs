//! A gate that keeps a repeating error from flooding the log.
//!
//! The first time a key is seen it passes at once. Further hits on the same key are
//! counted and held back until the window has passed; the next hit after that passes and
//! reports how many were held back, so the line can say `suppressed=N`. The gate does no
//! I/O and reads no clock: the caller passes the time, which keeps tests deterministic.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// What the caller should do with one occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Log this occurrence. `suppressed` is how many identical ones were held back since the
    /// last logged line (zero for the first).
    Log {
        /// How many identical occurrences were not logged since the previous line.
        suppressed: u64,
    },
    /// Stay silent; the occurrence was counted.
    Hold,
}

/// Per-key suppression state: the time of the last logged line and the held-back count.
#[derive(Debug)]
struct Slot {
    last: Instant,
    held: u64,
}

/// Lets one line per key through per window, counting the rest. Cheap to share: it keeps a
/// small map behind a mutex and drops keys that have been quiet for a window.
#[derive(Debug)]
pub struct RateLimitedLog {
    window: Duration,
    slots: Mutex<HashMap<String, Slot>>,
}

impl RateLimitedLog {
    /// A gate that lets one line per key through every `window`.
    #[must_use]
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            slots: Mutex::new(HashMap::new()),
        }
    }

    /// Records one occurrence of `key` at `now` and says whether to log it.
    pub fn hit(&self, key: &str, now: Instant) -> Gate {
        let Ok(mut slots) = self.slots.lock() else {
            // A poisoned lock must never silence a log line.
            return Gate::Log { suppressed: 0 };
        };
        if slots.len() > 1024 {
            let window = self.window;
            slots.retain(|_, slot| now.saturating_duration_since(slot.last) < window * 4);
        }
        match slots.get_mut(key) {
            None => {
                slots.insert(key.to_owned(), Slot { last: now, held: 0 });
                Gate::Log { suppressed: 0 }
            }
            Some(slot) if now.saturating_duration_since(slot.last) >= self.window => {
                let suppressed = slot.held;
                slot.held = 0;
                slot.last = now;
                Gate::Log { suppressed }
            }
            Some(slot) => {
                slot.held += 1;
                Gate::Hold
            }
        }
    }

    /// Like [`Self::hit`] with the real monotonic clock, for call sites outside tests.
    pub fn hit_now(&self, key: &str) -> Gate {
        self.hit(key, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_passes_then_holds_then_reports_the_count() {
        let gate = RateLimitedLog::new(Duration::from_secs(10));
        let t0 = Instant::now();
        assert_eq!(gate.hit("a", t0), Gate::Log { suppressed: 0 });
        for i in 1..=5 {
            assert_eq!(gate.hit("a", t0 + Duration::from_secs(i)), Gate::Hold);
        }
        assert_eq!(
            gate.hit("a", t0 + Duration::from_secs(11)),
            Gate::Log { suppressed: 5 }
        );
        assert_eq!(
            gate.hit("a", t0 + Duration::from_secs(12)),
            Gate::Hold,
            "the window restarts at the last logged line"
        );
    }

    #[test]
    fn keys_are_independent() {
        let gate = RateLimitedLog::new(Duration::from_secs(10));
        let t0 = Instant::now();
        assert_eq!(gate.hit("a", t0), Gate::Log { suppressed: 0 });
        assert_eq!(gate.hit("b", t0), Gate::Log { suppressed: 0 });
        assert_eq!(gate.hit("a", t0), Gate::Hold);
    }
}
