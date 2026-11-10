use domovoi_core::ports::{IncidentRepo, LlmError, LlmRequest, Part};
use domovoi_core::{ActionStatus, TriageOutcome, UserId};
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
async fn a_clear_while_a_question_is_running_drops_its_answer() {
    let w = world(Setup::default()).await;
    w.primary.hold();
    w.primary.push_text("Slow answer.");
    w.primary.push_text("Fresh answer.");

    let incident = w.incident.id();
    let triage = w.triage.clone();
    let running =
        tokio::spawn(async move { triage.follow_up(incident, "Running?", UserId::new()).await });

    // The provider has the request, so the question captured its conversation already.
    w.primary.wait_for_request().await;
    w.manager
        .clear_follow_ups(incident, UserId::new())
        .await
        .unwrap();
    w.primary.release(1);
    let answered = running.await.unwrap().unwrap();
    assert_eq!(
        answered,
        "Slow answer.".to_owned(),
        "the operator still gets the answer the model gave",
    );

    let timeline = IncidentRepo::timeline(&*w.store, incident).await.unwrap();
    assert!(
        timeline
            .iter()
            .any(|e| e.kind == domovoi_engine::triage::CLEAR_KIND),
        "the clear is a timeline entry of its own"
    );
    assert!(
        timeline
            .iter()
            .all(|e| e.kind != domovoi_engine::triage::FOLLOW_UP_KIND),
        "an answer finished after the clear is never recorded",
    );
    assert!(
        w.triage.history(incident).await.unwrap().is_empty(),
        "the conversation is empty after the clear",
    );

    w.triage
        .follow_up(incident, "Again?", UserId::new())
        .await
        .unwrap();
    let requests = w.primary.requests();
    assert_eq!(
        requests.len(),
        2,
        "one request for the cleared question, one for the new one"
    );
    let last = shown(&requests[1]);
    assert!(last.contains("Again?"), "the new question is asked: {last}");
    assert!(
        !last.contains("Running?") && !last.contains("Slow answer."),
        "an answer finished after the clear does not reappear in the next request: {last}",
    );
}

#[tokio::test]
async fn questions_answered_out_of_order_play_in_question_order() {
    let w = world(Setup {
        secondary: true,
        ..Setup::default()
    })
    .await;
    // The first question's provider fails on the primary and waits on the gated
    // secondary; the second question answers on the primary straight away, so it is
    // recorded first while it was asked later.
    w.primary.push_err(LlmError::Auth);
    w.primary.push_text("Quick answer for the second.");
    w.primary.push_text("Third.");
    w.secondary.push_text("Late answer for the first.");
    w.secondary.hold();

    let incident = w.incident.id();
    let triage = w.triage.clone();
    let running = tokio::spawn(async move {
        triage
            .follow_up(incident, "First asked?", UserId::new())
            .await
    });

    // The secondary holds the first question's request, so it cannot record yet.
    w.secondary.wait_for_request().await;
    w.clock.advance(jiff::SignedDuration::from_secs(1));
    let second = w
        .triage
        .follow_up(incident, "Second asked?", UserId::new())
        .await
        .unwrap();
    assert_eq!(
        second,
        "Quick answer for the second.".to_owned(),
        "the second question finished first, on the primary",
    );
    w.secondary.release(1);
    let first = running.await.unwrap().unwrap();
    assert_eq!(
        first,
        "Late answer for the first.".to_owned(),
        "the first question answers last, off the secondary",
    );

    let history = w.triage.history(incident).await.unwrap();
    assert_eq!(
        history
            .iter()
            .map(|t| t.question.clone())
            .collect::<Vec<String>>(),
        vec!["First asked?".to_owned(), "Second asked?".to_owned()],
        "the remembered turns follow the asking order, not the recording order",
    );

    w.triage
        .follow_up(incident, "Third asked?", UserId::new())
        .await
        .unwrap();
    let requests = w.primary.requests();
    assert_eq!(
        requests.len(),
        3,
        "three primary requests: the failed first, the second, the third"
    );
    let replay = shown(&requests[2]);
    let order = (
        replay.find("First asked?"),
        replay.find("Late answer for the first."),
        replay.find("Second asked?"),
        replay.find("Quick answer for the second."),
    );
    let new_question = replay.find("Third asked?");
    assert!(
        match order {
            (Some(a), Some(b), Some(c), Some(d)) =>
                a < b && b < c && c < d && Some(d) < new_question,
            _ => false,
        },
        "the third request replays question, answer, question, answer in asking order, \
            with the new question last: {replay}",
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

#[tokio::test]
async fn a_chat_answer_naming_a_proposal_stores_a_proposed_action() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let block = json!([
        {
            "skill": "restart_container",
            "args": {"container": "nginx"},
            "reason": "It is stuck."
        }
    ]);
    w.primary
        .push_text(format!("The config file is bad.\nPROPOSALS: {block}"));

    let answer = w
        .triage
        .follow_up(w.incident.id(), "What should we do?", UserId::new())
        .await
        .unwrap();
    assert!(
        answer.contains(
            "Proposed: restart_container (It is stuck.). Approve it on the Approvals page."
        ),
        "the answer names the proposal and where to approve it: {answer}"
    );
    assert!(
        !answer.contains("PROPOSALS:"),
        "the block is not shown to the operator: {answer}"
    );
    let actions = w.actions().await;
    assert_eq!(actions.len(), 1, "the chat proposal is on record");
    assert_eq!(actions[0].status(), &ActionStatus::Proposed);
    assert!(
        actions[0].approved_by().is_none(),
        "the proposal still waits for a human"
    );
}

#[tokio::test]
async fn a_chat_proposal_for_a_skill_the_host_lacks_is_reported_as_not_proposed() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let block = json!([{"skill": "wipe_disk", "args": {}, "reason": "The disk is full."}]);
    w.primary
        .push_text(format!("The disk is full.\nPROPOSALS: {block}"));

    let answer = w
        .triage
        .follow_up(w.incident.id(), "What should we do?", UserId::new())
        .await
        .unwrap();
    assert!(
        answer.contains("Not proposed: wipe_disk: not a skill on this host"),
        "the answer says why nothing was proposed: {answer}"
    );
    assert!(
        w.actions().await.is_empty(),
        "an unknown skill creates no action"
    );
}

#[tokio::test]
async fn a_malformed_proposals_block_is_ignored() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.primary
        .push_text("Nothing to do.\nPROPOSALS: restart_container(container=nginx)");

    let answer = w
        .triage
        .follow_up(w.incident.id(), "Anything else?", UserId::new())
        .await
        .unwrap();
    assert!(
        w.actions().await.is_empty(),
        "junk after the mark creates no action"
    );
    assert!(
        !answer.contains("restart_container"),
        "the junk block is cut from the answer: {answer}"
    );
}
