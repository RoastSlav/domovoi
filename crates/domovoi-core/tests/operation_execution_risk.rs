//! The risk gate of an execution row: which channels may approve which risks.

use std::collections::BTreeMap;

use domovoi_core::capability::ContractId;
use domovoi_core::{
    ApprovalChannel, ApprovalProvenance, Authorization, CheckKey, ExecutionError, ExecutionIntent,
    ExecutionParts, ExecutionRecord, HostId, HumanApproval, OperationId, PlanBinding, PlanHash,
    Risk, StepId, UserId,
};
use jiff::Timestamp;

const T0: Timestamp = Timestamp::constant(1_760_000_000, 0);

#[cfg(test)]
fn hash() -> PlanHash {
    hex::encode([0xab; 32]).parse().unwrap()
}

#[cfg(test)]
fn record(risk: Option<Risk>) -> ExecutionRecord {
    let intent = ExecutionIntent {
        contract: ContractId::parse("skill.restart_container").unwrap(),
        contract_version: 1,
        host: HostId::new(),
        check: CheckKey::parse("container.down").unwrap(),
        target: None,
        args: BTreeMap::new(),
        retry_of: None,
    };
    let proposed = ExecutionRecord::propose(
        &intent,
        StepId::new(),
        OperationId::new(),
        hash(),
        T0 + jiff::SignedDuration::from_hours(1),
        Risk::Write,
        T0,
    );
    ExecutionRecord::restore(ExecutionParts {
        operation: proposed.operation,
        step: proposed.step,
        host: proposed.host,
        contract: proposed.contract.clone(),
        contract_version: 1,
        check: proposed.check.clone(),
        target: None,
        args: BTreeMap::new(),
        plan_hash: hash(),
        expires_at: proposed.expires_at,
        risk,
        chat_token_hash: None,
        approval: None,
        waiting_since: None,
        dispatched: None,
        settled_at: None,
        created_at: T0,
        updated_at: T0,
    })
}

#[cfg(test)]
fn by(channel: ApprovalChannel) -> Authorization {
    Authorization::human(HumanApproval::new(UserId::new(), channel, T0))
}

#[cfg(test)]
fn chat() -> ApprovalChannel {
    ApprovalChannel::Chat {
        platform: "discord".to_owned(),
        platform_user: "1".to_owned(),
    }
}

#[cfg(test)]
fn web(typed: bool) -> ApprovalChannel {
    ApprovalChannel::Web {
        typed_confirmation: typed,
    }
}

#[cfg(test)]
fn try_approve(risk: Option<Risk>, channel: ApprovalChannel) -> Result<(), ExecutionError> {
    record(risk).authorize(
        by(channel),
        ApprovalProvenance::new(None, "test"),
        &PlanBinding::Full(hash()),
        T0,
    )
}

#[test]
fn chat_authorize_of_a_non_read_or_write_step_is_refused_by_the_record() {
    for risk in [Some(Risk::Destructive), None] {
        assert_eq!(
            try_approve(risk, chat()),
            Err(ExecutionError::NeedsTypedConfirmation)
        );
        assert_eq!(
            try_approve(risk, web(false)),
            Err(ExecutionError::NeedsTypedConfirmation)
        );
        assert_eq!(try_approve(risk, web(true)), Ok(()));
    }
    for risk in [Risk::Read, Risk::Write] {
        assert_eq!(try_approve(Some(risk), chat()), Ok(()));
        assert_eq!(try_approve(Some(risk), web(false)), Ok(()));
    }
}

#[test]
fn execution_authorize_risk_gate_order() {
    let wrong = PlanBinding::Prefix(vec![0, 0, 0]);
    // The risk gate comes before the binding.
    let mut destructive = record(Some(Risk::Destructive));
    let err = destructive
        .authorize(
            by(chat()),
            ApprovalProvenance::new(None, "test"),
            &wrong,
            T0,
        )
        .unwrap_err();
    assert_eq!(err, ExecutionError::NeedsTypedConfirmation);
    // The policy case comes before everything.
    let mut write = record(Some(Risk::Write));
    assert!(write.chat_token_hash().is_none());
    let err = write
        .authorize(
            by(web(false)),
            ApprovalProvenance::new(None, "test"),
            &wrong,
            T0,
        )
        .unwrap_err();
    assert_eq!(err, ExecutionError::PlanChanged);
}

#[test]
fn execution_record_round_trips_risk_and_token_hash() {
    let record = record(Some(Risk::Write)).with_chat_token_hash([7; 32]);
    assert_eq!(record.risk(), Some(Risk::Write));
    assert_eq!(record.chat_token_hash(), Some(&[7; 32]));
    let mut cleared = record;
    cleared.clear_chat_token_hash();
    assert_eq!(cleared.chat_token_hash(), None);
}
