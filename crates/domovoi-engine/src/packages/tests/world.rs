//! The harness: the install service over the in-memory store, a fake clock and the fake link.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::capability::{Availability, ContractId, ContractIndex, known_contracts};
use domovoi_core::package::{ArchiveDigest, KeyId, PackageName, PackageVersion, keys};
use domovoi_core::ports::Clock;
use domovoi_core::ports::{AggregateRef, InstallWrite, StateWrite};
use domovoi_core::ports::{
    AuditEvent, AuditFilter, AuditLog, HostRepo, InstallRepo, JournalRead, LinkError, Page,
    Presence, Transaction, UnitOfWork,
};
use domovoi_core::{
    ArtifactSource, HostId, HumanApproval, InstallJob, InstallKind, InstallTarget, JournalEpoch,
    PackageArtifact, PackageArtifactId, PackageOutputs, TargetGuard, TargetStatus, UserId,
};
use domovoi_testkit::build::{T0, host};
use domovoi_testkit::link::package_caps;
use domovoi_testkit::{
    FakeAgentLink, FakeCapabilityRegistry, FakeClock, FakePackageFiles, FakeTargetResolver,
    InMemoryStore, approvals,
};
use jiff::SignedDuration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::packages::{
    InstallService, JobRequest, PackageDeps, PackageError, PackageHandle, PackageSettings,
    PlanRequest,
};

/// The key id every fixture archive is signed with.
pub(super) const KEY: &str = "0123456789abcdef";

pub(super) struct World {
    pub(super) store: Arc<InMemoryStore>,
    pub(super) clock: Arc<FakeClock>,
    pub(super) link: Arc<FakeAgentLink>,
    pub(super) registry: Arc<FakeCapabilityRegistry>,
    pub(super) files: Arc<FakePackageFiles>,
    pub(super) handle: PackageHandle,
    pub(super) shutdown: CancellationToken,
    pub(super) user: UserId,
    pub(super) host: HostId,
}

impl Drop for World {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

fn contract(id: &str) -> ContractId {
    ContractId::parse(id).unwrap()
}

impl World {
    pub(super) async fn new() -> Self {
        let store = Arc::new(InMemoryStore::new());
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        let index = ContractIndex::new(known_contracts()).unwrap();
        let registry = Arc::new(FakeCapabilityRegistry::new(Arc::new(index)));
        let files = Arc::new(FakePackageFiles::new());
        let (handle, shutdown) = spawn(&store, &clock, &link, &registry, &files);
        let mut world = Self {
            store,
            clock,
            link,
            registry,
            files,
            handle,
            shutdown,
            user: UserId::new(),
            host: HostId::new(),
        };
        world.host = world.add_host("nas").await;
        world
    }

    /// Registers a host whose agent is connected.
    pub(super) async fn add_host(&self, name: &str) -> HostId {
        let registered = host(name).unwrap();
        let id = registered.id;
        HostRepo::upsert_host(&*self.store, &registered)
            .await
            .unwrap();
        self.link.set_presence(
            id,
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
        );
        self.link.set_journal_epoch(
            id,
            JournalEpoch::from_uuid(Uuid::from_u128(id.as_uuid().as_u128())),
        );
        id
    }

    /// Stops the running service and starts another over the same store, as a restart does.
    pub(super) fn restart(&mut self) {
        self.shutdown.cancel();
        let (handle, shutdown) = spawn(
            &self.store,
            &self.clock,
            &self.link,
            &self.registry,
            &self.files,
        );
        self.handle = handle;
        self.shutdown = shutdown;
    }

    /// Imports `bytes` as `name` `version`: the file is stored and the index row written.
    pub(super) async fn import(&self, name: &str, version: &str, bytes: &[u8]) -> PackageArtifact {
        let digest = self.files.store(bytes);
        let artifact = PackageArtifact {
            id: PackageArtifactId::new(),
            name: PackageName::parse(name).unwrap(),
            version: PackageVersion::parse(version).unwrap(),
            archive_sha256: digest,
            key_id: KeyId::parse(KEY).unwrap(),
            archive_size: u64::try_from(bytes.len()).unwrap(),
            signature_toml: "format = 1\nsignature = \"x\"\n".to_owned(),
            signer_label: Some("Owner".to_owned()),
            source: ArtifactSource::OwnerImport,
            imported_by: domovoi_core::Actor::User(self.user),
            imported_at: T0,
        };
        let tx = Transaction::new().write(StateWrite::Install(InstallWrite::ImportArtifact(
            artifact.clone(),
        )));
        UnitOfWork::commit(&*self.store, tx).await.unwrap();
        artifact
    }

    pub(super) fn request(
        &self,
        kind: InstallKind,
        name: &str,
        version: &str,
        hosts: &[HostId],
    ) -> PlanRequest {
        PlanRequest {
            kind,
            name: PackageName::parse(name).unwrap(),
            version: PackageVersion::parse(version).unwrap(),
            hosts: hosts.to_vec(),
        }
    }

    pub(super) fn approval(&self) -> HumanApproval {
        approvals::web_typed(self.user)
    }

    /// Plans and creates a job as the owner would.
    pub(super) async fn create(
        &self,
        request: PlanRequest,
    ) -> Result<(InstallJob, Vec<InstallTarget>), PackageError> {
        let plan = self.handle.plan(&request).await?;
        self.handle
            .create_job(
                JobRequest {
                    plan: request,
                    job_plan_hash: plan.job_plan_hash,
                },
                self.approval(),
                approvals::provenance(),
            )
            .await
    }

    /// Creates an install job for `name` `version` on this world's host.
    pub(super) async fn install(&self, name: &str, version: &str) -> InstallTarget {
        let request = self.request(InstallKind::Install, name, version, &[self.host]);
        self.create(request).await.unwrap().1.remove(0)
    }

    pub(super) async fn target(&self, target: &InstallTarget) -> InstallTarget {
        InstallRepo::target(&*self.store, target.id)
            .await
            .unwrap()
            .unwrap()
    }

    /// Lets the worker run until the target reaches a terminal status.
    pub(super) async fn settled(&self, target: &InstallTarget) -> InstallTarget {
        for _ in 0..50_000 {
            let now = self.target(target).await;
            if now.status.is_terminal() {
                return now;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "the target never settled: {:?}",
            self.target(target).await.status
        );
    }

    /// Lets the worker run until the target has `status`.
    pub(super) async fn until(
        &self,
        target: &InstallTarget,
        status: TargetStatus,
    ) -> InstallTarget {
        for _ in 0..50_000 {
            let now = self.target(target).await;
            if now.status == status {
                return now;
            }
            tokio::task::yield_now().await;
        }
        panic!(
            "the target never reached {status:?}: {:?}",
            self.target(target).await.status
        );
    }

    /// Lets every ready task run.
    pub(super) async fn quiet(&self) {
        for _ in 0..200 {
            tokio::task::yield_now().await;
        }
    }

    pub(super) fn set_availability(&self, host: HostId, id: &str, state: Availability) {
        self.registry.set_availability(host, &contract(id), state);
    }

    /// Every audit row of `kind`, oldest first.
    pub(super) async fn audit(&self, kind: &str) -> Vec<AuditEvent> {
        let filter = AuditFilter {
            kind: Some(kind.to_owned()),
            ..AuditFilter::default()
        };
        let mut rows = AuditLog::list(&*self.store, &filter, &Page::first(500))
            .await
            .unwrap()
            .items;
        rows.reverse();
        rows
    }

    /// The `(from, to)` pairs of a job's journal rows, oldest first.
    pub(super) async fn transitions(&self, target: &InstallTarget) -> Vec<(String, String)> {
        let rows =
            JournalRead::entries_for(&*self.store, &AggregateRef::install_job(target.job), 500)
                .await
                .unwrap();
        rows.into_iter()
            .filter(|row| row.kind == "install.target")
            .filter(|row| row.payload["target"] == target.id.to_string())
            .map(|row| {
                (
                    row.payload["from"].as_str().unwrap().to_owned(),
                    row.payload["to"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    }

    /// The outputs a successful install of `artifact` reports.
    pub(super) fn install_outputs(artifact: &PackageArtifact) -> BTreeMap<String, String> {
        BTreeMap::from([
            (keys::OUT_PACKAGE.to_owned(), artifact.name.to_string()),
            (keys::OUT_VERSION.to_owned(), artifact.version.to_string()),
            (keys::OUT_KEY_ID.to_owned(), artifact.key_id.to_string()),
            (
                keys::OUT_ARCHIVE_SHA256.to_owned(),
                artifact.archive_sha256.to_string(),
            ),
            (keys::OUT_ENABLED.to_owned(), "false".to_owned()),
            (keys::OUT_SKILLS.to_owned(), "restart,status".to_owned()),
            (
                keys::OUT_PRIVILEGE_BOUNDARY.to_owned(),
                "in_effect".to_owned(),
            ),
        ])
    }

    /// Makes every capability answer succeed with the outputs of `artifact`.
    pub(super) fn agent_installs(&self, artifact: &PackageArtifact) {
        let outputs = Self::install_outputs(artifact);
        self.link
            .on_capability(move |_, _| Ok(PackageOutputs(outputs.clone())));
    }

    /// Makes every capability answer be `error`.
    pub(super) fn agent_answers(&self, error: LinkError) {
        self.link.on_capability(move |_, _| Err(error.clone()));
    }

    pub(super) fn digest_of(bytes: &[u8]) -> ArchiveDigest {
        ArchiveDigest::of_bytes(bytes)
    }
}

fn spawn(
    store: &Arc<InMemoryStore>,
    clock: &Arc<FakeClock>,
    link: &Arc<FakeAgentLink>,
    registry: &Arc<FakeCapabilityRegistry>,
    files: &Arc<FakePackageFiles>,
) -> (PackageHandle, CancellationToken) {
    let shutdown = CancellationToken::new();
    let (handle, _task) = InstallService::spawn(
        PackageDeps {
            installs: store.clone(),
            uow: store.clone(),
            hosts: store.clone(),
            resolver: Arc::new(FakeTargetResolver::new()),
            registry: registry.clone(),
            caps: package_caps(link.clone()),
            files: files.clone(),
            clock: clock.clone(),
            shutdown: shutdown.clone(),
        },
        PackageSettings::default(),
    );
    (handle, shutdown)
}

/// The window past the default approval window, for tests that let an approval run out.
pub(super) const PAST_WINDOW: SignedDuration = SignedDuration::from_mins(11);

impl World {
    /// The journal epoch `add_host` gave `host`.
    pub(super) fn epoch(host: HostId) -> JournalEpoch {
        JournalEpoch::from_uuid(Uuid::from_u128(host.as_uuid().as_u128()))
    }

    /// Rewrites a stored target as a crash would have left it.
    pub(super) async fn force(
        &self,
        target: &InstallTarget,
        change: impl FnOnce(&mut InstallTarget),
    ) -> InstallTarget {
        let current = self.target(target).await;
        let mut next = current.clone();
        change(&mut next);
        let tx = Transaction::new().write(StateWrite::Install(InstallWrite::Target {
            target: next,
            expected: TargetGuard {
                status: current.status,
                version: current.version,
            },
        }));
        UnitOfWork::commit(&*self.store, tx).await.unwrap();
        self.target(target).await
    }

    /// Leaves a target dispatched under the host's current session and epoch, as a hub that
    /// stopped right after the dispatch commit would.
    pub(super) async fn dispatched(&self, target: &InstallTarget) -> InstallTarget {
        let host = target.host;
        let session = self.link.session_id(host);
        let epoch = Self::epoch(host);
        let now = self.clock.now();
        self.force(target, |t| {
            t.status = TargetStatus::Dispatched;
            t.dispatched_session = Some(session);
            t.dispatched_epoch = Some(epoch);
            t.started_at = Some(now);
        })
        .await
    }
}
