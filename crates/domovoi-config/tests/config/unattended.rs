use crate::helpers::{AGENT, agent, agent_errors, assert_issue, hub, hub_errors};

fn with(extra: &str) -> String {
    format!("{AGENT}{extra}")
}

const OK: &str = "[unattended]\nenabled = true\n[[unattended.allow]]\ncapability = \"unit.restart\"\ntargets = [\"example.service\"]\nmax_per_hour = 2\n";

#[test]
fn no_unattended_section_means_no_ceiling() {
    let (config, _) = agent(AGENT).unwrap();
    assert!(config.unattended.is_none());
}

#[test]
fn a_unit_restart_ceiling_loads() {
    let (config, _) = agent(&with(OK)).unwrap();
    let section = config.unattended.unwrap();
    assert!(section.enabled);
    assert_eq!(section.allow[0].capability, "unit.restart");
    assert_eq!(section.allow[0].max_per_hour, 2);
}

#[test]
fn unattended_config_rejects_unknown_keys_and_unfit_fields() {
    let unknown = with("[unattended]\nenabled = true\nwildcard = true\n");
    assert!(!agent_errors(&unknown).is_empty());

    for field in [
        "max_active = 1",
        "max_ttl = \"5m\"",
        "args = { unit = [\"x.service\"] }",
    ] {
        let text = with(&format!("{OK}{field}\n"));
        let name = field.split(' ').next().unwrap();
        assert_issue(
            &agent_errors(&text),
            &format!("unattended.allow[0].{name}"),
            "not accepted",
        );
    }
    let destructive = with(&OK.replace("unit.restart", "unit.identity"));
    assert_issue(
        &agent_errors(&destructive),
        "unattended.allow[0].capability",
        "cannot run unattended",
    );
    let container = with(&OK.replace("unit.restart", "container.restart"));
    assert_issue(
        &agent_errors(&container),
        "unattended.allow[0].capability",
        "cannot run unattended",
    );
    let unknown_contract = with(&OK.replace("unit.restart", "nope.restart"));
    assert_issue(
        &agent_errors(&unknown_contract),
        "unattended.allow[0].capability",
        "contract this agent knows",
    );
}

#[test]
fn targets_must_be_canonical_ids_or_an_explicit_star() {
    let empty = with(&OK.replace("[\"example.service\"]", "[]"));
    assert_issue(
        &agent_errors(&empty),
        "unattended.allow[0].targets",
        "at least one target",
    );
    let bad = with(&OK.replace("example.service", "Not A Unit"));
    assert_issue(
        &agent_errors(&bad),
        "unattended.allow[0].targets",
        "canonical unit id",
    );
    assert!(agent(&with(&OK.replace("\"example.service\"", "\"*\""))).is_ok());
    let zero = with(&OK.replace("max_per_hour = 2", "max_per_hour = 0"));
    assert_issue(
        &agent_errors(&zero),
        "unattended.allow[0].max_per_hour",
        "at least 1",
    );
}

#[test]
fn hub_policy_guardrails_have_defaults_and_bounds() {
    let (config, _) = hub("").unwrap();
    assert_eq!(config.policy.permit_ttl.as_signed().as_secs(), 300);
    assert_eq!(config.policy.circuit_failures, 2);
    assert_eq!(
        config.policy.circuit_window.as_signed().as_secs(),
        24 * 3600
    );
    assert_eq!(config.policy.max_permits_per_hour, 12);

    assert_issue(
        &hub_errors("[policy]\npermit_ttl = \"10s\"\n"),
        "policy.permit_ttl",
        "30 seconds",
    );
    assert_issue(
        &hub_errors("[policy]\npermit_ttl = \"2h\"\n"),
        "policy.permit_ttl",
        "1 hour",
    );
    assert_issue(
        &hub_errors("[policy]\ncircuit_failures = 0\n"),
        "policy.circuit_failures",
        "1 to 20",
    );
    assert_issue(
        &hub_errors("[policy]\ncircuit_failures = 21\n"),
        "policy.circuit_failures",
        "1 to 20",
    );
    assert_issue(
        &hub_errors("[policy]\ncircuit_window = \"10m\"\n"),
        "policy.circuit_window",
        "1 hour",
    );
    assert_issue(
        &hub_errors("[policy]\ncircuit_window = \"31d\"\n"),
        "policy.circuit_window",
        "30 days",
    );
    assert_issue(
        &hub_errors("[policy]\nmax_permits_per_hour = 121\n"),
        "policy.max_permits_per_hour",
        "0 to 120",
    );
    assert!(hub("[policy]\nmax_permits_per_hour = 0\n").is_ok());
}
