//! `firewall.block_source` and `firewall.list_blocks` through `run_capability`: the ordered
//! refusals, the unattended ceiling, the limiter, the journal flow and the helper's answers.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_agent::link::RequestKind;
use domovoi_agent::requests::{catalog_entries, handle};
use domovoi_agent::unattended::Ceiling;
use domovoi_config::{Duration, MutationLimits, UnattendedAllow, UnattendedSection};
use domovoi_core::attest::ExpectedSafety;
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse};
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

const SOURCE: &str = "203.0.113.7/32";
const BLOCK_SOURCE: &str = "firewall.block_source";

fn groups_answer(nft_named: bool) -> HelperResponse {
    HelperResponse::Groups {
        enabled: vec![HelperGroup::SystemdRead],
        ignored: if nft_named {
            vec![HelperGroup::Nft]
        } else {
            Vec::new()
        },
        write_opt_in: true,
        platform_tier: PlatformTier::Untested,
        platform_record: PlatformRecordState::Ok,
    }
}

fn outputs(pairs: &[(&str, &str)]) -> HelperResponse {
    HelperResponse::Outputs {
        outputs: pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

/// A helper that offers the nft group and says yes to everything, with `active` blocks held.
fn offering(active: &'static str) -> Answer {
    Arc::new(move |request| match request {
        HelperRequest::Groups => groups_answer(true),
        HelperRequest::FirewallListBlocks => outputs(&[("active", active)]),
        HelperRequest::FirewallApply { .. }
        | HelperRequest::FirewallConfirm { .. }
        | HelperRequest::FirewallRollback { .. } => outputs(&[("phase", "ok")]),
        _ => HelperResponse::Health {
            protocol: 1,
            build: "t".to_owned(),
            pid: 1,
        },
    })
}

struct Setup {
    rig: Rig,
    stub: crate::helper_stub::Stub,
}

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
    Setup { rig, stub }
}

fn firewall_requests(s: &Setup) -> Vec<HelperRequest> {
    s.stub
        .requests()
        .into_iter()
        .filter(|r| {
            matches!(
                r,
                HelperRequest::FirewallApply { .. }
                    | HelperRequest::FirewallConfirm { .. }
                    | HelperRequest::FirewallRollback { .. }
                    | HelperRequest::FirewallListBlocks
            )
        })
        .collect()
}

fn verified() -> ExpectedSafety {
    serde_json::from_value(serde_json::json!({
        "state": "verified",
        "fingerprint": "aa".repeat(32),
        "boot_id": BOOT_ID,
        "generation": 1,
        "config_hash": "bb".repeat(32),
        "route_state_hash": "cc".repeat(32),
    }))
    .unwrap()
}

fn unverified() -> ExpectedSafety {
    serde_json::from_value(serde_json::json!({"state": "unverified", "reason": "key_not_pinned"}))
        .unwrap()
}

fn plan_hash() -> PlanHash {
    "09".repeat(32).parse().unwrap()
}

/// One phase of the block of `block` (which is also the execution id of an apply).
fn phase(
    kind: AuthorizationKind,
    phase: &str,
    block: Uuid,
    ttl_s: u32,
    safety: Option<ExpectedSafety>,
) -> RunCapability {
    let execution_id = if phase == "apply" {
        block
    } else {
        Uuid::now_v7()
    };
    RunCapability {
        execution_id,
        contract: BLOCK_SOURCE.to_owned(),
        version: 1,
        target: TargetKey::parse(&format!("ipnet:{SOURCE}")).unwrap(),
        args: BTreeMap::from([
            ("phase".to_owned(), phase.to_owned()),
            ("source".to_owned(), SOURCE.to_owned()),
            ("ttl_s".to_owned(), ttl_s.to_string()),
            ("block".to_owned(), block.to_string()),
            ("prefix_len_v4".to_owned(), "32".to_owned()),
            ("prefix_len_v6".to_owned(), "128".to_owned()),
        ]),
        plan_hash: plan_hash(),
        authorization_kind: kind,
        expires_at: T0 + SignedDuration::from_mins(5),
        expected_safety: safety,
    }
}

fn human(phase_name: &str, block: Uuid) -> RunCapability {
    phase(
        AuthorizationKind::Human,
        phase_name,
        block,
        600,
        Some(unverified()),
    )
}

fn policy(phase_name: &str, block: Uuid, ttl_s: u32) -> RunCapability {
    phase(
        AuthorizationKind::Policy,
        phase_name,
        block,
        ttl_s,
        Some(verified()),
    )
}

fn section(
    max_per_hour: u32,
    max_active: Option<u32>,
    max_ttl_s: Option<i64>,
) -> UnattendedSection {
    UnattendedSection {
        enabled: true,
        allow: vec![UnattendedAllow {
            capability: BLOCK_SOURCE.to_owned(),
            targets: vec!["*".to_owned()],
            max_per_hour,
            max_active,
            max_ttl: max_ttl_s.and_then(Duration::from_secs),
            args: Some(BTreeMap::from([
                ("prefix_len_v4".to_owned(), vec!["24".to_owned()]),
                ("prefix_len_v6".to_owned(), vec!["64".to_owned()]),
            ])),
        }],
    }
}

fn with_ceiling(s: &mut Setup, section: &UnattendedSection) {
    s.rig.ctx.ceiling = Some(Arc::new(Ceiling::from_section(section)));
}

async fn call(s: &Setup, run: RunCapability) -> Message {
    handle(&s.rig.ctx, &RequestKind::Capability(run)).await
}

fn refusal(message: &Message) -> (Option<RefusalCode>, String, String) {
    match message {
        Message::Refused(r) => (
            r.code.known().copied(),
            r.code.raw().to_owned(),
            r.detail.clone(),
        ),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn succeeded(message: Message) -> BTreeMap<String, String> {
    match message {
        Message::CapabilityResult(r) => match r.outcome {
            CapabilityOutcome::Succeeded { outputs } => outputs,
            CapabilityOutcome::Failed { message, .. } => panic!("failed: {message}"),
        },
        other => panic!("expected a result, got {other:?}"),
    }
}

fn failed_message(message: Message) -> String {
    match message {
        Message::CapabilityResult(r) => match r.outcome {
            CapabilityOutcome::Failed { message, .. } => message,
            CapabilityOutcome::Succeeded { .. } => panic!("it succeeded"),
        },
        other => panic!("expected a result, got {other:?}"),
    }
}

fn state_of(s: &Setup, id: Uuid) -> ActionState {
    let journal = s.rig.ctx.journal.clone().unwrap();
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

#[tokio::test]
async fn agent_refuses_unverified_with_policy_label() {
    let mut s = setup(offering("0"), MutationLimits::default());
    with_ceiling(&mut s, &section(5, None, None));
    let run = phase(
        AuthorizationKind::Policy,
        "apply",
        Uuid::now_v7(),
        600,
        Some(unverified()),
    );
    let (code, _, _) = refusal(&call(&s, run).await);
    assert_eq!(code, Some(RefusalCode::PolicyRequiresVerifiedSafety));
    assert!(
        firewall_requests(&s).is_empty(),
        "the helper was never asked"
    );
}

#[tokio::test]
async fn expected_safety_missing_is_refused_malformed_safety() {
    let s = setup(offering("0"), MutationLimits::default());
    for name in ["apply", "confirm", "rollback"] {
        let run = phase(AuthorizationKind::Human, name, Uuid::now_v7(), 600, None);
        let (known, raw, _) = refusal(&call(&s, run).await);
        assert_eq!(known, None, "{name}");
        assert_eq!(raw, "malformed_safety", "{name}");
    }
    assert!(firewall_requests(&s).is_empty());
}

#[tokio::test]
async fn mutation_limit_counts_apply_only() {
    let s = setup(
        offering("0"),
        MutationLimits {
            per_hour: 1,
            per_capability: BTreeMap::new(),
        },
    );
    let block = Uuid::now_v7();
    succeeded(call(&s, human("apply", block)).await);
    // The block's other phases never spend the budget, even with the hour spent.
    succeeded(call(&s, human("confirm", block)).await);
    succeeded(call(&s, human("rollback", block)).await);
    // A second block is a second apply and the limiter refuses it.
    let (code, _, _) = refusal(&call(&s, human("apply", Uuid::now_v7())).await);
    assert_eq!(code, Some(RefusalCode::RateLimited));
}

#[tokio::test]
async fn the_hourly_ceiling_counts_apply_only() {
    let mut s = setup(offering("0"), MutationLimits::default());
    with_ceiling(&mut s, &section(1, None, None));
    let block = Uuid::now_v7();
    succeeded(call(&s, policy("apply", block, 600)).await);
    succeeded(call(&s, policy("confirm", block, 600)).await);
    let (code, _, detail) = refusal(&call(&s, policy("apply", Uuid::now_v7(), 600)).await);
    assert_eq!(code, Some(RefusalCode::CeilingExceeded));
    assert!(detail.contains("hourly unattended limit"), "{detail}");
}

#[tokio::test]
async fn agent_limit_stricter_than_helper_applies() {
    let mut s = setup(offering("1"), MutationLimits::default());
    // A lifetime above the agent's own maximum is refused before the helper is asked.
    with_ceiling(&mut s, &section(5, None, Some(60)));
    let (code, _, detail) = refusal(&call(&s, policy("apply", Uuid::now_v7(), 120)).await);
    assert_eq!(code, Some(RefusalCode::CeilingExceeded));
    assert!(detail.contains("lifetime"), "{detail}");
    assert!(firewall_requests(&s).is_empty());

    // So is a block once the helper's own count has reached the agent's `max_active`.
    with_ceiling(&mut s, &section(5, Some(1), None));
    let (code, _, detail) = refusal(&call(&s, policy("apply", Uuid::now_v7(), 600)).await);
    assert_eq!(code, Some(RefusalCode::CeilingExceeded));
    assert!(detail.contains("active limit"), "{detail}");
    assert!(
        firewall_requests(&s)
            .iter()
            .all(|r| matches!(r, HelperRequest::FirewallListBlocks)),
        "only the count was asked"
    );
}

#[tokio::test]
async fn agent_limit_above_helper_is_refused() {
    // The agent allows an hour; the helper's maximum is lower and says so. Nothing is clamped.
    let answer: Answer = Arc::new(|request| match request {
        HelperRequest::Groups => groups_answer(true),
        HelperRequest::FirewallApply { .. } => HelperResponse::ContainRefused {
            token: "ttl_above_maximum".to_owned(),
            detail: Some("the helper allows at most 600s".to_owned()),
        },
        _ => outputs(&[]),
    });
    let mut s = setup(answer, MutationLimits::default());
    with_ceiling(&mut s, &section(5, None, Some(3600)));
    let message = failed_message(call(&s, policy("apply", Uuid::now_v7(), 1800)).await);
    assert_eq!(message, "ttl_above_maximum: the helper allows at most 600s");
    let sent = firewall_requests(&s);
    assert!(
        matches!(
            &sent[..],
            [HelperRequest::FirewallApply { ttl_s: 1800, .. }]
        ),
        "the ttl went to the helper unchanged: {sent:?}"
    );
}

#[tokio::test]
async fn a_prefix_floor_below_the_ceiling_is_refused_for_a_policy_label() {
    let mut s = setup(offering("0"), MutationLimits::default());
    let mut narrow = section(5, None, None);
    narrow.allow[0].args = Some(BTreeMap::from([
        ("prefix_len_v4".to_owned(), vec!["33".to_owned()]),
        ("prefix_len_v6".to_owned(), vec!["64".to_owned()]),
    ]));
    with_ceiling(&mut s, &narrow);
    let (code, _, detail) = refusal(&call(&s, policy("apply", Uuid::now_v7(), 600)).await);
    assert_eq!(code, Some(RefusalCode::CeilingExceeded));
    assert!(detail.contains("argument"), "{detail}");
}

#[tokio::test]
async fn confirm_and_rollback_need_a_finished_apply_under_the_same_block() {
    let s = setup(offering("0"), MutationLimits::default());
    for name in ["confirm", "rollback"] {
        let (_, raw, _) = refusal(&call(&s, human(name, Uuid::now_v7())).await);
        assert_eq!(raw, "unknown_block", "{name}");
    }
    assert!(firewall_requests(&s).is_empty());
}

#[tokio::test]
async fn a_repeated_execution_id_is_answered_from_the_journal() {
    let s = setup(offering("0"), MutationLimits::default());
    let run = human("apply", Uuid::now_v7());
    let id = run.execution_id;
    let first = succeeded(call(&s, run.clone()).await);
    let again = succeeded(call(&s, run).await);
    assert_eq!(first, again);
    assert_eq!(firewall_requests(&s).len(), 1, "the helper ran it once");
    assert!(matches!(state_of(&s, id), ActionState::Finished { .. }));
}

#[tokio::test]
async fn arguments_are_parsed_before_anything_is_journaled() {
    let s = setup(offering("0"), MutationLimits::default());
    let mut run = human("apply", Uuid::now_v7());
    let id = run.execution_id;
    run.args.insert("prefix_len_v4".to_owned(), "24".to_owned());
    match call(&s, run.clone()).await {
        Message::Error(e) => assert_eq!(e.code, "bad_request"),
        other => panic!("expected bad_request, got {other:?}"),
    }
    run.args.insert("prefix_len_v4".to_owned(), "32".to_owned());
    run.args
        .insert("source".to_owned(), "203.0.113.7".to_owned());
    match call(&s, run).await {
        Message::Error(e) => assert_eq!(e.code, "bad_request"),
        other => panic!("expected bad_request, got {other:?}"),
    }
    assert!(matches!(state_of(&s, id), ActionState::NotFound));
    assert!(firewall_requests(&s).is_empty());
}

#[tokio::test]
async fn helper_tokens_come_back_as_a_refused_failure() {
    let answer: Answer = Arc::new(|request| match request {
        HelperRequest::Groups => groups_answer(true),
        _ => HelperResponse::ContainRefused {
            token: "exclusion_overlap".to_owned(),
            detail: None,
        },
    });
    let s = setup(answer, MutationLimits::default());
    let message = failed_message(call(&s, human("apply", Uuid::now_v7())).await);
    assert_eq!(message, "exclusion_overlap");
}

#[tokio::test]
async fn a_missing_helper_socket_is_contract_unknown() {
    let mut s = setup(offering("0"), MutationLimits::default());
    s.rig.ctx.helper = domovoi_agent::helper_client::HelperRelay::new(
        s.rig.fixture.dir.path().join("absent.sock"),
    );
    let (code, _, _) = refusal(&call(&s, human("apply", Uuid::now_v7())).await);
    assert_eq!(code, Some(RefusalCode::ContractUnknown));
}

#[tokio::test]
async fn a_helper_without_the_nft_group_is_contract_unknown() {
    let answer: Answer = Arc::new(|request| match request {
        HelperRequest::Groups => groups_answer(false),
        _ => outputs(&[]),
    });
    let s = setup(answer, MutationLimits::default());
    let (code, _, detail) = refusal(&call(&s, human("apply", Uuid::now_v7())).await);
    assert_eq!(code, Some(RefusalCode::ContractUnknown));
    assert!(detail.contains("nft"), "{detail}");
    assert!(firewall_requests(&s).is_empty());
}

#[tokio::test]
async fn listing_blocks_needs_a_human_and_is_not_limited() {
    let s = setup(
        offering("2"),
        MutationLimits {
            per_hour: 0,
            per_capability: BTreeMap::new(),
        },
    );
    let list = |kind| RunCapability {
        contract: "firewall.list_blocks".to_owned(),
        args: BTreeMap::new(),
        target: TargetKey::parse("host:alpha")
            .unwrap_or_else(|_| TargetKey::parse("ipnet:203.0.113.7/32").unwrap()),
        authorization_kind: kind,
        expected_safety: None,
        ..phase(AuthorizationKind::Human, "apply", Uuid::now_v7(), 600, None)
    };
    let (code, _, _) = refusal(&call(&s, list(AuthorizationKind::Policy)).await);
    assert_eq!(code, Some(RefusalCode::AllowRiskDenied));
    for _ in 0..3 {
        let out = succeeded(call(&s, list(AuthorizationKind::Human)).await);
        assert_eq!(out["active"], "2");
    }
}

#[test]
fn helper_path_contracts_are_not_in_the_agent_catalogue() {
    let rig = Rig::new(
        &[],
        Options {
            section: false,
            allow_install: false,
            limits: MutationLimits::default(),
        },
    );
    let entries = catalog_entries(
        &rig.ctx.catalog,
        domovoi_agent::packages::PackageSurface::default(),
    );
    assert!(
        entries
            .keys()
            .all(|id| !id.as_str().starts_with("firewall.")),
        "availability of a helper contract comes from the helper's status, not the catalogue"
    );
}

#[test]
fn the_helper_status_lists_the_four_nft_operations_only_when_offered() {
    use domovoi_agent::helper_client::GroupsInfo;
    let info = |tier, opt_in, named: bool| GroupsInfo {
        enabled: vec![HelperGroup::SystemdRead],
        ignored: if named {
            vec![HelperGroup::Nft]
        } else {
            Vec::new()
        },
        write_opt_in: opt_in,
        platform_tier: tier,
        platform_record: PlatformRecordState::Ok,
    };
    let ops = info(PlatformTier::Untested, true, true).nft_ops();
    assert_eq!(ops, domovoi_core::containment::ops::NFT_OPS.to_vec());
    assert!(
        info(PlatformTier::Untested, false, true)
            .nft_ops()
            .is_empty()
    );
    assert!(
        info(PlatformTier::Supported, false, false)
            .nft_ops()
            .is_empty()
    );
}
