//! The relay of `attest_safety`: bytes in, bytes out, and the helper's own tokens as errors.

#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use domovoi_agent::helper_client::{HelperClient, HelperRelay, probe_identity};
use domovoi_agent::link::RequestKind;
use domovoi_agent::link::STATUS_PROBE_INTERVAL;
use domovoi_agent::link::handshake::AgentStatusSource;
use domovoi_agent::requests::handle;
use domovoi_core::helper::{
    HELPER_PROTOCOL, HelperGroup, HelperRequest, HelperResponse, HelperResponseFrame,
    SignedAttestation, UnavailableReason, UnsignedToken, decode_request, encode_frame, frame_len,
};
use domovoi_core::identity::RefusalToken;
use domovoi_core::isolation::{PlatformRecordState, PlatformTier};
use domovoi_proto::messages::{AttestSafety, HelperIdentity, HelperStatus};
use domovoi_proto::{Message, Nonce, PublicKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio_util::sync::CancellationToken;

use crate::common::Fixture;
use crate::harness::{clock, settle};

type Script = Arc<dyn Fn(&HelperRequest) -> HelperResponse + Send + Sync>;

/// Serves one request per connection on `socket`, recording each request it was sent.
fn serve_stub(
    socket: &Path,
    script: Script,
) -> (tokio::task::JoinHandle<()>, Arc<Mutex<Vec<HelperRequest>>>) {
    let seen: Arc<Mutex<Vec<HelperRequest>>> = Arc::default();
    let recorded = seen.clone();
    let listener = UnixListener::bind(socket).unwrap();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut header = [0_u8; 4];
            if stream.read_exact(&mut header).await.is_err() {
                continue;
            }
            let mut body = vec![0_u8; frame_len(header).unwrap()];
            if stream.read_exact(&mut body).await.is_err() {
                continue;
            }
            let frame = decode_request(&body).unwrap();
            recorded.lock().unwrap().push(frame.request.clone());
            let answer = HelperResponseFrame {
                v: HELPER_PROTOCOL,
                id: frame.id,
                response: script(&frame.request),
            };
            let _ = stream.write_all(&encode_frame(&answer).unwrap()).await;
        }
    });
    (task, seen)
}

fn challenge(byte: u8) -> RequestKind {
    RequestKind::Attest(AttestSafety {
        nonce: Nonce([byte; 32]),
    })
}

fn error_code(message: &Message) -> &str {
    match message {
        Message::Error(error) => &error.code,
        other => panic!("expected an error frame, got {}", other.kind()),
    }
}

async fn relay(socket: &Path, request: RequestKind) -> Message {
    let rig = Fixture::new();
    let mut ctx = rig.ctx(Vec::new(), None, Vec::new());
    ctx.helper = HelperRelay::new(socket);
    handle(&ctx, &request).await
}

#[tokio::test]
async fn attest_safety_relays_report_and_signature_bytes_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    // Bytes a careless relay would change: CRLF, trailing space, a NUL and non-UTF-8.
    let report = b"format=1\r\nnote=a \n\x00\xff\xfe\n".to_vec();
    let sent = SignedAttestation {
        public_key: [7; 32],
        report: report.clone(),
        signature: [9; 64],
    };
    let answer = sent.clone();
    let (_stub, seen) = serve_stub(
        &socket,
        Arc::new(move |_| HelperResponse::SafetyAttested {
            attestation: answer.clone(),
        }),
    );
    let reply = relay(&socket, challenge(5)).await;
    let Message::SafetyAttestation(attestation) = &reply else {
        panic!("expected a safety_attestation, got {}", reply.kind());
    };
    assert_eq!(attestation.public_key.as_bytes(), &[7; 32]);
    assert_eq!(attestation.report.as_bytes(), report.as_slice());
    assert_eq!(attestation.signature.as_bytes(), &[9; 64]);
    // The helper was asked with the hub's nonce, byte for byte.
    assert_eq!(
        *seen.lock().unwrap(),
        [HelperRequest::SafetyAttest {
            nonce: domovoi_core::helper::Nonce::from_bytes([5; 32])
        }]
    );
    // And the frame the hub receives decodes to the same bytes.
    let id = domovoi_core::RequestId::new();
    let wire = domovoi_proto::encode(&reply, id, Some(id)).unwrap();
    let decoded = domovoi_proto::decode(&wire).unwrap();
    assert_eq!(decoded.message, reply);
}

#[tokio::test]
async fn attest_safety_with_unsigned_token_replies_error_with_that_code() {
    let dir = tempfile::tempdir().unwrap();
    for token in UnsignedToken::ALL {
        let socket = dir.path().join(format!("{token}.sock"));
        let (_stub, _) = serve_stub(
            &socket,
            Arc::new(move |_| HelperResponse::SafetyUnsigned { token }),
        );
        let reply = relay(&socket, challenge(1)).await;
        assert_eq!(error_code(&reply), token.as_str());
    }
    // A start refusal is relayed under its own token too.
    let socket = dir.path().join("start.sock");
    let (_stub, _) = serve_stub(
        &socket,
        Arc::new(|_| HelperResponse::StartRefused {
            token: RefusalToken::Uninitialized,
            faults: Vec::new(),
        }),
    );
    assert_eq!(
        error_code(&relay(&socket, challenge(1)).await),
        "uninitialized"
    );
}

#[tokio::test]
async fn attest_safety_without_helper_replies_helper_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    // No socket at all.
    let missing = dir.path().join("none.sock");
    assert_eq!(
        error_code(&relay(&missing, challenge(1)).await),
        "helper_unavailable"
    );
    // A helper whose attestation group is off.
    let off = dir.path().join("off.sock");
    let (_stub, _) = serve_stub(
        &off,
        Arc::new(|_| HelperResponse::Unavailable {
            group: HelperGroup::SafetyAttest,
            reason: UnavailableReason::GroupDisabled,
        }),
    );
    assert_eq!(
        error_code(&relay(&off, challenge(1)).await),
        "helper_unavailable"
    );
    // A helper that answers something else.
    let odd = dir.path().join("odd.sock");
    let (_stub, _) = serve_stub(
        &odd,
        Arc::new(|_| HelperResponse::Health {
            protocol: HELPER_PROTOCOL,
            build: "x".to_owned(),
            pid: 1,
        }),
    );
    assert_eq!(
        error_code(&relay(&odd, challenge(1)).await),
        "helper_unavailable"
    );
}

fn key_script(key_hex: String) -> Script {
    Arc::new(move |request| match request {
        HelperRequest::SafetyPublicKey => HelperResponse::SafetyPublicKey {
            public_key: key_hex.clone(),
        },
        HelperRequest::Groups => HelperResponse::Groups {
            enabled: vec![HelperGroup::SafetyAttest],
            ignored: Vec::new(),
            write_opt_in: false,
            platform_tier: PlatformTier::Blocked,
            platform_record: PlatformRecordState::Missing,
        },
        _ => HelperResponse::Health {
            protocol: HELPER_PROTOCOL,
            build: "test".to_owned(),
            pid: 7,
        },
    })
}

#[tokio::test]
async fn agent_announces_helper_public_key_and_no_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let (_stub, _) = serve_stub(&socket, key_script("0b".repeat(32)));
    let client = HelperClient::new(&socket);
    let identity = probe_identity(&client, &*clock()).await;
    assert_eq!(
        identity,
        Some(HelperIdentity {
            public_key: PublicKey([0x0b; 32])
        })
    );
    // The announcement carries the public key and nothing else.
    let json = serde_json::to_value(identity.unwrap()).unwrap();
    assert_eq!(
        json.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["public_key"]
    );

    // A helper without a usable key announces none.
    let none = dir.path().join("none.sock");
    let (_stub, _) = serve_stub(
        &none,
        Arc::new(|_| HelperResponse::SafetyUnsigned {
            token: UnsignedToken::Uninitialized,
        }),
    );
    assert_eq!(
        probe_identity(&HelperClient::new(&none), &*clock()).await,
        None
    );
    // A malformed key is not announced either.
    let bad = dir.path().join("bad.sock");
    let (_stub, _) = serve_stub(&bad, key_script("not-hex".to_owned()));
    assert_eq!(
        probe_identity(&HelperClient::new(&bad), &*clock()).await,
        None
    );
}

#[tokio::test]
async fn the_status_probe_keeps_the_announced_key_current() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let clock = clock();
    let status = AgentStatusSource::default();
    let stop = CancellationToken::new();
    let probe = tokio::spawn(domovoi_agent::helper_client::keep_status(
        HelperClient::new(&socket),
        clock.clone(),
        status.clone(),
        stop.clone(),
    ));
    settle().await;
    assert_eq!(status.current().unwrap().helper_identity, None);

    let (_stub, _) = serve_stub(&socket, key_script("0c".repeat(32)));
    clock.advance(STATUS_PROBE_INTERVAL);
    for _ in 0..20 {
        settle().await;
        if status.current().unwrap().helper_identity.is_some() {
            break;
        }
    }
    let current = status.current().unwrap();
    assert!(matches!(current.helper_status, HelperStatus::Ok { .. }));
    assert_eq!(
        current.helper_identity,
        Some(HelperIdentity {
            public_key: PublicKey([0x0c; 32])
        })
    );
    stop.cancel();
    probe.await.unwrap();
}
