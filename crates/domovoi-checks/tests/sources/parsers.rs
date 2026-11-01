use domovoi_checks::sources::{
    MAX_BODY_BYTES, MAX_EVIDENCE_KEYS, MAX_SIGNALS, ParseError, ParseOptions, SourceFormat,
    SourceSignal, parse,
};
use domovoi_core::{EvidenceValue, HostId, Severity, SignalStatus, SourceKind};
use jiff::Timestamp;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/webhooks/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

fn read(format: SourceFormat, name: &str) -> Vec<SourceSignal> {
    parse(format, &fixture(name), ParseOptions::default()).unwrap()
}

fn text(signal: &SourceSignal, key: &str) -> Option<String> {
    match signal.evidence.get(key) {
        Some(EvidenceValue::Text(t)) => Some(t.clone()),
        _ => None,
    }
}

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

#[test]
fn generic_single_and_array_parse() {
    let one = read(SourceFormat::Generic, "generic_single.json");
    assert_eq!(one.len(), 1);
    let s = &one[0];
    assert_eq!(s.host.as_deref(), Some("nas"));
    assert_eq!(s.check.as_str(), "backup.nightly");
    assert_eq!(s.target.as_ref().map(|t| t.as_str()), Some("photos"));
    assert_eq!(s.status, SignalStatus::Firing);
    assert_eq!(s.severity, Severity::Warning);
    assert_eq!(s.summary, "Backup failed");
    assert_eq!(s.evidence.get("exit_code"), Some(&EvidenceValue::Int(2)));
    assert_eq!(s.evidence.get("retried"), Some(&EvidenceValue::Bool(false)));

    let many = read(SourceFormat::Generic, "generic_array.json");
    assert_eq!(many.len(), 2);
    assert_eq!(many[0].status, SignalStatus::Resolved);
    assert_eq!(many[1].host, None, "the receiver fills in its default host");
    assert_eq!(many[1].severity, Severity::Critical);
    assert_eq!(many[1].observed_at, Some(at("2026-10-31T11:30:00Z")));

    let signal = many[1].clone().into_signal(
        HostId::new(),
        SourceKind::Webhook {
            kind: "generic".to_owned(),
        },
        at("2026-10-31T12:00:00Z"),
    );
    assert_eq!(
        signal.source(),
        &SourceKind::Webhook {
            kind: "generic".to_owned()
        }
    );
    assert_eq!(signal.observed_at(), at("2026-10-31T11:30:00Z"));
}

#[test]
fn generic_rejects_bad_check_and_severity() {
    let bad = |body: &str| {
        parse(
            SourceFormat::Generic,
            body.as_bytes(),
            ParseOptions::default(),
        )
    };
    assert!(matches!(
        bad(r#"{"check": "Backup Nightly", "summary": "x"}"#),
        Err(ParseError::Invalid { field, .. }) if field == "check"
    ));
    assert!(matches!(
        bad(r#"[{"check": "a.b", "summary": "x"}, {"check": "a.b", "summary": "x", "severity": "loud"}]"#),
        Err(ParseError::Invalid { field, .. }) if field == "[1].severity"
    ));
    assert!(matches!(
        bad(r#"{"check": "a.b", "summary": "x", "evidence": {"nested": {"a": 1}}}"#),
        Err(ParseError::Invalid { field, .. }) if field == "evidence.nested"
    ));
    assert!(matches!(
        bad(r#"{"check": "a.b", "summary": "x", "colour": "red"}"#),
        Err(ParseError::Malformed(_))
    ));
    assert!(matches!(bad("not json"), Err(ParseError::Malformed(_))));
}

#[test]
fn alertmanager_one_signal_per_alert() {
    let signals = read(SourceFormat::Alertmanager, "alertmanager_group.json");
    assert_eq!(signals.len(), 2);
    let firing = &signals[0];
    assert_eq!(firing.check.as_str(), "alertmanager.diskfilling");
    assert_eq!(firing.status, SignalStatus::Firing);
    assert_eq!(firing.severity, Severity::Warning);
    assert_eq!(firing.summary, "/srv on nas is 92% full");
    assert_eq!(firing.observed_at, Some(at("2026-10-31T11:52:00Z")));
    assert_eq!(text(firing, "mountpoint").as_deref(), Some("/srv"));

    let resolved = &signals[1];
    assert_eq!(resolved.status, SignalStatus::Resolved);
    assert_eq!(resolved.severity, Severity::Critical);
    assert_eq!(
        resolved.summary,
        "Filesystem / has less than 3% space left."
    );
    assert_eq!(resolved.observed_at, Some(at("2026-10-31T11:40:00Z")));
}

#[test]
fn volatile_labels_excluded_from_targets() {
    let signals = read(SourceFormat::Alertmanager, "alertmanager_group.json");
    let firing = &signals[0];
    assert_eq!(
        firing.target.as_ref().map(|t| t.as_str()),
        Some("nas.lan:9100"),
        "the target is the stable instance"
    );
    assert_eq!(text(firing, "pod_template_hash"), None);
    assert_eq!(text(firing, "fingerprint"), None);
    assert!(firing.evidence.iter().all(|(k, _)| k != "generatorURL"));
    assert_eq!(text(firing, "alertname").as_deref(), Some("DiskFilling"));
}

#[test]
fn alertmanager_host_from_label_or_instance() {
    let signals = read(SourceFormat::Alertmanager, "alertmanager_group.json");
    assert_eq!(
        signals[0].host.as_deref(),
        Some("nas"),
        "instance without port or domain"
    );
    assert_eq!(signals[1].host.as_deref(), Some("media"));

    let body = br#"{"alerts": [{"status": "firing", "labels": {"alertname": "Up", "node": "Backup-1", "instance": "10.0.0.5:9100"}}]}"#;
    let by_label = parse(
        SourceFormat::Alertmanager,
        body,
        ParseOptions {
            host_label: Some("node"),
        },
    )
    .unwrap();
    assert_eq!(by_label[0].host.as_deref(), Some("backup-1"));
    let by_ip = parse(SourceFormat::Alertmanager, body, ParseOptions::default()).unwrap();
    assert_eq!(by_ip[0].host, None, "an IP address names no known host");
    assert_eq!(by_ip[0].severity, Severity::Info);
    assert_eq!(by_ip[0].summary, "Up");
}

#[test]
fn uptime_kuma_down_and_up() {
    let down = read(SourceFormat::UptimeKuma, "uptime_kuma_down.json");
    assert_eq!(down.len(), 1);
    assert_eq!(down[0].check.as_str(), "uptime_kuma.monitor");
    assert_eq!(down[0].target.as_ref().map(|t| t.as_str()), Some("Plex"));
    assert_eq!(down[0].status, SignalStatus::Firing);
    assert_eq!(
        down[0].summary,
        "Plex: connect ECONNREFUSED 192.168.1.20:32400"
    );
    assert_eq!(
        text(&down[0], "url").as_deref(),
        Some("http://192.168.1.20:32400/web")
    );
    assert_eq!(text(&down[0], "type").as_deref(), Some("http"));

    let up = read(SourceFormat::UptimeKuma, "uptime_kuma_up.json");
    assert_eq!(up[0].status, SignalStatus::Resolved);
    assert_eq!(up[0].target, down[0].target, "down and up pair up");
}

#[test]
fn uptime_kuma_pending_is_ignored() {
    let pending =
        br#"{"heartbeat": {"status": 2, "msg": "retrying"}, "monitor": {"name": "Plex"}}"#;
    assert!(
        parse(SourceFormat::UptimeKuma, pending, ParseOptions::default())
            .unwrap()
            .is_empty()
    );
    let test = br#"{"heartbeat": null, "monitor": null, "msg": "Uptime Kuma test notification"}"#;
    assert!(
        parse(SourceFormat::UptimeKuma, test, ParseOptions::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn oversized_payload_is_rejected() {
    let body = vec![b' '; MAX_BODY_BYTES + 1];
    for format in SourceFormat::ALL {
        assert_eq!(
            parse(format, &body, ParseOptions::default()),
            Err(ParseError::TooLarge(MAX_BODY_BYTES + 1))
        );
    }
}

#[test]
fn too_many_signals_are_rejected() {
    let one = r#"{"check": "a.b", "summary": "x"}"#;
    let body = format!("[{}]", vec![one; MAX_SIGNALS + 1].join(","));
    assert_eq!(
        parse(
            SourceFormat::Generic,
            body.as_bytes(),
            ParseOptions::default()
        ),
        Err(ParseError::TooMany(MAX_SIGNALS + 1))
    );
    let alert = r#"{"status": "firing", "labels": {"alertname": "A"}}"#;
    let body = format!(
        r#"{{"alerts": [{}]}}"#,
        vec![alert; MAX_SIGNALS + 1].join(",")
    );
    assert_eq!(
        parse(
            SourceFormat::Alertmanager,
            body.as_bytes(),
            ParseOptions::default()
        ),
        Err(ParseError::TooMany(MAX_SIGNALS + 1))
    );
    let body = format!("[{}]", vec![one; MAX_SIGNALS].join(","));
    assert!(
        parse(
            SourceFormat::Generic,
            body.as_bytes(),
            ParseOptions::default()
        )
        .is_ok()
    );
}

#[test]
fn long_strings_are_clipped() {
    let long = "x".repeat(5000);
    let mut labels: Vec<String> = (0..MAX_EVIDENCE_KEYS + 10)
        .map(|i| format!(r#""label{i:02}": "v""#))
        .collect();
    labels.push(format!(r#""alertname": "{long}""#));
    labels.push(format!(r#""instance": "{long}""#));
    let body = format!(
        r#"{{"alerts": [{{"status": "firing", "labels": {{{}}}, "annotations": {{"summary": "{long}"}}}}]}}"#,
        labels.join(",")
    );
    let signals = parse(
        SourceFormat::Alertmanager,
        body.as_bytes(),
        ParseOptions::default(),
    )
    .unwrap();
    let s = &signals[0];
    assert!(s.check.as_str().len() <= 64);
    assert_eq!(s.summary.chars().count(), 300);
    assert!(s.summary.ends_with('…'));
    assert_eq!(
        s.target.as_ref().map(|t| t.as_str().chars().count()),
        Some(128)
    );
    assert_eq!(s.evidence.iter().count(), MAX_EVIDENCE_KEYS);
    assert!(s.evidence.iter().all(|(_, v)| match v {
        EvidenceValue::Text(t) => t.chars().count() <= 1024,
        _ => true,
    }));
}
