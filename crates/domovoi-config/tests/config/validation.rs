use crate::helpers::{AGENT, agent, agent_errors, assert_issue, hub, hub_errors};

#[test]
fn hub_url_must_use_wss() {
    let errors = agent_errors("[agent]\nhost_name = \"nas\"\nhub_url = \"ws://hub.lan:7421\"\n");
    assert_issue(&errors, "agent.hub_url", "must use wss://");
}

#[test]
fn bind_address_must_parse() {
    let errors = hub_errors("[web]\nbind = \"localhost\"\n");
    assert_issue(&errors, "web.bind", "invalid socket address");
}

#[test]
fn urls_must_parse() {
    let errors = hub_errors("[web]\npublic_url = \"not a url\"\n");
    assert_issue(&errors, "web.public_url", "relative URL");
    let errors = hub_errors("[web]\npublic_url = \"ftp://files.lan\"\n");
    assert_issue(&errors, "web.public_url", "http or https");
}

#[test]
fn quiet_hours_start_must_differ_from_end() {
    let errors = hub_errors("[notify]\nquiet_hours = { start = \"08:00\", end = \"08:00\" }\n");
    assert_issue(&errors, "notify.quiet_hours", "start and end are the same");
}

#[test]
fn discord_enabled_requires_allowed_users() {
    let text = "[discord]\nbot_token = { env = \"BOT\" }\nchannel_id = 1\n";
    let errors = hub_errors(text);
    assert_issue(
        &errors,
        "discord.allowed_user_ids",
        "at least one Discord user",
    );

    let disabled = format!("{text}enabled = false\n");
    let errors = hub_errors(&disabled);
    assert!(errors.iter().all(|e| e.path != "discord.allowed_user_ids"));
}

#[test]
fn triage_enabled_requires_llm_provider() {
    let errors = hub_errors("[triage]\nenabled = true\n");
    assert_issue(&errors, "llm.primary", "no LLM provider");
    assert!(hub("[triage]\nenabled = false\n").is_ok());
}

#[test]
fn cli_kind_requires_cli_block() {
    let errors = hub_errors("[llm.primary]\nkind = \"cli\"\n");
    assert_issue(&errors, "llm.primary.cli", "needs a cli block");
    let errors = hub_errors("[llm.secondary]\nkind = \"cli\"\ncli = { args = [\"-p\"] }\n");
    assert_issue(&errors, "llm.secondary.cli", "set preset");
    assert!(hub("[llm.primary]\nkind = \"cli\"\ncli = { preset = \"codex\" }\n").is_ok());
}

#[test]
fn api_providers_require_model_and_endpoint() {
    let errors = hub_errors("[llm.primary]\nkind = \"openai_compatible\"\n");
    assert_issue(&errors, "llm.primary.model", "needs a model");
    assert_issue(&errors, "llm.primary.base_url", "base_url");
    let errors = hub_errors("[llm.primary]\nkind = \"anthropic\"\nmodel = \"m\"\n");
    assert_issue(&errors, "llm.primary.api_key", "needs an api_key");
}

#[test]
fn host_names_are_validated() {
    let errors = agent_errors("[agent]\nhost_name = \"My NAS\"\nhub_url = \"wss://h:1\"\n");
    assert_issue(&errors, "agent.host_name", "host name");
    let errors = hub_errors("[[probes]]\nhost = \"Bad_Host\"\nkind = \"tcp\"\ntarget = \"a:1\"\n");
    assert_issue(&errors, "probes[0].host", "host name");
}

#[test]
fn http_probe_requires_expect_status() {
    let errors =
        hub_errors("[[probes]]\nhost = \"nas\"\nkind = \"http\"\ntarget = \"https://nas.lan/\"\n");
    assert_issue(&errors, "probes[0].expect_status", "expected status");
    let errors = hub_errors(
        "[[probes]]\nhost = \"nas\"\nkind = \"tcp\"\ntarget = \"nas.lan\"\nexpect_status = 200\n",
    );
    assert_issue(&errors, "probes[0].target", "host:port");
    assert_issue(&errors, "probes[0].expect_status", "only http probes");
}

#[test]
fn zero_concurrency_is_rejected() {
    let errors = hub_errors("[triage]\nconcurrency = 0\n[backup]\nkeep = 0\n");
    assert_issue(&errors, "triage.concurrency", "at least 1");
    assert_issue(&errors, "backup.keep", "at least 1");
}

#[test]
fn webhook_names_are_validated() {
    let errors = hub_errors(
        "[webhooks.\"My Hook\"]\nkind = \"generic\"\nauth = \"hmac\"\nsecret = { env = \"S\" }\n",
    );
    assert_issue(&errors, "webhooks.My Hook", "webhook names");
}

#[test]
fn docker_endpoint_must_be_unix_or_tcp() {
    let errors = agent_errors(&format!(
        "{AGENT}[docker]\nendpoint = \"http://docker:2375\"\n"
    ));
    assert_issue(&errors, "docker.endpoint", "unix://");
    assert!(
        agent(&format!(
            "{AGENT}[docker]\nendpoint = \"tcp://proxy:2375\"\n"
        ))
        .is_ok()
    );
}

#[test]
fn every_problem_is_reported_at_once() {
    let errors = hub_errors(
        "[triage]\nenabled = true\nconcurrency = 0\n[notify]\nquiet_hours = { start = \"01:00\", end = \"01:00\" }\n",
    );
    assert_eq!(errors.len(), 3, "{errors:#?}");
}
