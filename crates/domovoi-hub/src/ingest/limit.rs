//! Per-webhook token buckets driven by the clock port.

use jiff::Timestamp;

const CAPACITY: f64 = 60.0;
const REFILL_PER_MS: f64 = 0.001;

pub(super) struct TokenBucket {
    tokens: f64,
    last_ms: i64,
}

impl TokenBucket {
    pub(super) fn new(now: Timestamp) -> Self {
        Self {
            tokens: CAPACITY,
            last_ms: now.as_millisecond(),
        }
    }

    pub(super) fn take(&mut self, now: Timestamp) -> bool {
        let now_ms = now.as_millisecond();
        let elapsed = now_ms.saturating_sub(self.last_ms).max(0) as f64;
        self.tokens = (self.tokens + elapsed * REFILL_PER_MS).min(CAPACITY);
        self.last_ms = now_ms;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}
