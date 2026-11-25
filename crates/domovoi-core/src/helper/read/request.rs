//! Bounds of the read requests, checked when a request is decoded.

use super::limits::{MAX_EVENT_WINDOW_SECS, MAX_KERNEL_MINUTES, MAX_LOG_LINES};
use crate::helper::HelperRequest;

/// Which bound a request broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a read request field is outside its bounds")]
pub struct BoundsError;

impl HelperRequest {
    /// Checks the numeric bounds of a read request: log lines 1 to 500, kernel minutes 1 to
    /// 1 440, and an event window of at most 86 400 seconds whose start is before its end.
    pub fn check_bounds(&self) -> Result<(), BoundsError> {
        let ok = match self {
            Self::DockerEvents {
                since_secs,
                until_secs,
            } => since_secs > until_secs && *since_secs <= MAX_EVENT_WINDOW_SECS,
            Self::DockerLogs { lines, .. } => (1..=MAX_LOG_LINES).contains(lines),
            Self::KernelLog {
                since_minutes,
                lines,
            } => {
                (1..=MAX_KERNEL_MINUTES).contains(since_minutes)
                    && (1..=MAX_LOG_LINES).contains(lines)
            }
            _ => true,
        };
        if ok { Ok(()) } else { Err(BoundsError) }
    }
}
