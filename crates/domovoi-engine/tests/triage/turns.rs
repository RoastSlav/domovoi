use domovoi_core::ports::{Part, ToolOutput};
use domovoi_core::{ReadToolCall, TriageBudget, TriageOutcome};
use domovoi_engine::triage::{BUDGET_EXHAUSTED, MAX_RESULT_BYTES, TriageSettings};
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;
use serde_json::json;

use crate::harness::{SECRET, Setup, calls, host_id, report, results, world};

#[tokio::test]
async fn tool_calls_are_routed_to_agent_and_store() {
    let w = world(Setup::default()).await;
    w.note("nginx config", "nginx reads /etc/nginx/conf.d on start")
        .await;
    w.primary.push_ok(calls(&[(
        "c1",
        "docker_logs",
        json!({"container": "nginx"}),
    )]));
    w.primary.push_ok(calls(&[
        ("c2", "notes_search", json!({"query": "nginx"})),
        ("c3", "incident_history", json!({})),
    ]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let agent_calls: Vec<ReadToolCall> = w
        .link
        .requests()
        .into_iter()
        .filter_map(|r| match r {
            LinkRequest::Tool { host, call, .. } => {
                assert_eq!(host, host_id());
                Some(call)
            }
            _ => None,
        })
        .collect();
    assert!(agent_calls.contains(&ReadToolCall::DockerLogs {
        container: "nginx".to_owned(),
        lines: 100
    }));
    assert!(
        agent_calls.iter().all(|c| !matches!(
            c,
            ReadToolCall::NotesSearch { .. } | ReadToolCall::IncidentHistory { .. }
        )),
        "store tools never reach the agent"
    );

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(
        results(&requests[1]),
        vec![("output of docker_logs".to_owned(), false)]
    );
    let store_results = results(&requests[2]);
    assert!(
        store_results[0].0.contains("nginx config"),
        "{store_results:?}"
    );
    assert_eq!(
        store_results[1].0, "No earlier incidents on this host.",
        "the incident itself is not its own history"
    );
}

#[tokio::test]
async fn tool_results_are_truncated_and_redacted() {
    let w = world(Setup::default()).await;
    w.link.on_tool(|_, _| {
        Ok(ToolOutput {
            text: format!(
                "Authorization: Bearer abc.def-123 password={SECRET}\n{}",
                "log line\n".repeat(3000)
            ),
            truncated: false,
        })
    });
    w.primary
        .push_ok(calls(&[("c1", "journal", json!({"unit": "nginx"}))]));
    w.primary.push_text(report());
    w.run().await;

    let (content, is_error) = results(&w.primary.requests()[1]).remove(0);
    assert!(!is_error);
    assert!(content.len() <= MAX_RESULT_BYTES, "{}", content.len());
    assert!(content.ends_with("[output cut]"));
    assert!(!content.contains(SECRET));
    assert!(!content.contains("abc.def-123"));
    assert!(content.starts_with("Authorization: [REDACTED] password=[REDACTED]"));
}

#[tokio::test]
async fn invalid_json_gets_one_repair_turn_then_fails() {
    let w = world(Setup::default()).await;
    w.primary.push_text("The disk is full, I think.");
    w.primary.push_text(r#"{"headline": ""}"#);
    let outcome = w.run().await;
    assert!(
        matches!(&outcome, TriageOutcome::Failed { reason } if reason.contains("no usable report")),
        "{outcome:?}"
    );
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2, "exactly one repair turn");
    let repair = requests[1].messages.last().unwrap();
    assert!(matches!(&repair.parts[0], Part::Text(t) if t.contains("could not be used")));

    let w = world(Setup::default()).await;
    w.primary.push_text("```json\n{\"headline\": 3}\n```");
    w.primary
        .push_text(format!("Here you go:\n```json\n{}\n```", report()));
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
}

#[tokio::test]
async fn budget_exhaustion_forces_final_answer() {
    let settings = TriageSettings {
        budget: TriageBudget {
            max_tool_calls: 2,
            max_seconds: 90,
            max_output_tokens: 500,
        },
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary
        .push_ok(calls(&[("c1", "host_overview", json!({}))]));
    w.primary.push_ok(calls(&[
        ("c2", "docker_ps", json!({})),
        ("c3", "listening_ports", json!({})),
    ]));
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].max_output_tokens, 500);
    let last = requests[2].messages.last().unwrap();
    assert!(matches!(
        &last.parts[1],
        Part::ToolResult { is_error: true, .. }
    ));
    assert!(matches!(last.parts.last(), Some(Part::Text(t)) if t == BUDGET_EXHAUSTED));
    let tools_run = w
        .link
        .requests()
        .iter()
        .filter(|r| matches!(r, LinkRequest::Tool { call, .. } if !matches!(call, ReadToolCall::DockerEvents { .. })))
        .count();
    assert_eq!(tools_run, 2, "the third call was not run");

    // Time runs out too: each tool call takes 60 s of a 90 s budget, so the second call
    // ends it.
    let w = world(Setup::default()).await;
    let clock = w.clock.clone();
    w.link.on_tool(move |_, _| {
        clock.advance(SignedDuration::from_secs(60));
        Ok(ToolOutput {
            text: "ok".to_owned(),
            truncated: false,
        })
    });
    w.primary
        .push_ok(calls(&[("c1", "host_overview", json!({}))]));
    w.primary
        .push_ok(calls(&[("c2", "host_overview", json!({}))]));
    w.primary.push_text(report());
    assert!(matches!(w.run().await, TriageOutcome::Report(_)));
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 3);
    let last = requests[2].messages.last().unwrap();
    assert!(matches!(last.parts.last(), Some(Part::Text(t)) if t == BUDGET_EXHAUSTED));
}
