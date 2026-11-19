//! `[orchestration]`: the operation service and its default limits.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::duration::Duration;

/// `[orchestration]`: durable operations that wait for a person, dispatch skills and verify
/// the result. Off by default; while it is off the operation service does not start and
/// nothing else changes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationSection {
    /// Run investigations as operations: new incidents are investigated by an Investigate
    /// operation and its proposed actions are approved per step on the Operations page.
    /// Needs `[triage] enabled`. Read once at start, so changing it needs a restart.
    /// Default `false`.
    #[serde(default)]
    pub enabled: bool,
    /// Operations that may be running, waiting or paused at once; the rest queue.
    /// Default 4.
    #[serde(default = "default_max_running")]
    pub max_running: u16,
    /// How long a proposed action waits for a person to approve it. At most `"7d"`.
    /// Default `"24h"`.
    #[serde(default = "default_day")]
    pub approval_ttl: Duration,
    /// How long a question waits for an answer. Default `"24h"`.
    #[serde(default = "default_day")]
    pub question_timeout: Duration,
    /// How long an approved action waits for its host's agent to come back. Default `"30m"`.
    #[serde(default = "default_agent_wait_limit")]
    pub agent_wait_limit: Duration,
    /// Runs of one skill on one host allowed per hour before the loop guard refuses the
    /// next. Default 3.
    #[serde(default = "default_loop_guard")]
    pub loop_guard_per_hour: u32,
    /// Limits an operation starts with.
    #[serde(default)]
    pub budgets: OrchestrationBudgets,
}

impl Default for OrchestrationSection {
    fn default() -> Self {
        Self {
            enabled: false,
            max_running: default_max_running(),
            approval_ttl: default_day(),
            question_timeout: default_day(),
            agent_wait_limit: default_agent_wait_limit(),
            loop_guard_per_hour: default_loop_guard(),
            budgets: OrchestrationBudgets::default(),
        }
    }
}

/// `[orchestration.budgets]`: what one operation may spend before it pauses. A limit left
/// out of the file keeps its default; `cost_micros` has no default and so no limit.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationBudgets {
    /// Model turns. Default 20.
    #[serde(default = "default_model_turns")]
    pub model_turns: Option<u32>,
    /// Evidence requests. Default 40.
    #[serde(default = "default_evidence_requests")]
    pub evidence_requests: Option<u32>,
    /// Executions. Default 5.
    #[serde(default = "default_executions")]
    pub executions: Option<u32>,
    /// Repair cycles. Default 3.
    #[serde(default = "default_cycles")]
    pub cycles: Option<u32>,
    /// Wall-clock time from admission. Default `"72h"`.
    #[serde(default = "default_wall_time")]
    pub wall_time: Option<Duration>,
    /// Model tokens, input and output together. Default 200000.
    #[serde(default = "default_tokens")]
    pub tokens: Option<u64>,
    /// Model cost in millionths of a currency unit. No limit by default.
    #[serde(default)]
    pub cost_micros: Option<u64>,
}

impl Default for OrchestrationBudgets {
    fn default() -> Self {
        Self {
            model_turns: default_model_turns(),
            evidence_requests: default_evidence_requests(),
            executions: default_executions(),
            cycles: default_cycles(),
            wall_time: default_wall_time(),
            tokens: default_tokens(),
            cost_micros: None,
        }
    }
}

impl OrchestrationBudgets {
    /// Every counted limit with its key, for validation.
    pub(crate) fn counts(&self) -> [(&'static str, Option<u64>); 6] {
        [
            ("model_turns", self.model_turns.map(u64::from)),
            ("evidence_requests", self.evidence_requests.map(u64::from)),
            ("executions", self.executions.map(u64::from)),
            ("cycles", self.cycles.map(u64::from)),
            ("tokens", self.tokens),
            ("cost_micros", self.cost_micros),
        ]
    }
}

fn default_max_running() -> u16 {
    4
}
fn default_day() -> Duration {
    Duration::secs(24 * 3600)
}
fn default_agent_wait_limit() -> Duration {
    Duration::secs(30 * 60)
}
fn default_loop_guard() -> u32 {
    3
}
fn default_model_turns() -> Option<u32> {
    Some(20)
}
fn default_evidence_requests() -> Option<u32> {
    Some(40)
}
fn default_executions() -> Option<u32> {
    Some(5)
}
fn default_cycles() -> Option<u32> {
    Some(3)
}
fn default_wall_time() -> Option<Duration> {
    Some(Duration::secs(72 * 3600))
}
fn default_tokens() -> Option<u64> {
    Some(200_000)
}
