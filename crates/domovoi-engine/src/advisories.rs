//! Advisory rules: low-priority hygiene findings. Pure.
//!
//! Each rule looks at current state and says which advisories should be open. Open ones
//! whose condition cleared are resolved. Ids are derived from the kind, host and subject,
//! so evaluating again finds the same advisory instead of adding a copy. Advisories appear
//! in the digest and the web UI only; they never notify on their own.

use std::collections::BTreeSet;

use domovoi_core::{
    Advisory, AdvisoryId, AdvisoryKind, BaselineEntry, DigestFact, FactValue, HostId,
};
use jiff::{SignedDuration, Timestamp};
use sha2::{Digest, Sha256};

/// How long heartbeat pings may fail before it is an advisory.
pub const HEARTBEAT_GRACE: SignedDuration = SignedDuration::from_mins(10);
/// Images older than this many days are stale.
pub const STALE_IMAGE_DAYS: f64 = 180.0;

/// The hub's own heartbeat to an external monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatStatus {
    /// Whether a heartbeat URL is configured.
    pub configured: bool,
    /// Since when pings have been failing, if they are.
    pub failing_since: Option<Timestamp>,
}

/// One agent's version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentVersion {
    /// The agent's host.
    pub host: HostId,
    /// Its version, such as `0.3.1`.
    pub version: String,
}

/// What the rules look at.
#[derive(Debug, Clone, Copy)]
pub struct AdvisoryInputs<'a> {
    /// The hub's heartbeat.
    pub heartbeat: HeartbeatStatus,
    /// The latest fact per host and key.
    pub facts: &'a [DigestFact],
    /// Names of secrets marked exposed in the configuration.
    pub exposed_secrets: &'a [String],
    /// Every enrolled agent's version.
    pub agents: &'a [AgentVersion],
    /// The hub's version.
    pub hub_version: &'a str,
    /// Baseline entries that expired since the last evaluation.
    pub expired_baselines: &'a [BaselineEntry],
    /// Why triage is degraded, if it is.
    pub triage_degraded: Option<&'a str>,
}

/// What to store after evaluating.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdvisoryChanges {
    /// New or changed advisories.
    pub upserts: Vec<Advisory>,
    /// Open advisories whose condition cleared.
    pub resolve: Vec<AdvisoryId>,
}

/// The stable id of the advisory of `kind` about `subject` on `host`.
#[must_use]
pub fn advisory_id(kind: AdvisoryKind, host: Option<HostId>, subject: &str) -> AdvisoryId {
    let kind = serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let host = host.map(|h| h.to_string()).unwrap_or_default();
    let digest = Sha256::digest(format!("advisory:v1\n{kind}\n{host}\n{subject}"));
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    AdvisoryId::from_uuid(uuid::Uuid::from_bytes(bytes))
}

/// The `TriageDegraded` advisory for `reason`, raised at `now`. The triage service stores
/// it itself; [`evaluate`] produces the same one from [`AdvisoryInputs::triage_degraded`].
#[must_use]
pub fn triage_degraded(reason: &str, now: Timestamp) -> Advisory {
    Advisory {
        id: advisory_id(AdvisoryKind::TriageDegraded, None, ""),
        kind: AdvisoryKind::TriageDegraded,
        host: None,
        title: "Triage is degraded".to_owned(),
        detail: format!("{reason}. Alerts still go out without triage."),
        created_at: now,
        resolved_at: None,
    }
}

struct Wanted {
    kind: AdvisoryKind,
    host: Option<HostId>,
    subject: String,
    title: String,
    detail: String,
}

fn number(fact: &DigestFact) -> Option<f64> {
    match fact.value {
        FactValue::Number(v) => Some(v),
        FactValue::Text(_) => None,
    }
}

fn truthy(fact: &DigestFact) -> bool {
    match &fact.value {
        FactValue::Number(v) => *v > 0.0,
        FactValue::Text(t) => matches!(t.trim().to_lowercase().as_str(), "true" | "yes" | "1"),
    }
}

/// `(major, minor)` from a version such as `v0.3.1`.
fn major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

fn wanted(inputs: &AdvisoryInputs<'_>, now: Timestamp) -> Vec<Wanted> {
    let mut out = Vec::new();
    let heartbeat = inputs.heartbeat;
    if !heartbeat.configured {
        out.push(Wanted {
            kind: AdvisoryKind::HeartbeatMissing,
            host: None,
            subject: "unconfigured".to_owned(),
            title: "No heartbeat is configured".to_owned(),
            detail: "Set [heartbeat] url to an external monitor so you hear if the hub itself goes down.".to_owned(),
        });
    } else if let Some(since) = heartbeat.failing_since
        && now.duration_since(since) > HEARTBEAT_GRACE
    {
        out.push(Wanted {
            kind: AdvisoryKind::HeartbeatMissing,
            host: None,
            subject: "failing".to_owned(),
            title: "Heartbeat pings are failing".to_owned(),
            detail: format!("The heartbeat URL has not answered since {since}. Check the monitor and the hub's network."),
        });
    }
    if let Some(reason) = inputs.triage_degraded {
        let advisory = triage_degraded(reason, now);
        out.push(Wanted {
            kind: advisory.kind,
            host: advisory.host,
            subject: String::new(),
            title: advisory.title,
            detail: advisory.detail,
        });
    }
    for fact in inputs.facts {
        if let Some(name) = fact
            .key
            .strip_prefix("container.")
            .and_then(|k| k.strip_suffix(".image_age_days"))
            && number(fact).is_some_and(|days| days > STALE_IMAGE_DAYS)
        {
            let days = number(fact).unwrap_or_default().round();
            out.push(Wanted {
                kind: AdvisoryKind::StaleImage,
                host: Some(fact.host),
                subject: name.to_owned(),
                title: format!("{name} image is {days:.0} days old"),
                detail: "Pull a newer image and recreate the container.".to_owned(),
            });
        }
        if let Some(project) = fact
            .key
            .strip_prefix("stack.")
            .and_then(|k| k.strip_suffix(".source_missing"))
            && truthy(fact)
        {
            out.push(Wanted {
                kind: AdvisoryKind::StackWithoutSource,
                host: Some(fact.host),
                subject: project.to_owned(),
                title: format!("Stack {project} has no source files on disk"),
                detail: "Keep the compose file next to the stack, or in version control, so it can be rebuilt.".to_owned(),
            });
        }
    }
    for name in inputs.exposed_secrets {
        out.push(Wanted {
            kind: AdvisoryKind::SecretExposed,
            host: None,
            subject: name.clone(),
            title: format!("Secret {name} is marked exposed"),
            detail: "Rotate it, then remove it from [secrets] exposed.".to_owned(),
        });
    }
    for entry in inputs.expired_baselines {
        let kind = serde_json::to_value(entry.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        out.push(Wanted {
            kind: AdvisoryKind::BaselineEntryExpired,
            host: Some(entry.host),
            subject: format!("{kind}:{}", entry.key),
            title: format!("{} left the baseline after being gone too long", entry.key),
            detail: "If it should be running, start it again; it will be learned anew.".to_owned(),
        });
    }
    if let Some((hub_major, hub_minor)) = major_minor(inputs.hub_version) {
        for agent in inputs.agents {
            let behind = major_minor(&agent.version).is_some_and(|(major, minor)| {
                major < hub_major || (major == hub_major && minor < hub_minor)
            });
            if behind {
                out.push(Wanted {
                    kind: AdvisoryKind::AgentOutdated,
                    host: Some(agent.host),
                    subject: String::new(),
                    title: format!(
                        "Agent {} is older than the hub ({})",
                        agent.version, inputs.hub_version
                    ),
                    detail: "Upgrade the agent to the hub's version.".to_owned(),
                });
            }
        }
    }
    out
}

/// Which advisories to open, update or resolve, given the `open` ones.
#[must_use]
pub fn evaluate(inputs: &AdvisoryInputs<'_>, open: &[Advisory], now: Timestamp) -> AdvisoryChanges {
    let mut changes = AdvisoryChanges::default();
    let mut keep = BTreeSet::new();
    for want in wanted(inputs, now) {
        let id = advisory_id(want.kind, want.host, &want.subject);
        if !keep.insert(id) {
            continue;
        }
        let existing = open.iter().find(|a| a.id == id && a.resolved_at.is_none());
        let advisory = Advisory {
            id,
            kind: want.kind,
            host: want.host,
            title: want.title,
            detail: want.detail,
            created_at: existing.map_or(now, |a| a.created_at),
            resolved_at: None,
        };
        if existing != Some(&advisory) {
            changes.upserts.push(advisory);
        }
    }
    changes.resolve = open
        .iter()
        .filter(|a| a.resolved_at.is_none() && !keep.contains(&a.id))
        .map(|a| a.id)
        .collect();
    changes
}
