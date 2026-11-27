//! Which recorded Domovoi actions may explain a security event, and which may not.

use std::collections::BTreeMap;

use domovoi_core::{
    CheckKey, ContractId, Evidence, EvidenceValue, ExecutionIntent, ExecutionRecord, HostId,
    JournalEpoch, OperationId, PlanHash, Risk, Severity, Signal, SourceKind, StepId, TargetKey,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use super::match_records;

fn t0() -> Timestamp {
    Timestamp::from_second(1_700_000_000).unwrap()
}

fn after(mins: i64) -> Timestamp {
    t0() + SignedDuration::from_mins(mins)
}

fn host() -> HostId {
    HostId::new()
}

/// A recorded run of `contract` with `args`, dispatched at `dispatched` and settled a minute
/// later, as the hub stores one.
fn run(
    host: HostId,
    contract: &str,
    args: &[(&str, &str)],
    dispatched: Timestamp,
) -> ExecutionRecord {
    let intent = ExecutionIntent {
        contract: ContractId::parse(contract).unwrap(),
        contract_version: 1,
        host,
        check: CheckKey::parse("systemd.unit").unwrap(),
        target: Some(TargetKey::parse("web").unwrap()),
        args: args
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect::<BTreeMap<_, _>>(),
        retry_of: None,
    };
    let hash: PlanHash = "ab".repeat(32).parse().unwrap();
    let mut record = ExecutionRecord::propose(
        &intent,
        StepId::new(),
        OperationId::new(),
        hash,
        dispatched + SignedDuration::from_hours(1),
        Risk::Write,
        dispatched,
    );
    let epoch: JournalEpoch =
        serde_json::from_value(json!("0190a0a0-0000-7000-8000-000000000001")).unwrap();
    record.dispatched = Some((epoch, dispatched));
    record.settled_at = Some(dispatched + SignedDuration::from_mins(1));
    record
}

/// A failed-sudo event by `user` whose journal line shows `command`, observed at `at`.
fn sudo_event(host: HostId, command: &str, at: Timestamp) -> Signal {
    let mut evidence = Evidence::new();
    evidence.insert(
        "example",
        EvidenceValue::Text(format!(
            "kuker : 1 incorrect password attempt ; TTY=pts/0 ; COMMAND={command}"
        )),
    );
    Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse("security.sudo_failure").unwrap(),
        Some(TargetKey::parse("kuker").unwrap()),
        Severity::Warning,
        "failed sudo authentication for kuker".to_owned(),
        at,
    )
    .with_evidence(evidence)
}

fn explained(host: HostId, signals: &[Signal], records: &[ExecutionRecord]) -> Option<String> {
    let check = CheckKey::parse("security.sudo_failure").unwrap();
    match_records(&check, host, signals, records)
}

const RESTART: &[(&str, &str)] = &[("unit", "nginx.service"), ("identity_hash", "0123abcd")];

#[test]
fn a_recorded_restart_explains_the_event_that_names_its_unit() {
    let host = host();
    let record = run(host, "unit.restart", RESTART, after(0));
    let event = sudo_event(host, "/usr/bin/systemctl restart nginx.service", after(0));
    let line = explained(host, &[event], std::slice::from_ref(&record)).expect("explained");
    assert!(line.contains(&record.operation.to_string()), "{line}");
    assert!(line.contains("unit.restart"));
    assert!(line.contains("unit=nginx.service"));
    assert!(line.contains("dispatched"));
}

#[test]
fn an_unrelated_sudo_failure_is_not_explained_by_a_restart_of_another_unit() {
    let host = host();
    let record = run(host, "unit.restart", RESTART, after(0));
    let event = sudo_event(host, "/usr/bin/true", after(0));
    assert_eq!(explained(host, &[event], &[record]), None);
}

#[test]
fn a_record_on_another_host_is_not_accepted() {
    let (here, there) = (host(), host());
    let record = run(there, "unit.restart", RESTART, after(0));
    let event = sudo_event(here, "/usr/bin/systemctl restart nginx.service", after(0));
    assert_eq!(explained(here, &[event], &[record]), None);
}

#[test]
fn a_record_from_hours_earlier_is_not_accepted() {
    let host = host();
    let record = run(host, "unit.restart", RESTART, after(0));
    for minutes in [20, 120, 600] {
        let event = sudo_event(
            host,
            "/usr/bin/systemctl restart nginx.service",
            after(minutes),
        );
        assert_eq!(
            explained(host, &[event], std::slice::from_ref(&record)),
            None,
            "{minutes} minutes later"
        );
    }
    // And an event before the action started is not caused by it.
    let early = sudo_event(host, "/usr/bin/systemctl restart nginx.service", after(-30));
    assert_eq!(explained(host, &[early], &[record]), None);
}

#[test]
fn a_record_with_a_different_target_is_not_accepted() {
    let host = host();
    let record = run(
        host,
        "unit.restart",
        &[("unit", "postgresql.service"), ("identity_hash", "ff00")],
        after(0),
    );
    let event = sudo_event(host, "/usr/bin/systemctl restart nginx.service", after(0));
    assert_eq!(explained(host, &[event], &[record]), None);
}

#[test]
fn one_unexplained_event_keeps_the_whole_incident_unexplained() {
    let host = host();
    let record = run(host, "unit.restart", RESTART, after(0));
    let mine = sudo_event(host, "/usr/bin/systemctl restart nginx.service", after(0));
    let other = sudo_event(host, "/usr/bin/true", after(1));
    assert_eq!(explained(host, &[mine, other], &[record]), None);
}

#[test]
fn a_run_that_never_left_the_hub_and_a_word_of_the_check_name_explain_nothing() {
    let host = host();
    let mut unsent = run(host, "unit.restart", RESTART, after(0));
    unsent.dispatched = None;
    let event = sudo_event(host, "/usr/bin/systemctl restart nginx.service", after(0));
    assert_eq!(
        explained(host, std::slice::from_ref(&event), &[unsent]),
        None
    );
    // Installing a package called sudo does not explain every sudo failure.
    let install = run(host, "package.install", &[("package", "sudo")], after(0));
    assert_eq!(explained(host, &[event], &[install]), None);
}

#[test]
fn the_target_of_the_run_may_be_what_the_event_names() {
    let host = host();
    let record = run(
        host,
        "skill.restart_container",
        &[("container", "web-1")],
        after(0),
    );
    let event = Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse("security.sudo_failure").unwrap(),
        Some(TargetKey::parse("web").unwrap()),
        Severity::Warning,
        "failed sudo authentication for web".to_owned(),
        after(0),
    );
    assert!(explained(host, &[event], &[record]).is_some());
}
