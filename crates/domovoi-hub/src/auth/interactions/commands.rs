//! Chat commands: `/status`, `/incidents`, `/mute <incident> <1h|4h|24h>` and `/digest`,
//! answered through the same services as the API.

use domovoi_chat::discord::ids::short_id;
use domovoi_core::ports::{IncidentFilter, OutboundMessage, Page};
use domovoi_core::{Incident, IncidentId};
use jiff::SignedDuration;
use serde_json::json;

use super::{Accepted, Actor, InteractionAuthorizer};
use crate::api::{hosts, system};

/// The most incidents `/incidents` lists.
pub const LIST_MAX: usize = 10;

/// What `/mute` accepts.
fn mute_duration(text: &str) -> Option<SignedDuration> {
    match text {
        "1h" => Some(SignedDuration::from_hours(1)),
        "4h" => Some(SignedDuration::from_hours(4)),
        "24h" => Some(SignedDuration::from_hours(24)),
        _ => None,
    }
}

const TRY_AGAIN: &str = "Something went wrong; try again";

impl InteractionAuthorizer {
    async fn active(&self) -> Result<Vec<Incident>, String> {
        let filter = IncidentFilter {
            active: Some(true),
            ..IncidentFilter::default()
        };
        self.api
            .deps()
            .incidents
            .list(&filter, &Page::first(Page::MAX_LIMIT))
            .await
            .map(|page| page.items)
            .map_err(|_| TRY_AGAIN.to_owned())
    }

    fn incident_url(&self, incident: IncidentId) -> String {
        let base = self
            .api
            .auth()
            .web()
            .public_url
            .as_str()
            .trim_end_matches('/')
            .to_owned();
        format!("{base}/incidents/{incident}")
    }

    pub(super) async fn command(
        &self,
        name: &str,
        args: &[String],
        actor: &Actor<'_>,
    ) -> Result<Accepted, String> {
        let subject = format!("command:{name}");
        match name {
            "status" => self.status(subject).await,
            "incidents" => self.incidents(subject).await,
            "mute" => self.mute(actor, args).await,
            "digest" => self.digest(subject).await,
            _ => Err(format!("Unknown command /{name}")),
        }
    }

    async fn status(&self, subject: String) -> Result<Accepted, String> {
        let deps = self.api.deps();
        let all = deps
            .hosts
            .list_hosts()
            .await
            .map_err(|_| TRY_AGAIN.to_owned())?;
        if all.is_empty() {
            return Ok(Accepted::new("status", subject, "No hosts yet."));
        }
        let mut lines = Vec::with_capacity(all.len());
        for host in &all {
            let dto = hosts::host_dto(&self.api, host)
                .await
                .map_err(|_| TRY_AGAIN.to_owned())?;
            let agent = match (dto.connected, &dto.version) {
                (true, Some(v)) => format!("connected, agent {v}"),
                (true, None) => "connected".to_owned(),
                (false, _) => "offline".to_owned(),
            };
            let open = match dto.open_incidents {
                0 => "no open incidents".to_owned(),
                1 => "1 open incident".to_owned(),
                n => format!("{n} open incidents"),
            };
            lines.push(format!("• **{}**: {agent}, {open}", dto.name));
        }
        Ok(Accepted::new("status", subject, lines.join("\n")))
    }

    async fn incidents(&self, subject: String) -> Result<Accepted, String> {
        let active = self.active().await?;
        if active.is_empty() {
            return Ok(Accepted::new("incidents", subject, "No open incidents."));
        }
        let mut lines: Vec<String> = active
            .iter()
            .take(LIST_MAX)
            .map(|i| {
                format!(
                    "• `{}` {} {} — <{}>",
                    short_id(i.id()),
                    i.severity().to_string().to_uppercase(),
                    i.title(),
                    self.incident_url(i.id())
                )
            })
            .collect();
        if active.len() > LIST_MAX {
            lines.push(format!(
                "…and {} more in the web UI.",
                active.len() - LIST_MAX
            ));
        }
        Ok(Accepted::new("incidents", subject, lines.join("\n")))
    }

    async fn mute(&self, actor: &Actor<'_>, args: &[String]) -> Result<Accepted, String> {
        let [short, duration] = args else {
            return Err("Use /mute incident:<id> duration:<1h|4h|24h>".to_owned());
        };
        let duration = mute_duration(duration).ok_or("The duration must be 1h, 4h or 24h")?;
        let wanted = short.trim().to_ascii_lowercase();
        let matches: Vec<IncidentId> = self
            .active()
            .await?
            .iter()
            .map(Incident::id)
            .filter(|id| short_id(*id) == wanted || id.to_string() == wanted)
            .collect();
        let incident = match matches.as_slice() {
            [one] => *one,
            [] => return Err(format!("No open incident {wanted}")),
            _ => {
                return Err(format!(
                    "{wanted} matches several incidents; use the full id"
                ));
            }
        };
        let deps = self.api.deps();
        let now = deps.clock.now();
        let until = now.checked_add(duration).unwrap_or(now);
        deps.manager
            .mute(incident, until, actor.admin.id)
            .await
            .map_err(|e| format!("Not possible: {e}"))?;
        Ok(Accepted::new(
            "mute",
            format!("incident:{incident}"),
            format!("Muted {wanted} for {}", args[1]),
        )
        .with_detail(json!({ "minutes": duration.as_mins() })))
    }

    async fn digest(&self, subject: String) -> Result<Accepted, String> {
        let notifier = self
            .api
            .deps()
            .notifier
            .clone()
            .ok_or("No chat is configured to send the digest to")?;
        let digest = system::build(&self.api)
            .await
            .map_err(|_| TRY_AGAIN.to_owned())?;
        let message = OutboundMessage {
            title: format!("Daily digest for {}", digest.date),
            markdown: digest.markdown,
            severity: None,
        };
        notifier
            .send(&message)
            .await
            .map_err(|e| format!("Sending failed: {e}"))?;
        Ok(Accepted::new("digest", subject, "Digest sent"))
    }
}
