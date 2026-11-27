//! Tests for the Operator chat helpers.

use super::{
    CHAT_PROMPT, ChatRole, ChatTurn, MAX_TURNS, TURN_BYTES, claims_absence, cut, remembered,
};

#[test]
fn a_bare_json_answer_becomes_readable_lines() {
    let text = super::plain_answer(r#"{"cause": "ip missing", "next_step": "check"}"#);
    assert_eq!(text, "- cause: ip missing\n- next step: check");
    assert_eq!(super::plain_answer("Plain prose."), "Plain prose.");
    assert_eq!(super::plain_answer("[1, 2]"), "[1, 2]");
}

#[test]
fn a_claim_that_something_is_missing_is_recognised() {
    assert!(claims_absence("The ip package is not recorded by dpkg."));
    assert!(claims_absence("iproute2 is not installed"));
    assert!(!claims_absence("The unit failed at 10:00."));
}

#[test]
fn the_chat_prompt_says_to_name_what_was_not_checked() {
    assert!(CHAT_PROMPT.contains("I did not check"));
}
use domovoi_core::ports::{LlmMessage, Part, Role};
use domovoi_testkit::ListRedactor;
use domovoi_testkit::redact::REDACTED;

fn turn(role: ChatRole, text: String) -> ChatTurn {
    ChatTurn { role, text }
}

#[test]
fn chat_turns_come_back_as_messages_of_their_own_speaker() {
    let quiet = ListRedactor::default();
    let history: Vec<ChatTurn> = vec![
        turn(ChatRole::User, "Why is the NAS slow?".to_owned()),
        turn(ChatRole::Assistant, "The disks are at 94%.".to_owned()),
    ];
    assert_eq!(
        remembered(&quiet, &history),
        vec![
            LlmMessage {
                role: Role::User,
                parts: vec![Part::Text("Why is the NAS slow?".to_owned())],
            },
            LlmMessage {
                role: Role::Assistant,
                parts: vec![Part::Text("The disks are at 94%.".to_owned())],
            },
        ],
        "the earlier question and answer are sent back in order",
    );
}

#[test]
fn the_oldest_turns_are_dropped_while_the_conversation_is_over_budget() {
    let quiet = ListRedactor::default();
    let wide = "x".repeat(TURN_BYTES).to_owned();
    let history: Vec<ChatTurn> = (0..9)
        .map(|_| turn(ChatRole::Assistant, wide.to_owned()))
        .collect();
    let kept = remembered(&quiet, &history);
    assert_eq!(
        kept.len(),
        8,
        "nine turns of the per-turn limit cannot all fit the budget"
    );
    assert_eq!(
        kept[0].parts,
        vec![Part::Text(wide)],
        "every kept turn still carries its full text",
    );
}

#[test]
fn an_oversized_history_turn_is_cut_and_never_kept_whole() {
    let quiet = ListRedactor::default();
    let long = "y".repeat(17 * 1024).to_owned();
    let history = vec![turn(ChatRole::Assistant, long.to_owned())];
    let kept = remembered(&quiet, &history);
    assert_eq!(
        kept.len(),
        1,
        "the newest turn is replayed even when it is over budget"
    );
    assert_eq!(
        kept[0].parts,
        vec![Part::Text(cut(&long, TURN_BYTES))],
        "but only ever cut to the per-turn limit, never whole",
    );
}

#[test]
fn only_the_newest_twenty_earlier_turns_are_replayed() {
    let quiet = ListRedactor::default();
    let history: Vec<ChatTurn> = (0..25)
        .map(|n| turn(ChatRole::Assistant, format!("Turn {n}.")))
        .collect();
    assert_eq!(
        remembered(&quiet, &history).len(),
        MAX_TURNS,
        "the five oldest turns are dropped",
    );
}

#[test]
fn a_secret_in_an_earlier_turn_is_redacted_before_the_model_sees_it() {
    let redactor = ListRedactor::new(["hunter2"]);
    let history: Vec<ChatTurn> = vec![
        turn(ChatRole::User, "My key is hunter2.".to_owned()),
        turn(ChatRole::Assistant, "Keep the hunter2 key safe.".to_owned()),
    ];
    let shown: Vec<String> = remembered(&redactor, &history)
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(shown.len(), 2, "both earlier turns are replayed");
    assert!(
        shown.iter().all(|text| !text.contains("hunter2")),
        "the secret cannot replay into the next request: {shown:?}",
    );
    assert!(
        shown.iter().all(|text| text.contains(REDACTED)),
        "the marker shows where the secret was: {shown:?}",
    );
}
