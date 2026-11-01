//! Single-box mode: an agent in the hub's own process (ARCHITECTURE.md §7.3).

use std::sync::Arc;

use domovoi_core::ports::{Clock, HostRepo};
use domovoi_core::{AgentId, AgentIdentity, HostId};
use domovoi_proto::Transport;
use domovoi_store::Store;

use super::{RuntimeError, adapters};
use crate::agent_server::AgentServer;

/// What a local agent needs to save as its identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalEnrollment {
    /// The agent's id.
    pub agent_id: AgentId,
    /// The hub's own host, which the local agent watches.
    pub host_id: HostId,
    /// SHA-256 of the hub certificate, kept for parity with a remote identity.
    pub hub_fingerprint: [u8; 32],
}

/// Registers and attaches agents running in the hub process. A local agent is trusted
/// because it shares the process: it is enrolled for the hub's own host without a token,
/// then speaks the normal protocol, handshake included, over an in-memory transport.
#[derive(Clone)]
pub struct LocalAgents {
    server: AgentServer,
    store: Arc<Store>,
    clock: Arc<dyn Clock>,
}

impl LocalAgents {
    pub(crate) fn new(server: AgentServer, store: Arc<Store>, clock: Arc<dyn Clock>) -> Self {
        Self {
            server,
            store,
            clock,
        }
    }

    /// Enrolls `public_key` as the agent of the hub's own host. The same key gets the same
    /// agent back; a new key replaces (and revokes) the previous local agent.
    pub async fn register(
        &self,
        public_key: [u8; 32],
        version: &str,
    ) -> Result<LocalEnrollment, RuntimeError> {
        let host = adapters::hub_host(&self.store).await?;
        let now = self.clock.now();
        let agent_id = match self.store.live_agent(host).await? {
            Some(agent) if agent.public_key == public_key => agent.agent,
            existing => {
                if let Some(old) = existing {
                    self.store.revoke_agent(old.agent, now).await?;
                    tracing::info!(agent = %old.agent, "local agent key changed; previous agent revoked");
                }
                let agent = AgentIdentity {
                    agent: AgentId::new(),
                    host,
                    public_key,
                    version: version.to_owned(),
                    enrolled_at: now,
                    revoked_at: None,
                };
                self.store.put_agent(&agent).await?;
                tracing::info!(agent = %agent.agent, "local agent enrolled");
                agent.agent
            }
        };
        Ok(LocalEnrollment {
            agent_id,
            host_id: host,
            hub_fingerprint: self.server.fingerprint(),
        })
    }

    /// Runs one local connection over `transport` until it ends.
    pub async fn attach(&self, transport: Box<dyn Transport>) {
        self.server.attach_local(transport).await;
    }
}
