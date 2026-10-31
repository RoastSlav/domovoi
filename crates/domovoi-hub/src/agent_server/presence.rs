//! Telling a dead host from a dead agent, and both from the hub's own network trouble.
//!
//! An agent silent for three heartbeats is looked into once. The hub first doubts itself:
//! if its own gateway or the configured outside target does not answer, it reports
//! `hub.network_degraded` and blames no host. Otherwise it probes the silent host: an
//! answer means `agent.down`, silence means `host.down`. Hearing from the agent again
//! resolves whichever was raised.

use std::sync::atomic::Ordering;

use domovoi_core::{CheckKey, HostId, Severity, Signal, SignalClass, SourceKind};
use jiff::{SignedDuration, Timestamp};

use super::Inner;
use super::registry::{Liveness, Watch};

const AGENT_DOWN: &str = "agent.down";
const HOST_DOWN: &str = "host.down";
const NETWORK_DEGRADED: &str = "hub.network_degraded";

fn key(text: &str) -> Option<CheckKey> {
    match CheckKey::parse(text) {
        Ok(key) => Some(key),
        Err(err) => {
            tracing::error!(key = text, %err, "a built-in check key does not parse");
            None
        }
    }
}

/// "for 45s", "for 12m", "for 3h".
fn quiet_for(silence: SignedDuration) -> String {
    let secs = silence.as_secs().max(0);
    match secs {
        0..120 => format!("for {secs}s"),
        120..7200 => format!("for {}m", secs / 60),
        _ => format!("for {}h", secs / 3600),
    }
}

fn liveness_key(liveness: Liveness) -> &'static str {
    match liveness {
        Liveness::AgentDown => AGENT_DOWN,
        Liveness::HostDown => HOST_DOWN,
    }
}

/// Submits one signal; false when the sink refused it, so the next check tries again.
async fn submit(inner: &Inner, signal: Signal) -> bool {
    match inner.deps.sink.submit(vec![signal]).await {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(%err, "could not submit a liveness signal");
            false
        }
    }
}

/// One presence check at the clock's current time.
pub(crate) async fn check(inner: &Inner) {
    let now = inner.deps.clock.now();
    let silence = inner.settings.silence();
    let watches = inner.registry.watches();
    let is_silent = |w: &Watch| w.last_seen.is_none_or(|t| now.duration_since(t) >= silence);
    for watch in watches
        .iter()
        .filter(|w| w.raised.is_some() && !is_silent(w))
    {
        resolve(inner, watch, now).await;
    }
    let silent: Vec<&Watch> = watches
        .iter()
        .filter(|w| w.raised.is_none() && is_silent(w))
        .collect();
    let degraded = inner.network_degraded.load(Ordering::SeqCst);
    if silent.is_empty() && !degraded {
        return;
    }
    if !hub_network_ok(inner).await {
        if !degraded {
            let raised = match key(NETWORK_DEGRADED) {
                Some(check) => {
                    let summary = format!(
                        "The hub cannot reach its gateway or {}; host checks are paused",
                        inner.settings.self_check
                    );
                    let signal = Signal::firing(
                        SourceKind::Probe,
                        inner.deps.hub_host,
                        check,
                        None,
                        Severity::Warning,
                        summary,
                        now,
                    );
                    submit(inner, signal).await
                }
                None => false,
            };
            inner.network_degraded.store(raised, Ordering::SeqCst);
        }
        return;
    }
    if degraded && let Some(check) = key(NETWORK_DEGRADED) {
        let signal = Signal::resolved(
            SourceKind::Probe,
            inner.deps.hub_host,
            check,
            None,
            "The hub's network is back",
            now,
        );
        if submit(inner, signal).await {
            inner.network_degraded.store(false, Ordering::SeqCst);
        }
    }
    for watch in silent {
        raise(inner, watch, now).await;
    }
}

async fn hub_network_ok(inner: &Inner) -> bool {
    let probes = &*inner.deps.probes;
    probes.gateway_reachable().await && probes.tcp_reachable(&inner.settings.self_check).await
}

async fn host_name(inner: &Inner, host: HostId) -> String {
    match inner.deps.hosts.get_host(host).await {
        Ok(Some(found)) => found.name.to_string(),
        _ => host.to_string(),
    }
}

/// Where to look for the host: its `[[probes]]` TCP targets, else its last address on
/// the SSH and HTTPS ports.
async fn targets(inner: &Inner, watch: &Watch) -> Vec<String> {
    if let Ok(Some(host)) = inner.deps.hosts.get_host(watch.host).await
        && let Some(configured) = inner.settings.probes.get(&host.name)
    {
        return configured.clone();
    }
    match watch.remote_ip {
        Some(ip) => [22, 443]
            .iter()
            .map(|port| std::net::SocketAddr::new(ip, *port).to_string())
            .collect(),
        None => Vec::new(),
    }
}

async fn raise(inner: &Inner, watch: &Watch, now: Timestamp) {
    let name = host_name(inner, watch.host).await;
    let targets = targets(inner, watch).await;
    let mut reachable = targets.is_empty();
    for target in &targets {
        if inner.deps.probes.tcp_reachable(target).await {
            reachable = true;
            break;
        }
    }
    let quiet = watch
        .last_seen
        .map(|t| quiet_for(now.duration_since(t)))
        .unwrap_or_else(|| "since the hub started".to_owned());
    let (liveness, severity, summary) = if reachable {
        let summary = if targets.is_empty() {
            format!(
                "{name}: the agent has been silent {quiet}; add a [[probes]] entry to tell host down from agent down"
            )
        } else {
            format!("{name}: the agent has been silent {quiet}, but the host answers")
        };
        (Liveness::AgentDown, Severity::Warning, summary)
    } else {
        let summary = format!("{name} is down: no agent {quiet} and no answer on the network");
        (Liveness::HostDown, Severity::Critical, summary)
    };
    let Some(check) = key(liveness_key(liveness)) else {
        return;
    };
    let mut signal = Signal::firing(
        SourceKind::Probe,
        watch.host,
        check,
        None,
        severity,
        summary,
        now,
    );
    if liveness == Liveness::HostDown {
        signal = signal.with_class(SignalClass::Immediate);
    }
    if submit(inner, signal).await {
        inner.registry.set_raised(watch.host, Some(liveness));
    }
}

async fn resolve(inner: &Inner, watch: &Watch, now: Timestamp) {
    let Some(liveness) = watch.raised else {
        return;
    };
    let Some(check) = key(liveness_key(liveness)) else {
        return;
    };
    let name = host_name(inner, watch.host).await;
    let signal = Signal::resolved(
        SourceKind::Probe,
        watch.host,
        check,
        None,
        format!("{name}: the agent is back"),
        now,
    );
    if submit(inner, signal).await {
        inner.registry.set_raised(watch.host, None);
    }
}
