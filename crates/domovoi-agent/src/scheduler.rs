//! The check scheduler: runs checks on their cadence and turns observations into signals.
//!
//! - Each enabled check runs at its `[checks.<key>] interval`, else its default cadence;
//!   at most [`MAX_CONCURRENT`] run at once, and every wait goes through the `Clock`.
//! - A firing observation becomes a `firing` signal on every run; the hub folds repeats.
//! - A target that was firing and is now fine, or no longer reported, gets one `resolved`
//!   signal, except for an event check (`Check::resolves_itself` is false), whose incidents
//!   only a person closes. A check that errors changes nothing, so its incidents stay as
//!   they were.
//! - A check that cannot read the journal it watches raises its own warning, target
//!   [`BLIND_TARGET`], and resolves it at its next successful read.
//! - Each run's signals are one spool batch, and check state is saved only once that batch
//!   is on disk.
//! - Digest facts (latest per key) go to the hub once an hour.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use domovoi_checks::commands::Commands;
use domovoi_checks::framework::{
    Check, CheckContext, CheckError, CheckReport, CheckSettings, CheckState, number_fact,
    run_bounded,
};
use domovoi_core::ports::{Clock, HostFs, PrivilegedReads, ProcessExecutor};
use domovoi_core::{CheckKey, DigestFact, HostId, Signal, SourceKind, TargetKey};
use domovoi_proto::Message;
use domovoi_proto::messages::DigestFacts;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use jiff::{SignedDuration, Timestamp};
use tokio_util::sync::CancellationToken;

mod blind;

pub use blind::{BLIND_TARGET, blind_signal};

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
    /// Per-check settings in effect, which change when the hub sends overrides; a check
    /// without an entry uses the defaults.
    pub settings: crate::overrides::SettingsView,
    /// Reads served by the helper when the privilege boundary is in effect; `None` runs the
    /// checks' own commands.
    pub privileged: Option<Arc<dyn PrivilegedReads>>,
}

impl CheckEnv {
    /// The settings for `key`.
    #[must_use]
    pub fn settings_for(&self, key: &CheckKey) -> CheckSettings {
        self.settings.borrow().get(key).cloned().unwrap_or_default()
    }

    /// Runs `check` on a copy of its stored state and returns the report with the state it
    /// left, without storing it. The scheduler stores it only once the run's signals are
    /// spooled.
    pub async fn run_staged(
        &self,
        check: &dyn Check,
        store: &CheckStore,
    ) -> (Result<CheckReport, CheckError>, CheckState) {
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
            privileged: self.privileged.as_deref(),
            state: &mut state,
        };
        let report = run_bounded(check, &mut ctx).await;
        (report, state)
    }

    /// Runs `check` once on its stored state and stores the state it leaves, for on-demand
    /// runs that have no spool batch of their own.
    pub async fn run(
        &self,
        check: &dyn Check,
        store: &CheckStore,
    ) -> Result<CheckReport, CheckError> {
        let (report, state) = self.run_staged(check, store).await;
        store.put_state(&check.key(), state);
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
        } else if previous.contains(&name) && check.resolves_itself() {
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
    // An event check never reports a recovery; only a person closes its incident.
    signals.extend(blind::ended(host, check, previous, now));
    let gone_targets = if check.resolves_itself() {
        previous
            .difference(&seen)
            .filter(|gone| *gone != BLIND_TARGET)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    for gone in &gone_targets {
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
) -> (&'a dyn Check, (Result<CheckReport, CheckError>, CheckState)) {
    (check, env.run_staged(check, store).await)
}

/// One run's result, held until its signals are on disk in the spool.
struct Staged {
    /// Which check ran.
    key: CheckKey,
    /// The state it left behind.
    state: CheckState,
    /// The targets firing now, when the run changed what is firing.
    firing: Option<BTreeSet<String>>,
    /// The digest facts of a run that produced a report.
    facts: Option<Vec<DigestFact>>,
}

/// Runs checks until shutdown.
pub struct Scheduler {
    checks: Arc<dyn CheckSet>,
    env: CheckEnv,
    store: Arc<CheckStore>,
    spool: Arc<SharedSpool>,
    link: LinkHandle,
    /// The facts each check's last successful run reported, by check key. A run replaces its
    /// check's set, so a target a check stops reporting leaves the map.
    facts: BTreeMap<String, BTreeMap<String, DigestFact>>,
    /// Fact keys a successful run dropped and the hub has not been told about yet.
    retired: BTreeSet<String>,
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
            retired: BTreeSet::new(),
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
        let mut last_run: BTreeMap<CheckKey, Timestamp> = BTreeMap::new();
        let mut changes = self.env.settings.clone();
        changes.mark_unchanged();
        // A fixed view has no sender; stop watching it rather than spin on the error.
        let mut watching = true;
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
                    last_run.insert(key.clone(), after);
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
                let changed = async {
                    if watching {
                        changes.changed().await.is_ok()
                    } else {
                        std::future::pending().await
                    }
                };
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = clock.sleep_until(wake) => {}
                    ok = changed => {
                        watching = ok;
                        if ok {
                            self.reschedule(&mut due, &last_run);
                        }
                    }
                }
            }
        }
    }

    /// Applies new settings: a check turned off leaves the schedule, one turned on runs
    /// now, and the rest run one (possibly new) interval after their last run.
    fn reschedule(
        &self,
        due: &mut BTreeMap<CheckKey, Timestamp>,
        last_run: &BTreeMap<CheckKey, Timestamp>,
    ) {
        let now = self.env.clock.now();
        for check in self.checks.all() {
            let key = check.key();
            if !self.env.settings_for(&key).enabled {
                due.remove(&key);
                continue;
            }
            let next = last_run.get(&key).map_or(now, |at| {
                at.saturating_add(self.cadence(check)).unwrap_or(now)
            });
            due.insert(key, next);
        }
        tracing::debug!(
            scheduled = due.len(),
            "rescheduled checks after new settings"
        );
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
        let mut staged: Vec<Staged> = Vec::new();
        let mut batch = Vec::new();
        for (check, (report, state)) in reports {
            let key = check.key();
            let report = match report {
                Ok(report) => report,
                Err(err) if err.is_unavailable() => {
                    tracing::debug!(check = %key, %err, "check unavailable on this host");
                    staged.push(Staged {
                        key,
                        state,
                        firing: None,
                        facts: None,
                    });
                    continue;
                }
                Err(err) => {
                    tracing::warn!(check = %key, %err, "check failed");
                    let firing = if let CheckError::JournalUnreadable(why) = &err {
                        batch.push(blind_signal(self.env.host, check, why, now));
                        let mut firing = self.store.firing(&key);
                        firing.insert(BLIND_TARGET.to_owned());
                        Some(firing)
                    } else {
                        None
                    };
                    staged.push(Staged {
                        key,
                        state,
                        firing,
                        facts: None,
                    });
                    continue;
                }
            };
            let previous = self.store.firing(&key);
            let (signals, firing) = signals_for(self.env.host, check, &report, &previous, now);
            batch.extend(signals);
            staged.push(Staged {
                key,
                state,
                firing: Some(firing),
                facts: Some(report.facts),
            });
        }
        // Nothing is remembered from a run whose signals could not be spooled: the next run
        // repeats them instead of losing a firing or resolved signal.
        if !batch.is_empty()
            && let Err(err) = self.spool.push(batch).await
        {
            tracing::error!(
                %err,
                "could not spool signals; this run's state is discarded and the run repeats"
            );
            return;
        }
        for staged in staged {
            self.store.put_state(&staged.key, staged.state);
            // A failed or unavailable run keeps the previous facts: they are stale, not retired.
            if let Some(firing) = staged.firing {
                self.store.set_firing(&staged.key, firing);
            }
            if let Some(facts) = staged.facts {
                self.replace_facts(&staged.key, facts);
            }
        }
        if let Err(err) = self.store.save() {
            tracing::error!(%err, "could not save check state");
        }
    }

    /// Replaces the facts of `check` with those of its latest successful run and remembers the
    /// keys it no longer reports, so the hub can mark them "no longer reported".
    fn replace_facts(&mut self, check: &CheckKey, facts: Vec<DigestFact>) {
        let fresh: BTreeMap<String, DigestFact> =
            facts.into_iter().map(|f| (f.key.clone(), f)).collect();
        let old = self.facts.insert(check.to_string(), fresh);
        let Some(old) = old else { return };
        let now = self.facts.get(&check.to_string());
        for key in old.keys() {
            if now.is_none_or(|set| !set.contains_key(key)) {
                self.retired.insert(key.clone());
            }
        }
    }

    async fn publish_facts(&mut self) {
        let mut current: BTreeMap<String, DigestFact> = BTreeMap::new();
        for set in self.facts.values() {
            for (key, fact) in set {
                current.insert(key.clone(), fact.clone());
            }
        }
        // Another check may still report a key this one dropped.
        self.retired.retain(|key| !current.contains_key(key));
        let retired: Vec<String> = self.retired.iter().cloned().collect();
        let mut facts: Vec<DigestFact> = current.into_values().collect();
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
        if facts.is_empty() && retired.is_empty() {
            return;
        }
        if self
            .link
            .send(Message::DigestFacts(DigestFacts { facts, retired }), None)
        {
            self.retired.clear();
        } else {
            tracing::debug!("digest facts were not sent; the outbox is full");
        }
    }
}
