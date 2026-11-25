//! Round trips and strict decoding for the protocol 1.2 extensions.

use domovoi_core::attest::{BootId, Hash32, UnverifiedReason};
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

fn verified_json() -> serde_json::Value {
    serde_json::json!({
        "state": "verified",
        "fingerprint": "ab".repeat(32),
        "boot_id": "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f",
        "generation": 7,
        "config_hash": "01".repeat(32),
        "route_state_hash": "02".repeat(32),
    })
}

fn refused(value: serde_json::Value) -> bool {
    serde_json::from_value::<ExpectedSafety>(value).is_err()
}

#[test]
fn expected_safety_tagged_variants_roundtrip() {
    for safety in [
        ExpectedSafety::Verified {
            fingerprint: Hash32::from_bytes([0xab; 32]),
            boot_id: BootId::parse("5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f").unwrap(),
            generation: 7,
            config_hash: Hash32::from_bytes([1; 32]),
            route_state_hash: Hash32::from_bytes([2; 32]),
        },
        ExpectedSafety::Unverified {
            reason: UnverifiedReason::KeyNotPinned,
        },
    ] {
        let json = serde_json::to_string(&safety).expect("serializable");
        let back: ExpectedSafety = serde_json::from_str(&json).expect("deserializable");
        assert_eq!(back, safety);
    }
    let verified: ExpectedSafety = serde_json::from_value(verified_json()).unwrap();
    assert_eq!(serde_json::to_value(&verified).unwrap(), verified_json());
}

#[test]
fn malformed_expected_safety_refused() {
    // Unknown tag.
    assert!(refused(serde_json::json!({"state": "trusted"})));
    // Missing fields.
    assert!(refused(serde_json::json!({
        "state": "verified",
        "fingerprint": "ab".repeat(32),
        "boot_id": "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f",
    })));
    // Wrong hash length, uppercase hash, malformed boot id, null where a value is needed.
    for (field, value) in [
        ("config_hash", serde_json::json!("00")),
        ("fingerprint", serde_json::json!("AB".repeat(32))),
        (
            "boot_id",
            serde_json::json!("5F0C1D2E-3A4B-4C5D-8E9F-0A1B2C3D4E5F"),
        ),
        ("boot_id", serde_json::json!("5f0c")),
        ("generation", serde_json::json!(-1)),
        ("route_state_hash", serde_json::Value::Null),
    ] {
        let mut json = verified_json();
        json[field] = value;
        assert!(refused(json), "{field}");
    }
}

#[test]
fn expected_safety_refuses_extra_field() {
    let mut verified = verified_json();
    verified["extra"] = serde_json::json!(1);
    assert!(refused(verified));
    // A field of the other variant is an extra field too, even when it is null.
    let mut verified = verified_json();
    verified["reason"] = serde_json::json!("key_mismatch");
    assert!(refused(verified));
    assert!(refused(serde_json::json!({
        "state": "unverified", "reason": "key_not_pinned", "generation": 1,
    })));
    assert!(refused(serde_json::json!({
        "state": "unverified", "reason": "key_not_pinned", "fingerprint": null,
    })));
}

#[test]
fn expected_safety_reason_is_closed_enum() {
    for reason in UnverifiedReason::ALL {
        let json = serde_json::json!({"state": "unverified", "reason": reason.token()});
        assert!(!refused(json), "{}", reason.token());
    }
    assert!(refused(
        serde_json::json!({"state": "unverified", "reason": "helper absent"})
    ));
    assert!(refused(
        serde_json::json!({"state": "unverified", "reason": "Key_Not_Pinned"})
    ));
    assert!(refused(serde_json::json!({"state": "unverified"})));
}

#[test]
fn expected_safety_unknown_tag_refused() {
    for tag in ["trusted", "Verified", "UNVERIFIED", ""] {
        assert!(refused(serde_json::json!({"state": tag})), "{tag:?}");
    }
    assert!(refused(serde_json::json!({"reason": "key_not_pinned"})));
    assert!(refused(serde_json::json!("verified")));
}

#[test]
fn unverified_serialises_to_state_and_reason_only() {
    let value = serde_json::to_value(ExpectedSafety::Unverified {
        reason: UnverifiedReason::ConfirmationStale,
    })
    .unwrap();
    assert_eq!(
        value,
        serde_json::json!({"state": "unverified", "reason": "confirmation_stale"})
    );
}

#[test]
fn max_report_bytes_equals_base64_cap() {
    assert_eq!(
        domovoi_core::attest::MAX_REPORT_BYTES,
        domovoi_proto::MAX_BASE64_BYTES
    );
}

#[test]
fn hash32_and_sha256bytes_encode_identically() {
    for bytes in [[0_u8; 32], [0xab; 32], core::array::from_fn(|i| i as u8)] {
        let ours = serde_json::to_string(&Hash32::from_bytes(bytes)).unwrap();
        let theirs = serde_json::to_string(&Sha256Bytes(bytes)).unwrap();
        assert_eq!(ours, theirs);
        let back: Hash32 = serde_json::from_str(&theirs).unwrap();
        assert_eq!(back.as_bytes(), &bytes);
    }
}

#[test]
fn hash32_refuses_uppercase_hex() {
    let upper = serde_json::json!("AB".repeat(32));
    assert!(serde_json::from_value::<Hash32>(upper.clone()).is_err());
    assert!(serde_json::from_value::<Sha256Bytes>(upper).is_err());
    assert!(serde_json::from_value::<Hash32>(serde_json::json!("ab".repeat(31))).is_err());
    assert!(serde_json::from_value::<Hash32>(serde_json::json!("ab".repeat(33))).is_err());
}

#[test]
fn existing_1_2_snapshots_unchanged() {
    // The type changes leave every existing frame alone: its snapshot still exists and the
    // optional guard is still written as null for a capability that carries none.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/protocol/snapshots");
    for kind in Message::KINDS {
        let path = format!("{dir}/protocol__snapshots__{kind}.snap");
        assert!(std::path::Path::new(&path).exists(), "{path}");
    }
    let text =
        std::fs::read_to_string(format!("{dir}/protocol__snapshots__run_capability.snap")).unwrap();
    assert!(text.contains("\"expected_safety\": null"));
    for kind in ["attest_safety", "safety_attestation"] {
        let text =
            std::fs::read_to_string(format!("{dir}/protocol__snapshots__{kind}.snap")).unwrap();
        assert!(
            !text.contains("fingerprint"),
            "{kind} has no fingerprint field"
        );
    }
}

#[test]
fn contain_samples_carry_the_strict_guard_on_the_wire() {
    for (message, state) in [
        (samples::contain_verified(), "verified"),
        (samples::contain_unverified(), "unverified"),
    ] {
        let frame = encode(&message, request_id(1), None).unwrap();
        let value: serde_json::Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(value["payload"]["expected_safety"]["state"], state);
        assert_eq!(decode(&frame).unwrap().message, message);
    }
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
