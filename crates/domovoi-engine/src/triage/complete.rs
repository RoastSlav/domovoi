//! One model call: prompt cap, run deadline, attribution and error redaction.

use std::sync::Arc;

use domovoi_core::ports::{
    CallOutcome, LlmError, LlmProvider, LlmRequest, LlmResponse, Usage, UsageEntry,
};
use jiff::{SignedDuration, Timestamp};

use super::Shared;
use super::prompt::{fit_to_cap, prompt_size, redact_error};

/// Runs one model turn, bounded by `deadline`, and records its usage.
///
/// The run deadline covers the whole request, streamed or not. The provider's own hard
/// per-call deadline is enforced inside the adapter wrapper, so it holds even while a
/// provider keeps streaming. Every attempt, including a retry, records its usage, so
/// retries count towards the monthly budget, and a failed call records a row too.
pub(crate) async fn complete(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    req: LlmRequest,
    deadline: Timestamp,
) -> Result<LlmResponse, LlmError> {
    let (req, trimmed) = fit_to_cap(req, shared.settings.max_prompt_bytes)?;
    let bytes = prompt_size(&req);
    crate::telemetry::record_prompt_bytes(provider.id(), bytes);
    tracing::debug!(provider = provider.id(), bytes, "llm prompt size");
    if trimmed {
        crate::telemetry::record_prompt_trimmed();
        tracing::info!(
            provider = provider.id(),
            bytes,
            cap = shared.settings.max_prompt_bytes,
            "prompt trimmed to the size limit",
        );
    }
    let left = deadline.duration_since(shared.deps.clock.now());
    if left <= SignedDuration::ZERO {
        return Err(LlmError::Unavailable(
            "the run deadline passed before the model was asked; raise [triage] per_run.seconds"
                .to_owned(),
        ));
    }
    let started = shared.deps.clock.now();
    let result = match tokio::time::timeout(left.unsigned_abs(), provider.complete(req)).await {
        Ok(result) => result,
        Err(_) => Err(LlmError::Unavailable(
            "the run deadline passed while the model was answering; \
             raise [triage] per_run.seconds"
                .to_owned(),
        )),
    };
    let latency_ms = u64::try_from(
        shared
            .deps
            .clock
            .now()
            .duration_since(started)
            .as_millis()
            .max(0),
    )
    .unwrap_or(u64::MAX);
    match result {
        Ok(response) => {
            let usage = response.usage;
            record_usage(
                shared,
                provider,
                CallOutcome::Ok,
                Some(usage),
                latency_ms,
                started,
            )
            .await;
            crate::telemetry::record_llm(
                provider.id(),
                usage.input_tokens,
                usage.output_tokens,
                usage.cost_micros,
            );
            crate::telemetry::record_call(
                provider.id(),
                provider.model(),
                CallOutcome::Ok.as_str(),
            );
            // A monthly budget in dollars cannot count a turn whose price is unknown, and
            // an unpriced turn must never be written down as free. The row above keeps the
            // unknown price, so admission fails closed on it rather than reading it as zero.
            if usage.cost_micros.is_none() && shared.settings.monthly_budget_micros.is_some() {
                return Err(LlmError::Unavailable(format!(
                    "{} did not price this turn, so the monthly model budget cannot count it; \
                     set a price for it (0 for a free model) or remove the monthly budget",
                    super::run::provider_name(provider),
                )));
            }
            Ok(response)
        }
        Err(err) => {
            let err = redact_error(shared.deps.redactor.as_ref(), err);
            record_usage(
                shared,
                provider,
                CallOutcome::from(&err),
                None,
                latency_ms,
                started,
            )
            .await;
            crate::telemetry::record_call(
                provider.id(),
                provider.model(),
                CallOutcome::from(&err).as_str(),
            );
            Err(err)
        }
    }
}

/// Writes one usage row. A failed write is only logged: it never turns an answer into a
/// failure.
async fn record_usage(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    outcome: CallOutcome,
    usage: Option<Usage>,
    latency_ms: u64,
    at: Timestamp,
) {
    let entry = UsageEntry {
        provider: provider.id(),
        model: provider.model(),
        outcome,
        tokens_in: usage.map_or(0, |usage| u64::from(usage.input_tokens)),
        tokens_out: usage.map_or(0, |usage| u64::from(usage.output_tokens)),
        cost_micros: usage.and_then(|usage| usage.cost_micros),
        usage_source: usage.map(|usage| usage.source),
        latency_ms,
        at,
    };
    if let Err(err) = shared.deps.usage.record_call(&entry).await {
        tracing::warn!(%err, "could not record model usage");
    }
}
