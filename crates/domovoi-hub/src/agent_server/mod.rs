//! The hub side of the agent protocol (ARCHITECTURE.md §8, ADR 0003).
//!
//! A TLS WebSocket listener with a self-signed certificate that agents pin, single-use
//! enrollment tokens, a challenge signed with each agent's enrolled key, one session per
//! host, [`WsAgentLink`] for the engine, and presence checks that tell host down, agent
//! down and the hub's own network trouble apart.

mod conn;
mod error;
mod handshake;
mod link;
mod memory;
mod overrides;
pub(crate) use overrides::stored_overrides;
mod presence;
mod probes;
mod registry;
mod session;
mod settings;
mod tls;
mod tokens;
mod ws;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};

use domovoi_core::ports::{Clock, FactRepo, HostRepo, SignalSink};
use domovoi_core::{HostId, HostKind, HostName};
use domovoi_proto::Transport;
use jiff::SignedDuration;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

pub use error::AgentServerError;
pub use handshake::{BAD_TOKEN_DELAY, FIRST_FRAME_TIMEOUT, STEP_TIMEOUT};
pub use link::WsAgentLink;
pub use memory::MemoryTransport;
pub use probes::{NetProbes, RealNetProbes};
pub use registry::{AGENTS_CONNECTED, MAX_PENDING};
pub use settings::ServerSettings;
pub use tls::{CHANNEL_BINDING_LABEL, HUB_SAN, TlsIdentity, colon_hex};
pub use tokens::{EnrollmentInvite, TOKEN_TTL, token_hash};
pub use ws::MAX_FRAMES_PER_SECOND;

use registry::Registry;

/// What a local agent signs the challenge over: it has no TLS session to bind to.
pub const LOCAL_SESSION_HINT: &str = "local";

/// The ports the agent server works through.
pub struct AgentServerDeps {
    /// Time, for timeouts and presence.
    pub clock: Arc<dyn Clock>,
    /// Hosts, agents and enrollment tokens.
    pub hosts: Arc<dyn HostRepo>,
    /// Where `digest_facts` go.
    pub facts: Arc<dyn FactRepo>,
    /// Where agent signals and liveness signals go.
    pub sink: Arc<dyn SignalSink>,
    /// Network checks for presence.
    pub probes: Arc<dyn NetProbes>,
    /// The hub's own host, which `hub.network_degraded` is about.
    pub hub_host: HostId,
}

struct Inner {
    settings: ServerSettings,
    deps: AgentServerDeps,
    tls: TlsIdentity,
    acceptor: TlsAcceptor,
    registry: Arc<Registry>,
    link: Arc<WsAgentLink>,
    network_degraded: AtomicBool,
    shutdown: CancellationToken,
    /// Where per-host check overrides are read from, once the runtime provides it.
    overrides: OnceLock<Arc<dyn domovoi_core::ports::SettingsRepo>>,
}

/// The agent server. Cheap to clone; clones share one server.
#[derive(Clone)]
pub struct AgentServer {
    inner: Arc<Inner>,
}

impl AgentServer {
    /// A server with the TLS identity under `<data_dir>/tls`, created on first use.
    pub fn new(settings: ServerSettings, deps: AgentServerDeps) -> Result<Self, AgentServerError> {
        let dir = settings.data_dir.join("tls");
        let tls = TlsIdentity::load_or_create(&dir, &settings.hub_name, deps.clock.now())?;
        let config = tls.server_config().map_err(|e| AgentServerError::Tls {
            path: dir.display().to_string(),
            reason: e.to_string(),
        })?;
        let registry = Arc::new(Registry::default());
        let link = Arc::new(WsAgentLink::new(registry.clone(), deps.clock.clone()));
        Ok(Self {
            inner: Arc::new(Inner {
                settings,
                deps,
                tls,
                acceptor: TlsAcceptor::from(Arc::new(config)),
                registry,
                link,
                network_degraded: AtomicBool::new(false),
                shutdown: CancellationToken::new(),
                overrides: OnceLock::new(),
            }),
        })
    }

    /// SHA-256 of the hub certificate, which agents pin.
    #[must_use]
    pub fn fingerprint(&self) -> [u8; 32] {
        self.inner.tls.fingerprint()
    }

    /// The fingerprint as colon-separated hex.
    #[must_use]
    pub fn fingerprint_text(&self) -> String {
        self.inner.tls.fingerprint_text()
    }

    /// The link the engine uses to reach agents.
    #[must_use]
    pub fn link(&self) -> Arc<WsAgentLink> {
        self.inner.link.clone()
    }

    /// Binds the configured listen address.
    pub async fn bind(&self) -> Result<TcpListener, AgentServerError> {
        let addr = self.inner.settings.bind;
        TcpListener::bind(addr)
            .await
            .map_err(|e| AgentServerError::Listen(format!("{addr}: {e}")))
    }

    /// Accepts agents on `listener` until [`shutdown`](Self::shutdown). Run
    /// [`run_presence`](Self::run_presence) next to it.
    pub async fn serve(&self, listener: TcpListener) -> Result<(), AgentServerError> {
        loop {
            let accepted = tokio::select! {
                () = self.inner.shutdown.cancelled() => return Ok(()),
                accepted = listener.accept() => accepted,
            };
            match accepted {
                Ok((tcp, peer)) => {
                    let server = self.clone();
                    tokio::spawn(async move { conn::accept(&server.inner, tcp, peer).await });
                }
                Err(err) => tracing::warn!(%err, "could not accept an agent connection"),
            }
        }
    }

    /// Stops accepting, ends every session and stops the presence checks.
    pub fn shutdown(&self) {
        self.inner.shutdown.cancel();
    }

    /// Runs an agent in this process over `transport`: the same handshake and session as
    /// over the network, without TLS. The agent signs over [`LOCAL_SESSION_HINT`].
    /// Returns when the connection ends.
    pub async fn attach_local(&self, transport: Box<dyn Transport>) {
        conn::run(&self.inner, transport, LOCAL_SESSION_HINT, None).await;
    }

    /// A single-use enrollment token for the host called `name`, valid for
    /// [`TOKEN_TTL`]. A host that does not exist yet is created as a remote host.
    pub async fn create_token(
        &self,
        name: &HostName,
    ) -> Result<EnrollmentInvite, AgentServerError> {
        self.create_token_valid_for(name, TOKEN_TTL).await
    }

    /// Like [`create_token`](Self::create_token), valid for `ttl`.
    pub async fn create_token_valid_for(
        &self,
        name: &HostName,
        ttl: SignedDuration,
    ) -> Result<EnrollmentInvite, AgentServerError> {
        let inner = &self.inner;
        let invite = tokens::create(
            &*inner.deps.hosts,
            name,
            ttl,
            inner.deps.clock.now(),
            &inner.settings.public_url,
            &inner.tls.fingerprint_text(),
        )
        .await?;
        Ok(invite)
    }

    /// Revokes the host's agent and closes its live session. False when the host has no
    /// agent that is not already revoked.
    pub async fn revoke_agent(&self, host: HostId) -> Result<bool, AgentServerError> {
        let inner = &self.inner;
        let revoked = match inner.deps.hosts.live_agent(host).await? {
            Some(agent) => {
                inner
                    .deps
                    .hosts
                    .revoke_agent(agent.agent, inner.deps.clock.now())
                    .await?;
                tracing::info!(%host, agent = %agent.agent, "agent revoked");
                true
            }
            None => false,
        };
        inner.registry.close(host);
        Ok(revoked)
    }

    /// Runs one presence check now. [`serve`](Self::serve) does this every heartbeat.
    pub async fn check_presence(&self) {
        presence::check(&self.inner).await;
    }

    /// Starts watching every remote host with an enrolled agent, so one that never
    /// reconnects after a hub restart is still noticed.
    pub async fn watch_known_hosts(&self) -> Result<(), AgentServerError> {
        let inner = &self.inner;
        let now = inner.deps.clock.now();
        for host in inner.deps.hosts.list_hosts().await? {
            if host.kind == HostKind::Remote
                && inner.deps.hosts.live_agent(host.id).await?.is_some()
            {
                inner.registry.track(host.id, now);
            }
        }
        Ok(())
    }

    /// Watches the known hosts, then runs a presence check every heartbeat until
    /// [`shutdown`](Self::shutdown).
    pub async fn run_presence(&self) -> Result<(), AgentServerError> {
        self.watch_known_hosts().await?;
        let clock = self.inner.deps.clock.clone();
        loop {
            let now = clock.now();
            let next = now
                .saturating_add(self.inner.settings.heartbeat)
                .unwrap_or(now);
            tokio::select! {
                () = self.inner.shutdown.cancelled() => return Ok(()),
                () = clock.sleep_until(next) => {}
            }
            self.check_presence().await;
        }
    }
}
