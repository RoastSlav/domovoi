//! Whether an approved action is still worth dispatching.
//!
//! An approval can wait: a hub may restart while the host's agent is gone, and the agent
//! may take a while to come back. By the time the action would reach the agent, the
//! approval's window may have closed or the incident it was approved for may be gone or
//! already resolved. Dispatching then would run a skill nobody can still want, so the
//! executor and the writer both ask this module first. A store that cannot answer is an
//! error, not a pass: the action stays approved and the next attempt asks again.

use domovoi_core::ports::StoreError;
use domovoi_core::{ActionRequest, IncidentStatus};
use jiff::Timestamp;

use super::Shared;

/// Why an approved action must not be dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stale {
    /// The approval's window closed while the action waited.
    Expired,
    /// The incident the action belongs to is no longer in the store.
    IncidentGone,
    /// The incident was resolved while the action waited.
    IncidentResolved,
}

impl Stale {
    /// The sentence the person is shown when the action is rejected for this reason.
    pub(crate) const fn reason(self) -> &'static str {
        match self {
            Stale::Expired => "the approval expired before the action was dispatched",
            Stale::IncidentGone => "the incident no longer exists",
            Stale::IncidentResolved => "the incident was resolved before the action was dispatched",
        }
    }
}

/// Why `action` must not be dispatched at `now`, or `None` while it still is.
///
/// The incident is read rather than remembered, so an incident resolved by a signal
/// while the hub was down is seen here too.
pub(crate) async fn check(
    shared: &Shared,
    action: &ActionRequest,
    now: Timestamp,
) -> Result<Option<Stale>, StoreError> {
    if now >= action.expires_at() {
        return Ok(Some(Stale::Expired));
    }
    match shared.deps.incidents.get(action.incident()).await? {
        None => Ok(Some(Stale::IncidentGone)),
        Some(incident) if incident.status() == IncidentStatus::Resolved => {
            Ok(Some(Stale::IncidentResolved))
        }
        Some(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};

    use domovoi_core::ports::{ActionRepo, AuditFilter, AuditLog, IncidentRepo, Page};
    use domovoi_core::{
        ActionRequest, ActionStatus, HostId, IncidentEvent, IncidentPolicy, PlanHash, Principal,
        Risk, UserId, apply,
    };
    use domovoi_testkit::approvals::human;
    use domovoi_testkit::build::{T0, manifest, open_incident};
    use domovoi_testkit::{FakeAgentLink, FakeClock, InMemoryStore};
    use jiff::SignedDuration;
    use tokio::sync::{broadcast, oneshot, watch};
    use tokio_util::sync::CancellationToken;

    use crate::EnginePolicy;
    use crate::PatternRedactor;
    use crate::actions::executor::MAX_CATCH_UP_RUNS;
    use crate::actions::writer::{self, WriteCommand};
    use crate::actions::{ActionDeps, ActionSettings, Shared};
    use crate::manager::{IncidentManager, ManagerDeps};

    /// A world for the writer alone: the store, the clock and the fake agent, the writer's
    /// queue, and no executor at all. The writer loop is the only thing reading commands,
    /// so a dispatch that is refused here was refused by the writer's own check.
    struct Writer {
        shared: Arc<Shared>,
        store: Arc<InMemoryStore>,
        clock: Arc<FakeClock>,
        link: Arc<FakeAgentLink>,
        host: HostId,
        commands: writer::WriterHandle,
    }

    impl Writer {
        async fn start() -> Writer {
            let store = Arc::new(InMemoryStore::new());
            let clock = Arc::new(FakeClock::new(T0));
            let link = Arc::new(FakeAgentLink::new());
            let (bus, _events) = broadcast::channel(16);
            let shutdown = CancellationToken::new();
            let (manager, _manager_task) = IncidentManager::spawn(ManagerDeps {
                incidents: store.clone(),
                uow: store.clone(),
                versions: store.clone(),
                acks: store.clone(),
                clock: clock.clone(),
                redactor: Arc::new(PatternRedactor::default()),
                policy: EnginePolicy::default(),
                bus: bus.clone(),
                shutdown: shutdown.clone(),
            });
            let (commands, incoming) = writer::queue();
            let (recovered, _recovered_rx) = watch::channel(false);
            let shared = Arc::new(Shared {
                deps: ActionDeps {
                    actions: store.clone(),
                    incidents: store.clone(),
                    audit: store.clone(),
                    uow: store.clone(),
                    link: link.clone(),
                    redactor: Arc::new(PatternRedactor::default()),
                    clock: clock.clone(),
                    manager,
                    bus,
                    shutdown,
                },
                settings: ActionSettings::default(),
                dispatches: Mutex::new(BTreeMap::new()),
                running: Mutex::new(BTreeSet::new()),
                writer: commands.clone(),
                recovered,
                catch_up_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CATCH_UP_RUNS)),
            });
            tokio::spawn(writer::run(shared.clone(), incoming));
            Writer {
                shared,
                store,
                clock,
                link,
                host: HostId::new(),
                commands,
            }
        }

        /// Stores an incident on this host and one approved `restart_container` for it,
        /// returning the action's id.
        async fn approved(&self) -> (domovoi_core::ActionId, PlanHash) {
            let incident = open_incident(self.host, "docker.container", Some("nginx")).unwrap();
            IncidentRepo::insert(&*self.store, &incident).await.unwrap();
            let skill = manifest("restart_container", Risk::Write)
                .unwrap()
                .summary();
            let args = skill.validate_args(&BTreeMap::new()).unwrap();
            let mut action = ActionRequest::propose(
                incident.id(),
                self.host,
                &skill,
                &args,
                Principal::System("triage".into()),
                SignedDuration::from_hours(1),
                T0,
            );
            action.approve(human(UserId::new()), T0).unwrap();
            ActionRepo::insert(&*self.store, &action).await.unwrap();
            let plan = PlanHash::compute(&skill.name, skill.manifest_hash, self.host, &args);
            (action.id(), plan)
        }

        /// Sends a `Dispatch` command to the writer and answers with what it decided.
        async fn dispatch(&self, id: domovoi_core::ActionId, plan: PlanHash) -> ActionRequest {
            let (reply, answer) = oneshot::channel();
            self.commands
                .tx
                .send(WriteCommand::Dispatch { id, plan, reply })
                .await
                .unwrap();
            answer.await.unwrap().expect("the writer answered")
        }

        /// The audit kinds recorded for `id`, oldest first.
        async fn trail(&self, id: domovoi_core::ActionId) -> Vec<String> {
            let filter = AuditFilter {
                subject: Some(format!("action:{id}")),
                ..AuditFilter::default()
            };
            let mut kinds: Vec<String> = AuditLog::list(&*self.store, &filter, &Page::first(100))
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|entry| entry.kind)
                .collect();
            kinds.reverse();
            kinds
        }
    }

    #[tokio::test]
    async fn writer_refuses_an_approval_that_expired_before_dispatch() {
        let writer = Writer::start().await;
        let (id, plan) = writer.approved().await;
        // The window closes while the dispatch waits for the writer.
        writer.clock.advance(SignedDuration::from_hours(2));

        let decided = writer.dispatch(id, plan).await;
        assert_eq!(
            decided.status(),
            &ActionStatus::Rejected {
                reason: "the approval expired before the action was dispatched".to_owned(),
            },
            "the writer itself refuses a stale approval, without the executor's help",
        );
        let stored = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
        assert_eq!(
            stored.status(),
            decided.status(),
            "the refusal reached the store",
        );
        let trail = writer.trail(id).await;
        assert!(
            trail.contains(&"action.rejected".to_owned()),
            "the refusal is audited as a rejection: {trail:?}",
        );
        assert!(
            !trail.contains(&"action.dispatched".to_owned()),
            "nothing was dispatched: {trail:?}",
        );
        assert!(
            writer.link.requests().is_empty(),
            "no request reached the agent",
        );
        writer.shared.deps.shutdown.cancel();
    }

    #[tokio::test]
    async fn writer_refuses_an_approval_whose_incident_resolved_before_dispatch() {
        let writer = Writer::start().await;
        let (id, plan) = writer.approved().await;
        let stored = ActionRepo::get(&*writer.store, id).await.unwrap().unwrap();
        let mut incident = IncidentRepo::get(&*writer.store, stored.incident())
            .await
            .unwrap()
            .unwrap();
        apply(
            &mut incident,
            IncidentEvent::Resolved { observed_at: T0 },
            &IncidentPolicy::default(),
            T0,
        )
        .unwrap();
        IncidentRepo::update(&*writer.store, &incident)
            .await
            .unwrap();

        let decided = writer.dispatch(id, plan).await;
        assert_eq!(
            decided.status(),
            &ActionStatus::Rejected {
                reason: "the incident was resolved before the action was dispatched".to_owned(),
            },
            "an incident that closed while the dispatch waited stops it",
        );
        let trail = writer.trail(id).await;
        assert!(
            !trail.contains(&"action.dispatched".to_owned()),
            "nothing was dispatched: {trail:?}",
        );
        writer.shared.deps.shutdown.cancel();
    }
}
