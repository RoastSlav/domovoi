//! Recent log lines of the process that is running, for the web UI's Logs page.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// One captured log line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// When the line was logged.
    pub at: Timestamp,
    /// How severe the line is: `"error"`, `"warn"`, `"info"`, `"debug"` or `"trace"`.
    pub level: String,
    /// What logged the line, usually the module that called it.
    pub target: String,
    /// The message with its fields appended as `key=value` pairs.
    pub message: String,
}

/// Recent log lines of this process, kept in memory.
pub trait LogSource: Send + Sync {
    /// The newest `max` lines at `min_level` or more severe
    /// (`"error"`, `"warn"`, `"info"`, `"debug"`), oldest first.
    fn tail(&self, max: usize, min_level: &str) -> Vec<LogLine>;
}
