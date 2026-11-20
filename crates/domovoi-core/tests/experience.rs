//! Resolution summaries: field rules, derived code fields, tags and similarity scoring.

use std::collections::BTreeMap;

use domovoi_core::capability::ContractId;
use domovoi_core::experience::{
    ExecutedContract, ExperienceQuery, ExperienceScope, ResolutionOutcome, ResolutionSummary,
    SIMILAR_MIN_SCORE, SimilarHit, SimilarQuery, SimilarReason, SummaryField, SummarySource,
    SummarySubject, code_fields, order_similar, outcome_tags, query_words, score_similar,
};
use domovoi_core::ports::Versioned;
use domovoi_core::{
    Budgets, CheckKey, EdgeKind, ExecutionIntent, ExecutionRecord, HostId, JournalEpoch, Operation,
    OperationId, OperationKind, OperationSnapshot, OperationStatus, PinnedSubject, PlanHash, Risk,
    Step, StepEdge, StepId, StepIntent, StepOutcome, StepProgress, StepRecord, TargetKey,
};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

const T0: Timestamp = Timestamp::constant(1_760_000_000, 0);

#[cfg(test)]
fn host(n: u128) -> HostId {
    HostId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 + n,
    ))
}

#[cfg(test)]
fn plan_hash() -> PlanHash {
    "01".repeat(32).parse().unwrap()
}

#[cfg(test)]
fn check() -> CheckKey {
    CheckKey::parse("container.down").unwrap()
}

#[cfg(test)]
fn target() -> TargetKey {
    TargetKey::parse("plex").unwrap()
}

/// Builds a snapshot step by step.
#[cfg(test)]
struct Build {
    snapshot: OperationSnapshot,
}

#[cfg(test)]
impl Build {
    fn new(status: OperationStatus) -> Self {
        let mut operation = Operation::queued(
            OperationId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_00ff_0001)),
            OperationKind::Investigate,
            "plex is down",
            vec![PinnedSubject {
                host: host(1),
                check: check(),
                target: Some(target()),
            }],
            None,
            Budgets::default(),
            T0,
        )
        .unwrap();
        operation.status = status;
        Self {
            snapshot: OperationSnapshot {
                operation,
                version: 1,
                steps: Vec::new(),
                edges: Vec::new(),
                executions: Vec::new(),
                incidents: Vec::new(),
            },
        }
    }

    fn step(&mut self, intent: StepIntent, outcome: StepOutcome) -> StepId {
        let ordinal = u32::try_from(self.snapshot.steps.len()).unwrap();
        let id = StepId::from_uuid(Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_1000 + u128::from(ordinal),
        ));
        self.snapshot.steps.push(StepRecord {
            step: Step {
                id,
                operation: self.snapshot.operation.id,
                ordinal,
                kind: intent.kind(),
                phase: "work".to_owned(),
                parent: None,
                cycle: 0,
                intent,
                created_at: T0,
            },
            progress: StepProgress {
                outcome,
                ..StepProgress::pending()
            },
        });
        id
    }

    fn edge(&mut self, from: StepId, to: StepId, kind: EdgeKind) {
        self.snapshot.edges.push(StepEdge {
            operation: self.snapshot.operation.id,
            from,
            to,
            kind,
        });
    }

    /// An Execution step and its row; `dispatched` says whether anything was sent.
    fn execution(&mut self, outcome: StepOutcome, dispatched: bool, on: HostId) -> StepId {
        let intent = ExecutionIntent {
            contract: ContractId::parse("skill.restart_container").unwrap(),
            contract_version: 1,
            host: on,
            check: check(),
            target: Some(target()),
            args: BTreeMap::new(),
            retry_of: None,
        };
        let id = self.step(StepIntent::Execution(intent.clone()), outcome);
        let mut row = ExecutionRecord::propose(
            &intent,
            id,
            self.snapshot.operation.id,
            plan_hash(),
            T0 + SignedDuration::from_secs(600),
            Risk::Write,
            T0,
        );
        if dispatched {
            row.dispatched = Some((JournalEpoch::default(), T0));
        }
        self.snapshot.executions.push(Versioned {
            value: row,
            version: 1,
        });
        id
    }

    fn verification(&mut self, of: StepId, outcome: StepOutcome) -> StepId {
        let id = self.step(
            StepIntent::Verification {
                execution: of,
                delay_secs: 30,
            },
            outcome,
        );
        self.edge(id, of, EdgeKind::Verifies);
        id
    }
}

#[test]
fn summary_field_names_ownership_and_order_are_pinned() {
    let names: Vec<_> = SummaryField::ALL.iter().map(|f| f.as_str()).collect();
    assert_eq!(
        names,
        [
            "outcome",
            "verified",
            "subjects",
            "contracts_executed",
            "hosts",
            "problem",
            "cause",
            "fix"
        ]
    );
    for (i, field) in SummaryField::ALL.into_iter().enumerate() {
        assert_eq!(field.is_code_owned(), i < 5);
        assert_eq!(field.is_narrative(), i >= 5);
        assert_eq!(SummaryField::parse(field.as_str()), Some(field));
    }
    assert_eq!(SummaryField::parse("nope"), None);
}

#[test]
fn a_source_may_only_write_its_own_fields() {
    for field in SummaryField::ALL {
        assert_eq!(SummarySource::Code.may_write(field), field.is_code_owned());
        assert_eq!(SummarySource::Model.may_write(field), field.is_narrative());
        assert_eq!(SummarySource::User.may_write(field), field.is_narrative());
    }
}

#[test]
fn code_fields_are_none_until_the_operation_ends() {
    assert!(code_fields(&Build::new(OperationStatus::Running).snapshot).is_none());
}

#[test]
fn code_fields_of_a_verified_fix() {
    let mut b = Build::new(OperationStatus::Succeeded);
    let exec = b.execution(StepOutcome::Succeeded, true, host(2));
    b.verification(exec, StepOutcome::Succeeded);
    let fields = code_fields(&b.snapshot).unwrap();
    assert_eq!(fields.outcome, ResolutionOutcome::Succeeded);
    assert!(fields.verified);
    assert_eq!(fields.contracts.len(), 1);
    assert_eq!(fields.contracts[0].outcome, "succeeded");
    assert_eq!(fields.contracts[0].contract, "skill.restart_container");
    assert_eq!(fields.hosts, vec![host(1), host(2)]);
    assert_eq!(fields.values()[0].0, SummaryField::Outcome);
}

#[test]
fn a_report_only_investigation_is_succeeded_and_not_verified() {
    let fields = code_fields(&Build::new(OperationStatus::Succeeded).snapshot).unwrap();
    assert_eq!(fields.outcome, ResolutionOutcome::Succeeded);
    assert!(!fields.verified);
    assert!(fields.contracts.is_empty());
}

#[test]
fn only_dispatched_executions_count_as_executed() {
    let mut b = Build::new(OperationStatus::Cancelled);
    b.execution(StepOutcome::Refused, false, host(1));
    b.execution(StepOutcome::NotRun, false, host(1));
    b.execution(StepOutcome::Running, true, host(1));
    let fields = code_fields(&b.snapshot).unwrap();
    assert_eq!(fields.contracts.len(), 1);
    assert_eq!(fields.contracts[0].outcome, "running");

    let mut many = Build::new(OperationStatus::Failed);
    for _ in 0..40 {
        many.execution(StepOutcome::Failed, true, host(1));
    }
    assert_eq!(code_fields(&many.snapshot).unwrap().contracts.len(), 32);
}

#[test]
fn verified_needs_the_last_verification_to_pass_and_its_execution_to_succeed() {
    let mut b = Build::new(OperationStatus::Succeeded);
    let exec = b.execution(StepOutcome::Succeeded, true, host(1));
    b.verification(exec, StepOutcome::Failed);
    b.verification(exec, StepOutcome::Succeeded);
    assert!(code_fields(&b.snapshot).unwrap().verified);

    let mut b = Build::new(OperationStatus::Failed);
    let exec = b.execution(StepOutcome::Succeeded, true, host(1));
    b.verification(exec, StepOutcome::Succeeded);
    b.verification(exec, StepOutcome::Failed);
    assert!(!code_fields(&b.snapshot).unwrap().verified);

    let mut b = Build::new(OperationStatus::Failed);
    let exec = b.execution(StepOutcome::Failed, true, host(1));
    b.verification(exec, StepOutcome::Succeeded);
    assert!(!code_fields(&b.snapshot).unwrap().verified);
}

#[test]
fn outcome_tags_stay_inside_the_closed_vocabulary() {
    const VOCABULARY: [&str; 9] = [
        "denied",
        "expired",
        "refused",
        "verification_failed",
        "retried",
        "outcome_unknown",
        "question_asked",
        "escalated",
        "cancelled",
    ];
    let mut b = Build::new(OperationStatus::Escalated);
    let denied = b.execution(StepOutcome::Refused, false, host(1));
    b.step(
        StepIntent::Approval {
            execution: denied,
            plan_hash: plan_hash(),
            expires_at: T0,
        },
        StepOutcome::Refused,
    );
    let expired = b.execution(StepOutcome::NotRun, false, host(1));
    b.step(
        StepIntent::Approval {
            execution: expired,
            plan_hash: plan_hash(),
            expires_at: T0,
        },
        StepOutcome::NotRun,
    );
    let refused = b.execution(StepOutcome::Refused, false, host(1));
    let unknown = b.execution(StepOutcome::OutcomeUnknown, true, host(1));
    b.edge(unknown, refused, EdgeKind::RetryOf);
    b.verification(unknown, StepOutcome::Failed);
    b.step(
        StepIntent::Question {
            text: "ok?".to_owned(),
            expires_at: T0,
        },
        StepOutcome::Succeeded,
    );
    let tags = outcome_tags(&b.snapshot);
    let mut sorted = tags.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(tags, sorted);
    assert!(tags.iter().all(|t| VOCABULARY.contains(t)));
    for wanted in VOCABULARY.iter().filter(|t| **t != "cancelled") {
        assert!(tags.contains(wanted), "missing {wanted}");
    }
    let cancelled = Build::new(OperationStatus::Cancelled);
    assert_eq!(outcome_tags(&cancelled.snapshot), vec!["cancelled"]);
}

#[cfg(test)]
fn summary(
    check_name: &str,
    target_name: Option<&str>,
    contract: Option<&str>,
) -> ResolutionSummary {
    ResolutionSummary {
        operation: OperationId::new(),
        goal: "goal".to_owned(),
        ended_at: T0,
        outcome: ResolutionOutcome::Succeeded,
        verified: true,
        subjects: vec![SummarySubject {
            host: host(1),
            check: CheckKey::parse(check_name).unwrap(),
            target: target_name.map(|t| TargetKey::parse(t).unwrap()),
        }],
        contracts: contract
            .into_iter()
            .map(|c| ExecutedContract {
                contract: c.to_owned(),
                host: host(1),
                target: None,
                outcome: "succeeded".to_owned(),
            })
            .collect(),
        hosts: vec![host(1)],
        problem: None,
        cause: None,
        fix: None,
    }
}

#[cfg(test)]
fn similar_query(contracts: &[&str]) -> SimilarQuery {
    SimilarQuery {
        host: host(1),
        check: Some(check()),
        target: Some(target()),
        text: String::new(),
        contracts: contracts.iter().map(|c| (*c).to_owned()).collect(),
        exclude: None,
        limit: 5,
    }
}

#[test]
fn similarity_weights_are_pinned() {
    let q = similar_query(&[]);
    let same_check = summary("container.down", None, None);
    assert_eq!(score_similar(&q, &same_check, None).0, 8);
    let both = summary("container.down", Some("plex"), None);
    let (score, reasons) = score_similar(&q, &both, None);
    assert_eq!(score, 12);
    assert_eq!(
        reasons,
        [SimilarReason::SameCheck, SimilarReason::SameTarget]
    );
    let target_only = summary("disk.full", Some("plex"), None);
    assert_eq!(score_similar(&q, &target_only, None).0, 4);

    let contracts = ["a.one", "b.two", "c.three", "d.four"];
    let mut ran = summary("other.check", None, None);
    ran.contracts = contracts
        .iter()
        .map(|c| ExecutedContract {
            contract: (*c).to_owned(),
            host: host(1),
            target: None,
            outcome: "succeeded".to_owned(),
        })
        .collect();
    let mut q = similar_query(&contracts);
    q.check = None;
    q.target = None;
    assert_eq!(score_similar(&q, &ran, None).0, 6);
    q.contracts.truncate(1);
    assert_eq!(score_similar(&q, &ran, None).0, 2);

    let q = similar_query(&[]);
    let plain = summary("other.check", None, None);
    assert_eq!(score_similar(&q, &plain, Some(0)).0, 4);
    assert_eq!(score_similar(&q, &plain, Some(4)).0, 4);
    assert_eq!(score_similar(&q, &plain, Some(5)).0, 2);
    assert_eq!(score_similar(&q, &plain, Some(19)).0, 2);
    assert_eq!(score_similar(&q, &plain, Some(20)).0, 0);
    let (_, reasons) = score_similar(&q, &plain, Some(0));
    assert_eq!(reasons, [SimilarReason::SimilarWords]);
    assert_eq!(SIMILAR_MIN_SCORE, 4);
}

#[test]
fn similar_hits_order_by_score_then_recency_then_id() {
    let hit = |score: u32, secs: i64, id: u128| {
        let mut s = summary("container.down", None, None);
        s.operation = OperationId::from_uuid(Uuid::from_u128(id));
        s.ended_at = T0 + SignedDuration::from_secs(secs);
        SimilarHit {
            summary: s,
            score,
            reasons: Vec::new(),
        }
    };
    let mut hits = vec![hit(8, 0, 1), hit(12, 0, 2), hit(8, 10, 3), hit(8, 10, 4)];
    order_similar(&mut hits);
    let ids: Vec<_> = hits
        .iter()
        .map(|h| h.summary.operation.as_uuid().as_u128())
        .collect();
    assert_eq!(ids, [2, 4, 3, 1]);
}

#[test]
fn query_words_dedupe_and_cap() {
    assert_eq!(
        query_words("Plex plex PLEX, zfs* OR a:b"),
        ["plex", "zfs", "or", "a", "b"]
    );
    assert!(query_words("***").is_empty());
    let many = (0..20)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(query_words(&many).len(), 12);
    assert_eq!(query_words(&"x".repeat(100))[0].len(), 40);
}

#[test]
fn query_limits_are_clamped() {
    let q = ExperienceQuery {
        scope: ExperienceScope::Global,
        text: String::new(),
        limit: 0,
        exclude: None,
    };
    assert_eq!(q.limit(), 1);
    assert_eq!(similar_query(&[]).limit(), 5);
}

#[test]
fn summary_wire_shapes_snapshot() {
    use domovoi_core::experience::SummaryRevision;
    let fields = code_fields(&{
        let mut b = Build::new(OperationStatus::Succeeded);
        let exec = b.execution(StepOutcome::Succeeded, true, host(2));
        b.verification(exec, StepOutcome::Succeeded);
        b.snapshot
    })
    .unwrap();
    let operation =
        OperationId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_00ff_0001));
    let mut revisions: Vec<SummaryRevision> = fields
        .values()
        .into_iter()
        .map(|(field, value)| SummaryRevision {
            operation,
            field,
            revision: 1,
            value,
            source: SummarySource::Code,
            actor: "system:operations".to_owned(),
            created_at: T0,
        })
        .collect();
    for (field, text) in [
        (SummaryField::Problem, "plex was down"),
        (SummaryField::Cause, "out of memory"),
        (SummaryField::Fix, "restart it"),
    ] {
        revisions.push(SummaryRevision {
            operation,
            field,
            revision: 1,
            value: serde_json::json!(text),
            source: SummarySource::Model,
            actor: "system:operations".to_owned(),
            created_at: T0,
        });
    }
    insta::assert_json_snapshot!(revisions);
}
