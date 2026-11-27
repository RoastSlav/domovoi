//! JSON Schemas for read-tool arguments, as shown to the model.
//!
//! The limits here come from the same constants [`ReadToolCall::from_json`] enforces, so the
//! schema never promises more than parsing accepts.
//!
//! [`ReadToolCall::from_json`]: super::ReadToolCall::from_json

use serde_json::{Value, json};

use super::tool_call::{
    DEFAULT_HISTORY, DEFAULT_LINES, DEFAULT_NOTES, DEFAULT_SINCE_MINUTES, MAX_BINARY_CHARS,
    MAX_DEVICE_CHARS, MAX_HISTORY, MAX_LINES, MAX_NAME_CHARS, MAX_NOTES, MAX_QUERY_CHARS,
    MAX_SINCE_MINUTES, MAX_SKILL_ARGS,
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

/// One line per read tool naming every argument its schema accepts, `*` marking the required
/// ones, with the bounds or the allowed values. Built from [`ReadTool::json_schema`], for
/// a caller that shows one schema for all of them.
#[must_use]
pub fn argument_summary() -> String {
    ReadTool::ALL
        .iter()
        .map(|tool| {
            let schema = tool.json_schema();
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            let args: Vec<String> = schema["properties"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, spec)| {
                    let star = if required.iter().any(|r| r == name.as_str()) {
                        "*"
                    } else {
                        ""
                    };
                    let bounds = match (&spec["enum"], &spec["minimum"], &spec["maximum"]) {
                        (Value::Array(values), _, _) => {
                            let words: Vec<&str> =
                                values.iter().filter_map(Value::as_str).collect();
                            format!(" ({})", words.join("|"))
                        }
                        (_, Value::Number(min), Value::Number(max)) => format!(" ({min}-{max})"),
                        _ => String::new(),
                    };
                    format!("{name}{star}{bounds}")
                })
                .collect();
            let args = if args.is_empty() {
                "no arguments".to_owned()
            } else {
                args.join(", ")
            };
            format!("{}: {args}", tool.name())
        })
        .collect::<Vec<_>>()
        .join("; ")
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
                    "priority": {
                        "type": "string",
                        "enum": super::tool_call::JOURNAL_PRIORITIES,
                        "description": "Only lines of this priority or worse (for errors use err). Leave out for every priority.",
                    },
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
            Self::HostBinary => object(
                json!({
                    "name": {
                        "type": "string",
                        "pattern": "^[a-zA-Z0-9][a-zA-Z0-9._+-]*$",
                        "maxLength": MAX_BINARY_CHARS,
                        "description": "Program file name, for example ip.",
                    },
                    "package": {
                        "type": "string",
                        "pattern": "^[a-zA-Z0-9][a-zA-Z0-9._+-]*$",
                        "maxLength": MAX_BINARY_CHARS,
                        "description": "Package name to look up, for example iproute2, when it differs from the file name.",
                    },
                }),
                &["name"],
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
