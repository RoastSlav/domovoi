//! The live sink: what is published while a model writes, and what never is.

use std::sync::Arc;

use domovoi_core::ports::{AnswerSink, LiveEvent, ToolPhase};
use domovoi_core::{ConversationId, MessageId, TurnId};
use domovoi_engine::conversations::{LIVE_MIN_INTERVAL, TurnSink, UNKNOWN_TOOL, releasable};
use domovoi_testkit::{FakeClock, ListRedactor, RecordingLive};
use jiff::SignedDuration;

use crate::harness::SECRET;

struct Fixture {
    live: Arc<RecordingLive>,
    clock: Arc<FakeClock>,
    sink: TurnSink,
    conversation: ConversationId,
}

fn fixture() -> Fixture {
    let live = Arc::new(RecordingLive::new());
    let clock = Arc::new(FakeClock::new("2026-11-18T10:00:00Z".parse().unwrap()));
    let conversation = ConversationId::new();
    let sink = TurnSink::new(
        live.clone(),
        conversation,
        TurnId::new(),
        MessageId::new(),
        Arc::new(ListRedactor::new([SECRET])),
        clock.clone(),
    );
    Fixture {
        live,
        clock,
        sink,
        conversation,
    }
}

impl Fixture {
    fn texts(&self) -> Vec<(u32, String)> {
        self.live
            .events_of(self.conversation)
            .into_iter()
            .filter_map(|e| match e {
                LiveEvent::Text { attempt, text, .. } => Some((attempt, text)),
                _ => None,
            })
            .collect()
    }

    fn step(&self) {
        self.clock.advance(LIVE_MIN_INTERVAL);
    }
}

#[test]
fn live_text_holds_back_the_unfinished_word() {
    let f = fixture();
    f.sink.attempt_started();
    f.sink.text("The disk is at 9");
    f.step();
    f.sink.text("4% now");
    let texts = f.texts();
    assert_eq!(texts[1].1, "The disk is at ");
    assert_eq!(texts[2].1, "The disk is at 94% ");
}

#[test]
fn a_secret_split_across_fragments_never_reaches_a_live_frame() {
    let f = fixture();
    f.sink.attempt_started();
    f.sink.text("password: hunt");
    f.step();
    f.sink.text("er2hunter2 ok ");
    for (_, text) in f.texts() {
        assert!(!text.contains("hunt"), "leaked: {text}");
        assert!(!text.contains(SECRET));
    }
    assert!(f.texts().last().unwrap().1.contains("[REDACTED]"));
}

#[test]
fn an_unfinished_private_key_block_is_never_published() {
    let f = fixture();
    f.sink.attempt_started();
    f.sink.text("Here: -----BEGIN PRIVATE KEY-----\nMIIE");
    f.step();
    f.sink.text("...\n");
    for (_, text) in f.texts() {
        assert!(!text.contains("BEGIN"), "leaked: {text}");
        assert!(!text.contains("MIIE"));
    }
    f.step();
    f.sink.text("-----END PRIVATE KEY-----\ndone ");
    assert!(f.texts().last().unwrap().1.contains("done"));
}

#[test]
fn rewritten_text_starts_a_new_attempt() {
    let live = Arc::new(RecordingLive::new());
    let clock = Arc::new(FakeClock::new("2026-11-18T10:00:00Z".parse().unwrap()));
    let conversation = ConversationId::new();
    let sink = TurnSink::new(
        live.clone(),
        conversation,
        TurnId::new(),
        MessageId::new(),
        Arc::new(ListRedactor::new(["open sesame"])),
        clock.clone(),
    );
    sink.attempt_started();
    sink.text("say open ");
    clock.advance(LIVE_MIN_INTERVAL);
    sink.text("sesame now ");
    let texts: Vec<(u32, String)> = live
        .events_of(conversation)
        .into_iter()
        .filter_map(|e| match e {
            LiveEvent::Text { attempt, text, .. } => Some((attempt, text)),
            _ => None,
        })
        .collect();
    let reset = texts.iter().position(|(attempt, _)| *attempt == 2).unwrap();
    assert_eq!(texts[reset].1, "");
    assert_eq!(
        texts.last().unwrap(),
        &(2, "say [REDACTED] now ".to_owned())
    );
}

#[test]
fn a_new_request_starts_a_new_attempt_with_empty_text() {
    let f = fixture();
    f.sink.attempt_started();
    f.sink.text("one two ");
    f.sink.attempt_started();
    let texts = f.texts();
    assert_eq!(texts.first(), Some(&(1, String::new())));
    assert_eq!(texts.last(), Some(&(2, String::new())));
}

#[test]
fn live_text_is_throttled_by_the_clock() {
    let f = fixture();
    f.sink.attempt_started();
    f.sink.text("alpha ");
    f.clock.advance(SignedDuration::from_millis(10));
    f.sink.text("beta ");
    assert_eq!(f.texts().len(), 2);
    f.clock.advance(SignedDuration::from_millis(100));
    f.sink.text("gamma ");
    assert_eq!(f.texts().last().unwrap().1, "alpha beta gamma ");
    f.sink.attempt_started();
    assert_eq!(f.texts().last(), Some(&(2, String::new())));
}

#[test]
fn live_tools_report_only_closed_names() {
    let f = fixture();
    f.sink.tool("journal", ToolPhase::Started, false);
    f.sink.tool("rm -rf /", ToolPhase::Started, true);
    let names: Vec<_> = f
        .live
        .events_of(f.conversation)
        .into_iter()
        .filter_map(|e| match e {
            LiveEvent::Tool { tool, .. } => Some(tool),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["journal".to_owned(), UNKNOWN_TOOL.to_owned()]);
}

#[test]
fn live_text_is_cut_to_the_answer_limit() {
    let f = fixture();
    f.sink.attempt_started();
    f.sink.text(&"word ".repeat(1000));
    assert!(f.texts().last().unwrap().1.chars().count() <= 4000);
}

#[test]
fn an_unfinished_word_is_not_published_at_all_without_whitespace() {
    let f = fixture();
    f.sink.attempt_started();
    f.sink.text(&"x".repeat(500));
    assert_eq!(f.texts(), vec![(1, String::new())]);
    assert_eq!(releasable("abc", &ListRedactor::new([SECRET])), "");
}

#[tokio::test]
async fn a_turn_streams_only_safe_text_through_the_service() {
    use domovoi_core::conversation::MessageStatus;

    use crate::harness::Rig;

    let rig = Rig::new().await;
    rig.model.stream(&["password: hunt", "er2hunter2 ok "]);
    let conversation = rig.global().await;
    let started = rig.ask(conversation.id, "q").await;
    rig.wait_assistant(
        conversation.id,
        started.assistant.turn,
        MessageStatus::Complete,
    )
    .await;
    for event in rig.live.events_of(conversation.id) {
        if let LiveEvent::Text { text, .. } = event {
            assert!(!text.contains("hunt"), "leaked: {text}");
        }
    }
}
