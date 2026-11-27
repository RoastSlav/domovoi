use std::net::SocketAddr;

use domovoi_config::{AgentConfig, Duration, LocalTime, MapEnv, TimeZone, parse};
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
fn retention_keys_load_from_file_and_environment() {
    let (config, _) = hub_with(
        "[retention]\nsignals_days = 30\nconversations_days = 200\n",
        &MapEnv::new().with("DOMOVOI_RETENTION__SIGNALS_DAYS", "14"),
    )
    .unwrap();
    assert_eq!(config.retention.signals_days, 14, "environment over file");
    assert_eq!(config.retention.conversations_days, 200);
    assert_eq!(config.retention.notifications_days, 30, "default");
    assert_eq!(config.backup.keep_pre_migrate, 3, "default");
}

#[test]
fn unknown_retention_key_is_an_error() {
    let errors = hub_errors("[retention]\nsignal_days = 3\n");
    assert_issue(&errors, "retention.signal_days", "unknown key");
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

#[test]
fn discord_ids_load_from_numbers_or_strings() {
    let text = "[discord]\nbot_token = { env = \"BOT\" }\nchannel_id = \"1234567890123456789\"\nallowed_user_ids = [234567890123456789, \"345678901234567890\"]\n";
    let env = domovoi_config::MapEnv::new().with("BOT", "token");
    let (config, _) = crate::helpers::hub_with(text, &env).unwrap();
    let discord = config.discord.unwrap();
    assert_eq!(discord.channel_id, 1_234_567_890_123_456_789);
    assert_eq!(
        discord.allowed_user_ids,
        [234_567_890_123_456_789, 345_678_901_234_567_890]
    );
    let bad = crate::helpers::hub_errors(
        "[discord]\nbot_token = { env = \"BOT\" }\nchannel_id = \"x1\"\n",
    );
    assert!(
        bad.iter().any(|i| i.path == "discord.channel_id"),
        "{bad:#?}"
    );
}

#[test]
fn orchestration_is_enabled_by_default() {
    let (config, warnings) = hub("").unwrap();
    assert!(warnings.is_empty());
    let section = &config.orchestration;
    assert!(section.enabled);
    assert!(domovoi_config::hub::OrchestrationSection::default().enabled);
    assert_eq!(section.max_running, 4);
    assert_eq!(section.loop_guard_per_hour, 3);
    assert_eq!(
        section.approval_ttl.as_signed(),
        SignedDuration::from_hours(24)
    );
    assert_eq!(
        section.agent_wait_limit.as_signed(),
        SignedDuration::from_mins(30)
    );
    let budgets = &section.budgets;
    assert_eq!(budgets.model_turns, Some(20));
    assert_eq!(budgets.evidence_requests, Some(40));
    assert_eq!(budgets.executions, Some(5));
    assert_eq!(budgets.cycles, Some(3));
    assert_eq!(budgets.tokens, Some(200_000));
    assert_eq!(budgets.cost_micros, None);
    assert_eq!(
        budgets.wall_time.map(Duration::as_signed),
        Some(SignedDuration::from_hours(72))
    );
    let (config, _) = hub_with(
        "",
        &MapEnv::new().with("DOMOVOI_ORCHESTRATION__ENABLED", "false"),
    )
    .unwrap();
    assert!(!config.orchestration.enabled);
    let (config, _) = hub("[orchestration]
enabled = false
")
    .unwrap();
    assert!(!config.orchestration.enabled);
}

#[test]
fn hub_reads_policy_shadow_from_the_environment() {
    let (config, _) = hub_with("", &MapEnv::new().with("DOMOVOI_POLICY__SHADOW", "true")).unwrap();
    assert!(config.policy.shadow);
    assert!(!hub("").unwrap().0.policy.shadow);
}

#[test]
fn hub_ignores_the_agents_policy_allow_risk_variable() {
    let env = MapEnv::new().with("DOMOVOI_POLICY__ALLOW_RISK", "read");
    let (config, warnings) = hub_with("", &env).unwrap();
    assert!(warnings.is_empty());
    assert!(!config.policy.shadow);
}

#[test]
fn agent_ignores_the_hubs_policy_shadow_variable() {
    let env = MapEnv::new().with("DOMOVOI_POLICY__SHADOW", "true");
    let (config, warnings) = parse::<AgentConfig>(AGENT, &env).unwrap();
    assert!(warnings.is_empty());
    let (plain, _) = parse::<AgentConfig>(AGENT, &MapEnv::new()).unwrap();
    assert_eq!(config.policy, plain.policy);
}

#[test]
fn agent_still_reads_policy_allow_risk_from_the_environment() {
    // A list cannot be set from the environment, so the variable is read and rejected at
    // its path instead of being skipped as the hub's (which would load cleanly).
    let env = MapEnv::new().with("DOMOVOI_POLICY__ALLOW_RISK", "read");
    let errors = parse::<AgentConfig>(AGENT, &env).unwrap_err();
    assert_issue(&errors, "policy.allow_risk", "expected a sequence");
}

/// `DOMOVOI_AGENT__IN_CONTAINER` is documented (container-agent.md, reference/agent.md) and the
/// agent reads it itself; the config loader must not reject it as an unknown key.
#[test]
fn the_documented_in_container_variable_is_accepted_by_the_agent_loader() {
    use domovoi_config::{AgentConfig, MapEnv, parse};
    let text = "[agent]\nhost_name = \"kuker\"\nhub_url = \"wss://hub.example.net:7421\"\n";
    for value in ["true", "false"] {
        let env = MapEnv::new().with("DOMOVOI_AGENT__IN_CONTAINER", value);
        let (parsed, _): (AgentConfig, _) = parse(text, &env).unwrap_or_else(|issues| {
            panic!("{value}: {issues:?}");
        });
        assert_eq!(parsed.agent.in_container, Some(value == "true"));
    }
    let (none, _): (AgentConfig, _) = parse(text, &MapEnv::new()).unwrap();
    assert_eq!(none.agent.in_container, None);
    let (in_file, _): (AgentConfig, _) =
        parse(&format!("{text}in_container = true\n"), &MapEnv::new()).unwrap();
    assert_eq!(in_file.agent.in_container, Some(true));
}
