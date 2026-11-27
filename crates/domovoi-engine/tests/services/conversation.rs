use domovoi_core::ports::{IncidentRepo, LlmError, LlmRequest, Part};
use domovoi_core::{TriageOutcome, UserId};
use domovoi_engine::triage::MAX_ANSWER_CHARS;
use serde_json::json;

use crate::harness::{Setup, host_id, read_calls, report, restart_skill, results, world};

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
async fn the_follow_up_context_names_the_report() {
    let w = world(Setup::default()).await;
    w.primary.push_text(report());
    let TriageOutcome::Report(report) = w.run().await else {
        panic!("a report");
    };
    assert_eq!(report.cause, "A bad config file, fairly sure.");

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
}

/// A follow-up answer that ends in a `PROPOSALS:` line is shown as it is and creates nothing.
#[tokio::test]
async fn follow_up_answer_never_creates_an_action() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let block = json!([
        {"skill": "restart_container", "args": {"container": "nginx"}, "reason": "It is stuck."}
    ]);
    let text = format!(
        "The config file is bad.
PROPOSALS: {block}"
    );
    w.primary.push_text(text.clone());

    let answer = w
        .triage
        .follow_up(w.incident.id(), "What should we do?", UserId::new())
        .await
        .unwrap();
    assert_eq!(answer, text, "the answer is the model's text, unchanged");
    assert!(
        w.operations().await.is_empty(),
        "no operation or step was created"
    );
}

#[tokio::test]
async fn a_malformed_proposals_block_is_plain_text() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    w.primary.push_text(
        "Nothing to do.
PROPOSALS: restart_container(container=nginx)",
    );

    let answer = w
        .triage
        .follow_up(w.incident.id(), "Anything else?", UserId::new())
        .await
        .unwrap();
    assert!(answer.contains("restart_container"), "{answer}");
}

#[tokio::test]
async fn a_long_follow_up_answer_with_a_proposals_line_keeps_the_answer_limit() {
    let w = world(Setup::default()).await;
    w.link.set_skills(host_id(), vec![restart_skill()]);
    let body = "The pump stalled. ".to_owned().repeat(MAX_ANSWER_CHARS);
    w.primary.push_text(format!(
        "{body}
PROPOSALS: [{{\"skill\": \"restart_container\", \"args\": {{\"container\": \"nginx\"}}, \"reason\": \"x\"}}]"
    ));

    let answer = w
        .triage
        .follow_up(w.incident.id(), "What should we do?", UserId::new())
        .await
        .unwrap();
    assert_eq!(answer.chars().count(), MAX_ANSWER_CHARS);
}

/// The triage service has no queue: questions and chat answer on their own, and nothing
/// else is started on the incident.
#[tokio::test]
async fn triage_service_without_a_queue_answers_questions_and_chat() {
    let w = world(Setup::default()).await;
    w.primary.push_text("It is the config.");
    w.primary.push_text("Disk is fine.");

    let answer = w
        .triage
        .follow_up(w.incident.id(), "Why?", UserId::new())
        .await
        .unwrap();
    assert_eq!(answer, "It is the config.");
    let chat = w
        .triage
        .chat(Some(host_id()), "One host.", &Vec::new(), "Is the disk ok?")
        .await
        .unwrap();
    assert_eq!(
        chat,
        format!("Disk is fine.{}", domovoi_engine::triage::NO_TOOL_FOOTER)
    );
    assert_eq!(w.primary.requests().len(), 2, "one model call each");
    assert!(w.operations().await.is_empty(), "no investigation started");
}

/// 42: an answer that says something is not installed, with no `host_binary` result behind
/// it, does not lead with the claim; the model is first asked to check.
#[tokio::test]
async fn an_unbacked_claim_that_something_is_missing_never_leads_the_answer() {
    let w = world(Setup::default()).await;
    let claim = "It failed because iproute2 is not installed.";
    w.primary.push_text(claim);
    w.primary.push_text(claim);
    let chat = w
        .triage
        .chat(
            Some(host_id()),
            "One host.",
            &Vec::new(),
            "Why did it fail?",
        )
        .await
        .unwrap();
    assert!(
        chat.starts_with(domovoi_engine::triage::UNVERIFIED_LEAD),
        "{chat}"
    );
    let (before, after) = chat
        .split_once(domovoi_engine::triage::DRAFT_HEADING)
        .unwrap();
    assert!(!before.contains("is not installed"), "{chat}");
    assert!(after.contains(claim), "{chat}");
    let requests = w.primary.requests();
    assert_eq!(requests.len(), 2, "one prompt to check");
    assert!(shown(&requests[1]).contains("host_binary"));
}

/// 42d: a claim this turn's own check disproves is reported as checked and wrong, the model
/// is not told the check never ran, and the first answer of a conversation names no
/// earlier turn.
#[tokio::test]
async fn a_claim_this_turns_check_disproves_is_said_to_be_checked() {
    let w = world(Setup::default()).await;
    w.link.on_tool(|_, call| {
        Ok(domovoi_core::ports::ToolOutput {
            text: match call.tool().name() {
                "host_binary" => "file `ip`: found at /usr/sbin/ip.\n\
                    owner package: `iproute2` (dpkg lists /usr/sbin/ip).\n"
                    .to_owned(),
                other => format!("output of {other}"),
            },
            truncated: false,
        })
    });
    w.primary
        .push_ok(read_calls(&[("a", "host_binary", json!({"name": "ip"}))]));
    w.primary
        .push_text("It failed because iproute2 is not installed.");
    w.primary.push_text(
        "In my previous turn I called host_binary: ip is at /usr/sbin/ip. \
         The old claim that iproute2 is not installed is wrong.",
    );
    let chat = w
        .triage
        .chat(
            Some(host_id()),
            "One host.",
            &Vec::new(),
            "Is ip installed?",
        )
        .await
        .unwrap();
    assert_eq!(
        chat,
        "In this turn I called host_binary: ip is at /usr/sbin/ip. \
         The old claim that iproute2 is not installed is wrong."
    );
    let nudge = shown(&w.primary.requests()[2]);
    assert!(nudge.contains("still the same turn"), "{nudge}");
    assert!(nudge.contains("already found `iproute2`"), "{nudge}");
    assert!(!nudge.contains("no `host_binary` result"), "{nudge}");
}

/// 42: a Docker read that names a systemd unit is refused and points to the journal.
#[tokio::test]
async fn a_docker_read_of_a_systemd_unit_is_refused() {
    let w = world(Setup::default()).await;
    w.primary.push_ok(read_calls(&[(
        "a",
        "docker_logs",
        json!({"container": "domovoi-helper-selftest.service"}),
    )]));
    w.primary.push_text("I did not check.");
    w.triage
        .chat(
            Some(host_id()),
            "One host.",
            &Vec::new(),
            "Why did it fail?",
        )
        .await
        .unwrap();
    let answered = results(&w.primary.requests()[1]);
    assert!(answered[0].1, "an error result: {answered:?}");
    assert!(
        answered[0].0.contains("is a systemd unit, not a container"),
        "{answered:?}"
    );
}
