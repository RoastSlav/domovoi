//! Five hand-built operations that together use every step kind, outcome and edge kind.
//!
//! They are built by hand, not by the writer, so they may contain steps the writer does not
//! create yet (a policy decision). Ids and times are fixed so the JSON is stable.

use std::collections::BTreeMap;

use domovoi_core::ports::Versioned;
use domovoi_core::{
    Actor, ApprovalChannel, ApprovalProvenance, Authorization, BudgetKind, BudgetLimits, Budgets,
    CheckKey, EdgeKind, ExecutionIntent, ExecutionRecord, HostId, HumanApproval, Operation,
    OperationId, OperationKind, OperationSnapshot, OperationStatus, PauseReason, PinnedSubject,
    PlanBinding, PlanHash, RetryOf, Step, StepEdge, StepId, StepIntent, StepOutcome, StepProgress,
    StepRecord, TargetKey, UserId,
};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

const BASE: u128 = 0x0190_0000_0000_7000_8000_0000_0000_0000;
/// 2025-10-09T08:53:20Z.
pub const T0: Timestamp = Timestamp::constant(1_760_000_000, 0);

/// The host every scenario runs on.
pub fn host() -> HostId {
    HostId::from_uuid(Uuid::from_u128(BASE + 1))
}

fn at(seconds: i64) -> Timestamp {
    T0 + SignedDuration::from_secs(seconds)
}

pub fn plan_hash(byte: u8) -> PlanHash {
    hex::encode([byte; 32]).parse().unwrap()
}

fn contract() -> domovoi_core::capability::ContractId {
    domovoi_core::capability::ContractId::parse("skill.restart_container").unwrap()
}

fn intent(retry_of: Option<RetryOf>) -> ExecutionIntent {
    ExecutionIntent {
        contract: contract(),
        contract_version: 1,
        host: host(),
        check: CheckKey::parse("container.down").unwrap(),
        target: Some(TargetKey::parse("plex").unwrap()),
        args: BTreeMap::from([("container".to_owned(), "plex".to_owned())]),
        retry_of,
    }
}

/// An approval a person gave at `T0`, for tests only.
pub fn human() -> Authorization {
    Authorization::human(HumanApproval::new(
        UserId::from_uuid(Uuid::from_u128(BASE + 2)),
        ApprovalChannel::Web {
            typed_confirmation: false,
        },
        T0,
    ))
}

/// Builds one snapshot step by step.
struct Builder {
    scenario: u128,
    operation: OperationId,
    steps: Vec<StepRecord>,
    edges: Vec<StepEdge>,
    executions: Vec<Versioned<ExecutionRecord>>,
}

impl Builder {
    fn new(scenario: u128) -> Self {
        Self {
            scenario,
            operation: OperationId::from_uuid(Uuid::from_u128(BASE + (scenario << 16) + 0xffff)),
            steps: Vec::new(),
            edges: Vec::new(),
            executions: Vec::new(),
        }
    }

    /// The id the step after the next one will get: where an Execution goes when its Approval
    /// is added first.
    fn exec_next(&self) -> StepId {
        let ordinal = u128::try_from(self.steps.len()).unwrap();
        StepId::from_uuid(Uuid::from_u128(BASE + (self.scenario << 16) + ordinal + 2))
    }

    /// Adds the next step and returns its id.
    fn step(
        &mut self,
        phase: &str,
        cycle: u32,
        intent: StepIntent,
        outcome: StepOutcome,
        text: (Option<&str>, Option<&str>, Option<&str>),
    ) -> StepId {
        let ordinal = u32::try_from(self.steps.len()).unwrap();
        let id = StepId::from_uuid(Uuid::from_u128(
            BASE + (self.scenario << 16) + u128::from(ordinal) + 1,
        ));
        let created = at(i64::from(ordinal) * 60);
        let started =
            (outcome != StepOutcome::Pending).then(|| created + SignedDuration::from_secs(1));
        let ended = outcome
            .is_final()
            .then(|| created + SignedDuration::from_secs(10));
        let (summary, reason, output) = text;
        self.steps.push(StepRecord {
            step: Step {
                id,
                operation: self.operation,
                ordinal,
                kind: intent.kind(),
                phase: phase.to_owned(),
                parent: None,
                cycle,
                intent,
                created_at: created,
            },
            progress: StepProgress {
                outcome,
                started_at: started,
                ended_at: ended,
                summary: summary.map(str::to_owned),
                reason: reason.map(str::to_owned),
                output: output.map(str::to_owned),
            },
        });
        id
    }

    fn plain(
        &mut self,
        phase: &str,
        cycle: u32,
        intent: StepIntent,
        outcome: StepOutcome,
    ) -> StepId {
        self.step(phase, cycle, intent, outcome, (None, None, None))
    }

    fn edge(&mut self, from: StepId, to: StepId, kind: EdgeKind) {
        self.edges.push(StepEdge {
            operation: self.operation,
            from,
            to,
            kind,
        });
    }

    /// Adds an execution row for `step`, approved by a person when `approved`.
    fn execution(&mut self, step: StepId, approved: bool, retry_of: Option<RetryOf>) {
        let mut record = ExecutionRecord::propose(
            &intent(retry_of),
            step,
            self.operation,
            plan_hash(0xab),
            at(86_400),
            domovoi_core::Risk::Write,
            at(0),
        );
        if approved {
            record
                .authorize(
                    human(),
                    ApprovalProvenance::new(Some("ref-1".to_owned()), "web"),
                    &PlanBinding::Full(plan_hash(0xab)),
                    at(5),
                )
                .unwrap();
        }
        self.executions.push(Versioned {
            value: record,
            version: 1,
        });
    }

    fn finish(
        self,
        status: OperationStatus,
        cycle: u32,
        limits: BudgetLimits,
        ended: bool,
    ) -> OperationSnapshot {
        let operation = Operation {
            id: self.operation,
            kind: OperationKind::Investigate,
            goal: "find out why plex keeps going down".to_owned(),
            status,
            cycle,
            budgets: Budgets::new(limits),
            pinned: vec![PinnedSubject {
                host: host(),
                check: CheckKey::parse("container.down").unwrap(),
                target: Some(TargetKey::parse("plex").unwrap()),
            }],
            parent: None,
            created_at: at(-60),
            started_at: Some(at(0)),
            ended_at: ended.then(|| at(3_600)),
            updated_at: at(3_600),
        };
        OperationSnapshot {
            operation,
            version: 7,
            steps: self.steps,
            edges: self.edges,
            executions: self.executions,
            incidents: vec![domovoi_core::IncidentId::from_uuid(Uuid::from_u128(
                BASE + 3,
            ))],
        }
    }
}

fn evidence() -> StepIntent {
    StepIntent::EvidenceRequest {
        host: host(),
        tool: "docker_ps".to_owned(),
        args: serde_json::json!({"all": true}),
    }
}

fn plan(text: &str) -> StepIntent {
    StepIntent::Plan {
        text: text.to_owned(),
    }
}

fn approval(execution: StepId, expires: i64) -> StepIntent {
    StepIntent::Approval {
        execution,
        plan_hash: plan_hash(0xab),
        expires_at: at(expires),
    }
}

fn verification(execution: StepId) -> StepIntent {
    StepIntent::Verification {
        execution,
        delay_secs: 30,
    }
}

const OUTPUT: Option<&str> = Some("RAW-OUTPUT-MARKER: not for the trace");

/// Evidence done, plan made, execution proposed and waiting for a person.
fn waiting_approval() -> OperationSnapshot {
    let mut b = Builder::new(1);
    b.plain(
        "diagnose",
        0,
        StepIntent::ReasoningTurn {
            context_refs: vec!["incident".to_owned()],
        },
        StepOutcome::Succeeded,
    );
    b.step(
        "diagnose",
        0,
        evidence(),
        StepOutcome::Succeeded,
        (Some("plex exited 3 times in an hour"), None, OUTPUT),
    );
    b.plain(
        "diagnose",
        0,
        StepIntent::Assessment {
            text: "plex runs out of memory".to_owned(),
        },
        StepOutcome::Succeeded,
    );
    let planned = b.plain("fix", 0, plan("restart plex"), StepOutcome::Succeeded);
    let ask = b.plain(
        "fix",
        0,
        approval(b.exec_next(), 86_400),
        StepOutcome::Pending,
    );
    let run = b.plain(
        "fix",
        0,
        StepIntent::Execution(intent(None)),
        StepOutcome::Pending,
    );
    b.execution(run, false, None);
    let policy = b.step(
        "fix",
        0,
        StepIntent::PolicyDecision { execution: run },
        StepOutcome::Succeeded,
        (Some("a person has to approve this"), None, None),
    );
    b.edge(run, ask, EdgeKind::DependsOn);
    b.edge(run, planned, EdgeKind::CausedBy);
    b.edge(policy, run, EdgeKind::DependsOn);
    b.finish(
        OperationStatus::WaitingApproval,
        0,
        BudgetLimits::default(),
        false,
    )
}

/// A restart that worked and was verified.
fn verified_success() -> OperationSnapshot {
    let mut b = Builder::new(2);
    let looked = b.step(
        "diagnose",
        0,
        evidence(),
        StepOutcome::Succeeded,
        (Some("plex is exited"), None, OUTPUT),
    );
    let planned = b.plain("fix", 0, plan("restart plex"), StepOutcome::Succeeded);
    let ask = b.plain(
        "fix",
        0,
        approval(b.exec_next(), 86_400),
        StepOutcome::Succeeded,
    );
    let run = b.step(
        "fix",
        0,
        StepIntent::Execution(intent(None)),
        StepOutcome::Succeeded,
        (Some("restarted plex"), None, OUTPUT),
    );
    b.execution(run, true, None);
    let check = b.step(
        "verify",
        0,
        verification(run),
        StepOutcome::Succeeded,
        (Some("container.down is clear"), None, None),
    );
    b.edge(planned, looked, EdgeKind::DependsOn);
    b.edge(run, ask, EdgeKind::DependsOn);
    b.edge(run, planned, EdgeKind::CausedBy);
    b.edge(check, run, EdgeKind::Verifies);
    b.finish(OperationStatus::Succeeded, 0, BudgetLimits::default(), true)
}

/// The check still failed after a restart, so a second cycle retries with a change.
fn failed_verification_new_cycle() -> OperationSnapshot {
    let mut b = Builder::new(3);
    b.plain("diagnose", 0, evidence(), StepOutcome::Succeeded);
    let first_ask = b.plain(
        "fix",
        0,
        approval(b.exec_next(), 86_400),
        StepOutcome::Succeeded,
    );
    let first = b.step(
        "fix",
        0,
        StepIntent::Execution(intent(None)),
        StepOutcome::Succeeded,
        (Some("restarted plex"), None, OUTPUT),
    );
    b.execution(first, true, None);
    let check = b.step(
        "verify",
        0,
        verification(first),
        StepOutcome::Failed,
        (Some("container.down is still firing"), None, None),
    );
    let turn = b.plain(
        "diagnose",
        1,
        StepIntent::ReasoningTurn {
            context_refs: vec!["cycle-1".to_owned()],
        },
        StepOutcome::Succeeded,
    );
    let second_ask = b.plain(
        "fix",
        1,
        approval(b.exec_next(), 90_000),
        StepOutcome::Succeeded,
    );
    let retry = RetryOf {
        step: first,
        changed: "raise the memory limit before restarting".to_owned(),
    };
    let second = b.plain(
        "fix",
        1,
        StepIntent::Execution(intent(Some(retry.clone()))),
        StepOutcome::Running,
    );
    b.execution(second, true, Some(retry));
    b.edge(first, first_ask, EdgeKind::DependsOn);
    b.edge(check, first, EdgeKind::Verifies);
    b.edge(second, second_ask, EdgeKind::DependsOn);
    b.edge(second, first, EdgeKind::RetryOf);
    b.edge(second, turn, EdgeKind::CausedBy);
    b.finish(OperationStatus::Running, 1, BudgetLimits::default(), false)
}

/// A run whose result was lost: the operation pauses until a person looks.
fn outcome_unknown_paused() -> OperationSnapshot {
    let mut b = Builder::new(4);
    b.plain("diagnose", 0, evidence(), StepOutcome::Succeeded);
    let ask = b.plain(
        "fix",
        0,
        approval(b.exec_next(), 86_400),
        StepOutcome::Succeeded,
    );
    let run = b.step(
        "fix",
        0,
        StepIntent::Execution(intent(None)),
        StepOutcome::OutcomeUnknown,
        (
            None,
            Some("the agent restarted and cannot say whether it ran"),
            None,
        ),
    );
    b.execution(run, true, None);
    b.edge(run, ask, EdgeKind::DependsOn);
    let status = OperationStatus::Paused {
        reason: PauseReason::OutcomeUnknown { step: run },
    };
    b.finish(status, 0, BudgetLimits::default(), false)
}

/// A question answered, an unavailable host, a spent execution budget and the steps that
/// never ran.
fn question_and_budget_pause() -> OperationSnapshot {
    let mut b = Builder::new(5);
    let seen = b.plain(
        "diagnose",
        0,
        StepIntent::Assessment {
            text: "two containers could be the cause".to_owned(),
        },
        StepOutcome::Succeeded,
    );
    let question = b.step(
        "ask",
        0,
        StepIntent::Question {
            text: "Is plex allowed to restart during the evening?".to_owned(),
            expires_at: at(86_400),
        },
        StepOutcome::Succeeded,
        (Some("answered"), None, None),
    );
    let answer = b.plain(
        "ask",
        0,
        StepIntent::Note {
            text: "yes, after 22:00".to_owned(),
            author: Actor::User(UserId::from_uuid(Uuid::from_u128(BASE + 2))),
        },
        StepOutcome::Succeeded,
    );
    b.plain(
        "tell",
        0,
        StepIntent::Notification {
            channel: "discord".to_owned(),
            subject: "plex needs a restart tonight".to_owned(),
        },
        StepOutcome::Succeeded,
    );
    b.step(
        "diagnose",
        0,
        evidence(),
        StepOutcome::Unavailable,
        (None, Some("the host is not connected"), None),
    );
    let ask = b.plain(
        "fix",
        0,
        approval(b.exec_next(), 3_600),
        StepOutcome::NotRun,
    );
    let expired = b.step(
        "fix",
        0,
        StepIntent::Execution(intent(None)),
        StepOutcome::NotRun,
        (
            None,
            Some("the approval expired before the action was dispatched"),
            None,
        ),
    );
    b.execution(expired, false, None);
    b.step(
        "fix",
        0,
        StepIntent::Execution(intent(None)),
        StepOutcome::Refused,
        (None, Some("execution budget spent"), None),
    );
    b.plain(
        "wrap_up",
        0,
        StepIntent::Note {
            text: "left for later".to_owned(),
            author: Actor::System("operations".to_owned()),
        },
        StepOutcome::Skipped,
    );
    b.plain(
        "wrap_up",
        0,
        plan("restart plex tonight"),
        StepOutcome::Cancelled,
    );
    b.edge(answer, question, EdgeKind::Answers);
    b.edge(question, seen, EdgeKind::DependsOn);
    b.edge(expired, ask, EdgeKind::DependsOn);
    let limits = BudgetLimits {
        executions: Some(1),
        ..BudgetLimits::default()
    };
    let mut snapshot = b.finish(
        OperationStatus::Paused {
            reason: PauseReason::Budget {
                exhausted: BudgetKind::Executions,
            },
        },
        0,
        limits,
        false,
    );
    snapshot.operation.budgets.used.executions = 1;
    snapshot
}

/// The five scenarios with the file name each is stored under.
pub fn all() -> Vec<(&'static str, OperationSnapshot)> {
    vec![
        ("waiting-approval.json", waiting_approval()),
        ("verified-success.json", verified_success()),
        (
            "failed-verification-new-cycle.json",
            failed_verification_new_cycle(),
        ),
        ("outcome-unknown-paused.json", outcome_unknown_paused()),
        (
            "question-and-budget-pause.json",
            question_and_budget_pause(),
        ),
    ]
}
