//! [`PrivilegedReads`] over the typed helper reads.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::helper::read::{DeviceRef, KernelLogOutput, ListenerWire, SmartReportOutput};
use domovoi_core::helper::{HelperGroup, UnavailableReason};
use domovoi_core::ports::{Clock, PrivilegedError, PrivilegedReads};

use super::{HelperClient, ReadClientError};

/// SMART, kernel log and listening-socket reads served by the helper.
#[derive(Clone)]
pub struct HelperPrivilegedReads {
    client: HelperClient,
    clock: Arc<dyn Clock>,
}

impl HelperPrivilegedReads {
    /// Reads through `client`, timing every exchange on `clock`.
    #[must_use]
    pub fn new(client: HelperClient, clock: Arc<dyn Clock>) -> Self {
        Self { client, clock }
    }
}

/// What a failed read of `group` means to a caller of the port.
fn privileged_error(group: HelperGroup, err: ReadClientError) -> PrivilegedError {
    match err {
        ReadClientError::Missing => PrivilegedError::HelperMissing,
        ReadClientError::Unreachable => PrivilegedError::HelperError("unreachable".to_owned()),
        ReadClientError::Protocol => PrivilegedError::HelperError("protocol".to_owned()),
        ReadClientError::Unavailable(UnavailableReason::GroupDisabled) => {
            PrivilegedError::GroupDisabled(group)
        }
        ReadClientError::Unavailable(UnavailableReason::PlatformBlocked) => {
            PrivilegedError::PlatformBlocked
        }
        ReadClientError::Unavailable(_) => PrivilegedError::HelperError("unavailable".to_owned()),
        ReadClientError::Refused(token) | ReadClientError::StartRefused(token) => {
            PrivilegedError::HelperError(token.as_str().to_owned())
        }
        ReadClientError::Failed(token, _) => PrivilegedError::Failed(token),
        ReadClientError::RateLimited => PrivilegedError::RateLimited,
    }
}

#[async_trait]
impl PrivilegedReads for HelperPrivilegedReads {
    async fn smart_scan(&self) -> Result<Vec<DeviceRef>, PrivilegedError> {
        let scan = self.client.smart_scan(&*self.clock).await;
        scan.map_err(|e| privileged_error(HelperGroup::SmartRead, e))
    }

    async fn smart_report(&self, device: &DeviceRef) -> Result<SmartReportOutput, PrivilegedError> {
        let report = self.client.smart_report(&*self.clock, device).await;
        report.map_err(|e| privileged_error(HelperGroup::SmartRead, e))
    }

    async fn kernel_log(
        &self,
        since_minutes: u32,
        lines: u16,
    ) -> Result<KernelLogOutput, PrivilegedError> {
        let log = self
            .client
            .kernel_log(&*self.clock, since_minutes, lines)
            .await;
        log.map_err(|e| privileged_error(HelperGroup::KernelRead, e))
    }

    async fn net_listeners(&self) -> Result<Vec<ListenerWire>, PrivilegedError> {
        let listeners = self.client.net_listeners(&*self.clock).await;
        listeners.map_err(|e| privileged_error(HelperGroup::NetRead, e))
    }
}
