use std::net::SocketAddr;

use domovoi_config::{Duration, LocalTime, MapEnv, TimeZone};
use jiff::SignedDuration;

use crate::helpers::{AGENT, agent, assert_issue, hub, hub_errors, hub_with};

#[test]
fn empty_hub_config_uses_defaults() {
    let (config, warnings) = hub("").unwrap();
    assert!(warnings.is_empty());
    assert_eq!(
        config.web.bind,
        "127.0.0.1:7420".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        config.agent_server.bind,
        "0.0.0.0:7421".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.triage.concurrency, 2);
    assert_eq!(
        config.incidents.reopen_window.as_signed(),
        SignedDuration::from_hours(6)
    );
    assert_eq!(config.hub.timezone.name(), "UTC");
    let (agent, _) = agent(AGENT).unwrap();
    assert_eq!(
        agent.containers.baseline_expiry.as_signed(),
        SignedDuration::from_hours(7 * 24)
    );
}

#[test]
fn unknown_key_is_rejected_with_path() {
    let errors = hub_errors("[web]\nbnd = \"127.0.0.1:1\"\n");
    assert_issue(
        &errors,
        "web.bnd",
        "unknown key; remove it or check the spelling",
    );

    let errors = hub_errors("[notify.digest]\nenabled = true\ntiem = \"09:00\"\n");
    assert_issue(&errors, "notify.digest.tiem", "unknown key");

    let errors = hub_errors("[nope]\n");
    assert_issue(&errors, "nope", "unknown key");
}

#[test]
fn wrong_type_names_path() {
    let errors = hub_errors("[triage]\nconcurrency = \"two\"\n");
    assert_issue(&errors, "triage.concurrency", "invalid type");
}

#[test]
fn invalid_toml_is_reported() {
    let errors = hub_errors("[web\n");
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.starts_with("invalid TOML"));
}

#[test]
fn env_override_sets_nested_scalar() {
    let env = MapEnv::new()
        .with("DOMOVOI_WEB__BIND", "0.0.0.0:7420")
        .with("DOMOVOI_NOTIFY__DIGEST__ENABLED", "false")
        .with("DOMOVOI_TRIAGE__CONCURRENCY", "4");
    let (config, _) = hub_with("[web]\nbind = \"127.0.0.1:7420\"\n", &env).unwrap();
    assert_eq!(
        config.web.bind,
        "0.0.0.0:7420".parse::<SocketAddr>().unwrap()
    );
    assert!(!config.notify.digest.enabled);
    assert_eq!(config.triage.concurrency, 4);
}

#[test]
fn env_override_rejects_table_target() {
    let env = MapEnv::new().with("DOMOVOI_NOTIFY__RENOTIFY", "1h");
    let errors = hub_with("[notify]\nrenotify = { warning = \"1h\" }\n", &env).unwrap_err();
    assert_issue(&errors, "notify.renotify", "only scalar values");
    assert_issue(&errors, "notify.renotify", "DOMOVOI_NOTIFY__RENOTIFY");
}

#[test]
fn env_override_of_unknown_key_is_rejected() {
    let env = MapEnv::new().with("DOMOVOI_WEB__BINDD", "x");
    let errors = hub_with("", &env).unwrap_err();
    assert_issue(&errors, "web.bindd", "unknown key");
}

#[test]
fn durations_parse_friendly_strings_and_reject_negative() {
    for (text, secs) in [
        ("30s", 30),
        ("5m", 300),
        ("24h", 86_400),
        ("1h30m", 5_400),
        ("7d", 604_800),
    ] {
        let d: Duration = text.parse().unwrap();
        assert_eq!(d.as_signed(), SignedDuration::from_secs(secs), "{text}");
    }
    for bad in ["-5m", "0s", "", "soon", "5 parsecs"] {
        let err = bad.parse::<Duration>().unwrap_err();
        assert!(
            err.to_string().contains("positive duration"),
            "{bad}: {err}"
        );
    }
    let errors = hub_errors("[incidents]\nreopen_window = \"-1h\"\n");
    assert_issue(&errors, "incidents.reopen_window", "positive duration");
}

#[test]
fn local_time_and_timezone_validate() {
    assert_eq!("09:00".parse::<LocalTime>().unwrap().to_string(), "09:00");
    for bad in ["9:00", "24:00", "12:60", "noon"] {
        assert!(bad.parse::<LocalTime>().is_err(), "{bad}");
    }
    assert_eq!(
        "Europe/Sofia".parse::<TimeZone>().unwrap().name(),
        "Europe/Sofia"
    );
    assert!("Mars/Olympus".parse::<TimeZone>().is_err());
    let errors = hub_errors("[hub]\ntimezone = \"Mars/Olympus\"\n");
    assert_issue(&errors, "hub.timezone", "IANA time zone");
}
