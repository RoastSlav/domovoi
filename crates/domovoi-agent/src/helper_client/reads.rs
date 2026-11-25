//! Typed read methods of the helper client. Each builds exactly one request and decodes
//! exactly one response; none takes a free string.

use domovoi_core::helper::read::{
    ContainerRef, ContainerWire, DeviceRef, DockerEngineInfo, EventWire, KernelLogOutput,
    ListenerWire, ReadToken, SmartReportOutput,
};
use domovoi_core::helper::{HelperRequest, HelperResponse, UnavailableReason};
use domovoi_core::identity::RefusalToken;
use domovoi_core::ports::Clock;
use jiff::Timestamp;

use super::{HelperClient, HelperClientError};

/// Why a typed helper read gave no answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadClientError {
    /// The helper socket does not exist: no helper is installed.
    #[error("the helper is not installed")]
    Missing,
    /// Connecting, writing or reading failed, or the deadline passed.
    #[error("the helper is unreachable")]
    Unreachable,
    /// The helper answered something that is not a valid answer to the request.
    #[error("the helper broke the protocol")]
    Protocol,
    /// The helper does not serve the group, or the platform does not allow it.
    #[error("the helper does not serve this read: {0:?}")]
    Unavailable(UnavailableReason),
    /// The helper refused the request.
    #[error("the helper refused the request: {0}")]
    Refused(RefusalToken),
    /// The helper's start check failed.
    #[error("the helper refused to start: {0}")]
    StartRefused(RefusalToken),
    /// The helper served the request and the read failed.
    #[error("the read failed: {0}")]
    Failed(ReadToken, Option<String>),
    /// The agent's own rate limit refused the request without sending it.
    #[error("too many helper reads; try again shortly")]
    RateLimited,
}

impl From<HelperClientError> for ReadClientError {
    fn from(err: HelperClientError) -> Self {
        match err {
            HelperClientError::Absent => Self::Missing,
            HelperClientError::Unreachable(_) => Self::Unreachable,
            HelperClientError::Refused(token) => Self::Refused(token),
            HelperClientError::StartRefused(token) => Self::StartRefused(token),
            HelperClientError::Frame(_)
            | HelperClientError::Protocol(_)
            | HelperClientError::Unexpected
            | HelperClientError::Unsigned(_) => Self::Protocol,
            HelperClientError::Unavailable => Self::Unavailable(UnavailableReason::GroupDisabled),
        }
    }
}

/// Turns the failure answers into errors and passes every other answer on.
fn classify(response: HelperResponse) -> Result<HelperResponse, ReadClientError> {
    match response {
        HelperResponse::Unavailable { reason, .. } => Err(ReadClientError::Unavailable(reason)),
        HelperResponse::Refused { token } => Err(ReadClientError::Refused(token)),
        HelperResponse::StartRefused { token, .. } => Err(ReadClientError::StartRefused(token)),
        HelperResponse::ReadFailed { token, detail, .. } => {
            Err(ReadClientError::Failed(token, detail))
        }
        other => Ok(other),
    }
}

/// Seconds from `at` to `now`, never negative and never above `u32::MAX`.
fn secs_before(now: Timestamp, at: Timestamp) -> u32 {
    let secs = now.as_second().saturating_sub(at.as_second()).max(0);
    u32::try_from(secs).unwrap_or(u32::MAX)
}

impl HelperClient {
    /// Sends one read request through the cache and rate limit and returns its answer.
    async fn read(
        &self,
        request: HelperRequest,
        clock: &dyn Clock,
    ) -> Result<HelperResponse, ReadClientError> {
        let response = self
            .cache
            .fetch(&request, clock, || async {
                self.exchange(request.clone(), clock)
                    .await
                    .map_err(ReadClientError::from)
            })
            .await?;
        classify(response)
    }

    /// Which container engine the helper can reach, if any.
    pub async fn docker_probe(
        &self,
        clock: &dyn Clock,
    ) -> Result<Option<DockerEngineInfo>, ReadClientError> {
        match self.read(HelperRequest::DockerProbe, clock).await? {
            HelperResponse::DockerEngine { engine } => Ok(engine),
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// Every container, sorted by name, and whether the list was cut.
    pub async fn docker_list(
        &self,
        clock: &dyn Clock,
    ) -> Result<(Vec<ContainerWire>, bool), ReadClientError> {
        match self.read(HelperRequest::DockerList, clock).await? {
            HelperResponse::DockerContainers {
                containers,
                truncated,
            } => Ok((containers, truncated)),
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// Container events from `since` to `until`, sent as seconds before now.
    pub async fn docker_events(
        &self,
        clock: &dyn Clock,
        since: Timestamp,
        until: Timestamp,
    ) -> Result<(Vec<EventWire>, bool), ReadClientError> {
        let now = clock.now();
        let request = HelperRequest::DockerEvents {
            since_secs: secs_before(now, since),
            until_secs: secs_before(now, until),
        };
        match self.read(request, clock).await? {
            HelperResponse::DockerEvents { events, truncated } => Ok((events, truncated)),
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// The inspect document of `container`, without environment values.
    pub async fn docker_inspect(
        &self,
        clock: &dyn Clock,
        container: &ContainerRef,
    ) -> Result<serde_json::Value, ReadClientError> {
        let request = HelperRequest::DockerInspect {
            container: container.clone(),
        };
        match self.read(request, clock).await? {
            HelperResponse::DockerInspect { inspect } => Ok(inspect),
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// The last `lines` lines of `container`'s output, and whether the text was cut.
    pub async fn docker_logs(
        &self,
        clock: &dyn Clock,
        container: &ContainerRef,
        lines: u16,
    ) -> Result<(String, bool), ReadClientError> {
        let request = HelperRequest::DockerLogs {
            container: container.clone(),
            lines,
        };
        match self.read(request, clock).await? {
            HelperResponse::DockerLogs { text, truncated } => Ok((text, truncated)),
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// The SMART devices the helper's allowlist admits.
    pub async fn smart_scan(&self, clock: &dyn Clock) -> Result<Vec<DeviceRef>, ReadClientError> {
        match self.read(HelperRequest::SmartScan, clock).await? {
            HelperResponse::SmartScan { devices } => Ok(devices),
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// The SMART report of `device`.
    pub async fn smart_report(
        &self,
        clock: &dyn Clock,
        device: &DeviceRef,
    ) -> Result<SmartReportOutput, ReadClientError> {
        let request = HelperRequest::SmartReport {
            device: device.clone(),
        };
        match self.read(request, clock).await? {
            HelperResponse::SmartReport {
                json,
                exit_code,
                truncated,
            } => Ok(SmartReportOutput {
                json,
                exit_code,
                truncated,
            }),
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// Kernel log lines from the last `since_minutes` minutes, at most `lines`.
    pub async fn kernel_log(
        &self,
        clock: &dyn Clock,
        since_minutes: u32,
        lines: u16,
    ) -> Result<KernelLogOutput, ReadClientError> {
        let request = HelperRequest::KernelLog {
            since_minutes,
            lines,
        };
        match self.read(request, clock).await? {
            HelperResponse::KernelLog { text, truncated } => {
                Ok(KernelLogOutput { text, truncated })
            }
            _ => Err(ReadClientError::Protocol),
        }
    }

    /// Listening sockets with their owning processes.
    pub async fn net_listeners(
        &self,
        clock: &dyn Clock,
    ) -> Result<Vec<ListenerWire>, ReadClientError> {
        match self.read(HelperRequest::NetListeners, clock).await? {
            HelperResponse::NetListeners { listeners, .. } => Ok(listeners),
            _ => Err(ReadClientError::Protocol),
        }
    }
}
