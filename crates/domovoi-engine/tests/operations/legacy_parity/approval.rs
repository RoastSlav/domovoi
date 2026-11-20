//! Approval safety on the operation path: one decision per step, expiry that never undoes a
//! decision, and the dispatch checks that depend on the approval window and the incident.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionAnswer, AggregateRef, EffectDraft, EffectKind, EffectStatus, IncidentFilter,
    IncidentRepo, IncidentTimelineEntry, Page, Paged, StoreError, Transaction,
};
use domovoi_core::{
    EffectId, ExecutionError, ExecutionId, Fingerprint, HostId, Incident, IncidentId, OperationId,
    StepId, StepOutcome,
};
use domovoi_engine::operations::{OperationDeps, OperationError, OperationSeed, OperationService};
use domovoi_testkit::approvals;
use domovoi_testkit::build::{incident, open_incident};
use domovoi_testkit::link::{action_caps, evidence_caps, observer_of};
use serde_json::json;

use crate::harness::{World, host_id, restart_skill, seed, world, world_gated};

/// The default approval window, in seconds, plus one.
const PAST_WINDOW: i64 = 24 * 3600 + 1;

/// The reason an expired approval carries.
const EXPIRED: &str = "the approval expired before the action was dispatched";

/// Lets the runner and the writer work for a while, a second of fake time per round.
async fn drain(w: &World) {
    for _ in 0..60 {
        w.settle().await;
        w.advance(1);
    }
    w.settle().await;
}

/// A person denies `step`.
async fn deny(w: &World, op: OperationId, step: StepId) -> Result<(), OperationError> {
    w.handle().deny(op, step, approvals::human(w.user)).await
}

/// The reason stored on the step.
async fn reason(w: &World, op: OperationId, step: StepId) -> String {
    w.load(op)
        .await
        .step(step)
        .unwrap()
        .progress
        .reason
        .clone()
        .unwrap_or_default()
}

/// Whether the execution row of `step` holds a person's approval.
async fn has_approval(w: &World, op: OperationId, step: StepId) -> bool {
    w.load(op)
        .await
        .execution_of(step)
        .unwrap()
        .value
        .approval()
        .is_some()
}

/// Whether the audit log has an event of `kind`.
async fn audited(w: &World, kind: &str) -> bool {
    w.audit_kinds().await.iter().any(|seen| seen == kind)
}

/// The approval-expiry wakes of `op` that have not run yet.
async fn expiry_wakes(w: &World, op: OperationId) -> Vec<domovoi_core::ports::EffectRecord> {
    let mut wakes = Vec::new();
    for status in [EffectStatus::Pending, EffectStatus::Claimed] {
        wakes.extend(w.effects(op, status).await.into_iter().filter(|effect| {
            effect.kind == "wake" && effect.payload["purpose"] == "approval_expiry"
        }));
    }
    wakes
}

/// Queues a wake of `step` for the approval expiry, due now, as a duplicate delivery would.
async fn early_expiry_wake(w: &World, op: OperationId, step: StepId) -> EffectId {
    let now = w.now();
    let draft = EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Wake,
        aggregate: AggregateRef::operation(op),
        payload: json!({
            "v": 1,
            "operation": op.to_string(),
            "purpose": "approval_expiry",
            "step": step.to_string(),
        }),
        not_before: now,
        max_attempts: 5,
        dedup_key: None,
        created_at: now,
    };
    let id = draft.id;
    w.uow
        .commit(Transaction::new().effect(draft))
        .await
        .unwrap();
    id
}

/// An operation about `incident`, which must already be stored.
async fn operation_about(w: &World, incident: &Incident) -> OperationId {
    let seeded = OperationSeed {
        incidents: vec![incident.id()],
        ..seed()
    };
    w.handle().create(seeded).await.unwrap()
}

/// Resolves `incident` in the store, as a cleared check does.
async fn resolve(w: &World, incident: &Incident) {
    let mut found = IncidentRepo::get(&*w.store, incident.id())
        .await
        .unwrap()
        .unwrap();
    domovoi_core::apply(
        &mut found,
        domovoi_core::IncidentEvent::Resolved {
            observed_at: w.now(),
        },
        &domovoi_core::IncidentPolicy::default(),
        w.now(),
    )
    .unwrap();
    IncidentRepo::update(&*w.store, &found).await.unwrap();
}

#[tokio::test]
async fn approve_and_deny_at_once_leave_exactly_one_outcome_on_a_step() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;

    let (approved, denied) = tokio::join!(w.approve(op, step), deny(&w, op, step));

    assert!(
        approved.is_ok() != denied.is_ok(),
        "exactly one decision wins: approve {approved:?}, deny {denied:?}"
    );
    let audit = w.audit_kinds().await;
    assert!(
        !(audit.contains(&"execution.approved".to_owned())
            && audit.contains(&"execution.denied".to_owned())),
        "both decisions were recorded: {audit:?}"
    );
    w.queue.open();
    if approved.is_ok() {
        w.until_outcome(op, step, StepOutcome::Succeeded).await;
        assert_eq!(
            w.skill_requests().len(),
            1,
            "the approved run went out once"
        );
        assert!(audit.contains(&"execution.approved".to_owned()));
    } else {
        drain(&w).await;
        assert_eq!(w.outcome(op, step).await, StepOutcome::Refused);
        assert!(w.skill_requests().is_empty(), "a denied run is never sent");
        assert!(audit.contains(&"execution.denied".to_owned()));
    }
}

#[tokio::test]
async fn a_second_decision_on_a_decided_step_is_refused_in_either_order() {
    // Approved first: the denial changes nothing.
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    let late_deny = deny(&w, op, step).await;

    assert!(late_deny.is_err(), "a decided step cannot be denied");
    assert_eq!(w.outcome(op, step).await, StepOutcome::Pending);
    assert!(has_approval(&w, op, step).await, "the approval stands");
    assert!(!audited(&w, "execution.denied").await);
    w.queue.open();
    w.until_outcome(op, step, StepOutcome::Succeeded).await;

    // Denied first: the approval changes nothing.
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    deny(&w, op, step).await.unwrap();

    let late_approve = w.approve(op, step).await;

    assert!(late_approve.is_err(), "a denied step cannot be approved");
    assert_eq!(w.outcome(op, step).await, StepOutcome::Refused);
    assert!(!has_approval(&w, op, step).await, "no approval was stored");
    assert!(!audited(&w, "execution.approved").await);
    w.queue.open();
    drain(&w).await;
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn expiry_never_overwrites_a_step_approval() {
    // (a) Approved, then the window passes and the expiry wake runs.
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.advance(PAST_WINDOW);
    w.queue.open();
    w.until_outcome(op, step, StepOutcome::NotRun).await;
    drain(&w).await;
    assert!(
        expiry_wakes(&w, op).await.is_empty(),
        "the wake ran and finished"
    );
    assert!(
        !audited(&w, "execution.expired").await,
        "the wake did not settle an approved step; the dispatch did"
    );
    assert!(audited(&w, "execution.not_run").await);

    // (b) Denied, then the wake runs: the refusal stands.
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    deny(&w, op, step).await.unwrap();
    w.advance(PAST_WINDOW);
    w.queue.open();
    drain(&w).await;
    assert!(expiry_wakes(&w, op).await.is_empty());
    assert_eq!(w.outcome(op, step).await, StepOutcome::Refused);
    assert_eq!(reason(&w, op, step).await, "denied by a person");
    assert!(!audited(&w, "execution.expired").await);

    // (c) Already running, then the wake runs: nothing changes.
    let w = world().await;
    w.link.on_skill(|_, _| {
        Err(domovoi_core::ports::LinkError::Indeterminate(
            "link dropped".to_owned(),
        ))
    });
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.until_outcome(op, step, StepOutcome::Running).await;
    // The agent still has the run, so a reconcile leaves it running.
    w.link.answer(
        host_id(),
        ExecutionId::from_step(step).as_action(),
        ActionAnswer::Running,
    );
    w.queue.close();
    w.advance(PAST_WINDOW);
    let early = early_expiry_wake(&w, op, step).await;
    w.queue.open();
    drain(&w).await;
    assert_eq!(w.outcome(op, step).await, StepOutcome::Running);
    assert!(!audited(&w, "execution.expired").await);
    assert!(
        expiry_wakes(&w, op)
            .await
            .iter()
            .all(|wake| wake.id != early),
        "the wake was consumed"
    );
}

#[tokio::test]
async fn expiry_settles_only_steps_whose_window_closed() {
    let w = world().await;
    let first = w.create().await;
    let early_step = w.propose(first).await;
    w.advance(3600);
    let second = w.create().await;
    let later_step = w.propose(second).await;
    let first_expires = w
        .load(first)
        .await
        .execution_of(early_step)
        .unwrap()
        .value
        .expires_at;
    let second_expires = w
        .load(second)
        .await
        .execution_of(later_step)
        .unwrap()
        .value
        .expires_at;
    assert!(second_expires > first_expires);

    // A wake that arrives before the window closed only looks again at the close.
    let stray = early_expiry_wake(&w, first, early_step).await;
    drain(&w).await;

    assert_eq!(w.outcome(first, early_step).await, StepOutcome::Pending);
    assert_eq!(w.outcome(second, later_step).await, StepOutcome::Pending);
    let wakes = expiry_wakes(&w, first).await;
    assert!(!wakes.is_empty());
    assert!(wakes.iter().all(|wake| wake.id != stray));
    assert!(
        wakes.iter().all(|wake| wake.not_before == first_expires),
        "every remaining wake is due at the close of the window"
    );
    let others = expiry_wakes(&w, second).await;
    assert!(others.iter().all(|wake| wake.not_before == second_expires));

    // Once the first window is closed, only the first step expires.
    let to_first_close = first_expires.duration_since(w.now()).as_secs() + 1;
    w.advance(to_first_close);
    w.until_outcome(first, early_step, StepOutcome::NotRun)
        .await;
    assert_eq!(w.outcome(second, later_step).await, StepOutcome::Pending);
    assert!(w.now() < second_expires);
}

#[tokio::test]
async fn proposed_step_expiry_is_final() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;

    w.advance(PAST_WINDOW);
    w.until_outcome(op, step, StepOutcome::NotRun).await;

    assert_eq!(reason(&w, op, step).await, EXPIRED);
    assert!(audited(&w, "execution.expired").await);
    assert!(w.skill_requests().is_empty());
    let late = w.approve(op, step).await;
    assert!(late.is_err(), "an expired step cannot be approved");
    assert_eq!(w.outcome(op, step).await, StepOutcome::NotRun);
    assert!(!has_approval(&w, op, step).await);
    assert!(!audited(&w, "execution.approved").await);
}

#[tokio::test]
async fn approved_step_whose_window_closed_is_not_run() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    w.advance(PAST_WINDOW);
    w.queue.open();
    w.until_outcome(op, step, StepOutcome::NotRun).await;

    assert_eq!(reason(&w, op, step).await, EXPIRED);
    assert!(w.skill_requests().is_empty());
    assert!(audited(&w, "execution.not_run").await);
}

#[tokio::test]
async fn authorize_after_the_window_is_refused() {
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    // The window closed and the expiry wake has not run yet.
    w.advance(PAST_WINDOW);

    let late = w.approve(op, step).await;

    assert!(
        matches!(
            late,
            Err(OperationError::Execution(ExecutionError::Expired))
        ),
        "got {late:?}"
    );
    assert_eq!(w.outcome(op, step).await, StepOutcome::Pending);
    assert!(!has_approval(&w, op, step).await);
    assert!(!audited(&w, "execution.approved").await);
    assert!(
        w.effects(op, EffectStatus::Pending)
            .await
            .iter()
            .all(|effect| effect.kind != "dispatch"),
        "nothing was queued for dispatch"
    );
}

#[tokio::test]
async fn dispatch_refused_when_the_incident_resolved_before_dispatch() {
    let w = world_gated().await;
    let open = open_incident(host_id(), "docker.container", Some("plex")).unwrap();
    IncidentRepo::insert(&*w.store, &open).await.unwrap();
    let op = operation_about(&w, &open).await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    resolve(&w, &open).await;

    w.queue.open();
    w.until_outcome(op, step, StepOutcome::Refused).await;

    assert_eq!(
        reason(&w, op, step).await,
        "the incident was resolved before the action was dispatched"
    );
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn dispatch_refused_when_the_incident_no_longer_exists() {
    let w = world_gated().await;
    let pending = incident(host_id(), "docker.container", Some("plex")).unwrap();
    IncidentRepo::insert(&*w.store, &pending).await.unwrap();
    let op = operation_about(&w, &pending).await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    IncidentRepo::discard_pending(&*w.store, pending.id())
        .await
        .unwrap();

    w.queue.open();
    w.until_outcome(op, step, StepOutcome::Refused).await;

    assert_eq!(reason(&w, op, step).await, "the incident no longer exists");
    assert!(w.skill_requests().is_empty());
}

#[tokio::test]
async fn an_expiry_wins_over_a_refusal() {
    // The plan changed and the window closed: the window is the reason.
    let w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    let mut changed = restart_skill();
    changed.entry_script_digest = domovoi_core::ScriptDigest::of_bytes(b"another entry");
    w.link.set_skills(host_id(), vec![changed]);
    w.advance(PAST_WINDOW);
    w.queue.open();
    w.until_outcome(op, step, StepOutcome::NotRun).await;
    assert_eq!(reason(&w, op, step).await, EXPIRED);
    assert!(!audited(&w, "execution.refused").await);
    assert!(w.skill_requests().is_empty());

    // The incident resolved and the window closed: the window is the reason.
    let w = world_gated().await;
    let open = open_incident(host_id(), "docker.container", Some("plex")).unwrap();
    IncidentRepo::insert(&*w.store, &open).await.unwrap();
    let op = operation_about(&w, &open).await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    resolve(&w, &open).await;
    w.advance(PAST_WINDOW);
    w.queue.open();
    w.until_outcome(op, step, StepOutcome::NotRun).await;
    assert_eq!(reason(&w, op, step).await, EXPIRED);
    assert!(!audited(&w, "execution.refused").await);
    assert!(w.skill_requests().is_empty());
}

/// Incident storage that fails a number of reads, then recovers.
struct FlakyIncidents {
    inner: Arc<dyn IncidentRepo>,
    failures: AtomicU32,
    reads: AtomicU32,
}

#[async_trait]
impl IncidentRepo for FlakyIncidents {
    async fn insert(&self, incident: &Incident) -> Result<(), StoreError> {
        self.inner.insert(incident).await
    }

    async fn update(&self, incident: &Incident) -> Result<(), StoreError> {
        self.inner.update(incident).await
    }

    async fn get(&self, id: IncidentId) -> Result<Option<Incident>, StoreError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let failing = self
            .failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if failing {
            return Err(StoreError::Backend("the store is away".to_owned()));
        }
        self.inner.get(id).await
    }

    async fn find_active_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
    ) -> Result<Option<Incident>, StoreError> {
        self.inner.find_active_by_fingerprint(fingerprint).await
    }

    async fn find_recent_resolved_by_fingerprint(
        &self,
        fingerprint: &Fingerprint,
        since: jiff::Timestamp,
    ) -> Result<Option<Incident>, StoreError> {
        self.inner
            .find_recent_resolved_by_fingerprint(fingerprint, since)
            .await
    }

    async fn list(
        &self,
        filter: &IncidentFilter,
        page: &Page,
    ) -> Result<Paged<Incident>, StoreError> {
        self.inner.list(filter, page).await
    }

    async fn discard_pending(&self, id: IncidentId) -> Result<(), StoreError> {
        self.inner.discard_pending(id).await
    }

    async fn append_event(&self, entry: &IncidentTimelineEntry) -> Result<(), StoreError> {
        self.inner.append_event(entry).await
    }

    async fn timeline(&self, id: IncidentId) -> Result<Vec<IncidentTimelineEntry>, StoreError> {
        self.inner.timeline(id).await
    }

    async fn search(
        &self,
        text: &str,
        host: Option<HostId>,
        limit: u16,
    ) -> Result<Vec<Incident>, StoreError> {
        self.inner.search(text, host, limit).await
    }
}

/// Replaces the world's service with one reading incidents through `incidents`.
async fn serve_with_incidents(w: &mut World, incidents: Arc<dyn IncidentRepo>) {
    w.shutdown.cancel();
    if let Some(task) = w.task.take() {
        let _ = task.await;
    }
    w.shutdown = tokio_util::sync::CancellationToken::new();
    let caps = action_caps(w.link.clone());
    let (handle, task) = OperationService::spawn(
        OperationDeps {
            bus: w.bus.clone(),
            operations: w.store.clone(),
            uow: w.uow.clone(),
            effects: w.queue.clone(),
            effect_read: w.store.clone(),
            advisories: w.store.clone(),
            incidents,
            hosts: w.store.clone(),
            evidence: domovoi_engine::Evidence::new(
                evidence_caps(w.link.clone()),
                w.clock.clone(),
                w.registry.clone(),
            ),
            registry: w.registry.clone(),
            resolver: w.resolver.clone(),
            observer: observer_of(w.link.clone()),
            journal: caps.journal,
            handoff: caps.handoff,
            rerun: caps.rerun,
            redactor: Arc::new(domovoi_testkit::ListRedactor::new(["hunter2"])),
            clock: w.clock.clone(),
            shutdown: w.shutdown.clone(),
            investigate: None,
        },
        w.settings.clone(),
    );
    w.handle = Some(handle);
    w.task = Some(task);
}

#[tokio::test]
async fn a_freshness_store_error_leaves_the_step_approved() {
    let mut w = world_gated().await;
    let open = open_incident(host_id(), "docker.container", Some("plex")).unwrap();
    IncidentRepo::insert(&*w.store, &open).await.unwrap();
    let flaky = Arc::new(FlakyIncidents {
        inner: w.store.clone(),
        failures: AtomicU32::new(1),
        reads: AtomicU32::new(0),
    });
    serve_with_incidents(&mut w, flaky.clone()).await;
    let op = operation_about(&w, &open).await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();

    w.queue.open();
    for _ in 0..40 {
        w.settle().await;
        if flaky.reads.load(Ordering::SeqCst) > 0 {
            break;
        }
    }
    w.settle().await;

    assert!(
        flaky.reads.load(Ordering::SeqCst) >= 1,
        "the freshness check ran and failed"
    );
    // The failure settled nothing: the step is not refused, not run and not sent.
    assert_eq!(w.outcome(op, step).await, StepOutcome::Pending);
    assert!(has_approval(&w, op, step).await, "the approval stands");
    assert!(w.skill_requests().is_empty());
    assert!(!audited(&w, "execution.refused").await);
    assert!(!audited(&w, "execution.not_run").await);

    // The store recovers and the same approval is carried out.
    w.until_outcome(op, step, StepOutcome::Succeeded).await;
    assert_eq!(w.skill_requests().len(), 1);
}

#[tokio::test]
async fn dispatch_without_a_valid_approval_is_refused() {
    let w = world().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    // A dispatch for a step nobody approved, as a stray or forged effect would be.
    let now = w.now();
    let stray = EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Dispatch,
        aggregate: AggregateRef::operation(op),
        payload: json!({ "v": 1, "operation": op.to_string(), "step": step.to_string() }),
        not_before: now,
        max_attempts: 5,
        dedup_key: None,
        created_at: now,
    };
    w.uow
        .commit(Transaction::new().effect(stray))
        .await
        .unwrap();

    w.until_outcome(op, step, StepOutcome::Refused).await;

    assert_eq!(
        reason(&w, op, step).await,
        "the execution has no valid human authorization"
    );
    assert!(!has_approval(&w, op, step).await);
    assert!(w.skill_requests().is_empty(), "nothing reached the agent");
    assert!(!audited(&w, "execution.dispatched").await);
}

#[tokio::test]
async fn stale_step_approval_is_not_dispatched_after_a_restart() {
    let mut w = world_gated().await;
    let op = w.create().await;
    let step = w.propose(op).await;
    w.approve(op, step).await.unwrap();
    w.advance(PAST_WINDOW);

    w.restart().await;
    w.queue.open();
    w.until_outcome(op, step, StepOutcome::NotRun).await;

    assert_eq!(reason(&w, op, step).await, EXPIRED);
    assert!(w.skill_requests().is_empty());
    assert!(!audited(&w, "execution.dispatched").await);
}
