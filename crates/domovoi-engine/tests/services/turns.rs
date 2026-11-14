use std::time::Duration;

use domovoi_core::ports::{
    IncidentRepo, LlmResponse, Part, StopReason, ToolOutput, Usage, UsageRepo,
};
use domovoi_core::{ReadToolCall, TriageBudget, TriageOutcome};
use domovoi_engine::triage::{BUDGET_EXHAUSTED, MAX_RESULT_BYTES, TriageSettings};
use domovoi_testkit::link::LinkRequest;
use jiff::SignedDuration;
use jiff::tz::TimeZone;
use serde_json::json;

use crate::harness::{SECRET, Setup, calls, host_id, report, restart_skill, results, world};

/// A response that ended at the output limit without saying anything, as a reasoning model
/// that spent the whole allowance thinking does.
fn reasoning_only() -> LlmResponse {
    truncated(Vec::new())
}

/// A turn cut off at the output limit, carrying whatever it had produced so far.
fn truncated(parts: Vec<Part>) -> LlmResponse {
    LlmResponse {
        parts,
        stop: StopReason::MaxTokens,
        usage: Usage::default(),
    }
}

#[tokio::test]
async fn tool_use_is_recorded_on_the_timeline() {
    let w = world(Setup::default()).await;
    w.primary
        .push_ok(calls(&[("c1", "journal", json!({"unit": "nginx"}))]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let entries = w.store.timeline(w.incident.id()).await.expect("timeline");
    let entry = entries
        .iter()
        .find(|e| e.kind == "triage.tools")
        .expect("a triage.tools entry");
    assert_eq!(entry.summary, "triage used journal");
    assert_eq!(entry.detail["during"], "triage");
    assert_eq!(entry.detail["tools"][0]["tool"], "journal");
    assert_eq!(entry.detail["tools"][0]["failed"], false);
}

#[tokio::test]
async fn failed_skill_call_keeps_the_skill_name() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.primary.push_ok(calls(&[(
        "c1",
        "run_read_skill",
        json!({"skill": "restart_container", "bogus": 1}),
    )]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let entries = w.store.timeline(w.incident.id()).await.expect("timeline");
    let entry = entries
        .iter()
        .find(|e| e.kind == "triage.tools")
        .expect("a triage.tools entry");
    assert_eq!(entry.summary, "triage used restart_container");
    assert_eq!(entry.detail["tools"][0]["skill"], "restart_container");
    assert_eq!(entry.detail["tools"][0]["failed"], true);
}

#[tokio::test]
async fn prefetched_tools_are_recorded_for_a_provider_without_tools() {
    let w = world(Setup {
        cli: true,
        ..Setup::default()
    })
    .await;
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let entries = w.store.timeline(w.incident.id()).await.expect("timeline");
    let entry = entries
        .iter()
        .find(|e| e.kind == "triage.tools")
        .expect("a triage.tools entry");
    assert_eq!(
        entry.summary,
        "triage used docker_ps, docker_logs, journal, host_overview"
    );
    assert_eq!(entry.detail["tools"][0]["tool"], "docker_ps");
    assert_eq!(entry.detail["tools"][0]["failed"], false);
}

#[tokio::test]
async fn model_invented_tool_names_are_folded_into_unknown_tool() {
    let w = world(Setup::default()).await;
    let long_name = "j".repeat(10_000);
    w.primary.push_ok(calls(&[
        ("c1", "approve_action", json!({})),
        ("c2", long_name.as_str(), json!({})),
    ]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let entries = w.store.timeline(w.incident.id()).await.expect("timeline");
    let entry = entries
        .iter()
        .find(|e| e.kind == "triage.tools")
        .expect("a triage.tools entry");
    assert_eq!(entry.summary, "triage used unknown tool ×2");
    assert_eq!(entry.detail["tools"][0]["tool"], "unknown tool");
    assert!(
        entry.detail["tools"][0]["skill"].is_null(),
        "no skill attribution"
    );
    assert_eq!(entry.detail["tools"][0]["raw"], "approve_action");
    assert_eq!(entry.detail["tools"][1]["tool"], "unknown tool");
    assert_eq!(
        entry.detail["tools"][1]["raw"],
        "j".repeat(96),
        "cut to a fixed length"
    );
}

#[tokio::test]
async fn budget_refused_calls_are_recorded_as_not_run() {
    let settings = TriageSettings {
        budget: TriageBudget {
            max_tool_calls: 1,
            max_seconds: 90,
            max_output_tokens: 2000,
            max_total_output_tokens: 32_000,
        },
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_ok(calls(&[
        ("c1", "journal", json!({"unit": "nginx"})),
        (
            "c2",
            "docker_logs",
            json!({"container": "nginx", "lines": 100}),
        ),
    ]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let entries = w.store.timeline(w.incident.id()).await.expect("timeline");
    let entry = entries
        .iter()
        .find(|e| e.kind == "triage.tools")
        .expect("a triage.tools entry");
    assert_eq!(entry.summary, "triage used journal, docker_logs");
    assert_eq!(entry.detail["tools"][0]["refused"], false);
    assert_eq!(entry.detail["tools"][1]["refused"], true);
    assert_eq!(
        entry.detail["tools"][1]["failed"], false,
        "a refused call did not fail"
    );
}

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
async fn reasoning_only_answer_is_retried_with_a_larger_allowance() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(reasoning_only());
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].max_output_tokens, 2000);
    assert_eq!(
        requests[1].max_output_tokens, 8000,
        "the turn gets four times the configured allowance"
    );
    assert_eq!(
        requests[1].messages.len(),
        1,
        "the assistant turn that said nothing is not kept"
    );
}

#[tokio::test]
async fn a_truncated_answer_is_retried_with_more_room() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(truncated(vec![Part::Text(
        "{\"headline\": \"Disk is".to_owned(),
    )]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2, "the cut-off answer is not repaired");
    assert_eq!(requests[1].max_output_tokens, 8000);
    assert_eq!(
        requests[1].messages.len(),
        1,
        "the cut-off answer is not kept in the conversation"
    );
}

#[tokio::test]
async fn a_truncated_tool_call_is_never_run() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(truncated(vec![Part::ToolCall {
        id: "call_cut".to_owned(),
        name: "journal".to_owned(),
        input: json!({"unit": "nginx"}),
    }]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].max_output_tokens, 8000);
    assert_eq!(
        requests[1].messages.len(),
        1,
        "the incomplete turn is not kept"
    );
    let entries = w.store.timeline(w.incident.id()).await.expect("timeline");
    assert!(
        !entries.iter().any(|e| e.kind == "triage.tools"),
        "a call that was not run is not recorded"
    );
}

#[tokio::test]
async fn a_truncated_text_and_tool_call_is_never_run() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(truncated(vec![
        Part::Text("I will check the journal.".to_owned()),
        Part::ToolCall {
            id: "call_cut".to_owned(),
            name: "journal".to_owned(),
            input: json!({"unit": "nginx"}),
        },
    ]));
    w.primary.push_text(report());

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].max_output_tokens, 8000);
    assert_eq!(
        requests[1].messages.len(),
        1,
        "neither the text nor the call of the incomplete turn is kept"
    );
}

#[tokio::test]
async fn a_model_that_never_answers_fails_with_the_output_allowance() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(reasoning_only());
    w.primary.push_ok(reasoning_only());
    w.primary.push_ok(reasoning_only());

    let outcome = w.run().await;
    assert!(
        matches!(
            &outcome,
            TriageOutcome::Failed { reason }
                if reason.contains("16000-token output allowance")
                    && reason.contains("per_run.output_tokens")
        ),
        "{outcome:?}"
    );
    assert_eq!(
        w.primary.requests().len(),
        3,
        "the allowance grows to the ceiling, then the run fails"
    );
}

#[tokio::test]
async fn a_run_that_spends_its_output_budget_stops() {
    let settings = TriageSettings {
        budget: TriageBudget {
            max_tool_calls: 15,
            max_seconds: 90,
            max_output_tokens: 2000,
            max_total_output_tokens: 2500,
        },
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    // The first turn asks for 2000; the retry is clipped to the 500 left.
    w.primary.push_ok(reasoning_only());
    w.primary.push_ok(reasoning_only());

    let outcome = w.run().await;
    assert!(
        matches!(
            &outcome,
            TriageOutcome::Failed { reason }
                if reason.contains("total_tokens") && reason.contains("output token budget")
        ),
        "{outcome:?}"
    );
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].max_output_tokens, 2000);
    assert_eq!(
        requests[1].max_output_tokens, 500,
        "a retry never asks for more than the run has left"
    );
}

#[tokio::test]
async fn every_retry_is_recorded_as_usage() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(LlmResponse {
        parts: Vec::new(),
        stop: StopReason::MaxTokens,
        usage: Usage {
            input_tokens: 10,
            output_tokens: 30,
            cost_micros: None,
        },
    });
    w.primary.push_ok(LlmResponse {
        parts: vec![Part::Text(report())],
        stop: StopReason::EndTurn,
        usage: Usage {
            input_tokens: 20,
            output_tokens: 70,
            cost_micros: None,
        },
    });

    assert!(matches!(w.run().await, TriageOutcome::Report(_)));

    let month = w.now().to_zoned(TimeZone::UTC).date();
    let total = w.store.month_total(month).await.expect("usage");
    assert_eq!(
        total.tokens_out, 100,
        "the cut-off retry and the answer both count"
    );
    assert_eq!(total.tokens_in, 30);
}

#[tokio::test]
async fn an_allowance_at_the_ceiling_is_not_grown() {
    let settings = TriageSettings {
        budget: TriageBudget {
            max_tool_calls: 15,
            max_seconds: 90,
            max_output_tokens: 16_000,
            max_total_output_tokens: 32_000,
        },
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;
    w.primary.push_ok(reasoning_only());

    let outcome = w.run().await;
    assert!(
        matches!(&outcome, TriageOutcome::Failed { reason } if reason.contains("16000-token output allowance")),
        "{outcome:?}"
    );
    assert_eq!(w.primary.requests().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_slow_model_is_cut_off_at_the_run_deadline() {
    let w = world(Setup::default()).await;
    // The provider never answers: the request must still end when the run's time is up.
    w.primary.hold();
    let advance = async {
        tokio::time::advance(Duration::from_secs(91)).await;
    };
    let (outcome, ()) = tokio::join!(w.run(), advance);
    assert!(
        matches!(&outcome, TriageOutcome::Failed { reason } if reason.contains("run deadline")),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_deadline_that_has_passed_sends_no_request() {
    let settings = TriageSettings {
        budget: TriageBudget {
            max_tool_calls: 15,
            max_seconds: 0,
            max_output_tokens: 2000,
            max_total_output_tokens: 32_000,
        },
        ..TriageSettings::default()
    };
    let w = world(Setup {
        settings,
        ..Setup::default()
    })
    .await;

    let outcome = w.run().await;
    assert!(
        matches!(&outcome, TriageOutcome::Failed { reason } if reason.contains("run deadline")),
        "{outcome:?}"
    );
    assert_eq!(w.primary.requests().len(), 0);
}

#[tokio::test]
async fn budget_exhaustion_forces_final_answer() {
    let settings = TriageSettings {
        budget: TriageBudget {
            max_tool_calls: 2,
            max_seconds: 90,
            max_output_tokens: 500,
            max_total_output_tokens: 8000,
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
}

#[tokio::test]
async fn tool_work_past_the_deadline_ends_the_run() {
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

    let outcome = w.run().await;
    assert!(
        matches!(&outcome, TriageOutcome::Failed { reason } if reason.contains("run deadline")),
        "{outcome:?}"
    );
    assert_eq!(
        w.primary.requests().len(),
        2,
        "no further request after the run's time is gone"
    );
}
