//! Shared helpers for the config tests.

use domovoi_config::{AgentConfig, ConfigIssue, HubConfig, MapEnv, parse};

/// The smallest valid agent config.
pub const AGENT: &str = "[agent]\nhost_name = \"nas\"\nhub_url = \"wss://hub.lan:7421\"\n";

pub fn hub(text: &str) -> Result<(HubConfig, Vec<ConfigIssue>), Vec<ConfigIssue>> {
    parse(text, &MapEnv::new())
}

pub fn hub_with(
    text: &str,
    env: &MapEnv,
) -> Result<(HubConfig, Vec<ConfigIssue>), Vec<ConfigIssue>> {
    parse(text, env)
}

pub fn agent(text: &str) -> Result<(AgentConfig, Vec<ConfigIssue>), Vec<ConfigIssue>> {
    parse(text, &MapEnv::new())
}

/// The issues from a hub config that should fail to load; empty if it loaded.
pub fn hub_errors(text: &str) -> Vec<ConfigIssue> {
    hub(text).err().unwrap_or_default()
}

/// The issues from an agent config that should fail to load; empty if it loaded.
pub fn agent_errors(text: &str) -> Vec<ConfigIssue> {
    agent(text).err().unwrap_or_default()
}

/// Asserts that `issues` has an error at `path` whose message contains `needle`.
pub fn assert_issue(issues: &[ConfigIssue], path: &str, needle: &str) {
    assert!(
        issues
            .iter()
            .any(|i| i.is_error() && i.path == path && i.message.contains(needle)),
        "no error at {path:?} containing {needle:?} in {issues:#?}"
    );
}
