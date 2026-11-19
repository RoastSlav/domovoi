//! Replaying recorded decisions, locked-rule differences and the preview hash.

use domovoi_core::policy_store::{ShadowRecord, ShadowStatus};
use domovoi_engine::policy::{
    CapabilityScope, Condition, Finding, FindingKind, Outcome, Rule, RuleSelection, TimeOfDay,
    evaluate, evaluate_shadow,
};
use domovoi_engine::policy_shadow::{
    CandidateDoc, Comparison, DecisionEnvelope, InputsEnvelope, LockChange, LockChangeKind, build,
    compare_with_record, finding_key, locked_changes, preview_hash, replay, summarise,
};

use crate::policy_fixtures::rid;
use crate::policy_shadow_support::{env, rule, shadow_rule, skill_candidate, skill_rule, t};

/// Records the skill step under `rules` the way the recorder does, without a store.
async fn record(rules: &[Rule], secs: i64) -> ShadowRecord {
    let step = skill_candidate();
    let built = build(&step, rules, &env(), t(secs)).await;
    let request = built.request.unwrap();
    let enforced = DecisionEnvelope::from_result(&evaluate(rules, &request));
    let shadow = DecisionEnvelope::from_result(&evaluate_shadow(rules, &request));
    ShadowRecord {
        execution: step.execution,
        operation: step.operation,
        host: step.host,
        contract: step.contract.to_string(),
        step_created_at: step.created_at,
        evaluated_at: t(secs),
        status: ShadowStatus::Evaluated,
        reason: None,
        policy_version: 1,
        rules_hash: "0".repeat(64),
        enforced_outcome: Some(enforced.outcome_text().to_owned()),
        shadow_outcome: Some(shadow.outcome_text().to_owned()),
        degraded: !built.gaps.is_empty(),
        candidate: serde_json::to_string(&CandidateDoc::from_candidate(&step)).unwrap(),
        inputs: Some(serde_json::to_string(&InputsEnvelope::new(built.inputs.unwrap())).unwrap()),
        enforced: Some(enforced.to_json().unwrap()),
        shadow: Some(shadow.to_json().unwrap()),
        gaps: serde_json::to_string(&built.gaps).unwrap(),
    }
}

#[tokio::test]
async fn replay_reproduces_every_recorded_decision() {
    let rules = vec![
        skill_rule("enforced-allow", Outcome::Allow),
        shadow_rule("shadow-deny", Outcome::Deny),
    ];
    let row = record(&rules, 0).await;
    for selection in [RuleSelection::Enforced, RuleSelection::Shadow] {
        let replayed = replay(&rules, &row, selection);
        assert_eq!(replayed.comparison, Comparison::Same, "{selection:?}");
        assert_eq!(replayed.baseline.as_ref(), Some(&replayed.simulated));
    }
    let none = record(&[], 0).await;
    assert_eq!(
        replay(&[], &none, RuleSelection::Enforced).comparison,
        Comparison::Same
    );
}

#[tokio::test]
async fn replay_with_a_changed_rule_reports_the_difference() {
    let before = vec![skill_rule("r", Outcome::Allow)];
    let row = record(&before, 0).await;
    let after = vec![skill_rule("r", Outcome::Deny)];
    let changed = replay(&after, &row, RuleSelection::Enforced);
    assert_eq!(changed.comparison, Comparison::Changed);
    let unrelated = Rule {
        capability: CapabilityScope::Contract(
            domovoi_core::capability::ContractId::parse("skill.other").unwrap(),
        ),
        ..skill_rule("x", Outcome::Deny)
    };
    let same = replay(
        &[before[0].clone(), unrelated],
        &row,
        RuleSelection::Enforced,
    );
    assert_eq!(same.comparison, Comparison::Same);

    let mut broken = row.clone();
    broken.enforced = Some("not json".to_owned());
    assert_eq!(
        replay(&before, &broken, RuleSelection::Enforced).comparison,
        Comparison::NotReplayable
    );
    let mut skipped = row;
    skipped.status = ShadowStatus::Skipped;
    assert_eq!(
        replay(&before, &skipped, RuleSelection::Enforced).comparison,
        Comparison::NotReplayable
    );

    let summary = summarise(&[changed, same]);
    assert_eq!(
        (summary.evaluated, summary.same, summary.changed),
        (2, 1, 1)
    );
    assert_eq!(summary.newly_denied, 1);
    assert_eq!(summary.newly_allowed + summary.newly_requires_approval, 0);
}

#[tokio::test]
async fn current_inputs_mode_uses_the_clock_not_the_recorded_instant() {
    // The window holds at 21:00 UTC and not at 23:00 UTC.
    let window = Rule {
        conditions: vec![Condition::TimeWindow {
            start: TimeOfDay {
                hour: 20,
                minute: 0,
            },
            end: TimeOfDay {
                hour: 22,
                minute: 0,
            },
        }],
        ..skill_rule("evening", Outcome::Allow)
    };
    let rules = vec![window];
    // `t` counts from a fixed instant; align both readings to the wall clock explicitly.
    let midnight = 1_700_000_000 - 1_700_000_000 % 86_400;
    let at_21 = t(midnight - 1_700_000_000 + 21 * 3600);
    let at_23 = t(midnight - 1_700_000_000 + 23 * 3600);
    let row_inputs = build(&skill_candidate(), &rules, &env(), at_21).await;
    let request = row_inputs.request.clone().unwrap();
    let enforced = DecisionEnvelope::from_result(&evaluate(&rules, &request));
    let mut row = record(&rules, 0).await;
    row.inputs =
        Some(serde_json::to_string(&InputsEnvelope::new(row_inputs.inputs.unwrap())).unwrap());
    row.enforced = Some(enforced.to_json().unwrap());
    // Recorded inputs replay at the recorded instant.
    assert_eq!(
        replay(&rules, &row, RuleSelection::Enforced).comparison,
        Comparison::Same
    );
    // Current inputs are rebuilt at a later clock reading and compared with the row.
    let built = build(&skill_candidate(), &rules, &env(), at_23).await;
    let result = evaluate(&rules, &built.request.unwrap());
    let current = compare_with_record(
        &row,
        RuleSelection::Enforced,
        DecisionEnvelope::from_result(&result),
    );
    assert_eq!(current.comparison, Comparison::Changed);
}

fn locked(id: &str) -> Rule {
    Rule {
        locked: true,
        ..rule(id, Outcome::Deny)
    }
}

fn kinds(base: &[Rule], next: &[Rule]) -> Vec<(String, LockChangeKind)> {
    locked_changes(base, next)
        .into_iter()
        .map(|c| (c.rule.to_string(), c.change))
        .collect()
}

#[test]
fn locked_changes_classify_created_edited_unlocked_removed() {
    use LockChangeKind::{Created, Edited, Removed, Unlocked};
    let lock = locked("lock");
    assert_eq!(
        kinds(&[], std::slice::from_ref(&lock)),
        vec![("lock".into(), Created)]
    );
    let edited = Rule {
        note: "reworded".to_owned(),
        ..lock.clone()
    };
    assert_eq!(
        kinds(std::slice::from_ref(&lock), &[edited]),
        vec![("lock".into(), Edited)]
    );
    let disabled = Rule {
        enabled: false,
        ..lock.clone()
    };
    assert_eq!(
        kinds(std::slice::from_ref(&lock), &[disabled]),
        vec![("lock".into(), Edited)]
    );
    let unlocked = Rule {
        locked: false,
        ..lock.clone()
    };
    assert_eq!(
        kinds(std::slice::from_ref(&lock), &[unlocked]),
        vec![("lock".into(), Unlocked)]
    );
    assert_eq!(
        kinds(std::slice::from_ref(&lock), &[]),
        vec![("lock".into(), Removed)]
    );
    // An unrelated, unlocked edit is not a lock change.
    let plain = rule("plain", Outcome::Allow);
    let tweaked = Rule {
        note: "x".to_owned(),
        ..plain.clone()
    };
    assert!(kinds(&[plain, lock.clone()], &[tweaked, lock]).is_empty());
}

fn finding(kind: FindingKind) -> Finding {
    Finding {
        kind,
        first: rid("rule-a"),
        second: rid("rule-b"),
        resolution: Some(Outcome::Deny),
        shadow: false,
        note: "n".to_owned(),
        requires_acknowledgement: true,
    }
}

#[test]
fn finding_keys_and_preview_hash_are_stable() {
    assert_eq!(
        finding_key(&finding(FindingKind::IncomparableConflict)),
        "incomparable_conflict:rule-a:rule-b"
    );
    assert_eq!(
        finding_key(&finding(FindingKind::IneffectiveUnderLock)),
        "ineffective_under_lock:rule-a:rule-b"
    );
    let change = LockChange {
        rule: rid("no-prune"),
        change: LockChangeKind::Created,
    };
    let keys = vec!["a:x:y".to_owned()];
    let base = preview_hash(3, "h", &keys, std::slice::from_ref(&change));
    assert_eq!(
        base,
        preview_hash(3, "h", &keys, std::slice::from_ref(&change))
    );
    assert_ne!(
        base,
        preview_hash(4, "h", &keys, std::slice::from_ref(&change))
    );
    assert_ne!(
        base,
        preview_hash(3, "h2", &keys, std::slice::from_ref(&change))
    );
    assert_ne!(
        base,
        preview_hash(3, "h", &[], std::slice::from_ref(&change))
    );
    let edited = LockChange {
        change: LockChangeKind::Edited,
        ..change
    };
    assert_ne!(base, preview_hash(3, "h", &keys, &[edited]));
    assert_eq!(base.len(), 64);
}
