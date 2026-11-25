//! The shadow recorder crashed at every commit of a backlog sweep: nothing is lost, nothing is
//! recorded twice, and the counters count the rows that exist.
//!
//! The recorder keeps no cursor. The next sweep asks the store for the steps without a row,
//! and the insert keeps the first row written, so a restart after any crash converges on one
//! row per step.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::capability::{CeilingSummary, ContractIndex, ContractMeta, FactId};
use domovoi_core::policy_store::ShadowFilter;
use domovoi_core::ports::{
    ExecutionGuard, Expected, OperationWrite, Page, PolicyRepo, StateWrite, Transaction, UnitOfWork,
};
use domovoi_core::{
    Budgets, CheckKey, ContractId, ExecutionIntent, ExecutionRecord, Operation, OperationId,
    OperationKind, PinnedSubject, PlanHash, Risk, Step, StepId, StepIntent,
};
use domovoi_engine::policy::FactState;
use domovoi_engine::policy_shadow::{
    AutonomyFacts, AutonomyReading, BATCH, CeilingReading, ContainmentReading, GapReason,
    HostTarget, ShadowEnvironment, ShadowRecorder,
};
use domovoi_testkit::FakeClock;
use domovoi_testkit::InMemoryStore;
use domovoi_testkit::build::{T0, at};
use domovoi_testkit::crash::{CrashPlan, CrashPoint, CrashingStore};

/// An environment that knows nothing. The steps here are package installs, which are skipped
/// before any question is asked.
struct Unknown;

#[async_trait]
impl ShadowEnvironment for Unknown {
    async fn contract(&self, _: HostId, _: &ContractId) -> Option<ContractMeta> {
        None
    }
    async fn host_target(&self, _: HostId) -> Result<HostTarget, GapReason> {
        Err(GapReason::Unresolvable)
    }
    async fn ceiling(&self, _: HostId) -> CeilingReading {
        CeilingReading::HostNotConnected
    }
    async fn runs_as_root(&self, _: HostId) -> Option<bool> {
        None
    }
    async fn facts(&self, _: HostId, _: &BTreeSet<FactId>) -> BTreeMap<FactId, FactState> {
        BTreeMap::new()
    }
    async fn containment(&self, _: HostId) -> ContainmentReading {
        ContainmentReading::Unavailable
    }
    async fn autonomy(&self) -> AutonomyReading {
        AutonomyReading::Reported(AutonomyFacts::default())
    }
    async fn publication_index(&self) -> ContractIndex {
        ContractIndex::default()
    }
    async fn ceilings(&self) -> BTreeMap<HostId, CeilingSummary> {
        BTreeMap::new()
    }
}

/// Three batches of package steps: skipped rows need no inputs, so the sweep is all commits.
const STEPS: u32 = 120;

async fn seed(store: &InMemoryStore) {
    let host = "01900000-0000-7000-8000-000000000001".parse().unwrap();
    let check = CheckKey::parse("container.down").unwrap();
    let op = Operation::queued(
        OperationId::new(),
        OperationKind::Investigate,
        "find out why plex is down",
        vec![PinnedSubject {
            host,
            check: check.clone(),
            target: None,
        }],
        None,
        Budgets::default(),
        at(1),
    )
    .unwrap();
    let intent = ExecutionIntent {
        contract: ContractId::parse("agent.package.install").unwrap(),
        contract_version: 1,
        host,
        check,
        target: None,
        args: BTreeMap::new(),
        retry_of: None,
    };
    let mut tx = Transaction::new().write(StateWrite::Operation(OperationWrite::Operation {
        operation: op.clone(),
        expected: Expected::New,
    }));
    for ordinal in 0..STEPS {
        let step = Step {
            id: StepId::new(),
            operation: op.id,
            ordinal,
            kind: StepIntent::Execution(intent.clone()).kind(),
            phase: "investigate".to_owned(),
            parent: None,
            cycle: 0,
            intent: StepIntent::Execution(intent.clone()),
            created_at: at(10),
        };
        let hash: PlanHash = "ab".repeat(32).parse().unwrap();
        let record = ExecutionRecord::propose(
            &intent,
            step.id,
            op.id,
            hash,
            at(10_000),
            Risk::Write,
            at(10),
        );
        tx = tx
            .write(StateWrite::Operation(OperationWrite::Step(step)))
            .write(StateWrite::Operation(OperationWrite::Execution {
                execution: record,
                expected: ExecutionGuard::New,
            }));
    }
    store.commit(tx).await.unwrap();
}

fn recorder(store: &Arc<InMemoryStore>, uow: Arc<dyn UnitOfWork>) -> ShadowRecorder {
    ShadowRecorder::new(
        store.clone(),
        uow,
        Arc::new(Unknown),
        Arc::new(FakeClock::new(T0)),
    )
}

#[tokio::test]
async fn recorder_survives_a_crash_at_every_commit_in_memory() {
    let batches = u64::from(STEPS.div_ceil(u32::from(BATCH)));
    for at_commit in 1..=batches {
        for point in [CrashPoint::BeforeCommit, CrashPoint::AfterCommit] {
            let store = Arc::new(InMemoryStore::new());
            seed(&store).await;
            let crashing = Arc::new(CrashingStore::new(store.clone()));
            crashing.arm(CrashPlan { at_commit, point });
            let first = recorder(&store, crashing.clone()).sweep_once().await;
            assert!(
                first.is_err(),
                "commit {at_commit}, {point:?}: the crash never happened"
            );

            // A restart: a fresh recorder over the same store finishes the job.
            crashing.restart();
            recorder(&store, crashing.clone())
                .sweep_once()
                .await
                .unwrap();
            let mut count = 0_u64;
            let mut cursor = None;
            loop {
                let page = PolicyRepo::shadow_records(
                    &*store,
                    &ShadowFilter::default(),
                    &Page {
                        cursor: cursor.clone(),
                        limit: 50,
                    },
                )
                .await
                .unwrap();
                count += page.items.len() as u64;
                cursor = page.next_cursor;
                if cursor.is_none() {
                    break;
                }
            }
            let state = PolicyRepo::shadow_state(&*store).await.unwrap();
            assert_eq!(
                (count, state.total_skipped),
                (u64::from(STEPS), u64::from(STEPS)),
                "commit {at_commit}, {point:?}: one row per step, counted once",
            );
            assert_eq!(
                PolicyRepo::shadow_candidates(&*store, 1)
                    .await
                    .unwrap()
                    .len(),
                0,
                "commit {at_commit}, {point:?}: no step is left without a row",
            );
        }
    }
}
