//! The shapes read answers travel in. They mirror what the checks use for containers; the
//! agent maps them back.

use std::collections::BTreeMap;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Which engine answered and on which socket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerEngineInfo {
    /// `Docker` or `Podman`.
    pub engine: String,
    /// The socket path that accepted a connection.
    pub endpoint: String,
}

/// A container's run state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireRunState {
    /// Running.
    Running,
    /// Restarting.
    Restarting,
    /// Paused.
    Paused,
    /// Created, never started.
    Created,
    /// Exited.
    Exited,
    /// Dead.
    Dead,
}

/// A container's health-check result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireHealth {
    /// Still in the start period.
    Starting,
    /// Passing.
    Healthy,
    /// Failing.
    Unhealthy,
}

/// One container as the helper lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerWire {
    /// The name without Docker's leading `/`.
    pub name: String,
    /// The image reference it was created from.
    pub image: String,
    /// When that image was built.
    pub image_created: Option<Timestamp>,
    /// Its run state.
    pub state: WireRunState,
    /// Its health check result, when it has one.
    pub health: Option<WireHealth>,
    /// How often Docker has restarted it.
    pub restart_count: u64,
    /// Its labels.
    pub labels: BTreeMap<String, String>,
    /// Whether Docker removes it when it exits.
    pub auto_remove: bool,
    /// When it last started.
    pub started_at: Option<Timestamp>,
}

/// One container lifecycle event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventWire {
    /// When it happened.
    pub at: Timestamp,
    /// The container name.
    pub name: String,
    /// What happened, such as `die` or `start`.
    pub action: String,
    /// The container's image, when the engine reports it.
    pub image: Option<String>,
}

/// A transport protocol of a listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireProto {
    /// TCP.
    Tcp,
    /// UDP.
    Udp,
}

/// One listening socket and, when the helper could see it, the process that owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerWire {
    /// The protocol.
    pub proto: WireProto,
    /// The local address, without brackets.
    pub addr: String,
    /// The local port.
    pub port: u16,
    /// The owning process name, at most 64 printable ASCII bytes.
    pub process: Option<String>,
    /// The owning process id.
    pub pid: Option<u32>,
}

/// A SMART report as the helper returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartReportOutput {
    /// The raw `smartctl --json` text.
    pub json: String,
    /// The tool's exit code, a bit mask.
    pub exit_code: Option<i32>,
    /// Whether the text was cut.
    pub truncated: bool,
}

/// Kernel log lines as the helper returns them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelLogOutput {
    /// The lines, one per line.
    pub text: String,
    /// Whether the text was cut.
    pub truncated: bool,
}
