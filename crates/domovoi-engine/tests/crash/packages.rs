//! The install service crashed at every commit of a short history: creating a job, cancelling a
//! target, and settling a target whose approval ran out. Each commit is atomic, so a crash leaves
//! either all of its change, journal and audit rows or none, and the service picks up from what
//! is stored after a restart.

use std::sync::Arc;

use domovoi_core::capability::{Availability, ContractId, ContractIndex, known_contracts};
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::ports::{HostRepo, InstallRepo, Presence};
use domovoi_core::{HostId, InstallKind, InstallTarget, JournalEpoch, TargetStatus, UserId};
use domovoi_engine::packages::{
    InstallService, JobRequest, PackageDeps, PackageError, PackageHandle, PackageSettings,
    PlanRequest,
};
use domovoi_testkit::build::{T0, host};
use domovoi_testkit::crash::{CrashPlan, CrashPoint, CrashingStore};
use domovoi_testkit::link::package_caps;
use domovoi_testkit::{
    FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakePackageFiles, FakeTargetResolver,
    InMemoryStore, approvals,
};
use jiff::SignedDuration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const UNINSTALL: &str = "agent.package.uninstall";

struct Rig {
    store: Arc<InMemoryStore>,
    crashing: Arc<CrashingStore<InMemoryStore>>,
    clock: Arc<FakeClock>,
    handle: PackageHandle,
    shutdown: CancellationToken,
    host: HostId,
    user: UserId,
}

impl Rig {
    /// A service over a crashing store, with one host whose agent is not offered the uninstall
    /// contract yet, so a queued target waits and nothing else commits.
    async fn new() -> Self {
        let store = Arc::new(InMemoryStore::new());
        let crashing = Arc::new(CrashingStore::new(store.clone()));
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        let index = ContractIndex::new(known_contracts()).unwrap();
        let registry = Arc::new(FakeCapabilityRegistry::new(Arc::new(index)));
        let registered = host("nas").unwrap();
        let id = registered.id;
        HostRepo::upsert_host(&*store, &registered).await.unwrap();
        link.set_presence(
            id,
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
        );
        link.set_journal_epoch(
            id,
            JournalEpoch::from_uuid(Uuid::from_u128(id.as_uuid().as_u128())),
        );
        registry.set_availability(
            id,
            &ContractId::parse(UNINSTALL).unwrap(),
            Availability::HostUnknown,
        );
        let shutdown = CancellationToken::new();
        let (handle, _task) = InstallService::spawn(
            PackageDeps {
                installs: store.clone(),
                uow: crashing.clone(),
                hosts: store.clone(),
                resolver: Arc::new(FakeTargetResolver::new()),
                registry,
                caps: package_caps(link),
                files: Arc::new(FakePackageFiles::new()),
                clock: clock.clone(),
                shutdown: shutdown.clone(),
            },
            PackageSettings::default(),
        );
        Self {
            store,
            crashing,
            clock,
            handle,
            shutdown,
            host: id,
            user: UserId::new(),
        }
    }

    fn request(&self) -> PlanRequest {
        PlanRequest {
            kind: InstallKind::Uninstall,
            name: PackageName::parse("web-tools").unwrap(),
            version: PackageVersion::parse("1.0.0").unwrap(),
            hosts: vec![self.host],
        }
    }

    async fn create(&self) -> Result<InstallTarget, PackageError> {
        let request = self.request();
        let plan = self.handle.plan(&request).await?;
        let (_, mut targets) = self
            .handle
            .create_job(
                JobRequest {
                    plan: request,
                    job_plan_hash: plan.job_plan_hash,
                },
                approvals::web_typed(self.user),
                approvals::provenance(),
            )
            .await?;
        Ok(targets.remove(0))
    }

    async fn active(&self) -> Vec<InstallTarget> {
        InstallRepo::non_terminal_targets(&*self.store)
            .await
            .unwrap()
    }

    /// Lets the worker run until `done` holds.
    async fn until(&self, what: &str, done: impl Fn(&[InstallTarget]) -> bool) {
        for _ in 0..50_000 {
            if done(&self.active().await) {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("{what}");
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

#[tokio::test]
async fn creating_and_cancelling_a_job_survives_a_crash_at_every_commit() {
    for at_commit in 1..=2 {
        for point in [CrashPoint::BeforeCommit, CrashPoint::AfterCommit] {
            let rig = Rig::new().await;
            rig.crashing.arm(CrashPlan { at_commit, point });
            let history = async {
                let target = rig.create().await?;
                rig.handle
                    .cancel_target(target.job, target.host, rig.user)
                    .await
            };
            assert!(
                history.await.is_err(),
                "commit {at_commit}, {point:?}: the crash never happened"
            );
            rig.crashing.restart();

            // What survived is whole: a job has its target, and a cancelled target is terminal.
            for target in rig.active().await {
                assert_eq!(target.status, TargetStatus::Queued);
                rig.handle
                    .cancel_target(target.job, target.host, rig.user)
                    .await
                    .unwrap();
            }
            assert!(
                rig.active().await.is_empty(),
                "commit {at_commit}, {point:?}"
            );
            // The host is free again for a new job.
            rig.create().await.unwrap();
        }
    }
}

#[tokio::test]
async fn an_expired_approval_settles_after_a_crash_at_the_settling_commit() {
    for point in [CrashPoint::BeforeCommit, CrashPoint::AfterCommit] {
        let rig = Rig::new().await;
        let target = rig.create().await.unwrap();
        rig.clock.advance(SignedDuration::from_mins(11));
        rig.crashing.arm(CrashPlan {
            at_commit: 1,
            point,
        });
        rig.handle.wake();
        for _ in 0..50_000 {
            if rig.crashing.crashed() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            rig.crashing.crashed(),
            "{point:?}: the crash never happened"
        );
        rig.crashing.restart();
        rig.handle.wake();
        rig.until("the target never settled", |active| active.is_empty())
            .await;
        let settled = InstallRepo::target(&*rig.store, target.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(settled.status, TargetStatus::NotRun, "{point:?}");
        assert_eq!(
            settled.failure.as_ref().map(|failure| failure.code()),
            Some("approval_expired"),
            "{point:?}"
        );
    }
}
