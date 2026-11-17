//! Tests for the one read boundary: who may read which host, what is refused before a
//! request is built, and that the agent's output reaches the caller unchanged.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{Presence, ToolOutput};
use domovoi_core::{HostId, ReadToolCall, Risk, SkillName, SkillSummary};
use domovoi_testkit::FakeAgentLink;
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;

use super::{Evidence, EvidenceError, EvidenceRequest, HostCatalog};

/// The deadline every request here uses; the fake link records it but never enforces it.
fn deadline() -> jiff::Timestamp {
    T0.checked_add(SignedDuration::from_secs(15)).unwrap()
}

fn connected(link: &FakeAgentLink, host: HostId) {
    link.set_presence(
        host,
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        },
    );
}

/// A read skill on the host, as the agent would have announced it.
fn read_skill(name: &str) -> SkillSummary {
    manifest(name, Risk::Read)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"read entry"))
}

#[tokio::test]
async fn evidence_collect_uses_the_scope_host() {
    let link = Arc::new(FakeAgentLink::new());
    let incident_host = HostId::new();
    let other = HostId::new();
    connected(&link, incident_host);
    connected(&link, other);
    link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: format!("output of {}", call.tool().name()),
            truncated: false,
        })
    });
    let incident = open_incident(incident_host, "docker.container", Some("nginx")).unwrap();
    let evidence = Evidence::new(link.clone());

    let request =
        EvidenceRequest::for_incident(&incident, ReadToolCall::DockerPs { all: true }, deadline());
    assert_eq!(request.host(), incident_host);
    let out = evidence.collect_evidence(request).await.unwrap();
    assert_eq!(out.text, "output of docker_ps");

    let hosts: Vec<HostId> = link
        .requests()
        .into_iter()
        .filter_map(|request| match request {
            LinkRequest::Tool { host, .. } => Some(host),
            _ => None,
        })
        .collect();
    assert_eq!(
        hosts,
        vec![incident_host],
        "only the incident's host is read"
    );
}

#[tokio::test]
async fn evidence_collect_returns_not_connected_for_an_absent_host() {
    let link = Arc::new(FakeAgentLink::new());
    let never_seen = HostId::new();
    let left = HostId::new();
    link.set_presence(
        left,
        Presence::Disconnected {
            last_seen: Some(T0),
        },
    );
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "unreachable".to_owned(),
            truncated: false,
        })
    });
    let evidence = Evidence::new(link.clone());

    for host in [never_seen, left] {
        let request = EvidenceRequest::for_host(host, ReadToolCall::HostOverview, deadline());
        assert_eq!(
            evidence.collect_evidence(request).await.unwrap_err(),
            EvidenceError::NotConnected,
            "an absent host answers before a request is built"
        );
    }
    assert!(
        link.requests().is_empty(),
        "no request reached the link for an absent host"
    );
}

#[tokio::test]
async fn evidence_collect_refuses_a_write_skill_as_a_read() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    link.set_skills(
        host,
        vec![
            read_skill("disk_report"),
            manifest("restart_container", Risk::Write)
                .unwrap()
                .summary(domovoi_core::ScriptDigest::of_bytes(b"write entry")),
        ],
    );
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "ran".to_owned(),
            truncated: false,
        })
    });
    let evidence = Evidence::new(link.clone());

    let write = ReadToolCall::RunReadSkill {
        skill: SkillName::parse("restart_container").unwrap(),
        args: BTreeMap::new(),
    };
    let refused = evidence
        .collect_evidence(EvidenceRequest::for_host(host, write, deadline()))
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        EvidenceError::NotEnabled("skill `restart_container` is not read-only".to_owned()),
        "the text is the one the tool returned before the read moved here"
    );

    let missing = ReadToolCall::RunReadSkill {
        skill: SkillName::parse("not_on_this_host").unwrap(),
        args: BTreeMap::new(),
    };
    let refused = evidence
        .collect_evidence(EvidenceRequest::for_host(host, missing, deadline()))
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        EvidenceError::NotEnabled(
            "skill `not_on_this_host` is not installed on this host".to_owned()
        )
    );
    assert!(link.requests().is_empty(), "nothing was sent to the agent");

    let read = ReadToolCall::RunReadSkill {
        skill: SkillName::parse("disk_report").unwrap(),
        args: BTreeMap::new(),
    };
    evidence
        .collect_evidence(EvidenceRequest::for_host(host, read, deadline()))
        .await
        .expect("a read-only skill is enabled");
    assert_eq!(link.requests().len(), 1);
}

#[tokio::test]
async fn evidence_collect_returns_the_agent_output_unchanged() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "  a line with trailing space  ".to_owned(),
            truncated: true,
        })
    });
    let evidence = Evidence::new(link.clone());
    let catalog = HostCatalog::new(link.clone());

    let out = evidence
        .collect_evidence(EvidenceRequest::for_host(
            host,
            ReadToolCall::HostOverview,
            deadline(),
        ))
        .await
        .unwrap();
    assert_eq!(
        out,
        ToolOutput {
            text: "  a line with trailing space  ".to_owned(),
            truncated: true,
        },
        "no redaction, cutting or note is added here"
    );
    assert_eq!(
        catalog.presence(host),
        Presence::Connected {
            since: T0,
            version: "0.1.0".to_owned(),
        }
    );
    assert!(catalog.snapshot(host).is_some());
    assert!(catalog.skills(host).is_empty());
}
