//! Plan hash v2 end to end: the approval binding, dispatch recomputation and the two hashes.

use std::collections::BTreeMap;

use domovoi_core::ports::{ActionRepo, HostRepo, IncidentRepo, Presence};
use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, ApprovalProvenance, Authorization, ExecutionId,
    ImplHash, Incident, KeyFingerprint, PlanBinding, PlanContract, PlanHash, PlanTarget,
    PolicyPermit, PolicyPermitParts, Principal, ScriptDigest, StoredAction, UserId,
};
use domovoi_engine::{ActionSettings, ApprovalError};
use domovoi_testkit::approvals::human;
use domovoi_testkit::build::{T0, open_incident};
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;

use crate::actions_harness::{Actions, actions, verified_restart};
use crate::harness::host_id;

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
    let changed = ActionRequest::from_stored(StoredAction {
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
    });
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
    let mut reenrolled = enrolled.clone();
    reenrolled.public_key = [8; 32];
    HostRepo::put_agent(&*a.store, &reenrolled).await.unwrap();

    reconnect(&a);
    rejected(&a, action.id()).await;

    // Putting the original key back does not resurrect a terminal action.
    HostRepo::put_agent(&*a.store, &enrolled).await.unwrap();
    a.settle().await;
    assert!(matches!(
        a.get(action.id()).await.status(),
        ActionStatus::Rejected { .. }
    ));
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
    let legacy = ActionRequest::from_stored(legacy_action(&a));
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
    let legacy = ActionRequest::from_stored(legacy_action(&a));
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
#[cfg(test)]
async fn v1_action_dispatch_recomputes_the_v1_hash() {
    let a = actions(ActionSettings::default()).await;
    let legacy = ActionRequest::from_stored(legacy_action(&a));
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
