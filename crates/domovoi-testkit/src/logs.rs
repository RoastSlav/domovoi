//! A log source that answers with fixed lines, for tests of the logs endpoint.

use domovoi_core::ports::{LogLine, LogSource};

/// A [`LogSource`] that hands back the lines it was given, oldest first. It keeps the
/// newest `max` of them but does not filter by level: that filtering is the ring's own
/// behaviour, tested with `LogRing`.
#[derive(Debug, Clone, Default)]
pub struct CannedLogSource {
    lines: Vec<LogLine>,
}

impl CannedLogSource {
    /// A source that answers with `lines`.
    #[must_use]
    pub fn new(lines: Vec<LogLine>) -> Self {
        Self { lines }
    }
}

impl LogSource for CannedLogSource {
    fn tail(&self, max: usize, _min_level: &str) -> Vec<LogLine> {
        let skip = if self.lines.len() > max {
            self.lines.len() - max
        } else {
            0
        };
        self.lines.iter().skip(skip).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::CannedLogSource;
    use domovoi_core::ports::{LogLine, LogSource};
    use jiff::Timestamp;

    fn line(target: &str, message: &str) -> LogLine {
        LogLine {
            at: Timestamp::from_second(1_700_000_000).unwrap(),
            level: "info".to_owned(),
            target: target.to_owned(),
            message: message.to_owned(),
        }
    }

    #[test]
    fn canned_source_answers_with_its_newest_lines_oldest_first() {
        let lines: Vec<LogLine> = [line("a", "first"), line("b", "second")].to_vec();
        let source = CannedLogSource::new(lines);
        let all = LogSource::tail(&source, 10, "info");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].target, "a");
        let only_new = LogSource::tail(&source, 1, "info");
        assert_eq!(only_new.len(), 1);
        assert_eq!(only_new[0].target, "b");
    }
}
