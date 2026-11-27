//! The live sink of one turn: turns what the model writes into safe, throttled snapshots.
//!
//! Only text that is already redacted and not the trailing half-written word is ever
//! published (INV-5). The sink never touches storage.

use std::sync::{Arc, Mutex, PoisonError};

use domovoi_core::ports::{AnswerSink, Clock, LiveEvent, LivePublisher, Redactor, ToolPhase};
use domovoi_core::{ConversationId, MessageId, ReadTool, TurnId};
use jiff::{SignedDuration, Timestamp};

use crate::triage::{MAX_ANSWER_CHARS, before_claim};

/// The shortest time between two text snapshots of one attempt.
pub const LIVE_MIN_INTERVAL: SignedDuration = SignedDuration::from_millis(100);

/// The name published for a tool outside the closed read-tool set.
pub const UNKNOWN_TOOL: &str = "unknown tool";

/// The part of `raw` that is safe to show: no unfinished private-key block, redacted, and
/// cut after the last whitespace so a half-written word (which could be half a secret) waits.
#[must_use]
pub fn releasable(raw: &str, redactor: &dyn Redactor) -> String {
    let visible = match raw.rfind("-----BEGIN") {
        Some(start) if !raw[start..].contains("-----END") => &raw[..start],
        _ => raw,
    };
    let redacted = redactor.redact(visible);
    match redacted.rfind(char::is_whitespace) {
        Some(at) => {
            let width = redacted[at..].chars().next().map_or(1, char::len_utf8);
            redacted[..at + width].to_owned()
        }
        None => String::new(),
    }
}

fn closed_name(name: &str) -> String {
    ReadTool::ALL
        .iter()
        .find(|tool| tool.name() == name)
        .map_or_else(|| UNKNOWN_TOOL.to_owned(), |tool| tool.name().to_owned())
}

#[derive(Default)]
struct State {
    attempt: u32,
    raw: String,
    sent: String,
    last_emit: Option<Timestamp>,
    started: bool,
}

/// Publishes the live events of one turn.
pub struct TurnSink {
    live: Arc<dyn LivePublisher>,
    conversation: ConversationId,
    turn: TurnId,
    message: MessageId,
    redactor: Arc<dyn Redactor>,
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
}

impl TurnSink {
    /// A sink for the answer `message` of `turn` in `conversation`.
    #[must_use]
    pub fn new(
        live: Arc<dyn LivePublisher>,
        conversation: ConversationId,
        turn: TurnId,
        message: MessageId,
        redactor: Arc<dyn Redactor>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            live,
            conversation,
            turn,
            message,
            redactor,
            clock,
            state: Mutex::new(State::default()),
        }
    }

    fn publish(&self, event: LiveEvent) {
        self.live.publish(self.conversation, event);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Announces the start once, when the model is first asked (after admission passed).
    fn announce(&self, state: &mut State) {
        if !state.started {
            state.started = true;
            self.publish(LiveEvent::Start {
                turn: self.turn,
                message: self.message,
            });
        }
    }

    fn restart(&self, state: &mut State) {
        state.attempt += 1;
        state.raw.clear();
        state.sent.clear();
        state.last_emit = None;
        self.publish(LiveEvent::Text {
            turn: self.turn,
            attempt: state.attempt,
            text: String::new(),
        });
    }
}

impl AnswerSink for TurnSink {
    fn attempt_started(&self) {
        let mut state = self.lock();
        self.announce(&mut state);
        self.restart(&mut state);
    }

    fn text(&self, fragment: &str) {
        let mut state = self.lock();
        self.announce(&mut state);
        if state.attempt == 0 {
            self.restart(&mut state);
        }
        state.raw.push_str(fragment);
        // A claim that something is missing waits for the finished answer's review.
        let released = releasable(&state.raw, self.redactor.as_ref());
        let released = before_claim(&released).to_owned();
        if !released.starts_with(&state.sent) {
            // Redaction rewrote text that was already shown: the client starts over now.
            state.attempt += 1;
            state.sent.clear();
            self.publish(LiveEvent::Text {
                turn: self.turn,
                attempt: state.attempt,
                text: String::new(),
            });
        }
        if released.len() == state.sent.len() {
            return;
        }
        let now = self.clock.now();
        if let Some(last) = state.last_emit
            && now.duration_since(last) < LIVE_MIN_INTERVAL
        {
            return;
        }
        let shown: String = released.chars().take(MAX_ANSWER_CHARS).collect();
        state.sent = released;
        state.last_emit = Some(now);
        self.publish(LiveEvent::Text {
            turn: self.turn,
            attempt: state.attempt,
            text: shown,
        });
    }

    fn tool(&self, tool: &str, phase: ToolPhase, failed: bool) {
        let mut state = self.lock();
        self.announce(&mut state);
        drop(state);
        self.publish(LiveEvent::Tool {
            turn: self.turn,
            tool: closed_name(tool),
            phase,
            failed,
        });
    }
}
