//! What the registry reads from one host's agent, as the hub's capability layer sees it.
//!
//! The registry combines an agent's own words with hub state, but it must not hold the raw
//! link to ask for them: the composition root turns the link into this narrow view, exactly
//! as it does for the engine's capability objects (ADR 0023, EXEC-4). Nothing here can run a
//! tool, dispatch a skill or approve anything.

use async_trait::async_trait;

use domovoi_core::HostId;
use domovoi_core::capability::{CatalogPageData, CatalogQuery};
use domovoi_core::ports::{HostAvailability, HostView, LinkError};
use jiff::Timestamp;

/// The read-only facts one host's agent reports to the capability registry.
#[async_trait]
pub trait HostCapabilities: Send + Sync {
    /// Whether the host's agent is connected, gone, never seen, or refused.
    async fn availability(&self, host: HostId) -> HostAvailability;

    /// The catalogue hash the agent announced on its last handshake, if it announced one.
    fn catalog_hash(&self, host: HostId) -> Option<String>;

    /// One page of the agent's capability catalogue.
    async fn catalog_page(
        &self,
        host: HostId,
        query: CatalogQuery,
        deadline: Timestamp,
    ) -> Result<CatalogPageData, LinkError>;

    /// The 1.2 status words and announced skills of `host`'s live session, read together, or
    /// `None` when no session is live.
    fn view(&self, host: HostId) -> Option<HostView>;
}
