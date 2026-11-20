//! Plan hashes: the first generation binds no key fingerprint, the second does, and an
//! approved action without authorization evidence never loads.

use std::collections::BTreeMap;

use domovoi_core::{
    ActionId, ActionRequest, ActionStatus, CheckKey, ExecutionId, ImplHash, IncidentId,
    KeyFingerprint, PlanContract, PlanHash, PlanTarget, Principal, StoredAction, TargetKey,
};
use domovoi_testkit::build::{T0, open_incident};
use jiff::SignedDuration;

use crate::harness::{host_id, restart_skill};

#[test]
fn plan_hash_v1_has_no_key_fingerprint_and_v2_binds_it() {
    let skill = restart_skill();
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
    let v2 = |key: u8| {
        PlanHash::compute_v2(
            &PlanContract::skill(&skill.name, 1).unwrap(),
            ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
            &PlanTarget::of_incident(&incident, KeyFingerprint::of_public_key(&[key; 32])),
            &args,
            ExecutionId::from_uuid(uuid::Uuid::from_u128(1)),
        )
    };
    assert_ne!(v2(7), v2(8), "v2 binds the key fingerprint");
}

/// A stored v2 action in the approved state with no authorization evidence.
fn approved_row_without_evidence(incident: IncidentId) -> StoredAction {
    let skill = restart_skill();
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
        incident,
        host: host_id(),
        skill: skill.name.clone(),
        risk: skill.risk,
        args: args.to_map(),
        plan_hash,
        plan_hash_v2: Some(plan_hash_v2),
        approved_plan_hash: None,
        provenance: None,
        authorization_kind: None,
        status: ActionStatus::Approved,
        proposed_by: Principal::System("triage".into()),
        approved_by: None,
        expires_at: T0.checked_add(SignedDuration::from_hours(1)).unwrap(),
        created_at: T0,
        updated_at: T0,
        chat_token_hash: None,
    }
}

#[test]
fn an_approved_action_without_authorization_evidence_cannot_be_loaded() {
    let stored = approved_row_without_evidence(IncidentId::new());
    assert!(
        matches!(
            ActionRequest::try_from_stored(stored),
            Err(domovoi_core::StoredActionError::MissingAuthorization { .. })
        ),
        "an approved action with no authorization case fails closed at the load boundary",
    );
}
