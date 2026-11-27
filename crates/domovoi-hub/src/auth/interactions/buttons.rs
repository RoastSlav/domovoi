//! Card buttons: Ack, Mute 1h, Details, and Approve or Deny with the action's single-use
//! token. Only operation steps can be decided; a press naming a retired action is refused.

use domovoi_core::triage::current_episode;
use domovoi_core::{
    CardButton, ExecutionError, ExecutionId, IncidentId, PlanBinding, Risk, TriageOutcome,
    TriageReport,
};
use domovoi_engine::operations::OperationError;
use jiff::SignedDuration;
use serde_json::json;

use super::{Accepted, Actor, InteractionAuthorizer};
use crate::services::{OperationCommandError, StepRef, latest_triage};

/// How long the Mute 1h button mutes.
pub const MUTE_BUTTON: SignedDuration = SignedDuration::from_hours(1);

/// What a destructive action's Approve says (INV-6).
pub const WEB_ONLY: &str = "Approve this one in the web UI";

/// What an approval button says when the card is older than the plan binding it needs.
pub const STALE_CARD: &str = "This card is out of date; open the incident in the web UI to approve";

/// The report as the Details button posts it.
#[must_use]
pub fn details_text(report: &TriageReport) -> String {
    let mut text = format!("**{}**\n\n{}", report.headline, report.summary);
    if !report.cause.is_empty() {
        text.push_str(&format!("\n\n**Likely cause:** {}", report.cause));
    }
    if !report.evidence.is_empty() {
        text.push_str("\n\n**Evidence**");
        for e in &report.evidence {
            text.push_str(&format!("\n• {}: {} ({})", e.label, e.value, e.source));
        }
    }
    text.push_str(&format!("\n\n**Next step:** {}", report.next_step));
    if !report.details.is_empty() {
        text.push_str(&format!("\n\n{}", report.details));
    }
    text
}

/// Why a chat press on a step was refused, in one line for the thread.
#[must_use]
pub fn step_reason(err: &OperationError) -> String {
    match err {
        OperationError::NotFound(_) => "That action no longer exists",
        OperationError::BadToken => "This button no longer works",
        OperationError::Execution(ExecutionError::PlanChanged) => {
            "This card is out of date; open the incident to review it"
        }
        OperationError::Execution(ExecutionError::NotYetSupported) => {
            "Policy approvals are not supported yet; a person must approve"
        }
        OperationError::Execution(ExecutionError::PermitNotAllowed) => {
            "A person must approve this one"
        }
        OperationError::Execution(ExecutionError::NeedsTypedConfirmation) => {
            "Approve this one in the web UI"
        }
        OperationError::Execution(ExecutionError::AlreadyAuthorized)
        | OperationError::Decide(_) => "Someone already decided this action",
        OperationError::Execution(ExecutionError::Expired) => "The approval window has closed",
        OperationError::Conflict | OperationError::StaleEffect => {
            "This step was taken over; try again"
        }
        OperationError::HostUnavailable(_) => "The host's agent is not connected; try again later",
        OperationError::Store(_) => "Something went wrong; try again",
        OperationError::Unavailable => "The service that decides actions is restarting; try again",
    }
    .to_owned()
}

/// Why a service command on a step was refused, in one line for the thread.
fn command_reason(err: &OperationCommandError) -> String {
    match err {
        OperationCommandError::Operation(err) => step_reason(err),
        OperationCommandError::WebOnly | OperationCommandError::TypedConfirmationRequired => {
            WEB_ONLY.to_owned()
        }
        OperationCommandError::NotFound => "That action no longer exists".to_owned(),
        OperationCommandError::NotEnabled => {
            "Operations are not running; open the incident in the web UI".to_owned()
        }
        OperationCommandError::Conflict(_) | OperationCommandError::Invalid(_) => {
            "Something went wrong; try again".to_owned()
        }
    }
}

/// The binding a chat press carries: the first 3 bytes of the plan hash, or a stale card.
fn prefix_binding(plan_prefix: Option<&str>) -> Result<PlanBinding, String> {
    plan_prefix
        .and_then(|text| hex::decode(text).ok())
        .filter(|bytes| bytes.len() == 3)
        .map(PlanBinding::Prefix)
        .ok_or_else(|| STALE_CARD.to_owned())
}

impl InteractionAuthorizer {
    pub(super) async fn button(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        button: &CardButton,
    ) -> Result<Accepted, String> {
        let subject = format!("incident:{incident}");
        match button {
            CardButton::Ack => {
                self.services
                    .incidents
                    .ack(&actor.caller(), incident)
                    .await
                    .map_err(|e| format!("Not possible: {e}"))?;
                Ok(Accepted::new("ack", subject, "Acknowledged"))
            }
            CardButton::Mute1h => {
                let now = self.chat.clock.now();
                let until = now.checked_add(MUTE_BUTTON).unwrap_or(now);
                self.services
                    .incidents
                    .mute(&actor.caller(), incident, until)
                    .await
                    .map_err(|e| format!("Not possible: {e}"))?;
                Ok(Accepted::new("mute", subject, "Muted for an hour")
                    .with_detail(json!({ "minutes": 60 })))
            }
            CardButton::Details => self.details(incident, subject).await,
            CardButton::Approve {
                execution,
                token,
                plan_prefix,
            } => {
                self.approve(actor, incident, *execution, token, plan_prefix.as_deref())
                    .await
            }
            CardButton::Deny { execution, token } => {
                self.deny(actor, incident, *execution, token).await
            }
            CardButton::OpenInWeb { .. } => Err(WEB_ONLY.to_owned()),
        }
    }

    async fn details(&self, incident: IncidentId, subject: String) -> Result<Accepted, String> {
        let timeline = self
            .chat
            .incidents
            .timeline(incident)
            .await
            .map_err(|_| "Something went wrong; try again".to_owned())?;
        let found = self.chat.incidents.get(incident).await.ok().flatten();
        let current = found.map_or(&timeline[..], |i| current_episode(i.check(), &timeline));
        let text = match latest_triage(current) {
            Some(TriageOutcome::Report(r) | TriageOutcome::Cached { report: r, .. }) => {
                details_text(&r)
            }
            _ => "No triage report yet.".to_owned(),
        };
        let accepted = Accepted::new("details", subject, text.clone());
        if self.post_in_thread(incident, &text).await {
            Ok(Accepted::new(
                "details",
                accepted.subject,
                "Posted in the thread",
            ))
        } else {
            Ok(accepted)
        }
    }

    async fn approve(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        execution: ExecutionId,
        token: &str,
        plan_prefix: Option<&str>,
    ) -> Result<Accepted, String> {
        if let Some(target) = self.step_for(incident, execution).await? {
            return self
                .approve_step(actor, incident, target, token, plan_prefix)
                .await;
        }
        Err(STALE_CARD.to_owned())
    }

    async fn deny(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        execution: ExecutionId,
        token: &str,
    ) -> Result<Accepted, String> {
        if let Some(target) = self.step_for(incident, execution).await? {
            return self.deny_step(actor, incident, target, token).await;
        }
        Err(STALE_CARD.to_owned())
    }

    /// The Execution step the button's id names, when it names one. `Ok(None)` means the id
    /// is not a stored execution, so the button is out of date.
    async fn step_for(
        &self,
        incident: IncidentId,
        execution: ExecutionId,
    ) -> Result<Option<StepRef>, String> {
        let Some(target) = self
            .services
            .operations
            .find_by_execution(execution)
            .await
            .map_err(|_| "Something went wrong; try again".to_owned())?
        else {
            return Ok(None);
        };
        if !target.incidents.contains(&incident) {
            return Err("That action no longer exists".to_owned());
        }
        if !self.services.operations.enabled() {
            return Err("Operations are not running; open the incident in the web UI".to_owned());
        }
        Ok(Some(target))
    }

    async fn approve_step(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        target: StepRef,
        token: &str,
        plan_prefix: Option<&str>,
    ) -> Result<Accepted, String> {
        if !matches!(target.risk, Some(Risk::Read | Risk::Write)) {
            return Err(WEB_ONLY.to_owned());
        }
        let binding = prefix_binding(plan_prefix)?;
        self.services
            .operations
            .approve_step(
                &actor.caller(),
                target.operation,
                target.step,
                binding,
                None,
                Some(token),
            )
            .await
            .map_err(|e| command_reason(&e))?;
        Ok(Accepted::new(
            "approve",
            format!("execution:{}", target.step),
            format!("Approved {}", target.skill),
        )
        .with_detail(json!({
            "incident": incident.to_string(),
            "operation": target.operation.to_string(),
        })))
    }

    async fn deny_step(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        target: StepRef,
        token: &str,
    ) -> Result<Accepted, String> {
        self.services
            .operations
            .deny_step(&actor.caller(), target.operation, target.step, Some(token))
            .await
            .map_err(|e| command_reason(&e))?;
        Ok(Accepted::new(
            "deny",
            format!("execution:{}", target.step),
            format!("Denied {}", target.skill),
        )
        .with_detail(json!({
            "incident": incident.to_string(),
            "operation": target.operation.to_string(),
        })))
    }
}
