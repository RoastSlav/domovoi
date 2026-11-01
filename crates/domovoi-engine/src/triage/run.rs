//! One triage run: caps, the repeat cache, the provider chain, proposals, then the outcome
//! attached through the incident manager. Also tracks consecutive failures.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use domovoi_core::ports::{LlmError, LlmMessage, LlmProvider, LlmRequest, Part, Role};
use domovoi_core::{ActionId, Incident, IncidentId, SkillSummary, TriageOutcome, TriageReport};
use jiff::{SignedDuration, Timestamp};

use super::agent::{self, text_of};
use super::cache::evidence_digest;
use super::report::parse_report;
use super::{DEGRADED_AFTER, SYSTEM_PROMPT, Shared, context, proposals, tools};
use crate::advisories::triage_degraded;
use crate::events::{DomainEvent, EventKind};

/// Run starts in the last hour and the failure streak.
#[derive(Debug, Default)]
pub(crate) struct Limits {
    starts: VecDeque<Timestamp>,
    failures: u32,
    degraded: bool,
}

fn limits(shared: &Shared) -> std::sync::MutexGuard<'_, Limits> {
    shared
        .limits
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Triages `incident`, attaches the outcome and returns it.
pub(crate) async fn triage(shared: &Shared, incident: &Incident) -> TriageOutcome {
    let (outcome, tokens) = outcome(shared, incident).await;
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
) -> (TriageOutcome, BTreeMap<ActionId, String>) {
    if let Some(reason) = over_cap(shared).await {
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
    if let Some((report, from)) = cached(shared, incident, &digest).await {
        return (TriageOutcome::Cached { report, from }, BTreeMap::new());
    }
    let skills = deps.link.skills(incident.host());
    let bundle = context::build(shared, incident, &skills).await;
    match providers(shared, incident, &skills, &bundle).await {
        Ok(mut report) => {
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
            (TriageOutcome::Failed { reason }, BTreeMap::new())
        }
    }
}

/// Why this run may not start, or `None` after recording its start.
async fn over_cap(shared: &Shared) -> Option<String> {
    let deps = &shared.deps;
    let settings = &shared.settings;
    let now = deps.clock.now();
    if let Some(budget) = settings.monthly_budget_micros {
        let month = now.to_zoned(jiff::tz::TimeZone::UTC).date();
        match deps.usage.month_total(month).await {
            Ok(total) if total.cost_micros >= budget => {
                return Some("the monthly triage budget is spent".to_owned());
            }
            Ok(_) => {}
            Err(err) => tracing::warn!(%err, "could not read model spending; triaging anyway"),
        }
    }
    let hour_ago = now
        .checked_sub(SignedDuration::from_hours(1))
        .unwrap_or(now);
    let mut limits = limits(shared);
    while limits.starts.front().is_some_and(|t| *t <= hour_ago) {
        limits.starts.pop_front();
    }
    if limits.starts.len() >= settings.per_hour_runs as usize {
        return Some(format!(
            "the limit of {} triage runs per hour is reached",
            settings.per_hour_runs
        ));
    }
    limits.starts.push_back(now);
    None
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

/// Whether the next provider should be tried after `err`.
fn falls_back(err: &LlmError) -> bool {
    matches!(
        err,
        LlmError::Unavailable(_) | LlmError::RateLimited { .. } | LlmError::Transport(_)
    )
}

/// The primary, then the secondary when the primary is unavailable.
async fn providers(
    shared: &Shared,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
) -> Result<TriageReport, String> {
    let deps = &shared.deps;
    let chain: Vec<&Arc<dyn LlmProvider>> = std::iter::once(&deps.primary)
        .chain(deps.secondary.as_ref())
        .collect();
    let mut last = String::from("no provider is configured");
    for (i, provider) in chain.iter().enumerate() {
        match attempt(shared, provider, incident, skills, bundle).await {
            Ok(report) => return Ok(report),
            Err(Failure::Provider(err)) => {
                last = format!("{} failed: {err}", provider.id());
                if !(falls_back(&err) && i + 1 < chain.len()) {
                    return Err(last);
                }
                tracing::info!(provider = provider.id(), %err, "triage falls back to the next provider");
            }
            Err(Failure::Invalid(why)) => return Err(why),
        }
    }
    Err(last)
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
/// tool output.
async fn opening(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
) -> String {
    let mut text = format!("Triage this incident.\n\n{bundle}");
    if !provider.supports_tools() {
        text.push_str("\n\n## Tool output gathered for you\n");
        for call in tools::prefetch(incident) {
            let name = call.tool().name();
            let result = tools::run(shared, incident, skills, call).await;
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
) -> Result<TriageReport, Failure> {
    let first = opening(shared, provider, incident, skills, bundle).await;
    let mut messages = vec![LlmMessage {
        role: Role::User,
        parts: vec![Part::Text(first)],
    }];
    let answer = agent::converse(
        shared,
        provider,
        incident,
        skills,
        SYSTEM_PROMPT,
        &mut messages,
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
