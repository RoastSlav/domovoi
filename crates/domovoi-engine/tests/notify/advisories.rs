//! Advisory rules: each finding, stable ids, and resolution when the condition clears.

use domovoi_core::{
    Advisory, AdvisoryKind, BaselineEntry, BaselineKind, Criticality, DigestFact, FactValue, HostId,
};
use domovoi_engine::advisories::{
    AdvisoryChanges, AdvisoryInputs, AgentVersion, HeartbeatStatus, advisory_id, evaluate,
};
use jiff::{SignedDuration, Timestamp};

use crate::harness::ts;

fn healthy() -> HeartbeatStatus {
    HeartbeatStatus {
        configured: true,
        failing_since: None,
    }
}

struct Input {
    heartbeat: HeartbeatStatus,
    facts: Vec<DigestFact>,
    secrets: Vec<String>,
    agents: Vec<AgentVersion>,
    expired: Vec<BaselineEntry>,
    triage: Option<&'static str>,
}

impl Input {
    fn new() -> Self {
        Self {
            heartbeat: healthy(),
            facts: Vec::new(),
            secrets: Vec::new(),
            agents: Vec::new(),
            expired: Vec::new(),
            triage: None,
        }
    }

    fn run(&self, open: &[Advisory], now: Timestamp) -> AdvisoryChanges {
        let inputs = AdvisoryInputs {
            heartbeat: self.heartbeat,
            facts: &self.facts,
            exposed_secrets: &self.secrets,
            agents: &self.agents,
            hub_version: "0.4.2",
            expired_baselines: &self.expired,
            triage_degraded: self.triage,
        };
        evaluate(&inputs, open, now)
    }
}

fn kinds(changes: &AdvisoryChanges) -> Vec<AdvisoryKind> {
    changes.upserts.iter().map(|a| a.kind).collect()
}

fn fact(host: HostId, key: &str, value: FactValue) -> DigestFact {
    DigestFact {
        host,
        key: key.to_owned(),
        label: key.to_owned(),
        value,
        unit: None,
        observed_at: ts("2026-10-31T00:00:00Z"),
    }
}

const NOW: &str = "2026-10-31T12:00:00Z";

#[test]
fn nothing_wrong_means_no_advisories() {
    assert_eq!(Input::new().run(&[], ts(NOW)), AdvisoryChanges::default());
}

#[test]
fn heartbeat_missing_when_unconfigured_or_failing_over_ten_minutes() {
    let now = ts(NOW);
    let mut input = Input::new();
    input.heartbeat = HeartbeatStatus {
        configured: false,
        failing_since: None,
    };
    assert_eq!(
        kinds(&input.run(&[], now)),
        [AdvisoryKind::HeartbeatMissing]
    );
    input.heartbeat = HeartbeatStatus {
        configured: true,
        failing_since: Some(now - SignedDuration::from_mins(9)),
    };
    assert!(
        input.run(&[], now).upserts.is_empty(),
        "within the grace period"
    );
    input.heartbeat.failing_since = Some(now - SignedDuration::from_mins(11));
    assert_eq!(
        kinds(&input.run(&[], now)),
        [AdvisoryKind::HeartbeatMissing]
    );
}

#[test]
fn triage_degraded_while_reported() {
    let mut input = Input::new();
    input.triage = Some("The LLM provider failed 5 times in a row");
    let changes = input.run(&[], ts(NOW));
    assert_eq!(kinds(&changes), [AdvisoryKind::TriageDegraded]);
    assert!(changes.upserts[0].detail.contains("Alerts still go out"));
}

#[test]
fn stale_image_over_180_days() {
    let host = HostId::new();
    let mut input = Input::new();
    input.facts = vec![
        fact(
            host,
            "container.sonarr.image_age_days",
            FactValue::Number(200.0),
        ),
        fact(
            host,
            "container.plex.image_age_days",
            FactValue::Number(30.0),
        ),
    ];
    let changes = input.run(&[], ts(NOW));
    assert_eq!(kinds(&changes), [AdvisoryKind::StaleImage]);
    assert_eq!(changes.upserts[0].title, "sonarr image is 200 days old");
    assert_eq!(changes.upserts[0].host, Some(host));
}

#[test]
fn stack_without_source() {
    let host = HostId::new();
    let mut input = Input::new();
    input.facts = vec![
        fact(host, "stack.media.source_missing", FactValue::Number(1.0)),
        fact(host, "stack.tools.source_missing", FactValue::Number(0.0)),
    ];
    let changes = input.run(&[], ts(NOW));
    assert_eq!(kinds(&changes), [AdvisoryKind::StackWithoutSource]);
    assert!(changes.upserts[0].title.contains("media"));
}

#[test]
fn secret_exposed_per_name() {
    let mut input = Input::new();
    input.secrets = vec!["discord.bot_token".to_owned(), "llm.api_key".to_owned()];
    let changes = input.run(&[], ts(NOW));
    assert_eq!(
        kinds(&changes),
        [AdvisoryKind::SecretExposed, AdvisoryKind::SecretExposed]
    );
}

#[test]
fn baseline_entry_expired() {
    let host = HostId::new();
    let mut input = Input::new();
    input.expired = vec![BaselineEntry {
        host,
        kind: BaselineKind::Container,
        key: "jellyfin".to_owned(),
        criticality: Criticality::Warning,
        learned_at: ts("2026-09-01T00:00:00Z"),
        last_seen_at: ts("2026-10-20T00:00:00Z"),
        managed: true,
    }];
    let changes = input.run(&[], ts(NOW));
    assert_eq!(kinds(&changes), [AdvisoryKind::BaselineEntryExpired]);
    assert!(changes.upserts[0].title.contains("jellyfin"));
}

#[test]
fn agent_outdated_when_a_minor_version_behind() {
    let (old, same, newer) = (HostId::new(), HostId::new(), HostId::new());
    let mut input = Input::new();
    input.agents = vec![
        AgentVersion {
            host: old,
            version: "0.3.9".to_owned(),
        },
        AgentVersion {
            host: same,
            version: "v0.4.0".to_owned(),
        },
        AgentVersion {
            host: newer,
            version: "0.5.0".to_owned(),
        },
    ];
    let changes = input.run(&[], ts(NOW));
    assert_eq!(kinds(&changes), [AdvisoryKind::AgentOutdated]);
    assert_eq!(
        changes.upserts[0].host,
        Some(old),
        "patch differences do not count"
    );
}

#[test]
fn advisory_ids_are_stable() {
    let host = HostId::new();
    let a = advisory_id(AdvisoryKind::StaleImage, Some(host), "sonarr");
    assert_eq!(
        a,
        advisory_id(AdvisoryKind::StaleImage, Some(host), "sonarr")
    );
    assert_ne!(
        a,
        advisory_id(AdvisoryKind::StaleImage, Some(host), "radarr")
    );
    assert_ne!(a, advisory_id(AdvisoryKind::StaleImage, None, "sonarr"));
    assert_ne!(
        a,
        advisory_id(AdvisoryKind::StackWithoutSource, Some(host), "sonarr")
    );

    // Evaluating again keeps the open advisory as it is: no duplicate, same created_at.
    let mut input = Input::new();
    input.secrets = vec!["llm.api_key".to_owned()];
    let first = input.run(&[], ts(NOW));
    let open = first.upserts.clone();
    let again = input.run(&open, ts(NOW) + SignedDuration::from_hours(1));
    assert_eq!(again, AdvisoryChanges::default());
}

#[test]
fn cleared_condition_resolves() {
    let mut input = Input::new();
    input.secrets = vec!["llm.api_key".to_owned()];
    let open = input.run(&[], ts(NOW)).upserts;
    input.secrets.clear();
    let changes = input.run(&open, ts(NOW) + SignedDuration::from_hours(1));
    assert!(changes.upserts.is_empty());
    assert_eq!(changes.resolve, [open[0].id]);
}
