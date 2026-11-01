//! Card buttons: Ack, Mute 1h, Details, and Approve or Deny with the action's single-use
//! token. Destructive actions are refused here, before the approval service is asked.

use domovoi_core::{ActionId, CardButton, IncidentId, Risk, TriageOutcome, TriageReport};
use domovoi_engine::ApprovalError;
use jiff::SignedDuration;
use serde_json::json;

use super::{Accepted, Actor, InteractionAuthorizer};
use crate::api::incidents::latest_triage;
use crate::auth::approval::chat_approval;

/// How long the Mute 1h button mutes.
pub const MUTE_BUTTON: SignedDuration = SignedDuration::from_hours(1);

/// What a destructive action's Approve says (INV-6).
pub const WEB_ONLY: &str = "Approve this one in the web UI";

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

fn approval_reason(err: &ApprovalError) -> String {
    match err {
        ApprovalError::NotFound(_) => "That action no longer exists".to_owned(),
        ApprovalError::BadToken => "This button no longer works".to_owned(),
        ApprovalError::Refused(refused) => format!("Not possible: {refused}"),
        ApprovalError::Store(_) => "Something went wrong; try again".to_owned(),
    }
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
            CardButton::Approve { action, token } => {
                self.approve(actor, incident, *action, token).await
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
    ) -> Result<Accepted, String> {
        let found = self.load(incident, action).await?;
        if found.risk() == Risk::Destructive {
            return Err(WEB_ONLY.to_owned());
        }
        let deps = self.api.deps();
        let approval = chat_approval(
            actor.admin.id,
            actor.platform,
            actor.platform_user,
            deps.clock.now(),
        );
        let approved = deps
            .actions_service
            .approve(action, approval, Some(token))
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
}
