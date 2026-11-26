//! Tests of the closed containment call.

use jiff::{SignedDuration, Timestamp};

use super::*;
use crate::action::{AuthorizationKind, PlanHash};
use crate::attest::{ExpectedSafety, UnverifiedReason};
use crate::id::{ExecutionId, StepId};

fn spec(text: &str) -> BlockSpec {
    BlockSpec {
        block: ExecutionId::from_step(StepId::new()),
        net: Net::parse(text).unwrap(),
        ttl: SignedDuration::from_secs(600),
    }
}

fn plan() -> PlanHash {
    "ab".repeat(32).parse().unwrap()
}

fn unverified() -> ExpectedSafety {
    ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyNotPinned,
    }
}

#[test]
fn prefix_len_args_always_carry_both_families() {
    let v4 = spec("203.0.113.0/24").args(keys::PHASE_APPLY);
    assert_eq!(v4[keys::ARG_PREFIX_LEN_V4], "24");
    assert_eq!(v4[keys::ARG_PREFIX_LEN_V6], "128");
    let v6 = spec("2001:db8::/48").args(keys::PHASE_APPLY);
    assert_eq!(v6[keys::ARG_PREFIX_LEN_V4], "32");
    assert_eq!(v6[keys::ARG_PREFIX_LEN_V6], "48");
}

#[test]
fn contain_call_has_no_script_or_key_argument() {
    let block = spec("203.0.113.7/32");
    for call in [
        ContainCall::Apply(block.clone()),
        ContainCall::Confirm(block.clone()),
        ContainCall::Rollback(block.clone()),
    ] {
        let args = call.args();
        let mut keys_seen: Vec<&str> = args.keys().map(String::as_str).collect();
        keys_seen.sort_unstable();
        let mut expected = keys::ARG_KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys_seen, expected);
        assert_eq!(args[keys::ARG_SOURCE], "203.0.113.7/32");
        assert_eq!(args[keys::ARG_TTL_S], "600");
    }
    assert!(ContainCall::List.args().is_empty());
}

#[test]
fn phases_of_one_block_differ_only_in_the_phase_argument() {
    let block = spec("203.0.113.0/24");
    let mut apply = ContainCall::Apply(block.clone()).args();
    let mut confirm = ContainCall::Confirm(block).args();
    assert_ne!(apply[keys::ARG_PHASE], confirm[keys::ARG_PHASE]);
    apply.remove(keys::ARG_PHASE);
    confirm.remove(keys::ARG_PHASE);
    assert_eq!(apply, confirm);
}

#[test]
fn a_phase_always_carries_expected_safety_and_a_list_never_does() {
    let expires = Timestamp::UNIX_EPOCH;
    let phase = ContainDispatch::phase(
        ExecutionId::from_step(StepId::new()),
        ContainCall::Apply(spec("203.0.113.7/32")),
        plan(),
        AuthorizationKind::Human,
        expires,
        unverified(),
    )
    .unwrap();
    assert!(phase.expected_safety().is_some());
    let refused = ContainDispatch::phase(
        ExecutionId::from_step(StepId::new()),
        ContainCall::List,
        plan(),
        AuthorizationKind::Human,
        expires,
        unverified(),
    );
    assert_eq!(refused, Err(DispatchError::NotAPhase));
    let list = ContainDispatch::list(ExecutionId::from_step(StepId::new()), plan(), expires);
    assert!(list.expected_safety().is_none());
    assert_eq!(list.authorization(), AuthorizationKind::Human);
}
