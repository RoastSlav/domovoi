//! Rules that need more than one field, or more than a type, to check.

use url::Url;

use crate::agent::AgentConfig;
use crate::hub::{
    AgentServerSection, HubConfig, HubPolicySection, LlmKind, LlmProvider, OrchestrationSection,
    Probe, ProbeKind,
};
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
        if self.triage.enabled && self.triage.per_hour_runs == 0 {
            issues.push(ConfigIssue::error(
                "triage.per_hour_runs",
                "0 would skip every triage run; set triage.enabled = false to turn triage off, or allow at least 1 run per hour",
            ));
        }
        // Only an explicit reservation can be wrong: an omitted one is derived from the
        // hour, and a zero hour has nothing to keep free.
        if self.triage.per_hour_runs > 0
            && self
                .triage
                .reserved_for_incidents
                .is_some_and(|reserved| reserved >= self.triage.per_hour_runs)
        {
            issues.push(ConfigIssue::error(
                "triage.reserved_for_incidents",
                "must be less than `per_hour_runs`; the hour must still admit triage runs",
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
        if self.triage.per_run.prompt_bytes < 32_768 {
            issues.push(ConfigIssue::error(
                "triage.per_run.prompt_bytes",
                "must be at least 32768 (32 KiB); the opening context may not fit below it",
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
        if self.backup.keep_pre_migrate == 0 {
            issues.push(ConfigIssue::error(
                "backup.keep_pre_migrate",
                "must be at least 1",
            ));
        }
        for (key, days) in self.retention.days() {
            if !(1..=3650).contains(&days) {
                issues.push(ConfigIssue::error(
                    format!("retention.{key}"),
                    "must be 1 to 3650 days",
                ));
            }
        }
        orchestration_rules(&self.orchestration, issues);
        policy_rules(&self.policy, issues);
        self.packages.validate(issues);
    }
}

fn policy_rules(section: &HubPolicySection, issues: &mut Vec<ConfigIssue>) {
    let ttl = section.permit_ttl.as_signed();
    if ttl < jiff::SignedDuration::from_secs(30) || ttl > jiff::SignedDuration::from_hours(1) {
        issues.push(ConfigIssue::error(
            "policy.permit_ttl",
            "must be between 30 seconds and 1 hour",
        ));
    }
    if !(1..=20).contains(&section.circuit_failures) {
        issues.push(ConfigIssue::error(
            "policy.circuit_failures",
            "must be 1 to 20",
        ));
    }
    let window = section.circuit_window.as_signed();
    if window < jiff::SignedDuration::from_hours(1)
        || window > jiff::SignedDuration::from_hours(30 * 24)
    {
        issues.push(ConfigIssue::error(
            "policy.circuit_window",
            "must be between 1 hour and 30 days",
        ));
    }
    if section.max_permits_per_hour > 120 {
        issues.push(ConfigIssue::error(
            "policy.max_permits_per_hour",
            "must be 0 to 120",
        ));
    }
}

fn orchestration_rules(section: &OrchestrationSection, issues: &mut Vec<ConfigIssue>) {
    if section.max_running == 0 {
        issues.push(ConfigIssue::error(
            "orchestration.max_running",
            "must be at least 1",
        ));
    }
    if section.loop_guard_per_hour == 0 {
        issues.push(ConfigIssue::error(
            "orchestration.loop_guard_per_hour",
            "must be at least 1",
        ));
    }
    if section.approval_ttl.as_signed() > jiff::SignedDuration::from_hours(7 * 24) {
        issues.push(ConfigIssue::error(
            "orchestration.approval_ttl",
            "must be at most 7d; an older approval no longer matches what was shown",
        ));
    }
    for (key, limit) in section.budgets.counts() {
        if limit == Some(0) {
            issues.push(ConfigIssue::error(
                format!("orchestration.budgets.{key}"),
                "must be at least 1; remove it for the default",
            ));
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
    if let Some(url) = &provider.base_url
        && !(matches!(url.scheme(), "http" | "https") && url.has_host())
    {
        issues.push(ConfigIssue::error(
            format!("{path}.base_url"),
            "must be an http or https URL such as \"http://localhost:11434/v1\"",
        ));
    }
    if let Some(price) = &provider.price
        && !(non_negative(price.input_per_mtok_usd) && non_negative(price.output_per_mtok_usd))
    {
        issues.push(ConfigIssue::error(
            format!("{path}.price"),
            "prices must be zero or more",
        ));
    }
    if provider.fallback_models.len() > 5 {
        issues.push(ConfigIssue::error(
            format!("{path}.fallback_models"),
            "at most 5 fallback models",
        ));
    }
    if provider
        .fallback_models
        .iter()
        .any(|model| model.is_empty())
    {
        issues.push(ConfigIssue::error(
            format!("{path}.fallback_models"),
            "every fallback model needs a name",
        ));
    }
    if !provider.fallback_models.is_empty() && provider.kind == LlmKind::Cli {
        issues.push(ConfigIssue::error(
            format!("{path}.fallback_models"),
            "fallback models need an anthropic or openai_compatible provider",
        ));
    }
    if provider
        .call_deadline
        .is_some_and(|deadline| deadline.as_signed() < jiff::SignedDuration::from_secs(10))
    {
        issues.push(ConfigIssue::error(
            format!("{path}.call_deadline"),
            "must be at least 10s; a shorter hard deadline would refuse normal calls",
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
        let endpoint_ok = self.docker.endpoint.as_deref().is_none_or(|endpoint| {
            Url::parse(endpoint).is_ok_and(|u| matches!(u.scheme(), "unix" | "tcp"))
        });
        if !endpoint_ok {
            issues.push(ConfigIssue::error(
                "docker.endpoint",
                "must be unix:///path/to/engine.sock or tcp://host:port",
            ));
        }
        let packages_dir = self.agent.packages_dir.to_string_lossy();
        if !self.agent.packages_dir.is_absolute() && !packages_dir.starts_with('/') {
            issues.push(ConfigIssue::error(
                "agent.packages_dir",
                "must be an absolute path such as \"/var/lib/domovoi-packages\"",
            ));
        }
        if let Some(unattended) = &self.unattended {
            unattended.validate(issues);
        }
        if let Some(packages) = &self.packages {
            crate::validate_packages::package_rules(packages, issues);
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
