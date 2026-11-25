//! The containment half of [`WsAgentLink`]: `firewall.block_source` phases and
//! `firewall.list_blocks` over `run_capability`.
//!
//! Wire 1.2 is unchanged. Each phase is its own execution; the closed [`ContainCall`] decides
//! the contract, the target and the six arguments, and nothing else reaches the frame.

use domovoi_core::TargetKey;
use domovoi_core::containment::ContainDispatch;
use domovoi_core::ports::LinkError;
use domovoi_proto::messages::{AuthorizationKind, RunCapability};

/// The contract version containment is requested at.
const CONTRACT_VERSION: u32 = 1;

/// The `run_capability` request of one containment call: the wire target of a block is
/// `ipnet:{canonical}`, of a list `firewall:all`.
pub(super) fn contain_run_capability_of(req: &ContainDispatch) -> Result<RunCapability, LinkError> {
    let target_text = match req.call().spec() {
        Some(spec) => format!("ipnet:{}", spec.net.canonical()),
        None => "firewall:all".to_owned(),
    };
    let target = TargetKey::parse(&target_text)
        .map_err(|err| LinkError::Rejected(format!("the containment target is invalid: {err}")))?;
    Ok(RunCapability {
        execution_id: req.execution().as_uuid(),
        contract: req.call().contract_id().to_owned(),
        version: CONTRACT_VERSION,
        target,
        args: req.call().args(),
        plan_hash: req.plan_hash(),
        authorization_kind: match req.authorization() {
            domovoi_core::AuthorizationKind::Human => AuthorizationKind::Human,
            domovoi_core::AuthorizationKind::Policy => AuthorizationKind::Policy,
        },
        expires_at: req.expires_at(),
        expected_safety: req.expected_safety().cloned(),
    })
}

#[cfg(test)]
mod tests {
    use domovoi_core::attest::{ExpectedSafety, UnverifiedReason};
    use domovoi_core::containment::{BlockSpec, ContainCall, Net, keys};
    use domovoi_core::{AuthorizationKind, ExecutionId, StepId};
    use jiff::{SignedDuration, Timestamp};

    use super::*;

    fn spec() -> BlockSpec {
        BlockSpec {
            block: ExecutionId::from_step(StepId::new()),
            net: Net::parse("203.0.113.0/24").unwrap(),
            ttl: SignedDuration::from_secs(900),
        }
    }

    fn dispatch(call: ContainCall) -> ContainDispatch {
        ContainDispatch::phase(
            ExecutionId::from_step(StepId::new()),
            call,
            "ab".repeat(32).parse().unwrap(),
            AuthorizationKind::Human,
            Timestamp::UNIX_EPOCH,
            ExpectedSafety::Unverified {
                reason: UnverifiedReason::KeyNotPinned,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_phase_frame_names_the_network_and_carries_expected_safety() {
        let run = contain_run_capability_of(&dispatch(ContainCall::Apply(spec()))).unwrap();
        assert_eq!(run.contract, "firewall.block_source");
        assert_eq!(run.target.to_string(), "ipnet:203.0.113.0/24");
        assert_eq!(run.args.len(), 6);
        assert_eq!(run.args[keys::ARG_PHASE], "apply");
        assert!(run.expected_safety.is_some());
    }

    #[test]
    fn a_list_frame_is_a_human_read_without_expected_safety() {
        let list = ContainDispatch::list(
            ExecutionId::from_step(StepId::new()),
            "00".repeat(32).parse().unwrap(),
            Timestamp::UNIX_EPOCH,
        );
        let run = contain_run_capability_of(&list).unwrap();
        assert_eq!(run.contract, "firewall.list_blocks");
        assert!(run.args.is_empty());
        assert!(run.expected_safety.is_none());
        assert_eq!(
            run.authorization_kind,
            domovoi_proto::messages::AuthorizationKind::Human
        );
    }
}
