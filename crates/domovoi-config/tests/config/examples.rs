use std::path::PathBuf;

use domovoi_config::{MapEnv, load_agent, load_hub};

fn example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

#[test]
fn examples_parse_and_validate() {
    let env = MapEnv::new()
        .with("ANTHROPIC_API_KEY", "sk-test")
        .with("DISCORD_BOT_TOKEN", "bot-test")
        .with("DISCORD_WEBHOOK_URL", "https://discord.test/hook")
        .with("DOMOVOI_ALERTS_TOKEN", "alerts-test")
        .with("DOMOVOI_HEARTBEAT_URL", "https://hc.test/ping")
        .with("DOMOVOI_LOCAL_WEBHOOK_TOKEN", "local-test");

    let (hub, warnings) = load_hub(&example("hub.full.toml"), &env).unwrap();
    assert!(warnings.is_empty(), "{warnings:#?}");
    assert!(hub.triage.enabled);
    assert_eq!(hub.probes.len(), 2);
    assert_eq!(hub.webhooks.len(), 1);

    let (short, warnings) = load_hub(&example("hub.toml"), &MapEnv::new()).unwrap();
    assert!(warnings.is_empty(), "{warnings:#?}");
    assert!(
        short.discord.is_none(),
        "the short example only shows commented options"
    );

    let (agent, warnings) = load_agent(&example("agent.toml"), &env).unwrap();
    assert!(warnings.is_empty(), "{warnings:#?}");
    assert_eq!(agent.agent.host_name.as_str(), "nas");
    assert_eq!(agent.checks.len(), 1);
}

#[test]
fn missing_file_is_reported() {
    let errors = load_hub(&example("missing.toml"), &MapEnv::new()).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("cannot read"));
}
