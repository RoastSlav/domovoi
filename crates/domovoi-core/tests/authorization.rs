//! `Authorization` and `PolicyPermit`: the only construction paths and the policy refusal.

use domovoi_core::{
    ApprovalChannel, Authorization, AuthorizationKind, HumanApproval, PermitError, PlanHash,
    PolicyPermit, PolicyPermitParts, UserId,
};
use jiff::{SignedDuration, Timestamp};

#[cfg(test)]
fn approval() -> HumanApproval {
    HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Web {
            typed_confirmation: false,
        },
        Timestamp::UNIX_EPOCH,
    )
}

#[cfg(test)]
fn permit(decision: &str, version: &str, issued: Timestamp, expires: Timestamp) -> PolicyPermit {
    PolicyPermit::issue(PolicyPermitParts {
        decision_id: decision,
        rule_ids: vec!["b".to_owned(), "a".to_owned(), "a".to_owned()],
        policy_version: version,
        plan_hash: "ab".repeat(32).parse().unwrap(),
        issued_at: issued,
        expires_at: expires,
        ceiling_hash: &"cd".repeat(32),
        safety_identity: None,
    })
    .unwrap()
}

#[test]
fn authorization_has_no_public_variants() {
    let human = Authorization::human(approval());
    assert_eq!(human.kind(), AuthorizationKind::Human);
    assert!(human.human_approval().is_some());
    assert!(human.policy_permit().is_none());

    let policy = Authorization::policy(permit(
        "dec-1",
        "1",
        Timestamp::UNIX_EPOCH,
        Timestamp::UNIX_EPOCH + SignedDuration::from_hours(1),
    ));
    assert_eq!(policy.kind(), AuthorizationKind::Policy);
    assert!(policy.policy_permit().is_some());
    assert!(policy.human_approval().is_none());
}

#[test]
fn policy_permit_holds_the_decision_it_packs() {
    let permit = permit(
        "dec-1",
        "7",
        Timestamp::UNIX_EPOCH,
        Timestamp::UNIX_EPOCH + SignedDuration::from_hours(1),
    );
    assert_eq!(permit.decision_id(), "dec-1");
    assert_eq!(permit.plan_hash(), "ab".repeat(32).parse().unwrap());
    assert_eq!(
        permit.expires_at(),
        Timestamp::UNIX_EPOCH + SignedDuration::from_hours(1)
    );
}

#[test]
fn policy_permit_refuses_an_empty_identifier_or_a_bad_window() {
    let now = Timestamp::UNIX_EPOCH;
    assert_eq!(
        PolicyPermit::issue(PolicyPermitParts {
            decision_id: "",
            rule_ids: Vec::new(),
            policy_version: "1",
            plan_hash: "ab".repeat(32).parse().unwrap(),
            issued_at: now,
            expires_at: now + SignedDuration::from_hours(1),
            ceiling_hash: "cd",
            safety_identity: None,
        }),
        Err(PermitError::EmptyDecisionId)
    );
    assert_eq!(
        PolicyPermit::issue(PolicyPermitParts {
            decision_id: "d",
            rule_ids: Vec::new(),
            policy_version: "",
            plan_hash: "ab".repeat(32).parse().unwrap(),
            issued_at: now,
            expires_at: now + SignedDuration::from_hours(1),
            ceiling_hash: "cd",
            safety_identity: None,
        }),
        Err(PermitError::EmptyPolicyVersion)
    );
    assert_eq!(
        PolicyPermit::issue(PolicyPermitParts {
            decision_id: "d",
            rule_ids: vec![String::new()],
            policy_version: "1",
            plan_hash: "ab".repeat(32).parse().unwrap(),
            issued_at: now,
            expires_at: now + SignedDuration::from_hours(1),
            ceiling_hash: "cd",
            safety_identity: None,
        }),
        Err(PermitError::EmptyRuleId)
    );
    assert_eq!(
        PolicyPermit::issue(PolicyPermitParts {
            decision_id: "d",
            rule_ids: Vec::new(),
            policy_version: "1",
            plan_hash: "ab".repeat(32).parse::<PlanHash>().unwrap(),
            issued_at: now,
            expires_at: now,
            ceiling_hash: "cd",
            safety_identity: None,
        }),
        Err(PermitError::ExpiresNotAfterIssued)
    );
}

#[test]
fn policy_permit_restore_matches_issue() {
    let now = Timestamp::UNIX_EPOCH;
    let issued = permit("dec-1", "7", now, now + SignedDuration::from_hours(1));
    let restored = PolicyPermit::restore(PolicyPermitParts {
        decision_id: issued.decision_id(),
        rule_ids: issued.rule_ids().to_vec(),
        policy_version: issued.policy_version(),
        plan_hash: issued.plan_hash(),
        issued_at: issued.issued_at(),
        expires_at: issued.expires_at(),
        ceiling_hash: issued.ceiling_hash(),
        safety_identity: issued.safety_identity(),
    })
    .unwrap();
    assert_eq!(restored, issued);
    assert_eq!(restored.rule_ids(), ["a", "b"]);
}
