//! Per-operation limits of the read operations. Every one is a constant so a test can pin it.

/// Seconds a Docker probe may take: a connect only, nothing is sent.
pub const DOCKER_PROBE_TIMEOUT_SECS: u64 = 2;
/// Seconds one Docker Engine call may take.
pub const DOCKER_CALL_TIMEOUT_SECS: u64 = 10;
/// Seconds a `smartctl`, `dmesg` or `ss` run may take.
pub const TOOL_TIMEOUT_SECS: u64 = 15;
/// Bytes a tool may write to each of stdout and stderr before it is cut.
pub const TOOL_OUTPUT_BYTES: u32 = 2 * 1024 * 1024;

/// Containers one list answers with.
pub const MAX_CONTAINERS: usize = 500;
/// Events one query answers with.
pub const MAX_EVENTS: usize = 1_000;
/// The longest event window, in seconds.
pub const MAX_EVENT_WINDOW_SECS: u32 = 86_400;
/// Bytes of an inspect document.
pub const MAX_INSPECT_BYTES: usize = 1024 * 1024;
/// Bytes of container log text.
pub const MAX_LOG_TEXT_BYTES: usize = 256 * 1024;
/// Lines of container or kernel log one request may ask for.
pub const MAX_LOG_LINES: u16 = 500;
/// Minutes of kernel log one request may ask for.
pub const MAX_KERNEL_MINUTES: u32 = 1_440;
/// SMART devices one scan reports.
pub const MAX_DEVICES: usize = 64;
/// Listening sockets one query reports.
pub const MAX_LISTENERS: usize = 1_000;
