//! The wire contract: the exact JSON of every message kind. A change to one of these
//! snapshots that is not purely additive needs a protocol major version bump.

use domovoi_proto::{Message, PROTOCOL_VERSION, encode};

use crate::samples::{self, request_id};

fn pretty(message: &Message, reply_to: bool) -> serde_json::Value {
    let frame = encode(message, request_id(1), reply_to.then(|| request_id(2))).unwrap();
    serde_json::from_str(&frame).unwrap()
}

#[test]
fn every_message_kind_matches_its_wire_snapshot() {
    for message in samples::all() {
        let reply = message.kind().ends_with("_result")
            || matches!(
                message.kind(),
                "pong"
                    | "enrolled"
                    | "signals_ack"
                    | "challenge"
                    | "welcome"
                    | "reject"
                    | "challenge_response"
                    | "logs"
                    | "action_status_answers"
                    | "catalog_page"
                    | "safety_attestation"
            );
        insta::assert_json_snapshot!(message.kind().to_owned(), pretty(&message, reply));
    }
    insta::assert_json_snapshot!("skill_result_ok", pretty(&samples::skill_success(), true));
    insta::assert_json_snapshot!(
        "reject_agent_too_old",
        pretty(&samples::agent_too_old(), true)
    );
}

/// The 1.1 frames, kept as literal fixtures. They are history: nothing regenerates them, and
/// the 1.2 type changes must not edit them.
#[test]
fn golden_frames_stay_at_1_1() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/protocol/fixtures/1_1");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).expect("the 1.1 fixtures exist") {
        let path = entry.expect("a readable fixture").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("a readable fixture");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let kind = path.file_stem().and_then(|s| s.to_str()).expect("a name");
        let expected = match kind {
            "skill_result_ok" => "skill_result",
            "reject_agent_too_old" => "reject",
            other => other,
        };
        assert_eq!(value["v"], 1, "{} is a 1.1 frame", path.display());
        assert_eq!(
            value["kind"],
            expected,
            "{} names its own kind",
            path.display()
        );
        // Byte-for-byte: the file is the canonical pretty JSON of its own value.
        assert_eq!(
            serde_json::to_string_pretty(&value).expect("serializable"),
            text.trim(),
            "{} was edited",
            path.display()
        );
        if kind == "unsupported" {
            assert_eq!(
                value["payload"]["kind"], "future_kind",
                "1.1 carried the field as `kind`"
            );
            assert!(
                value["payload"].get("type").is_none(),
                "1.1 never carried `type`"
            );
        }
        seen += 1;
    }
    assert!(seen >= 25, "every 1.1 message kind is pinned");
}

#[test]
fn golden_frames_pin_1_2() {
    assert_eq!(PROTOCOL_VERSION.to_string(), "1.2");
    let hello = samples::all()
        .into_iter()
        .find(|message| message.kind() == "hello")
        .expect("the samples include a hello");
    let frame = pretty(&hello, false);
    let payload = &frame["payload"];
    assert_eq!(
        payload["protocol"],
        serde_json::json!({"major": 1, "minor": 2})
    );
    assert_eq!(payload["features"], serde_json::json!([]));
    assert_eq!(payload["runs_as_root"], true);
    assert!(payload["journal_epoch"].is_string());
    assert_eq!(
        payload["skills"][0]["entry_script_digest"]
            .as_str()
            .map(str::len),
        Some(64),
        "the hello skill summary carries a 64-hex digest"
    );

    let unsupported = samples::all()
        .into_iter()
        .find(|message| message.kind() == "unsupported")
        .expect("the samples include an unsupported");
    let frame = pretty(&unsupported, true);
    assert_eq!(frame["payload"]["type"], "future_kind");
    assert!(frame["payload"].get("kind").is_none());
}

#[test]
fn unsupported_1_1_fixture_is_unchanged() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/protocol/fixtures/1_1/unsupported.json"
    );
    let text = std::fs::read_to_string(path).expect("the fixture exists");
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(value["payload"]["kind"], "future_kind");
    assert!(value["payload"].get("type").is_none());
}
