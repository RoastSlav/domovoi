//! `domovoi selftest`: one synthetic critical signal through manager, triage, card and
//! gateway, with PASS or FAIL per stage (INV-11: delivery is proven, not assumed).
//!
//! The hub is assembled from the given configuration in a temporary data directory, so
//! the run never touches the real database and works next to a running hub. No
//! listener is opened. Offline mode swaps the model and chat for built-in stand-ins.

mod fakes;

use std::fmt;
use std::sync::Arc;

use domovoi_config::hub::HubConfig;
use domovoi_core::ports::{AdvisoryRepo, ChatGateway, Clock, LlmProvider, NotifyStateRepo};
use domovoi_core::{
    AdvisoryKind, CheckKey, HostId, IncidentId, Severity, Signal, SignalClass, SourceKind,
    TriageOutcome,
};
use domovoi_engine::{DomainEvent, EventKind};
use jiff::SignedDuration;
use tokio::sync::broadcast;

use crate::runtime::{AdapterOverrides, HubDeps, HubRuntime, adapters};

/// Where to send the synthetic incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The configured model and chat gateways: proves real delivery.
    Online,
    /// A built-in scripted model and a recording gateway: proves the pipeline only.
    Offline,
}

/// How one stage went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It worked.
    Pass,
    /// It did not run, with why (for example, triage is disabled).
    Skip(String),
    /// It failed, with why.
    Fail(String),
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageResult {
    /// `config`, `store`, `manager`, `triage`, `card` or `gateway`.
    pub stage: &'static str,
    /// What happened.
    pub outcome: Outcome,
}

/// Every stage in order. Stages after a failure are reported as skipped.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelftestReport {
    /// The stages, in pipeline order.
    pub stages: Vec<StageResult>,
}

impl SelftestReport {
    /// Whether no stage failed.
    #[must_use]
    pub fn passed(&self) -> bool {
        !self
            .stages
            .iter()
            .any(|s| matches!(s.outcome, Outcome::Fail(_)))
    }

    fn record(&mut self, stage: &'static str, outcome: Outcome) -> bool {
        let ok = !matches!(outcome, Outcome::Fail(_));
        self.stages.push(StageResult { stage, outcome });
        ok
    }

    fn skip_rest(&mut self, from: &[&'static str]) {
        for stage in from {
            self.record(stage, Outcome::Skip("an earlier stage failed".to_owned()));
        }
    }
}

impl fmt::Display for SelftestReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for stage in &self.stages {
            match &stage.outcome {
                Outcome::Pass => writeln!(f, "PASS  {}", stage.stage)?,
                Outcome::Skip(why) => writeln!(f, "SKIP  {}: {why}", stage.stage)?,
                Outcome::Fail(why) => writeln!(f, "FAIL  {}: {why}", stage.stage)?,
            }
        }
        Ok(())
    }
}

/// How long the manager may take to open the incident.
const MANAGER_WAIT: SignedDuration = SignedDuration::from_secs(10);
/// How long triage may take; a real model can be slow.
const TRIAGE_WAIT: SignedDuration = SignedDuration::from_secs(120);
/// How long delivery may take, retries included.
const DELIVERY_WAIT: SignedDuration = SignedDuration::from_secs(60);
/// How often delivery state is polled.
const POLL: SignedDuration = SignedDuration::from_millis(100);
/// How long the update after resolving may take to be delivered.
const SETTLE: SignedDuration = SignedDuration::from_secs(1);
/// How long the assembled hub may take to stop.
const STOP_WAIT: SignedDuration = SignedDuration::from_secs(10);

const AFTER_STORE: [&str; 4] = ["manager", "triage", "card", "gateway"];

/// Runs the selftest. `config` must be validated with secrets resolved.
pub async fn run(mut config: HubConfig, deps: HubDeps, mode: Mode) -> SelftestReport {
    let mut report = SelftestReport::default();
    report.record("config", Outcome::Pass);
    let dir = match tempfile::Builder::new()
        .prefix("domovoi-selftest-")
        .tempdir()
    {
        Ok(dir) => dir,
        Err(err) => {
            report.record(
                "store",
                Outcome::Fail(format!("no temporary directory: {err}")),
            );
            report.skip_rest(&AFTER_STORE);
            return report;
        }
    };
    config.hub.data_dir = dir.path().to_path_buf();
    config.backup.dir = Some(dir.path().join("backups"));
    config.notify.quiet_hours = None;
    let recording = Arc::new(fakes::RecordingGateway::default());
    let overrides = match mode {
        Mode::Online => AdapterOverrides::default(),
        Mode::Offline => {
            config.triage.enabled = true;
            let primary: Arc<dyn LlmProvider> = Arc::new(fakes::ScriptedProvider);
            AdapterOverrides {
                primary: Some(primary),
                gateways: Some(vec![recording.clone() as Arc<dyn ChatGateway>]),
                notifiers: Some(Vec::new()),
                ..AdapterOverrides::default()
            }
        }
    };
    let triage_on = config.triage.enabled;
    let runtime = match HubRuntime::build_with(config, deps, overrides).await {
        Ok(runtime) => runtime,
        Err(err) => {
            report.record(
                "store",
                Outcome::Fail(format!("the hub did not assemble: {err}")),
            );
            report.skip_rest(&AFTER_STORE);
            return report;
        }
    };
    report.record("store", Outcome::Pass);
    let probe = (mode == Mode::Offline).then_some(&*recording);
    pipeline(&runtime, &mut report, triage_on, probe).await;
    stop(runtime).await;
    drop(dir);
    report
}

/// `recording` is the offline gateway, whose counts are waited on directly; without it,
/// delivery is read back from the store.
async fn pipeline(
    runtime: &HubRuntime,
    report: &mut SelftestReport,
    triage_on: bool,
    recording: Option<&fakes::RecordingGateway>,
) {
    let mut events = runtime.bus.subscribe();
    let dispatcher = tokio::spawn(
        runtime
            .dispatcher
            .clone()
            .run(runtime.bus.subscribe(), runtime.delivery_shutdown.clone()),
    );
    let platforms: Vec<String> = runtime
        .gateways
        .iter()
        .map(|(gateway, _)| gateway.platform().to_owned())
        .collect();

    let incident = match open_incident(runtime, &mut events).await {
        Ok(incident) => {
            report.record("manager", Outcome::Pass);
            incident
        }
        Err(why) => {
            report.record("manager", Outcome::Fail(why));
            report.skip_rest(&AFTER_STORE[1..]);
            dispatcher.abort();
            return;
        }
    };
    let triage = if triage_on {
        wait_for_triage(&*runtime.clock, &mut events, incident).await
    } else {
        Outcome::Skip("triage is disabled in [triage]".to_owned())
    };
    if !report.record("triage", triage) {
        report.skip_rest(&AFTER_STORE[2..]);
        dispatcher.abort();
        return;
    }
    let card = match (recording, platforms.is_empty()) {
        (_, true) => {
            Outcome::Fail("no chat gateway is enabled, so no card can be posted".to_owned())
        }
        (Some(recording), false) => {
            let posted = recording.wait(|counts| counts.posted >= 1);
            within(
                &*runtime.clock,
                DELIVERY_WAIT,
                posted,
                "no card was posted in time",
            )
            .await
        }
        (None, false) => wait_for_cards(runtime, incident, &platforms).await,
    };
    let card_ok = report.record("card", card);
    let gateway = if card_ok {
        resolve_and_check(runtime, &mut events, incident, recording).await
    } else {
        Outcome::Skip("no card was posted".to_owned())
    };
    report.record("gateway", gateway);
    dispatcher.abort();
}

async fn open_incident(
    runtime: &HubRuntime,
    events: &mut broadcast::Receiver<DomainEvent>,
) -> Result<IncidentId, String> {
    let host = adapters::hub_host(&runtime.store)
        .await
        .map_err(|err| err.to_string())?;
    runtime
        .manager
        .ingest(vec![signal(host, true, runtime)?])
        .await
        .map_err(|err| format!("the manager refused the signal: {err}"))?;
    wait_for(&*runtime.clock, events, MANAGER_WAIT, |event| {
        matches!(event.kind, EventKind::IncidentOpened).then(|| event.incident.id())
    })
    .await
    .ok_or_else(|| "no incident opened".to_owned())
}

fn signal(host: HostId, firing: bool, runtime: &HubRuntime) -> Result<Signal, String> {
    let check = CheckKey::parse("domovoi.selftest").map_err(|err| err.to_string())?;
    let now = runtime.clock.now();
    Ok(if firing {
        Signal::firing(
            SourceKind::Probe,
            host,
            check,
            None,
            Severity::Critical,
            "domovoi selftest: synthetic critical signal",
            now,
        )
        .with_class(SignalClass::Immediate)
    } else {
        Signal::resolved(
            SourceKind::Probe,
            host,
            check,
            None,
            "domovoi selftest: resolved",
            now,
        )
    })
}

async fn wait_for_triage(
    clock: &dyn Clock,
    events: &mut broadcast::Receiver<DomainEvent>,
    incident: IncidentId,
) -> Outcome {
    let outcome = wait_for(clock, events, TRIAGE_WAIT, |event| match &event.kind {
        EventKind::TriageCompleted { outcome, .. } if event.incident.id() == incident => {
            Some(outcome.clone())
        }
        _ => None,
    })
    .await;
    match outcome {
        Some(TriageOutcome::Report(_) | TriageOutcome::Cached { .. }) => Outcome::Pass,
        Some(TriageOutcome::Skipped { reason }) => Outcome::Fail(format!("skipped: {reason}")),
        Some(other) => Outcome::Fail(format!("{other:?}")),
        None => Outcome::Fail("no triage result in time".to_owned()),
    }
}

async fn wait_for_cards(
    runtime: &HubRuntime,
    incident: IncidentId,
    platforms: &[String],
) -> Outcome {
    let clock = &*runtime.clock;
    let deadline = after(clock, DELIVERY_WAIT);
    loop {
        match NotifyStateRepo::get(&*runtime.store, incident).await {
            Ok(Some(state)) if platforms.iter().all(|p| state.messages.contains_key(p)) => {
                return Outcome::Pass;
            }
            Ok(_) => {}
            Err(err) => return Outcome::Fail(err.to_string()),
        }
        if let Some(why) = delivery_failure(runtime).await {
            return Outcome::Fail(why);
        }
        if clock.now() >= deadline {
            return Outcome::Fail(format!("no card reached {} in time", platforms.join(", ")));
        }
        clock.sleep_until(after(clock, POLL)).await;
    }
}

/// Resolves the synthetic incident, so its card turns green, and checks that no delivery
/// failed along the way.
async fn resolve_and_check(
    runtime: &HubRuntime,
    events: &mut broadcast::Receiver<DomainEvent>,
    incident: IncidentId,
    recording: Option<&fakes::RecordingGateway>,
) -> Outcome {
    let updates_before = recording.map(|r| r.counts().updated);
    let host = match adapters::hub_host(&runtime.store).await {
        Ok(host) => host,
        Err(err) => return Outcome::Fail(err.to_string()),
    };
    let signal = match signal(host, false, runtime) {
        Ok(signal) => signal,
        Err(why) => return Outcome::Fail(why),
    };
    if let Err(err) = runtime.manager.ingest(vec![signal]).await {
        return Outcome::Fail(format!("the manager refused the resolution: {err}"));
    }
    let resolved = wait_for(&*runtime.clock, events, MANAGER_WAIT, |event| {
        (matches!(event.kind, EventKind::IncidentResolved) && event.incident.id() == incident)
            .then_some(())
    })
    .await;
    if resolved.is_none() {
        return Outcome::Fail("the incident did not resolve".to_owned());
    }
    match (recording, updates_before) {
        (Some(recording), Some(before)) => {
            let updated = recording.wait(|counts| counts.updated > before);
            let outcome = within(
                &*runtime.clock,
                DELIVERY_WAIT,
                updated,
                "the resolved card was not updated in time",
            )
            .await;
            if outcome != Outcome::Pass {
                return outcome;
            }
        }
        _ => {
            // Give the dispatcher a moment to deliver the update before checking for
            // failures.
            let settled = after(&*runtime.clock, SETTLE);
            runtime.clock.sleep_until(settled).await;
        }
    }
    match delivery_failure(runtime).await {
        Some(why) => Outcome::Fail(why),
        None => Outcome::Pass,
    }
}

async fn delivery_failure(runtime: &HubRuntime) -> Option<String> {
    match AdvisoryRepo::list_open(&*runtime.store).await {
        Ok(advisories) => advisories
            .into_iter()
            .find(|a| a.kind == AdvisoryKind::DeliveryFailed)
            .map(|a| format!("delivery failed: {}: {}", a.title, a.detail)),
        Err(err) => Some(err.to_string()),
    }
}

/// [`Outcome::Pass`] when `done` finishes within `limit` on `clock`.
async fn within(
    clock: &dyn Clock,
    limit: SignedDuration,
    done: impl Future<Output = ()>,
    late: &str,
) -> Outcome {
    tokio::select! {
        () = done => Outcome::Pass,
        () = clock.sleep_until(after(clock, limit)) => Outcome::Fail(late.to_owned()),
    }
}

fn after(clock: &dyn Clock, wait: SignedDuration) -> jiff::Timestamp {
    let now = clock.now();
    now.checked_add(wait).unwrap_or(now)
}

/// The first event `matches` accepts, or `None` once `limit` has passed on `clock`.
async fn wait_for<T>(
    clock: &dyn Clock,
    events: &mut broadcast::Receiver<DomainEvent>,
    limit: SignedDuration,
    mut matches: impl FnMut(&DomainEvent) -> Option<T>,
) -> Option<T> {
    let deadline = after(clock, limit);
    let found = async {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if let Some(found) = matches(&event) {
                        return Some(found);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    };
    tokio::select! {
        found = found => found,
        () = clock.sleep_until(deadline) => None,
    }
}

async fn stop(runtime: HubRuntime) {
    runtime.intake_shutdown.cancel();
    runtime.agent_server.shutdown();
    runtime.worker_shutdown.cancel();
    runtime.manager_shutdown.cancel();
    runtime.delivery_shutdown.cancel();
    let tasks = async {
        if let Some(task) = runtime.triage_task {
            let _ = task.await;
        }
        let _ = runtime.actions_task.await;
        let _ = runtime.manager_task.await;
    };
    let clock = runtime.clock.clone();
    tokio::select! {
        () = tasks => {}
        () = clock.sleep_until(after(&*clock, STOP_WAIT)) => {
            tracing::warn!("selftest hub did not stop within 10 seconds");
        }
    }
    runtime.store.close().await;
}

#[cfg(test)]
mod tests;
