//! `unit.identity` and `unit.restart` through `run_capability`: the ordered refusals, the
//! unattended ceiling, the journal flow and the way the helper's answers are relayed.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_agent::Journal;
use domovoi_agent::link::RequestKind;
use domovoi_agent::requests::handle;
use domovoi_agent::unattended::Ceiling;
use domovoi_config::{MutationLimits, UnattendedAllow, UnattendedSection};
use domovoi_core::capability::CeilingSummary;
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse, UnavailableReason};
use domovoi_core::isolation::{PlatformRecordState, PlatformTier};
use domovoi_core::{PlanHash, TargetKey};
use domovoi_proto::Message;
use domovoi_proto::messages::{
    ActionState, ActionStatusQuery, AuthorizationKind, CapabilityOutcome, RefusalCode,
    RunCapability,
};
use domovoi_testkit::build::T0;
use jiff::SignedDuration;
use uuid::Uuid;

use crate::helper_stub::{Answer, serve};
use crate::package_rig::{BOOT_ID, Options, Rig};

const UNIT: &str = "example.service";

#[cfg(test)]
fn hash() -> String {
    "ab".repeat(32)
}

#[cfg(test)]
fn groups(tier: PlatformTier, opt_in: bool, write_named: bool) -> HelperResponse {
    HelperResponse::Groups {
        enabled: vec![HelperGroup::SystemdRead],
        ignored: if write_named {
            vec![HelperGroup::SystemdWrite]
        } else {
            Vec::new()
        },
        write_opt_in: opt_in,
        platform_tier: tier,
        platform_record: PlatformRecordState::Ok,
    }
}

#[cfg(test)]
fn outputs(pairs: &[(&str, &str)]) -> HelperResponse {
    HelperResponse::Outputs {
        outputs: pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

/// A helper that offers both operations (untested platform, opted in) and restarts fine.
#[cfg(test)]
fn offering() -> Answer {
    Arc::new(|request| match request {
        HelperRequest::Groups => groups(PlatformTier::Untested, true, true),
        HelperRequest::UnitIdentity { .. } => outputs(&[("id", UNIT), ("identity_hash", "h")]),
        HelperRequest::UnitRestart { .. } => outputs(&[("restarted", "true")]),
        _ => HelperResponse::Health {
            protocol: 1,
            build: "t".to_owned(),
            pid: 1,
        },
    })
}

#[cfg(test)]
fn section(targets: &[&str], max_per_hour: u32, enabled: bool) -> UnattendedSection {
    UnattendedSection {
        enabled,
        allow: vec![UnattendedAllow {
            capability: "unit.restart".to_owned(),
            targets: targets.iter().map(|t| (*t).to_owned()).collect(),
            max_per_hour,
            max_active: None,
            max_ttl: None,
            args: None,
        }],
    }
}

#[cfg(test)]
struct Setup {
    rig: Rig,
    _stub: crate::helper_stub::Stub,
}

#[cfg(test)]
fn setup(answer: Answer, limits: MutationLimits) -> Setup {
    let mut rig = Rig::new(
        &[],
        Options {
            section: false,
            allow_install: false,
            limits,
        },
    );
    let socket = rig.fixture.dir.path().join("helper.sock");
    let stub = serve(&socket, answer);
    rig.ctx.helper = domovoi_agent::helper_client::HelperRelay::new(&socket);
    rig.ctx.allow_risk = vec![domovoi_core::Risk::Read, domovoi_core::Risk::Write];
    Setup { rig, _stub: stub }
}

#[cfg(test)]
fn journal_state(journal: &Journal, id: Uuid) -> ActionState {
    journal
        .action_status(&ActionStatusQuery {
            execution_ids: vec![id],
            expected_epoch: journal.epoch(),
        })
        .unwrap()
        .answers
        .remove(0)
        .state
}

#[cfg(test)]
fn restart(kind: AuthorizationKind, target: &str) -> RunCapability {
    RunCapability {
        execution_id: Uuid::now_v7(),
        contract: "unit.restart".to_owned(),
        version: 1,
        target: TargetKey::parse(target).unwrap(),
        args: BTreeMap::from([
            ("unit".to_owned(), target.to_owned()),
            ("identity_hash".to_owned(), hash()),
        ]),
        plan_hash: "07".repeat(32).parse::<PlanHash>().unwrap(),
        authorization_kind: kind,
        expires_at: T0 + SignedDuration::from_mins(5),
        expected_safety: None,
    }
}

#[cfg(test)]
fn identity(target: &str) -> RunCapability {
    RunCapability {
        contract: "unit.identity".to_owned(),
        args: BTreeMap::from([("unit".to_owned(), target.to_owned())]),
        ..restart(AuthorizationKind::Human, target)
    }
}

#[cfg(test)]
async fn call(s: &Setup, run: RunCapability) -> Message {
    handle(&s.rig.ctx, &RequestKind::Capability(run)).await
}

#[cfg(test)]
fn refused(message: &Message) -> (RefusalCode, String) {
    match message {
        Message::Refused(r) => (*r.code.known().expect("known"), r.detail.clone()),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[cfg(test)]
fn succeeded(message: Message) -> BTreeMap<String, String> {
    match message {
        Message::CapabilityResult(r) => match r.outcome {
            CapabilityOutcome::Succeeded { outputs } => outputs,
            CapabilityOutcome::Failed { message, .. } => panic!("failed: {message}"),
        },
        other => panic!("expected a result, got {other:?}"),
    }
}

#[cfg(test)]
fn failed_message(message: Message) -> String {
    match message {
        Message::CapabilityResult(r) => match r.outcome {
            CapabilityOutcome::Failed { message, .. } => message,
            CapabilityOutcome::Succeeded { .. } => panic!("it succeeded"),
        },
        other => panic!("expected a result, got {other:?}"),
    }
}

#[tokio::test]
async fn agent_refuses_policy_dispatch_outside_ceiling() {
    // No ceiling at all.
    let mut s = setup(offering(), MutationLimits::default());
    let (code, detail) = refused(&call(&s, restart(AuthorizationKind::Policy, UNIT)).await);
    assert_eq!(code, RefusalCode::CeilingExceeded);
    assert!(detail.contains("no unattended ceiling"), "{detail}");

    // Disabled.
    s.rig.ctx.ceiling = Some(Arc::new(Ceiling::from_section(&section(&[UNIT], 5, false))));
    let (code, detail) = refused(&call(&s, restart(AuthorizationKind::Policy, UNIT)).await);
    assert_eq!(code, RefusalCode::CeilingExceeded);
    assert!(detail.contains("not enabled"), "{detail}");

    // No entry for the contract.
    let mut none = section(&[UNIT], 5, true);
    none.allow.clear();
    s.rig.ctx.ceiling = Some(Arc::new(Ceiling::from_section(&none)));
    let (_, detail) = refused(&call(&s, restart(AuthorizationKind::Policy, UNIT)).await);
    assert!(detail.contains("no unattended entry"), "{detail}");

    // Target outside the list.
    s.rig.ctx.ceiling = Some(Arc::new(Ceiling::from_section(&section(
        &["other.service"],
        5,
        true,
    ))));
    let (_, detail) = refused(&call(&s, restart(AuthorizationKind::Policy, UNIT)).await);
    assert!(detail.contains("not in the unattended list"), "{detail}");

    // Inside the ceiling it runs, and the hourly count is then spent.
    s.rig.ctx.ceiling = Some(Arc::new(Ceiling::from_section(&section(&[UNIT], 1, true))));
    let done = succeeded(call(&s, restart(AuthorizationKind::Policy, UNIT)).await);
    assert_eq!(done["restarted"], "true");
    let (code, detail) = refused(&call(&s, restart(AuthorizationKind::Policy, UNIT)).await);
    assert_eq!(code, RefusalCode::CeilingExceeded);
    assert!(detail.contains("hourly unattended limit"), "{detail}");
}

#[tokio::test]
async fn human_label_skips_the_ceiling_but_not_the_limiter_or_allow_risk() {
    let mut s = setup(
        offering(),
        MutationLimits {
            per_hour: 1,
            per_capability: BTreeMap::new(),
        },
    );
    // No ceiling, human label: the ceiling is not consulted.
    succeeded(call(&s, restart(AuthorizationKind::Human, UNIT)).await);
    // The mutation limiter counts human runs too.
    let (code, _) = refused(&call(&s, restart(AuthorizationKind::Human, UNIT)).await);
    assert_eq!(code, RefusalCode::RateLimited);

    // allow_risk applies to a human label as well.
    s.rig.ctx.allow_risk = vec![domovoi_core::Risk::Read];
    let (code, _) = refused(&call(&s, restart(AuthorizationKind::Human, UNIT)).await);
    assert_eq!(code, RefusalCode::AllowRiskDenied);
}

#[tokio::test]
async fn unit_request_without_helper_op_is_contract_unknown() {
    // The helper has no write group named: it does not offer a restart, but does offer a read.
    let answer: Answer = Arc::new(|request| match request {
        HelperRequest::Groups => groups(PlatformTier::Supported, false, false),
        _ => outputs(&[]),
    });
    let s = setup(answer, MutationLimits::default());
    let (code, detail) = refused(&call(&s, restart(AuthorizationKind::Human, UNIT)).await);
    assert_eq!(code, RefusalCode::ContractUnknown);
    assert!(detail.contains("does not offer unit.restart"), "{detail}");
    // An untested platform without the opt-in does not offer it either.
    let answer: Answer = Arc::new(|request| match request {
        HelperRequest::Groups => groups(PlatformTier::Untested, false, true),
        _ => outputs(&[]),
    });
    let s = setup(answer, MutationLimits::default());
    let (code, _) = refused(&call(&s, restart(AuthorizationKind::Human, UNIT)).await);
    assert_eq!(code, RefusalCode::ContractUnknown);
    // The read is still offered.
    let _ = succeeded(call(&s, identity(UNIT)).await);
}

#[tokio::test]
async fn a_missing_helper_socket_is_contract_unknown() {
    let mut s = setup(offering(), MutationLimits::default());
    s.rig.ctx.helper = domovoi_agent::helper_client::HelperRelay::new(
        s.rig.fixture.dir.path().join("absent.sock"),
    );
    let (code, _) = refused(&call(&s, restart(AuthorizationKind::Human, UNIT)).await);
    assert_eq!(code, RefusalCode::ContractUnknown);
}

#[tokio::test]
async fn unit_identity_read_is_journaled_and_skips_the_limiter() {
    let s = setup(
        offering(),
        MutationLimits {
            per_hour: 0,
            per_capability: BTreeMap::new(),
        },
    );
    // Every mutation is blocked, yet reads still run, repeatedly.
    for _ in 0..3 {
        let out = succeeded(call(&s, identity(UNIT)).await);
        assert_eq!(out["id"], UNIT);
        assert_eq!(out["platform_tier"], "untested");
        assert_eq!(out["write_opt_in"], "true");
    }
    let run = identity(UNIT);
    let id = run.execution_id;
    succeeded(call(&s, run).await);
    let journal = s.rig.ctx.journal.clone().unwrap();
    let status = journal_state(&journal, id);
    assert!(matches!(status, ActionState::Finished { .. }), "{status:?}");
}

#[tokio::test]
async fn helper_tokens_come_back_as_a_refused_failure_and_tier_reasons_map_to_platform_tokens() {
    let refusing: Answer = Arc::new(|request| match request {
        HelperRequest::Groups => groups(PlatformTier::Untested, true, true),
        _ => HelperResponse::UnitRefused {
            token: "target_stale".to_owned(),
            detail: None,
        },
    });
    let s = setup(refusing, MutationLimits::default());
    assert_eq!(
        failed_message(call(&s, restart(AuthorizationKind::Human, UNIT)).await),
        "target_stale"
    );

    for (reason, token) in [
        (UnavailableReason::WriteGroupNeedsOptIn, "platform_untested"),
        (UnavailableReason::PlatformBlocked, "platform_blocked"),
    ] {
        let gated: Answer = Arc::new(move |request| match request {
            HelperRequest::Groups => groups(PlatformTier::Untested, true, true),
            _ => HelperResponse::Unavailable {
                group: HelperGroup::SystemdWrite,
                reason,
            },
        });
        let s = setup(gated, MutationLimits::default());
        assert_eq!(
            failed_message(call(&s, restart(AuthorizationKind::Human, UNIT)).await),
            token
        );
    }
}

#[tokio::test]
async fn arguments_are_parsed_before_anything_is_journaled() {
    let s = setup(offering(), MutationLimits::default());
    let mut bad = restart(AuthorizationKind::Human, UNIT);
    bad.args.insert("extra".to_owned(), "x".to_owned());
    let id = bad.execution_id;
    let answer = call(&s, bad).await;
    assert!(
        matches!(&answer, Message::Error(e) if e.code == "bad_request"),
        "{answer:?}"
    );
    let mut wrong_target = restart(AuthorizationKind::Human, UNIT);
    wrong_target.target = TargetKey::parse("other.service").unwrap();
    assert!(matches!(call(&s, wrong_target).await, Message::Error(e) if e.code == "bad_request"));
    let journal = s.rig.ctx.journal.clone().unwrap();
    assert!(matches!(journal_state(&journal, id), ActionState::NotFound));
}

#[tokio::test]
async fn an_expired_authorization_is_refused_before_any_work() {
    let s = setup(offering(), MutationLimits::default());
    let mut old = restart(AuthorizationKind::Human, UNIT);
    old.expires_at = T0;
    let (code, _) = refused(&call(&s, old).await);
    assert_eq!(code, RefusalCode::AuthorizationExpired);
}

#[tokio::test]
async fn policy_rate_counted_from_the_journal_survives_restart() {
    let s = setup(offering(), MutationLimits::default());
    let mut s = s;
    s.rig.ctx.ceiling = Some(Arc::new(Ceiling::from_section(&section(&[UNIT], 1, true))));
    succeeded(call(&s, restart(AuthorizationKind::Policy, UNIT)).await);

    // The agent restarts: a new journal over the same state directory still counts it.
    let state = s.rig.dirs.state_dir.clone();
    s.rig.ctx.journal = None;
    let reopened = Journal::open(&state, BOOT_ID, 1, MutationLimits::default()).unwrap();
    s.rig.ctx.journal = Some(Arc::new(reopened));
    let (code, detail) = refused(&call(&s, restart(AuthorizationKind::Policy, UNIT)).await);
    assert_eq!(code, RefusalCode::CeilingExceeded);
    assert!(detail.contains("hourly unattended limit"), "{detail}");
}

#[test]
fn ceiling_hash_and_summary_reach_the_wire_and_parse_as_the_hubs_summary() {
    use sha2::{Digest as _, Sha256};
    let ceiling = Ceiling::from_section(&section(&[UNIT], 2, true));
    let parsed: CeilingSummary = serde_json::from_str(ceiling.summary()).unwrap();
    assert!(parsed.enabled);
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].contract.as_str(), "unit.restart");
    assert_eq!(parsed.entries[0].max_per_hour, 2);
    assert_eq!(
        ceiling.hash().0,
        <[u8; 32]>::from(Sha256::digest(ceiling.summary().as_bytes()))
    );
    // Any change of the ceiling changes the hash.
    let other = Ceiling::from_section(&section(&[UNIT], 3, true));
    assert_ne!(ceiling.hash(), other.hash());
    // A wildcard target is written explicitly and reads back as `any`.
    let any = Ceiling::from_section(&section(&["*"], 2, true));
    assert!(any.summary().contains("\"any\""));
}
