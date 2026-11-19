//! The steps of one triage run: caps, the repeat cache and the provider chain. Also tracks
//! consecutive failures.

use std::sync::Arc;

use domovoi_core::ports::{LlmCapability, LlmError, LlmMessage, LlmProvider, Part, Role};
use domovoi_core::{Incident, IncidentId, ReadToolCall, SkillSummary, TriageReport};
use jiff::{SignedDuration, Timestamp};

use super::admission::{ADMISSION_RETRY, Purpose, Refusal, admit, unpriced_text};
use super::agent::{self, ToolUse, text_of};
use super::cache::evidence_digest;
use super::report::{parse_report, redact_report};
use super::{DEGRADED_AFTER, SYSTEM_PROMPT, Shared, tools};
use crate::advisories::triage_degraded;
use crate::events::{DomainEvent, EventKind};

/// The failure streak and whether the degraded advisory was raised.
#[derive(Debug, Default)]
pub(crate) struct Limits {
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

/// What the gates before a model call decided.
pub(crate) enum Prepared {
    /// A report for the same fingerprint and evidence exists, and the resolved incident it
    /// came from.
    Cached {
        /// The earlier report.
        report: TriageReport,
        /// The incident it was made for.
        from: IncidentId,
    },
    /// Admission refused the run.
    Refused(Refusal),
    /// The run may go ahead; its start is recorded.
    Admitted,
}

/// The digest of the evidence a report for `incident` would rest on: the incident and its
/// latest signal. Equal digests mean the repeat cache may answer.
pub(crate) async fn digest(shared: &Shared, incident: &Incident) -> [u8; 32] {
    let latest = shared
        .deps
        .signals
        .list_for_incident(incident.id(), 1)
        .await
        .ok()
        .and_then(|s| s.into_iter().next());
    evidence_digest(incident, latest.as_ref())
}

/// Runs the repeat cache and the admission gate, in that order: an answer that costs no
/// model call must not take a run of the hour or read the monthly budget. With `fresh` the
/// cache is skipped.
pub(crate) async fn prepare(shared: &Shared, incident: &Incident, fresh: bool) -> Prepared {
    if !fresh {
        let digest = digest(shared, incident).await;
        if let Some((report, from)) = cached(shared, incident, &digest).await {
            return Prepared::Cached { report, from };
        }
    }
    match admit(shared, Purpose::Triage).await {
        Ok(()) => Prepared::Admitted,
        Err(refusal) => Prepared::Refused(refusal),
    }
}

/// The sentence a skipped run shows and the time a retry is worth trying, if any.
pub(crate) fn refusal_text(shared: &Shared, refusal: &Refusal) -> (String, Option<Timestamp>) {
    match refusal {
        Refusal::BudgetSpent => ("the monthly triage budget is spent".to_owned(), None),
        Refusal::Unpriced { provider } => (unpriced_text(provider), None),
        Refusal::Hourly { retry_at: Some(at) } => (
            format!(
                "the limit of {} triage runs per hour is reached; it tries again from {}",
                shared.settings.per_hour_runs,
                at.strftime("%H:%M UTC"),
            ),
            Some(*at),
        ),
        Refusal::Hourly { retry_at: None } => (
            format!(
                "the limit of {} triage runs per hour is reached",
                shared.settings.per_hour_runs,
            ),
            None,
        ),
        // The alert is already out, so the run ends as skipped and is retried once the
        // limits are readable again.
        Refusal::Unverifiable => (
            "the model limits could not be checked, so triage did not run; it tries again \
             shortly"
                .to_owned(),
            shared.deps.clock.now().checked_add(ADMISSION_RETRY).ok(),
        ),
    }
}

/// What one model run produced.
pub(crate) struct Generated {
    /// The redacted report, or the redacted reason every provider failed.
    pub(crate) result: Result<TriageReport, String>,
    /// The tool calls the run made.
    pub(crate) tool_use: Vec<ToolUse>,
    /// What the successful responses used.
    pub(crate) usage: agent::RunUsage,
}

/// Runs the provider chain over `bundle` under the run's budgets and redacts the answer.
pub(crate) async fn generate(
    shared: &Shared,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
) -> Generated {
    let deps = &shared.deps;
    let mut tool_use: Vec<ToolUse> = Vec::new();
    let mut run = agent::RunBudget::for_run(shared);
    let result = providers(shared, incident, skills, bundle, &mut tool_use, &mut run)
        .await
        .map(|mut report| {
            redact_report(deps.redactor.as_ref(), &mut report);
            report
        })
        .map_err(|reason| deps.redactor.redact(&reason));
    Generated {
        result,
        tool_use,
        usage: run.usage(),
    }
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
    run: &mut agent::RunBudget,
) -> Result<TriageReport, String> {
    let chain = shared.chain();
    let mut reasons: Vec<String> = Vec::new();
    for (i, provider) in chain.iter().enumerate() {
        // A provider that refuses a capability changes how the run is built (the opening
        // prompt gathers tool output itself when tools are gone), so the attempt is rebuilt
        // once per distinct capability from the top instead of continuing with a prompt
        // written for the old shape. Each capability may be handled once per provider.
        let used_before = used.len();
        let mut handled: Vec<LlmCapability> = Vec::new();
        let why = loop {
            match attempt(shared, provider, incident, skills, bundle, used, run).await {
                Ok(report) => return Ok(report),
                Err(Failure::Provider(LlmError::CapabilityRejected { capability, .. }))
                    if !handled.contains(&capability) =>
                {
                    handled.push(capability);
                    used.truncate(used_before);
                    tracing::info!(
                        provider = provider.id(),
                        %capability,
                        "the provider refused a capability; rebuilding the run without it",
                    );
                }
                Err(Failure::Provider(err)) => {
                    break shared
                        .deps
                        .redactor
                        .redact(&format!("{} failed: {err}", provider_name(provider)));
                }
                Err(Failure::Invalid(why)) => {
                    break shared
                        .deps
                        .redactor
                        .redact(&format!("{} failed: {why}", provider_name(provider)));
                }
            }
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
/// tool output. Every prefetched call is pushed onto `used`. A prefetch stops at the run
/// deadline instead of extending it.
async fn opening(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    incident: &Incident,
    bundle: &str,
    used: &mut Vec<ToolUse>,
    run: &agent::RunBudget,
) -> Result<String, LlmError> {
    let mut text = format!("Triage this incident.\n\n{bundle}");
    if !provider.supports_tools() {
        text.push_str("\n\n## Tool output gathered for you\n");
        let scope = tools::Scope::of(incident);
        for call in tools::prefetch(incident) {
            if shared.deps.clock.now() >= run.deadline() {
                return Err(agent::deadline_error());
            }
            let name = call.tool().name();
            let skill = match &call {
                ReadToolCall::RunReadSkill { skill, .. } => Some(skill.to_string()),
                _ => None,
            };
            let result = tools::run(shared, scope, call, run.deadline()).await;
            used.push(ToolUse {
                tool: name.to_owned(),
                skill,
                failed: result.is_error,
                refused: false,
                raw: None,
            });
            text.push_str(&format!("\n### {name}\n\n{}\n", result.content));
        }
        if shared.deps.clock.now() >= run.deadline() {
            return Err(agent::deadline_error());
        }
        text.push_str("\nYou cannot call tools. Reply with the JSON report only.");
    }
    Ok(text)
}

/// One provider's run: the conversation, then the report, with one repair turn.
async fn attempt(
    shared: &Shared,
    provider: &Arc<dyn LlmProvider>,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
    used: &mut Vec<ToolUse>,
    run: &mut agent::RunBudget,
) -> Result<TriageReport, Failure> {
    let first = opening(shared, provider, incident, bundle, used, run).await?;
    let mut messages = vec![LlmMessage {
        role: Role::User,
        parts: vec![Part::Text(first)],
    }];
    let brief = agent::Brief {
        system: SYSTEM_PROMPT,
        scope: tools::Scope::of(incident),
        skills,
    };
    let answer = agent::converse(shared, provider, brief, &mut messages, used, run).await?;
    let error = match parse_report(&answer) {
        Ok(report) => return Ok(report),
        Err(error) => shared.deps.redactor.redact(&error),
    };
    messages.push(LlmMessage {
        role: Role::User,
        parts: vec![Part::Text(format!(
            "Your answer could not be used: {error}. Reply with only the JSON object described in your instructions."
        ))],
    });
    // The repair turn goes through the same request helper as every other: it draws on the
    // shared run budget, respects the deadline, and an incomplete answer is never accepted
    // or repaired again.
    let tools = if provider.supports_tools() {
        agent::read_tools()
    } else {
        Vec::new()
    };
    let repaired = agent::ask(shared, provider, SYSTEM_PROMPT, &messages, tools, run).await?;
    parse_report(&text_of(&repaired.parts))
        .map_err(|e| Failure::Invalid(format!("the model gave no usable report: {e}")))
}

pub(crate) async fn failed(shared: &Shared, incident: &Incident, reason: &str) {
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

pub(crate) async fn succeeded(shared: &Shared) {
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
