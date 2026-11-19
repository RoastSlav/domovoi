//! The frames of the stream: what each journal row looks like on the wire.

use axum::response::sse::Event;
use domovoi_core::ports::{JournalEntry, Redactor, topics_of};
use serde::Serialize;
use serde_json::{Value, json};

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
