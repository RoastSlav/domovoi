//! Prometheus metric names and the helpers that record them. The binary installs the
//! recorder; without one these calls do nothing.

use domovoi_core::{Incident, IncidentStatus, SourceKind, TriageOutcome};

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
/// Model calls, by provider, model and outcome (`ok`, `rate_limited`, `auth`, `timeout`,
/// `transport`, `invalid_response`, `unavailable`, `capability_rejected`).
pub const LLM_CALLS_TOTAL: &str = "domovoi_llm_calls_total";
/// Prompt size in bytes, by provider.
pub const LLM_PROMPT_BYTES: &str = "domovoi_llm_prompt_bytes";
/// Prompts that had to be shortened to fit `triage.per_run.prompt_bytes`.
pub const LLM_PROMPT_TRIMMED_TOTAL: &str = "domovoi_llm_prompt_trimmed_total";
/// Admission refusals, by reason (`budget`, `unpriced`, `hourly`, `unverifiable`).
pub const LLM_ADMISSION_REJECTIONS_TOTAL: &str = "domovoi_llm_admission_rejections_total";
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

/// Records one model call's outcome. Labels carry only configured identifiers: a model name
/// or `unknown`, never text from an answer or an error.
pub(crate) fn record_call(provider: &str, model: Option<&str>, outcome: &'static str) {
    metrics::counter!(
        LLM_CALLS_TOTAL,
        "provider" => provider.to_owned(),
        "model" => model.unwrap_or("unknown").to_owned(),
        "outcome" => outcome,
    )
    .increment(1);
}

/// Records the size of one prompt in bytes.
pub(crate) fn record_prompt_bytes(provider: &str, bytes: usize) {
    metrics::histogram!(LLM_PROMPT_BYTES, "provider" => provider.to_owned()).record(bytes as f64);
}

/// Records that one prompt had to be shortened to fit the configured cap.
pub(crate) fn record_prompt_trimmed() {
    metrics::counter!(LLM_PROMPT_TRIMMED_TOTAL).increment(1);
}

/// Records an admission refusal by reason.
pub(crate) fn record_admission_rejection(reason: &'static str) {
    metrics::counter!(LLM_ADMISSION_REJECTIONS_TOTAL, "reason" => reason).increment(1);
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
