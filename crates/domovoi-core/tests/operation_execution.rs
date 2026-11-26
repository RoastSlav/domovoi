//! The execution row: authorization rules and the state derived from the step's outcome.

use std::collections::BTreeMap;

use domovoi_core::capability::ContractId;
use domovoi_core::{
    ApprovalChannel, ApprovalProvenance, Authorization, CheckKey, ExecutionError, ExecutionIntent,
    ExecutionRecord, ExecutionState, HostId, HumanApproval, OperationId, PlanBinding, PlanHash,
    PolicyPermit, PolicyPermitParts, StepId, StepOutcome, TargetKey, UserId,
};
use jiff::{SignedDuration, Timestamp};

const T0: Timestamp = Timestamp::constant(1_760_000_000, 0);

#[cfg(test)]
fn hash(byte: u8) -> PlanHash {
    hex::encode([byte; 32]).parse().unwrap()
}

#[cfg(test)]
fn proposed() -> ExecutionRecord {
    let intent = ExecutionIntent {
        contract: ContractId::parse("skill.restart_container").unwrap(),
        contract_version: 1,
        host: HostId::new(),
        check: CheckKey::parse("container.down").unwrap(),
        target: Some(TargetKey::parse("plex").unwrap()),
        args: BTreeMap::new(),
        retry_of: None,
    };
    ExecutionRecord::propose(
        &intent,
        StepId::new(),
        OperationId::new(),
        hash(0xab),
        T0 + SignedDuration::from_hours(24),
        domovoi_core::Risk::Write,
        T0,
    )
}

#[cfg(test)]
fn human() -> Authorization {
    Authorization::human(HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Web {
            typed_confirmation: false,
        },
        T0,
    ))
}

#[cfg(test)]
fn provenance() -> ApprovalProvenance {
    ApprovalProvenance::new(Some("session-ref".to_owned()), "web")
}

#[test]
fn execution_id_is_the_step_id() {
    let record = proposed();
    assert_eq!(record.execution.as_step(), record.step);
}

#[test]
fn execution_state_is_derived_from_the_row_and_the_step_outcome() {
    let mut record = proposed();
    let state = |r: &ExecutionRecord, step, approval| r.state(step, approval);
    assert_eq!(
        state(&record, StepOutcome::Pending, None),
        ExecutionState::Proposed
    );
    assert_eq!(
        state(&record, StepOutcome::Pending, Some(StepOutcome::Pending)),
        ExecutionState::AwaitingApproval
    );
    assert_eq!(
        state(&record, StepOutcome::Pending, Some(StepOutcome::Refused)),
        ExecutionState::Proposed
    );
    record
        .authorize(human(), provenance(), &PlanBinding::Full(hash(0xab)), T0)
        .unwrap();
    assert_eq!(
        state(&record, StepOutcome::Pending, Some(StepOutcome::Succeeded)),
        ExecutionState::Authorized
    );
    assert_eq!(
        state(&record, StepOutcome::Running, None),
        ExecutionState::Dispatched
    );
    let finals = [
        (StepOutcome::Succeeded, ExecutionState::Succeeded),
        (StepOutcome::Failed, ExecutionState::Failed),
        (StepOutcome::Refused, ExecutionState::Refused),
        (StepOutcome::NotRun, ExecutionState::NotRun),
        (StepOutcome::Unavailable, ExecutionState::Unavailable),
        (StepOutcome::OutcomeUnknown, ExecutionState::OutcomeUnknown),
        (StepOutcome::Skipped, ExecutionState::Skipped),
        (StepOutcome::Cancelled, ExecutionState::Cancelled),
    ];
    for (outcome, expected) in finals {
        assert_eq!(state(&record, outcome, None), expected);
        assert_eq!(expected.name(), outcome.as_str());
    }
}

#[test]
fn authorize_with_the_displayed_hash_records_the_approval() {
    let mut record = proposed();
    let at = T0 + SignedDuration::from_secs(30);
    record
        .authorize(human(), provenance(), &PlanBinding::Full(hash(0xab)), at)
        .unwrap();
    let approval = record.approval().unwrap();
    assert_eq!(approval.displayed(), hash(0xab));
    assert_eq!(approval.provenance().client(), "web");
    assert_eq!(record.updated_at, at);
}

#[test]
fn authorize_with_a_chat_prefix_records_the_stored_hash() {
    let mut record = proposed();
    let prefix = PlanBinding::Prefix(hash(0xab).as_bytes()[..4].to_vec());
    record
        .authorize(human(), provenance(), &prefix, T0)
        .unwrap();
    assert_eq!(record.approval().unwrap().displayed(), hash(0xab));
}

#[test]
fn authorize_with_another_hash_changes_nothing() {
    let mut record = proposed();
    let before = record.clone();
    let result = record.authorize(human(), provenance(), &PlanBinding::Full(hash(0xcd)), T0);
    assert_eq!(result, Err(ExecutionError::PlanChanged));
    assert_eq!(record, before);
}

#[cfg(test)]
fn permit_for(plan: PlanHash, expires: Timestamp) -> Authorization {
    Authorization::policy(
        PolicyPermit::issue(PolicyPermitParts {
            decision_id: "d1",
            rule_ids: vec!["r".to_owned()],
            policy_version: "1",
            plan_hash: plan,
            issued_at: T0,
            expires_at: expires,
            ceiling_hash: &"cd".repeat(32),
            safety_identity: None,
        })
        .unwrap(),
    )
}

#[test]
fn execution_authorize_by_permit_checks_in_order() {
    let hour = T0 + SignedDuration::from_hours(1);

    // 1: the stored risk must be exactly write.
    let mut destructive = proposed();
    destructive = ExecutionRecord::restore(domovoi_core::ExecutionParts {
        operation: destructive.operation,
        step: destructive.step,
        host: destructive.host,
        contract: destructive.contract.clone(),
        contract_version: destructive.contract_version,
        check: destructive.check.clone(),
        target: destructive.target.clone(),
        args: destructive.args.clone(),
        plan_hash: destructive.plan_hash,
        expires_at: destructive.expires_at,
        risk: Some(domovoi_core::Risk::Destructive),
        chat_token_hash: None,
        approval: None,
        permit: None,
        waiting_since: None,
        dispatched: None,
        settled_at: None,
        created_at: T0,
        updated_at: T0,
    });
    let before = destructive.clone();
    let result = destructive.authorize_by_permit(&permit_for(hash(0xab), hour), "doc".into(), T0);
    assert_eq!(result, Err(ExecutionError::PermitNotAllowed));
    assert_eq!(destructive, before);

    // 2: the permit must name the stored plan hash.
    let mut record = proposed();
    let before = record.clone();
    let result = record.authorize_by_permit(&permit_for(hash(0xcd), hour), "doc".into(), T0);
    assert_eq!(result, Err(ExecutionError::PlanChanged));
    assert_eq!(record, before);

    // 3: a second authorization is refused, whichever kind came first.
    let mut record = proposed();
    record
        .authorize(human(), provenance(), &PlanBinding::Full(hash(0xab)), T0)
        .unwrap();
    let before = record.clone();
    let result = record.authorize_by_permit(&permit_for(hash(0xab), hour), "doc".into(), T0);
    assert_eq!(result, Err(ExecutionError::AlreadyAuthorized));
    assert_eq!(record, before);

    // 4: an expired permit, or a closed window, is refused.
    let mut record = proposed();
    let before = record.clone();
    let result = record.authorize_by_permit(&permit_for(hash(0xab), hour), "doc".into(), hour);
    assert_eq!(result, Err(ExecutionError::Expired));
    assert_eq!(record, before);

    // The permit is stored with its decision document.
    let mut record = proposed();
    record
        .authorize_by_permit(&permit_for(hash(0xab), hour), "doc".into(), T0)
        .unwrap();
    assert_eq!(
        record.authorization_kind(),
        Some(domovoi_core::AuthorizationKind::Policy)
    );
    let stored = record.permit().unwrap();
    assert_eq!(stored.decision_id(), "d1");
    assert_eq!(stored.decision(), "doc");
    assert!(record.approval().is_none());
}

/// A destructive-risk execution of `contract`, the way a proposal stores a block.
#[cfg(test)]
fn destructive_of(contract: &str) -> ExecutionRecord {
    let intent = ExecutionIntent {
        contract: ContractId::parse(contract).unwrap(),
        contract_version: 1,
        host: HostId::new(),
        check: CheckKey::parse("firewall.block").unwrap(),
        target: Some(TargetKey::parse("ipnet:203.0.113.0/24").unwrap()),
        args: BTreeMap::new(),
        retry_of: None,
    };
    ExecutionRecord::propose(
        &intent,
        StepId::new(),
        OperationId::new(),
        hash(0xab),
        T0 + SignedDuration::from_hours(1),
        domovoi_core::Risk::Destructive,
        T0,
    )
}

/// A permit for `plan` that names a verified safety identity, or none.
#[cfg(test)]
fn identified_permit(plan: PlanHash, identity: Option<&str>) -> Authorization {
    Authorization::policy(
        PolicyPermit::issue(PolicyPermitParts {
            decision_id: "d1",
            rule_ids: vec!["r".to_owned()],
            policy_version: "1",
            plan_hash: plan,
            issued_at: T0,
            expires_at: T0 + SignedDuration::from_mins(5),
            ceiling_hash: &"cd".repeat(32),
            safety_identity: identity,
        })
        .unwrap(),
    )
}

#[test]
fn a_permit_with_a_verified_identity_authorizes_a_block() {
    let mut block = destructive_of("firewall.block_source");
    let permit = identified_permit(hash(0xab), Some("v1:identity"));
    block
        .authorize_by_permit(&permit, "doc".into(), T0)
        .expect("a verified block may be permitted");
    assert!(block.is_authorized());
    assert_eq!(
        block.authorization_kind(),
        Some(domovoi_core::AuthorizationKind::Policy)
    );
}

#[test]
fn a_permit_without_a_verified_identity_never_authorizes_a_block() {
    let mut block = destructive_of("firewall.block_source");
    let before = block.clone();
    let result = block.authorize_by_permit(&identified_permit(hash(0xab), None), "doc".into(), T0);
    assert_eq!(result, Err(ExecutionError::PermitNotAllowed));
    assert_eq!(block, before);
}

#[test]
fn a_permit_never_authorizes_another_destructive_contract() {
    for contract in ["container.restart", "skill.prune_images", "unit.restart"] {
        let mut other = destructive_of(contract);
        let before = other.clone();
        let result = other.authorize_by_permit(
            &identified_permit(hash(0xab), Some("v1:identity")),
            "doc".into(),
            T0,
        );
        assert_eq!(result, Err(ExecutionError::PermitNotAllowed), "{contract}");
        assert_eq!(other, before);
    }
}

#[test]
fn execution_cannot_hold_approval_and_permit() {
    let hour = T0 + SignedDuration::from_hours(1);
    let mut record = proposed();
    record
        .authorize_by_permit(&permit_for(hash(0xab), hour), "doc".into(), T0)
        .unwrap();
    let result = record.authorize(human(), provenance(), &PlanBinding::Full(hash(0xab)), T0);
    assert_eq!(result, Err(ExecutionError::AlreadyAuthorized));
    assert!(record.approval().is_none());
    assert!(record.permit().is_some());
}

#[test]
fn authorize_after_expiry_is_refused() {
    let mut record = proposed();
    let before = record.clone();
    let late = record.expires_at;
    let result = record.authorize(human(), provenance(), &PlanBinding::Full(hash(0xab)), late);
    assert_eq!(result, Err(ExecutionError::Expired));
    assert_eq!(record, before);
}

#[test]
fn second_authorize_is_refused() {
    let mut record = proposed();
    record
        .authorize(human(), provenance(), &PlanBinding::Full(hash(0xab)), T0)
        .unwrap();
    let before = record.clone();
    let result = record.authorize(human(), provenance(), &PlanBinding::Full(hash(0xab)), T0);
    assert_eq!(result, Err(ExecutionError::AlreadyAuthorized));
    assert_eq!(record, before);
}
