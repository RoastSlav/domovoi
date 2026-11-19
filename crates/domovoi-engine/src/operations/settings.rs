//! Limits and waits of the operation service. The defaults equal the `[orchestration]`
//! defaults in the hub configuration; the engine does not read configuration itself.

use domovoi_core::BudgetLimits;
use jiff::SignedDuration;

/// Limits and waits for operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationSettings {
    /// How many operations may be running, waiting or paused at once. The rest queue.
    pub max_running: u16,
    /// How long a proposed execution waits for a person.
    pub approval_ttl: SignedDuration,
    /// How long a question waits for an answer.
    pub question_timeout: SignedDuration,
    /// The longest an approved execution waits for its host's agent.
    pub agent_wait_limit: SignedDuration,
    /// Dispatches of one contract on one host per rolling hour; more are blocked.
    pub loop_guard_per_hour: u32,
    /// The limits a new operation gets when its seed names none.
    pub budgets: BudgetLimits,
    /// How long a failed attempt waits before the next one.
    pub retry_interval: SignedDuration,
    /// How long one evidence read may take.
    pub evidence_deadline: SignedDuration,
    /// The wait before verifying a write skill that declares no `verify`.
    pub verify_default_delay: SignedDuration,
    /// Added to a skill's own timeout for the dispatch deadline.
    pub dispatch_grace: SignedDuration,
    /// How long after a dispatch the first reconciliation becomes due.
    pub reconcile_delay: SignedDuration,
    /// How long reconciliation waits before asking again about a run still going.
    pub reconcile_interval: SignedDuration,
    /// How long one verification re-check may take.
    pub recheck_deadline: SignedDuration,
}

impl Default for OperationSettings {
    fn default() -> Self {
        Self {
            max_running: 4,
            approval_ttl: SignedDuration::from_hours(24),
            question_timeout: SignedDuration::from_hours(24),
            agent_wait_limit: SignedDuration::from_mins(30),
            loop_guard_per_hour: 3,
            budgets: BudgetLimits {
                model_turns: Some(20),
                evidence_requests: Some(40),
                executions: Some(5),
                cycles: Some(3),
                wall_seconds: Some(72 * 3600),
                tokens: Some(200_000),
                cost_micros: None,
            },
            retry_interval: SignedDuration::from_secs(5),
            evidence_deadline: SignedDuration::from_secs(30),
            verify_default_delay: SignedDuration::from_secs(30),
            dispatch_grace: SignedDuration::from_secs(30),
            reconcile_delay: SignedDuration::from_secs(60),
            reconcile_interval: SignedDuration::from_secs(30),
            recheck_deadline: SignedDuration::from_secs(60),
        }
    }
}
