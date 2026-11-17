//! Plan hash v2 end to end: the approval binding, dispatch recomputation and the two hashes.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    ActionRepo, AggregateRef, EnrollmentToken, HostRepo, IncidentRepo, JournalRead, Page, Presence,
    StoreError,
};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, AgentId, AgentIdentity, AgentReport, ApprovalProvenance,
    AuthenticatedReport, Authorization, AuthorizationKind, CheckKey, ExecutionId, Host, HostId,
    HostName, ImplHash, Incident, KeyFingerprint, PlanBinding, PlanContract, PlanHash, PlanTarget,
    PolicyPermit, PolicyPermitParts, Principal, ReportOutcome, ScriptDigest, StoredAction,
    TargetKey, UserId,
};
use domovoi_engine::{ActionSettings, ApprovalError};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;

use crate::actions_harness::{Actions, actions, seeded, verified_restart};
use crate::harness::host_id;

/// A host repository that holds `live_agent` until the test releases it, so the executor's
/// session snapshot and target can be paused before the writer decides.
struct GatedHosts {
    inner: Arc<domovoi_testkit::InMemoryStore>,
    gate: Arc<tokio::sync::Semaphore>,
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait]
impl HostRepo for GatedHosts {
    async fn upsert_host(&self, host: &Host) -> Result<(), StoreError> {
        self.inner.upsert_host(host).await
    }

    async fn get_host(&self, id: HostId) -> Result<Option<Host>, StoreError> {
        self.inner.get_host(id).await
    }

    async fn get_host_by_name(&self, name: &HostName) -> Result<Option<Host>, StoreError> {
        self.inner.get_host_by_name(name).await
    }

    async fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        self.inner.list_hosts().await
    }

    async fn put_agent(&self, agent: &AgentIdentity) -> Result<(), StoreError> {
        self.inner.put_agent(agent).await
    }

    async fn get_agent(&self, id: AgentId) -> Result<Option<AgentIdentity>, StoreError> {
        self.inner.get_agent(id).await
    }

    async fn live_agent(&self, host: HostId) -> Result<Option<AgentIdentity>, StoreError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| StoreError::Backend("the test gate closed".to_owned()))?;
        self.inner.live_agent(host).await
    }

    async fn revoke_agent(&self, id: AgentId, at: jiff::Timestamp) -> Result<(), StoreError> {
        self.inner.revoke_agent(id, at).await
    }

    async fn agent_report(&self, agent: AgentId) -> Result<Option<AgentReport>, StoreError> {
        self.inner.agent_report(agent).await
    }

    async fn record_agent_report(
        &self,
        agent: AgentId,
        seen: &AuthenticatedReport,
        now: jiff::Timestamp,
    ) -> Result<ReportOutcome, StoreError> {
        self.inner.record_agent_report(agent, seen, now).await
    }

    async fn create_enrollment_token(&self, token: &EnrollmentToken) -> Result<(), StoreError> {
        self.inner.create_enrollment_token(token).await
    }

    async fn consume_enrollment_token(
        &self,
        token_hash: &[u8; 32],
        now: jiff::Timestamp,
    ) -> Result<HostId, StoreError> {
        self.inner.consume_enrollment_token(token_hash, now).await
    }
}

/// A world whose enrolled-key read the test can hold.
async fn gated_world() -> (
    crate::actions_harness::Seeded,
    Arc<tokio::sync::Semaphore>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let world = seeded().await;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hosts = Arc::new(GatedHosts {
        inner: world.store.clone(),
        gate: gate.clone(),
        calls: calls.clone(),
    });
    (world.with_hosts(hosts), gate, calls)
}

async fn wait_for_key_read(calls: &std::sync::atomic::AtomicUsize) {
    for _ in 0..5000 {
        if calls.load(std::sync::atomic::Ordering::SeqCst) > 0 {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the executor never reached the enrolled-key read");
}

#[tokio::test]
async fn a_replacement_session_never_receives_the_v2_snapshot() {
    let (world, gate, calls) = gated_world().await;
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    wait_for_key_read(&calls).await;

    // The executor has taken the snapshot and is held at the key read. Replace the session,
    // keeping the journal epoch and the manifest hash but changing the entry digest.
    let mut replacement = verified_restart();
    replacement.entry_script_digest = ScriptDigest::of_bytes(b"a different entry");
    a.link.set_skills(host_id(), vec![replacement]);
    a.link.new_session(host_id());
    gate.add_permits(4);

    let settled = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::Rejected { .. })
        })
        .await;
    assert!(
        matches!(settled.status(), ActionStatus::Rejected { reason } if reason.contains("session")),
        "the replacement session is refused: {:?}",
        settled.status(),
    );
    assert_eq!(a.runs(), 0, "the replacement agent received no request");
}

#[tokio::test]
async fn changed_incident_subject_between_executor_and_writer_rejects_dispatch() {
    let (world, gate, calls) = gated_world().await;
    let a = world.start(ActionSettings::default(), world.store.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    wait_for_key_read(&calls).await;

    let original = IncidentRepo::get(&*a.store, action.incident())
        .await
        .unwrap()
        .expect("the incident is stored");
    let changed = open_incident(host_id(), "docker.container", Some("/different"))
        .unwrap()
        .with_id(original.id());
    IncidentRepo::update(&*a.store, &changed).await.unwrap();
    gate.add_permits(4);

    let settled = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::Rejected { .. })
        })
        .await;
    assert!(
        matches!(settled.status(), ActionStatus::Rejected { reason } if reason.contains("plan")),
        "the changed subject invalidates the plan: {:?}",
        settled.status(),
    );
    assert_eq!(a.runs(), 0);
}

#[tokio::test]
async fn a_session_change_after_the_dispatch_commit_reaches_no_agent() {
    let a = actions(ActionSettings::default()).await;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    a.link.hold_skill_handoffs(gate.clone());
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    a.until(action.id(), |status| {
        matches!(status, ActionStatus::Dispatched { .. })
    })
    .await;

    a.link.new_session(host_id());
    gate.add_permits(1);
    let settled = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::NotRun { .. })
        })
        .await;
    assert!(
        matches!(settled.status(), ActionStatus::NotRun { .. }),
        "a failed handoff is positive evidence nothing ran",
    );
    assert_eq!(a.runs(), 0);
}

#[tokio::test]
async fn unchanged_session_and_target_still_run() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    approve(&a, &action).await;
    let settled = a
        .until(action.id(), |status| {
            matches!(status, ActionStatus::Succeeded | ActionStatus::Verified)
        })
        .await;
    assert!(matches!(
        settled.status(),
        ActionStatus::Succeeded | ActionStatus::Verified
    ));
    assert_eq!(a.runs(), 1);
}

/// A run whose agent never comes back, so the executor waits while a test changes the world.
#[cfg(test)]
async fn stalled() -> Actions {
    let a = actions(ActionSettings::default()).await;
    a.link
        .set_presence(host_id(), Presence::Disconnected { last_seen: None });
    a
}

#[cfg(test)]
fn reconnect(a: &Actions) {
    a.link.set_presence(
        host_id(),
        Presence::Connected {
            since: a.now(),
            version: "0.1.0".to_owned(),
        },
    );
}

#[cfg(test)]
async fn approve(a: &Actions, action: &ActionRequest) -> ActionRequest {
    a.service
        .approve(
            action.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(action.plan_hash_for_approval()),
            ApprovalProvenance::new(Some("session".to_owned()), "web"),
        )
        .await
        .expect("the approval is accepted")
}

#[cfg(test)]
async fn rejected(a: &Actions, id: ActionId) -> ActionRequest {
    let settled = a
        .until(id, |status| matches!(status, ActionStatus::Rejected { .. }))
        .await;
    assert_eq!(a.runs(), 0, "no request may reach the agent");
    settled
}

#[tokio::test]
#[cfg(test)]
async fn approve_refuses_a_hash_from_another_plan_v2() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let (other, _) = a
        .proposal(
            &verified_restart(),
            &[("container", "postgres")],
            SignedDuration::from_hours(1),
        )
        .await;

    let refused = a
        .service
        .approve(
            action.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(other.plan_hash_for_approval()),
            ApprovalProvenance::new(None, "test"),
        )
        .await;
    assert_eq!(refused.err(), Some(ApprovalError::PlanChanged));
    assert_eq!(a.get(action.id()).await.status(), &ActionStatus::Proposed);
}

#[tokio::test]
#[cfg(test)]
async fn approve_records_the_displayed_hash_and_provenance() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let shown = action.plan_hash_for_approval();

    let approved = approve(&a, &action).await;
    let stored = a.get(approved.id()).await;
    assert_eq!(stored.approved_plan_hash(), Some(shown));
    assert_eq!(
        stored.provenance().map(|p| p.session_ref()),
        Some(Some("session"))
    );
    assert_eq!(stored.provenance().map(|p| p.client()), Some("web"));
    assert_eq!(
        stored.authorization_kind(),
        Some(domovoi_core::AuthorizationKind::Human)
    );
}

#[tokio::test]
#[cfg(test)]
async fn approval_bound_to_displayed_plan_hash() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let refused = a
        .service
        .approve(
            action.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(action.plan_hash()),
            ApprovalProvenance::new(None, "test"),
        )
        .await;
    assert_eq!(refused.err(), Some(ApprovalError::PlanChanged));
    assert_eq!(a.get(action.id()).await.status(), &ActionStatus::Proposed);

    let approved = approve(&a, &action).await;
    assert_eq!(approved.status(), &ActionStatus::Approved);
}

#[tokio::test]
#[cfg(test)]
async fn changed_args_invalidate_approval() {
    let a = stalled().await;
    let (action, _) = a.propose().await;

    let stored = a.get(action.id()).await;
    let changed = ActionRequest::try_from_stored(StoredAction {
        id: stored.id(),
        incident: stored.incident(),
        host: stored.host(),
        skill: stored.skill().clone(),
        risk: stored.risk(),
        args: BTreeMap::from([("container".to_owned(), "postgres".to_owned())]),
        plan_hash: stored.plan_hash(),
        plan_hash_v2: stored.plan_hash_v2(),
        approved_plan_hash: stored.approved_plan_hash(),
        provenance: stored.provenance().cloned(),
        authorization_kind: stored.authorization_kind(),
        status: stored.status().clone(),
        proposed_by: stored.proposed_by().clone(),
        approved_by: stored.approved_by().cloned(),
        expires_at: stored.expires_at(),
        created_at: stored.created_at(),
        updated_at: stored.updated_at(),
        chat_token_hash: stored.chat_token_hash().copied(),
    })
    .expect("the changed proposal is still a valid stored action");
    ActionRepo::update_from(&*a.store, &changed, "proposed")
        .await
        .unwrap();

    approve(&a, &action).await;

    reconnect(&a);
    rejected(&a, action.id()).await;
}

#[tokio::test]
#[cfg(test)]
async fn dispatch_refuses_after_host_reenrollment() {
    let a = stalled().await;
    let (action, _) = a.propose().await;
    approve(&a, &action).await;

    let enrolled = HostRepo::live_agent(&*a.store, host_id())
        .await
        .unwrap()
        .expect("the harness host has an agent");
    HostRepo::revoke_agent(&*a.store, enrolled.agent, a.now())
        .await
        .unwrap();
    let mut reenrolled = enrolled.clone();
    reenrolled.agent = AgentId::new();
    reenrolled.public_key = [8; 32];
    HostRepo::put_agent(&*a.store, &reenrolled).await.unwrap();

    reconnect(&a);
    rejected(&a, action.id()).await;

    // The revoked identity cannot be restored, and the terminal action stays rejected.
    assert!(
        HostRepo::put_agent(&*a.store, &enrolled).await.is_err(),
        "a revoked agent identity never comes back"
    );
    a.settle().await;
    assert!(matches!(
        a.get(action.id()).await.status(),
        ActionStatus::Rejected { .. }
    ));
}

#[tokio::test]
#[cfg(test)]
async fn a_legacy_v1_action_is_not_dispatched_to_a_replacement_enrollment() {
    let a = stalled().await;
    let legacy = ActionRequest::try_from_stored(legacy_action(&a))
        .expect("a legacy proposed action is a valid stored action");
    ActionRepo::insert(&*a.store, &legacy).await.unwrap();
    approve(&a, &legacy).await;

    // A re-enrollment revokes the old identity and enrolls a new `AgentId`, even when the
    // operator reused the same key.
    let enrolled = HostRepo::live_agent(&*a.store, host_id())
        .await
        .unwrap()
        .expect("the harness host has an agent");
    HostRepo::revoke_agent(&*a.store, enrolled.agent, a.now())
        .await
        .unwrap();
    let mut replacement = enrolled.clone();
    replacement.agent = AgentId::new();
    replacement.public_key = [8; 32];
    HostRepo::put_agent(&*a.store, &replacement).await.unwrap();

    reconnect(&a);
    rejected(&a, legacy.id()).await;
}

#[tokio::test]
#[cfg(test)]
async fn dispatch_refuses_when_the_host_has_no_live_agent() {
    let a = stalled().await;
    let (action, _) = a.propose().await;
    approve(&a, &action).await;

    let enrolled = HostRepo::live_agent(&*a.store, host_id())
        .await
        .unwrap()
        .expect("the harness host has an agent");
    HostRepo::revoke_agent(&*a.store, enrolled.agent, a.now())
        .await
        .unwrap();

    reconnect(&a);
    rejected(&a, action.id()).await;
}

#[tokio::test]
#[cfg(test)]
async fn changed_target_invalidates_approval() {
    let a = stalled().await;
    let (action, _) = a.propose().await;
    approve(&a, &action).await;

    let original = IncidentRepo::get(&*a.store, action.incident())
        .await
        .unwrap()
        .expect("the incident is stored");
    let changed: Incident = open_incident(host_id(), "docker.container", Some("/other"))
        .unwrap()
        .with_id(original.id());
    IncidentRepo::update(&*a.store, &changed).await.unwrap();

    reconnect(&a);
    let settled = rejected(&a, action.id()).await;
    assert!(
        matches!(settled.status(), ActionStatus::Rejected { reason } if reason.contains("plan")),
        "the rejection names the plan",
    );
}

#[tokio::test]
#[cfg(test)]
async fn changed_entry_script_digest_invalidates_approval() {
    let a = stalled().await;
    let (action, _) = a.propose().await;
    approve(&a, &action).await;

    let mut changed = verified_restart();
    changed.entry_script_digest = ScriptDigest::of_bytes(b"a replaced entry");
    a.link.set_skills(host_id(), vec![changed]);

    reconnect(&a);
    rejected(&a, action.id()).await;
}

#[tokio::test]
#[cfg(test)]
async fn dispatch_rejects_a_mismatched_plan_hash() {
    let a = stalled().await;
    let (action, _) = a.propose().await;
    let approved = approve(&a, &action).await;

    let mut copy = approved.clone();
    let change = copy
        .dispatch("00".repeat(32).parse().unwrap(), None, a.now())
        .unwrap();
    assert!(matches!(change.to, ActionStatus::Rejected { .. }));
    assert!(
        a.get(action.id()).await.status() == &ActionStatus::Approved,
        "the stored row is untouched by a copy's refused transition",
    );
}

#[tokio::test]
#[cfg(test)]
async fn new_v2_action_binds_the_approval_and_dispatch_with_v2() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let v2 = action.plan_hash_v2().expect("a new action has a v2 plan");
    assert_eq!(action.plan_hash_for_approval(), v2);

    let approved = approve(&a, &action).await;
    assert_eq!(approved.approved_plan_hash(), Some(v2));

    let (other, _) = a.propose().await;
    let refused = a
        .service
        .approve(
            other.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(other.plan_hash()),
            ApprovalProvenance::new(None, "test"),
        )
        .await;
    assert_eq!(refused.err(), Some(ApprovalError::PlanChanged));
}

#[tokio::test]
#[cfg(test)]
async fn policy_authorization_is_refused_at_m1() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let permit = PolicyPermit::issue(PolicyPermitParts {
        decision_id: "dec-1",
        rule_ids: vec!["rule".to_owned()],
        policy_version: "1",
        plan_hash: action.plan_hash_for_approval(),
        issued_at: a.now(),
        expires_at: a.now() + SignedDuration::from_mins(5),
        ceiling_hash: &"cd".repeat(32),
        safety_identity: None,
    })
    .unwrap();

    let refused = a
        .service
        .approve(
            action.id(),
            Authorization::policy(permit),
            None,
            PlanBinding::Full(action.plan_hash_for_approval()),
            ApprovalProvenance::new(None, "policy"),
        )
        .await;
    assert_eq!(refused.err(), Some(ApprovalError::NotYetSupported));
    assert_eq!(a.get(action.id()).await.status(), &ActionStatus::Proposed);
    assert_eq!(a.runs(), 0);
    assert_eq!(a.trail(action.id()).await, ["action.proposed"]);

    // The dispatch path independently refuses a stored policy authorization.
    let world = seeded().await;
    let stored = authorized_row(&world, Some(AuthorizationKind::Policy), false);
    let id = stored.id;
    ActionRepo::insert(
        &*world.store,
        &ActionRequest::try_from_stored(stored).expect("an approved policy row is loadable"),
    )
    .await
    .unwrap();
    let a = world.start(ActionSettings::default(), world.store.clone());
    wait_for_snapshot(&a).await;
    assert_eq!(
        a.get(id).await.status(),
        &ActionStatus::Approved,
        "a refused dispatch leaves the action approved",
    );
    assert_eq!(a.runs(), 0, "no skill reached the agent");
    assert!(a.trail(id).await.is_empty(), "no dispatch audit entry");
    let journal = JournalRead::entries_for(&*a.store, &AggregateRef::action(id), Page::MAX_LIMIT)
        .await
        .unwrap();
    assert!(journal.is_empty(), "no dispatch journal entry");
}

/// A stored v2 action approved by the given case, or with no authorization evidence.
fn authorized_row(
    world: &crate::actions_harness::Seeded,
    kind: Option<AuthorizationKind>,
    with_approver: bool,
) -> StoredAction {
    let skill = verified_restart();
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let id = ActionId::new();
    let target = PlanTarget::new(
        host_id(),
        KeyFingerprint::of_public_key(&[7; 32]),
        CheckKey::parse("docker.container").unwrap(),
        Some(TargetKey::parse("nginx").unwrap()),
    );
    let plan_hash = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args);
    let plan_hash_v2 = PlanHash::compute_v2(
        &PlanContract::skill(&skill.name, 1).unwrap(),
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &target,
        &args,
        ExecutionId::from_action(id),
    );
    StoredAction {
        id,
        incident: world.incident.id(),
        host: host_id(),
        skill: skill.name.clone(),
        risk: skill.risk,
        args: args.to_map(),
        plan_hash,
        plan_hash_v2: Some(plan_hash_v2),
        approved_plan_hash: with_approver.then_some(plan_hash_v2),
        provenance: with_approver.then(|| ApprovalProvenance::new(None, "policy")),
        authorization_kind: kind,
        status: ActionStatus::Approved,
        proposed_by: Principal::System("triage".into()),
        approved_by: with_approver.then(|| human(UserId::new())),
        expires_at: T0.checked_add(SignedDuration::from_hours(1)).unwrap(),
        created_at: T0,
        updated_at: T0,
        chat_token_hash: None,
    }
}

#[tokio::test]
async fn an_approved_action_without_authorization_evidence_cannot_be_loaded() {
    let world = seeded().await;
    let stored = authorized_row(&world, None, false);
    assert!(
        matches!(
            ActionRequest::try_from_stored(stored),
            Err(domovoi_core::StoredActionError::MissingAuthorization { .. })
        ),
        "an approved action with no authorization case fails closed at the load boundary",
    );
}

/// Waits until the executor took a session snapshot, proving a negative dispatch test
/// actually reached the dispatch path instead of merely starting no run.
async fn wait_for_snapshot(a: &Actions) {
    for _ in 0..5000 {
        if a.link.snapshot_calls() > 0 {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("the executor never reached the dispatch path");
}

#[tokio::test]
#[cfg(test)]
async fn runskill_request_carries_the_v1_local_check_hash() {
    let a = actions(ActionSettings::default()).await;
    let (action, _) = a.propose().await;
    let v2 = action.plan_hash_for_approval();
    approve(&a, &action).await;
    let _ = a
        .until(action.id(), |status| {
            matches!(
                status,
                ActionStatus::Succeeded | ActionStatus::Verified | ActionStatus::Failed { .. }
            )
        })
        .await;

    let sent = a
        .link
        .requests()
        .into_iter()
        .find_map(|request| match request {
            LinkRequest::Skill { dispatch, .. } => Some(dispatch),
            _ => None,
        })
        .expect("the agent got a run_skill request");
    let skill = verified_restart();
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let v1 = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args);
    assert_eq!(sent.plan_hash, v1, "the agent's own check stays v1");
    assert_ne!(sent.plan_hash, v2, "and is not the approval binding");
}

/// A legacy row: `plan_hash` only, no v2 binding, approved by a human before v2 existed.
#[cfg(test)]
fn legacy_action(a: &Actions) -> StoredAction {
    let skill = verified_restart();
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let id = ActionId::new();
    StoredAction {
        id,
        incident: a.incident.id(),
        host: host_id(),
        skill: skill.name.clone(),
        risk: skill.risk,
        args: args.to_map(),
        plan_hash: PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args),
        plan_hash_v2: None,
        approved_plan_hash: None,
        provenance: None,
        authorization_kind: None,
        status: ActionStatus::Proposed,
        proposed_by: Principal::System("triage".into()),
        approved_by: None,
        expires_at: T0.checked_add(SignedDuration::from_hours(1)).unwrap(),
        created_at: T0,
        updated_at: T0,
        chat_token_hash: None,
    }
}

#[tokio::test]
#[cfg(test)]
async fn v1_actions_keep_v1_binding_after_upgrade() {
    let a = actions(ActionSettings::default()).await;
    let legacy = ActionRequest::try_from_stored(legacy_action(&a))
        .expect("a legacy proposed action is a valid stored action");
    ActionRepo::insert(&*a.store, &legacy).await.unwrap();

    assert_eq!(legacy.plan_hash_v2(), None);
    assert_eq!(legacy.plan_hash_for_approval(), legacy.plan_hash());
    let approved = a
        .service
        .approve(
            legacy.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(legacy.plan_hash()),
            ApprovalProvenance::new(None, "test"),
        )
        .await
        .unwrap();
    assert_eq!(approved.approved_plan_hash(), Some(legacy.plan_hash()));

    let settled = a
        .until(legacy.id(), |status| {
            !matches!(status, ActionStatus::Approved | ActionStatus::Proposed)
        })
        .await;
    assert!(
        !matches!(settled.status(), ActionStatus::Rejected { .. }),
        "a legacy action never reaches a v2 recomputation",
    );
    assert_eq!(a.runs(), 1);
}

#[tokio::test]
#[cfg(test)]
async fn legacy_v1_action_keeps_v1_for_approval_and_runskill() {
    let a = actions(ActionSettings::default()).await;
    let legacy = ActionRequest::try_from_stored(legacy_action(&a))
        .expect("a legacy proposed action is a valid stored action");
    ActionRepo::insert(&*a.store, &legacy).await.unwrap();
    a.service
        .approve(
            legacy.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(legacy.plan_hash()),
            ApprovalProvenance::new(None, "test"),
        )
        .await
        .unwrap();

    let _ = a
        .until(legacy.id(), |status| {
            matches!(
                status,
                ActionStatus::Succeeded | ActionStatus::Verified | ActionStatus::Failed { .. }
            )
        })
        .await;
    let sent = a
        .link
        .requests()
        .into_iter()
        .find_map(|request| match request {
            LinkRequest::Skill { dispatch, .. } => Some(dispatch),
            _ => None,
        })
        .expect("the agent got a run_skill request");
    assert_eq!(sent.plan_hash, legacy.plan_hash());
}

#[tokio::test]
async fn a_historical_v1_approval_without_displayed_fields_dispatches() {
    let world = seeded().await;
    let skill = verified_restart();
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let id = ActionId::new();
    let action = ActionRequest::try_from_stored(StoredAction {
        id,
        incident: world.incident.id(),
        host: host_id(),
        skill: skill.name.clone(),
        risk: skill.risk,
        args: args.to_map(),
        plan_hash: PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args),
        plan_hash_v2: None,
        // A pre-migration approval recorded neither the displayed hash nor provenance.
        approved_plan_hash: None,
        provenance: None,
        authorization_kind: Some(AuthorizationKind::Human),
        status: ActionStatus::Approved,
        proposed_by: Principal::System("triage".into()),
        approved_by: Some(human(UserId::new())),
        expires_at: T0.checked_add(SignedDuration::from_hours(1)).unwrap(),
        created_at: T0,
        updated_at: T0,
        chat_token_hash: None,
    })
    .expect("a pre-migration approval is a valid stored action");
    ActionRepo::insert(&*world.store, &action).await.unwrap();

    let a = world.start(ActionSettings::default(), world.store.clone());
    let settled = a
        .until(id, |status| {
            !matches!(status, ActionStatus::Approved | ActionStatus::Proposed)
        })
        .await;
    assert!(
        !matches!(settled.status(), ActionStatus::Rejected { .. }),
        "a historical v1 approval still dispatches: {:?}",
        settled.status(),
    );
    assert_eq!(a.runs(), 1);
}

#[tokio::test]
#[cfg(test)]
async fn v1_action_dispatch_recomputes_the_v1_hash() {
    let a = actions(ActionSettings::default()).await;
    let legacy = ActionRequest::try_from_stored(legacy_action(&a))
        .expect("a legacy proposed action is a valid stored action");
    ActionRepo::insert(&*a.store, &legacy).await.unwrap();
    a.service
        .approve(
            legacy.id(),
            Authorization::human(human(UserId::new())),
            None,
            PlanBinding::Full(legacy.plan_hash()),
            ApprovalProvenance::new(None, "test"),
        )
        .await
        .unwrap();

    let settled = a
        .until(legacy.id(), |status| {
            !matches!(status, ActionStatus::Approved | ActionStatus::Proposed)
        })
        .await;
    assert!(
        !matches!(settled.status(), ActionStatus::Rejected { .. }),
        "the v1 plan still matches"
    );
}

#[tokio::test]
#[cfg(test)]
async fn legacy_v1_binding_remains_unchanged() {
    let skill = verified_restart();
    let args = skill
        .validate_args(&BTreeMap::from([(
            "container".to_owned(),
            "nginx".to_owned(),
        )]))
        .unwrap();
    let first = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args);
    let second = PlanHash::compute(&skill.name, skill.manifest_hash, host_id(), &args);
    assert_eq!(first, second, "v1 has no key fingerprint");

    let incident = open_incident(host_id(), "docker.container", Some("nginx")).unwrap();
    let v2_first = PlanHash::compute_v2(
        &PlanContract::skill(&skill.name, 1).unwrap(),
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &PlanTarget::of_incident(&incident, KeyFingerprint::of_public_key(&[7; 32])),
        &args,
        ExecutionId::from_uuid(uuid::Uuid::from_u128(1)),
    );
    let v2_second = PlanHash::compute_v2(
        &PlanContract::skill(&skill.name, 1).unwrap(),
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &PlanTarget::of_incident(&incident, KeyFingerprint::of_public_key(&[8; 32])),
        &args,
        ExecutionId::from_uuid(uuid::Uuid::from_u128(1)),
    );
    assert_ne!(v2_first, v2_second, "v2 binds the key fingerprint");
}
