//! Round trips and strict decoding for the protocol 1.2 extensions.

use domovoi_core::{ScriptDigest, SkillSummary};
use domovoi_proto::messages::*;
use domovoi_proto::{
    Base64Bytes, Message, Nonce, Open, PublicKey, Sha256Bytes, Signature, decode, encode,
};

use crate::samples::{self, request_id};

fn frame_with_digest(digest: &str) -> String {
    let hello = samples::all()
        .into_iter()
        .find(|message| message.kind() == "hello")
        .expect("the samples include a hello");
    let frame = encode(&hello, request_id(1), None).expect("encodable");
    let value: serde_json::Value = serde_json::from_str(&frame).expect("valid JSON");
    let old = value["payload"]["skills"][0]["entry_script_digest"]
        .as_str()
        .expect("a digest")
        .to_owned();
    frame.replace(&old, digest)
}

#[test]
fn hello_1_2_round_trips_with_every_extension() {
    let hello = samples::all()
        .into_iter()
        .find(|message| message.kind() == "hello")
        .expect("the samples include a hello");
    let Message::Hello(payload) = &hello else {
        unreachable!("kind hello is the Hello variant")
    };
    assert!(payload.features.is_empty());
    assert_eq!(payload.skill_runner_status, SkillRunnerStatus::Absent);
    assert_eq!(payload.helper_status, HelperStatus::Absent);
    assert!(payload.helper_identity.is_none());
    assert!(payload.catalog_hash.is_none());
    assert!(payload.ceiling_hash.is_none());
    assert!(payload.ceiling_summary.is_none());
    assert!(payload.runs_as_root);
    assert!(!payload.journal_epoch.is_nil());

    let frame = encode(&hello, request_id(2), None).expect("encodable");
    assert_eq!(decode(&frame).expect("decodable").message, hello);
}

#[test]
fn agent_status_round_trips_with_every_extension() {
    let status = Message::AgentStatus(AgentStatus {
        helper_status: HelperStatus::Ok {
            ops: vec!["contain".to_owned()],
        },
        helper_identity: Some(HelperIdentity {
            public_key: PublicKey([9; 32]),
        }),
        skill_runner_status: SkillRunnerStatus::Ok {
            version: "1".to_owned(),
        },
        runs_as_root: true,
        ceiling_hash: Some(Sha256Bytes([8; 32])),
        ceiling_summary: Some("read and write below 90%".to_owned()),
    });
    let frame = encode(&status, request_id(3), None).expect("encodable");
    assert_eq!(decode(&frame).expect("decodable").message, status);
}

#[test]
fn expected_safety_tagged_variants_roundtrip() {
    for safety in [
        ExpectedSafety::Verified {
            fingerprint: "ab".repeat(32),
            boot_id: "5f0c".to_owned(),
            generation: 7,
            config_hash: Sha256Bytes([1; 32]),
            route_state_hash: Sha256Bytes([2; 32]),
        },
        ExpectedSafety::Unverified {
            reason: "helper absent".to_owned(),
        },
    ] {
        let json = serde_json::to_string(&safety).expect("serializable");
        let back: ExpectedSafety = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(back, safety);
    }
}

#[test]
fn malformed_expected_safety_refused() {
    // Unknown tag.
    assert!(serde_json::from_str::<ExpectedSafety>(r#"{"state":"trusted"}"#).is_err());
    // Missing field.
    assert!(
        serde_json::from_str::<ExpectedSafety>(
            r#"{"state":"verified","fingerprint":"ab","boot_id":"x"}"#
        )
        .is_err()
    );
    // Wrong hash length.
    assert!(
        serde_json::from_str::<ExpectedSafety>(
            r#"{"state":"verified","fingerprint":"ab","boot_id":"x","generation":1,"config_hash":"00","route_state_hash":"00"}"#
        )
        .is_err()
    );
}

#[test]
fn unknown_enum_value_preserved() {
    let open: Open<RefusalCode> =
        serde_json::from_str(r#""invented_later""#).expect("an open value decodes");
    assert!(open.known().is_none());
    assert_eq!(open.raw(), "invented_later");
    assert_eq!(
        serde_json::to_string(&open).expect("serializable"),
        r#""invented_later""#
    );

    let known: Open<RefusalCode> =
        serde_json::from_str(r#""journal_corrupt""#).expect("a known value decodes");
    assert_eq!(known.known(), Some(&RefusalCode::JournalCorrupt));
    assert_eq!(known.raw(), "journal_corrupt");
}

#[test]
fn attestation_roundtrip_over_wire() {
    for message in [
        Message::AttestSafety(AttestSafety {
            nonce: Nonce([1; 32]),
        }),
        Message::SafetyAttestation(SafetyAttestation {
            public_key: PublicKey([2; 32]),
            report: Base64Bytes(b"report".to_vec()),
            signature: Signature([3; 64]),
        }),
    ] {
        let frame = encode(&message, request_id(4), None).expect("encodable");
        assert_eq!(decode(&frame).expect("decodable").message, message);
    }
}

#[test]
fn helper_identity_rejects_wrong_key_length() {
    assert!(
        serde_json::from_str::<HelperIdentity>(r#"{"public_key":"AAAA"}"#).is_err(),
        "a public key must decode to exactly 32 bytes"
    );
}

#[test]
fn transfer_chunk_rejects_oversized_payload() {
    let chunk = TransferChunk {
        transfer_id: uuid::Uuid::nil(),
        seq: 0,
        bytes: Base64Bytes(vec![0; domovoi_proto::MAX_BASE64_BYTES + 1]),
    };
    let json = serde_json::to_value(&chunk).expect("serializable");
    assert!(
        serde_json::from_value::<TransferChunk>(json).is_err(),
        "a chunk above 64 KiB is refused at decode time"
    );
}

#[test]
fn entry_script_digest_survives_hello_roundtrip() {
    let digest = ScriptDigest::of_bytes(b"#!/bin/sh\nexit 0\n").to_string();
    let frame = frame_with_digest(&digest);
    let decoded = decode(&frame).expect("a canonical digest decodes");
    let Message::Hello(hello) = decoded.message else {
        panic!("kind hello is the Hello variant");
    };
    assert_eq!(hello.skills[0].entry_script_digest.to_string(), digest);
}

#[test]
fn entry_script_digest_rejects_noncanonical_hex() {
    let good = ScriptDigest::of_bytes(b"hello").to_string();
    let upper = good.to_uppercase();
    let mixed = format!("{}{}", &good[..32], &upper[32..]);
    let cases = [
        upper,
        mixed,
        good[..63].to_owned(),
        format!("{good}0"),
        String::new(),
        format!("0x{good}"),
        format!(" {good}"),
        format!("{good} "),
    ];
    for bad in cases {
        let frame = frame_with_digest(&bad);
        assert!(
            matches!(
                decode(&frame),
                Err(domovoi_proto::ProtoError::InvalidPayload { .. })
            ),
            "digest {bad:?} must be refused"
        );
    }
}

#[test]
fn hello_without_entry_script_digest_is_refused() {
    // A missing field is a protocol error, not an older agent.
    assert!(
        serde_json::from_value::<SkillSummary>(serde_json::json!({
            "name": "restart_container",
            "description": "Restart one container.",
            "risk": "write",
            "params": [],
            "applies_to": [],
            "manifest_hash": "ab".repeat(32),
        }))
        .is_err()
    );
}
