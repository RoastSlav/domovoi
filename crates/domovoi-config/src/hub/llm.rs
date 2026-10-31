//! `[llm]` and `[triage]` in `hub.toml`.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;
use url::Url;

use crate::secret::Secret;

/// `[llm]`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LlmSection {
    /// The provider tried first. Required when triage is enabled.
    #[serde(default)]
    pub primary: Option<LlmProvider>,
    /// A fallback used when the primary fails. Optional.
    #[serde(default)]
    pub secondary: Option<LlmProvider>,
}

/// `[llm.primary]` or `[llm.secondary]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LlmProvider {
    /// Which adapter talks to the model.
    pub kind: LlmKind,
    /// Model name. Required for `anthropic` and `openai_compatible`.
    #[serde(default)]
    pub model: Option<String>,
    /// API base URL. Required for `openai_compatible`; optional for `anthropic`.
    #[serde(default)]
    pub base_url: Option<Url>,
    /// API key. Required for `anthropic`.
    #[serde(default)]
    pub api_key: Option<Secret>,
    /// How to run a local CLI. Required for `cli`.
    #[serde(default)]
    pub cli: Option<CliConfig>,
    /// Prices used for the monthly budget. Optional.
    #[serde(default)]
    pub price: Option<Price>,
}

/// The kind of language model adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LlmKind {
    /// The Anthropic Messages API.
    Anthropic,
    /// Any server speaking the OpenAI chat completions API.
    OpenaiCompatible,
    /// A local command-line tool.
    Cli,
}

/// `cli = { .. }` on an LLM provider.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CliConfig {
    /// A known tool whose arguments Domovoi supplies. Set this or `command`.
    #[serde(default)]
    pub preset: Option<CliPreset>,
    /// Absolute path of the program to run instead of a preset's default.
    #[serde(default)]
    pub command: Option<PathBuf>,
    /// Extra arguments passed to the program.
    #[serde(default)]
    pub args: Vec<String>,
}

/// A known LLM command-line tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CliPreset {
    /// Claude Code.
    Claude,
    /// OpenAI Codex CLI.
    Codex,
}

/// `price = { .. }`: what a provider charges, in US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Price {
    /// Dollars per million input tokens.
    pub input_per_mtok_usd: f64,
    /// Dollars per million output tokens.
    pub output_per_mtok_usd: f64,
}

/// `[triage]`.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TriageSection {
    /// Whether new incidents are triaged automatically. Default `false`.
    #[serde(default)]
    pub enabled: bool,
    /// Triage runs at the same time. Default 2.
    #[serde(default = "default_concurrency")]
    pub concurrency: u8,
    /// Limits for a single run.
    #[serde(default)]
    pub per_run: TriageLimits,
    /// Runs started per hour at most. Default 6.
    #[serde(default = "default_per_hour_runs")]
    pub per_hour_runs: u32,
    /// Spending cap per calendar month, in US dollars. Optional.
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
    /// Days a triage report is reused for the same evidence. Default 7.
    #[serde(default = "default_cache_days")]
    pub cache_days: u32,
}

impl Default for TriageSection {
    fn default() -> Self {
        Self {
            enabled: false,
            concurrency: default_concurrency(),
            per_run: TriageLimits::default(),
            per_hour_runs: default_per_hour_runs(),
            monthly_budget_usd: None,
            cache_days: default_cache_days(),
        }
    }
}

/// `per_run = { .. }` in `[triage]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TriageLimits {
    /// Tool calls per run. Default 15.
    #[serde(default = "default_tool_calls")]
    pub tool_calls: u16,
    /// Wall-clock seconds per run. Default 90.
    #[serde(default = "default_run_seconds")]
    pub seconds: u32,
    /// Output tokens per run. Default 2000.
    #[serde(default = "default_output_tokens")]
    pub output_tokens: u32,
}

impl Default for TriageLimits {
    fn default() -> Self {
        Self {
            tool_calls: default_tool_calls(),
            seconds: default_run_seconds(),
            output_tokens: default_output_tokens(),
        }
    }
}

fn default_concurrency() -> u8 {
    2
}
fn default_per_hour_runs() -> u32 {
    6
}
fn default_cache_days() -> u32 {
    7
}
fn default_tool_calls() -> u16 {
    15
}
fn default_run_seconds() -> u32 {
    90
}
fn default_output_tokens() -> u32 {
    2000
}
