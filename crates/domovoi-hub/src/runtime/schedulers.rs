//! Clock-driven notification, maintenance and heartbeat jobs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use domovoi_config::HubConfig;
use domovoi_core::ports::{AdvisoryRepo, BaselineRepo, Clock, HostRepo, SettingsRepo};
use domovoi_core::{AdvisoryKind, DigestFact};
use domovoi_engine::ActionHandle;
use domovoi_engine::advisories::{AdvisoryInputs, AgentVersion, HeartbeatStatus, evaluate};
use domovoi_engine::digest::DigestService;
use domovoi_store::{RetentionPolicy, Store};
use jiff::civil::Time;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::digest::{self, DigestDeps};

use super::Dispatcher;

const DIGEST_LAST_SENT: &str = "digest_last_sent_at";

/// How long to wait before trying again after every notifier refused the digest.
const DIGEST_RETRY: SignedDuration = SignedDuration::from_mins(15);

/// How long one heartbeat ping may take.
const HEARTBEAT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// All runtime schedulers. Each returned task is independently cancellable.
#[derive(Clone)]
pub(crate) struct Schedulers {
    config: Arc<HubConfig>,
    clock: Arc<dyn Clock>,
    store: Arc<Store>,
    actions: ActionHandle,
    dispatcher: Dispatcher,
    digest: Arc<DigestDeps>,
    heartbeat: Arc<Mutex<HeartbeatStatus>>,
    /// The client heartbeat pings use; `None` when TLS could not be set up.
    http: Option<reqwest::Client>,
    /// After a failed digest, when to try again.
    digest_retry_at: Arc<Mutex<Option<Timestamp>>>,
}

impl Schedulers {
    pub(crate) fn new(
        config: Arc<HubConfig>,
        clock: Arc<dyn Clock>,
        store: Arc<Store>,
        actions: ActionHandle,
        dispatcher: Dispatcher,
        digest: DigestDeps,
    ) -> Self {
        let heartbeat = HeartbeatStatus {
            configured: config.heartbeat.is_some(),
            failing_since: None,
        };
        let http = reqwest::Client::builder()
            .timeout(HEARTBEAT_TIMEOUT)
            .user_agent(concat!("domovoi/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| tracing::error!(err = %err.without_url(), "no HTTP client for heartbeat pings"))
            .ok();
        Self {
            config,
            clock,
            store,
            actions,
            dispatcher,
            digest: Arc::new(digest),
            heartbeat: Arc::new(Mutex::new(heartbeat)),
            http,
            digest_retry_at: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn spawn(self, shutdown: CancellationToken) -> Vec<JoinHandle<()>> {
        let mut tasks = vec![
            spawn_minute(self.clone(), shutdown.clone()),
            spawn_advisories(self.clone(), shutdown.clone()),
            spawn_daily(
                self.clone(),
                shutdown.clone(),
                Time::new(3, 30, 0, 0).unwrap_or(Time::midnight()),
                DailyJob::Retention,
            ),
            spawn_daily(
                self.clone(),
                shutdown.clone(),
                Time::new(4, 0, 0, 0).unwrap_or(Time::midnight()),
                DailyJob::Backup,
            ),
        ];
        if self.config.heartbeat.is_some() {
            tasks.push(spawn_heartbeat(self, shutdown));
        }
        tasks
    }

    fn digest_service(&self) -> DigestService {
        DigestService {
            enabled: self.config.notify.digest.enabled,
            time: self.config.notify.digest.time.time(),
            tz: self.config.hub.timezone.zone().clone(),
        }
    }

    pub(crate) async fn minute(&self) {
        self.dispatcher.tick().await;
        if let Err(err) = self.actions.expire_due().await {
            tracing::warn!(%err, "could not expire action approvals");
        }
        self.digest_due().await;
    }

    async fn digest_due(&self) {
        // Digests go out through notifiers only; without one there is nothing to send.
        if !self.dispatcher.has_notifiers() {
            return;
        }
        let now = self.clock.now();
        let retry_at = *self
            .digest_retry_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if retry_at.is_some_and(|at| now < at) {
            return;
        }
        let last = SettingsRepo::get(&*self.store, DIGEST_LAST_SENT)
            .await
            .ok()
            .flatten()
            .and_then(|value| {
                value
                    .as_str()
                    .and_then(|text| text.parse::<Timestamp>().ok())
            });
        if !self.digest_service().due(now, last) {
            return;
        }
        match digest::build(&self.digest).await {
            Ok(rendered) => {
                let sent = self
                    .dispatcher
                    .digest(rendered.date, rendered.markdown)
                    .await;
                *self
                    .digest_retry_at
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    (!sent).then(|| now.checked_add(DIGEST_RETRY).unwrap_or(now));
                if !sent {
                    tracing::warn!("every notifier refused the digest; trying again later");
                    return;
                }
                if let Err(err) = SettingsRepo::set(
                    &*self.store,
                    DIGEST_LAST_SENT,
                    &serde_json::json!(now.to_string()),
                )
                .await
                {
                    tracing::warn!(%err, "could not persist the digest delivery date");
                }
            }
            Err(err) => tracing::warn!(%err, "could not build the daily digest"),
        }
    }

    async fn advisories(&self) {
        let now = self.clock.now();
        let open = match self.store.list_open().await {
            Ok(open) => open,
            Err(err) => {
                tracing::warn!(%err, "could not load open advisories");
                return;
            }
        };
        let hosts = self.store.list_hosts().await.unwrap_or_default();
        let since = now
            .checked_sub(SignedDuration::from_hours(24 * 200))
            .unwrap_or(Timestamp::MIN);
        let mut latest = BTreeMap::new();
        let mut agents = Vec::new();
        let mut expired = Vec::new();
        for host in &hosts {
            if let Ok(facts) = self.store.facts_for_advisories(host.id, since, now).await {
                for fact in facts {
                    latest.insert((fact.host, fact.key.clone()), fact);
                }
            }
            if let Ok(Some(agent)) = self.store.live_agent(host.id).await {
                agents.push(AgentVersion {
                    host: host.id,
                    version: agent.version,
                });
            }
            if let Ok(entries) = self.store.list(host.id).await {
                expired.extend(entries.into_iter().filter(|entry| {
                    now.duration_since(entry.last_seen_at) > SignedDuration::from_hours(24 * 7)
                }));
            }
        }
        let facts: Vec<DigestFact> = latest.into_values().collect();
        let heartbeat = *self
            .heartbeat
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let inputs = AdvisoryInputs {
            heartbeat,
            facts: &facts,
            exposed_secrets: &self.config.secrets.exposed,
            agents: &agents,
            hub_version: env!("CARGO_PKG_VERSION"),
            expired_baselines: &expired,
            triage_degraded: None,
        };
        let mut changes = evaluate(&inputs, &open, now);
        changes.resolve.retain(|id| {
            open.iter()
                .find(|advisory| advisory.id == *id)
                .is_none_or(|advisory| {
                    !matches!(
                        advisory.kind,
                        AdvisoryKind::TriageDegraded | AdvisoryKind::DeliveryFailed
                    )
                })
        });
        for advisory in changes.upserts {
            if let Err(err) = AdvisoryRepo::upsert(&*self.store, &advisory).await {
                tracing::warn!(%err, id = %advisory.id, "could not save an advisory");
            }
        }
        for id in changes.resolve {
            if let Err(err) = self.store.resolve(id, now).await {
                tracing::warn!(%err, %id, "could not resolve an advisory");
            }
        }
    }

    async fn heartbeat(&self) {
        let Some(config) = &self.config.heartbeat else {
            return;
        };
        let now = self.clock.now();
        let timeout = now
            .checked_add(SignedDuration::from_secs(10))
            .unwrap_or(now);
        let result = match &self.http {
            None => Err("no HTTP client".to_owned()),
            Some(http) => {
                let request = http
                    .get(secrecy::ExposeSecret::expose_secret(config.url.value()))
                    .send();
                tokio::select! {
                    response = request => response
                        .map(|response| response.status().is_success())
                        .map_err(|err| err.without_url().to_string()),
                    () = self.clock.sleep_until(timeout) => Err("heartbeat timed out".to_owned()),
                }
            }
        };
        {
            let mut status = self
                .heartbeat
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match result {
                Ok(true) => status.failing_since = None,
                Ok(false) => {
                    status.failing_since.get_or_insert(now);
                }
                Err(err) => {
                    status.failing_since.get_or_insert(now);
                    tracing::warn!(%err, "heartbeat ping failed");
                }
            }
        }
        self.advisories().await;
    }
}

fn spawn_minute(runtime: Schedulers, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            runtime.minute().await;
            let now = runtime.clock.now();
            let next = now.checked_add(SignedDuration::from_mins(1)).unwrap_or(now);
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = runtime.clock.sleep_until(next) => {}
            }
        }
    })
}

fn spawn_advisories(runtime: Schedulers, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            runtime.advisories().await;
            let now = runtime.clock.now();
            let next = now.checked_add(SignedDuration::from_mins(5)).unwrap_or(now);
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = runtime.clock.sleep_until(next) => {}
            }
        }
    })
}

#[derive(Clone, Copy)]
enum DailyJob {
    Retention,
    Backup,
}

fn spawn_daily(
    runtime: Schedulers,
    shutdown: CancellationToken,
    time: Time,
    job: DailyJob,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let next = next_local(
                runtime.clock.now(),
                time,
                runtime.config.hub.timezone.zone().clone(),
            );
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = runtime.clock.sleep_until(next) => {}
            }
            let now = runtime.clock.now();
            match job {
                DailyJob::Retention => {
                    if let Err(err) = runtime.store.purge(now, &RetentionPolicy::default()).await {
                        tracing::warn!(%err, "retention purge failed");
                    }
                }
                DailyJob::Backup => {
                    let dir = runtime
                        .config
                        .backup
                        .dir
                        .clone()
                        .unwrap_or_else(|| runtime.config.hub.data_dir.join("backups"));
                    if let Err(err) = runtime.store.backup_to(&dir, now).await {
                        tracing::warn!(%err, "nightly backup failed");
                    }
                }
            }
        }
    })
}

fn spawn_heartbeat(runtime: Schedulers, shutdown: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        let interval = runtime
            .config
            .heartbeat
            .as_ref()
            .map(|heartbeat| heartbeat.interval.as_signed())
            .unwrap_or(SignedDuration::from_mins(1));
        loop {
            runtime.heartbeat().await;
            let now = runtime.clock.now();
            let next = now.checked_add(interval).unwrap_or(now);
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = runtime.clock.sleep_until(next) => {}
            }
        }
    })
}

fn next_local(now: Timestamp, time: Time, tz: TimeZone) -> Timestamp {
    let zoned = now.to_zoned(tz.clone());
    let today = zoned.date();
    let at = today.to_zoned(tz.clone()).ok().and_then(|midnight| {
        midnight
            .with()
            .time(time)
            .build()
            .ok()
            .map(|zoned| zoned.timestamp())
    });
    match at {
        Some(target) if target > now => target,
        _ => {
            let tomorrow = today.tomorrow().unwrap_or(today);
            tomorrow
                .to_zoned(tz)
                .ok()
                .and_then(|midnight| {
                    midnight
                        .with()
                        .time(time)
                        .build()
                        .ok()
                        .map(|zoned| zoned.timestamp())
                })
                .unwrap_or_else(|| {
                    now.checked_add(SignedDuration::from_hours(24))
                        .unwrap_or(now)
                })
        }
    }
}

trait AdvisoryFacts {
    async fn facts_for_advisories(
        &self,
        host: domovoi_core::HostId,
        from: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<DigestFact>, domovoi_core::ports::StoreError>;
}

impl AdvisoryFacts for Store {
    async fn facts_for_advisories(
        &self,
        host: domovoi_core::HostId,
        from: Timestamp,
        until: Timestamp,
    ) -> Result<Vec<DigestFact>, domovoi_core::ports::StoreError> {
        domovoi_core::ports::FactRepo::list(self, host, from, until).await
    }
}
