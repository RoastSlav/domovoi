//! The model's read path: every host read goes through the one evidence boundary, on the
//! host the call was scoped to, and the agent's text reaches the model unchanged.

use domovoi_core::ReadToolCall;
use domovoi_core::TriageOutcome;
use domovoi_core::ports::{HostAvailability, ToolOutput};
use domovoi_core::{Risk, SkillSummary};
use domovoi_testkit::build::manifest;
use domovoi_testkit::link::LinkRequest;
use serde_json::json;

use crate::harness::{Mode, Setup, calls, host_id, report, results, world};

/// A read skill the host announced, as the agent would list it.
fn read_skill() -> SkillSummary {
    manifest("disk_report", Risk::Read)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"read entry"))
}

/// The requests that would run a read skill on the agent: the calls this file refuses must
/// not appear here. Triage also reads host context through the same boundary, so a test
/// about one model call looks for that call rather than for an empty list.
fn read_skill_requests(w: &crate::harness::World) -> Vec<LinkRequest> {
    w.link
        .requests()
        .into_iter()
        .filter(|request| {
            matches!(
                request,
                LinkRequest::Tool {
                    call: ReadToolCall::RunReadSkill { .. },
                    ..
                } | LinkRequest::Skill { .. }
            )
        })
        .collect()
}

#[tokio::test]
async fn the_triage_tool_path_reads_through_the_boundary() {
    let w = world(Setup::default()).await;
    w.link.on_tool(|_, call| {
        Ok(ToolOutput {
            text: format!("text of {}", call.tool().name()),
            truncated: false,
        })
    });
    w.primary
        .push_ok(calls(&[("c1", "docker_ps", json!({"all": true}))]));
    w.primary.push_text(report());
    assert!(matches!(
        w.run(Mode::Legacy).await,
        TriageOutcome::Report(_)
    ));

    // The agent's output reached the model as it arrived: no extra formatting here.
    let shown = results(&w.primary.requests()[1]);
    assert!(
        shown
            .iter()
            .any(|(content, is_error)| !is_error && content == "text of docker_ps"),
        "the model saw the read: {shown:?}"
    );

    let reads: Vec<LinkRequest> = w
        .link
        .requests()
        .into_iter()
        .filter(|request| matches!(request, LinkRequest::Tool { .. }))
        .collect();
    for request in &reads {
        match request {
            LinkRequest::Tool { host, .. } => {
                assert_eq!(*host, host_id(), "a read reached another host");
            }
            _ => unreachable!("filtered to tool requests"),
        }
    }
    let asked = reads
        .iter()
        .filter(|request| {
            matches!(
                request,
                LinkRequest::Tool {
                    call: ReadToolCall::DockerPs { all: true },
                    ..
                }
            )
        })
        .count();
    assert_eq!(asked, 1, "the model's one read reached the agent once");
}

/// The model's read tool answers the host's availability, not a skill's enablement: a host
/// that is gone is not reported as one missing a skill.
#[tokio::test]
async fn an_unavailable_host_reaches_the_model_as_not_connected() {
    let w = world(Setup::default()).await;
    w.link
        .set_availability(host_id(), HostAvailability::Absent { since: w.now() });
    // A write skill would be refused for its own reason if enablement ran first.
    w.link
        .set_skills(host_id(), vec![crate::harness::restart_skill()]);
    w.primary.push_ok(calls(&[(
        "c1",
        "run_read_skill",
        json!({"skill": "restart_container", "args": {}}),
    )]));
    w.primary.push_text(report());
    assert!(matches!(
        w.run(Mode::Legacy).await,
        TriageOutcome::Report(_)
    ));

    let shown = results(&w.primary.requests()[1]);
    assert!(
        shown
            .iter()
            .any(|(content, is_error)| *is_error && content == "error: agent not connected"),
        "the model was told the host is away: {shown:?}"
    );
    assert!(
        w.link.requests().is_empty(),
        "nothing left the hub for an unavailable host"
    );
}

/// A read skill is validated against the host's own catalogue and the arguments its
/// manifest declares; a call the skill cannot accept never leaves the hub.
#[tokio::test]
async fn a_read_skill_with_unexpected_arguments_never_reaches_the_agent() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![read_skill()]);
    w.primary.push_ok(calls(&[(
        "c1",
        "run_read_skill",
        json!({"skill": "disk_report", "args": {"unexpected": "1"}}),
    )]));
    w.primary.push_text(report());
    assert!(matches!(
        w.run(Mode::Legacy).await,
        TriageOutcome::Report(_)
    ));

    let shown = results(&w.primary.requests()[1]);
    assert!(
        shown
            .iter()
            .any(|(content, is_error)| *is_error
                && content == "error: unknown argument `unexpected`"),
        "the manifest's own words reached the model: {shown:?}"
    );
    assert!(
        read_skill_requests(&w).is_empty(),
        "an invalid read skill call never reached the agent"
    );
}
