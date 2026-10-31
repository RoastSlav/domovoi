//! The daily digest: quiet hosts collapse, changes show deltas, free space projects.

use domovoi_core::{
    Advisory, AdvisoryId, AdvisoryKind, DigestFact, FactValue, Host, HostId, HostKind, HostName,
    Severity,
};
use domovoi_engine::digest::{DigestInputs, DigestService, build_digest, projection};
use domovoi_engine::render;
use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

use crate::harness::{open, sofia, ts};

fn host(name: &str, n: u128) -> Host {
    Host {
        id: HostId::from_uuid(Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_3000 + n,
        )),
        name: HostName::parse(name).unwrap(),
        kind: HostKind::Remote,
        labels: Default::default(),
        created_at: ts("2026-01-01T00:00:00Z"),
    }
}

fn fact(host: &Host, key: &str, label: &str, value: f64, unit: &str, at: Timestamp) -> DigestFact {
    DigestFact {
        host: host.id,
        key: key.to_owned(),
        label: label.to_owned(),
        value: FactValue::Number(value),
        unit: Some(unit.to_owned()),
        observed_at: at,
    }
}

const GB: f64 = 1e9;

/// Pool free space falling `per_day` GB a day for 14 days, ending at `free` GB today.
fn shrinking(
    host: &Host,
    free: f64,
    per_day: f64,
    today: Timestamp,
) -> (Vec<DigestFact>, Vec<DigestFact>) {
    let mut history = Vec::new();
    for day in (1..=14).rev() {
        let at = today - SignedDuration::from_hours(24 * day);
        let value = (free + per_day * day as f64) * GB;
        history.push(fact(
            host,
            "zfs.tank.free_bytes",
            "Pool free",
            value,
            "bytes",
            at,
        ));
    }
    let today = vec![fact(
        host,
        "zfs.tank.free_bytes",
        "Pool free",
        free * GB,
        "bytes",
        today,
    )];
    (today, history)
}

fn digest(
    hosts: &[Host],
    today: &[DigestFact],
    history: &[DigestFact],
    incidents: &[domovoi_core::Incident],
    advisories: &[Advisory],
    now: Timestamp,
) -> domovoi_core::Digest {
    let tz = TimeZone::UTC;
    build_digest(&DigestInputs {
        date: now.to_zoned(tz.clone()).date(),
        hosts,
        facts_today: today,
        facts_history: history,
        open_incidents: incidents,
        advisories,
        tz: &tz,
        now,
    })
}

#[test]
fn quiet_host_collapses_to_one_line() {
    let now = ts("2026-10-31T09:00:00Z");
    let pi = host("pi", 1);
    let today = vec![fact(&pi, "disk.root.used_pct", "Root used", 41.0, "%", now)];
    let history = vec![fact(
        &pi,
        "disk.root.used_pct",
        "Root used",
        41.2,
        "%",
        now - SignedDuration::from_hours(24),
    )];
    let digest = digest(&[pi], &today, &history, &[], &[], now);
    assert!(digest.hosts[0].quiet, "a 0.2-point wobble is not a change");
    let text = render::digest(&digest);
    let pi_lines: Vec<&str> = text.lines().filter(|l| l.contains("pi")).collect();
    assert_eq!(pi_lines, ["pi: healthy, nothing changed"]);
}

#[test]
fn changed_fact_shows_delta() {
    let now = ts("2026-10-31T09:00:00Z");
    let nas = host("nas", 2);
    let (today, history) = shrinking(&nas, 480.0, 41.0 / 7.0, now);
    let digest = digest(&[nas], &today, &history, &[], &[], now);
    let nas = &digest.hosts[0];
    assert!(!nas.quiet);
    assert!(
        nas.lines[0].starts_with("Pool free 480 GB, down 41 GB in 7d"),
        "{}",
        nas.lines[0]
    );
    assert!((nas.trends[0].delta + 41.0 * GB).abs() < 1e6);
}

#[test]
fn declining_free_space_projects_time_to_full() {
    let now = ts("2026-10-31T09:00:00Z");
    let nas = host("nas", 2);
    // 480 GB left, losing ~5.9 GB a day: about 82 days, ~12 weeks.
    let (today, history) = shrinking(&nas, 480.0, 41.0 / 7.0, now);
    let digest = digest(&[nas], &today, &history, &[], &[], now);
    let line = &digest.hosts[0].lines[0];
    assert!(line.ends_with("full in ~12 weeks at this rate"), "{line}");
    assert_eq!(
        digest.hosts[0].trends[0].projection.as_deref(),
        Some("full in ~12 weeks at this rate")
    );
}

#[test]
fn projection_hidden_beyond_180_days() {
    let now = ts("2026-10-31T09:00:00Z");
    let nas = host("nas", 2);
    // 2 TB left, losing 5 GB a day: 400 days.
    let (today, history) = shrinking(&nas, 2000.0, 5.0, now);
    let points: Vec<(Timestamp, f64)> = history
        .iter()
        .chain(&today)
        .map(|f| {
            (
                f.observed_at,
                match f.value {
                    FactValue::Number(v) => v,
                    FactValue::Text(_) => 0.0,
                },
            )
        })
        .collect();
    assert_eq!(projection(&points, 0.0, now), None);
    let digest = digest(&[nas], &today, &history, &[], &[], now);
    // 0.25 % a day and no projection: nothing worth a line.
    assert!(digest.hosts[0].quiet, "{:?}", digest.hosts[0].lines);
    // A slow decline that does run out within 180 days is shown even though each day's
    // change is under the tolerance: 600 GB at 5 GB a day is 120 days.
    let (today, history) = shrinking(&host("nas", 2), 600.0, 5.0, now);
    let slow = self::digest(&[host("nas", 2)], &today, &history, &[], &[], now);
    assert!(
        slow.hosts[0].lines[0].contains("full in ~17 weeks"),
        "{:?}",
        slow.hosts[0].lines
    );
    // Rising free space never projects.
    let (today, history) = shrinking(&host("nas", 2), 500.0, -10.0, now);
    let digest = self::digest(&[host("nas", 2)], &today, &history, &[], &[], now);
    assert!(digest.hosts[0].lines[0].contains(", up "));
    assert!(digest.hosts[0].trends[0].projection.is_none());
}

#[test]
fn digest_lists_open_incidents_and_advisories() {
    let now = ts("2026-10-31T09:00:00Z");
    let nas = host("nas", 2);
    let mut incident = open(Severity::Warning, now - SignedDuration::from_hours(5));
    incident = serde_json::from_value({
        let mut v = serde_json::to_value(&incident).unwrap();
        v["host"] = serde_json::to_value(nas.id).unwrap();
        v
    })
    .unwrap();
    let advisory = Advisory {
        id: AdvisoryId::new(),
        kind: AdvisoryKind::StaleImage,
        host: Some(nas.id),
        title: "sonarr image is 200 days old".to_owned(),
        detail: "Pull a newer image.".to_owned(),
        created_at: now - SignedDuration::from_hours(48),
        resolved_at: None,
    };
    let digest = digest(
        &[nas],
        &[],
        &[],
        &[incident],
        std::slice::from_ref(&advisory),
        now,
    );
    assert!(!digest.hosts[0].quiet, "an open incident is not quiet");
    assert_eq!(
        digest.open_incidents[0].1,
        "nas: plex restarted 7 times in 10 minutes (open 5h, warning)"
    );
    assert_eq!(digest.advisories, [advisory]);
    let text = render::digest(&digest);
    assert!(text.contains("**Open incidents**") && text.contains("**Advisories**"));
}

#[test]
fn digest_due_once_per_day_after_local_time() {
    let service = DigestService {
        enabled: true,
        time: Time::constant(9, 0, 0, 0),
        tz: sofia(),
    };
    // 09:00 EET is 07:00 UTC in winter.
    assert!(!service.due(ts("2026-11-02T06:59:00Z"), None));
    assert!(service.due(ts("2026-11-02T07:00:00Z"), None));
    let sent = ts("2026-11-02T07:00:30Z");
    assert!(
        !service.due(ts("2026-11-02T07:01:00Z"), Some(sent)),
        "once per day"
    );
    assert!(
        !service.due(ts("2026-11-02T21:59:00Z"), Some(sent)),
        "23:59 local, same day"
    );
    assert!(
        !service.due(ts("2026-11-02T22:30:00Z"), Some(sent)),
        "00:30 local, before 09:00"
    );
    assert!(
        service.due(ts("2026-11-03T07:00:00Z"), Some(sent)),
        "the next day"
    );
    // In summer 09:00 EEST is 06:00 UTC.
    assert!(service.due(ts("2026-07-01T06:00:00Z"), None));
    let off = DigestService {
        enabled: false,
        ..service
    };
    assert!(!off.due(ts("2026-11-03T07:00:00Z"), None));
}

#[test]
fn two_host_digest_rendering() {
    let now = ts("2026-10-31T09:00:00Z");
    let (nas, pi) = (host("nas", 2), host("pi", 1));
    let (mut today, mut history) = shrinking(&nas, 480.0, 41.0 / 7.0, now);
    today.push(fact(&pi, "disk.root.used_pct", "Root used", 41.0, "%", now));
    history.push(fact(
        &pi,
        "disk.root.used_pct",
        "Root used",
        41.0,
        "%",
        now - SignedDuration::from_hours(24),
    ));
    let digest = build_digest(&DigestInputs {
        date: Date::constant(2026, 10, 31),
        hosts: &[pi, nas],
        facts_today: &today,
        facts_history: &history,
        open_incidents: &[],
        advisories: &[],
        tz: &TimeZone::UTC,
        now,
    });
    insta::assert_snapshot!(render::digest(&digest));
}
