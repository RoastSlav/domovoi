//! What changed on the host in the last day: reboots and container events.

use std::fmt::Write as _;

use async_trait::async_trait;
use domovoi_core::ReadToolCall;
use domovoi_core::context::{
    ContextBatch, ContextLayer, ContextQuery, ContextSelector, ContextSourceId, ItemStatus, Origin,
};
use domovoi_core::ports::{ContextSource, IncidentFilter, Page, Presence};
use jiff::SignedDuration;

use super::{Maker, Spec, host_of};
use crate::context::EvidenceSource;
use crate::triage::TriageDeps;

/// How long reading the host's container events may take.
const EVENTS_DEADLINE: SignedDuration = SignedDuration::from_secs(2);

/// Reboots of the last 24 hours from the incident list, and the host's container events from
/// one read through the evidence boundary. A host whose agent is not connected has unknown
/// container events, not none.
#[derive(Clone)]
pub(crate) struct ChangesSource {
    deps: TriageDeps,
}

impl ChangesSource {
    /// A source over the incident repository and the evidence boundary.
    pub(crate) fn new(deps: TriageDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl ContextSource for ChangesSource {
    fn id(&self) -> ContextSourceId {
        super::source_id("changes")
    }

    fn layer(&self) -> ContextLayer {
        ContextLayer::Evidence
    }

    fn supports(&self, selector: &ContextSelector) -> bool {
        matches!(selector, ContextSelector::Overview)
    }

    async fn fetch(&self, query: &ContextQuery) -> ContextBatch {
        let deps = &self.deps;
        let maker = Maker::new(
            "changes",
            ContextLayer::Evidence,
            deps.redactor.as_ref(),
            query,
        );
        let now = deps.clock.now();
        if maker.expired(now) {
            return maker.gap_batch("timed out");
        }
        let host = host_of(query);
        let mut batch = ContextBatch::default();
        let day_ago = now
            .checked_sub(SignedDuration::from_hours(24))
            .unwrap_or(now);
        let filter = IncidentFilter {
            host: Some(host),
            opened_since: Some(day_ago),
            ..IncidentFilter::default()
        };
        match deps.incidents.list(&filter, &Page::first(50)).await {
            Err(_) => batch
                .gaps
                .push(maker.gap("recent incidents could not be read, so reboots are unknown")),
            Ok(recent) => {
                let reboots: Vec<_> = recent
                    .items
                    .iter()
                    .filter(|i| i.check().as_str() == "system.reboot")
                    .collect();
                if let Some(newest) = reboots.iter().max_by_key(|i| i.opened_at()) {
                    let mut body = String::new();
                    for reboot in &reboots {
                        let _ =
                            writeln!(body, "- The host rebooted around {}.", reboot.opened_at());
                    }
                    batch.items.push(maker.item(Spec {
                        key: "reboots",
                        title: "Reboots",
                        body: &body,
                        status: ItemStatus::Known,
                        origin: Origin::Observed,
                        reference: &format!("incident:{}", newest.id()),
                        observed_at: Some(newest.opened_at()),
                    }));
                }
            }
        }
        const EVENTS_REFERENCE: &str = "docker_events 24h";
        if !matches!(deps.catalog.presence(host), Presence::Connected { .. }) {
            batch.items.push(maker.unknown(
                "docker_events",
                "Container events (24 h)",
                "the host's agent is not connected, so its container events of the last 24 hours were not read",
                EVENTS_REFERENCE,
            ));
            return batch;
        }
        let deadline = now
            .checked_add(EVENTS_DEADLINE)
            .map_or(query.deadline, |at| at.min(query.deadline));
        let read = ContextQuery {
            scope: query.scope.clone(),
            selector: ContextSelector::Read {
                call: ReadToolCall::DockerEvents {
                    container: None,
                    since_minutes: 24 * 60,
                },
            },
            limit: 1,
            max_item_bytes: query.max_item_bytes,
            deadline,
        };
        let answer = EvidenceSource::new(deps.clone()).fetch(&read).await;
        for gap in answer.gaps {
            batch.gaps.push(maker.gap(&gap.reason));
        }
        for item in answer.items {
            match item.status {
                ItemStatus::Known if item.body.trim().is_empty() => {}
                ItemStatus::Known => batch.items.push(maker.item(Spec {
                    key: "docker_events",
                    title: "Container events (24 h)",
                    body: &format!("Docker events:\n{}", item.body.trim()),
                    status: ItemStatus::Known,
                    origin: Origin::Observed,
                    reference: EVENTS_REFERENCE,
                    observed_at: item.provenance.observed_at,
                })),
                ItemStatus::Unknown { reason } => batch.items.push(maker.unknown(
                    "docker_events",
                    "Container events (24 h)",
                    &reason,
                    EVENTS_REFERENCE,
                )),
                other => batch.items.push(maker.item(Spec {
                    key: "docker_events",
                    title: "Container events (24 h)",
                    body: &item.body,
                    status: other,
                    origin: Origin::Observed,
                    reference: EVENTS_REFERENCE,
                    observed_at: item.provenance.observed_at,
                })),
            }
        }
        batch
    }
}
