//! The scripted catalogue one [`FakeAgentLink`] serves.
//!
//! [`FakeAgentLink`]: super::FakeAgentLink

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_core::HostId;
use domovoi_core::capability::{CatalogPageData, CatalogQuery};
use domovoi_core::ports::LinkError;
use jiff::Timestamp;

use crate::lock;

/// What answers one catalogue page for a host.
pub(super) type CatalogHandler = Arc<
    dyn Fn(HostId, &CatalogQuery, Timestamp) -> Result<CatalogPageData, LinkError> + Send + Sync,
>;

/// The announced hashes, scripted pages and answers of one fake link.
#[derive(Default)]
pub(super) struct Catalogue {
    hashes: Mutex<BTreeMap<HostId, String>>,
    pages: Mutex<BTreeMap<HostId, BTreeMap<u32, CatalogPageData>>>,
    calls: Mutex<Vec<(HostId, CatalogQuery)>>,
    handler: Mutex<Option<CatalogHandler>>,
}

impl Catalogue {
    /// Announces `hash` for `host`.
    pub(super) fn set_hash(&self, host: HostId, hash: impl Into<String>) {
        lock(&self.hashes).insert(host, hash.into());
    }

    /// Scripts the pages a host answers with, in page order.
    pub(super) fn set_pages(&self, host: HostId, pages: Vec<CatalogPageData>) {
        let pages = pages
            .into_iter()
            .enumerate()
            .map(|(index, page)| (u32::try_from(index).unwrap_or(u32::MAX), page))
            .collect();
        lock(&self.pages).insert(host, pages);
    }

    /// Answers every page with `handler`, instead of the scripted ones.
    pub(super) fn on_page(&self, handler: CatalogHandler) {
        *lock(&self.handler) = Some(handler);
    }

    /// Every page requested so far, oldest first.
    pub(super) fn calls(&self) -> Vec<(HostId, CatalogQuery)> {
        lock(&self.calls).clone()
    }

    /// The hash announced for `host`, if any.
    pub(super) fn hash(&self, host: HostId) -> Option<String> {
        lock(&self.hashes).get(&host).cloned()
    }

    /// One page, or the refusal of a link with nothing scripted for it.
    pub(super) fn page(
        &self,
        host: HostId,
        query: &CatalogQuery,
        deadline: Timestamp,
    ) -> Result<CatalogPageData, LinkError> {
        lock(&self.calls).push((host, *query));
        if let Some(handler) = lock(&self.handler).clone() {
            return handler(host, query, deadline);
        }
        lock(&self.pages)
            .get(&host)
            .and_then(|pages| pages.get(&query.page).cloned())
            .ok_or_else(|| {
                LinkError::Rejected(format!("no catalogue page {} is scripted", query.page))
            })
    }
}
