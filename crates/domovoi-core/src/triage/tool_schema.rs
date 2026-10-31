//! JSON Schemas for read-tool arguments, as shown to the model.
//!
//! The limits here come from the same constants [`ReadToolCall::from_json`] enforces, so the
//! schema never promises more than parsing accepts.
//!
//! [`ReadToolCall::from_json`]: super::ReadToolCall::from_json

use serde_json::{Value, json};

use super::tool_call::{
    DEFAULT_HISTORY, DEFAULT_LINES, DEFAULT_NOTES, DEFAULT_SINCE_MINUTES, MAX_DEVICE_CHARS,
    MAX_HISTORY, MAX_LINES, MAX_NAME_CHARS, MAX_NOTES, MAX_QUERY_CHARS, MAX_SINCE_MINUTES,
    MAX_SKILL_ARGS,
};
use super::tools::ReadTool;

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn name_field(description: &str) -> Value {
    json!({ "type": "string", "minLength": 1, "maxLength": MAX_NAME_CHARS, "description": description })
}

fn lines_field() -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": MAX_LINES, "default": DEFAULT_LINES })
}

fn since_field() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": MAX_SINCE_MINUTES,
        "default": DEFAULT_SINCE_MINUTES,
        "description": "How many minutes back to look.",
    })
}

impl ReadTool {
    /// The JSON Schema of this tool's arguments.
    #[must_use]
    pub fn json_schema(&self) -> Value {
        match self {
            Self::DockerPs => object(
                json!({ "all": { "type": "boolean", "default": false, "description": "Include stopped containers." } }),
                &[],
            ),
            Self::DockerLogs => object(
                json!({ "container": name_field("Container name."), "lines": lines_field() }),
                &["container"],
            ),
            Self::DockerInspect => object(
                json!({ "container": name_field("Container name.") }),
                &["container"],
            ),
            Self::DockerEvents => object(
                json!({
                    "container": name_field("Only events for this container."),
                    "since_minutes": since_field(),
                }),
                &[],
            ),
            Self::HostOverview | Self::ListeningPorts => object(json!({}), &[]),
            Self::Journal => object(
                json!({
                    "unit": name_field("Only lines from this systemd unit."),
                    "since_minutes": since_field(),
                    "lines": lines_field(),
                    "kernel": { "type": "boolean", "default": false, "description": "Kernel messages only." },
                }),
                &[],
            ),
            Self::SmartReport => object(
                json!({
                    "device": {
                        "type": "string",
                        "pattern": "^/dev/",
                        "maxLength": MAX_DEVICE_CHARS,
                        "description": "Disk device, for example /dev/sda.",
                    }
                }),
                &["device"],
            ),
            Self::IncidentHistory => object(
                json!({ "limit": { "type": "integer", "minimum": 1, "maximum": MAX_HISTORY, "default": DEFAULT_HISTORY } }),
                &[],
            ),
            Self::NotesSearch => object(
                json!({
                    "query": { "type": "string", "minLength": 1, "maxLength": MAX_QUERY_CHARS },
                    "limit": { "type": "integer", "minimum": 1, "maximum": MAX_NOTES, "default": DEFAULT_NOTES },
                }),
                &["query"],
            ),
            Self::RunReadSkill => object(
                json!({
                    "skill": { "type": "string", "pattern": "^[a-z][a-z0-9_]{0,63}$", "description": "Name of a read-only skill." },
                    "args": {
                        "type": "object",
                        "additionalProperties": { "type": "string" },
                        "maxProperties": MAX_SKILL_ARGS,
                        "description": "Arguments as declared by the skill.",
                    },
                }),
                &["skill"],
            ),
        }
    }
}
