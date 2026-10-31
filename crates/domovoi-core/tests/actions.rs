//! The action state machine: one test per arrow, plus approval and plan-hash rules.
//!
//! These tests construct `HumanApproval` directly; `xtask arch-check` allows that only in
//! test paths.

use std::collections::BTreeMap;

use domovoi_core::{
    ActionChange, ActionError, ActionId, ActionRequest, ActionStatus, ApprovalChannel, HostId,
    HumanApproval, IncidentId, ManifestHash, ParamKind, ParamName, ParamSpec, PlanHash, Principal,
    Risk, SkillName, SkillSummary, UserId, ValidatedArgs,
};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

#[cfg(test)]
fn at(secs: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + secs).unwrap()
}

#[cfg(test)]
fn summary(risk: Risk) -> SkillSummary {
    SkillSummary {
        name: SkillName::parse("restart_container").unwrap(),
        description: "Restart a container".to_owned(),
        risk,
        params: vec![ParamSpec {
            name: ParamName::parse("container").unwrap(),
            description: "Container name".to_owned(),
            required: true,
            kind: ParamKind::String {
                pattern: "^[a-z0-9_.-]{1,64}$".to_owned(),
                max_len: 64,
            },
        }],
        applies_to: Vec::new(),
        manifest_hash: "ab".repeat(32).parse().unwrap(),
    }
}

#[cfg(test)]
fn args(skill: &SkillSummary, container: &str) -> ValidatedArgs {
    let mut map = BTreeMap::new();
    map.insert("container".to_owned(), container.to_owned());
    skill.validate_args(&map).unwrap()
}

fn host() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001))
}

/// A request proposed at `at(0)` with a one-hour approval window.
#[cfg(test)]
fn proposed(risk: Risk) -> ActionRequest {
    let skill = summary(risk);
    ActionRequest::propose(
        IncidentId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0002)),
        host(),
        &skill,
        &args(&skill, "plex"),
        Principal::System("triage"),
        SignedDuration::from_hours(1),
        at(0),
    )
}

fn web(typed: bool) -> HumanApproval {
    HumanApproval::new(
        UserId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0003)),
        ApprovalChannel::Web {
            typed_confirmation: typed,
        },
        at(60),
    )
}

fn chat() -> HumanApproval {
    HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Chat {
            platform: "discord".to_owned(),
            platform_user: "1234".to_owned(),
        },
        at(60),
    )
}

#[cfg(test)]
fn approved(risk: Risk) -> ActionRequest {
    let mut req = proposed(risk);
    req.approve(web(true), at(60)).unwrap();
    req
}

#[cfg(test)]
fn dispatched() -> ActionRequest {
    let mut req = approved(Risk::Write);
    let hash = req.plan_hash();
    req.dispatch(hash, at(120)).unwrap();
    req
}

#[cfg(test)]
fn succeeded() -> ActionRequest {
    let mut req = dispatched();
    req.complete(Ok(()), at(180)).unwrap();
    req
}

#[test]
fn proposed_is_approved_by_a_human() {
    let mut req = proposed(Risk::Write);
    let change = req.approve(web(false), at(60)).unwrap();
    assert_eq!(
        change,
        ActionChange {
            from: ActionStatus::Proposed,
            to: ActionStatus::Approved
        }
    );
    assert_eq!(req.approved_by(), Some(&web(false)));
    assert_eq!(req.updated_at(), at(60));
}

#[test]
fn write_skill_can_be_approved_from_chat() {
    let mut req = proposed(Risk::Write);
    assert!(req.approve(chat(), at(60)).is_ok());
}

#[test]
fn proposed_is_denied() {
    let mut req = proposed(Risk::Write);
    req.deny(at(60)).unwrap();
    assert_eq!(req.status(), &ActionStatus::Denied);
}

#[test]
fn proposed_expires_after_the_window() {
    let mut req = proposed(Risk::Write);
    assert_eq!(req.expire(at(3599)), Err(ActionError::NotYetExpired));
    req.expire(at(3600)).unwrap();
    assert_eq!(req.status(), &ActionStatus::Expired);
}

#[test]
fn approve_after_expiry_fails() {
    let mut req = proposed(Risk::Write);
    let before = req.clone();
    assert_eq!(req.approve(web(true), at(3600)), Err(ActionError::Expired));
    assert_eq!(req, before);
}

#[test]
fn destructive_needs_web_typed_confirmation() {
    for approval in [chat(), web(false)] {
        let mut req = proposed(Risk::Destructive);
        let before = req.clone();
        assert_eq!(
            req.approve(approval, at(60)),
            Err(ActionError::DestructiveNeedsTypedConfirmation)
        );
        assert_eq!(req, before);
    }
    let mut req = proposed(Risk::Destructive);
    assert!(req.approve(web(true), at(60)).is_ok());
}

#[test]
fn approved_dispatches_when_plan_hash_matches() {
    let mut req = approved(Risk::Write);
    let hash = req.plan_hash();
    req.dispatch(hash, at(120)).unwrap();
    assert_eq!(req.status(), &ActionStatus::Dispatched);
}

#[test]
fn dispatch_with_different_plan_hash_rejects() {
    let mut req = approved(Risk::Write);
    let skill = summary(Risk::Write);
    let changed = PlanHash::compute(
        &skill.name,
        skill.manifest_hash,
        host(),
        &args(&skill, "sonarr"),
    );
    let change = req.dispatch(changed, at(120)).unwrap();
    assert!(matches!(change.to, ActionStatus::Rejected { .. }));
    assert!(matches!(req.status(), ActionStatus::Rejected { .. }));
}

#[test]
fn dispatched_succeeds_or_fails() {
    let mut ok = dispatched();
    ok.complete(Ok(()), at(180)).unwrap();
    assert_eq!(ok.status(), &ActionStatus::Succeeded);

    let mut failed = dispatched();
    failed
        .complete(Err("exit code 1".to_owned()), at(180))
        .unwrap();
    assert_eq!(
        failed.status(),
        &ActionStatus::Failed {
            reason: "exit code 1".to_owned()
        }
    );
}

#[test]
fn succeeded_is_verified_or_fails_verification() {
    let mut ok = succeeded();
    ok.verified(true, at(240)).unwrap();
    assert_eq!(ok.status(), &ActionStatus::Verified);

    let mut bad = succeeded();
    bad.verified(false, at(240)).unwrap();
    assert_eq!(bad.status(), &ActionStatus::VerifyFailed);
}

#[test]
fn illegal_operations_leave_request_unchanged() {
    type Op = fn(&mut ActionRequest) -> Result<ActionChange, ActionError>;
    let ops: [(&str, Op); 6] = [
        ("approve", |r| r.approve(web(true), at(60))),
        ("deny", |r| r.deny(at(60))),
        ("expire", |r| r.expire(at(7200))),
        ("dispatch", |r| {
            let hash = r.plan_hash();
            r.dispatch(hash, at(60))
        }),
        ("complete", |r| r.complete(Ok(()), at(60))),
        ("verify", |r| r.verified(true, at(60))),
    ];
    // (state name, request, operations that are legal from it)
    let states: Vec<(&str, ActionRequest, &[&str])> = vec![
        (
            "proposed",
            proposed(Risk::Write),
            &["approve", "deny", "expire"],
        ),
        ("approved", approved(Risk::Write), &["dispatch"]),
        ("dispatched", dispatched(), &["complete"]),
        ("succeeded", succeeded(), &["verify"]),
        (
            "denied",
            {
                let mut r = proposed(Risk::Write);
                r.deny(at(1)).unwrap();
                r
            },
            &[],
        ),
    ];
    for (state, base, legal) in states {
        for (op_name, op) in ops {
            if legal.contains(&op_name) {
                continue;
            }
            let mut req = base.clone();
            let result = op(&mut req);
            assert!(
                matches!(result, Err(ActionError::NotAllowed { .. })),
                "{op_name} from {state}: {result:?}"
            );
            assert_eq!(req, base, "{op_name} from {state} changed the request");
        }
    }
}

#[test]
fn plan_hash_changes_with_args_manifest_and_host() {
    let skill = summary(Risk::Write);
    let base = PlanHash::compute(
        &skill.name,
        skill.manifest_hash,
        host(),
        &args(&skill, "plex"),
    );
    assert_eq!(
        base,
        PlanHash::compute(
            &skill.name,
            skill.manifest_hash,
            host(),
            &args(&skill, "plex")
        )
    );
    let other_args = PlanHash::compute(
        &skill.name,
        skill.manifest_hash,
        host(),
        &args(&skill, "sonarr"),
    );
    let other_manifest: ManifestHash = "cd".repeat(32).parse().unwrap();
    let other_manifest =
        PlanHash::compute(&skill.name, other_manifest, host(), &args(&skill, "plex"));
    let other_host = PlanHash::compute(
        &skill.name,
        skill.manifest_hash,
        HostId::new(),
        &args(&skill, "plex"),
    );
    let other_skill = PlanHash::compute(
        &SkillName::parse("stop_container").unwrap(),
        skill.manifest_hash,
        host(),
        &args(&skill, "plex"),
    );
    for (what, hash) in [
        ("args", other_args),
        ("manifest", other_manifest),
        ("host", other_host),
        ("skill", other_skill),
    ] {
        assert_ne!(hash, base, "plan hash ignores {what}");
    }
    assert_eq!(proposed(Risk::Write).plan_hash(), base);
}

#[test]
fn propose_copies_risk_and_normalised_args() {
    let req = proposed(Risk::Destructive).with_chat_token_hash([7; 32]);
    assert_eq!(req.risk(), Risk::Destructive);
    assert_eq!(
        req.args().get("container").map(String::as_str),
        Some("plex")
    );
    assert_eq!(req.expires_at(), at(3600));
    assert_eq!(req.chat_token_hash(), Some(&[7; 32]));
    assert_eq!(req.proposed_by(), Principal::System("triage"));
}

#[test]
fn action_request_json_snapshot() {
    let req = approved(Risk::Write).with_id(ActionId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0004,
    )));
    insta::assert_json_snapshot!(req);
}
