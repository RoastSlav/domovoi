//! The one-shot retirement of legacy actions at the first start after the upgrade.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionFilter, ActionRepo, AggregateKind, AggregateRef, AuditEvent, AuditFilter, AuditLog,
    Clock, CommitError, CommitReceipt, EffectDraft, EffectKind, EffectQueue, EffectRead,
    EffectStatus, JournalRead, Page, Paged, StoreError, Transaction, UnitOfWork,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, ApprovalProvenance, Authorization, EffectId, HostId,
    IncidentId, OperationId, PlanBinding, Risk, UserId,
};
use domovoi_engine::legacy::{
    LegacyDrainDeps, LegacyDrainError, LegacyDrainReport, drain_legacy_actions,
};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::{T0, at};
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::SignedDuration;
use serde_json::json;

const EXPIRED: &str = "legacy action expired at upgrade";
const UNKNOWN: &str = "legacy executor removed before this run was reconciled";
const DROPPED: &str = "legacy verification dropped at upgrade";

/// Every status name, in the order the tests seed them.
const ALL: [&str; 12] = [
    "proposed",
    "approved",
    "dispatched",
    "succeeded",
    "denied",
    "expired",
    "rejected",
    "failed",
    "verified",
    "verify_failed",
    "outcome_unknown",
    "not_run",
];

/// A world over a fresh store with the clock at [`T0`].
struct World {
    store: Arc<InMemoryStore>,
    clock: Arc<FakeClock>,
    host: HostId,
    incident: IncidentId,
}

#[cfg(test)]
impl World {
    fn new() -> Self {
        Self {
            store: Arc::new(InMemoryStore::new()),
            clock: Arc::new(FakeClock::new(T0)),
            host: HostId::new(),
            incident: IncidentId::new(),
        }
    }

    /// Deps over the store itself.
    fn deps(&self) -> LegacyDrainDeps {
        self.deps_over(self.store.clone(), self.store.clone())
    }

    /// Deps that list through `actions` and commit through `uow`.
    fn deps_over(&self, actions: Arc<dyn ActionRepo>, uow: Arc<dyn UnitOfWork>) -> LegacyDrainDeps {
        LegacyDrainDeps {
            actions,
            incidents: self.store.clone(),
            uow,
            effects: self.store.clone(),
            effect_read: self.store.clone(),
            clock: self.clock.clone(),
        }
    }

    /// An action as the first-generation executor left it in `status`.
    fn action(&self, status: &str) -> ActionRequest {
        let mut a = domovoi_testkit::build::action(self.incident, self.host, Risk::Write)
            .expect("a valid action");
        if status == "proposed" {
            return a;
        }
        if status == "denied" {
            a.deny(at(10)).unwrap();
            return a;
        }
        if status == "expired" {
            a.expire(at(7200)).unwrap();
            return a;
        }
        let binding = PlanBinding::Full(a.plan_hash_for_approval());
        a.approve(
            Authorization::human(human(UserId::new())),
            ApprovalProvenance::new(None, "test"),
            &binding,
            at(10),
        )
        .unwrap();
        if status == "approved" {
            return a;
        }
        if status == "rejected" {
            a.reject("changed".to_owned(), at(20)).unwrap();
            return a;
        }
        if status == "not_run" {
            a.never_ran("never started".to_owned(), at(20)).unwrap();
            return a;
        }
        let hash = a.plan_hash_for_approval();
        a.dispatch(hash, None, at(20)).unwrap();
        match status {
            "dispatched" => {}
            "outcome_unknown" => {
                a.outcome_unknown("lost".to_owned(), at(30)).unwrap();
            }
            "failed" => {
                a.complete(Err("boom".to_owned()), at(30)).unwrap();
            }
            _ => {
                a.complete(Ok(()), at(30)).unwrap();
                match status {
                    "verified" => {
                        a.verified(true, at(40)).unwrap();
                    }
                    "verify_failed" => {
                        a.verified(false, at(40)).unwrap();
                    }
                    _ => assert_eq!(status, "succeeded"),
                }
            }
        }
        a
    }

    /// Stores a new action in `status` and returns it.
    async fn seed(&self, status: &str) -> ActionRequest {
        let a = self.action(status);
        assert_eq!(a.status().name(), status);
        ActionRepo::insert(&*self.store, &a).await.unwrap();
        a
    }

    async fn stored(&self, id: ActionId) -> ActionRequest {
        ActionRepo::get(&*self.store, id)
            .await
            .unwrap()
            .expect("the row is still there")
    }

    /// Audit events about `id`, oldest first.
    async fn audit(&self, id: ActionId) -> Vec<AuditEvent> {
        let filter = AuditFilter {
            subject: Some(format!("action:{id}")),
            ..AuditFilter::default()
        };
        let mut items = AuditLog::list(&*self.store, &filter, &Page::first(500))
            .await
            .unwrap()
            .items;
        items.reverse();
        items
    }

    async fn audit_total(&self) -> usize {
        AuditLog::list(&*self.store, &AuditFilter::default(), &Page::first(500))
            .await
            .unwrap()
            .items
            .len()
    }

    async fn journal(&self, id: ActionId) -> Vec<domovoi_core::ports::JournalEntry> {
        JournalRead::entries_for(&*self.store, &AggregateRef::action(id), 500)
            .await
            .unwrap()
    }

    async fn journal_total(&self) -> usize {
        JournalRead::entries_after(&*self.store, 0, 500)
            .await
            .unwrap()
            .len()
    }

    /// Queues a pending `wake` for `id`, as the old executor did to verify a run.
    async fn queue_wake(&self, id: ActionId, purpose: &str) -> EffectId {
        let effect = EffectId::new();
        let draft = EffectDraft {
            id: effect,
            kind: EffectKind::Wake,
            aggregate: AggregateRef::action(id),
            payload: json!({ "v": 1, "purpose": purpose, "action": id.to_string() }),
            not_before: T0,
            max_attempts: 3,
            dedup_key: None,
            created_at: T0,
        };
        UnitOfWork::commit(&*self.store, Transaction::new().effect(draft))
            .await
            .unwrap();
        effect
    }

    async fn effect_status(&self, id: EffectId) -> String {
        EffectRead::effect(&*self.store, id)
            .await
            .unwrap()
            .unwrap()
            .status
    }

    /// Effects in every status, as a cheap fingerprint of the outbox.
    async fn effect_fingerprint(&self) -> Vec<(String, usize)> {
        let mut out = Vec::new();
        for status in [
            EffectStatus::Pending,
            EffectStatus::Claimed,
            EffectStatus::Done,
            EffectStatus::Dead,
            EffectStatus::Cancelled,
        ] {
            let n = EffectRead::effects_with_status(&*self.store, status, 500)
                .await
                .unwrap()
                .len();
            out.push((status.as_str().to_owned(), n));
        }
        out
    }
}

#[tokio::test]
async fn proposed_action_inside_its_window_is_expired_at_upgrade() {
    let w = World::new();
    let mut action = w.action("proposed");
    action = action.with_chat_token_hash([9; 32]);
    ActionRepo::insert(&*w.store, &action).await.unwrap();
    let id = action.id();
    w.clock.advance(SignedDuration::from_mins(10));
    assert!(w.clock.now() < action.expires_at(), "the window is open");

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    let stored = w.stored(id).await;
    assert_eq!(stored.status(), &ActionStatus::Expired);
    assert_eq!(stored.chat_token_hash(), None);
    let audit = w.audit(id).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].kind, "action.expired");
    assert_eq!(audit[0].actor, "system:legacy");
    assert_eq!(audit[0].detail["reason"], EXPIRED);
    let journal = w.journal(id).await;
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].payload["from"], "proposed");
    assert_eq!(journal[0].payload["to"], "expired");
    assert_eq!(
        report,
        LegacyDrainReport {
            expired: 1,
            ..LegacyDrainReport::default()
        }
    );
}

#[tokio::test]
async fn approved_action_is_not_run_at_upgrade() {
    let w = World::new();
    let action = w.seed("approved").await;
    let id = action.id();
    let wake = w.queue_wake(id, "anything").await;

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    assert_eq!(
        w.stored(id).await.status(),
        &ActionStatus::NotRun {
            reason: EXPIRED.to_owned()
        }
    );
    let audit = w.audit(id).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].kind, "action.not_run");
    assert_eq!(audit[0].detail["reason"], EXPIRED);
    assert_eq!(w.journal(id).await.len(), 1);
    assert_eq!(w.effect_status(wake).await, "cancelled");
    assert_eq!(report.not_run, 1);
    assert_eq!(report.effects_cancelled, 1);
    assert_eq!(report.expired + report.outcome_unknown, 0);
}

#[tokio::test]
async fn dispatched_action_becomes_outcome_unknown_not_failed() {
    let w = World::new();
    let action = w.seed("dispatched").await;
    let id = action.id();

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    let status = w.stored(id).await.status().clone();
    assert_eq!(
        status,
        ActionStatus::OutcomeUnknown {
            reason: UNKNOWN.to_owned()
        }
    );
    assert!(!matches!(status, ActionStatus::Failed { .. }));
    let audit = w.audit(id).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].kind, "action.outcome_unknown");
    assert_eq!(audit[0].detail["reason"], UNKNOWN);
    assert_eq!(w.journal(id).await.len(), 1);
    assert_eq!(report.outcome_unknown, 1);
}

/// Seeds a succeeded action with a pending verification wake.
async fn succeeded_with_wake(w: &World) -> (ActionId, EffectId) {
    let action = w.seed("succeeded").await;
    let wake = w.queue_wake(action.id(), "verify_action").await;
    (action.id(), wake)
}

#[tokio::test]
async fn pending_verification_is_cancelled_and_the_action_stays_succeeded() {
    let w = World::new();
    let (id, wake) = succeeded_with_wake(&w).await;

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    assert_eq!(w.effect_status(wake).await, "cancelled");
    assert_eq!(w.stored(id).await.status(), &ActionStatus::Succeeded);
    let audit = w.audit(id).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].kind, "action.verification_dropped");
    assert_eq!(audit[0].detail["reason"], DROPPED);
    assert_eq!(report.verification_dropped, 1);
    assert_eq!(report.effects_cancelled, 1);
}

#[tokio::test]
async fn a_claimed_verification_wake_is_cancelled_too() {
    let w = World::new();
    let (id, wake) = succeeded_with_wake(&w).await;
    let batch = EffectQueue::claim_due(
        &*w.store,
        "old-session",
        &[EffectKind::Wake],
        &[AggregateKind::Action],
        T0,
        SignedDuration::from_mins(5),
        10,
    )
    .await
    .unwrap();
    assert_eq!(batch.claimed.len(), 1);
    assert_eq!(w.effect_status(wake).await, "claimed");

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    assert_eq!(w.effect_status(wake).await, "cancelled");
    assert_eq!(w.stored(id).await.status(), &ActionStatus::Succeeded);
    assert_eq!(report.verification_dropped, 1);
    assert_eq!(report.effects_cancelled, 1);
}

#[tokio::test]
async fn terminal_actions_are_left_untouched() {
    let w = World::new();
    let mut before = Vec::new();
    for status in [
        "denied",
        "expired",
        "rejected",
        "failed",
        "verified",
        "verify_failed",
        "outcome_unknown",
        "not_run",
    ] {
        before.push(w.seed(status).await);
    }

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    assert_eq!(report, LegacyDrainReport::default());
    for action in before {
        assert_eq!(w.stored(action.id()).await, action);
    }
    assert_eq!(w.audit_total().await, 0);
    assert_eq!(w.journal_total().await, 0);
}

#[tokio::test]
async fn drain_is_idempotent() {
    let w = World::new();
    w.seed("proposed").await;
    w.seed("approved").await;
    w.seed("dispatched").await;
    let (_, _) = succeeded_with_wake(&w).await;
    let first = drain_legacy_actions(&w.deps()).await.unwrap();
    assert_ne!(first, LegacyDrainReport::default());
    let audit = w.audit_total().await;
    let journal = w.journal_total().await;
    let effects = w.effect_fingerprint().await;

    let second = drain_legacy_actions(&w.deps()).await.unwrap();

    assert_eq!(second, LegacyDrainReport::default());
    assert_eq!(w.audit_total().await, audit);
    assert_eq!(w.journal_total().await, journal);
    assert_eq!(w.effect_fingerprint().await, effects);
}

/// Lists through the store, but can fail listings and can move a proposal after it has
/// been listed.
struct Meddling {
    store: Arc<InMemoryStore>,
    failures: AtomicU32,
    rival: Mutex<Option<ActionRequest>>,
}

impl Meddling {
    fn new(store: Arc<InMemoryStore>) -> Self {
        Self {
            store,
            failures: AtomicU32::new(0),
            rival: Mutex::new(None),
        }
    }
}

#[async_trait]
impl ActionRepo for Meddling {
    async fn insert(&self, action: &ActionRequest) -> Result<(), StoreError> {
        ActionRepo::insert(&*self.store, action).await
    }

    async fn update_from(&self, action: &ActionRequest, expected: &str) -> Result<(), StoreError> {
        ActionRepo::update_from(&*self.store, action, expected).await
    }

    async fn get(&self, id: ActionId) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::get(&*self.store, id).await
    }

    async fn list(
        &self,
        filter: &ActionFilter,
        page: &Page,
    ) -> Result<Paged<ActionRequest>, StoreError> {
        let left = self.failures.load(Ordering::SeqCst);
        if left > 0 {
            self.failures.store(left - 1, Ordering::SeqCst);
            return Err(StoreError::Backend("listing failed".to_owned()));
        }
        let listed = ActionRepo::list(&*self.store, filter, page).await?;
        if filter.status.as_deref() == Some("proposed") {
            let rival = self
                .rival
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(rival) = rival {
                ActionRepo::update_from(&*self.store, &rival, "proposed").await?;
            }
        }
        Ok(listed)
    }

    async fn find_by_chat_token_hash(
        &self,
        hash: &[u8; 32],
    ) -> Result<Option<ActionRequest>, StoreError> {
        ActionRepo::find_by_chat_token_hash(&*self.store, hash).await
    }

    async fn last_run_by_skill(
        &self,
        host: HostId,
    ) -> Result<std::collections::BTreeMap<domovoi_core::SkillName, jiff::Timestamp>, StoreError>
    {
        ActionRepo::last_run_by_skill(&*self.store, host).await
    }
}

#[tokio::test]
async fn drain_skips_a_row_decided_meanwhile() {
    let w = World::new();
    let raced = w.seed("proposed").await;
    let other = w.seed("proposed").await;
    let approved = w.seed("approved").await;
    let meddling = Arc::new(Meddling::new(w.store.clone()));
    let mut denied = raced.clone();
    denied.deny(at(30)).unwrap();
    *meddling.rival.lock().unwrap() = Some(denied);
    let deps = w.deps_over(meddling, w.store.clone());

    let report = drain_legacy_actions(&deps).await.unwrap();

    assert_eq!(w.stored(raced.id()).await.status(), &ActionStatus::Denied);
    assert!(
        w.audit(raced.id()).await.is_empty(),
        "nothing is audited for a skipped row"
    );
    assert_eq!(w.stored(other.id()).await.status(), &ActionStatus::Expired);
    assert!(matches!(
        w.stored(approved.id()).await.status(),
        ActionStatus::NotRun { .. }
    ));
    assert_eq!(report.expired, 1);
    assert_eq!(report.not_run, 1);
}

#[tokio::test]
async fn drain_survives_a_failing_page_by_restarting_the_listing() {
    let w = World::new();
    let a = w.seed("proposed").await;
    let meddling = Arc::new(Meddling::new(w.store.clone()));
    meddling.failures.store(1, Ordering::SeqCst);
    let deps = w.deps_over(meddling.clone(), w.store.clone());

    let report = drain_legacy_actions(&deps).await.unwrap();

    assert_eq!(report.expired, 1);
    assert_eq!(w.stored(a.id()).await.status(), &ActionStatus::Expired);

    let b = w.seed("proposed").await;
    meddling.failures.store(3, Ordering::SeqCst);
    let result = drain_legacy_actions(&deps).await;

    assert!(matches!(result, Err(LegacyDrainError::Store(_))));
    assert_eq!(w.stored(b.id()).await.status(), &ActionStatus::Proposed);
}

/// Refuses every transaction that carries an audit row, and applies nothing.
struct NoAudit(Arc<InMemoryStore>);

#[async_trait]
impl UnitOfWork for NoAudit {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        if !tx.audit.is_empty() {
            return Err(CommitError::Constraint("audit refused".to_owned()));
        }
        UnitOfWork::commit(&*self.0, tx).await
    }
}

#[tokio::test]
async fn drain_writes_through_the_unit_of_work_only() {
    let w = World::new();
    let a = w.seed("proposed").await;

    let refusing = w.deps_over(w.store.clone(), Arc::new(NoAudit(w.store.clone())));
    let result = drain_legacy_actions(&refusing).await;
    assert!(matches!(result, Err(LegacyDrainError::Store(_))));
    assert_eq!(w.stored(a.id()).await.status(), &ActionStatus::Proposed);
    assert_eq!(w.audit_total().await, 0);
    assert_eq!(w.journal_total().await, 0);

    let b = w.seed("approved").await;
    let c = w.seed("dispatched").await;
    drain_legacy_actions(&w.deps()).await.unwrap();
    for id in [a.id(), b.id(), c.id()] {
        assert_eq!(w.audit(id).await.len(), 1, "one audit row for {id}");
        assert_eq!(w.journal(id).await.len(), 1, "one journal row for {id}");
    }
}

#[tokio::test]
async fn no_open_legacy_action_remains_after_the_drain() {
    let w = World::new();
    let mut ids = Vec::new();
    for status in ALL {
        ids.push(w.seed(status).await.id());
    }
    let (_, _) = succeeded_with_wake(&w).await;

    drain_legacy_actions(&w.deps()).await.unwrap();

    for id in ids {
        let name = w.stored(id).await.status().name();
        assert!(
            !matches!(name, "proposed" | "approved" | "dispatched"),
            "{id} is still {name}"
        );
    }
}

#[tokio::test]
async fn verification_wake_behind_many_unrelated_pending_effects_is_still_dropped() {
    let w = World::new();
    let mut crowd = Transaction::new();
    for _ in 0..501 {
        crowd = crowd.effect(EffectDraft {
            id: EffectId::new(),
            kind: EffectKind::Wake,
            aggregate: AggregateRef::operation(OperationId::new()),
            payload: json!({ "v": 1, "purpose": "other" }),
            not_before: T0,
            max_attempts: 3,
            dedup_key: None,
            created_at: T0,
        });
    }
    UnitOfWork::commit(&*w.store, crowd).await.unwrap();
    let (id, wake) = succeeded_with_wake(&w).await;
    let ahead = EffectRead::effects_with_status(&*w.store, EffectStatus::Pending, 500)
        .await
        .unwrap();
    assert!(
        ahead.iter().all(|effect| effect.id != wake),
        "the wake is beyond the first 500 pending effects"
    );

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    assert_eq!(w.effect_status(wake).await, "cancelled");
    assert_eq!(w.stored(id).await.status(), &ActionStatus::Succeeded);
    let audit = w.audit(id).await;
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].kind, "action.verification_dropped");
    assert_eq!(report.verification_dropped, 1);
    assert_eq!(report.effects_cancelled, 1);
}

/// Commits normally until its budget is spent, then refuses every transaction.
struct FailAfter {
    store: Arc<InMemoryStore>,
    left: AtomicU32,
}

#[async_trait]
impl UnitOfWork for FailAfter {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        let left = self.left.load(Ordering::SeqCst);
        if left == 0 {
            return Err(CommitError::Constraint("store went away".to_owned()));
        }
        self.left.store(left - 1, Ordering::SeqCst);
        UnitOfWork::commit(&*self.store, tx).await
    }
}

#[tokio::test]
async fn a_drain_interrupted_after_some_rows_resumes_on_the_next_start() {
    let w = World::new();
    let first = w.seed("proposed").await.id();
    let second = w.seed("proposed").await.id();
    let approved = w.seed("approved").await.id();
    let dispatched = w.seed("dispatched").await.id();
    let ids = [first, second, approved, dispatched];

    let flaky = Arc::new(FailAfter {
        store: w.store.clone(),
        left: AtomicU32::new(2),
    });
    let result = drain_legacy_actions(&w.deps_over(w.store.clone(), flaky)).await;

    assert!(matches!(result, Err(LegacyDrainError::Store(_))));
    assert_eq!(w.stored(first).await.status(), &ActionStatus::Expired);
    assert_eq!(w.stored(second).await.status(), &ActionStatus::Expired);
    assert_eq!(w.stored(approved).await.status(), &ActionStatus::Approved);
    assert_eq!(w.stored(dispatched).await.status().name(), "dispatched");

    let report = drain_legacy_actions(&w.deps()).await.unwrap();

    assert_eq!(report.not_run, 1);
    assert_eq!(report.outcome_unknown, 1);
    assert_eq!(report.expired, 0);
    assert!(matches!(
        w.stored(approved).await.status(),
        ActionStatus::NotRun { .. }
    ));
    assert!(matches!(
        w.stored(dispatched).await.status(),
        ActionStatus::OutcomeUnknown { .. }
    ));
    for id in ids {
        assert!(!matches!(
            w.stored(id).await.status(),
            ActionStatus::Failed { .. }
        ));
        assert_eq!(w.audit(id).await.len(), 1, "one audit row for {id}");
        assert_eq!(w.journal(id).await.len(), 1, "one journal row for {id}");
    }

    let third = drain_legacy_actions(&w.deps()).await.unwrap();
    assert_eq!(third, LegacyDrainReport::default());
}

#[tokio::test]
async fn a_claimed_legacy_effect_never_reaches_a_handler_after_the_drain() {
    let w = World::new();
    let (id, wake) = succeeded_with_wake(&w).await;
    let batch = EffectQueue::claim_due(
        &*w.store,
        "old-session",
        &[EffectKind::Wake],
        &[AggregateKind::Action],
        T0,
        SignedDuration::from_mins(5),
        10,
    )
    .await
    .unwrap();
    assert_eq!(batch.claimed.len(), 1);

    drain_legacy_actions(&w.deps()).await.unwrap();

    let later = EffectQueue::claim_due(
        &*w.store,
        "new-session",
        &[EffectKind::Wake],
        &[AggregateKind::Action],
        at(10_000_000),
        SignedDuration::from_mins(5),
        10,
    )
    .await
    .unwrap();
    assert!(later.claimed.is_empty() && later.dead.is_empty());
    assert_eq!(w.effect_status(wake).await, "cancelled");
    assert_eq!(w.stored(id).await.status(), &ActionStatus::Succeeded);
}
