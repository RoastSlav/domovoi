//! Where the management endpoints come from.
//!
//! The source is behind one trait so that replacing it touches this file only. The default is
//! the most conservative one the rules allow: the hub's address is the literal IP in the host
//! part of `[agent] hub_url` of the root-owned `agent.toml` (the agent cannot change that
//! file), and there are no link peers in this release. A hub URL that names a host gives no hub
//! address at all: resolving a name would churn the attested state with DNS, and a name the
//! agent controls must not decide an exclusion. The owner covers such a hub with a
//! `management_cidrs` entry.

use core::net::IpAddr;
use std::path::Path;

use domovoi_config::hub_url_ip;
use domovoi_core::ports::HostInspect;

use super::observe::ReadError;

/// The addresses a report names as management endpoints, besides the configured networks.
pub trait EndpointSource: Send + Sync {
    /// The hub's literal addresses.
    ///
    /// # Errors
    ///
    /// [`ReadError::ConfigUnreadable`] when the source cannot be read.
    fn hub_addrs(&self) -> Result<Vec<IpAddr>, ReadError>;

    /// The agent's link peers: an address and a port each.
    ///
    /// # Errors
    ///
    /// [`ReadError::ConfigUnreadable`] when the source cannot be read.
    fn link_peers(&self) -> Result<Vec<(IpAddr, u16)>, ReadError>;
}

/// The default source: `agent.toml`'s hub URL, and no link peers.
#[derive(Debug, Clone)]
pub struct AgentTomlEndpoints {
    hub: Result<Option<IpAddr>, ReadError>,
}

impl AgentTomlEndpoints {
    /// Reads `agent.toml` at `path` through `inspect`. A file that is missing, unreadable, not
    /// TOML or without a usable `[agent] hub_url` makes every later read fail.
    #[must_use]
    pub fn load(inspect: &dyn HostInspect, path: &Path) -> Self {
        let hub = inspect
            .read_text(path)
            .map_err(|_| ReadError::ConfigUnreadable)
            .and_then(|text| hub_url_ip(&text).map_err(|_| ReadError::ConfigUnreadable));
        Self { hub }
    }
}

impl EndpointSource for AgentTomlEndpoints {
    fn hub_addrs(&self) -> Result<Vec<IpAddr>, ReadError> {
        self.hub.clone().map(|ip| ip.into_iter().collect())
    }

    fn link_peers(&self) -> Result<Vec<(IpAddr, u16)>, ReadError> {
        self.hub.clone().map(|_| Vec::new())
    }
}
