//! Tests for the helper protocol types and framing.

use uuid::Uuid;

use super::*;
use crate::identity::{FaultKind, PathFacts, PathFault, PathId, PathKind, RefusalToken};
use crate::isolation::{PlatformRecordState, PlatformTier};

fn id() -> Uuid {
    Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)
}

fn every_request() -> Vec<HelperRequest> {
    let mut out = vec![HelperRequest::Health, HelperRequest::Groups];
    for group in [
        ReadGroup::DockerRead,
        ReadGroup::SystemdRead,
        ReadGroup::SmartRead,
        ReadGroup::KernelRead,
        ReadGroup::NetRead,
    ] {
        out.push(HelperRequest::Read { group });
    }
    out.push(HelperRequest::SafetyAttest {
        nonce: Nonce::from_bytes([0xab; 32]),
    });
    out.push(HelperRequest::SafetyPublicKey);
    out
}

fn every_response() -> Vec<HelperResponse> {
    let fault = PathFault {
        id: PathId::HelperDir,
        path: "/var/lib/domovoi-helper".into(),
        found: Some(PathFacts {
            kind: Some(PathKind::Dir),
            symlink: false,
            mode: 0o755,
            uid: 0,
            gid: 0,
        }),
        expected: PathFacts {
            kind: Some(PathKind::Dir),
            symlink: false,
            mode: 0o700,
            uid: 0,
            gid: 0,
        },
        why: FaultKind::Mode,
    };
    vec![
        HelperResponse::Health {
            protocol: HELPER_PROTOCOL,
            build: "0.0.10".into(),
            pid: 42,
        },
        HelperResponse::Groups {
            enabled: vec![HelperGroup::DockerRead, HelperGroup::SafetyAttest],
            ignored: vec![HelperGroup::Nft],
            write_opt_in: false,
            platform_tier: PlatformTier::Blocked,
            platform_record: PlatformRecordState::Missing,
        },
        HelperResponse::Unavailable {
            group: HelperGroup::SmartRead,
            reason: UnavailableReason::NotImplemented,
        },
        HelperResponse::SafetyAttested {
            attestation: SignedAttestation {
                public_key: [7; 32],
                report: b"format=1\n".to_vec(),
                signature: [9; 64],
            },
        },
        HelperResponse::SafetyUnsigned {
            token: UnsignedToken::KeyMissing,
        },
        HelperResponse::SafetyPublicKey {
            public_key: "07".repeat(32),
        },
        HelperResponse::Refused {
            token: RefusalToken::PeerNotAgent,
        },
        HelperResponse::StartRefused {
            token: RefusalToken::HelperStatePermissions,
            faults: vec![fault],
        },
    ]
}

// covers: frame_too_large
#[test]
fn helper_frame_rejects_length_above_cap_before_reading_body() {
    let header = ((MAX_FRAME_BYTES as u32) + 1).to_be_bytes();
    // Only the header is given: the check cannot depend on any body byte.
    let err = frame_len(header).expect_err("above the cap");
    assert_eq!(err, FrameError::FrameTooLarge(MAX_FRAME_BYTES + 1));
    assert_eq!(err.token(), RefusalToken::FrameTooLarge);
    assert_eq!(
        frame_len((MAX_FRAME_BYTES as u32).to_be_bytes()),
        Ok(MAX_FRAME_BYTES)
    );
    assert_eq!(
        frame_len(u32::MAX.to_be_bytes()).map_err(|e| e.token()),
        Err(RefusalToken::FrameTooLarge)
    );
}

#[test]
fn helper_frame_round_trips_every_request_and_response() {
    for request in every_request() {
        let frame = HelperRequestFrame {
            v: HELPER_PROTOCOL,
            id: id(),
            request,
        };
        let bytes = encode_frame(&frame).expect("encodes");
        let len = frame_len([bytes[0], bytes[1], bytes[2], bytes[3]]).expect("within cap");
        assert_eq!(len, bytes.len() - 4);
        assert_eq!(decode_request(&bytes[4..]), Ok(frame));
    }
    for response in every_response() {
        let frame = HelperResponseFrame {
            v: HELPER_PROTOCOL,
            id: id(),
            response,
        };
        let bytes = encode_frame(&frame).expect("encodes");
        assert_eq!(decode_response(&bytes[4..]), Ok(frame));
    }
}

// covers: unknown_operation
// covers: frame_malformed
// covers: unsupported_version
#[test]
fn decode_request_maps_unknown_op_malformed_and_version() {
    let ok = format!(r#"{{"v":1,"id":"{}","request":{{"op":"health"}}}}"#, id());
    assert!(decode_request(ok.as_bytes()).is_ok());

    let unknown = format!(
        r#"{{"v":1,"id":"{}","request":{{"op":"docker_restart","name":"x"}}}}"#,
        id()
    );
    assert_eq!(
        decode_request(unknown.as_bytes()),
        Err(RefusalToken::UnknownOperation)
    );

    let version = format!(r#"{{"v":2,"id":"{}","request":{{"op":"health"}}}}"#, id());
    assert_eq!(
        decode_request(version.as_bytes()),
        Err(RefusalToken::UnsupportedVersion)
    );

    let extra_field = format!(
        r#"{{"v":1,"id":"{}","request":{{"op":"health","path":"/"}}}}"#,
        id()
    );
    assert_eq!(
        decode_request(extra_field.as_bytes()),
        Err(RefusalToken::FrameMalformed)
    );
    let extra_frame_field = format!(
        r#"{{"v":1,"id":"{}","request":{{"op":"health"}},"x":1}}"#,
        id()
    );
    assert_eq!(
        decode_request(extra_frame_field.as_bytes()),
        Err(RefusalToken::FrameMalformed)
    );
    let bad_nonce = format!(
        r#"{{"v":1,"id":"{}","request":{{"op":"safety_attest","nonce":"AB"}}}}"#,
        id()
    );
    assert_eq!(
        decode_request(bad_nonce.as_bytes()),
        Err(RefusalToken::FrameMalformed)
    );

    for body in [&b"not json"[..], b"[]", b"{}", b"{\"v\":1}", b"\xff\xfe"] {
        assert_eq!(
            decode_request(body),
            Err(RefusalToken::FrameMalformed),
            "{body:?}"
        );
    }
}

#[test]
fn helper_request_has_no_write_or_free_form_variant() {
    // The variant set is pinned: a new variant must extend this list on purpose.
    let ops: Vec<String> = every_request()
        .iter()
        .map(|r| {
            let value = serde_json::to_value(r).expect("serialises");
            value["op"].as_str().expect("tagged").to_owned()
        })
        .collect();
    let mut distinct = ops.clone();
    distinct.dedup();
    assert_eq!(
        distinct,
        [
            "health",
            "groups",
            "read",
            "safety_attest",
            "safety_public_key"
        ],
        "request variants changed"
    );
    assert_eq!(
        OPERATIONS,
        [
            "health",
            "groups",
            "read",
            "safety_attest",
            "safety_public_key"
        ]
    );
    // No write group can be named in a read request.
    let write = r#"{"op":"read","group":"docker.write"}"#;
    assert!(serde_json::from_str::<HelperRequest>(write).is_err());
    // No free-form field type appears in the request definitions.
    let source = include_str!("request.rs");
    for needle in [": String", "PathBuf", "Vec<String>", "&str,", "OsString"] {
        let hits: Vec<&str> = source.lines().filter(|l| l.contains(needle)).collect();
        assert!(hits.is_empty(), "request.rs has {needle}: {hits:?}");
    }
}

#[test]
fn helper_group_write_set_is_exact() {
    let writes: Vec<HelperGroup> = HelperGroup::ALL
        .iter()
        .copied()
        .filter(|g| g.is_write())
        .collect();
    assert_eq!(
        writes,
        [
            HelperGroup::DockerWrite,
            HelperGroup::SystemdWrite,
            HelperGroup::Nft
        ]
    );
    assert_eq!(HelperGroup::ALL.len(), 9);
    for group in HelperGroup::ALL {
        assert_eq!(HelperGroup::parse(group.as_str()), Some(*group));
        let json = serde_json::to_string(group).expect("serialises");
        assert_eq!(json, format!("\"{}\"", group.as_str()));
    }
    assert_eq!(HelperGroup::parse("docker"), None);
    assert_eq!(HelperGroup::parse("Docker.read"), None);
}

#[test]
fn nonce_requires_64_lowercase_hex() {
    let good = "0123456789abcdef".repeat(4);
    let nonce = Nonce::parse(&good).expect("valid");
    assert_eq!(nonce.to_hex(), good);
    assert_eq!(Nonce::parse(&good.to_uppercase()), Err(NonceError));
    assert_eq!(Nonce::parse(&good[..63]), Err(NonceError));
    assert_eq!(Nonce::parse(&format!("{good}0")), Err(NonceError));
    assert_eq!(Nonce::parse(&"g".repeat(64)), Err(NonceError));
    assert_eq!(Nonce::parse(""), Err(NonceError));
    let json = serde_json::to_string(&nonce).expect("serialises");
    assert_eq!(json, format!("\"{good}\""));
}

#[test]
fn answer_header_allows_a_maximum_size_report_but_a_request_does_not() {
    let wide = (MAX_RESPONSE_BYTES as u32).to_be_bytes();
    assert_eq!(frame_len_response(wide), Ok(MAX_RESPONSE_BYTES));
    assert!(frame_len(wide).is_err());
    assert!(frame_len_response(((MAX_RESPONSE_BYTES as u32) + 1).to_be_bytes()).is_err());
    // A 64 KiB report as hex, with its key and signature, fits one answer.
    let frame = HelperResponseFrame {
        v: HELPER_PROTOCOL,
        id: id(),
        response: HelperResponse::SafetyAttested {
            attestation: SignedAttestation {
                public_key: [1; 32],
                report: vec![b'a'; 64 * 1024],
                signature: [2; 64],
            },
        },
    };
    let bytes = encode_frame(&frame).expect("encodes");
    assert!(bytes.len() - 4 <= MAX_RESPONSE_BYTES);
}

#[test]
fn signed_attestation_refuses_uppercase_and_wrong_length_hex() {
    let ok = format!(
        r#"{{"public_key":"{}","report":"00","signature":"{}"}}"#,
        "11".repeat(32),
        "22".repeat(64)
    );
    assert!(serde_json::from_str::<SignedAttestation>(&ok).is_ok());
    for bad in [
        ok.replace("\"00\"", "\"0A\""),
        ok.replace(&"11".repeat(32), &"11".repeat(31)),
        ok.replace(&"22".repeat(64), &"2A".repeat(64)),
        ok.replace('}', ",\"extra\":1}"),
    ] {
        assert!(
            serde_json::from_str::<SignedAttestation>(&bad).is_err(),
            "{bad}"
        );
    }
}
