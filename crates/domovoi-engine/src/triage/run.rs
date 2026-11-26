//! The steps of one triage run: caps, the repeat cache and the provider chain. Also tracks
//! consecutive failures.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::meta::PlanStepInput;
use domovoi_core::ports::{LlmError, LlmMessage, LlmProvider, Part, Role};
use domovoi_core::{Incident, IncidentId, ReadToolCall, SkillSummary, TriageReport};
use jiff::{SignedDuration, Timestamp};

use super::admission::{ADMISSION_RETRY, Purpose, Refusal, admit, unpriced_text};
use super::agent::{self, ToolUse};
use super::cache::evidence_digest;
use super::chain::{Attempt, AttemptError, ChainState, run_chain};
use super::meta::{self, EvidenceRecord, LoopEnd, LoopOutput, LoopScope, meta_prompt};
use super::report::{guard_security_verdict, redact_report};
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

/// How a successful model run ended.
pub(crate) enum GeneratedEnd {
    /// The redacted report, its proposals the validated plan steps.
    Report(TriageReport),
    /// The redacted question the model asked.
    Asked(String),
}

/// What one model run produced.
pub(crate) struct Generated {
    /// The redacted end of the run, or the redacted reason every provider failed.
    pub(crate) result: Result<GeneratedEnd, String>,
    /// The tool calls the run made.
    pub(crate) tool_use: Vec<ToolUse>,
    /// What the successful responses used.
    pub(crate) usage: agent::RunUsage,
    /// Every parsed read the run made, also when it failed.
    pub(crate) evidence: Vec<EvidenceRecord>,
    /// The staged plan steps as the model wrote them, at most three.
    pub(crate) plan: Vec<PlanStepInput>,
    /// The "Proposals not offered" lines, redacted.
    pub(crate) dropped: Vec<String>,
}

/// Runs the provider chain over `bundle` under the run's budgets, within `scope`, and
/// redacts the answer.
pub(crate) async fn generate(
    shared: &Shared,
    incident: &Incident,
    skills: &[SkillSummary],
    bundle: &str,
    scope: LoopScope,
) -> Generated {
    let deps = &shared.deps;
    let mut state = ChainState::new(shared);
    let attempt = InvestigateAttempt {
        shared,
        incident,
        skills,
        bundle,
        scope: &scope,
    };
    let mut plan = Vec::new();
    let mut dropped = Vec::new();
    let result = match run_chain(shared, &mut state, &attempt).await {
        Ok(output) => {
            plan = output.plan.into_iter().map(|step| step.input).collect();
            dropped = output.dropped;
            match output.end {
                LoopEnd::Report(mut report) => {
                    redact_report(deps.redactor.as_ref(), &mut report);
                    if guard_security_verdict(incident.check().as_str(), &mut report, &state.used) {
                        tracing::info!(
                            incident = %incident.id(),
                            "a security event was called expected without tool evidence; \
                             the verdict is now monitor"
                        );
                    }
                    Ok(GeneratedEnd::Report(report))
                }
                LoopEnd::Asked(question) => Ok(GeneratedEnd::Asked(question)),
            }
        }
        Err(reasons) => Err(deps.redactor.redact(&reasons.join("; "))),
    };
    Generated {
        result,
        tool_use: state.used,
        usage: state.run.usage(),
        evidence: state.evidence,
        plan,
        dropped,
    }
}

/// One investigation run over the provider chain: the incident, the context bundle and the
/// loop's scope.
struct InvestigateAttempt<'a> {
    shared: &'a Shared,
    incident: &'a Incident,
    skills: &'a [SkillSummary],
    bundle: &'a str,
    scope: &'a LoopScope,
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
            let result = tools::run(&shared.deps, scope, call, run.deadline()).await;
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

#[async_trait]
impl Attempt for InvestigateAttempt<'_> {
    type Output = LoopOutput;

    /// One provider's run: the opening, then the investigation loop. A provider that calls
    /// tools gets the meta-tools and their prompt; one that does not gets the prefetched
    /// reads and the text prompt, and its answer is read as a report.
    async fn attempt(
        &self,
        provider: &Arc<dyn LlmProvider>,
        state: &mut ChainState,
    ) -> Result<LoopOutput, AttemptError> {
        let shared = self.shared;
        let first = opening(
            shared,
            provider,
            self.incident,
            self.bundle,
            &mut state.used,
            &state.run,
        )
        .await?;
        let mut messages = vec![LlmMessage {
            role: Role::User,
            parts: vec![Part::Text(first)],
        }];
        let system = if provider.supports_tools() {
            meta_prompt(self.scope.tools)
        } else {
            SYSTEM_PROMPT.to_owned()
        };
        let ctx = meta::Ctx {
            shared,
            provider,
            system: &system,
            scope: tools::Scope::of(self.incident),
            skills: self.skills,
        };
        meta::investigate(
            &ctx,
            self.scope,
            &mut messages,
            &mut state.used,
            &mut state.evidence,
            &mut state.run,
        )
        .await
    }
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
