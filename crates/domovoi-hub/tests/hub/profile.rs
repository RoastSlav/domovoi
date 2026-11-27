//! Collecting a host profile and showing it: what the agent's read tools said, what the
//! note looks like, and when the minute scheduler decides a profile must be collected
//! again.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::api_harness::{Api, TestAgents, api, api_without_profiles};
use axum::http::{Method, StatusCode};
use domovoi_core::capability::{ContractIndex, builtin_contracts};
use domovoi_core::ports::{
    CapabilityRegistry, HostRepo, LinkError, NoteRepo, Presence, SettingsRepo, ToolOutput,
};
use domovoi_core::{
    CheckKey, HostId, ParamKind, ParamName, ParamSpec, ReadToolCall, Risk, SkillSummary,
};
use domovoi_engine::{Evidence, HostCatalog, PatternRedactor};
use domovoi_hub::capability::{HubRegistry, RegistryDeps};
use domovoi_hub::profile::diff::{MAX_CHANGES, changes, recent_changes};
use domovoi_hub::profile::{
    ContainerLine, HostProfile, ProfileDeps, ProfileError, STALE_AFTER, basis, needs_refresh,
    note_text, profile_key, refresh, spawn, stored,
};
use domovoi_proto::messages::{CheckInfo, HostFacts};
use domovoi_testkit::FakeCapabilityRegistry;
use domovoi_testkit::build::{host, manifest};
use domovoi_testkit::link::{LinkRequest, evidence_caps, observer_of};
use serde_json::json;

use jiff::SignedDuration;
use jiff::Timestamp;
use tokio::sync::Semaphore;

/// What the agent's `HostOverview` answers with.
const OVERVIEW_TEXT: &str = "uptime: 41 days\nload: 0.08 0.11 0.09\nmemory: 31 GiB total, 6.2 GiB used\ndisks:\n  /srv 2.0T 91%";

/// One `docker ps` with its header: one healthy container and one degraded.
const DOCKER_TEXT: &str = "name\timage\tstate\thealth\trestarts\nnfsd\tnfs\trunning\thealthy\t0\ndata\tpolardb\trunning\tdegraded\t3";

/// Two sockets answered outside loopback.
const PORTS_TEXT: &str = "2049 ephemeris (nfs)\n8101 samba (LISTEN) fd 12 (*:8101)";

/// The last day of kernel messages, newest quoted first.
const KERNEL_TEXT: &str =
    "Nov 04 05:12 kernel[1]: nfsd: read error on /srv\nNov 04 05:13 systemd[1]: Restarted job.";

/// What one of the host's read skills prints.
const SKILL_TEXT: &str = "listening_ports: 2049 ephemeris (nfs)";

/// What the agent said its machine is, as a handshake would.
pub(crate) fn handshake(when: Timestamp) -> HostFacts {
    HostFacts {
        hostname: "diskstation".to_owned(),
        os: "Debian GNU/Linux 13".to_owned(),
        kernel: "6.12.9".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_count: 4,
        mem_total_bytes: 34_359_738_368,
        boot_time: when,
        boot_id: "8f2c1e4a".to_owned(),
    }
}

/// The checks the agent's own config leaves on.
fn announced() -> Vec<CheckInfo> {
    [
        CheckInfo {
            key: CheckKey::parse("disk.usage").unwrap(),
            enabled: true,
            interval_seconds: 300,
            thresholds: BTreeMap::new(),
        },
        CheckInfo {
            key: CheckKey::parse("mem.used").unwrap(),
            enabled: false,
            interval_seconds: 60,
            thresholds: BTreeMap::new(),
        },
    ]
    .to_vec()
}

/// The skills the agent announced: one the hub may run, one it may not.
fn announced_skills() -> Vec<SkillSummary> {
    [
        manifest("listening_ports", Risk::Read)
            .unwrap()
            .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry")),
        manifest("restart_container", Risk::Write)
            .unwrap()
            .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry")),
    ]
    .to_vec()
}

/// The skills a host announces when a huge answer per skill must fill the note.
fn announced_many() -> Vec<SkillSummary> {
    ["list_shares", "show_pool", "show_tank", "read_logs"]
        .iter()
        .map(|name| {
            manifest(name, Risk::Read)
                .unwrap()
                .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"))
        })
        .collect::<Vec<SkillSummary>>()
}

/// The text one of the agent's read tools answers with.
fn answered(call: &ReadToolCall) -> String {
    match call {
        ReadToolCall::HostOverview => OVERVIEW_TEXT,
        ReadToolCall::DockerPs { .. } => DOCKER_TEXT,
        ReadToolCall::ListeningPorts => PORTS_TEXT,
        ReadToolCall::RunReadSkill { .. } => SKILL_TEXT,
        _ => KERNEL_TEXT,
    }
    .to_owned()
}

/// Connects `id`'s agent and lets it say what its machine is.
fn connect(a: &Api, id: HostId, version: &str) {
    let when = a.now();
    a.link.set_presence(
        id,
        Presence::Connected {
            since: when,
            version: version.to_owned(),
        },
    );
    a.link.set_facts(id, handshake(when));
}

/// The connected agent of the harness host, its facts, its checks and its answers.
pub(crate) fn seed(a: &Api, version: &str) {
    connect(a, a.nas.id, version);
    a.link.set_checks(a.nas.id, announced());
    a.link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        })
    });
}

/// The hub's pieces put together as the profile task and the API use them.
pub(crate) fn profile_deps(a: &Api) -> ProfileDeps {
    ProfileDeps {
        evidence: Evidence::new(
            evidence_caps(a.link.clone()),
            a.clock.clone(),
            Arc::new(FakeCapabilityRegistry::following(a.link.clone())),
        ),
        catalog: HostCatalog::new(observer_of(a.link.clone())),
        capabilities: Arc::new(FakeCapabilityRegistry::following(a.link.clone())),
        agents: Arc::new(TestAgents {
            link: a.link.clone(),
            hosts: a.store.clone(),
            packages: a.packages.clone(),
        }),
        hosts: a.store.clone(),
        notes: a.store.clone(),
        settings: a.store.clone(),
        clock: a.clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        knowledge: None,
    }
}

#[tokio::test]
async fn profile_collection_reads_through_the_evidence_boundary() {
    let a = api().await;
    seed(&a, "0.6.0");
    a.link.set_skills(a.nas.id, announced_skills());
    // One read fails at the agent: the page has to say so, with the agent's own words.
    a.link.on_tool(|_, call| {
        if matches!(call, ReadToolCall::ListeningPorts) {
            return Err(LinkError::Rejected(
                "the port table is unreadable".to_owned(),
            ));
        }
        Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        })
    });

    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let titles: Vec<&str> = profile
        .sections
        .iter()
        .map(|section| section.title.as_str())
        .collect();
    assert!(
        [
            "Overview",
            "Containers",
            "Listening ports",
            "Kernel messages (24 h)"
        ]
        .iter()
        .all(|fixed| titles.contains(fixed)),
        "the fixed sections are still there: {titles:?}"
    );
    let ports = profile
        .sections
        .iter()
        .find(|section| section.title == "Listening ports")
        .expect("the failed section stays on the page");
    assert!(!ports.ok, "a failed read is never shown as an empty one");
    assert_eq!(
        ports.text,
        "rejected by agent: the port table is unreadable"
    );

    // Every read that reached the agent was on the host being profiled.
    for request in a.link.requests() {
        if let LinkRequest::Tool { host, .. } = request {
            assert_eq!(host, a.nas.id, "a profile read reached another host");
        }
    }
}

#[tokio::test]
async fn the_profile_records_the_agents_own_tools_and_settings() {
    let a = api().await;
    seed(&a, "0.6.0");
    a.link.set_skills(a.nas.id, announced_skills());
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    assert_eq!(profile.agent_version, "0.6.0");
    assert_eq!(profile.facts.hostname, "diskstation");
    assert_eq!(profile.facts.cpu_count, 4);
    assert_eq!(profile.facts.mem_total_bytes, 34_359_738_368);
    assert_eq!(
        profile.containers.len(),
        2,
        "the header line is not a container"
    );
    assert_eq!(profile.containers[1].health, "degraded");
    assert_eq!(profile.containers[1].restarts, "3");
    assert_eq!(
        profile.listeners.join(", "),
        "2049 ephemeris (nfs), 8101 samba (LISTEN) fd 12 (*:8101)",
    );
    assert_eq!(
        profile.skills.join(", "),
        "listening_ports (Read), restart_container (Write)",
    );
    assert_eq!(
        profile.checks.join(", "),
        "disk.usage: every 300 s on, mem.used: every 60 s off",
    );
    let titles = profile
        .sections
        .iter()
        .map(|section| section.title.clone())
        .collect::<Vec<String>>();
    assert_eq!(
        titles.join(", "),
        "Overview, Containers, Listening ports, Kernel messages (24 h), listening_ports",
    );
}

#[tokio::test]
async fn read_skills_that_need_arguments_are_skipped_not_asked_empty() {
    let a = api().await;
    seed(&a, "0.6.0");
    let mut needed = manifest("disk_usage_top", Risk::Read)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"));
    needed.params = [ParamSpec {
        name: ParamName::parse("path").unwrap(),
        description: "mount point".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: "^/.*$".to_owned(),
            max_len: 255,
        },
    }]
    .to_vec();
    a.link.set_skills(
        a.nas.id,
        [
            manifest("listening_ports", Risk::Read)
                .unwrap()
                .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry")),
            needed,
        ]
        .to_vec(),
    );
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    assert!(
        profile
            .sections
            .iter()
            .all(|section| section.title != "disk_usage_top"),
        "a skill that needs arguments gets no section of its own"
    );
    let note = profile
        .sections
        .iter()
        .find(|section| section.title == "Not collected")
        .expect("the skills left out are named once");
    assert!(note.ok, "leaving it out by design is not a warning");
    assert!(note.text.contains("disk_usage_top"), "{}", note.text);
    let asked = profile
        .sections
        .iter()
        .any(|section| section.title == "listening_ports");
    assert!(
        asked,
        "an argument-free read skill next to it is still asked"
    );
}

#[tokio::test]
async fn only_a_read_skill_the_agent_installed_ever_gets_asked() {
    let a = api().await;
    seed(&a, "0.6.0");
    a.link.set_skills(a.nas.id, announced_skills());
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let asked = profile
        .sections
        .iter()
        .any(|section| section.title == "listening_ports");
    assert!(asked, "the installed read skill has its own section");
    let write_skill = profile
        .sections
        .iter()
        .any(|section| section.title == "restart_container");
    assert!(
        !write_skill,
        "the hub never runs a skill that changes state"
    );
    let never_announced = profile
        .sections
        .iter()
        .any(|section| section.title == "show_pool");
    assert!(
        !never_announced,
        "the hub never asks for a skill this host lacks"
    );
}

#[tokio::test]
async fn a_disabled_read_skill_is_never_asked_for_a_profile() {
    let a = api().await;
    let login = a.admin().await;
    seed(&a, "0.6.0");
    a.link.set_skills(a.nas.id, announced_skills());
    let saved = a
        .send(
            Method::PUT,
            "/api/v1/hosts/nas/skills/listening_ports",
            Some(&login),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(
        saved.status,
        StatusCode::NO_CONTENT,
        "{}: {}",
        saved.status,
        saved.text
    );
    let registry = match HubRegistry::new(RegistryDeps {
        contracts: Arc::new(match ContractIndex::new(builtin_contracts()) {
            Ok(index) => index,
            Err(err) => panic!("{err}"),
        }),
        agent: Arc::new(domovoi_hub::runtime::caps::LinkCapabilities::new(
            a.link.clone(),
        )),
        hosts: a.store.clone(),
        store: a.store.clone(),
        clock: a.clock.clone(),
    })
    .await
    {
        Ok(registry) => Arc::new(registry),
        Err(err) => panic!("{err}"),
    };
    let capabilities: Arc<dyn CapabilityRegistry> = registry.clone();
    let deps = ProfileDeps {
        evidence: Evidence::new(
            domovoi_hub::runtime::caps::evidence_caps(a.link.clone()),
            a.clock.clone(),
            capabilities.clone(),
        ),
        catalog: HostCatalog::new(domovoi_hub::runtime::caps::observer_of(a.link.clone())),
        capabilities,
        agents: Arc::new(TestAgents {
            link: a.link.clone(),
            hosts: a.store.clone(),
            packages: a.packages.clone(),
        }),
        hosts: a.store.clone(),
        notes: a.store.clone(),
        settings: a.store.clone(),
        clock: a.clock.clone(),
        redactor: Arc::new(PatternRedactor::default()),
        knowledge: None,
    };
    let profile = refresh(&deps, &a.nas).await.unwrap();
    let titles = profile
        .sections
        .iter()
        .map(|section| section.title.clone())
        .collect::<Vec<String>>();
    assert_eq!(
        titles,
        [
            "Overview",
            "Containers",
            "Listening ports",
            "Kernel messages (24 h)"
        ]
        .to_vec(),
        "only the tools the hub always asks: the switched-off skill has no section",
    );
    let suggested = profile.skills.iter().any(|name| name == "listening_ports");
    assert!(
        !suggested,
        "the profile does not offer a skill the host has switched off"
    );
    let fired = a.link.requests().iter().any(|call| {
        matches!(
            call,
            LinkRequest::Tool {
                call: ReadToolCall::RunReadSkill { skill, .. },
                ..
            } if skill.as_str() == "listening_ports"
        )
    });
    assert!(!fired, "the disabled skill never reached the agent");
}

#[tokio::test]
async fn a_tool_that_never_answers_leaves_a_visible_gap_in_the_profile() {
    let a = api().await;
    connect(&a, a.nas.id, "0.6.0");
    a.link.set_checks(a.nas.id, announced());
    a.link.on_tool(|_, call| match call {
        ReadToolCall::ListeningPorts => Err(LinkError::Timeout),
        _ => Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        }),
    });
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let gap = profile
        .sections
        .iter()
        .find(|section| section.title == "Listening ports");
    let gap = gap.unwrap();
    assert!(!gap.ok, "the gap says the tool did not answer");
    assert_eq!(gap.text, "timed out");
    assert!(
        profile.listeners.is_empty(),
        "nothing was invented for the missing sockets"
    );
    assert_eq!(
        profile.containers.len(),
        2,
        "the rest of the profile was still collected"
    );
}

#[tokio::test]
async fn an_availability_check_pending_past_the_deadline_times_out_every_section() {
    let a = api_without_profiles().await;
    connect(&a, a.nas.id, "0.6.0");
    a.link.set_checks(a.nas.id, announced());
    a.link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        })
    });
    // One gate with no permit, so every availability the profile asks stays pending,
    // and a notifier that fires on entry: the test moves the clock only while a check
    // is on hold, exactly where the deadline has to cut in.
    let entered = a
        .link
        .hold_availability(a.nas.id, Arc::new(Semaphore::new(0)));
    let clock = a.clock.clone();
    let deps = profile_deps(&a);
    let nas = a.nas.clone();
    let task = tokio::spawn(async move { refresh(&deps, &nas).await });
    // Each section asks for its own availability check with its own deadline: past the
    // deadline, the boundary times out the pending ask and the next check enters.
    for _ in 0..4 {
        entered.notified().await;
        clock.advance(SignedDuration::from_secs(21));
    }
    let profile = task.await.unwrap().unwrap();
    assert_eq!(
        profile.sections.len(),
        4,
        "the four fixed sections are all there is"
    );
    for section in profile.sections.iter() {
        assert!(
            !section.ok,
            "no availability check ever answered, so no section did"
        );
        assert_eq!(
            section.text, "timed out",
            "every gap says the deadline passed"
        );
    }
    let asked = a
        .link
        .requests()
        .iter()
        .any(|call| matches!(call, LinkRequest::Tool { .. }));
    assert!(
        !asked,
        "the read never ran past a pending availability check"
    );
}

#[tokio::test]
async fn a_disconnected_agent_is_never_asked_and_nothing_is_stored() {
    let a = api().await;
    a.link.set_facts(a.nas.id, handshake(a.now()));
    a.link.set_checks(a.nas.id, announced());
    let deps = profile_deps(&a);
    let collected = refresh(&deps, &a.nas).await;
    assert_eq!(
        collected,
        Err(ProfileError::NotConnected),
        "there is nobody to ask"
    );
    assert!(
        a.link.requests().is_empty(),
        "the hub did not send a request to an absent agent"
    );
    let none = stored(&deps, a.nas.id).await;
    assert_eq!(none, Ok(None), "a failed collection stores nothing");
    assert_eq!(
        NoteRepo::list(&*a.store, Some(a.nas.id))
            .await
            .unwrap()
            .len(),
        0,
        "no note either"
    );
}

#[tokio::test]
async fn the_generated_note_names_the_machine_the_tools_described() {
    let a = api().await;
    seed(&a, "0.6.0");
    a.link.set_skills(a.nas.id, announced_skills());
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let body = note_text(&a.nas, &profile);
    assert!(body.contains("# Host profile: nas"), "{}", body);
    assert!(body.contains("_Generated by Domovoi."), "{}", body);
    assert!(body.contains("Agent version 0.6.0."), "{}", body);
    assert!(
        body.contains("Debian GNU/Linux 13 on x86_64, kernel 6.12.9"),
        "{}",
        body
    );
    assert!(body.contains("4 logical CPUs, 32 GiB memory"), "{}", body);
    assert!(
        body.contains("2 containers (2 running): nfsd (nfs), data (polardb)"),
        "{}",
        body
    );
    assert!(
        body.contains(
            "listening sockets: 2049 ephemeris (nfs), 8101 samba (LISTEN) fd 12 (*:8101)"
        ),
        "{}",
        body,
    );
    assert!(
        body.contains("read skills: listening_ports (Read), restart_container (Write)"),
        "{}",
        body
    );
    assert!(body.contains("last kernel messages:"), "{}", body);
    assert!(body.contains("nfsd: read error on /srv"), "{}", body);
    assert!(body.contains("## Details"), "{}", body);
    assert!(body.contains("## Summary"), "{}", body);
    let notes = NoteRepo::list(&*a.store, Some(a.nas.id)).await.unwrap();
    assert!(notes.is_empty(), "a profile is never a note: {notes:?}");
    let text = SettingsRepo::get(&*a.store, &domovoi_core::profile_text_key(a.nas.id))
        .await
        .unwrap()
        .expect("triage reads the stored text");
    assert_eq!(text["markdown"], body, "triage reads what the API shows");
}

#[tokio::test]
async fn the_profile_diff_names_a_container_that_arrived() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.containers.push(ContainerLine {
        name: "backup".to_owned(),
        image: "rsync:stable".to_owned(),
        state: "running".to_owned(),
        health: "-".to_owned(),
        restarts: "0".to_owned(),
    });
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        ["container \"backup\" added"].to_vec(),
        "one line names the container that came",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_container_that_left() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.containers.remove(0);
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        [format!(
            "container \"{}\" removed",
            before.containers[0].name
        )]
        .to_vec(),
        "one line names the container that left",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_container_that_changed_image() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.containers[0].image = "nginx:1.31".to_owned();
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        [format!(
            "container \"{}\" image changed from \"{}\" to \"{}\"",
            before.containers[0].name, before.containers[0].image, after.containers[0].image,
        ),]
        .to_vec(),
        "the line quotes both images",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_listening_socket_that_arrived() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.listeners.push("8102 cups (LISTEN)".to_owned());
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        ["listening socket \"8102 cups (LISTEN)\" added"].to_vec(),
        "one line names the socket that came",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_listening_socket_that_left() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.listeners.remove(0);
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        [format!(
            "listening socket \"{}\" removed",
            before.listeners[0]
        )]
        .to_vec(),
        "one line names the socket that left",
    );
}

#[tokio::test]
async fn a_failed_container_query_invents_no_removals() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    a.link.on_tool(|_, call| match call {
        ReadToolCall::DockerPs { .. } => Err(LinkError::Timeout),
        _ => Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        }),
    });
    let after = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    assert!(
        after.containers.is_empty(),
        "a query that did not answer leaves no inventory behind"
    );
    let expected: Vec<String> = vec![];
    assert_eq!(
        changes(&before, &after),
        expected,
        "containers that are still there are not reported as removed because docker did not answer",
    );
}

#[tokio::test]
async fn a_failed_port_query_invents_no_socket_removals() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    a.link.on_tool(|_, call| match call {
        ReadToolCall::ListeningPorts => Err(LinkError::Timeout),
        _ => Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        }),
    });
    let after = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    assert!(
        after.listeners.is_empty(),
        "a query that did not answer leaves no sockets behind"
    );
    let expected: Vec<String> = vec![];
    assert_eq!(
        changes(&before, &after),
        expected,
        "sockets that are still listening are not reported as removed because the tool did not answer",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_read_skill_that_arrived() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.skills.push("show_pool (Read)".to_owned());
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        ["read skill \"show_pool (Read)\" added"].to_vec(),
        "one line names the skill that came",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_read_skill_that_left() {
    let a = api().await;
    seed(&a, "0.6.0");
    a.link.set_skills(a.nas.id, announced_many());
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.skills.remove(after.skills.len() - 1);
    let last = before.skills[before.skills.len() - 1].clone();
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        [format!("read skill \"{}\" removed", last)].to_vec(),
        "one line names the skill that left",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_new_operating_system() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.facts.os = "Ubuntu 24.04".to_owned();
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        [format!(
            "os changed from \"{}\" to \"{}\"",
            before.facts.os, after.facts.os,
        ),]
        .to_vec(),
        "the line quotes both operating systems",
    );
}

#[tokio::test]
async fn the_profile_diff_names_a_new_kernel() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.facts.kernel = "6.13.0".to_owned();
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        [format!(
            "kernel changed from \"{}\" to \"6.13.0\"",
            before.facts.kernel,
        ),]
        .to_vec(),
        "the line quotes both kernels",
    );
}

#[tokio::test]
async fn the_profile_diff_names_more_memory() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.facts.mem_total_bytes = 68_719_476_736;
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        [format!(
            "memory changed from {} GiB to {} GiB",
            "32.0", "64.0"
        )]
        .to_vec(),
        "memory is printed in the unit a person reads",
    );
}

#[tokio::test]
async fn a_memory_change_smaller_than_a_gigabyte_still_says_something() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.facts.mem_total_bytes += 200 * 1024 * 1024;
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        ["memory changed from 32.0 GiB to 32.1 GiB"].to_vec(),
        "the line shows the difference, not two equal whole numbers",
    );
}

#[tokio::test]
async fn the_profile_diff_names_more_cpus() {
    let a = api().await;
    seed(&a, "0.6.0");
    let before = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut after = before.clone();
    after.facts.cpu_count = 8;
    let lines = changes(&before, &after);
    assert_eq!(
        lines,
        ["cpu count changed from 4 to 8"].to_vec(),
        "the line counts the logical CPUs",
    );
}

#[tokio::test]
async fn two_identical_profile_snapshots_produce_no_diff_lines() {
    let a = api().await;
    seed(&a, "0.6.0");
    let first = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let second = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let lines = changes(&first, &second);
    assert!(
        lines.is_empty(),
        "identical snapshots give no lines: {}",
        lines.join(",")
    );
}

#[tokio::test]
async fn the_note_shows_what_changed_since_the_last_refresh() {
    let a = api().await;
    seed(&a, "0.6.0");
    let deps = profile_deps(&a);
    let first = refresh(&deps, &a.nas).await.unwrap();
    assert!(
        first.changes.is_empty(),
        "a first profile has no history yet"
    );
    a.link.set_facts(
        a.nas.id,
        HostFacts {
            cpu_count: 8,
            ..handshake(a.now())
        },
    );
    let second = refresh(&deps, &a.nas).await.unwrap();
    assert_eq!(
        second.changes,
        [format!(
            "{}: cpu count changed from 4 to 8",
            second.collected_at.strftime("%Y-%m-%d %H:%M UTC"),
        ),]
        .to_vec(),
        "the change is dated with the refresh that found it",
    );
    let body = note_text(&a.nas, &second);
    assert!(body.contains("## Recent changes"), "{}", body);
    assert!(body.contains("cpu count changed from 4 to 8"), "{}", body);
    let text = SettingsRepo::get(&*a.store, &domovoi_core::profile_text_key(a.nas.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        text["markdown"], body,
        "the stored text shows the change too"
    );
}

#[tokio::test]
async fn the_profile_keeps_only_the_twenty_newest_change_lines() {
    let a = api().await;
    seed(&a, "0.6.0");
    let first = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let mut older = first.clone();
    older.changes = (1..31).map(|n| format!("kept {}", n)).collect();
    let previous: Option<HostProfile> = Some(older);
    let lines = recent_changes(&previous, a.now(), &first);
    assert_eq!(lines.len(), MAX_CHANGES, "the cap holds");
    let newest: Vec<String> = (1..21).map(|n| format!("kept {}", n)).collect();
    assert_eq!(lines, newest, "the twenty newest kept lines come first");
}

#[tokio::test]
async fn refreshing_the_profile_replaces_its_text_and_adds_no_note() {
    let a = api().await;
    seed(&a, "0.6.0");
    let deps = profile_deps(&a);
    refresh(&deps, &a.nas).await.unwrap();
    let second = refresh(&deps, &a.nas).await.unwrap();
    let notes = NoteRepo::list(&*a.store, Some(a.nas.id)).await.unwrap();
    assert!(notes.is_empty(), "no profile note is ever created");
    let text = SettingsRepo::get(&*a.store, &domovoi_core::profile_text_key(a.nas.id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(text["markdown"], note_text(&a.nas, &second));
}

#[tokio::test]
async fn a_tool_that_prints_far_too_much_is_cut_and_says_so() {
    let a = api().await;
    connect(&a, a.nas.id, "0.6.0");
    a.link.set_checks(a.nas.id, announced());
    a.link.on_tool(|_, call| match call {
        ReadToolCall::HostOverview => Ok(ToolOutput {
            text: "x".repeat(40 * 1024),
            truncated: true,
        }),
        _ => Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        }),
    });
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let overview = profile
        .sections
        .iter()
        .find(|section| section.title == "Overview");
    let overview = overview.unwrap();
    assert!(
        overview.text.len() <= 8 * 1024,
        "the section stays inside its cap"
    );
    assert!(
        overview.text.ends_with("[the text was cut]"),
        "{}",
        overview.text
    );
    let body = note_text(&a.nas, &profile);
    assert!(
        body.len() < 64 * 1024,
        "one big section does not blow the note away"
    );
    assert!(
        !body.contains("uptime"),
        "the cut kept the start of the output, not its last lines",
    );
}

#[tokio::test]
async fn the_whole_note_stays_under_its_size_cap() {
    let a = api().await;
    connect(&a, a.nas.id, "0.6.0");
    a.link.set_skills(a.nas.id, announced_many());
    a.link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "x".repeat(40 * 1024),
            truncated: true,
        })
    });
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    assert!(
        profile
            .sections
            .iter()
            .all(|section| section.text.len() <= 8 * 1024),
        "every section keeps its own cap",
    );
    let body = note_text(&a.nas, &profile);
    assert_eq!(body.len(), 64 * 1024, "the note is cut to its cap exactly");
    assert!(body.ends_with("[the text was cut]"), "{}", body);
}

#[tokio::test]
async fn a_secret_the_tool_printed_never_reaches_the_note() {
    let a = api().await;
    connect(&a, a.nas.id, "0.6.0");
    a.link.set_checks(a.nas.id, announced());
    a.link.on_tool(|_, call| match call {
        ReadToolCall::HostOverview => Ok(ToolOutput {
            text: "uptime: 41 days\nmount nfs at /srv password=hunter2".to_owned(),
            truncated: false,
        }),
        _ => Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        }),
    });
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let body = note_text(&a.nas, &profile);
    assert!(body.contains("password=[REDACTED]"), "{}", body);
    assert!(
        !body.contains("hunter2"),
        "the secret is gone from every copy",
    );
    let text = SettingsRepo::get(&*a.store, &domovoi_core::profile_text_key(a.nas.id))
        .await
        .unwrap()
        .unwrap();
    assert!(
        !text.to_string().contains("hunter2"),
        "the stored text is clean too"
    );
    let kept = SettingsRepo::get(&*a.store, &profile_key(a.nas.id))
        .await
        .unwrap();
    let raw = kept.map(|value| value.to_string()).unwrap_or_default();
    assert!(
        !raw.contains("hunter2"),
        "the stored profile JSON is clean too"
    );
}

#[tokio::test]
async fn an_unchanged_recent_profile_is_left_as_it_is() {
    let a = api().await;
    seed(&a, "0.6.0");
    let deps = profile_deps(&a);
    let profile = refresh(&deps, &a.nas).await.unwrap();
    let now = basis(&deps, a.nas.id).await;
    let due =
        |facts: HostFacts, version: &str| needs_refresh(&profile, &facts, version, &now, a.now());
    assert_eq!(due(handshake(a.now()), "0.6.0"), None, "nothing changed");
    let updated = HostFacts {
        kernel: "6.13.0".to_owned(),
        ..handshake(a.now())
    };
    assert!(due(updated, "0.6.0").is_some(), "the machine was updated");
    let memory = HostFacts {
        mem_total_bytes: 68_719_476_736,
        ..handshake(a.now())
    };
    assert!(
        due(memory, "0.6.0").is_some(),
        "the machine got more memory"
    );
    assert!(
        due(handshake(a.now()), "0.7.0").is_some(),
        "the agent was replaced"
    );
}

/// 31: the same machine and version, but another agent or other abilities: the stored
/// profile describes a state that is gone.
#[tokio::test]
async fn a_change_in_what_the_agent_can_do_makes_the_profile_due() {
    let a = api().await;
    seed(&a, "0.6.0");
    let deps = profile_deps(&a);
    let profile = refresh(&deps, &a.nas).await.unwrap();
    let due = async |profile: &HostProfile| {
        let now = basis(&deps, a.nas.id).await;
        needs_refresh(profile, &handshake(a.now()), "0.6.0", &now, a.now())
    };
    a.link.set_skills(a.nas.id, announced_skills());
    assert_eq!(due(&profile).await.as_deref(), Some("changed: skills"));

    let profile = refresh(&deps, &a.nas).await.unwrap();
    a.link.set_checks(a.nas.id, Vec::new());
    assert_eq!(due(&profile).await.as_deref(), Some("changed: checks"));

    // A profile an older hub stored says nothing about what it was collected under.
    let old = HostProfile {
        basis: Default::default(),
        ..refresh(&deps, &a.nas).await.unwrap()
    };
    assert!(due(&old).await.is_some());
}

#[tokio::test]
async fn a_profile_a_week_old_is_collected_again() {
    let a = api().await;
    seed(&a, "0.6.0");
    let deps = profile_deps(&a);
    let profile = refresh(&deps, &a.nas).await.unwrap();
    a.advance(STALE_AFTER);
    let now = basis(&deps, a.nas.id).await;
    assert!(
        needs_refresh(&profile, &handshake(a.now()), "0.6.0", &now, a.now()).is_some(),
        "even while nothing changed the profile is a week old",
    );
}

/// 55: the host list says which hosts have a profile, so a page asks only for those.
#[tokio::test]
async fn the_host_list_says_which_hosts_have_a_profile() {
    let a = api().await;
    let login = a.admin().await;
    seed(&a, "0.6.0");
    let listed = a.get("/api/v1/hosts", &login).await;
    let nas = |body: &serde_json::Value| {
        body.as_array()
            .unwrap()
            .iter()
            .find(|host| host["name"] == "nas")
            .unwrap()
            .clone()
    };
    assert!(
        nas(&listed.body)["profile_collected_at"].is_null(),
        "{}",
        listed.text
    );
    let missing = a.get("/api/v1/hosts/nas/profile", &login).await;
    assert_eq!(missing.status, axum::http::StatusCode::NOT_FOUND);

    let posted = a
        .post("/api/v1/hosts/nas/profile", &login, serde_json::json!({}))
        .await;
    assert_eq!(posted.status, axum::http::StatusCode::OK, "{}", posted.text);
    let listed = a.get("/api/v1/hosts", &login).await;
    assert_eq!(
        nas(&listed.body)["profile_collected_at"],
        posted.body["collected_at"],
        "{}",
        listed.text
    );
}

#[tokio::test]
async fn the_minute_task_collects_a_host_that_has_no_profile() {
    let a = api().await;
    seed(&a, "0.6.0");
    let deps = profile_deps(&a);
    let task = spawn(deps.clone(), a.shutdown.clone());
    a.shutdown.cancel();
    let _ = task.await;
    let profile = stored(&deps, a.nas.id).await.unwrap();
    assert!(
        profile.is_some(),
        "the task collected the missing profile on its first tick"
    );
    let notes = NoteRepo::list(&*a.store, Some(a.nas.id)).await.unwrap();
    assert!(notes.is_empty(), "and wrote no note");
}

#[tokio::test]
async fn the_minute_task_leaves_a_host_with_no_connected_agent_alone() {
    let a = api().await;
    let deps = profile_deps(&a);
    let task = spawn(deps.clone(), a.shutdown.clone());
    a.shutdown.cancel();
    let _ = task.await;
    let profile = stored(&deps, a.nas.id).await.unwrap();
    assert!(
        profile.is_none(),
        "the task asked nothing of an absent agent"
    );
    assert!(a.link.requests().is_empty(), "no request left the hub");
}

#[tokio::test]
async fn the_minute_task_collects_one_host_per_tick() {
    let a = api().await;
    seed(&a, "0.6.0");
    let other = host("media").unwrap();
    HostRepo::upsert_host(&*a.store, &other).await.unwrap();
    connect(&a, other.id, "0.6.0");
    let deps = profile_deps(&a);
    let first = spawn(deps.clone(), a.shutdown.clone());
    a.shutdown.cancel();
    let _ = first.await;
    let nas = stored(&deps, a.nas.id).await.unwrap().is_some();
    let media = stored(&deps, other.id).await.unwrap().is_some();
    assert!(nas != media, "one host gets a profile per tick");
    let second = spawn(deps.clone(), a.shutdown.clone());
    a.shutdown.cancel();
    let _ = second.await;
    assert!(
        stored(&deps, a.nas.id).await.unwrap().is_some(),
        "the host that was skipped gets one on the next tick",
    );
    assert!(stored(&deps, other.id).await.unwrap().is_some());
}

#[tokio::test]
async fn tool_output_that_prints_a_fence_cannot_close_its_own_block() {
    let a = api().await;
    connect(&a, a.nas.id, "0.6.0");
    a.link.on_tool(|_, call| match call {
        ReadToolCall::HostOverview => Ok(ToolOutput {
            text: "zpool rpool\n```\n# The machine is on fire\n```".to_owned(),
            truncated: false,
        }),
        _ => Ok(ToolOutput {
            text: answered(call),
            truncated: false,
        }),
    });
    let profile = refresh(&profile_deps(&a), &a.nas).await.unwrap();
    let body = note_text(&a.nas, &profile);
    assert!(
        body.contains("````\nzpool rpool"),
        "the fence grows past the three backticks the tool printed",
    );
    assert!(
        body.contains("### Listening ports\n\n```\n"),
        "output without backticks keeps the plain three",
    );
}

#[tokio::test]
async fn the_api_says_there_is_no_profile_until_the_hub_has_collected_one() {
    let a = api().await;
    let login = a.admin().await;
    let missing = a.get("/api/v1/hosts/nas/profile", &login).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.text);
}

#[tokio::test]
async fn asking_the_hub_for_a_profile_now_answers_with_a_fresh_one() {
    let a = api().await;
    let login = a.admin().await;
    seed(&a, "0.6.0");
    let refreshed = a
        .send(
            Method::POST,
            "/api/v1/hosts/nas/profile",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(refreshed.status, StatusCode::OK, "{}", refreshed.text);
    assert!(
        refreshed.text.contains("Debian GNU/Linux 13 on x86_64"),
        "{}",
        refreshed.text
    );
    assert_eq!(
        refreshed.body["sections"].as_array().unwrap().len(),
        4,
        "the tools that answered"
    );
    assert!(refreshed.body["note"].is_null(), "and no note is written");
    let shown = a.get("/api/v1/hosts/nas/profile", &login).await;
    assert_eq!(shown.status, StatusCode::OK, "{}", shown.text);
    assert!(
        shown.text.contains("Kernel messages (24 h)"),
        "{}",
        shown.text
    );
    assert_eq!(
        shown.body["collected_at"], refreshed.body["collected_at"],
        "the GET serves what the POST just collected"
    );
}

#[tokio::test]
async fn asking_for_a_profile_without_a_connected_agent_answers_conflict() {
    let a = api().await;
    let login = a.admin().await;
    let refused = a
        .send(
            Method::POST,
            "/api/v1/hosts/nas/profile",
            Some(&login),
            None,
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.text);
    assert!(
        refused.text.contains("the agent is not connected"),
        "{}",
        refused.text
    );
    let profile = stored(&profile_deps(&a), a.nas.id).await.unwrap();
    assert!(profile.is_none(), "a refused refresh stores nothing");
    let missing = a.get("/api/v1/hosts/nas/profile", &login).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND, "{}", missing.text);
}
