use domovoi_proto::messages::{ErrorMessage, Ping};
use domovoi_proto::{MAX_FRAME_BYTES, Message, ProtoError, decode, encode};

use crate::samples::{self, T0, request_id};

#[test]
fn every_message_kind_round_trips() {
    let mut messages = samples::all();
    let kinds: Vec<&str> = messages.iter().map(Message::kind).collect();
    assert_eq!(
        kinds,
        Message::KINDS,
        "samples must cover every kind, in order"
    );
    messages.push(samples::skill_success());

    for (n, message) in messages.into_iter().enumerate() {
        let id = request_id(n as u128);
        let reply_to = (n % 2 == 0).then(|| request_id(1000));
        let frame = encode(&message, id, reply_to).unwrap();
        let decoded = decode(&frame).unwrap();
        assert_eq!(decoded.id, id);
        assert_eq!(decoded.reply_to, reply_to);
        assert_eq!(decoded.message, message, "{}", message.kind());
    }
}

#[test]
fn unknown_kind_decodes_as_unknown() {
    let frame = format!(
        r#"{{"v":1,"id":"{}","kind":"teleport","payload":{{"to":"mars"}}}}"#,
        request_id(1)
    );
    let decoded = decode(&frame).unwrap();
    assert_eq!(
        decoded.message,
        Message::Unknown {
            kind: "teleport".to_owned()
        }
    );
    assert_eq!(decoded.message.kind(), "teleport");
}

#[test]
fn unknown_fields_are_ignored() {
    let frame = format!(
        r#"{{"v":1,"id":"{}","kind":"ping","trace":"x","payload":{{"at":"2023-11-14T22:13:20Z","extra":[1,2]}}}}"#,
        request_id(1)
    );
    let decoded = decode(&frame).unwrap();
    assert_eq!(decoded.message, Message::Ping(Ping { at: T0 }));
}

#[test]
fn oversized_frame_is_rejected() {
    let frame = " ".repeat(MAX_FRAME_BYTES + 1);
    assert_eq!(
        decode(&frame),
        Err(ProtoError::FrameTooLarge {
            len: MAX_FRAME_BYTES + 1
        })
    );
    let big = Message::from(ErrorMessage {
        code: "x".to_owned(),
        message: "a".repeat(MAX_FRAME_BYTES),
    });
    assert!(matches!(
        encode(&big, request_id(1), None),
        Err(ProtoError::FrameTooLarge { .. })
    ));
}

#[test]
fn other_major_is_rejected() {
    let frame = format!(
        r#"{{"v":2,"id":"{}","kind":"ping","payload":{{"at":"2023-11-14T22:13:20Z"}}}}"#,
        request_id(1)
    );
    assert_eq!(decode(&frame), Err(ProtoError::UnsupportedMajor { v: 2 }));
}

#[test]
fn malformed_envelope_is_rejected() {
    assert!(matches!(decode("not json"), Err(ProtoError::Malformed(_))));
    assert!(matches!(
        decode(r#"{"v":1,"kind":"ping"}"#),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn bad_payload_names_kind() {
    let frame = format!(
        r#"{{"v":1,"id":"{}","kind":"signals_ack","payload":{{"up_to":1}}}}"#,
        request_id(1)
    );
    match decode(&frame) {
        Err(ProtoError::InvalidPayload { kind, reason }) => {
            assert_eq!(kind, "signals_ack");
            assert!(reason.contains("up_to_seq"), "{reason}");
        }
        other => panic!("expected InvalidPayload, got {other:?}"),
    }
}

#[test]
fn wrong_length_base64_is_rejected() {
    let frame = format!(
        r#"{{"v":1,"id":"{}","kind":"challenge","payload":{{"nonce":"AAAA"}}}}"#,
        request_id(1)
    );
    match decode(&frame) {
        Err(ProtoError::InvalidPayload { reason, .. }) => {
            assert!(reason.contains("expected 32 bytes"), "{reason}");
        }
        other => panic!("expected InvalidPayload, got {other:?}"),
    }
}

#[test]
fn run_tool_call_is_revalidated() {
    let frame = format!(
        r#"{{"v":1,"id":"{}","kind":"run_tool","payload":{{"call":{{"tool":"docker_logs","container":"pg","lines":100000}},"deadline":"2023-11-14T22:14:50Z"}}}}"#,
        request_id(1)
    );
    assert!(matches!(
        decode(&frame),
        Err(ProtoError::InvalidPayload { kind, .. }) if kind == "run_tool"
    ));
}

#[test]
fn unknown_failure_code_still_decodes() {
    let frame = format!(
        r#"{{"v":1,"id":"{}","kind":"reject","payload":{{"reason":{{"code":"quota_exceeded"}}}}}}"#,
        request_id(1)
    );
    let decoded = decode(&frame).unwrap();
    assert_eq!(decoded.message.kind(), "reject");
}

#[test]
fn deadline_and_timestamps_serialize_as_rfc3339() {
    let frame = encode(&Message::Ping(Ping { at: T0 }), request_id(1), None).unwrap();
    assert!(frame.contains(r#""at":"2023-11-14T22:13:20Z""#), "{frame}");
    let run_check = &samples::all()[Message::KINDS
        .iter()
        .position(|k| *k == "run_check")
        .unwrap()];
    let frame = encode(run_check, request_id(2), None).unwrap();
    assert!(
        frame.contains(r#""deadline":"2023-11-14T22:14:50Z""#),
        "{frame}"
    );
}

#[test]
fn enroll_token_is_not_in_debug_output() {
    let enroll = &samples::all()[0];
    assert!(!format!("{enroll:?}").contains("dmv_enroll_0123456789abcdef"));
}
