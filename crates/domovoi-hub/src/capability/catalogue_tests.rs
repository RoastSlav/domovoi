//! The catalogue walk's state machine, driven through a scripted link.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::HostId;
use domovoi_core::capability::{
    CatalogImplementation, CatalogPageData, CatalogueGap, CatalogueState, ContractId,
    ContractIndex, ContractVersion, builtin_contracts, catalog_hash,
};
use domovoi_core::ports::Presence;
use domovoi_testkit::{FakeAgentLink, FakeClock};
use jiff::{SignedDuration, Timestamp};

use super::CatalogueCache;

const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);

fn id(text: &str) -> ContractId {
    match ContractId::parse(text) {
        Ok(id) => id,
        Err(err) => panic!("{text} is not a contract id: {err}"),
    }
}

fn version(value: u32) -> ContractVersion {
    match ContractVersion::new(value) {
        Ok(version) => version,
        Err(err) => panic!("{value} is not a version: {err}"),
    }
}

fn implementation(contract: &str, at: u32) -> CatalogImplementation {
    CatalogImplementation {
        contract: id(contract),
        version: version(at),
    }
}

fn page(entries: Vec<CatalogImplementation>, next: Option<u32>) -> CatalogPageData {
    CatalogPageData {
        entries,
        next_page: next,
    }
}

/// The hash an agent announces for `entries`.
fn hash_of(entries: &[(&str, u32)]) -> String {
    let map: BTreeMap<ContractId, ContractVersion> = entries
        .iter()
        .map(|(contract, at)| (id(contract), version(*at)))
        .collect();
    catalog_hash(&map)
}

/// A connected host, its fake link and the cache over both.
struct Fixture {
    host: HostId,
    link: Arc<FakeAgentLink>,
    clock: Arc<FakeClock>,
    cache: CatalogueCache,
}

impl Fixture {
    fn new() -> Self {
        let host = HostId::new();
        let link = Arc::new(FakeAgentLink::new());
        link.set_presence(
            host,
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
        );
        let clock = Arc::new(FakeClock::new(T0));
        let contracts = match ContractIndex::new(builtin_contracts()) {
            Ok(index) => Arc::new(index),
            Err(err) => panic!("the built-in index is invalid: {err}"),
        };
        let agent: Arc<dyn domovoi_core::ports::AgentLink> = link.clone();
        Self {
            host,
            cache: CatalogueCache::new(
                Arc::new(crate::runtime::caps::LinkCapabilities::new(agent)),
                clock.clone(),
                contracts,
            ),
            link,
            clock,
        }
    }

    /// The pages asked for so far.
    fn calls(&self) -> usize {
        self.link.catalog_calls().len()
    }
}

#[tokio::test]
async fn catalogue_walk_stops_on_a_repeating_cursor() {
    let fixture = Fixture::new();
    fixture.link.set_catalog_hash(fixture.host, hash_of(&[]));
    fixture.link.set_catalog(
        fixture.host,
        vec![page(vec![implementation("container.logs", 1)], Some(0))],
    );

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert!(
        matches!(
            snapshot.state,
            CatalogueState::Partial {
                gap: CatalogueGap::CursorNotAdvancing { page: 0 },
            }
        ),
        "{:?}",
        snapshot.state
    );
    assert!(fixture.calls() <= 2);
}

#[tokio::test]
async fn catalogue_walk_bounds_the_page_count() {
    let fixture = Fixture::new();
    fixture.link.set_catalog_hash(fixture.host, hash_of(&[]));
    let pages = (0..51)
        .map(|index| page(Vec::new(), Some(index + 1)))
        .collect();
    fixture.link.set_catalog(fixture.host, pages);

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert!(
        matches!(
            snapshot.state,
            CatalogueState::Partial {
                gap: CatalogueGap::TooManyPages,
            }
        ),
        "{:?}",
        snapshot.state
    );
    assert_eq!(fixture.calls(), 50);
}

#[tokio::test]
async fn catalogue_hash_mismatch_is_partial() {
    let fixture = Fixture::new();
    fixture.link.set_catalog_hash(fixture.host, "a".repeat(64));
    fixture.link.set_catalog(
        fixture.host,
        vec![page(vec![implementation("container.logs", 1)], None)],
    );

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert!(
        matches!(
            snapshot.state,
            CatalogueState::Partial {
                gap: CatalogueGap::HashMismatch,
            }
        ),
        "{:?}",
        snapshot.state
    );
    assert_eq!(snapshot.entries.len(), 1);
}

#[tokio::test]
async fn catalogue_reconnect_with_a_new_hash_replaces_the_snapshot() {
    let fixture = Fixture::new();
    let first = hash_of(&[("container.logs", 1)]);
    fixture.link.set_catalog_hash(fixture.host, first.clone());
    fixture.link.set_catalog(
        fixture.host,
        vec![page(vec![implementation("container.logs", 1)], None)],
    );

    let walked = fixture.cache.catalog(fixture.host).await;
    assert_eq!(walked.state, CatalogueState::Complete { hash: first });

    // A new session, a different list: the announcement moved on, so the snapshot read
    // under the old one cannot answer for it.
    fixture.link.new_session(fixture.host);
    let second = hash_of(&[("container.events", 2)]);
    fixture.link.set_catalog_hash(fixture.host, second.clone());
    fixture.link.set_catalog(
        fixture.host,
        vec![page(vec![implementation("container.events", 2)], None)],
    );

    let rewalked = fixture.cache.catalog(fixture.host).await;

    assert_eq!(rewalked.state, CatalogueState::Complete { hash: second });
    assert_eq!(
        rewalked.entries.into_keys().collect::<Vec<_>>(),
        vec![id("container.events")]
    );
    assert_eq!(fixture.calls(), 2);
}

#[tokio::test]
async fn catalogue_walk_uses_one_deadline() {
    let fixture = Fixture::new();
    fixture.link.set_catalog_hash(fixture.host, hash_of(&[]));
    let clock = fixture.clock.clone();
    fixture.link.on_catalog(move |_host, _query, _deadline| {
        // The page arrives, but the walk as a whole has run out of time by the time the
        // next one would be asked for.
        clock.advance(SignedDuration::from_secs(31));
        Ok(page(Vec::new(), Some(1)))
    });

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert!(
        matches!(
            snapshot.state,
            CatalogueState::Partial {
                gap: CatalogueGap::Deadline,
            }
        ),
        "{:?}",
        snapshot.state
    );
    assert!(fixture.calls() <= 2);
}

#[tokio::test]
async fn catalogue_walk_reads_sequential_pages_to_complete() {
    let fixture = Fixture::new();
    let listed = [("container.logs", 1), ("container.inspect", 1)];
    fixture
        .link
        .set_catalog_hash(fixture.host, hash_of(&listed));
    fixture.link.set_catalog(
        fixture.host,
        vec![
            page(vec![implementation("container.logs", 1)], Some(1)),
            page(vec![implementation("container.inspect", 1)], None),
        ],
    );

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert_eq!(
        snapshot.state,
        CatalogueState::Complete {
            hash: hash_of(&listed)
        }
    );
    assert_eq!(snapshot.pages, 2);
    assert_eq!(fixture.calls(), 2);
}

#[tokio::test]
async fn catalogue_walk_stops_on_a_backwards_cursor() {
    let fixture = Fixture::new();
    fixture.link.set_catalog_hash(fixture.host, hash_of(&[]));
    fixture.link.set_catalog(
        fixture.host,
        vec![
            page(vec![implementation("container.logs", 1)], Some(3)),
            page(Vec::new(), None),
            page(Vec::new(), None),
            page(vec![implementation("container.inspect", 1)], Some(1)),
        ],
    );

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert!(
        matches!(
            snapshot.state,
            CatalogueState::Partial {
                gap: CatalogueGap::CursorNotAdvancing { page: 3 },
            }
        ),
        "{:?}",
        snapshot.state
    );
}

#[tokio::test]
async fn a_skipped_page_never_ends_complete() {
    // The agent announces a list of three entries but serves page 0 and page 2, skipping
    // the page that holds the third. Strictly-increasing cursors are allowed by the walk;
    // the hash is what proves the list, so the walk must not be Complete.
    let fixture = Fixture::new();
    let listed = [
        ("container.logs", 1),
        ("container.inspect", 1),
        ("container.events", 1),
    ];
    fixture
        .link
        .set_catalog_hash(fixture.host, hash_of(&listed));
    fixture.link.set_catalog(
        fixture.host,
        vec![
            page(vec![implementation("container.logs", 1)], Some(2)),
            page(vec![implementation("container.inspect", 1)], None),
            page(vec![implementation("container.events", 1)], None),
        ],
    );

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert_eq!(
        snapshot.state,
        CatalogueState::Partial {
            gap: CatalogueGap::HashMismatch,
        },
        "{:?}",
        snapshot.state
    );
}

#[tokio::test]
async fn an_empty_terminal_page_ends_the_walk() {
    let fixture = Fixture::new();
    fixture.link.set_catalog_hash(fixture.host, hash_of(&[]));
    fixture
        .link
        .set_catalog(fixture.host, vec![page(Vec::new(), None)]);

    let snapshot = fixture.cache.catalog(fixture.host).await;

    assert_eq!(
        snapshot.state,
        CatalogueState::Complete { hash: hash_of(&[]) }
    );
    assert_eq!(snapshot.pages, 1);
}
/// A host whose page request waits on a gate, so a test can hold one walk and start another.
struct GatedHost {
    announced: String,
    listed: CatalogPageData,
    calls: std::sync::atomic::AtomicUsize,
    entered: Arc<tokio::sync::Semaphore>,
    gate: Arc<tokio::sync::Semaphore>,
}

#[async_trait::async_trait]
impl super::HostCapabilities for GatedHost {
    async fn availability(&self, _host: HostId) -> domovoi_core::ports::HostAvailability {
        domovoi_core::ports::HostAvailability::Known { since: T0 }
    }

    fn catalog_hash(&self, _host: HostId) -> Option<String> {
        Some(self.announced.clone())
    }

    async fn catalog_page(
        &self,
        _host: HostId,
        _query: domovoi_core::capability::CatalogQuery,
        _deadline: Timestamp,
    ) -> Result<CatalogPageData, domovoi_core::ports::LinkError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.entered.add_permits(1);
        let _permit = self.gate.acquire().await;
        Ok(self.listed.clone())
    }

    fn view(&self, _host: HostId) -> Option<domovoi_core::ports::HostView> {
        None
    }
}

#[tokio::test]
async fn concurrent_queries_share_one_walk() {
    let host = HostId::new();
    let listed = vec![("container.logs", 1)];
    let gated = Arc::new(GatedHost {
        announced: hash_of(&listed),
        listed: page(vec![implementation("container.logs", 1)], None),
        calls: std::sync::atomic::AtomicUsize::new(0),
        entered: Arc::new(tokio::sync::Semaphore::new(0)),
        gate: Arc::new(tokio::sync::Semaphore::new(0)),
    });
    let contracts = match ContractIndex::new(builtin_contracts()) {
        Ok(index) => Arc::new(index),
        Err(err) => panic!("the built-in index is invalid: {err}"),
    };
    let cache = Arc::new(CatalogueCache::new(
        gated.clone(),
        Arc::new(FakeClock::new(T0)),
        contracts,
    ));

    let first = tokio::spawn({
        let cache = cache.clone();
        async move { cache.catalog(host).await }
    });
    let second = tokio::spawn({
        let cache = cache.clone();
        async move { cache.catalog(host).await }
    });
    // The first caller is inside its page request; the second is waiting on the walk gate.
    let _entered = gated.entered.acquire().await;
    gated.gate.add_permits(1);
    let (one, two) = (first.await.expect("first"), second.await.expect("second"));

    assert_eq!(one.state, two.state);
    assert_eq!(
        gated.calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "two queries walked the same catalogue twice"
    );
}
