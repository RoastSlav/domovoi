//! Rules that need more than one field, or more than a type, to check.

use url::Url;

use crate::agent::AgentConfig;
use crate::hub::{AgentServerSection, HubConfig, LlmKind, LlmProvider, Probe, ProbeKind};
use crate::issue::ConfigIssue;

/// Cross-field validation run after a config deserializes.
pub trait Validate {
    /// Pushes every problem found onto `issues`.
    fn validate(&self, issues: &mut Vec<ConfigIssue>);
}

impl Validate for HubConfig {
    fn validate(&self, issues: &mut Vec<ConfigIssue>) {
        if let Some(url) = &self.web.public_url
            && !matches!(url.scheme(), "http" | "https")
        {
            issues.push(ConfigIssue::error(
                "web.public_url",
                "must be an http or https URL such as \"https://domovoi.example.net\"",
            ));
        }
        if self.triage.enabled && self.llm.primary.is_none() {
            issues.push(ConfigIssue::error(
                "llm.primary",
                "triage is enabled but no LLM provider is configured; add [llm.primary] or set triage.enabled = false",
            ));
        }
        for (slot, provider) in [
            ("primary", &self.llm.primary),
            ("secondary", &self.llm.secondary),
        ] {
            if let Some(provider) = provider {
                provider_rules(&format!("llm.{slot}"), provider, issues);
            }
        }
        if self.triage.concurrency == 0 {
            issues.push(ConfigIssue::error(
                "triage.concurrency",
                "must be at least 1",
            ));
        }
        if self
            .triage
            .monthly_budget_usd
            .is_some_and(|b| !non_negative(b))
        {
            issues.push(ConfigIssue::error(
                "triage.monthly_budget_usd",
                "must be zero or more; remove it for no budget",
            ));
        }
        agent_server_rules(&self.agent_server, issues);
        if self.incidents.confirm_runs == 0 {
            issues.push(ConfigIssue::error(
                "incidents.confirm_runs",
                "must be at least 1",
            ));
        }
        if let Some(quiet) = &self.notify.quiet_hours
            && quiet.start == quiet.end
        {
            issues.push(ConfigIssue::error(
                "notify.quiet_hours",
                "start and end are the same; use different times or remove quiet_hours",
            ));
        }
        if let Some(discord) = &self.discord
            && discord.enabled
            && discord.allowed_user_ids.is_empty()
        {
            issues.push(ConfigIssue::error(
                "discord.allowed_user_ids",
                "must list at least one Discord user ID when Discord is enabled",
            ));
        }
        for (name, hook) in &self.webhooks {
            if hook
                .host_label
                .as_deref()
                .is_some_and(|l| l.is_empty() || l.len() > 128)
            {
                issues.push(ConfigIssue::error(
                    format!("webhooks.{name}.host_label"),
                    "must be a label name of 1 to 128 characters, such as \"host\"",
                ));
            }
            if !valid_webhook_name(name) {
                issues.push(ConfigIssue::error(
                    format!("webhooks.{name}"),
                    "webhook names may use only a-z, 0-9, '-' and '_' (at most 64 characters)",
                ));
            }
        }
        for (i, probe) in self.probes.iter().enumerate() {
            probe_rules(&format!("probes[{i}]"), probe, issues);
        }
        if self.backup.keep == 0 {
            issues.push(ConfigIssue::error("backup.keep", "must be at least 1"));
        }
    }
}

fn provider_rules(path: &str, provider: &LlmProvider, issues: &mut Vec<ConfigIssue>) {
    match provider.kind {
        LlmKind::Cli => match &provider.cli {
            None => issues.push(ConfigIssue::error(
                format!("{path}.cli"),
                "kind = \"cli\" needs a cli block such as cli = { preset = \"claude\" }",
            )),
            Some(cli) => {
                if cli.preset.is_none() && cli.command.is_none() {
                    issues.push(ConfigIssue::error(
                        format!("{path}.cli"),
                        "set preset (\"claude\" or \"codex\") or command",
                    ));
                }
                if cli.command.as_ref().is_some_and(|c| !c.has_root()) {
                    issues.push(ConfigIssue::error(
                        format!("{path}.cli.command"),
                        "must be an absolute path",
                    ));
                }
            }
        },
        LlmKind::Anthropic | LlmKind::OpenaiCompatible => {
            if provider.model.as_deref().is_none_or(str::is_empty) {
                issues.push(ConfigIssue::error(
                    format!("{path}.model"),
                    "this provider kind needs a model name",
                ));
            }
            if provider.kind == LlmKind::OpenaiCompatible && provider.base_url.is_none() {
                issues.push(ConfigIssue::error(
                    format!("{path}.base_url"),
                    "openai_compatible needs the server's base_url",
                ));
            }
            if provider.kind == LlmKind::Anthropic && provider.api_key.is_none() {
                issues.push(ConfigIssue::error(
                    format!("{path}.api_key"),
                    "anthropic needs an api_key such as { env = \"ANTHROPIC_API_KEY\" }",
                ));
            }
        }
    }
    if let Some(price) = &provider.price
        && !(non_negative(price.input_per_mtok_usd) && non_negative(price.output_per_mtok_usd))
    {
        issues.push(ConfigIssue::error(
            format!("{path}.price"),
            "prices must be zero or more",
        ));
    }
}

fn probe_rules(path: &str, probe: &Probe, issues: &mut Vec<ConfigIssue>) {
    match probe.kind {
        ProbeKind::Http => {
            let ok = Url::parse(&probe.target)
                .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some());
            if !ok {
                issues.push(ConfigIssue::error(
                    format!("{path}.target"),
                    "an http probe needs an http or https URL such as \"https://example.net/health\"",
                ));
            }
            if probe.expect_status.is_none() {
                issues.push(ConfigIssue::error(
                    format!("{path}.expect_status"),
                    "an http probe needs the expected status, such as 200",
                ));
            }
        }
        ProbeKind::Tcp => {
            if !valid_host_port(&probe.target) {
                issues.push(ConfigIssue::error(
                    format!("{path}.target"),
                    "a tcp probe needs host:port such as \"db.lan:5432\"",
                ));
            }
            if probe.expect_status.is_some() {
                issues.push(ConfigIssue::error(
                    format!("{path}.expect_status"),
                    "only http probes have a status; remove it",
                ));
            }
        }
    }
}

fn non_negative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn agent_server_rules(server: &AgentServerSection, issues: &mut Vec<ConfigIssue>) {
    if let Some(url) = &server.public_url
        && (url.scheme() != "wss" || url.host().is_none())
    {
        issues.push(ConfigIssue::error(
            "agent_server.public_url",
            "must be a wss:// URL such as \"wss://hub.lan:7421\"",
        ));
    }
    if !valid_host_port(&server.self_check) {
        issues.push(ConfigIssue::error(
            "agent_server.self_check",
            "needs host:port outside your network, such as \"1.1.1.1:53\"",
        ));
    }
}

fn valid_host_port(target: &str) -> bool {
    let Some((host, port)) = target.rsplit_once(':') else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    !host.is_empty() && !host.contains('/') && port.parse::<u16>().is_ok_and(|p| p > 0)
}

fn valid_webhook_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl Validate for AgentConfig {
    fn validate(&self, issues: &mut Vec<ConfigIssue>) {
        if self.agent.hub_url.scheme() != "wss" {
            issues.push(ConfigIssue::error(
                "agent.hub_url",
                "must use wss://, such as \"wss://hub.example.net:7421\"",
            ));
        }
        let endpoint_ok =
            Url::parse(&self.docker.endpoint).is_ok_and(|u| matches!(u.scheme(), "unix" | "tcp"));
        if !endpoint_ok {
            issues.push(ConfigIssue::error(
                "docker.endpoint",
                "must be unix:///path/to/docker.sock or tcp://host:port",
            ));
        }
        if let Some(relay) = &self.local_webhooks
            && !relay.bind.ip().is_loopback()
            && !relay.allow_non_loopback
        {
            issues.push(ConfigIssue::error(
                "local_webhooks.bind",
                "must be a loopback address such as 127.0.0.1:7422; set allow_non_loopback = true to open it to the network",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_port_accepts_names_and_ipv6() {
        assert!(valid_host_port("db.lan:5432"));
        assert!(valid_host_port("[::1]:22"));
        assert!(!valid_host_port("db.lan"));
        assert!(!valid_host_port("db.lan:0"));
        assert!(!valid_host_port(":80"));
    }
}
