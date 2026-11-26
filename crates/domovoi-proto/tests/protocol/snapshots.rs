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
        "run_capability_contain",
        pretty(&samples::contain_verified(), false)
    );
    insta::assert_json_snapshot!(
        "run_capability_contain_unverified",
        pretty(&samples::contain_unverified(), false)
    );
    insta::assert_json_snapshot!(
        "reject_agent_too_old",
        pretty(&samples::agent_too_old(), true)
    );
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
