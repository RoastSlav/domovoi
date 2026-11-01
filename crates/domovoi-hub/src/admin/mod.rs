//! Administration without a running hub: the command line's `token`, `agent`,
//! `set-password`, `digest now` and `notify test`.
//!
//! These open the store directly while holding the data directory's [`HubLock`], so
//! they refuse to run next to a hub, which owns the database while it runs. With a hub
//! running, the same actions are in the web UI.

mod notify;

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{
    Clock, HostRepo, IngestError, OutboundMessage, SignalSink, StoreError, UserRepo,
};
use domovoi_core::{HostKind, HostName, Signal, User, UserId};
use domovoi_store::Store;
use jiff::{SignedDuration, Timestamp};

pub use notify::{NotifyOutcome, notify_test};

use crate::agent_server::{
    AgentServer, AgentServerDeps, AgentServerError, EnrollmentInvite, RealNetProbes, ServerSettings,
};
use crate::auth::password;
use crate::digest::{DigestDeps, DigestError};
use crate::runtime::{AdapterOverrides, FanOut, HubLock, LockError, RuntimeError, adapters};

/// Why an admin command failed.
#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    /// A hub is running on this data directory; use the web UI instead.
    #[error("{0}; stop it or use the web UI instead")]
    HubRunning(LockError),
    /// The data directory could not be locked.
    #[error(transparent)]
    Lock(LockError),
    /// The store or adapters could not be opened.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// A store read or write failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The agent server's TLS identity or token handling failed.
    #[error(transparent)]
    Agent(#[from] AgentServerError),
    /// The input was not acceptable.
    #[error("{0}")]
    Invalid(String),
    /// Nothing to send through, or sending failed.
    #[error("{0}")]
    Delivery(String),
}

impl From<LockError> for AdminError {
    fn from(err: LockError) -> Self {
        match err {
            held @ LockError::Held(_) => Self::HubRunning(held),
            other => Self::Lock(other),
        }
    }
}

/// One row of `domovoi hub agent list`. Without a running hub nothing is connected, so
/// this is enrollment state, not live presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    /// Host name.
    pub host: String,
    /// `local`, `remote` or `external`.
    pub kind: &'static str,
    /// The live agent's version, when one is enrolled.
    pub version: Option<String>,
    /// When the live agent enrolled.
    pub enrolled_at: Option<Timestamp>,
}

/// The data directory, opened for one admin command.
pub struct OfflineAdmin {
    _lock: HubLock,
    config: Arc<HubConfig>,
    clock: Arc<dyn Clock>,
    store: Arc<Store>,
    server: AgentServer,
}

/// Agent signals have nowhere to go while no hub runs; the server here never serves.
struct Discard;

#[async_trait]
impl SignalSink for Discard {
    async fn submit(&self, _: Vec<Signal>) -> Result<(), IngestError> {
        Ok(())
    }
}

impl OfflineAdmin {
    /// Locks the data directory and opens the store and the agent server's identity.
    /// [`AdminError::HubRunning`] when a hub holds the directory.
    pub async fn open(config: HubConfig, clock: Arc<dyn Clock>) -> Result<Self, AdminError> {
        let lock = HubLock::acquire(&config.hub.data_dir)?;
        let store = Arc::new(adapters::open_store(&config, &*clock).await?);
        adapters::ensure_hub_host(&store, &config, clock.now()).await?;
        let hub_host = adapters::hub_host(&store).await?;
        let server = AgentServer::new(
            ServerSettings::from_config(&config)?,
            AgentServerDeps {
                clock: clock.clone(),
                hosts: store.clone(),
                facts: store.clone(),
                sink: Arc::new(Discard),
                probes: Arc::new(RealNetProbes::new(clock.clone())),
                hub_host,
            },
        )?;
        Ok(Self {
            _lock: lock,
            config: Arc::new(config),
            clock,
            store,
            server,
        })
    }

    /// A single-use enrollment invite for `host`, valid for `ttl`.
    pub async fn create_token(
        &self,
        host: &str,
        ttl: SignedDuration,
    ) -> Result<EnrollmentInvite, AdminError> {
        if ttl <= SignedDuration::ZERO {
            return Err(AdminError::Invalid("the TTL must be positive".to_owned()));
        }
        let name = host_name(host)?;
        Ok(self.server.create_token_valid_for(&name, ttl).await?)
    }

    /// Every host and its enrolled agent, sorted by name.
    pub async fn agents(&self) -> Result<Vec<AgentRow>, AdminError> {
        let mut rows = Vec::new();
        for host in self.store.list_hosts().await? {
            let agent = self.store.live_agent(host.id).await?;
            rows.push(AgentRow {
                host: host.name.to_string(),
                kind: match host.kind {
                    HostKind::Local => "local",
                    HostKind::Remote => "remote",
                    HostKind::External => "external",
                },
                version: agent.as_ref().map(|a| a.version.clone()),
                enrolled_at: agent.map(|a| a.enrolled_at),
            });
        }
        Ok(rows)
    }

    /// Revokes the agent of `host`. False when it had none.
    pub async fn revoke(&self, host: &str) -> Result<bool, AdminError> {
        let name = host_name(host)?;
        let Some(host) = self.store.get_host_by_name(&name).await? else {
            return Err(AdminError::Invalid(format!("no host named {name}")));
        };
        Ok(self.server.revoke_agent(host.id).await?)
    }

    /// Sets the admin password and returns the username. With no account yet, creates
    /// the admin account as `username` (default `admin`); otherwise updates the account
    /// named `username`, or the first account when `username` is `None`.
    pub async fn set_password(
        &self,
        username: Option<&str>,
        password: &str,
    ) -> Result<String, AdminError> {
        if password.chars().count() < password::MIN_PASSWORD_CHARS {
            return Err(AdminError::Invalid(format!(
                "the password must be at least {} characters",
                password::MIN_PASSWORD_CHARS
            )));
        }
        let hash = password::hash(password)
            .map_err(|err| AdminError::Invalid(format!("could not hash the password: {err}")))?;
        let existing = match username {
            Some(name) => self.store.get_by_username(name).await?,
            None => self.store.first_user().await?,
        };
        if let Some(user) = existing {
            self.store.set_password_hash(user.id, &hash).await?;
            return Ok(user.username);
        }
        if self.store.count().await? > 0 {
            return Err(AdminError::Invalid(format!(
                "no account named {}",
                username.unwrap_or_default()
            )));
        }
        let user = User {
            id: UserId::new(),
            username: username.unwrap_or("admin").to_owned(),
            password_hash: hash,
            created_at: self.clock.now(),
        };
        self.store.create(&user).await?;
        Ok(user.username)
    }

    /// Builds today's digest and sends it to every notifier. Returns the date it covers.
    pub async fn send_digest(&self) -> Result<String, AdminError> {
        let public_url = crate::runtime::support_public_url(&self.config)?;
        let (_, notifiers) = adapters::build_chat(
            &self.config,
            &self.clock,
            &public_url,
            &AdapterOverrides::default(),
        )?;
        let notifier = FanOut::over(&notifiers).ok_or_else(|| {
            AdminError::Delivery("no notifier is configured ([discord] webhook_url)".to_owned())
        })?;
        let digest = crate::digest::build(&DigestDeps {
            config: self.config.clone(),
            clock: self.clock.clone(),
            hosts: self.store.clone(),
            facts: self.store.clone(),
            incidents: self.store.clone(),
            advisories: self.store.clone(),
        })
        .await
        .map_err(|err| match err {
            DigestError::Store(err) => AdminError::Store(err),
            DigestError::DayStart(err) => AdminError::Invalid(err),
        })?;
        notifier
            .send(&OutboundMessage {
                title: format!("Daily digest for {}", digest.date),
                markdown: digest.markdown,
                severity: None,
            })
            .await
            .map_err(|err| AdminError::Delivery(format!("sending failed: {err}")))?;
        Ok(digest.date)
    }

    /// Closes the store cleanly, then releases the lock.
    pub async fn close(self) {
        self.store.close().await;
    }
}

fn host_name(input: &str) -> Result<HostName, AdminError> {
    HostName::parse(input).map_err(|err| AdminError::Invalid(format!("invalid host name: {err}")))
}

#[cfg(test)]
mod tests;
