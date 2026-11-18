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
