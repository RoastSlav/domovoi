//! Recent log lines of this process, kept in memory so the web UI can show them.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use domovoi_core::ports::{LogLine, LogSource};
use jiff::Timestamp;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// How many lines the ring keeps.
pub const CAPACITY: usize = 2000;

/// How long one captured message may be, in bytes.
pub const MESSAGE_LIMIT: usize = 2048;

/// Severity of a level name: more severe means a larger number. Unknown names
/// count as informational, so asking for `info` still shows them.
fn severity(name: &str) -> u8 {
    match name {
        "error" => 4,
        "warn" => 3,
        "debug" => 1,
        "trace" => 0,
        _ => 2,
    }
}

/// The log lines this process has logged recently, newest at the back. The
/// tracing layer and the API share the same buffer through `Arc`.
pub struct LogRing {
    buffer: Mutex<VecDeque<LogLine>>,
}

impl LogRing {
    /// A ring with no lines.
    pub fn new() -> LogRing {
        LogRing {
            buffer: Mutex::new(VecDeque::new()),
        }
    }

    /// Adds one line, cutting an over-long message and dropping the oldest
    /// line once the ring is full.
    pub fn push(&self, mut line: LogLine) {
        if line.message.len() > MESSAGE_LIMIT {
            let mut cut: usize = MESSAGE_LIMIT;
            while !line.message.is_char_boundary(cut) {
                cut -= 1;
            }
            line.message.truncate(cut);
        }
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        buffer.push_back(line);
        if buffer.len() > CAPACITY {
            buffer.pop_front();
        }
    }
}

/// The newest lines at `min_level` or more severe, oldest first.
impl LogSource for LogRing {
    fn tail(&self, max: usize, min_level: &str) -> Vec<LogLine> {
        let wanted = severity(min_level);
        let mut picked: Vec<LogLine> = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .rev()
            .filter(|line| severity(&line.level) >= wanted)
            .take(max)
            .map(Clone::clone)
            .collect();
        picked.reverse();
        picked
    }
}

/// Collects an event's message and its other fields as `key=value` pairs.
struct EntryVisitor {
    parts: Vec<String>,
}

impl EntryVisitor {
    /// A visitor with nothing collected yet.
    pub fn new() -> EntryVisitor {
        EntryVisitor { parts: Vec::new() }
    }

    /// Stores the message field first and every other field as a pair.
    pub fn add(&mut self, field: &Field, value: String) {
        if field.name().eq("message") {
            self.parts.insert(0, value);
        } else {
            self.parts.push(format!("{}={}", field.name(), value));
        }
    }

    /// The message with its other fields appended as `key=value` pairs.
    pub fn text(self: EntryVisitor) -> String {
        self.parts.join(" ")
    }
}

impl Visit for EntryVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.add(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.add(field, format!("{:?}", value));
    }
}

/// The tracing layer that feeds the ring. It holds the ring as `Arc` so the
/// API can read the same buffer.
pub struct RingLayer {
    pub ring: Arc<LogRing>,
}

/// Captures every logged event into the ring while the process runs.
impl<S> Layer<S> for RingLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let at = Timestamp::now();
        let meta = event.metadata();
        let level = meta.level().as_str().to_ascii_lowercase();
        let target = meta.target().to_string();
        let mut visitor = EntryVisitor::new();
        event.record(&mut visitor);
        self.ring.push(LogLine {
            at,
            level,
            target,
            message: visitor.text(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{CAPACITY, LogRing, MESSAGE_LIMIT};
    use domovoi_core::ports::{LogLine, LogSource};
    use jiff::Timestamp;

    fn line(level: &str, target: &str, message: String) -> LogLine {
        LogLine {
            at: Timestamp::from_second(1_700_000_000).unwrap(),
            level: level.to_owned(),
            target: target.to_owned(),
            message,
        }
    }

    #[test]
    fn ring_keeps_only_the_newest_two_thousand_lines() {
        let ring = LogRing::new();
        for n in 0..2_500 {
            ring.push(line("info", "counting", format!("line {n}")));
        }
        let kept = LogSource::tail(&ring, CAPACITY + 10, "trace");
        assert_eq!(kept.len(), CAPACITY);
        assert_eq!(kept[0].message, "line 500");
        assert_eq!(kept[CAPACITY - 1].message, "line 2499");
    }

    #[test]
    fn ring_tail_returns_only_lines_at_or_above_the_level() {
        let ring = LogRing::new();
        for level in ["trace", "debug", "info", "warn", "error"] {
            ring.push(line(level, "ladder", level.to_owned()));
        }
        let from_warn: Vec<String> = LogSource::tail(&ring, 10, "warn")
            .iter()
            .map(|l| l.message.clone())
            .collect();
        assert_eq!(from_warn, ["warn", "error"]);
        let from_trace: Vec<String> = LogSource::tail(&ring, 10, "trace")
            .iter()
            .map(|l| l.message.clone())
            .collect();
        assert_eq!(from_trace, ["trace", "debug", "info", "warn", "error"]);
    }

    #[test]
    fn ring_cuts_an_over_long_message_to_two_kibibytes() {
        let ring = LogRing::new();
        ring.push(line("info", "long", "x".repeat(MESSAGE_LIMIT + 500)));
        let kept: Vec<LogLine> = LogSource::tail(&ring, 1, "info");
        assert_eq!(kept[0].message.len(), MESSAGE_LIMIT);
    }

    #[test]
    fn ring_cuts_a_long_message_at_a_character_boundary() {
        let ring = LogRing::new();
        ring.push(line("info", "long", format!("x{}", "あ".repeat(1_500))));
        let kept: Vec<LogLine> = LogSource::tail(&ring, 1, "info");
        assert_eq!(
            kept[0].message.len(),
            MESSAGE_LIMIT - 1,
            "the cut backed off to a whole character",
        );
    }
}
