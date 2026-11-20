//! The frames of the stream: what each journal row looks like on the wire.

use axum::response::sse::Event;
use domovoi_core::ports::{JournalEntry, Redactor, topics_of};
use serde::Serialize;
use serde_json::{Value, json};

use crate::api::operator::live::LiveFrame;

/// The largest serialized payload a `journal` frame carries; bigger ones are dropped.
pub(crate) const MAX_FRAME_PAYLOAD_BYTES: usize = 16 * 1024;

/// One journal row as the browser sees it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct JournalFrame {
    seq: u64,
    aggregate_type: String,
    aggregate_id: String,
    kind: String,
    payload_v: u32,
    payload: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_truncated: Option<bool>,
    actor: String,
    at: String,
    topics: Vec<String>,
}

/// The payload a client may see: a timeline row loses its `detail` and has its summary
/// redacted; any payload over the size cap is dropped. The flag says whether it was dropped.
fn visible_payload(entry: &JournalEntry, redactor: &dyn Redactor) -> (Value, Option<bool>) {
    let payload = if entry.kind == "incident.timeline" {
        let summary = entry.payload["summary"].as_str().unwrap_or_default();
        json!({
            "kind": entry.payload["kind"].clone(),
            "summary": redactor.redact(summary),
        })
    } else {
        entry.payload.clone()
    };
    let size = serde_json::to_string(&payload).map_or(usize::MAX, |text| text.len());
    if size > MAX_FRAME_PAYLOAD_BYTES {
        (Value::Null, Some(true))
    } else {
        (payload, None)
    }
}

impl JournalFrame {
    /// The frame for `entry`.
    pub(crate) fn new(entry: &JournalEntry, redactor: &dyn Redactor) -> Self {
        let (payload, payload_truncated) = visible_payload(entry, redactor);
        Self {
            seq: entry.seq,
            aggregate_type: entry.aggregate.kind.as_str().to_owned(),
            aggregate_id: entry.aggregate.id.clone(),
            kind: entry.kind.clone(),
            payload_v: entry.payload_v,
            payload,
            payload_truncated,
            actor: entry.actor.clone(),
            at: entry.occurred_at.to_string(),
            topics: topics_of(entry).iter().map(|t| t.as_wire()).collect(),
        }
    }

    /// The SSE event, with `id` set to the journal `seq`.
    pub(crate) fn event(&self) -> Option<Event> {
        Event::default()
            .event("journal")
            .id(self.seq.to_string())
            .json_data(self)
            .ok()
    }
}

/// `ready`: the connection is open. A resumed connection carries no `id`, so the client's
/// cursor never moves backwards.
pub(crate) fn ready(head: u64, resumed: bool, topics: &[String]) -> Option<Event> {
    let event = Event::default().event("ready");
    let event = if resumed {
        event
    } else {
        event.id(head.to_string())
    };
    event
        .json_data(json!({"head_seq": head, "resumed": resumed, "topics": topics}))
        .ok()
}

/// `reset`: the client may hold an incomplete picture and should refetch.
pub(crate) fn reset(reason: &str, head: u64) -> Option<Event> {
    Event::default()
        .event("reset")
        .id(head.to_string())
        .json_data(json!({"reason": reason, "head_seq": head}))
        .ok()
}

/// A live event: ephemeral, with no `id`, so the client's cursor never moves and a reconnect
/// never replays it.
pub(crate) fn live_event(frame: &LiveFrame) -> Event {
    Event::default().event(frame.name).data(&*frame.data)
}

/// A comment line that keeps idle connections open.
pub(crate) fn keepalive() -> Event {
    Event::default().comment("keepalive")
}

#[cfg(test)]
mod tests {
    use domovoi_core::ports::{AggregateKind, AggregateRef};
    use jiff::Timestamp;

    use super::*;

    struct Mask;
    impl Redactor for Mask {
        fn redact(&self, text: &str) -> String {
            text.replace("hunter2", "[redacted]")
        }
    }

    fn entry(kind: &str, payload: Value) -> JournalEntry {
        JournalEntry {
            seq: 4,
            aggregate: AggregateRef {
                kind: AggregateKind::Incident,
                id: "0190a0a0-0000-7000-8000-0000000000a1".to_owned(),
            },
            kind: kind.to_owned(),
            payload_v: 1,
            payload,
            actor: "system:incidents".to_owned(),
            occurred_at: Timestamp::UNIX_EPOCH,
        }
    }

    #[test]
    fn timeline_frames_drop_detail_and_redact_the_summary() {
        let row = entry(
            "incident.timeline",
            json!({"kind": "triage.report", "summary": "the password is hunter2", "detail": {"big": "x"}}),
        );
        let frame = JournalFrame::new(&row, &Mask);
        assert_eq!(
            frame.payload,
            json!({"kind": "triage.report", "summary": "the password is [redacted]"})
        );
    }

    #[test]
    fn oversized_payloads_are_dropped_with_a_flag() {
        let row = entry(
            "operation.step_finished",
            json!({"blob": "x".repeat(20_000)}),
        );
        let frame = JournalFrame::new(&row, &Mask);
        assert_eq!(frame.payload, Value::Null);
        assert_eq!(frame.payload_truncated, Some(true));
    }

    struct Plain;
    impl Redactor for Plain {
        fn redact(&self, text: &str) -> String {
            text.to_owned()
        }
    }

    /// The live frames as the fixture records them: one entry per event, rendered by the
    /// production serializer for fixed ids. `OPERATOR_LIVE_UPDATE=1` rewrites the file.
    fn live_fixture() -> Value {
        use domovoi_core::ports::{LiveEnd, LiveEvent, ToolPhase};
        use domovoi_core::{ConversationId, MessageId, TurnId};

        let conversation: ConversationId = "0190a0a0-0000-7000-8000-0000000000c1".parse().unwrap();
        let turn: TurnId = "0190a0a0-0000-7000-8000-0000000000c2".parse().unwrap();
        let message: MessageId = "0190a0a0-0000-7000-8000-0000000000c3".parse().unwrap();
        let events = [
            ("live_start", LiveEvent::Start { turn, message }),
            (
                "live_text_attempt1",
                LiveEvent::Text {
                    turn,
                    attempt: 1,
                    text: "The journal shows two restarts of nginx at ".to_owned(),
                },
            ),
            (
                "live_text_attempt2",
                LiveEvent::Text {
                    turn,
                    attempt: 2,
                    text: String::new(),
                },
            ),
            (
                "live_tool_started",
                LiveEvent::Tool {
                    turn,
                    tool: "journal".to_owned(),
                    phase: ToolPhase::Started,
                    failed: false,
                },
            ),
            (
                "live_tool_finished",
                LiveEvent::Tool {
                    turn,
                    tool: "journal".to_owned(),
                    phase: ToolPhase::Finished,
                    failed: false,
                },
            ),
            (
                "live_end_complete",
                LiveEvent::End {
                    turn,
                    outcome: LiveEnd::Complete,
                },
            ),
            (
                "live_end_discarded",
                LiveEvent::End {
                    turn,
                    outcome: LiveEnd::Discarded,
                },
            ),
        ];
        let frames: Vec<Value> = events
            .iter()
            .map(|(label, event)| {
                let frame = LiveFrame::of(conversation, event);
                json!({
                    "name": label,
                    "event": frame.name,
                    "data": serde_json::from_str::<Value>(&frame.data).unwrap(),
                })
            })
            .collect();
        json!({ "schema_version": 1, "frames": frames })
    }

    #[test]
    fn live_frames_match_the_committed_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/test/fixtures/operator-live/frames.json"
        );
        let rendered = live_fixture();
        if std::env::var_os("OPERATOR_LIVE_UPDATE").is_some() {
            std::fs::create_dir_all(std::path::Path::new(path).parent().unwrap()).unwrap();
            let text = serde_json::to_string_pretty(&rendered).unwrap();
            std::fs::write(
                path,
                format!(
                    "{text}
"
                ),
            )
            .unwrap();
        }
        let committed: Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(
            committed, rendered,
            "run with OPERATOR_LIVE_UPDATE=1 to rewrite"
        );
    }

    #[test]
    fn a_live_event_frame_has_a_name_and_no_id() {
        let frame = LiveFrame {
            conversation: "c".to_owned(),
            name: "live.text",
            data: std::sync::Arc::from("{}"),
        };
        let text = format!("{:?}", live_event(&frame));
        assert!(text.contains("live.text"), "{text}");
        assert!(!text.contains("id: Some"), "{text}");
    }

    #[test]
    fn journal_frames_match_the_committed_fixture() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../../web/src/test/fixtures/journal-frames/frames.json"
        ))
        .unwrap();
        let mut checked = 0;
        for frame in fixture["frames"].as_array().unwrap() {
            if frame["event"] != "journal" {
                continue;
            }
            let data = &frame["data"];
            let mut payload = data["payload"].clone();
            if data["kind"] == "incident.timeline" {
                // The stored row also carries the full detail, which the frame must drop.
                payload["detail"] = json!({"big": "triage outcome"});
            }
            let row = JournalEntry {
                seq: data["seq"].as_u64().unwrap(),
                aggregate: AggregateRef {
                    kind: AggregateKind::parse(data["aggregate_type"].as_str().unwrap()).unwrap(),
                    id: data["aggregate_id"].as_str().unwrap().to_owned(),
                },
                kind: data["kind"].as_str().unwrap().to_owned(),
                payload_v: 1,
                payload,
                actor: data["actor"].as_str().unwrap().to_owned(),
                occurred_at: data["at"].as_str().unwrap().parse().unwrap(),
            };
            let rendered = serde_json::to_value(JournalFrame::new(&row, &Plain)).unwrap();
            assert_eq!(&rendered, data, "{}", frame["name"]);
            assert_eq!(frame["id"], data["seq"]);
            checked += 1;
        }
        assert_eq!(checked, 12);
    }
}
