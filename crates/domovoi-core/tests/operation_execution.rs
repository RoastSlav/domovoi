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

#[test]
fn authorize_with_a_policy_permit_is_not_yet_supported() {
    let mut record = proposed();
    let before = record.clone();
    let permit = PolicyPermit::issue(PolicyPermitParts {
        decision_id: "d1",
        rule_ids: vec!["r".to_owned()],
        policy_version: "1",
        plan_hash: hash(0xab),
        issued_at: T0,
        expires_at: T0 + SignedDuration::from_hours(1),
        ceiling_hash: &"cd".repeat(32),
        safety_identity: None,
    })
    .unwrap();
    let result = record.authorize(
        Authorization::policy(permit),
        provenance(),
        &PlanBinding::Full(hash(0xab)),
        T0,
    );
    assert_eq!(result, Err(ExecutionError::NotYetSupported));
    assert_eq!(record, before);
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
