//! SMART, kernel log and listening-socket reads that need more than the agent's own rights.

use async_trait::async_trait;

use crate::helper::HelperGroup;
use crate::helper::read::{DeviceRef, KernelLogOutput, ListenerWire, ReadToken, SmartReportOutput};

/// Why a privileged read gave no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrivilegedError {
    /// The helper is not installed, so nothing can serve the read.
    #[error("the helper is not installed")]
    HelperMissing,
    /// The helper could not be used: unreachable, protocol error or a refused start.
    #[error("the helper is not usable: {0}")]
    HelperError(String),
    /// The group is switched off in `helper.toml`.
    #[error("{0} is switched off in helper.toml")]
    GroupDisabled(HelperGroup),
    /// The platform's isolation checks failed.
    #[error("this platform's isolation checks failed")]
    PlatformBlocked,
    /// The helper served the request and the read failed.
    #[error("the read failed: {0}")]
    Failed(ReadToken),
    /// The agent's own rate limit refused the request without sending it.
    #[error("too many helper reads; try again shortly")]
    RateLimited,
}

/// Reads that need the helper when the privilege boundary is in effect.
#[async_trait]
pub trait PrivilegedReads: Send + Sync {
    /// The SMART devices the helper's allowlist admits.
    async fn smart_scan(&self) -> Result<Vec<DeviceRef>, PrivilegedError>;
    /// The SMART report of one device.
    async fn smart_report(&self, device: &DeviceRef) -> Result<SmartReportOutput, PrivilegedError>;
    /// The last `lines` kernel log lines of the last `since_minutes` minutes.
    async fn kernel_log(
        &self,
        since_minutes: u32,
        lines: u16,
    ) -> Result<KernelLogOutput, PrivilegedError>;
    /// Listening sockets with their owning processes.
    async fn net_listeners(&self) -> Result<Vec<ListenerWire>, PrivilegedError>;
}
