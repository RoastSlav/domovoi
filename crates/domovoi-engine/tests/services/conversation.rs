use domovoi_core::ports::{IncidentRepo, LlmRequest, Part};
use domovoi_core::{TriageOutcome, UserId};
use serde_json::json;

use crate::harness::{Setup, host_id, report_json, restart_skill, world};

/// Every text the model was shown in one request, in order.
fn shown(req: &LlmRequest) -> String {
    req.messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.to_owned()),
            _ => None,
        })
        .collect::<Vec<String>>()
        .join("\n")
}

#[tokio::test]
async fn a_second_question_remembers_the_first_question_and_answer() {
    let w = world(Setup::default()).await;
    w.primary.push_text("The config file is bad, fairly sure.");
    w.primary.push_text("Yes, the restart helped.");

    w.triage
        .follow_up(w.incident.id(), "Why does nginx crash?", UserId::new())
        .await
        .unwrap();
    w.triage
        .follow_up(w.incident.id(), "Did the restart help?", UserId::new())
        .await
        .unwrap();

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2, "the model answered each question once");
    let second = shown(&requests[1]);
    assert!(
        second.contains("Why does nginx crash?"),
        "the first question is sent back to the model: {}",
        second
    );
    assert!(
        second.contains("The config file is bad, fairly sure."),
        "the first answer is sent back to the model: {}",
        second
    );
    assert!(
        second.contains("Did the restart help?"),
        "the new question is the last message: {}",
        second
    );
}

#[tokio::test]
async fn a_clear_forgets_the_earlier_questions() {
    let w = world(Setup::default()).await;
    w.primary.push_text("One.");
    w.primary.push_text("Two.");
    w.primary.push_text("Three.");

    w.triage
        .follow_up(w.incident.id(), "First question?", UserId::new())
        .await
        .unwrap();
    w.triage
        .follow_up(w.incident.id(), "Second question?", UserId::new())
        .await
        .unwrap();
    w.manager
        .clear_follow_ups(w.incident.id(), UserId::new())
        .await
        .unwrap();
    w.triage
        .follow_up(w.incident.id(), "Third question?", UserId::new())
        .await
        .unwrap();

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 3, "one request per question");
    let last = shown(&requests[2]);
    assert!(
        last.contains("Third question?"),
        "the new question is asked: {last}"
    );
    assert!(
        !last.contains("First question?") && !last.contains("Second question?"),
        "after the clear the model is not shown the earlier questions: {last}"
    );

    let timeline = IncidentRepo::timeline(&*w.store, w.incident.id())
        .await
        .unwrap();
    assert!(
        timeline
            .iter()
            .any(|e| e.kind == domovoi_engine::triage::CLEAR_KIND),
        "the clear is a timeline entry of its own"
    );
}

#[tokio::test]
async fn the_follow_up_context_names_the_report_and_the_action_on_record() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.primary.push_text(report_json(
        "ACTION_NEEDED",
        json!([
            {"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}
        ]),
    ));
    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert_eq!(report.cause, "A bad config file, fairly sure.");
    assert_eq!(
        w.actions().await.len(),
        1,
        "the proposal became an action on record"
    );

    w.primary.push_text("Nothing new to add.");
    w.triage
        .follow_up(w.incident.id(), "What is the plan?", UserId::new())
        .await
        .unwrap();

    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2, "the triage run, then the follow-up");
    let Part::Text(opening) = &requests[1].messages[0].parts[0] else {
        panic!("the first message is text");
    };
    assert!(
        opening.contains(report.cause.as_str()),
        "the incident's own report is in the follow-up context: {opening}"
    );
    assert!(
        opening.contains("restart_container container=nginx status: proposed"),
        "the action on record is named with its arguments and status: {opening}"
    );
}
