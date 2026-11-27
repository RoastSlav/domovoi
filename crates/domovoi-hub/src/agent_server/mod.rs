//! The hub side of the agent protocol (ARCHITECTURE.md §8, ADR 0003).
//!
//! A TLS WebSocket listener with a self-signed certificate that agents pin, single-use
//! enrollment tokens, a challenge signed with each agent's enrolled key, one session per
//! host, `WsAgentLink` for the engine, and presence checks that tell host down, agent
//! down and the hub's own network trouble apart.

mod accept_log;
mod admin;
mod compat;
mod conn;
mod describe;
mod error;
mod handshake;
mod link;
mod link_answers;
mod link_helper;
mod live;
mod memory;
mod overrides;
pub(crate) use overrides::{stored_docker, stored_overrides};
mod package_report;
mod presence;
mod probes;
mod registry;
mod registry_state;
mod retire;
mod session;
mod settings;
#[cfg(test)]
mod tests;
mod tls;
mod tokens;
mod ws;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use domovoi_core::ports::{
    AgentLink, AuditLog, Clock, HostReconciler, HostRepo, IncidentFilter, IncidentRepo,
    MetricSampleRepo, Page, SignalSink, StoreError,
};
use domovoi_core::{HostId, HostKind, HostName};
use domovoi_proto::messages::{CheckInfo, HostFacts};
use domovoi_proto::{PROTOCOL_VERSION, Transport};
use jiff::SignedDuration;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

pub use admin::AgentAdmin;
pub use compat::{ProtocolState, protocol_state};
pub use error::AgentServerError;
pub use handshake::{BAD_TOKEN_DELAY, FIRST_FRAME_TIMEOUT, STEP_TIMEOUT};
#[cfg(any(test, feature = "test-support"))]
pub use link::WsAgentLink;
pub use memory::MemoryTransport;
pub use package_report::PackageReport;
pub use probes::{NetProbes, RealNetProbes};
pub use registry::{AGENTS_CONNECTED, MAX_PENDING};
pub use settings::ServerSettings;
pub use tls::{CHANNEL_BINDING_LABEL, HUB_SAN, TlsIdentity, colon_hex};
pub use tokens::{EnrollmentInvite, TOKEN_TTL, token_hash};
pub use ws::MAX_FRAMES_PER_SECOND;

#[cfg(not(any(test, feature = "test-support")))]
pub(crate) use link::WsAgentLink;

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
    pub facts: Arc<dyn MetricSampleRepo>,
    /// Where agent signals and liveness signals go.
    pub sink: Arc<dyn SignalSink>,
    /// Where a change of an agent's reported protocol is recorded.
    pub audit: Arc<dyn AuditLog>,
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
    /// Hosts whose liveness incidents were closed after this hub started, once their agent
    /// was seen alive. The raised state is memory only, so an incident raised before a
    /// restart would otherwise stay open after the agent is back.
    reconciled: std::sync::Mutex<std::collections::BTreeSet<HostId>>,
    shutdown: CancellationToken,
    /// Where per-host check overrides are read from, once the runtime provides it.
    overrides: OnceLock<Arc<dyn domovoi_core::ports::SettingsRepo>>,
    /// The action service, once the runtime has it, so an authenticated agent's
    /// dispatched runs can be reconciled against its journal.
    reconciler: OnceLock<Arc<dyn HostReconciler>>,
    /// Where each host's last announced package skills are kept.
    package_reports: OnceLock<Arc<dyn domovoi_core::ports::SettingsRepo>>,
    /// Told when a host's agent reports new runtime status, so the helper attestation can be
    /// refreshed when the helper or its key changed.
    status_listener: OnceLock<Arc<dyn HostReconciler>>,
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
        let link = Arc::new(WsAgentLink::new(
            registry.clone(),
            deps.clock.clone(),
            deps.hosts.clone(),
        ));
        Ok(Self {
            inner: Arc::new(Inner {
                settings,
                deps,
                tls,
                acceptor: TlsAcceptor::from(Arc::new(config)),
                registry,
                link,
                network_degraded: AtomicBool::new(false),
                reconciled: std::sync::Mutex::new(std::collections::BTreeSet::new()),
                shutdown: CancellationToken::new(),
                overrides: OnceLock::new(),
                reconciler: OnceLock::new(),
                status_listener: OnceLock::new(),
                package_reports: OnceLock::new(),
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

    /// The raw link, reachable only inside this module and by the runtime that composes the
    /// engine's capabilities from it.
    ///
    /// The runtime turns it into the engine's narrow capability objects and the capability
    /// registry's view (ADR 0023); a transport reaches agents through
    /// [`AgentAdmin`] (EXEC-4).
    pub(crate) fn link(&self) -> Arc<WsAgentLink> {
        self.inner.link.clone()
    }

    /// The link this server owns, for the test suites that drive an agent's wire directly.
    ///
    /// Only compiled with the `test-support` feature, which is not a default feature and is
    /// turned on by nothing but this crate's and the binary's test builds: a normal or
    /// release build has no method that hands out the link. The suites that use it live
    /// under `tests/`, which every architecture scan exempts.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn link_for_tests(&self) -> Arc<WsAgentLink> {
        self.link()
    }

    /// The connection state of `host`'s agent, for a transport reading it through
    /// [`AgentAdmin`].
    #[must_use]
    pub fn host_presence(&self, host: HostId) -> domovoi_core::ports::Presence {
        self.inner.link.presence(host)
    }

    /// The skills `host`'s agent announced on its last handshake.
    #[must_use]
    pub fn host_skills(&self, host: HostId) -> Vec<domovoi_core::SkillSummary> {
        self.inner.link.skills(host)
    }

    /// Keeps each host's announced package skills in `store`, so they can be shown while
    /// the agent is away. Call once, before the server serves.
    pub fn keep_package_reports(&self, store: Arc<dyn domovoi_core::ports::SettingsRepo>) {
        if self.inner.package_reports.set(store).is_err() {
            tracing::warn!("the agent server already stores package reports; keeping the first");
        }
    }

    /// `host`'s package skills: what its live agent announced, else what it announced when
    /// it was last connected.
    pub async fn host_package_report(&self, host: HostId) -> PackageReport {
        if let Some(skills) = self.inner.link.package_skills(host) {
            return PackageReport { skills, live: true };
        }
        let skills = match self.inner.package_reports.get() {
            Some(store) => package_report::load(&**store, host).await,
            None => Vec::new(),
        };
        PackageReport {
            skills,
            live: false,
        }
    }

    /// Notes the agent's answer to an enable or disable at once, in case it goes away
    /// before it announces its new list.
    pub async fn note_package_enabled(
        &self,
        host: HostId,
        package: &str,
        version: &str,
        enabled: bool,
    ) {
        let Some(store) = self.inner.package_reports.get() else {
            return;
        };
        let mut skills = package_report::load(&**store, host).await;
        for skill in skills
            .iter_mut()
            .filter(|s| s.package == package && s.version == version)
        {
            skill.enabled = enabled;
            skill.reason = (!enabled).then(|| "disabled by a person".to_owned());
        }
        package_report::save(&**store, host, &skills).await;
    }

    /// The agent's recent log lines, as the log page asks for them.
    pub async fn host_logs(
        &self,
        host: HostId,
        lines: u16,
        min_level: String,
    ) -> Result<Vec<domovoi_core::ports::LogLine>, domovoi_core::ports::LinkError> {
        self.inner.link.logs(host, lines, min_level).await
    }

    /// Gives the server the action service, so every authenticated agent's dispatched
    /// runs are reconciled against its journal. The runtime calls this once, after both
    /// services exist; the first reconciler wins, because a second would duplicate work.
    /// An agent that authenticated before the action service existed has runs to settle
    /// too, so each host that is live now is reconciled straight away, bound to that
    /// session's own cancellation token.
    pub fn set_reconciler(&self, reconciler: Arc<dyn HostReconciler>) {
        if self.inner.reconciler.set(reconciler).is_err() {
            tracing::warn!("the agent server already has a reconciler; keeping the first");
            return;
        }
        for host in self.inner.registry.live_hosts() {
            if let Some(session) = self.inner.registry.live_cancel(host) {
                session::reconcile(&self.inner, host, &session);
            }
        }
    }

    /// Gives the server a listener told whenever a host's agent reports new runtime status.
    /// The first listener wins.
    pub fn set_status_listener(&self, listener: Arc<dyn HostReconciler>) {
        if self.inner.status_listener.set(listener).is_err() {
            tracing::warn!("the agent server already has a status listener; keeping the first");
        }
    }

    /// Picks up a `hub.network_degraded` incident left open by an earlier run. What was
    /// raised is kept in memory, so without this nothing would ever resolve it. Call once,
    /// before presence checks start.
    pub async fn restore_network_degraded(
        &self,
        incidents: &dyn IncidentRepo,
    ) -> Result<(), StoreError> {
        let filter = IncidentFilter {
            host: Some(self.inner.deps.hub_host),
            active: Some(true),
            ..IncidentFilter::default()
        };
        let mut page = Page::first(Page::MAX_LIMIT);
        loop {
            let got = incidents.list(&filter, &page).await?;
            if got
                .items
                .iter()
                .any(|incident| incident.check().as_str() == presence::NETWORK_DEGRADED)
            {
                self.inner.network_degraded.store(true, Ordering::SeqCst);
                return Ok(());
            }
            match got.next_cursor {
                Some(cursor) => page.cursor = Some(cursor),
                None => return Ok(()),
            }
        }
    }

    /// What the host's agent said about its machine on its last handshake; `None` while
    /// it is offline.
    #[must_use]
    pub fn host_facts(&self, host: HostId) -> Option<HostFacts> {
        self.inner.registry.facts(host)
    }

    /// The checks the host's agent reported when it connected; empty while it is offline.
    #[must_use]
    pub fn host_checks(&self, host: HostId) -> Vec<CheckInfo> {
        self.inner.registry.checks(host).unwrap_or_default()
    }

    /// Whether the host's live agent reports that it runs in a container; `false` while it
    /// is offline.
    #[must_use]
    pub fn host_in_container(&self, host: HostId) -> bool {
        self.inner
            .registry
            .host_view(host)
            .is_some_and(|view| view.words.in_container)
    }

    /// The last `agent_status` the host's live session reported; `None` while it is offline
    /// or before the agent sent one in this session.
    #[must_use]
    pub fn agent_status(&self, host: HostId) -> Option<domovoi_proto::messages::AgentStatus> {
        self.inner.registry.agent_status(host)
    }

    /// How the host's agent stands against this hub's protocol, from its stored report.
    pub async fn protocol_state(&self, host: HostId) -> Result<ProtocolState, StoreError> {
        let Some(agent) = self.inner.deps.hosts.live_agent(host).await? else {
            return Ok(ProtocolState::Unknown);
        };
        let report = self.inner.deps.hosts.agent_report(agent.agent).await?;
        Ok(protocol_state(report.as_ref(), PROTOCOL_VERSION))
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
        accept_log::serve(self, listener).await
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
        let now = inner.deps.clock.now();
        match inner.deps.hosts.live_agent(host).await? {
            Some(agent) => {
                inner.deps.hosts.revoke_agent(agent.agent, now).await?;
                // Retirement, not a plain close: a handshake for this identity that is still
                // in flight must not register itself after the revocation.
                inner.registry.retire(agent.agent, now);
                if let Some(store) = inner.package_reports.get() {
                    package_report::forget(&**store, host).await;
                }
                tracing::info!(%host, agent = %agent.agent, "agent revoked");
                Ok(true)
            }
            None => {
                inner.registry.close(host, now);
                Ok(false)
            }
        }
    }

    /// Retires `agent` in the live registry when its identity is revoked outside this module,
    /// as the local agent path does through the store. Closes its session and refuses any
    /// registration that began before the revocation.
    pub(crate) fn retire_agent(&self, agent: domovoi_core::AgentId) {
        self.inner
            .registry
            .retire(agent, self.inner.deps.clock.now());
    }

    /// Whether the live registry has retired `agent`. Test support for the local enrollment
    /// path; it is not part of the production API.
    #[cfg(test)]
    pub(crate) fn is_retired(&self, agent: domovoi_core::AgentId) -> bool {
        self.inner.registry.is_retired(agent)
    }

    /// Runs one presence check now. [`serve`](Self::serve) does this every heartbeat.
    pub async fn check_presence(&self) {
        presence::check(&self.inner).await;
    }

    /// Starts watching every host that is not External and has an enrolled agent, so
    /// one that never reconnects after a hub restart is still noticed. The hub's own
    /// machine counts too: its local agent can be enrolled in the store while the
    /// process starts without one.
    pub async fn watch_known_hosts(&self) -> Result<(), AgentServerError> {
        let inner = &self.inner;
        let now = inner.deps.clock.now();
        for host in inner.deps.hosts.list_hosts().await? {
            if host.kind != HostKind::External
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
