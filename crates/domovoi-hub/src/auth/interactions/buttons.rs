//! Card buttons: Ack, Mute 1h, Details, and Approve or Deny with the action's single-use
//! token. Destructive actions are refused here, before the approval service is asked.

use domovoi_core::{
    ActionError, ActionId, CardButton, ExecutionError, ExecutionId, ExecutionRecord, IncidentId,
    OperationId, PlanBinding, Risk, StepId, TriageOutcome, TriageReport,
};
use domovoi_engine::ApprovalError;
use domovoi_engine::operations::OperationError;
use jiff::SignedDuration;
use serde_json::json;

use super::{Accepted, Actor, InteractionAuthorizer};
use crate::api::incidents::latest_triage;
use crate::auth::approval::{chat_approval, chat_authorization};

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

/// Why a chat Approve or Deny was refused, in one line for the thread. Chat has
/// nothing to reload, so a row another writer already moved says someone decided
/// it, and a service that is restarting says to try again.
#[must_use]
pub fn approval_reason(err: &ApprovalError) -> String {
    match err {
        ApprovalError::NotFound(_) => "That action no longer exists".to_owned(),
        ApprovalError::BadToken => "This button no longer works".to_owned(),
        ApprovalError::PlanChanged => {
            "This card is out of date; open the incident to review it".to_owned()
        }
        ApprovalError::NotYetSupported => {
            "Policy approvals are not supported yet; a person must approve".to_owned()
        }
        ApprovalError::MissingAuthorization | ApprovalError::InconsistentAuthorization => {
            "This approval is not valid; open the incident to review it".to_owned()
        }
        ApprovalError::Refused(ActionError::ChangedMeanwhile) => {
            "Someone already decided this action".to_owned()
        }
        ApprovalError::Refused(refused) => format!("Not possible: {refused}"),
        ApprovalError::StaleEffect => "This step was taken over; try again".to_owned(),
        ApprovalError::Store(_) => "Something went wrong; try again".to_owned(),
        ApprovalError::Unavailable => {
            "The service that decides actions is restarting; try again".to_owned()
        }
    }
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

/// A button's action id resolved to an Execution step of an operation about the card's
/// incident.
struct StepTarget {
    operation: OperationId,
    step: StepId,
    record: ExecutionRecord,
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
        let deps = self.api.deps();
        match button {
            CardButton::Ack => {
                deps.manager
                    .ack(incident, actor.admin.id)
                    .await
                    .map_err(|e| format!("Not possible: {e}"))?;
                Ok(Accepted::new("ack", subject, "Acknowledged"))
            }
            CardButton::Mute1h => {
                let now = deps.clock.now();
                let until = now.checked_add(MUTE_BUTTON).unwrap_or(now);
                deps.manager
                    .mute(incident, until, actor.admin.id)
                    .await
                    .map_err(|e| format!("Not possible: {e}"))?;
                Ok(Accepted::new("mute", subject, "Muted for an hour")
                    .with_detail(json!({ "minutes": 60 })))
            }
            CardButton::Details => self.details(incident, subject).await,
            CardButton::Approve {
                action,
                token,
                plan_prefix,
            } => {
                self.approve(actor, incident, *action, token, plan_prefix.as_deref())
                    .await
            }
            CardButton::Deny { action, .. } => self.deny(actor, incident, *action).await,
            CardButton::OpenInWeb { .. } => Err(WEB_ONLY.to_owned()),
        }
    }

    async fn details(&self, incident: IncidentId, subject: String) -> Result<Accepted, String> {
        let timeline = self
            .api
            .deps()
            .incidents
            .timeline(incident)
            .await
            .map_err(|_| "Something went wrong; try again".to_owned())?;
        let text = match latest_triage(&timeline) {
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

    async fn load(
        &self,
        incident: IncidentId,
        action: ActionId,
    ) -> Result<domovoi_core::ActionRequest, String> {
        match self.api.deps().actions.get(action).await {
            Ok(Some(found)) if found.incident() == incident => Ok(found),
            Ok(_) => Err("That action no longer exists".to_owned()),
            Err(_) => Err("Something went wrong; try again".to_owned()),
        }
    }

    async fn approve(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        action: ActionId,
        token: &str,
        plan_prefix: Option<&str>,
    ) -> Result<Accepted, String> {
        if let Some(target) = self.step_for(incident, action).await? {
            return self
                .approve_step(actor, incident, target, token, plan_prefix)
                .await;
        }
        let found = self.load(incident, action).await?;
        if found.risk() == Risk::Destructive {
            return Err(WEB_ONLY.to_owned());
        }
        let binding = prefix_binding(plan_prefix)?;
        let deps = self.api.deps();
        let (authorization, provenance) = chat_authorization(
            actor.admin.id,
            actor.platform,
            actor.platform_user,
            deps.clock.now(),
        );
        let approved = deps
            .actions_service
            .approve(action, authorization, Some(token), binding, provenance)
            .await
            .map_err(|e| approval_reason(&e))?;
        Ok(Accepted::new(
            "approve",
            format!("action:{action}"),
            format!("Approved {}", approved.skill()),
        )
        .with_detail(json!({ "incident": incident.to_string() })))
    }

    async fn deny(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        action: ActionId,
    ) -> Result<Accepted, String> {
        if let Some(target) = self.step_for(incident, action).await? {
            return self.deny_step(actor, incident, target).await;
        }
        let found = self.load(incident, action).await?;
        let deps = self.api.deps();
        let approval = chat_approval(
            actor.admin.id,
            actor.platform,
            actor.platform_user,
            deps.clock.now(),
        );
        deps.actions_service
            .deny(found.id(), approval)
            .await
            .map_err(|e| approval_reason(&e))?;
        Ok(Accepted::new(
            "deny",
            format!("action:{action}"),
            format!("Denied {}", found.skill()),
        )
        .with_detail(json!({ "incident": incident.to_string() })))
    }

    /// The Execution step the button's action id names, when it names one. `Ok(None)` means
    /// the id is not an execution and the legacy action path takes over.
    async fn step_for(
        &self,
        incident: IncidentId,
        action: ActionId,
    ) -> Result<Option<StepTarget>, String> {
        let deps = self.api.deps();
        let failed = || "Something went wrong; try again".to_owned();
        let execution = ExecutionId::from_action(action);
        let Some(row) = deps
            .operation_repo
            .execution(execution)
            .await
            .map_err(|_| failed())?
        else {
            return Ok(None);
        };
        let record = row.value;
        let snapshot = deps
            .operation_repo
            .load(record.operation)
            .await
            .map_err(|_| failed())?;
        if !snapshot.is_some_and(|s| s.incidents.contains(&incident)) {
            return Err("That action no longer exists".to_owned());
        }
        if deps.operations.is_none() {
            return Err("Operations are not running; open the incident in the web UI".to_owned());
        }
        Ok(Some(StepTarget {
            operation: record.operation,
            step: record.step,
            record,
        }))
    }

    async fn approve_step(
        &self,
        actor: &Actor<'_>,
        incident: IncidentId,
        target: StepTarget,
        token: &str,
        plan_prefix: Option<&str>,
    ) -> Result<Accepted, String> {
        if !matches!(target.record.risk(), Some(Risk::Read | Risk::Write)) {
            return Err(WEB_ONLY.to_owned());
        }
        let binding = prefix_binding(plan_prefix)?;
        let deps = self.api.deps();
        let Some(operations) = &deps.operations else {
            return Err("Operations are not running; open the incident in the web UI".to_owned());
        };
        let (authorization, provenance) = chat_authorization(
            actor.admin.id,
            actor.platform,
            actor.platform_user,
            deps.clock.now(),
        );
        operations
            .authorize(
                target.operation,
                target.step,
                authorization,
                binding,
                provenance,
                Some(token),
            )
            .await
            .map_err(|e| step_reason(&e))?;
        Ok(Accepted::new(
            "approve",
            format!("execution:{}", target.step),
            format!("Approved {}", skill_of(&target.record)),
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
        target: StepTarget,
    ) -> Result<Accepted, String> {
        let deps = self.api.deps();
        let Some(operations) = &deps.operations else {
            return Err("Operations are not running; open the incident in the web UI".to_owned());
        };
        let approval = chat_approval(
            actor.admin.id,
            actor.platform,
            actor.platform_user,
            deps.clock.now(),
        );
        operations
            .deny(target.operation, target.step, approval)
            .await
            .map_err(|e| step_reason(&e))?;
        Ok(Accepted::new(
            "deny",
            format!("execution:{}", target.step),
            format!("Denied {}", skill_of(&target.record)),
        )
        .with_detail(json!({
            "incident": incident.to_string(),
            "operation": target.operation.to_string(),
        })))
    }
}

/// The skill's name, for the reply.
fn skill_of(record: &ExecutionRecord) -> String {
    record
        .contract
        .skill_name()
        .map_or_else(|| record.contract.to_string(), |name| name.to_string())
}
