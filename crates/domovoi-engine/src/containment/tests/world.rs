//! The harness: the driver's ports over the in-memory store, a fake clock and the scripted relay.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domovoi_core::action::{AuthorizationKind, ContainmentPlan};
use domovoi_core::attest::{Authority, Cidr, TrustView};
use domovoi_core::containment::{ContainOutputs, Net, keys};
use domovoi_core::ports::{
    AuditEvent, AuditFilter, AuditLog, BlockRow, CommitError, CommitReceipt, ContainmentRepo, Page,
    Presence, Transaction, UnitOfWork,
};
use domovoi_core::{
    AgentSessionId, CheckKey, ExecutionId, ExecutionIntent, ExecutionRecord, HostId, JournalEpoch,
    OperationId, Risk, StepId, TargetKey,
};
use domovoi_testkit::build::{T0, host};
use domovoi_testkit::contain::contain_caps;
use domovoi_testkit::crash::CrashingStore;
use domovoi_testkit::helper_attest::{FakeHelperTrust, TestHelper, verified_view};
use domovoi_testkit::{
    FakeAgentLink, FakeClock, FakeContainRelay, FakeTargetResolver, InMemoryStore,
};
use jiff::SignedDuration;
use uuid::Uuid;

use crate::containment::input::assess;
use crate::containment::{
    AuthorizedContainment, ContainDeps, ContainmentDriver, PathSource, Prepared,
};

/// Discovered management paths a test sets.
#[derive(Default)]
pub(super) struct FakePaths {
    paths: Mutex<Vec<Cidr>>,
}

impl FakePaths {
    pub(super) fn set(&self, paths: Vec<Cidr>) {
        *self.paths.lock().unwrap() = paths;
    }
}

#[async_trait]
impl PathSource for FakePaths {
    async fn discovered(&self, _host: HostId) -> Result<Vec<Cidr>, String> {
        Ok(self.paths.lock().unwrap().clone())
    }
}

type Hook = Box<dyn Fn(&Transaction) + Send + Sync>;

/// The store with a hook that sees every transaction before it commits, so a test can spend time
/// or drop the session at an exact point.
pub(super) struct HookedUow {
    inner: Arc<InMemoryStore>,
    hook: Mutex<Option<Hook>>,
}

impl HookedUow {
    pub(super) fn on_commit(&self, hook: impl Fn(&Transaction) + Send + Sync + 'static) {
        *self.hook.lock().unwrap() = Some(Box::new(hook));
    }
}

#[async_trait]
impl UnitOfWork for HookedUow {
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError> {
        let receipt = UnitOfWork::commit(&*self.inner, tx.clone()).await;
        if let Some(hook) = self.hook.lock().unwrap().as_ref() {
            hook(&tx);
        }
        receipt
    }
}

pub(super) struct World {
    pub(super) store: Arc<InMemoryStore>,
    pub(super) clock: Arc<FakeClock>,
    pub(super) link: Arc<FakeAgentLink>,
    pub(super) relay: Arc<FakeContainRelay>,
    pub(super) trust: Arc<FakeHelperTrust>,
    pub(super) paths: Arc<FakePaths>,
    pub(super) uow: Arc<HookedUow>,
    pub(super) driver: Arc<ContainmentDriver>,
    pub(super) host: HostId,
}

pub(super) fn net(text: &str) -> Net {
    Net::parse(text).unwrap()
}

pub(super) fn ok(pairs: &[(&str, &str)]) -> Result<ContainOutputs, domovoi_core::ports::LinkError> {
    Ok(ContainOutputs(
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    ))
}

pub(super) fn rejected(message: &str) -> Result<ContainOutputs, domovoi_core::ports::LinkError> {
    Err(domovoi_core::ports::LinkError::Rejected(message.to_owned()))
}

impl World {
    pub(super) async fn new() -> Self {
        Self::build(false).await.0
    }

    /// A world whose unit of work crashes on a plan, and the store that runs the plan.
    pub(super) async fn crashing() -> (Self, Arc<CrashingStore<InMemoryStore>>) {
        let (world, crash) = Self::build(true).await;
        (world, crash.expect("a crashing build has a crashing store"))
    }

    async fn build(crash: bool) -> (Self, Option<Arc<CrashingStore<InMemoryStore>>>) {
        let store = Arc::new(InMemoryStore::new());
        let clock = Arc::new(FakeClock::new(T0));
        let link = Arc::new(FakeAgentLink::new());
        let relay = Arc::new(FakeContainRelay::new());
        relay.watch(store.clone());
        let trust = Arc::new(FakeHelperTrust::new());
        let paths = Arc::new(FakePaths::default());
        let uow = Arc::new(HookedUow {
            inner: store.clone(),
            hook: Mutex::new(None),
        });
        let resolver = Arc::new(FakeTargetResolver::new());
        let registered = host("nas").unwrap();
        domovoi_core::ports::HostRepo::upsert_host(&*store, &registered)
            .await
            .unwrap();
        let id = registered.id;
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
        let crashing = crash.then(|| Arc::new(CrashingStore::new(store.clone())));
        let unit: Arc<dyn UnitOfWork> = match &crashing {
            Some(crashing) => crashing.clone(),
            None => uow.clone(),
        };
        let deps = ContainDeps {
            caps: contain_caps(link.clone(), relay.clone()),
            uow: unit,
            blocks: store.clone(),
            trust: trust.clone(),
            paths: paths.clone(),
            resolver,
            clock: clock.clone(),
            result_grace: SignedDuration::from_secs(30),
        };
        let world = Self {
            store,
            clock,
            link,
            relay,
            trust,
            paths,
            uow,
            driver: ContainmentDriver::new(deps),
            host: id,
        };
        (world, crashing)
    }

    /// Makes the host's helper verified and fresh, covering the sample management networks.
    pub(super) fn verify(&self) -> TestHelper {
        let helper = TestHelper::new(7);
        let view: TrustView = verified_view(&helper, 1, 5, T0).unwrap();
        self.trust.set(self.host, view);
        self.paths
            .set(vec![Cidr::host("192.0.2.10".parse().unwrap())]);
        helper
    }

    /// An approved containment step for `source`, the plan the driver would hash, and the session
    /// and journal epoch it would be committed against.
    pub(super) async fn step(
        &self,
        source: &str,
        ttl_s: i64,
        authority: Authority,
    ) -> (
        AuthorizedContainment,
        Prepared,
        AgentSessionId,
        JournalEpoch,
    ) {
        let net = net(source);
        let ttl = SignedDuration::from_secs(ttl_s);
        let deps = self.driver.deps();
        let assessment = assess(&*deps.trust, &*deps.paths, self.host, authority, T0)
            .await
            .unwrap();
        let key = domovoi_testkit::build::key_fingerprint();
        let plan = ContainmentPlan::new(&net, ttl, self.host, &key, &assessment.assurance).hash();
        let step = StepId::new();
        let intent = ExecutionIntent {
            contract: domovoi_core::ContractId::parse("firewall.block_source").unwrap(),
            contract_version: 1,
            host: self.host,
            check: CheckKey::parse("firewall.block").unwrap(),
            target: Some(TargetKey::parse(&format!("ipnet:{}", net.canonical())).unwrap()),
            args: BTreeMap::from([
                (keys::ARG_SOURCE.to_owned(), net.canonical()),
                (keys::ARG_TTL_S.to_owned(), ttl_s.to_string()),
            ]),
            retry_of: None,
        };
        let expires_at = T0.checked_add(SignedDuration::from_secs(600)).unwrap();
        let record = ExecutionRecord::propose(
            &intent,
            step,
            OperationId::new(),
            plan,
            expires_at,
            Risk::Destructive,
            T0,
        );
        let session = deps
            .caps
            .observer
            .observe_snapshot(self.host)
            .unwrap()
            .session;
        let epoch = deps.caps.journal.epoch_of(self.host).unwrap();
        let prepared = Prepared {
            net,
            ttl,
            plan_hash: plan,
            assessment,
            authorization: match authority {
                Authority::Human => AuthorizationKind::Human,
                Authority::Policy => AuthorizationKind::Policy,
            },
            expires_at,
        };
        (
            AuthorizedContainment::for_test(record),
            prepared,
            session,
            epoch,
        )
    }

    /// The stored block `id`.
    pub(super) async fn block(&self, id: ExecutionId) -> BlockRow {
        self.store.get_block(id).await.unwrap().unwrap()
    }
}

impl World {
    /// Every audit row, oldest first.
    pub(super) async fn audits(&self) -> Vec<AuditEvent> {
        let mut rows = AuditLog::list(&*self.store, &AuditFilter::default(), &Page::first(500))
            .await
            .unwrap()
            .items;
        rows.reverse();
        rows
    }
}

impl World {
    /// What the hub holds about the host's helper.
    pub(super) async fn trust_view(&self) -> TrustView {
        self.driver.deps().trust.view(self.host).await.unwrap()
    }
}

impl World {
    /// The stored block `id`, if there is one.
    pub(super) async fn store_block(&self, id: ExecutionId) -> Option<BlockRow> {
        self.store.get_block(id).await.unwrap()
    }
}
