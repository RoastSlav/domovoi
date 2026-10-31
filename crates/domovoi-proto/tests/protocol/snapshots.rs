//! The wire contract: the exact JSON of every message kind. A change to one of these
//! snapshots that is not purely additive needs a protocol major version bump.

use domovoi_proto::{Message, encode};

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
            );
        insta::assert_json_snapshot!(message.kind().to_owned(), pretty(&message, reply));
    }
    insta::assert_json_snapshot!("skill_result_ok", pretty(&samples::skill_success(), true));
}
