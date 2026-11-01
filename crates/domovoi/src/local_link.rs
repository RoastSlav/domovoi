//! Single-box mode (ARCHITECTURE.md §7.3): the agent runs in the hub's process and talks
//! to it over an in-memory transport pair, through the same handshake and protocol as a
//! remote agent. The pair is the hub's [`MemoryTransport`], which already implements
//! `domovoi_proto::Transport` over bounded `tokio::sync::mpsc` channels.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_agent::LinkError;
use domovoi_agent::link::ws::{Connected, Connector};
use domovoi_agent::local::{Registered, local_identity};
use domovoi_agent::{AgentRuntime, production};
use domovoi_config::AgentConfig;
use domovoi_core::ports::{Clock, ProcessExecutor};
use domovoi_hub::agent_server::{LOCAL_SESSION_HINT, MemoryTransport};
use domovoi_hub::runtime::LocalAgents;
use tokio_util::sync::CancellationToken;

/// Dials the hub in this process: every connection is a fresh in-memory pair whose hub
/// end is attached to the agent server.
pub struct LocalConnector {
    local: LocalAgents,
}

#[async_trait]
impl Connector for LocalConnector {
    async fn connect(&self) -> Result<Connected, LinkError> {
        let (hub_end, agent_end) = MemoryTransport::pair();
        let local = self.local.clone();
        tokio::spawn(async move { local.attach(Box::new(hub_end)).await });
        Ok(Connected {
            transport: Box::new(agent_end),
            session_hint: LOCAL_SESSION_HINT.to_owned(),
        })
    }
}

/// Registers this machine's agent with the hub (generating its key on first start) and
/// runs it until `shutdown`.
pub async fn run_agent(
    local: LocalAgents,
    config: AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let registrar = local.clone();
    local_identity(&config.agent.state_dir, |public_key| async move {
        registrar
            .register(public_key, env!("CARGO_PKG_VERSION"))
            .await
            .map(|enrolled| Registered {
                agent_id: enrolled.agent_id,
                host_id: enrolled.host_id,
                hub_fingerprint: enrolled.hub_fingerprint,
            })
            .map_err(|err| err.to_string())
    })
    .await?;
    let redactor = crate::cmd::redactor(&config);
    let deps = production::deps(&config, executor, clock, redactor)?;
    let agent = AgentRuntime::new(config, deps)
        .await?
        .with_connector(Arc::new(LocalConnector { local }));
    tracing::info!("local agent started");
    agent.run(shutdown).await?;
    Ok(())
}
