//! The open-file limit of this process, for a start-up warning.
//!
//! Every connection, database file and log handle is one file descriptor. A hub started with
//! the usual soft limit of 1024 (or a container with `--ulimit nofile=64:64`) can run out of
//! them, and then `accept` fails with "too many open files" and the UI and the agents are both
//! unreachable. The shipped systemd unit and the compose file raise the limit; this tells the
//! operator when a hub runs with a low one anyway.

/// A soft limit below this is warned about: the web cap, the agent cap, every live agent session
/// and the database files together can need more than a few hundred descriptors.
pub const LOW_SOFT_LIMIT: u64 = 4096;

/// The soft and hard limits on open files; `None` stands for "unlimited".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdLimit {
    /// What the process may open now.
    pub soft: Option<u64>,
    /// The most it could raise the soft limit to.
    pub hard: Option<u64>,
}

impl FdLimit {
    /// Whether the soft limit is below [`LOW_SOFT_LIMIT`].
    #[must_use]
    pub fn is_low(&self) -> bool {
        self.soft.is_some_and(|soft| soft < LOW_SOFT_LIMIT)
    }

    /// The limits as `soft/hard` text for the banner.
    #[must_use]
    pub fn text(&self) -> String {
        let show =
            |value: Option<u64>| value.map_or_else(|| "unlimited".to_owned(), |v| v.to_string());
        format!("{}/{}", show(self.soft), show(self.hard))
    }
}

/// Reads the limits from the text of `/proc/self/limits`.
#[must_use]
pub fn parse(limits: &str) -> Option<FdLimit> {
    let line = limits.lines().find(|l| l.starts_with("Max open files"))?;
    let mut numbers = line
        .trim_start_matches("Max open files")
        .split_whitespace()
        .map(|word| {
            if word == "unlimited" {
                Some(None)
            } else {
                word.parse::<u64>().ok().map(Some)
            }
        });
    let soft = numbers.next()??;
    let hard = numbers.next()??;
    Some(FdLimit { soft, hard })
}

/// This process's limits, or `None` where `/proc` is not available.
#[must_use]
pub fn current() -> Option<FdLimit> {
    std::fs::read_to_string("/proc/self/limits")
        .ok()
        .and_then(|text| parse(&text))
}

/// Logs one WARN line when `limit` is low, saying what can go wrong and how to raise it.
pub fn warn_if_low(limit: &FdLimit) {
    if limit.is_low() {
        tracing::warn!(
            soft = limit.soft,
            hard = limit.hard,
            recommended = LOW_SOFT_LIMIT,
            "the open-file limit is low; a few idle connections can use every descriptor and the hub then fails with 'too many open files' (the UI and the agents both stop). Raise it and restart the hub: systemd `LimitNOFILE=65536` (the shipped unit sets it), Docker `--ulimit nofile=65536:65536` or `ulimits` in compose"
        );
    }
}

#[cfg(test)]
mod tests {
    use domovoi_testkit::log_capture::LogCapture;

    use super::*;

    const SAMPLE: &str = "Limit                     Soft Limit           Hard Limit           Units     \n\
Max cpu time              unlimited            unlimited            seconds   \n\
Max open files            1024                 524288               files     \n\
Max locked memory         8388608              8388608              bytes     \n";

    #[test]
    fn the_limits_are_read_from_the_proc_text() {
        let limit = parse(SAMPLE).unwrap();
        assert_eq!(limit.soft, Some(1024));
        assert_eq!(limit.hard, Some(524_288));
        assert!(limit.is_low());
        assert_eq!(limit.text(), "1024/524288");
        let open = parse("Max open files   unlimited   unlimited   files\n").unwrap();
        assert!(!open.is_low());
        assert_eq!(open.text(), "unlimited/unlimited");
        assert!(parse("nothing here").is_none());
    }

    #[test]
    fn a_low_limit_logs_one_warning_with_the_remedy_and_a_high_one_none() {
        let (logs, _guard) = LogCapture::start();
        warn_if_low(&FdLimit {
            soft: Some(64),
            hard: Some(64),
        });
        warn_if_low(&FdLimit {
            soft: Some(65_536),
            hard: Some(65_536),
        });
        let lines = logs.lines_with("the open-file limit is low");
        assert_eq!(lines.len(), 1, "{}", logs.text());
        for field in ["WARN", "soft=64", "LimitNOFILE=65536", "--ulimit nofile"] {
            assert!(lines[0].contains(field), "line lacks {field}: {}", lines[0]);
        }
    }
}
