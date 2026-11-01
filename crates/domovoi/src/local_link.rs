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
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Dials the hub in this process: every connection is a fresh in-memory pair whose hub
/// end is attached to the agent server of the runtime running now. While the hub
/// restarts there is none, and the agent retries with its usual backoff.
pub struct LocalConnector {
    current: watch::Receiver<Option<LocalAgents>>,
}

#[async_trait]
impl Connector for LocalConnector {
    async fn connect(&self) -> Result<Connected, LinkError> {
        let Some(local) = self.current.borrow().clone() else {
            return Err(LinkError::Connect("the hub is restarting".to_owned()));
        };
        let (hub_end, agent_end) = MemoryTransport::pair();
        tokio::spawn(async move { local.attach(Box::new(hub_end)).await });
        Ok(Connected {
            transport: Box::new(agent_end),
            session_hint: LOCAL_SESSION_HINT.to_owned(),
        })
    }
}

/// Registers this machine's agent with the hub (generating its key on first start) and
/// runs it until `shutdown`, following the hub across restarts through `current`.
pub async fn run_agent(
    mut current: watch::Receiver<Option<LocalAgents>>,
    config: AgentConfig,
    executor: Arc<dyn ProcessExecutor>,
    clock: Arc<dyn Clock>,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let registrar = current
        .wait_for(Option::is_some)
        .await?
        .clone()
        .ok_or_else(|| anyhow::anyhow!("the hub stopped before the local agent started"))?;
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
        .with_connector(Arc::new(LocalConnector { current }));
    tracing::info!("local agent started");
    agent.run(shutdown).await?;
    Ok(())
}
