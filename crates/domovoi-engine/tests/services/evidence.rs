//! The model's read path: every host read goes through the one evidence boundary, on the
//! host the call was scoped to, and the agent's text reaches the model unchanged.

use domovoi_core::ReadToolCall;
use domovoi_core::TriageOutcome;
use domovoi_core::ports::ToolOutput;
use domovoi_testkit::link::LinkRequest;
use serde_json::json;

use crate::harness::{Setup, calls, host_id, report, results, world};

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
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

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
