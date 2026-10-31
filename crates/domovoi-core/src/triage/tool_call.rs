//! Typed, bounded arguments for read-tool calls coming from the model.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;

use super::tools::ReadTool;
use crate::skill::SkillName;

/// Most log or journal lines one call may return.
pub const MAX_LINES: u16 = 500;
/// Furthest back, in minutes, a time-windowed call may look.
pub const MAX_SINCE_MINUTES: u32 = 1440;
/// Longest container or unit name.
pub const MAX_NAME_CHARS: usize = 128;
/// Longest disk device path.
pub const MAX_DEVICE_CHARS: usize = 64;
/// Longest notes search query.
pub const MAX_QUERY_CHARS: usize = 200;
/// Most notes one search may return.
pub const MAX_NOTES: u16 = 20;
/// Most past incidents one call may return.
pub const MAX_HISTORY: u16 = 50;
/// Most arguments a read skill call may carry.
pub const MAX_SKILL_ARGS: usize = 32;

pub(super) const DEFAULT_LINES: u16 = 100;
pub(super) const DEFAULT_SINCE_MINUTES: u32 = 60;
pub(super) const DEFAULT_HISTORY: u16 = 10;
pub(super) const DEFAULT_NOTES: u16 = 5;

/// Why a tool call from the model was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolArgError {
    /// The name is not in the [`ReadTool`] catalogue.
    #[error("unknown tool `{0}`")]
    UnknownTool(String),
    /// The arguments are malformed or out of range.
    #[error("{tool}: {reason}")]
    InvalidArgs {
        /// The tool's name.
        tool: &'static str,
        /// What is wrong.
        reason: String,
    },
}

/// One validated call to a [`ReadTool`].
///
/// `RunReadSkill` names a skill, but this type cannot know its risk: the engine must still
/// check the manifest says `risk = "read"` and validate `args` against it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ReadToolCall {
    /// List containers.
    DockerPs {
        /// Include stopped containers.
        all: bool,
    },
    /// Tail one container's logs.
    DockerLogs {
        /// Container name.
        container: String,
        /// Lines to return, at most [`MAX_LINES`].
        lines: u16,
    },
    /// Inspect one container.
    DockerInspect {
        /// Container name.
        container: String,
    },
    /// Recent Docker events.
    DockerEvents {
        /// Restrict to one container.
        container: Option<String>,
        /// How far back to look, at most [`MAX_SINCE_MINUTES`].
        since_minutes: u32,
    },
    /// Host summary.
    HostOverview,
    /// Read the systemd journal.
    Journal {
        /// Restrict to one unit.
        unit: Option<String>,
        /// How far back to look, at most [`MAX_SINCE_MINUTES`].
        since_minutes: u32,
        /// Lines to return, at most [`MAX_LINES`].
        lines: u16,
        /// Kernel messages only.
        kernel: bool,
    },
    /// Listening sockets.
    ListeningPorts,
    /// SMART health of one disk.
    SmartReport {
        /// Device path under `/dev/`.
        device: String,
    },
    /// Past incidents with the same fingerprint.
    IncidentHistory {
        /// Incidents to return, at most [`MAX_HISTORY`].
        limit: u16,
    },
    /// Search runbook notes.
    NotesSearch {
        /// Search text.
        query: String,
        /// Notes to return, at most [`MAX_NOTES`].
        limit: u16,
    },
    /// Run a read-only skill.
    RunReadSkill {
        /// The skill to run.
        skill: SkillName,
        /// Arguments, validated against the manifest by the engine and the agent.
        args: BTreeMap<String, String>,
    },
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PsArgs {
    #[serde(default)]
    all: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LogsArgs {
    container: String,
    #[serde(default = "default_lines")]
    lines: u16,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectArgs {
    container: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EventsArgs {
    #[serde(default)]
    container: Option<String>,
    #[serde(default = "default_since")]
    since_minutes: u32,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalArgs {
    #[serde(default)]
    unit: Option<String>,
    #[serde(default = "default_since")]
    since_minutes: u32,
    #[serde(default = "default_lines")]
    lines: u16,
    #[serde(default)]
    kernel: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SmartArgs {
    device: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryArgs {
    #[serde(default = "default_history")]
    limit: u16,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NotesArgs {
    query: String,
    #[serde(default = "default_notes")]
    limit: u16,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillArgs {
    skill: SkillName,
    #[serde(default)]
    args: BTreeMap<String, String>,
}

fn default_lines() -> u16 {
    DEFAULT_LINES
}

fn default_since() -> u32 {
    DEFAULT_SINCE_MINUTES
}

fn default_history() -> u16 {
    DEFAULT_HISTORY
}

fn default_notes() -> u16 {
    DEFAULT_NOTES
}

struct Checker {
    tool: ReadTool,
}

impl Checker {
    fn fail(&self, reason: impl Into<String>) -> ToolArgError {
        ToolArgError::InvalidArgs {
            tool: self.tool.name(),
            reason: reason.into(),
        }
    }

    fn parse<T: DeserializeOwned>(&self, args: serde_json::Value) -> Result<T, ToolArgError> {
        serde_json::from_value(args).map_err(|e| self.fail(e.to_string()))
    }

    fn in_range<N: PartialOrd + std::fmt::Display>(
        &self,
        field: &str,
        value: N,
        min: N,
        max: N,
    ) -> Result<N, ToolArgError> {
        if value < min || value > max {
            return Err(self.fail(format!("{field} must be between {min} and {max}")));
        }
        Ok(value)
    }

    fn text(&self, field: &str, value: String, max: usize) -> Result<String, ToolArgError> {
        let len = value.chars().count();
        if len == 0 || len > max {
            return Err(self.fail(format!("{field} must be 1 to {max} characters")));
        }
        if value.chars().any(char::is_control) {
            return Err(self.fail(format!("{field} must not contain control characters")));
        }
        Ok(value)
    }

    fn optional_text(
        &self,
        field: &str,
        value: Option<String>,
        max: usize,
    ) -> Result<Option<String>, ToolArgError> {
        value.map(|v| self.text(field, v, max)).transpose()
    }
}

impl ReadToolCall {
    /// Parses a tool call from the model. Unknown tools, unknown fields and out-of-range
    /// values are rejected. `null` arguments are treated as `{}`.
    pub fn from_json(tool: &str, args: serde_json::Value) -> Result<Self, ToolArgError> {
        let tool =
            ReadTool::from_name(tool).ok_or_else(|| ToolArgError::UnknownTool(tool.to_owned()))?;
        let args = if args.is_null() {
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            args
        };
        let c = Checker { tool };
        Ok(match tool {
            ReadTool::DockerPs => {
                let a: PsArgs = c.parse(args)?;
                Self::DockerPs { all: a.all }
            }
            ReadTool::DockerLogs => {
                let a: LogsArgs = c.parse(args)?;
                Self::DockerLogs {
                    container: c.text("container", a.container, MAX_NAME_CHARS)?,
                    lines: c.in_range("lines", a.lines, 1, MAX_LINES)?,
                }
            }
            ReadTool::DockerInspect => {
                let a: InspectArgs = c.parse(args)?;
                Self::DockerInspect {
                    container: c.text("container", a.container, MAX_NAME_CHARS)?,
                }
            }
            ReadTool::DockerEvents => {
                let a: EventsArgs = c.parse(args)?;
                Self::DockerEvents {
                    container: c.optional_text("container", a.container, MAX_NAME_CHARS)?,
                    since_minutes: c.in_range(
                        "since_minutes",
                        a.since_minutes,
                        1,
                        MAX_SINCE_MINUTES,
                    )?,
                }
            }
            ReadTool::HostOverview => {
                let _: NoArgs = c.parse(args)?;
                Self::HostOverview
            }
            ReadTool::Journal => {
                let a: JournalArgs = c.parse(args)?;
                Self::Journal {
                    unit: c.optional_text("unit", a.unit, MAX_NAME_CHARS)?,
                    since_minutes: c.in_range(
                        "since_minutes",
                        a.since_minutes,
                        1,
                        MAX_SINCE_MINUTES,
                    )?,
                    lines: c.in_range("lines", a.lines, 1, MAX_LINES)?,
                    kernel: a.kernel,
                }
            }
            ReadTool::ListeningPorts => {
                let _: NoArgs = c.parse(args)?;
                Self::ListeningPorts
            }
            ReadTool::SmartReport => {
                let a: SmartArgs = c.parse(args)?;
                let device = c.text("device", a.device, MAX_DEVICE_CHARS)?;
                if !device.starts_with("/dev/") || device.split('/').any(|part| part == "..") {
                    return Err(c.fail("device must be a path under /dev/"));
                }
                Self::SmartReport { device }
            }
            ReadTool::IncidentHistory => {
                let a: HistoryArgs = c.parse(args)?;
                Self::IncidentHistory {
                    limit: c.in_range("limit", a.limit, 1, MAX_HISTORY)?,
                }
            }
            ReadTool::NotesSearch => {
                let a: NotesArgs = c.parse(args)?;
                Self::NotesSearch {
                    query: c.text("query", a.query, MAX_QUERY_CHARS)?,
                    limit: c.in_range("limit", a.limit, 1, MAX_NOTES)?,
                }
            }
            ReadTool::RunReadSkill => {
                let a: SkillArgs = c.parse(args)?;
                c.in_range("args", a.args.len(), 0, MAX_SKILL_ARGS)?;
                Self::RunReadSkill {
                    skill: a.skill,
                    args: a.args,
                }
            }
        })
    }

    /// The tool this call is for.
    #[must_use]
    pub fn tool(&self) -> ReadTool {
        match self {
            Self::DockerPs { .. } => ReadTool::DockerPs,
            Self::DockerLogs { .. } => ReadTool::DockerLogs,
            Self::DockerInspect { .. } => ReadTool::DockerInspect,
            Self::DockerEvents { .. } => ReadTool::DockerEvents,
            Self::HostOverview => ReadTool::HostOverview,
            Self::Journal { .. } => ReadTool::Journal,
            Self::ListeningPorts => ReadTool::ListeningPorts,
            Self::SmartReport { .. } => ReadTool::SmartReport,
            Self::IncidentHistory { .. } => ReadTool::IncidentHistory,
            Self::NotesSearch { .. } => ReadTool::NotesSearch,
            Self::RunReadSkill { .. } => ReadTool::RunReadSkill,
        }
    }
}
