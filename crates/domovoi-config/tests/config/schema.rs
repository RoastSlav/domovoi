use domovoi_config::{AgentConfig, HubConfig};

#[test]
fn hub_schema_is_stable() {
    insta::assert_json_snapshot!("hub_schema", HubConfig::json_schema());
}

#[test]
fn agent_schema_is_stable() {
    insta::assert_json_snapshot!("agent_schema", AgentConfig::json_schema());
}
