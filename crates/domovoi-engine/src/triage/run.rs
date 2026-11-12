//! One triage run: caps, the repeat cache, the provider chain, proposals, then the outcome
//! attached through the incident manager. Also tracks consecutive failures.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use domovoi_core::ports::{LlmError, LlmMessage, LlmProvider, LlmRequest, Part, Role};
use domovoi_core::{
    ActionId, Incident, IncidentId, ReadToolCall, SkillSummary, TriageOutcome, TriageReport,
};
use jiff::{SignedDuration, Timestamp};

use super::admission::{Purpose, Refusal, admit};
use super::agent::{self, ToolUse, text_of};
use super::cache::evidence_digest;
use super::report::{parse_report, redact_report};
use super::{
    DEGRADED_AFTER, FAILED_RETRY, RetryReason, SYSTEM_PROMPT, Shared, context, proposals, tools,
};
use crate::advisories::triage_degraded;
use crate::events::{DomainEvent, EventKind};

/// Run starts in the last hour and the failure streak.
#[derive(Debug, Default)]
pub(crate) struct Limits {
    pub(crate) starts: VecDeque<Timestamp>,
    pub(crate) failures: u32,
    pub(crate) degraded: bool,
}

/// Guards the run window and failure streak for every gate and outcome path.
pub(crate) fn limits(shared: &Shared) -> std::sync::MutexGuard<'_, Limits> {
    shared
        .limits
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Triages `incident`, attaches the outcome and returns it. With `fresh`, the repeat cache
/// is skipped.
pub(crate) async fn triage(shared: &Shared, incident: &Incident, fresh: bool) -> TriageOutcome {
    let (outcome, tokens) = outcome(shared, incident, fresh).await;
    if let Err(err) = shared
        .deps
        .manager
        .attach_triage(incident.id(), outcome.clone(), tokens)
        .await
    {
        tracing::warn!(%err, incident = %incident.id(), "could not attach the triage outcome");
    }
    outcome
}

async fn outcome(
    shared: &Shared,
    incident: &Incident,
    fresh: bool,
) -> (TriageOutcome, BTreeMap<ActionId, String>) {
    if let Err(refusal) = admit(shared, Purpose::Triage).await {
        let reason = match refusal {
            Refusal::BudgetSpent => "the monthly triage budget is spent".to_owned(),
            Refusal::Hourly { retry_at } => {
                if let Some(at) = retry_at {
                    shared.defer(incident.id(), at, RetryReason::Cap);
                    format!(
                        "the limit of {} triage runs per hour is reached; it tries again from {}",
                        shared.settings.per_hour_runs,
                        at.strftime("%H:%M UTC"),
                    )
                } else {
                    format!(
                        "the limit of {} triage runs per hour is reached",
                        shared.settings.per_hour_runs,
                    )
                }
            }
        };
        return (TriageOutcome::Skipped { reason }, BTreeMap::new());
    }
    let deps = &shared.deps;
    let latest = deps
        .signals
        .list_for_incident(incident.id(), 1)
        .await
        .ok()
        .and_then(|s| s.into_iter().next());
    let digest = evidence_digest(incident, latest.as_ref());
    if !fresh && let Some((mut report, from)) = cached(shared, incident, &digest).await {
        // The repeat incident gets the same approve buttons a fresh triage would: the
        // cached report's proposals are re-validated against the host's current skills.
        let skills = deps.link.skills(incident.host());
        let tokens = proposals::create(shared, incident, &skills, &mut report).await;
        return (TriageOutcome::Cached { report, from }, tokens);
    }
    let skills = deps.link.skills(incident.host());
    let bundle = context::build(shared, incident, &skills).await;
    let mut tool_use: Vec<ToolUse> = Vec::new();
    let outcome = match providers(shared, incident, &skills, &bundle, &mut tool_use).await {
        Ok(mut report) => {
            redact_report(deps.redactor.as_ref(), &mut report);
            let tokens = proposals::create(shared, incident, &skills, &mut report).await;
            if let Err(err) = deps
                .cache
                .put(&incident.fingerprint(), &digest, &report, deps.clock.now())
                .await
            {
                tracing::warn!(%err, "could not cache the triage report");
            }
            succeeded(shared).await;
            (TriageOutcome::Report(report), tokens)
        }
        Err(reason) => {
            let reason = deps.redactor.redact(&reason);
            failed(shared, incident, &reason).await;
            if !limits(shared).degraded
                && let Some(at) = deps.clock.now().checked_add(FAILED_RETRY).ok()
            {
                shared.defer(incident.id(), at, RetryReason::Failed);
            }
            (TriageOutcome::Failed { reason }, BTreeMap::new())
        }
    };
    if !tool_use.is_empty()
        && let Err(err) = deps
            .manager
            .record_tool_use(incident.id(), "triage".to_owned(), tool_use)
            .await
    {
        tracing::warn!(%err, incident = %incident.id(), "could not record the triage tool use");
    }
    outcome
}

/// A cached report for the same fingerprint and evidence, and the resolved incident it
/// came from.
async fn cached(
    shared: &Shared,
    incident: &Incident,
    digest: &[u8; 32],
) -> Option<(TriageReport, IncidentId)> {
    let deps = &shared.deps;
    let now = deps.clock.now();
    let since = now
        .checked_sub(SignedDuration::from_hours(
            24 * i64::from(shared.settings.cache_days),
        ))
        .unwrap_or(now);
    let fingerprint = incident.fingerprint();
    let report = deps.cache.get(&fingerprint, digest, since).await.ok()??;
    let from = deps
        .incidents
        .find_recent_resolved_by_fingerprint(&fingerprint, since)
        .await
        .ok()??;
    (from.id() != incident.id()).then(|| (report, from.id()))
}

/// The primary, then each fallback provider in order whenever the provider before it failed
/// for any reason, including an unusable answer. Tool calls from every attempt end up in `used`.
/// How a provider is named in failure text: its id, plus its model when it has one.
pub(crate) fn provider_name(provider: &Arc<dyn LlmProvider>) -> String {
    match provider.model() {
        Some(model) => format!("{} model {}", provider.id(), model),
        None => provider.id().to_owned(),
    }
}

async fn providers(
    shared: &Shared,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
    used: &mut Vec<ToolUse>,
) -> Result<TriageReport, String> {
    let chain = shared.chain();
    let mut reasons: Vec<String> = Vec::new();
    for (i, provider) in chain.iter().enumerate() {
        let why = match attempt(shared, provider, incident, skills, bundle, used).await {
            Ok(report) => return Ok(report),
            Err(Failure::Provider(err)) => format!("{} failed: {err}", provider_name(provider)),
            Err(Failure::Invalid(why)) => format!("{} failed: {why}", provider_name(provider)),
        };
        if i + 1 < chain.len() {
            tracing::info!(
                provider = provider.id(),
                reason = why.as_str(),
                "triage falls back to the next provider",
            );
        }
        reasons.push(why);
    }
    Err(reasons.join("; "))
}

enum Failure {
    Provider(LlmError),
    Invalid(String),
}

impl From<LlmError> for Failure {
    fn from(err: LlmError) -> Self {
        Self::Provider(err)
    }
}

/// The first user message: the bundle and, for providers without tools, the prefetched
/// tool output. Every prefetched call is pushed onto `used`.
async fn opening(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
    used: &mut Vec<ToolUse>,
) -> String {
    let mut text = format!("Triage this incident.\n\n{bundle}");
    if !provider.supports_tools() {
        text.push_str("\n\n## Tool output gathered for you\n");
        let scope = tools::Scope::of(incident);
        for call in tools::prefetch(incident) {
            let name = call.tool().name();
            let skill = match &call {
                ReadToolCall::RunReadSkill { skill, .. } => Some(skill.to_string()),
                _ => None,
            };
            let result = tools::run(shared, scope, skills, call).await;
            used.push(ToolUse {
                tool: name.to_owned(),
                skill,
                failed: result.is_error,
                refused: false,
                raw: None,
            });
            text.push_str(&format!("\n### {name}\n\n{}\n", result.content));
        }
        text.push_str("\nYou cannot call tools. Reply with the JSON report only.");
    }
    text
}

/// One provider's run: the conversation, then the report, with one repair turn.
async fn attempt(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
    used: &mut Vec<ToolUse>,
) -> Result<TriageReport, Failure> {
    let first = opening(shared, provider, incident, skills, bundle, used).await;
    let mut messages = vec![LlmMessage {
        role: Role::User,
        parts: vec![Part::Text(first)],
    }];
    let answer = agent::converse(
        shared,
        provider,
        tools::Scope::of(incident),
        skills,
        SYSTEM_PROMPT,
        &mut messages,
        used,
    )
    .await?;
    let error = match parse_report(&answer) {
        Ok(report) => return Ok(report),
        Err(error) => error,
    };
    messages.push(LlmMessage {
        role: Role::User,
        parts: vec![Part::Text(format!(
            "Your answer could not be used: {error}. Reply with only the JSON object described in your instructions."
        ))],
    });
    let req = LlmRequest {
        system: SYSTEM_PROMPT.to_owned(),
        messages,
        tools: if provider.supports_tools() {
            agent::read_tools()
        } else {
            Vec::new()
        },
        max_output_tokens: shared.settings.budget.max_output_tokens,
    };
    let repaired = agent::complete(shared, provider, req).await?;
    parse_report(&text_of(&repaired.parts))
        .map_err(|e| Failure::Invalid(format!("the model gave no usable report: {e}")))
}

async fn failed(shared: &Shared, incident: &Incident, reason: &str) {
    let raise = {
        let mut limits = limits(shared);
        limits.failures += 1;
        let raise = limits.failures >= DEGRADED_AFTER && !limits.degraded;
        if raise {
            limits.degraded = true;
        }
        raise.then_some(limits.failures)
    };
    let Some(failures) = raise else { return };
    let deps = &shared.deps;
    let why = format!("The last {failures} triage runs failed, most recently: {reason}");
    if let Err(err) = deps
        .advisories
        .upsert(&triage_degraded(&why, deps.clock.now()))
        .await
    {
        tracing::warn!(%err, "could not store the triage advisory");
    }
    let event = DomainEvent::new(EventKind::TriageDegraded { reason: why }, incident, true);
    let _ = deps.bus.send(event);
}

async fn succeeded(shared: &Shared) {
    let clear = {
        let mut limits = limits(shared);
        limits.failures = 0;
        std::mem::take(&mut limits.degraded)
    };
    if clear {
        let deps = &shared.deps;
        let id = triage_degraded("", deps.clock.now()).id;
        if let Err(err) = deps.advisories.resolve(id, deps.clock.now()).await {
            tracing::warn!(%err, "could not resolve the triage advisory");
        }
    }
}
