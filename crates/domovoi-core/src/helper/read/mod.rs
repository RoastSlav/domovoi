//! The helper's read operations: validated slots, wire shapes, limits and failure tokens.
//!
//! Pure types shared by the helper (which answers) and the agent (which asks). A request names
//! an operation and carries only a [`ContainerRef`] or a [`DeviceRef`]; no path, program,
//! argument vector or Docker API path ever crosses the socket.

mod limits;
mod request;
mod response;
mod slots;
#[cfg(test)]
mod tests;
mod token;
mod wire;

pub use limits::{
    DOCKER_CALL_TIMEOUT_SECS, DOCKER_PROBE_TIMEOUT_SECS, MAX_CONTAINERS, MAX_DEVICES,
    MAX_EVENT_WINDOW_SECS, MAX_EVENTS, MAX_INSPECT_BYTES, MAX_KERNEL_MINUTES, MAX_LISTENERS,
    MAX_LOG_LINES, MAX_LOG_TEXT_BYTES, TOOL_OUTPUT_BYTES, TOOL_TIMEOUT_SECS,
};
pub use request::BoundsError;
pub use response::{DETAIL_MAX_BYTES, ReadFailure};
pub use slots::{ContainerRef, DeviceRef, SlotError};
pub use token::ReadToken;
pub use wire::{
    ContainerWire, DockerEngineInfo, EventWire, KernelLogOutput, ListenerWire, SmartReportOutput,
    WireHealth, WireProto, WireRunState,
};
