//! Prometheus metric names and the helpers that record them. The binary installs the
//! recorder; without one these calls do nothing.

use domovoi_core::{ActionStatus, Incident, IncidentStatus, SourceKind, TriageOutcome};

/// Signals received, by source (`builtin`, `webhook`, `probe`, `agent`).
pub const SIGNALS_TOTAL: &str = "domovoi_signals_total";
/// Active incidents, by status (`pending`, `open`, `acked`, `muted`).
pub const INCIDENTS: &str = "domovoi_incidents";
/// Triage runs, by outcome (`report`, `cached`, `skipped`, `failed`).
pub const TRIAGE_RUNS_TOTAL: &str = "domovoi_triage_runs_total";
/// Model tokens, by provider and direction (`input`, `output`).
pub const LLM_TOKENS_TOTAL: &str = "domovoi_llm_tokens_total";
/// Model cost in US dollars, as reported or implied by the provider.
pub const LLM_COST_USD_TOTAL: &str = "domovoi_llm_cost_usd_total";
/// Action status changes, by the new status.
pub const ACTIONS_TOTAL: &str = "domovoi_actions_total";

/// Every incident status a gauge is kept for; resolved incidents are not active.
const ACTIVE_STATUSES: [&str; 4] = ["pending", "open", "acked", "muted"];

pub(crate) fn source_label(source: &SourceKind) -> &'static str {
    match source {
        SourceKind::BuiltIn => "builtin",
        SourceKind::Webhook { .. } => "webhook",
        SourceKind::Probe => "probe",
        SourceKind::Agent => "agent",
    }
}

fn status_label(status: IncidentStatus) -> &'static str {
    match status {
        IncidentStatus::Pending => "pending",
        IncidentStatus::Open => "open",
        IncidentStatus::Acked => "acked",
        IncidentStatus::Muted { .. } => "muted",
        IncidentStatus::Resolved => "resolved",
    }
}

pub(crate) fn outcome_label(outcome: &TriageOutcome) -> &'static str {
    match outcome {
        TriageOutcome::Report(_) => "report",
        TriageOutcome::Cached { .. } => "cached",
        TriageOutcome::Skipped { .. } => "skipped",
        _ => "failed",
    }
}

/// Sets the incident gauges from the active set.
pub(crate) fn record_active(active: &[Incident]) {
    for label in ACTIVE_STATUSES {
        let count = active
            .iter()
            .filter(|i| status_label(i.status()) == label)
            .count();
        let count = u32::try_from(count).map_or(f64::from(u32::MAX), f64::from);
        metrics::gauge!(INCIDENTS, "status" => label).set(count);
    }
}

/// Records an action's new status.
pub(crate) fn record_action(status: &ActionStatus) {
    metrics::counter!(ACTIONS_TOTAL, "status" => status.name()).increment(1);
}

/// Records one model turn's usage.
pub(crate) fn record_llm(provider: &str, input: u32, output: u32, cost_micros: Option<u64>) {
    let provider = provider.to_owned();
    metrics::counter!(LLM_TOKENS_TOTAL, "provider" => provider.clone(), "direction" => "input")
        .increment(u64::from(input));
    metrics::counter!(LLM_TOKENS_TOTAL, "provider" => provider, "direction" => "output")
        .increment(u64::from(output));
    if let Some(micros) = cost_micros {
        let dollars = u32::try_from(micros).map_or_else(
            |_| f64::from(u32::try_from(micros / 1_000_000).unwrap_or(u32::MAX)),
            |micros| f64::from(micros) / 1_000_000.0,
        );
        // Counters are integers; the cost total is a float that only grows.
        metrics::gauge!(LLM_COST_USD_TOTAL).increment(dollars);
    }
}
