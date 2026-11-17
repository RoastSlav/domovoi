//! Tests for the one read boundary: who may read which host, what is refused before a
//! request is built, and that the agent's output reaches the caller unchanged.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::ports::{HostAvailability, Presence, ToolOutput};
use domovoi_core::{HostId, ReadToolCall, Risk, SkillName, SkillSummary};
use domovoi_testkit::build::{T0, manifest, open_incident};
use domovoi_testkit::link::{LinkRequest, evidence_caps, observer_of};
use domovoi_testkit::{FakeAgentLink, FakeClock};
use jiff::SignedDuration;

use super::{Evidence, EvidenceError, EvidenceRequest, HostCatalog};

/// The deadline every request here uses; the fake link records it but never enforces it.
fn deadline() -> jiff::Timestamp {
    T0.checked_add(SignedDuration::from_secs(15)).unwrap()
}

/// The read boundary over the fake link, with a clock at [`T0`].
fn evidence(link: &Arc<FakeAgentLink>) -> Evidence {
    Evidence::new(evidence_caps(link.clone()), Arc::new(FakeClock::new(T0)))
}

/// The read boundary over the fake link and a clock the test moves.
fn evidence_at(link: &Arc<FakeAgentLink>, clock: Arc<FakeClock>) -> Evidence {
    Evidence::new(evidence_caps(link.clone()), clock)
}

/// Whether any read reached the agent. An availability check is not a request here.
fn read_reached_the_agent(link: &FakeAgentLink) -> bool {
    link.requests()
        .iter()
        .any(|request| matches!(request, LinkRequest::Tool { .. }))
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

/// A write skill on the host, as the agent would have announced it.
fn write_skill(name: &str) -> SkillSummary {
    manifest(name, Risk::Write)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"write entry"))
}

/// The three ways a host can be unavailable, with the presence and availability that say
/// so and the error the boundary must answer with. `Absent` and `Unknown` share an error
/// but stay separate cases: a host seen before and one never seen are different states.
fn unavailable_states() -> Vec<(Presence, HostAvailability, EvidenceError)> {
    vec![
        (
            Presence::Disconnected { last_seen: None },
            HostAvailability::Unknown,
            EvidenceError::NotConnected,
        ),
        (
            Presence::Disconnected {
                last_seen: Some(T0),
            },
            HostAvailability::Absent { since: T0 },
            EvidenceError::NotConnected,
        ),
        (
            Presence::Connected {
                since: T0,
                version: "0.1.0".to_owned(),
            },
            HostAvailability::Incompatible {
                reported: "0.9.0".to_owned(),
            },
            EvidenceError::Incompatible,
        ),
    ]
}

/// Puts `host` in one of the unavailable states.
fn set_unavailable(
    link: &FakeAgentLink,
    host: HostId,
    state: &(Presence, HostAvailability, EvidenceError),
) {
    link.set_presence(host, state.0.clone());
    link.set_availability(host, state.1.clone());
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
    let evidence = evidence(&link);

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
async fn a_read_tool_answers_the_hosts_availability_first() {
    let link = Arc::new(FakeAgentLink::new());
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "unreachable".to_owned(),
            truncated: false,
        })
    });
    let evidence = evidence(&link);

    for state in unavailable_states() {
        let host = HostId::new();
        set_unavailable(&link, host, &state);
        let request = EvidenceRequest::for_host(host, ReadToolCall::HostOverview, deadline());
        assert_eq!(
            evidence.collect_evidence(request).await.unwrap_err(),
            state.2,
            "an unavailable host answers before a request is built"
        );
    }
    assert!(
        link.requests().is_empty(),
        "no request reached the link for an unavailable host"
    );
}

#[tokio::test]
async fn a_read_skill_is_not_validated_until_the_host_is_available() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    // A write skill would be refused for its own reason if enablement ran first, so this
    // test fails with the ordering it is here to protect.
    link.set_skills(host, vec![write_skill("restart_container")]);
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "ran".to_owned(),
            truncated: false,
        })
    });
    let evidence = evidence(&link);

    for state in unavailable_states() {
        set_unavailable(&link, host, &state);
        let call = ReadToolCall::RunReadSkill {
            skill: SkillName::parse("restart_container").unwrap(),
            args: BTreeMap::new(),
        };
        let request = EvidenceRequest::for_host(host, call, deadline());
        assert_eq!(
            evidence.collect_evidence(request).await.unwrap_err(),
            state.2,
            "availability answers before the read skill is validated"
        );
    }
    assert!(
        link.requests().is_empty(),
        "nothing was sent for an unavailable host"
    );
}

#[tokio::test]
async fn a_read_skill_must_be_installed_read_only_and_given_valid_arguments() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    link.set_skills(
        host,
        vec![read_skill("disk_report"), write_skill("restart_container")],
    );
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "ran".to_owned(),
            truncated: false,
        })
    });
    let evidence = evidence(&link);

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

    let extra = ReadToolCall::RunReadSkill {
        skill: SkillName::parse("disk_report").unwrap(),
        args: BTreeMap::from([("unexpected".to_owned(), "1".to_owned())]),
    };
    let refused = evidence
        .collect_evidence(EvidenceRequest::for_host(host, extra, deadline()))
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        EvidenceError::NotEnabled("unknown argument `unexpected`".to_owned()),
        "the manifest's own words, not a message written here"
    );
    assert!(
        link.requests().is_empty(),
        "none of the refusals reached the agent"
    );

    let read = ReadToolCall::RunReadSkill {
        skill: SkillName::parse("disk_report").unwrap(),
        args: BTreeMap::new(),
    };
    let out = evidence
        .collect_evidence(EvidenceRequest::for_host(host, read, deadline()))
        .await
        .expect("an installed read-only skill with valid arguments is enabled");
    assert_eq!(out.text, "ran");
    assert_eq!(
        link.requests().len(),
        1,
        "the enabled read reached the agent exactly once"
    );
}

#[test]
fn evidence_errors_keep_the_text_the_tools_already_returned() {
    assert_eq!(
        EvidenceError::NotConnected.to_string(),
        "agent not connected"
    );
    assert_eq!(
        EvidenceError::Incompatible.to_string(),
        "agent_incompatible: the agent speaks another protocol version; upgrade it"
    );
    assert_eq!(EvidenceError::Timeout.to_string(), "timed out");
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
    let evidence = evidence(&link);
    let catalog = HostCatalog::new(observer_of(link.clone()));

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

#[tokio::test]
async fn an_available_host_is_probed_then_read_once() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "up 3 days".to_owned(),
            truncated: false,
        })
    });
    let evidence = evidence(&link);

    let out = evidence
        .collect_evidence(EvidenceRequest::for_host(
            host,
            ReadToolCall::HostOverview,
            deadline(),
        ))
        .await
        .expect("an available host is read");
    assert_eq!(out.text, "up 3 days");
    assert_eq!(
        link.requests()
            .iter()
            .filter(|request| matches!(request, LinkRequest::Tool { .. }))
            .count(),
        1,
        "the read reached the agent exactly once, after the availability check"
    );
}

#[tokio::test]
async fn a_deadline_already_passed_answers_as_timed_out_without_asking() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "unreachable".to_owned(),
            truncated: false,
        })
    });
    let evidence = evidence(&link);

    let refused = evidence
        .collect_evidence(EvidenceRequest::for_host(
            host,
            ReadToolCall::HostOverview,
            T0,
        ))
        .await
        .unwrap_err();
    assert_eq!(refused, EvidenceError::Timeout);
    assert!(
        !read_reached_the_agent(&link),
        "a passed deadline does not reach the agent"
    );
}

#[tokio::test]
async fn an_availability_check_pending_past_the_deadline_answers_as_timed_out() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "read".to_owned(),
            truncated: false,
        })
    });
    // The check is held with no permit, so it stays pending until the clock moves.
    let entered = link.hold_availability(host, Arc::new(tokio::sync::Semaphore::new(0)));
    let clock = Arc::new(FakeClock::new(T0));
    let evidence = evidence_at(&link, clock.clone());
    let request = EvidenceRequest::for_host(host, ReadToolCall::HostOverview, deadline());

    let pending = tokio::spawn(async move { evidence.collect_evidence(request).await });
    entered.notified().await;
    clock.advance(SignedDuration::from_secs(16));

    let refused = pending.await.unwrap().unwrap_err();
    assert_eq!(refused, EvidenceError::Timeout);
    assert!(
        !read_reached_the_agent(&link),
        "the read never ran after the availability check passed its deadline"
    );
}

#[tokio::test]
async fn unavailable_and_incompatible_hosts_never_reach_the_read() {
    let link = Arc::new(FakeAgentLink::new());
    link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: "unreachable".to_owned(),
            truncated: false,
        })
    });
    let evidence = evidence(&link);

    for state in unavailable_states() {
        let host = HostId::new();
        set_unavailable(&link, host, &state);
        let refused = evidence
            .collect_evidence(EvidenceRequest::for_host(
                host,
                ReadToolCall::HostOverview,
                deadline(),
            ))
            .await
            .unwrap_err();
        assert_eq!(refused, state.2, "availability answers before the read");
    }
    assert!(
        !read_reached_the_agent(&link),
        "run_tool is never reached for an unavailable or incompatible host"
    );
}

/// A read capability that follows the deadline contract the production adapter implements:
/// it holds the read behind a stalled lookup, as `WsAgentLink` does when a session vanished
/// and the store is slow, and gives up with `Timeout` when the clock reaches the deadline.
struct StalledFallbackRead {
    clock: Arc<FakeClock>,
    entered: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl domovoi_core::ports::HostRead for StalledFallbackRead {
    async fn read_host(
        &self,
        _host: HostId,
        _call: ReadToolCall,
        deadline: jiff::Timestamp,
    ) -> Result<ToolOutput, domovoi_core::ports::LinkError> {
        use domovoi_core::ports::Clock;
        self.entered.notify_one();
        tokio::select! {
            () = std::future::pending::<()>() => unreachable!("the lookup never answers"),
            () = self.clock.sleep_until(deadline) => Err(domovoi_core::ports::LinkError::Timeout),
        }
    }
}

#[tokio::test]
async fn a_read_stalled_after_availability_answers_timeout_not_a_late_result() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    let clock = Arc::new(FakeClock::new(T0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let mut caps = evidence_caps(link.clone());
    caps.read = Arc::new(StalledFallbackRead {
        clock: clock.clone(),
        entered: entered.clone(),
    });
    let evidence = Evidence::new(caps, clock.clone());
    let request = EvidenceRequest::for_host(host, ReadToolCall::HostOverview, deadline());

    let pending = tokio::spawn(async move { evidence.collect_evidence(request).await });
    entered.notified().await;
    assert!(!pending.is_finished(), "the read is still stalled");
    clock.advance(SignedDuration::from_secs(16));
    assert_eq!(pending.await.unwrap().unwrap_err(), EvidenceError::Timeout);
    assert!(
        !read_reached_the_agent(&link),
        "the replaced read capability, not the fake agent, answered"
    );
}

#[tokio::test]
async fn a_deadline_already_passed_calls_no_capability() {
    let link = Arc::new(FakeAgentLink::new());
    let host = HostId::new();
    connected(&link, host);
    let clock = Arc::new(FakeClock::new(T0));
    clock.advance(SignedDuration::from_secs(16));
    let evidence = evidence_at(&link, clock);
    let request = EvidenceRequest::for_host(host, ReadToolCall::HostOverview, deadline());
    assert_eq!(
        evidence.collect_evidence(request).await.unwrap_err(),
        EvidenceError::Timeout
    );
    assert!(link.requests().is_empty(), "no capability was asked");
}

#[tokio::test]
async fn link_failures_before_the_deadline_keep_their_classification() {
    use domovoi_core::ports::LinkError;
    for (failure, expected) in [
        (LinkError::NotConnected, EvidenceError::NotConnected),
        (LinkError::Incompatible, EvidenceError::Incompatible),
        (LinkError::Timeout, EvidenceError::Timeout),
    ] {
        let link = Arc::new(FakeAgentLink::new());
        let host = HostId::new();
        connected(&link, host);
        link.on_tool(move |_, _| Err(failure.clone()));
        let request = EvidenceRequest::for_host(host, ReadToolCall::HostOverview, deadline());
        assert_eq!(
            evidence(&link).collect_evidence(request).await.unwrap_err(),
            expected
        );
    }
}
