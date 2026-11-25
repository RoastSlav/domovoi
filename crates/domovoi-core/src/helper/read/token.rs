//! Why a read operation failed after it was understood.

use serde::{Deserialize, Serialize};

/// The closed set of reasons a read operation fails. The helper reports one of these, never an
/// error string from a tool or the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadToken {
    /// `smartctl`, `dmesg` or `ss` is not installed in any fixed candidate path.
    ToolMissing,
    /// No Docker or Podman socket exists.
    EngineAbsent,
    /// The socket exists and the helper may not open it.
    EngineDenied,
    /// The engine answered a transport error or an unexpected status.
    EngineError,
    /// The engine, or a proxy in front of it, answered 403 or 405 to one of the five reads.
    EngineApiRefused,
    /// The engine answered 404 for the container.
    TargetNotFound,
    /// The device does not match `[smart] devices`.
    DeviceNotAllowed,
    /// The tool exited non-zero without usable output.
    CommandFailed,
    /// The tool output did not parse in the expected shape.
    OutputMalformed,
    /// The per-call timeout or the request deadline elapsed.
    TimedOut,
}

impl ReadToken {
    /// Every token.
    pub const ALL: &'static [Self] = &[
        Self::ToolMissing,
        Self::EngineAbsent,
        Self::EngineDenied,
        Self::EngineError,
        Self::EngineApiRefused,
        Self::TargetNotFound,
        Self::DeviceNotAllowed,
        Self::CommandFailed,
        Self::OutputMalformed,
        Self::TimedOut,
    ];

    /// The wire spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ToolMissing => "tool_missing",
            Self::EngineAbsent => "engine_absent",
            Self::EngineDenied => "engine_denied",
            Self::EngineError => "engine_error",
            Self::EngineApiRefused => "engine_api_refused",
            Self::TargetNotFound => "target_not_found",
            Self::DeviceNotAllowed => "device_not_allowed",
            Self::CommandFailed => "command_failed",
            Self::OutputMalformed => "output_malformed",
            Self::TimedOut => "timed_out",
        }
    }
}

impl std::fmt::Display for ReadToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
