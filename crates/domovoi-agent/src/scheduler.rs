//! The check scheduler: runs checks on their cadence and turns observations into signals.
//!
//! - Each enabled check runs at its `[checks.<key>] interval`, else its default cadence;
//!   at most [`MAX_CONCURRENT`] run at once, and every wait goes through the `Clock`.
//! - A firing observation becomes a `firing` signal on every run; the hub folds repeats.
//! - A target that was firing and is now fine, or no longer reported, gets one `resolved`
//!   signal. A check that errors changes nothing, so its incidents stay as they were.
//! - Each run's signals are one spool batch; check state is saved after each run.
//! - Digest facts (latest per key) go to the hub once an hour.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_checks::commands::Commands;
use domovoi_checks::framework::{
    Check, CheckContext, CheckReport, CheckSettings, number_fact, run_bounded,
};
use domovoi_core::ports::{Clock, HostFs, ProcessExecutor};
use domovoi_core::{CheckKey, DigestFact, HostId, Signal, SourceKind, TargetKey};
use domovoi_proto::Message;
use domovoi_proto::messages::DigestFacts;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use jiff::{SignedDuration, Timestamp};
use tokio_util::sync::CancellationToken;

use crate::checks::CheckSet;
use crate::link::LinkHandle;
use crate::spool::SharedSpool;
use crate::store::{CheckStore, NO_TARGET};

/// Most checks running at once.
pub const MAX_CONCURRENT: usize = 4;
/// How often digest facts are published.
pub const FACTS_EVERY: SignedDuration = SignedDuration::from_hours(1);

/// What checks run with.
#[derive(Clone)]
pub struct CheckEnv {
    /// The host the checks report on.
    pub host: HostId,
    /// The host's files.
    pub fs: Arc<dyn HostFs>,
    /// Runs the commands checks use.
    pub exec: Arc<dyn ProcessExecutor>,
    /// The resolved command templates.
    pub commands: Arc<Commands>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Per-check settings; a check without an entry uses the defaults.
    pub settings: Arc<BTreeMap<CheckKey, CheckSettings>>,
}

impl CheckEnv {
    /// The settings for `key`.
    #[must_use]
    pub fn settings_for(&self, key: &CheckKey) -> CheckSettings {
        self.settings.get(key).cloned().unwrap_or_default()
    }

    /// Runs `check` once on its stored state and stores the state it leaves.
    pub async fn run(
        &self,
        check: &dyn Check,
        store: &CheckStore,
    ) -> Result<CheckReport, domovoi_checks::framework::CheckError> {
        let key = check.key();
        let settings = self.settings_for(&key);
        let mut state = store.state(&key);
        let mut ctx = CheckContext {
            host: self.host,
            fs: &*self.fs,
            exec: &*self.exec,
            commands: &self.commands,
            clock: &*self.clock,
            settings: &settings,
            state: &mut state,
        };
        let report = run_bounded(check, &mut ctx).await;
        store.put_state(&key, state);
        report
    }
}

fn target_name(target: Option<&TargetKey>) -> String {
    target.map_or_else(|| NO_TARGET.to_owned(), |t| t.as_str().to_owned())
}

/// The signals for one run of `check`, given the targets that were firing before, and the
/// targets firing now.
#[must_use]
pub fn signals_for(
    host: HostId,
    check: &dyn Check,
    report: &CheckReport,
    previous: &BTreeSet<String>,
    now: Timestamp,
) -> (Vec<Signal>, BTreeSet<String>) {
    let key = check.key();
    let mut signals = Vec::new();
    let mut firing = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for obs in &report.observations {
        let name = target_name(obs.target.as_ref());
        seen.insert(name.clone());
        let class = obs.class.unwrap_or(check.class());
        if obs.firing {
            firing.insert(name);
            let signal = Signal::firing(
                SourceKind::Agent,
                host,
                key.clone(),
                obs.target.clone(),
                obs.severity,
                &obs.summary,
                now,
            );
            signals.push(signal.with_class(class).with_evidence(obs.evidence.clone()));
        } else if previous.contains(&name) {
            let signal = Signal::resolved(
                SourceKind::Agent,
                host,
                key.clone(),
                obs.target.clone(),
                &obs.summary,
                now,
            );
            signals.push(signal.with_class(class).with_evidence(obs.evidence.clone()));
        }
    }
    for gone in previous.difference(&seen) {
        let target = (gone != NO_TARGET)
            .then(|| TargetKey::parse(gone).ok())
            .flatten();
        let signal = Signal::resolved(
            SourceKind::Agent,
            host,
            key.clone(),
            target,
            "no longer reported",
            now,
        );
        signals.push(signal.with_class(check.class()));
    }
    (signals, firing)
}

async fn run_one<'a>(
    env: &'a CheckEnv,
    store: &'a CheckStore,
    check: &'a dyn Check,
) -> (
    &'a dyn Check,
    Result<CheckReport, domovoi_checks::framework::CheckError>,
) {
    (check, env.run(check, store).await)
}

/// Runs checks until shutdown.
pub struct Scheduler {
    checks: Arc<dyn CheckSet>,
    env: CheckEnv,
    store: Arc<CheckStore>,
    spool: Arc<SharedSpool>,
    link: LinkHandle,
    facts: BTreeMap<String, DigestFact>,
}

impl Scheduler {
    /// A scheduler for `checks`, spooling signals and sending facts through `link`.
    #[must_use]
    pub fn new(
        checks: Arc<dyn CheckSet>,
        env: CheckEnv,
        store: Arc<CheckStore>,
        spool: Arc<SharedSpool>,
        link: LinkHandle,
    ) -> Self {
        Self {
            checks,
            env,
            store,
            spool,
            link,
            facts: BTreeMap::new(),
        }
    }

    fn cadence(&self, check: &dyn Check) -> SignedDuration {
        let cadence = self
            .env
            .settings_for(&check.key())
            .cadence
            .unwrap_or_else(|| check.default_cadence());
        cadence.max(SignedDuration::from_secs(1))
    }

    /// Runs every enabled check at once, then each on its cadence, until `shutdown`.
    pub async fn run(mut self, shutdown: CancellationToken) {
        let clock = self.env.clock.clone();
        let start = clock.now();
        let mut due: BTreeMap<CheckKey, Timestamp> = self
            .checks
            .all()
            .into_iter()
            .filter(|c| self.env.settings_for(&c.key()).enabled)
            .map(|c| (c.key(), start))
            .collect();
        let mut facts_at = start.saturating_add(FACTS_EVERY).unwrap_or(start);
        loop {
            let now = clock.now();
            let ready: Vec<CheckKey> = due
                .iter()
                .filter(|(_, at)| **at <= now)
                .map(|(key, _)| key.clone())
                .collect();
            if !ready.is_empty() {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = self.run_due(&ready) => {}
                }
                let after = clock.now();
                for key in &ready {
                    if let Some(check) = self.checks.get(key) {
                        let next = after.saturating_add(self.cadence(check)).unwrap_or(after);
                        due.insert(key.clone(), next);
                    }
                }
            }
            if clock.now() >= facts_at {
                self.publish_facts().await;
                facts_at = facts_at.saturating_add(FACTS_EVERY).unwrap_or(facts_at);
            }
            let wake = due
                .values()
                .copied()
                .min()
                .unwrap_or(facts_at)
                .min(facts_at);
            if wake > clock.now() {
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = clock.sleep_until(wake) => {}
                }
            }
        }
    }

    async fn run_due(&mut self, keys: &[CheckKey]) {
        let checks = self.checks.clone();
        let mut queue: Vec<&dyn Check> = Vec::new();
        for key in keys {
            if let Some(check) = checks.get(key) {
                queue.push(check);
            }
        }
        let mut queue = queue.into_iter();
        let mut running = FuturesUnordered::new();
        let mut reports = Vec::new();
        loop {
            while running.len() < MAX_CONCURRENT {
                let Some(check) = queue.next() else { break };
                running.push(run_one(&self.env, &self.store, check));
            }
            match running.next().await {
                Some(done) => reports.push(done),
                None => break,
            }
        }
        drop(running);
        let now = self.env.clock.now();
        let mut batch = Vec::new();
        for (check, report) in reports {
            let key = check.key();
            let report = match report {
                Ok(report) => report,
                Err(err) => {
                    tracing::warn!(check = %key, %err, "check failed");
                    continue;
                }
            };
            let previous = self.store.firing(&key);
            let (signals, firing) = signals_for(self.env.host, check, &report, &previous, now);
            self.store.set_firing(&key, firing);
            batch.extend(signals);
            for fact in report.facts {
                self.facts.insert(fact.key.clone(), fact);
            }
        }
        if !batch.is_empty()
            && let Err(err) = self.spool.push(batch).await
        {
            tracing::error!(%err, "could not spool signals");
        }
        if let Err(err) = self.store.save() {
            tracing::error!(%err, "could not save check state");
        }
    }

    async fn publish_facts(&mut self) {
        let mut facts: Vec<DigestFact> = self.facts.values().cloned().collect();
        let now = self.env.clock.now();
        let dropped = self.spool.lock().await.dropped();
        if dropped > 0 {
            facts.push(number_fact(
                self.env.host,
                "spool.dropped",
                "Signals dropped while the hub was unreachable",
                dropped as f64,
                "",
                now,
            ));
        }
        if facts.is_empty() {
            return;
        }
        if !self
            .link
            .send(Message::DigestFacts(DigestFacts { facts }), None)
        {
            tracing::debug!("digest facts were not sent; the outbox is full");
        }
    }
}
