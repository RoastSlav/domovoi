//! A locally installed coding-agent CLI in print mode, run through the injected
//! [`ProcessExecutor`] (ADR 0004, ADR 0009).
//!
//! The CLI gets one prompt (system text and the conversation rendered as plain text) and
//! no tools; its final answer comes back as text.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    LlmError, LlmProvider, LlmRequest, LlmResponse, Part, ProcessExecutor, ProcessSpec, Role,
    StopReason, Usage,
};
use jiff::SignedDuration;
use serde_json::Value;

use crate::error::{LlmBuildError, excerpt};

/// The cap on captured stdout and stderr.
pub const MAX_OUTPUT_BYTES: u32 = 1 << 20;

/// An argument replaced by the prompt when it is passed as an argument.
pub const PROMPT_PLACEHOLDER: &str = "{prompt}";

/// How the prompt reaches the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptVia {
    /// Written to standard input.
    Stdin,
    /// Put in place of a [`PROMPT_PLACEHOLDER`] argument, or appended when there is none.
    Arg,
}

/// How to read the answer from standard output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliOutput {
    /// A JSON object whose string `field` holds the answer.
    Json {
        /// The field with the answer, for example `result`.
        field: String,
    },
    /// All of standard output is the answer.
    Text,
}

/// The program and argv template for one CLI.
///
/// The built-in presets are starting points. Flags change between CLI versions: check
/// them against the installed CLI's `--help` and override `args` in config when they
/// differ. A preset must ask for non-interactive print mode, a single turn, no tool use
/// and machine-readable output where the CLI supports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliPreset {
    /// Absolute path of the program.
    pub program: String,
    /// Arguments, passed as separate argv elements with no shell.
    pub args: Vec<String>,
    /// How the prompt is passed.
    pub prompt_via: PromptVia,
    /// How the answer is read.
    pub output: CliOutput,
    /// The complete environment of the process, for example `HOME` so the CLI finds its
    /// login. Empty by default.
    pub env: BTreeMap<String, String>,
}

impl CliPreset {
    /// Claude Code: print mode, JSON output, every built-in tool and MCP server off, no
    /// saved session, prompt on stdin.
    #[must_use]
    pub fn claude(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: [
                "-p",
                "--output-format",
                "json",
                "--tools",
                "",
                "--strict-mcp-config",
                "--no-session-persistence",
            ]
            .map(str::to_owned)
            .to_vec(),
            prompt_via: PromptVia::Stdin,
            output: CliOutput::Json {
                field: "result".to_owned(),
            },
            env: BTreeMap::new(),
        }
    }

    /// OpenAI Codex CLI: `exec` in a read-only sandbox, prompt on stdin (`-`), the final
    /// message as text.
    #[must_use]
    pub fn codex(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: [
                "exec",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "-",
            ]
            .map(str::to_owned)
            .to_vec(),
            prompt_via: PromptVia::Stdin,
            output: CliOutput::Text,
            env: BTreeMap::new(),
        }
    }

    /// The same preset with `args` in place of its own, unless `args` is empty.
    #[must_use]
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        if !args.is_empty() {
            self.args = args;
        }
        self
    }
}

/// A provider that runs a CLI through a [`ProcessExecutor`]. It offers no tools.
pub struct Cli {
    executor: Arc<dyn ProcessExecutor>,
    preset: CliPreset,
    timeout: SignedDuration,
}

impl std::fmt::Debug for Cli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cli")
            .field("program", &self.preset.program)
            .field("args", &self.preset.args)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Cli {
    /// A provider running `preset` through `executor`, killed after `timeout`.
    pub fn new(
        executor: Arc<dyn ProcessExecutor>,
        preset: CliPreset,
        timeout: SignedDuration,
    ) -> Result<Self, LlmBuildError> {
        if !is_absolute(&preset.program) {
            return Err(LlmBuildError::RelativeProgram(preset.program));
        }
        Ok(Self {
            executor,
            preset,
            timeout,
        })
    }

    /// The process spec for `prompt`.
    #[must_use]
    pub fn spec(&self, prompt: String) -> ProcessSpec {
        let (args, stdin) = match self.preset.prompt_via {
            PromptVia::Stdin => (self.preset.args.clone(), Some(prompt)),
            PromptVia::Arg => {
                let mut args = self.preset.args.clone();
                match args.iter_mut().find(|a| *a == PROMPT_PLACEHOLDER) {
                    Some(slot) => *slot = prompt,
                    None => args.push(prompt),
                }
                (args, None)
            }
        };
        ProcessSpec {
            program: self.preset.program.clone(),
            args,
            env: self.preset.env.clone(),
            cwd: None,
            stdin,
            timeout: self.timeout,
            max_output_bytes: MAX_OUTPUT_BYTES,
            run_as: None,
        }
    }
}

#[async_trait]
impl LlmProvider for Cli {
    fn id(&self) -> &str {
        "cli"
    }

    fn supports_tools(&self) -> bool {
        false
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let spec = self.spec(render_prompt(&req));
        tracing::debug!(program = %spec.program, "cli turn");
        let output = self
            .executor
            .run(spec)
            .await
            .map_err(|e| LlmError::Unavailable(format!("cannot run the CLI: {e}")))?;
        if output.timed_out {
            return Err(LlmError::Unavailable(format!(
                "the CLI timed out: {}",
                excerpt(&output.stderr, None)
            )));
        }
        if output.exit_code != Some(0) {
            let code = output
                .exit_code
                .map_or_else(|| "a signal".to_owned(), |c| format!("exit code {c}"));
            return Err(LlmError::Unavailable(format!(
                "the CLI failed with {code}: {}",
                excerpt(&output.stderr, None)
            )));
        }
        let (text, usage) = read_output(&self.preset.output, &output.stdout)?;
        Ok(LlmResponse {
            parts: vec![Part::Text(text)],
            stop: StopReason::EndTurn,
            usage,
        })
    }
}

/// The system text followed by the conversation as plain text.
#[must_use]
pub fn render_prompt(req: &LlmRequest) -> String {
    let mut out = req.system.trim().to_owned();
    for message in &req.messages {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(match message.role {
            Role::User => "User:",
            Role::Assistant => "Assistant:",
        });
        for part in &message.parts {
            out.push('\n');
            match part {
                Part::Text(text) => out.push_str(text),
                Part::ToolCall { name, input, .. } => {
                    out.push_str(&format!("[tool call {name}: {input}]"));
                }
                Part::ToolResult {
                    content, is_error, ..
                } => {
                    let label = if *is_error {
                        "tool error"
                    } else {
                        "tool result"
                    };
                    out.push_str(&format!("[{label}]\n{content}"));
                }
            }
        }
    }
    out
}

/// The answer and, when the CLI reports it, token usage.
fn read_output(output: &CliOutput, stdout: &str) -> Result<(String, Usage), LlmError> {
    let field = match output {
        CliOutput::Text => return Ok((stdout.trim().to_owned(), Usage::default())),
        CliOutput::Json { field } => field,
    };
    let value: Value = serde_json::from_str(stdout.trim()).map_err(|e| {
        LlmError::InvalidResponse(format!(
            "the CLI output is not JSON ({e}): {}",
            excerpt(stdout, None)
        ))
    })?;
    let text = value.get(field).and_then(Value::as_str).ok_or_else(|| {
        LlmError::InvalidResponse(format!("the CLI output has no text field `{field}`"))
    })?;
    if value.get("is_error").and_then(Value::as_bool) == Some(true) {
        return Err(LlmError::Unavailable(format!(
            "the CLI reported an error: {}",
            excerpt(text, None)
        )));
    }
    let tokens = |key: &str| {
        value
            .pointer(&format!("/usage/{key}"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };
    let usage = Usage {
        input_tokens: tokens("input_tokens"),
        output_tokens: tokens("output_tokens"),
        cost_micros: None,
    };
    Ok((text.trim().to_owned(), usage))
}

/// Absolute on Unix (`/usr/bin/claude`) or Windows (`C:\…`), independent of the build host.
fn is_absolute(program: &str) -> bool {
    let bytes = program.as_bytes();
    program.starts_with('/')
        || program.starts_with("\\\\")
        || (bytes.len() > 2
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}
