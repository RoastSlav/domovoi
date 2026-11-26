//! Single-box mode: an agent in the hub's own process (ARCHITECTURE.md §7.3).

use std::sync::Arc;

use domovoi_core::ports::{Clock, HostRepo};
use domovoi_core::{AgentId, AgentIdentity, HostId};
use domovoi_proto::Transport;
use domovoi_store::Store;

use super::{RuntimeError, adapters};
use crate::agent_server::AgentServer;

/// What a local agent needs to save as its identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalEnrollment {
    /// The agent's id.
    pub agent_id: AgentId,
    /// The hub's own host, which the local agent watches.
    pub host_id: HostId,
    /// The name of that host: `[hub] name`, whatever the agent's own `host_name` says.
    pub host_name: String,
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
                    // The same retirement as a token re-enrollment or an admin revoke: the
                    // old identity's live session ends and a handshake already in flight for
                    // it cannot register itself later.
                    self.server.retire_agent(old.agent);
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
        let host_name = self
            .store
            .get_host(host)
            .await?
            .map_or_else(|| host.to_string(), |found| found.name.to_string());
        Ok(LocalEnrollment {
            agent_id,
            host_id: host,
            host_name,
            hub_fingerprint: self.server.fingerprint(),
        })
    }

    /// Runs one local connection over `transport` until it ends.
    pub async fn attach(&self, transport: Box<dyn Transport>) {
        self.server.attach_local(transport).await;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use domovoi_core::ports::{HostRepo, SettingsRepo};
    use domovoi_core::{Host, HostId, HostKind, HostName};
    use domovoi_store::{Store, StoreOptions};
    use domovoi_testkit::FakeClock;
    use jiff::Timestamp;

    use super::LocalAgents;
    use crate::agent_server::{AgentServer, AgentServerDeps, NetProbes, ServerSettings};

    const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);

    struct NoProbes;

    #[async_trait]
    impl NetProbes for NoProbes {
        async fn gateway_reachable(&self) -> bool {
            true
        }

        async fn tcp_reachable(&self, _target: &str) -> bool {
            true
        }
    }

    fn settings(dir: &std::path::Path) -> ServerSettings {
        ServerSettings {
            hub_name: "testhub".to_owned(),
            data_dir: dir.to_path_buf(),
            bind: "127.0.0.1:0".parse().unwrap(),
            public_url: url::Url::parse("wss://testhub:7421").unwrap(),
            self_check: "1.1.1.1:53".to_owned(),
            heartbeat: jiff::SignedDuration::from_secs(10),
            probes: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn replacing_a_local_key_retires_the_previous_agent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hub.db");
        let store = Arc::new(
            Store::open(&path, StoreOptions::beside(&path, T0))
                .await
                .unwrap(),
        );
        let host = HostId::new();
        HostRepo::upsert_host(
            &*store,
            &Host {
                id: host,
                name: HostName::parse("hubbox").unwrap(),
                kind: HostKind::Local,
                labels: BTreeMap::new(),
                created_at: T0,
            },
        )
        .await
        .unwrap();
        SettingsRepo::set(&*store, "hub_host_id", &serde_json::json!(host.to_string()))
            .await
            .unwrap();

        let clock: Arc<dyn domovoi_core::ports::Clock> = Arc::new(FakeClock::new(T0));
        let deps = AgentServerDeps {
            clock: clock.clone(),
            hosts: store.clone(),
            facts: store.clone(),
            sink: Arc::new(domovoi_testkit::RecordingSignalSink::new()),
            audit: store.clone(),
            probes: Arc::new(NoProbes),
            hub_host: host,
        };
        let server = AgentServer::new(settings(dir.path()), deps).unwrap();
        let local = LocalAgents::new(server.clone(), store.clone(), clock);

        let first = local.register([1; 32], "0.1.0").await.unwrap();
        assert_eq!(first.host_id, host);
        assert!(!server.is_retired(first.agent_id));

        let second = local.register([2; 32], "0.1.0").await.unwrap();
        assert_ne!(first.agent_id, second.agent_id);
        assert!(
            server.is_retired(first.agent_id),
            "the local path uses the shared retirement, closing the old session"
        );
        assert!(!server.is_retired(second.agent_id));
        assert_eq!(
            store.live_agent(host).await.unwrap().unwrap().agent,
            second.agent_id
        );

        let again = local.register([2; 32], "0.1.0").await.unwrap();
        assert_eq!(again.agent_id, second.agent_id, "the same key keeps its id");
        assert!(!server.is_retired(second.agent_id));
    }
}
