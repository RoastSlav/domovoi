//! `Notify`: attaches the outcome to the incident, records the tool use and the cached report,
//! and closes the investigation when nothing is left.
//!
//! Delivery is at least once, like every notification here: a crash between attaching the
//! outcome and finishing the notification step attaches the outcome once more on redelivery,
//! which shows as a second timeline entry and card edit. The tool use is recorded before the
//! outcome, as a direct triage run does, so the timeline reads the same either way.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{ClaimToken, EffectRecord};
use domovoi_core::{ActionId, Incident, OperationId, StepKind, StepOutcome, TriageOutcome};

use super::close::try_close;
use super::plumbing::{claim, conclude, handle_of, incident_of, runtime, short, step_of};
use super::turn_output::{add_pointer, caused_by, report_view};
use crate::cards::{MAX_PLANS, step_offers_chat};
use crate::chat_token::{mint_token, token_hash};
use crate::effects::{EffectHandler, EffectOutcome};
use crate::operations::effects::{Address, address, finish, load, unreadable};
use crate::operations::waiting::waiting_steps;
use crate::operations::writer::{Conclusion, OpShared};
use crate::triage::agent::ToolUse;
use crate::triage::run::digest;

/// The handler of `notify` effects.
pub(super) struct Notify {
    shared: Arc<OpShared>,
}

impl Notify {
    /// A handler over the operation service.
    pub(super) fn new(shared: Arc<OpShared>) -> Self {
        Self { shared }
    }
}

fn headline(outcome: &TriageOutcome) -> String {
    match outcome {
        TriageOutcome::Report(report) | TriageOutcome::Cached { report, .. } => {
            report.headline.clone()
        }
        TriageOutcome::Skipped { reason } | TriageOutcome::Failed { reason } => reason.clone(),
    }
}

/// One chat token for each waiting step of `operation` that may have chat buttons, rotated
/// into the step's row so only the hash is stored, and how many waiting steps have no buttons
/// and are approved in the web UI. A failure never fails the notification: that step is
/// counted as web only.
async fn mint_tokens(
    shared: &Arc<OpShared>,
    snapshot: &domovoi_core::OperationSnapshot,
    incident: &Incident,
    operation: OperationId,
) -> (BTreeMap<ActionId, String>, usize) {
    let mut tokens = BTreeMap::new();
    let host = match shared.deps.hosts.get_host(incident.host()).await {
        Ok(Some(host)) => host.name,
        _ => return (tokens, 1),
    };
    let waiting =
        match waiting_steps(&*shared.deps.operations, incident, &host, MAX_PLANS + 1).await {
            Ok(waiting) => waiting,
            Err(err) => {
                tracing::warn!(%err, "could not list the steps waiting for approval");
                return (tokens, 1);
            }
        };
    let handle = handle_of(shared);
    let mut web_only = 0;
    for (index, step) in waiting.iter().enumerate() {
        let ours = snapshot.execution_of(step.execution.as_step()).is_some();
        if index >= MAX_PLANS || !ours || !step_offers_chat(step) {
            web_only += 1;
            continue;
        }
        let token = mint_token();
        match handle
            .rotate_chat_token(operation, step.execution.as_step(), token_hash(&token))
            .await
        {
            Ok(()) => {
                tokens.insert(step.execution.as_action(), token);
            }
            Err(err) => {
                tracing::debug!(%err, "a step could not take a chat token");
                web_only += 1;
            }
        }
    }
    (tokens, web_only)
}

#[async_trait]
impl EffectHandler for Notify {
    async fn handle(&self, effect: &EffectRecord, token: ClaimToken) -> EffectOutcome {
        let shared = &self.shared;
        let held = claim(effect, token);
        let (Some(Address { operation, step }), Some(runtime)) = (address(effect), runtime(shared))
        else {
            return unreadable();
        };
        let Some(turn_id) = step else {
            return unreadable();
        };
        let snapshot = match load(shared, operation).await {
            Ok(snapshot) => snapshot,
            Err(done) => return done,
        };
        if snapshot.operation.status.is_terminal() {
            return finish(shared, held).await;
        }
        let dropped: Vec<String> = effect
            .payload
            .get("dropped")
            .and_then(|lines| serde_json::from_value(lines.clone()).ok())
            .unwrap_or_default();
        let tools: Vec<ToolUse> = effect
            .payload
            .get("tools")
            .and_then(|tools| serde_json::from_value(tools.clone()).ok())
            .unwrap_or_default();
        let Some(mut outcome) =
            step_of(&snapshot, turn_id).and_then(|turn| report_view(&snapshot, turn, &dropped))
        else {
            return EffectOutcome::Dead {
                error: "the turn has no outcome to attach".to_owned(),
            };
        };
        let incident = match incident_of(&runtime, &snapshot).await {
            Ok(Some(incident)) => incident,
            Ok(None) => return finish(shared, held).await,
            Err(done) => return done,
        };
        let triage = &runtime.triage;
        let deps = &triage.deps;
        if let TriageOutcome::Report(report) = &outcome {
            let evidence = digest(triage, &incident).await;
            if let Err(err) = deps
                .cache
                .put(&incident.fingerprint(), &evidence, report, deps.clock.now())
                .await
            {
                tracing::warn!(%err, "could not cache the triage report");
            }
        }
        if !tools.is_empty()
            && let Err(err) = deps
                .manager
                .record_tool_use(incident.id(), "triage".to_owned(), tools)
                .await
        {
            tracing::warn!(%err, incident = %incident.id(), "could not record the triage tool use");
        }
        let (tokens, web_only) = mint_tokens(shared, &snapshot, &incident, operation).await;
        if web_only > 0 {
            add_pointer(&mut outcome);
        }
        if let Err(err) = deps
            .manager
            .attach_triage(incident.id(), outcome.clone(), tokens)
            .await
        {
            return EffectOutcome::Retry {
                error: err.to_string(),
            };
        }
        let notification = caused_by(&snapshot, turn_id, StepKind::Notification)
            .into_iter()
            .find(|record| !record.progress.outcome.is_final())
            .map(|record| record.step.id);
        let ended = notification.map(|step| Conclusion {
            step,
            outcome: StepOutcome::Succeeded,
            summary: Some(short(&headline(&outcome))),
            reason: None,
            output: None,
            usage: None,
        });
        let done = conclude(shared, held, operation, ended, Vec::new()).await;
        runtime.waiters.resolve(incident.id(), &outcome);
        if matches!(done, EffectOutcome::Handled)
            && let Ok(fresh) = load(shared, operation).await
        {
            try_close(&handle_of(shared), &fresh).await;
        }
        done
    }
}
