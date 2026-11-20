use domovoi_core::{HostId, UserId};
use serde_json::json;

use super::*;

fn ts(s: i64) -> Timestamp {
    Timestamp::from_second(s).unwrap()
}

fn draft(
    subject: &SubjectRef,
    predicate: Predicate,
    value: Value,
    source: FactSource,
) -> ValidDraft {
    ValidDraft {
        subject: subject.clone(),
        predicate,
        value,
        source,
        observed_at: ts(100),
        valid_until: None,
        evidence: None,
    }
}

fn live_fact(d: &ValidDraft, status: KnowledgeStatus) -> Versioned<Fact> {
    let mut f = new_fact(
        d,
        &Actor::System("knowledge".into()),
        ts(0),
        FactRecordId::new(),
        None,
    );
    f.status = status;
    Versioned {
        value: f,
        version: 3,
    }
}

fn system() -> Actor {
    Actor::System("knowledge".into())
}

fn host() -> SubjectRef {
    SubjectRef::Host(HostId::new())
}

#[test]
fn a_first_fact_is_created_current() {
    let s = host();
    let d = draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered);
    let plan = plan_record(&[], &d, &system(), ts(100), FactRecordId::new());
    assert_eq!(plan.effect, RecordEffect::Created);
    assert_eq!(plan.inserts.len(), 1);
    assert!(plan.updates.is_empty());
    assert_eq!(plan.inserts[0].status, KnowledgeStatus::Current);
}

#[test]
fn another_source_with_another_value_disputes_both() {
    let s = host();
    let a = live_fact(
        &draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
        KnowledgeStatus::Current,
    );
    let d = draft(
        &s,
        Predicate::Role,
        json!("backup"),
        FactSource::Integration,
    );
    let plan = plan_record(
        std::slice::from_ref(&a),
        &d,
        &system(),
        ts(100),
        FactRecordId::new(),
    );
    assert_eq!(plan.effect, RecordEffect::Disputed);
    assert_eq!(plan.inserts[0].status, KnowledgeStatus::Disputed);
    assert_eq!(plan.updates.len(), 1);
    assert_eq!(plan.updates[0].id, a.value.id);
    assert_eq!(
        plan.updates[0].update.status,
        Some(KnowledgeStatus::Disputed)
    );
    assert_eq!(plan.updates[0].change, "disputed");
    assert_eq!(plan.updates[0].expected, 3);
}

#[test]
fn same_source_new_value_supersedes_without_dispute() {
    let s = host();
    let a = live_fact(
        &draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
        KnowledgeStatus::Current,
    );
    let d = draft(&s, Predicate::Role, json!("backup"), FactSource::Discovered);
    let plan = plan_record(
        std::slice::from_ref(&a),
        &d,
        &system(),
        ts(100),
        FactRecordId::new(),
    );
    assert_eq!(plan.effect, RecordEffect::Superseded);
    assert_eq!(plan.inserts[0].supersedes, Some(a.value.id));
    assert_eq!(plan.inserts[0].status, KnowledgeStatus::Current);
    assert_eq!(
        plan.updates[0].update.status,
        Some(KnowledgeStatus::Superseded)
    );
}

#[test]
fn owner_assertion_supersedes_a_dispute() {
    let s = host();
    let a = live_fact(
        &draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
        KnowledgeStatus::Disputed,
    );
    let b = live_fact(
        &draft(
            &s,
            Predicate::Role,
            json!("backup"),
            FactSource::Integration,
        ),
        KnowledgeStatus::Disputed,
    );
    let d = draft(&s, Predicate::Role, json!("nas"), FactSource::User);
    let user = Actor::User(UserId::new());
    let plan = plan_record(&[a, b], &d, &user, ts(100), FactRecordId::new());
    assert_eq!(plan.effect, RecordEffect::Superseded);
    assert_eq!(plan.updates.len(), 2);
    assert!(
        plan.updates
            .iter()
            .all(|u| u.update.status == Some(KnowledgeStatus::Superseded))
    );
    assert_eq!(plan.inserts[0].source, FactSource::User);
    assert_eq!(plan.inserts[0].status, KnowledgeStatus::Current);
}

#[test]
fn agreement_resolves_a_dispute() {
    let s = host();
    let x = live_fact(
        &draft(&s, Predicate::Role, json!("x"), FactSource::Discovered),
        KnowledgeStatus::Disputed,
    );
    let y = live_fact(
        &draft(&s, Predicate::Role, json!("y"), FactSource::User),
        KnowledgeStatus::Disputed,
    );
    let d = draft(&s, Predicate::Role, json!("y"), FactSource::Discovered);
    let plan = plan_record(
        &[x.clone(), y.clone()],
        &d,
        &system(),
        ts(100),
        FactRecordId::new(),
    );
    assert!(plan.inserts.is_empty());
    assert_eq!(plan.result, y.value.id);
    let ux = plan.updates.iter().find(|u| u.id == x.value.id).unwrap();
    assert_eq!(ux.update.status, Some(KnowledgeStatus::Superseded));
    let uy = plan.updates.iter().find(|u| u.id == y.value.id).unwrap();
    assert_eq!(uy.update.status, Some(KnowledgeStatus::Current));
    assert_eq!(uy.change, "resolved");
}

#[test]
fn equal_value_from_the_same_source_refreshes() {
    let s = host();
    let a = live_fact(
        &draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered),
        KnowledgeStatus::Stale,
    );
    let mut d = draft(&s, Predicate::Role, json!("nas"), FactSource::Discovered);
    d.observed_at = ts(500);
    let plan = plan_record(&[a], &d, &system(), ts(600), FactRecordId::new());
    assert_eq!(plan.effect, RecordEffect::Refreshed);
    let u = &plan.updates[0];
    assert_eq!(u.update.status, Some(KnowledgeStatus::Current));
    assert_eq!(u.update.observed_at, Some(ts(500)));
    assert_eq!(u.update.last_verified_at, Some(ts(600)));
}

#[test]
fn many_valued_predicates_keep_distinct_values() {
    let s = host();
    let a = live_fact(
        &draft(&s, Predicate::OwnerNote, json!("a"), FactSource::User),
        KnowledgeStatus::Current,
    );
    let d = draft(&s, Predicate::OwnerNote, json!("b"), FactSource::Discovered);
    let plan = plan_record(&[a], &d, &system(), ts(100), FactRecordId::new());
    assert_eq!(plan.effect, RecordEffect::Created);
    assert!(plan.updates.is_empty());
}

#[test]
fn safety_critical_facts_are_not_revived_by_observation_or_reassertion() {
    let s = host();
    let v = json!({"cidr": "10.0.0.0/8"});
    let stale = live_fact(
        &draft(&s, Predicate::ManagementPath, v.clone(), FactSource::User),
        KnowledgeStatus::Stale,
    );
    let user = Actor::User(UserId::new());
    for (source, actor) in [
        (FactSource::User, user.clone()),
        (FactSource::Discovered, system()),
    ] {
        let d = draft(&s, Predicate::ManagementPath, v.clone(), source);
        let plan = plan_record(
            std::slice::from_ref(&stale),
            &d,
            &actor,
            ts(100),
            FactRecordId::new(),
        );
        assert_eq!(plan.effect, RecordEffect::Unchanged);
        assert!(plan.updates.is_empty() && plan.inserts.is_empty());
    }
    let discovered = live_fact(
        &draft(
            &s,
            Predicate::ManagementPath,
            v.clone(),
            FactSource::Discovered,
        ),
        KnowledgeStatus::Current,
    );
    let d = draft(&s, Predicate::ManagementPath, v, FactSource::User);
    let plan = plan_record(
        std::slice::from_ref(&discovered),
        &d,
        &user,
        ts(100),
        FactRecordId::new(),
    );
    assert_eq!(plan.effect, RecordEffect::Superseded);
    assert_eq!(plan.inserts[0].supersedes, Some(discovered.value.id));
}

#[test]
fn settling_after_a_retraction_resolves_the_remaining_dispute() {
    let s = host();
    let left = live_fact(
        &draft(&s, Predicate::Role, json!("x"), FactSource::Discovered),
        KnowledgeStatus::Disputed,
    );
    let updates = plan_settle(&[left]);
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].change, "resolved");
    assert_eq!(updates[0].update.status, Some(KnowledgeStatus::Current));
}
