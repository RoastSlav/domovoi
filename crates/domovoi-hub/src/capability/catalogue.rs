//! The paged catalogue walk, and the per-host cache of what it read.
//!
//! One walk asks the agent for pages of ids and versions until its cursor ends, under one
//! deadline and one page bound, and then decides how much of the list it has proved. Nothing
//! here understands a contract: a page says which implementations exist, never what they may
//! do. An incomplete walk is reported as an incomplete walk, so a contract missing
//! from it is never read as missing from the host.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use domovoi_core::HostId;
use domovoi_core::capability::{
    CatalogQuery, CatalogSnapshot, CatalogueGap, CatalogueState, ContractId, ContractIndex,
    ContractVersion, catalog_hash, is_hash,
};
use domovoi_core::ports::{CATALOGUE_UNSUPPORTED, Clock, HostAvailability, LinkError};
use jiff::{SignedDuration, Timestamp};

use super::host::HostCapabilities;

/// Entries asked for in one page; the wire's cap.
const PAGE_SIZE: u32 = 100;
/// The most pages one walk reads before it gives up on proving the list.
const MAX_PAGES: u32 = 50;
/// How long one walk may take, in total, rather than per page.
const WALK: SignedDuration = SignedDuration::from_secs(30);

/// One host's catalogue as last read, and the walk that reads it.
///
/// The snapshot is keyed by the pair `(host, announced hash)`: it is replaced when the
/// agent announces a different hash, and reported stale until the re-walk finishes.
pub struct CatalogueCache {
    agent: Arc<dyn HostCapabilities>,
    clock: Arc<dyn Clock>,
    contracts: Arc<ContractIndex>,
    snapshots: Mutex<BTreeMap<HostId, CatalogSnapshot>>,
    /// One walk gate per host, so concurrent queries share one walk instead of repeating it.
    /// A host's gate never blocks another host's.
    walks: Mutex<BTreeMap<HostId, Arc<tokio::sync::Mutex<()>>>>,
}

impl CatalogueCache {
    /// A cache that walks `agent` for hosts whose contracts `contracts` describes.
    pub fn new(
        agent: Arc<dyn HostCapabilities>,
        clock: Arc<dyn Clock>,
        contracts: Arc<ContractIndex>,
    ) -> Self {
        Self {
            agent,
            clock,
            contracts,
            snapshots: Mutex::new(BTreeMap::new()),
            walks: Mutex::new(BTreeMap::new()),
        }
    }

    /// The host's catalogue, walking the agent when no verified snapshot can answer.
    ///
    /// A cached snapshot is served only when it is `Complete` and the agent still announces
    /// the hash it was walked under. Anything else walks again, because a list read under
    /// another hash says nothing about the list the agent runs now. Concurrent callers for
    /// one host share a single walk: the first holds the host's gate, and a waiter re-reads
    /// the cache before deciding to walk.
    pub async fn catalog(&self, host: HostId) -> CatalogSnapshot {
        if let Some(served) = self.served(host) {
            return served;
        }
        let gate = self.gate(host);
        let _walking = gate.lock().await;
        // The caller this one waited for may have finished the walk for this hash.
        if let Some(served) = self.served(host) {
            return served;
        }
        let announced = self.agent.catalog_hash(host);
        let cached = self.lock().get(&host).cloned();
        let Some(announced) = announced.as_deref().filter(|text| is_hash(text)) else {
            // Nothing announced, or nothing readable: no walk could prove a list against
            // it, so the entries are kept and the state says they are unverified.
            let gap = unannounced_gap(announced.as_deref());
            let unverified = self.unverified(host, cached.as_ref(), gap);
            self.lock().insert(host, unverified.clone());
            return unverified;
        };
        let announced = announced.to_owned();
        let walked = self.walk(host, announced).await;
        self.lock().insert(host, walked.clone());
        walked
    }

    /// The snapshot that answers `host` without walking, when the agent still announces the
    /// hash it was walked under.
    fn served(&self, host: HostId) -> Option<CatalogSnapshot> {
        let announced = self.agent.catalog_hash(host);
        let announced = announced.as_deref().filter(|text| is_hash(text))?;
        let cached = self.lock().get(&host).cloned()?;
        match &cached.state {
            CatalogueState::Complete { hash } if hash == announced => Some(cached),
            _ => None,
        }
    }

    /// The walk gate of one host.
    fn gate(&self, host: HostId) -> Arc<tokio::sync::Mutex<()>> {
        self.walks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(host)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// The cached snapshot for `host`, marked `Stale` when the announced hash moved on.
    ///
    /// Never walks, so a caller that must not wait (a page render, a diagnostic list) reads
    /// what is already known.
    pub fn cached(&self, host: HostId) -> Option<CatalogSnapshot> {
        let cached = self.lock().get(&host).cloned()?;
        let announced = self.agent.catalog_hash(host);
        let Some(announced) = announced.as_deref().filter(|text| is_hash(text)) else {
            let gap = unannounced_gap(announced.as_deref());
            return Some(self.unverified(host, Some(&cached), gap));
        };
        let announced = announced.to_owned();
        match &cached.state {
            CatalogueState::Complete { hash } if *hash == announced => Some(cached),
            CatalogueState::Complete { hash } => Some(CatalogSnapshot {
                state: CatalogueState::Stale {
                    announced: announced.clone(),
                    snapshot: hash.clone(),
                },
                announced_hash: Some(announced),
                ..cached
            }),
            CatalogueState::Stale { .. } | CatalogueState::Partial { .. } => Some(cached),
        }
    }

    /// A snapshot that keeps `cached`'s entries but says nothing is proved about them.
    fn unverified(
        &self,
        host: HostId,
        cached: Option<&CatalogSnapshot>,
        gap: CatalogueGap,
    ) -> CatalogSnapshot {
        CatalogSnapshot {
            host,
            announced_hash: None,
            state: CatalogueState::Partial { gap },
            entries: cached.map(|c| c.entries.clone()).unwrap_or_default(),
            unknown: cached.map(|c| c.unknown.clone()).unwrap_or_default(),
            pages: 0,
            session_since: cached.and_then(|c| c.session_since),
            at: self.clock.now(),
        }
    }

    /// Reads every page of the host's catalogue under one deadline.
    async fn walk(&self, host: HostId, announced: String) -> CatalogSnapshot {
        let started = self.clock.now();
        let deadline = started.saturating_add(WALK).unwrap_or(started);
        let session = match self.agent.availability(host).await {
            HostAvailability::Known { since } => since,
            _ => {
                return self.snapshot(
                    host,
                    Some(announced),
                    CatalogueState::Partial {
                        gap: CatalogueGap::Disconnected { after_page: 0 },
                    },
                    Entries::default(),
                    None,
                );
            }
        };

        let mut entries = Entries::default();
        let mut page = 0u32;
        loop {
            if entries.pages >= MAX_PAGES {
                return self.snapshot(
                    host,
                    Some(announced),
                    CatalogueState::Partial {
                        gap: CatalogueGap::TooManyPages,
                    },
                    entries,
                    Some(session),
                );
            }
            if self.clock.now() >= deadline {
                return self.snapshot(
                    host,
                    Some(announced),
                    CatalogueState::Partial {
                        gap: CatalogueGap::Deadline,
                    },
                    entries,
                    Some(session),
                );
            }
            let query = CatalogQuery {
                page,
                page_size: PAGE_SIZE,
            };
            let answer = self.agent.catalog_page(host, query, deadline).await;
            entries.pages += 1;
            let data = match answer {
                Ok(data) => data,
                Err(error) => {
                    let gap = gap_of(&error, page, entries.pages);
                    return self.snapshot(
                        host,
                        Some(announced),
                        CatalogueState::Partial { gap },
                        entries,
                        Some(session),
                    );
                }
            };
            for implementation in data.entries {
                if entries
                    .by_id
                    .insert(implementation.contract.clone(), implementation.version)
                    .is_some()
                {
                    let contract = implementation.contract;
                    return self.snapshot(
                        host,
                        Some(announced),
                        CatalogueState::Partial {
                            gap: CatalogueGap::DuplicateEntry { contract },
                        },
                        entries,
                        Some(session),
                    );
                }
            }
            match data.next_page {
                None => break,
                Some(next) if next > page => page = next,
                Some(_) => {
                    return self.snapshot(
                        host,
                        Some(announced),
                        CatalogueState::Partial {
                            gap: CatalogueGap::CursorNotAdvancing { page },
                        },
                        entries,
                        Some(session),
                    );
                }
            }
        }

        let state = self.verdict(host, &announced, session, &entries).await;
        self.snapshot(host, Some(announced), state, entries, Some(session))
    }

    /// What the walk proved: the whole list, or the first thing that stops it counting.
    async fn verdict(
        &self,
        host: HostId,
        announced: &str,
        session: Timestamp,
        entries: &Entries,
    ) -> CatalogueState {
        match self.agent.availability(host).await {
            HostAvailability::Known { since } if since == session => {}
            _ => {
                return CatalogueState::Partial {
                    gap: CatalogueGap::Disconnected {
                        after_page: entries.pages,
                    },
                };
            }
        }
        if self.agent.catalog_hash(host).as_deref() != Some(announced) {
            return CatalogueState::Partial {
                gap: CatalogueGap::HashMismatch,
            };
        }
        if catalog_hash(&entries.by_id) != announced {
            return CatalogueState::Partial {
                gap: CatalogueGap::HashMismatch,
            };
        }
        CatalogueState::Complete {
            hash: announced.to_owned(),
        }
    }

    /// Assembles the snapshot a walk reports, naming the ids the hub cannot describe.
    fn snapshot(
        &self,
        host: HostId,
        announced: Option<String>,
        state: CatalogueState,
        entries: Entries,
        session: Option<Timestamp>,
    ) -> CatalogSnapshot {
        let unknown = entries
            .by_id
            .keys()
            .filter(|id| self.contracts.get(id).is_none() && !id.is_skill())
            .map(ToString::to_string)
            .collect();
        CatalogSnapshot {
            host,
            announced_hash: announced,
            state,
            entries: entries.by_id,
            unknown,
            pages: entries.pages,
            session_since: session,
            at: self.clock.now(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<HostId, CatalogSnapshot>> {
        self.snapshots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// The entries one walk has collected, and how many pages it took.
#[derive(Default)]
struct Entries {
    by_id: BTreeMap<ContractId, ContractVersion>,
    pages: u32,
}

/// Why the announced hash cannot verify a walk: none arrived, or it is not 64 hex characters.
fn unannounced_gap(announced: Option<&str>) -> CatalogueGap {
    match announced {
        None => CatalogueGap::HashUnannounced,
        Some("") => CatalogueGap::HashUnannounced,
        Some(_) => CatalogueGap::HashMalformed,
    }
}

/// Why one page request did not produce a page.
fn gap_of(error: &LinkError, page: u32, pages: u32) -> CatalogueGap {
    match error {
        LinkError::Rejected(text) if text == CATALOGUE_UNSUPPORTED => CatalogueGap::Unsupported,
        // An unreadable reply: the page arrived, but its ids or versions did not parse.
        LinkError::Indeterminate(_) => CatalogueGap::PageMalformed { page },
        LinkError::Rejected(_) => CatalogueGap::PageFailed { page },
        _ => CatalogueGap::Disconnected { after_page: pages },
    }
}

#[cfg(test)]
#[path = "catalogue_tests.rs"]
mod tests;
