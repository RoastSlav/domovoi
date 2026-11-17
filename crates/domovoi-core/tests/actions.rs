//! The action state machine: one test per arrow, plus approval and plan-hash rules.
//!
//! These tests construct `HumanApproval` directly; `xtask arch-check` allows that only in
//! test paths.

use std::collections::BTreeMap;

use domovoi_core::{
    ActionChange, ActionError, ActionId, ActionRequest, ActionStatus, ApprovalChannel,
    ApprovalEvidenceError, ApprovalProvenance, Authorization, AuthorizationKind, CheckKey,
    ExecutionId, HostId, HumanApproval, ImplHash, IncidentId, JournalEpoch, KeyFingerprint,
    ManifestHash, ParamKind, ParamName, ParamSpec, PlanBinding, PlanContract, PlanFamily, PlanHash,
    PlanTarget, Principal, Risk, SkillName, SkillSummary, StoredAction, StoredActionError, UserId,
    ValidatedArgs,
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
        entry_script_digest: domovoi_core::ScriptDigest::of_bytes(b"#!/bin/sh\nexit 0\n"),
        timeout: jiff::SignedDuration::from_secs(60),
        verify: None,
        readme: None,
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

/// A v2 target for `host` with a fixed enrolled key.
#[cfg(test)]
fn target(host: HostId) -> PlanTarget {
    PlanTarget::new(
        host,
        KeyFingerprint::of_public_key(&[7u8; 32]),
        CheckKey::parse("disk.usage").unwrap(),
        None,
    )
}

/// Approves `req` as `approval` did, bound to the plan it currently shows.
#[cfg(test)]
fn approve_with(
    req: &mut ActionRequest,
    approval: HumanApproval,
    now: Timestamp,
) -> Result<ActionChange, ActionError> {
    let binding = PlanBinding::Full(req.plan_hash_for_approval());
    req.approve(
        Authorization::human(approval),
        ApprovalProvenance::new(None, "test"),
        &binding,
        now,
    )
}

/// A request proposed at `at(0)` with a one-hour approval window.
#[cfg(test)]
fn proposed(risk: Risk) -> ActionRequest {
    let skill = summary(risk);
    ActionRequest::propose(
        IncidentId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0002)),
        &skill,
        &args(&skill, "plex"),
        Some(target(host())),
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        at(0),
    )
    .unwrap()
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
    approve_with(&mut req, web(true), at(60)).unwrap();
    req
}

#[cfg(test)]
fn dispatched() -> ActionRequest {
    let mut req = approved(Risk::Write);
    let hash = req.plan_hash_for_approval();
    req.dispatch(hash, None, at(120)).unwrap();
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
    let change = approve_with(&mut req, web(false), at(60)).unwrap();
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
    assert!(approve_with(&mut req, chat(), at(60)).is_ok());
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
    assert_eq!(
        approve_with(&mut req, web(true), at(3600)),
        Err(ActionError::Expired)
    );
    assert_eq!(req, before);
}

#[test]
fn destructive_needs_web_typed_confirmation() {
    for approval in [chat(), web(false)] {
        let mut req = proposed(Risk::Destructive);
        let before = req.clone();
        assert_eq!(
            approve_with(&mut req, approval, at(60)),
            Err(ActionError::DestructiveNeedsTypedConfirmation)
        );
        assert_eq!(req, before);
    }
    let mut req = proposed(Risk::Destructive);
    assert!(approve_with(&mut req, web(true), at(60)).is_ok());
}

#[test]
fn approved_dispatches_when_plan_hash_matches() {
    let mut req = approved(Risk::Write);
    let hash = req.plan_hash_for_approval();
    req.dispatch(hash, None, at(120)).unwrap();
    assert_eq!(
        req.status(),
        &ActionStatus::Dispatched { agent_epoch: None }
    );
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
    let change = req.dispatch(changed, None, at(120)).unwrap();
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
        ("approve", |r| approve_with(r, web(true), at(60))),
        ("deny", |r| r.deny(at(60))),
        ("expire", |r| r.expire(at(7200))),
        ("dispatch", |r| {
            let hash = r.plan_hash_for_approval();
            r.dispatch(hash, None, at(60))
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
fn dispatch_records_the_journal_epoch_the_hub_saw() {
    let mut req = approved(Risk::Write);
    let hash = req.plan_hash_for_approval();
    let epoch = JournalEpoch::from_uuid(uuid::Uuid::nil());
    req.dispatch(hash, Some(epoch), at(120)).unwrap();
    assert_eq!(
        req.status(),
        &ActionStatus::Dispatched {
            agent_epoch: Some(epoch),
        },
        "the run remembers which journal it was sent under",
    );
}

#[test]
fn an_unknown_outcome_is_terminal() {
    let mut req = dispatched();
    req.outcome_unknown("the journal lost the record".to_owned(), at(180))
        .unwrap();
    assert_eq!(
        req.status(),
        &ActionStatus::OutcomeUnknown {
            reason: "the journal lost the record".to_owned(),
        },
    );

    let settled = req.clone();
    for refused in [
        req.complete(Ok(()), at(200)),
        req.verified(true, at(200)),
        req.never_ran("late evidence".to_owned(), at(200)),
    ] {
        assert!(
            matches!(refused, Err(ActionError::NotAllowed { .. })),
            "an unknown outcome is never turned into anything else: {refused:?}",
        );
    }
    assert_eq!(req, settled, "a refused change leaves the request alone");
}

#[test]
fn never_ran_settles_an_approved_or_dispatched_run() {
    let mut approved_but_unsent = approved(Risk::Write);
    approved_but_unsent
        .never_ran("the approval expired".to_owned(), at(7200))
        .unwrap();
    assert_eq!(
        approved_but_unsent.status(),
        &ActionStatus::NotRun {
            reason: "the approval expired".to_owned(),
        },
        "an approval that closed before dispatch never ran",
    );

    let mut sent = dispatched();
    sent.never_ran("the agent has no record".to_owned(), at(180))
        .unwrap();
    assert_eq!(
        sent.status(),
        &ActionStatus::NotRun {
            reason: "the agent has no record".to_owned(),
        },
        "a journal with no record proves nothing started",
    );

    let mut undecided = proposed(Risk::Write);
    assert!(matches!(
        undecided.never_ran("nothing was decided".to_owned(), at(7200)),
        Err(ActionError::NotAllowed { .. }),
    ));
    assert_eq!(
        undecided.status(),
        &ActionStatus::Proposed,
        "a proposal nobody decided expires; it does not become a run that never happened",
    );
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
    assert_eq!(req.proposed_by(), &Principal::System("triage".into()));
}

#[test]
fn action_request_json_snapshot() {
    let id = ActionId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0004));
    let skill = summary(Risk::Write);
    let validated = args(&skill, "plex");
    let plan_hash = PlanHash::compute(&skill.name, skill.manifest_hash, host(), &validated);
    let plan_hash_v2 = PlanHash::compute_v2(
        &PlanContract::skill(&skill.name, 1).unwrap(),
        ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest),
        &target(host()),
        &validated,
        ExecutionId::from_action(id),
    );
    let req = ActionRequest::try_from_stored(StoredAction {
        id,
        incident: IncidentId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0002)),
        host: host(),
        skill: skill.name.clone(),
        risk: Risk::Write,
        args: validated.to_map(),
        plan_hash,
        plan_hash_v2: Some(plan_hash_v2),
        approved_plan_hash: Some(plan_hash_v2),
        provenance: Some(ApprovalProvenance::new(None, "test")),
        authorization_kind: Some(domovoi_core::AuthorizationKind::Human),
        status: ActionStatus::Approved,
        proposed_by: Principal::System("triage".into()),
        approved_by: Some(web(true)),
        expires_at: at(3600),
        created_at: at(0),
        updated_at: at(60),
        chat_token_hash: None,
    })
    .expect("the stored approval is consistent");
    insta::assert_json_snapshot!(req);
}

#[test]
fn approved_is_rejected_before_dispatch_only() {
    let mut req = approved(Risk::Write);
    let change = req.reject("loop guard".to_owned(), at(120)).unwrap();
    assert_eq!(
        change.to,
        ActionStatus::Rejected {
            reason: "loop guard".to_owned()
        }
    );
    assert!(matches!(
        req.reject("again".to_owned(), at(130)),
        Err(ActionError::NotAllowed {
            operation: "reject",
            ..
        })
    ));
    for mut other in [proposed(Risk::Write), dispatched(), succeeded()] {
        let before = other.status().clone();
        assert!(other.reject("no".to_owned(), at(200)).is_err());
        assert_eq!(other.status(), &before, "a refused reject changes nothing");
    }
}

#[test]
fn chat_token_hash_can_be_cleared() {
    let mut req = proposed(Risk::Write).with_chat_token_hash([7; 32]);
    assert_eq!(req.chat_token_hash(), Some(&[7; 32]));
    req.clear_chat_token_hash();
    assert_eq!(req.chat_token_hash(), None);
}

/// The stored form of `req`, for exercising the validated constructor.
#[cfg(test)]
fn stored(req: &ActionRequest) -> StoredAction {
    StoredAction {
        id: req.id(),
        incident: req.incident(),
        host: req.host(),
        skill: req.skill().clone(),
        risk: req.risk(),
        args: req.args().clone(),
        plan_hash: req.plan_hash(),
        plan_hash_v2: req.plan_hash_v2(),
        approved_plan_hash: req.approved_plan_hash(),
        provenance: req.provenance().cloned(),
        authorization_kind: req.authorization_kind(),
        status: req.status().clone(),
        proposed_by: req.proposed_by().clone(),
        approved_by: req.approved_by().cloned(),
        expires_at: req.expires_at(),
        created_at: req.created_at(),
        updated_at: req.updated_at(),
        chat_token_hash: req.chat_token_hash().copied(),
    }
}

/// A stored approval with the v1 family and the approval fields `hash` and `provenance`.
#[cfg(test)]
fn historical_v1(
    approved_plan_hash: Option<PlanHash>,
    provenance: Option<ApprovalProvenance>,
) -> StoredAction {
    let req = approved(Risk::Write);
    let mut stored = stored(&req);
    stored.plan_hash_v2 = None;
    stored.approved_plan_hash = approved_plan_hash;
    stored.provenance = provenance;
    stored
}

#[test]
fn a_valid_stored_v2_approval_loads() {
    let req = approved(Risk::Write);
    assert_eq!(
        ActionRequest::try_from_stored(stored(&req)).unwrap(),
        req,
        "a consistent v2 approval round-trips"
    );
}

#[test]
fn a_stored_v2_approval_without_a_displayed_hash_is_refused() {
    let mut stored = stored(&approved(Risk::Write));
    stored.approved_plan_hash = None;
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::Evidence(
            ApprovalEvidenceError::MissingDisplayedHash
        ))
    );
}

#[test]
fn a_stored_v2_approval_with_a_mismatched_hash_is_refused() {
    let mut stored = stored(&approved(Risk::Write));
    stored.approved_plan_hash = Some(stored.plan_hash);
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::Evidence(
            ApprovalEvidenceError::MismatchedDisplayedHash { family: "v2" }
        ))
    );
}

#[test]
fn a_stored_v2_approval_without_provenance_is_refused() {
    let mut stored = stored(&approved(Risk::Write));
    stored.provenance = None;
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::Evidence(
            ApprovalEvidenceError::MissingProvenance
        ))
    );
}

#[test]
fn a_stored_approval_without_an_approver_is_refused() {
    let mut stored = stored(&approved(Risk::Write));
    stored.approved_by = None;
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::MissingApprover)
    );
}

#[test]
fn a_proposed_row_with_approval_metadata_is_refused() {
    let mut stored = stored(&proposed(Risk::Write));
    stored.authorization_kind = Some(AuthorizationKind::Human);
    stored.approved_by = Some(web(true));
    stored.approved_plan_hash = stored.plan_hash_v2;
    stored.provenance = Some(ApprovalProvenance::new(None, "web"));
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::UndecidedApproval)
    );
}

#[test]
fn a_historical_v1_approval_without_displayed_fields_loads() {
    let loaded = ActionRequest::try_from_stored(historical_v1(None, None))
        .expect("a pre-migration approval is valid");
    assert_eq!(loaded.approval_plan().family(), PlanFamily::V1);
    assert_eq!(loaded.approved_plan_hash(), None);
    assert_eq!(loaded.provenance(), None);
}

#[test]
fn a_current_v1_approval_with_matching_hash_and_provenance_loads() {
    let req = approved(Risk::Write);
    let v1 = req.plan_hash();
    let loaded = ActionRequest::try_from_stored(historical_v1(
        Some(v1),
        Some(ApprovalProvenance::new(None, "web")),
    ))
    .expect("a current v1 approval with both fields is valid");
    assert_eq!(loaded.approved_plan_hash(), Some(v1));
}

#[test]
fn a_v1_approval_with_only_a_displayed_hash_is_refused() {
    let req = approved(Risk::Write);
    assert_eq!(
        ActionRequest::try_from_stored(historical_v1(Some(req.plan_hash()), None)),
        Err(StoredActionError::Evidence(
            ApprovalEvidenceError::MissingProvenance
        ))
    );
}

#[test]
fn a_v1_approval_with_only_provenance_is_refused() {
    assert_eq!(
        ActionRequest::try_from_stored(historical_v1(
            None,
            Some(ApprovalProvenance::new(None, "web")),
        )),
        Err(StoredActionError::Evidence(
            ApprovalEvidenceError::MissingDisplayedHash
        ))
    );
}

#[test]
fn a_v1_approval_with_a_mismatched_hash_is_refused() {
    let mut stored = historical_v1(
        Some(PlanHash::compute(
            &summary(Risk::Write).name,
            summary(Risk::Write).manifest_hash,
            host(),
            &args(&summary(Risk::Write), "sonarr"),
        )),
        Some(ApprovalProvenance::new(None, "web")),
    );
    stored.plan_hash = approved(Risk::Write).plan_hash();
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::Evidence(
            ApprovalEvidenceError::MismatchedDisplayedHash { family: "v1" }
        ))
    );
}

#[test]
fn a_policy_row_with_a_human_approver_is_refused() {
    let mut stored = stored(&approved(Risk::Write));
    stored.authorization_kind = Some(AuthorizationKind::Policy);
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::PolicyWithApprover)
    );
}

#[test]
fn a_policy_row_that_claims_to_have_run_is_refused() {
    let mut stored = stored(&approved(Risk::Write));
    stored.authorization_kind = Some(AuthorizationKind::Policy);
    stored.approved_by = None;
    stored.approved_plan_hash = None;
    stored.provenance = None;
    stored.status = ActionStatus::Dispatched { agent_epoch: None };
    assert_eq!(
        ActionRequest::try_from_stored(stored),
        Err(StoredActionError::PolicyNotApproved {
            status: "dispatched"
        })
    );
}

#[test]
fn an_approved_policy_row_loads_so_dispatch_can_refuse_it() {
    let mut stored = stored(&approved(Risk::Write));
    stored.authorization_kind = Some(AuthorizationKind::Policy);
    stored.approved_by = None;
    stored.approved_plan_hash = None;
    stored.provenance = None;
    assert!(
        ActionRequest::try_from_stored(stored).is_ok(),
        "the writer needs to load an approved policy row to refuse it by name"
    );
}
