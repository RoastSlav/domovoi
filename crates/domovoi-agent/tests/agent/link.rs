//! Enrollment, certificate pinning, the handshake, reconnects and heartbeats.

use std::sync::Arc;

use domovoi_agent::enroll::{EnrollEnv, enroll};
use domovoi_agent::identity::{Identity, generate_key};
use domovoi_agent::link::RequestKind;
use domovoi_agent::link::status;
use domovoi_agent::link::tls::PinnedVerifier;
use domovoi_agent::link::ws::{Connector, TlsConnector};
use domovoi_agent::{EnrollError, LinkError};
use domovoi_config::{AgentConfig, MapEnv};
use domovoi_core::ReadToolCall;
use domovoi_core::{AgentId, HostId};
use domovoi_proto::messages::{
    Enrolled, Ping, RejectReason, RunTool, ToolFailure, ToolFailureCode, ToolOutcome, ToolResult,
};
use domovoi_proto::{
    CertFingerprint, EnrollToken, Message, PROTOCOL_VERSION, ProtocolVersion, Release,
    challenge_message,
};
use domovoi_testkit::FakeClock;
use domovoi_testkit::build::T0;
use ed25519_dalek::{Signature, VerifyingKey};
use jiff::SignedDuration;

use crate::harness::{clock, empty_fs, identity, settle, start_link};
use crate::hub::{TestHub, guard};

pub fn config(dir: &tempfile::TempDir, hub: &TestHub) -> AgentConfig {
    let text = format!(
        "[agent]\nhost_name = \"nas\"\nhub_url = \"{}\"\nstate_dir = '{}'\nskills_dir = '{}'\n",
        hub.url,
        dir.path().display(),
        dir.path().join("skills").display(),
    );
    domovoi_config::parse(&text, &MapEnv::new()).unwrap().0
}

/// Advances `clock` in `step`s, letting other tasks run in between, until `fut` finishes.
/// Returns its output and how far the clock moved.
pub async fn advancing<F: std::future::Future>(
    clock: &FakeClock,
    step: SignedDuration,
    fut: F,
) -> (F::Output, SignedDuration) {
    tokio::pin!(fut);
    let mut moved = SignedDuration::ZERO;
    guard(async {
        loop {
            tokio::select! {
                biased;
                out = &mut fut => return (out, moved),
                () = settle() => {
                    clock.advance(step);
                    moved += step;
                }
            }
        }
    })
    .await
}

#[tokio::test]
async fn enrollment_pins_fingerprint_and_persists_identity_with_0600() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let config = config(&dir, &hub);
    let (agent_id, host_id) = (AgentId::new(), HostId::new());
    let pin = hub.fingerprint;
    let url = hub.url.clone();
    let hub_side = async {
        let mut conn = hub.accept().await;
        let request = conn.expect().await;
        let Message::Enroll(enroll) = request.message else {
            panic!("expected enroll");
        };
        assert_eq!(enroll.token.expose(), "one-time-token");
        assert_eq!(enroll.host_name.as_str(), "nas");
        let reply = Enrolled {
            agent_id,
            host_id,
            hub_cert_sha256: CertFingerprint(pin),
        };
        conn.send(Message::Enrolled(reply), Some(request.id)).await;
        enroll.public_key
    };
    let clock = clock();
    let fs = empty_fs(&dir);
    let env = EnrollEnv {
        clock: &*clock,
        fs: &*fs,
    };
    let agent_side = enroll(
        &url,
        EnrollToken::new("one-time-token"),
        None,
        &config,
        dir.path(),
        env,
    );
    let (identity, public_key) = tokio::join!(agent_side, hub_side);
    let identity = identity.unwrap();
    assert_eq!(identity.hub_fingerprint, pin);
    assert_eq!(identity.public_key(), public_key);

    let loaded = Identity::load(dir.path()).unwrap().unwrap();
    assert_eq!(loaded.agent_id, agent_id);
    assert_eq!(loaded.host_id, host_id);
    assert_eq!(loaded.hub_fingerprint, pin);
    assert_eq!(loaded.public_key(), public_key);
    assert!(!format!("{loaded:?}").contains(&hex::encode(public_key.as_bytes())));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(Identity::path(dir.path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[tokio::test]
async fn enrollment_aborts_on_unexpected_fingerprint() {
    let hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let config = config(&dir, &hub);
    let clock = clock();
    let fs = empty_fs(&dir);
    let env = EnrollEnv {
        clock: &*clock,
        fs: &*fs,
    };
    let result = enroll(
        &hub.url,
        EnrollToken::new("one-time-token"),
        Some([7; 32]),
        &config,
        dir.path(),
        env,
    )
    .await;
    assert!(
        matches!(
            result,
            Err(EnrollError::Link(LinkError::CertificateMismatch))
        ),
        "{result:?}"
    );
    assert_eq!(hub.accepted(), 0, "nothing may reach an unexpected hub");
    assert!(Identity::load(dir.path()).unwrap().is_none());
}

#[tokio::test]
async fn link_refuses_hub_with_different_certificate() {
    let hub = TestHub::start().await;
    let mut pin = hub.fingerprint;
    pin[0] ^= 1;
    let connector = TlsConnector::new(hub.url.clone(), Arc::new(PinnedVerifier::new(pin)), clock());
    let result = connector.connect().await;
    assert!(matches!(result, Err(LinkError::CertificateMismatch)));
    assert_eq!(hub.accepted(), 0);
}

#[tokio::test]
async fn handshake_signs_challenge_with_enrolled_key() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let link = start_link(&hub, &identity, &clock(), &dir);
    let mut conn = hub.accept().await;
    let (hello, nonce) = conn.challenge(&identity).await;
    assert_eq!(hello.protocol, PROTOCOL_VERSION);
    let Message::ChallengeResponse(response) = conn.expect().await.message else {
        panic!("expected challenge_response");
    };
    let key = VerifyingKey::from_bytes(identity.public_key().as_bytes()).unwrap();
    let signature = Signature::from_bytes(response.signature.as_bytes());
    let bound = challenge_message(&nonce, identity.agent_id, &conn.session_hint);
    assert!(key.verify_strict(&bound, &signature).is_ok());
    // The signature covers this TLS session, so it is useless on any other.
    let elsewhere = challenge_message(&nonce, identity.agent_id, &"00".repeat(32));
    assert!(key.verify_strict(&elsewhere, &signature).is_err());
    let other_key = VerifyingKey::from_bytes(&generate_key().verifying_key().to_bytes()).unwrap();
    assert!(other_key.verify_strict(&bound, &signature).is_err());
    link.shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

#[tokio::test]
async fn revoked_reject_stops_retrying() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let mut conn = hub.accept().await;
    conn.reject(&identity, RejectReason::Revoked).await;
    let result = guard(link.task).await.unwrap();
    assert!(
        matches!(result, Err(LinkError::Revoked("revoked"))),
        "{result:?}"
    );
    clock.advance(SignedDuration::from_secs(600));
    settle().await;
    assert_eq!(hub.accepted(), 1);
}

/// A too-old reject from a hub that still sends the legacy reason leaves the agent idle,
/// not stopped and restarted.
#[tokio::test]
async fn agent_too_old_reject_from_older_hub_is_idle() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let shutdown = link.shutdown.clone();
    let mut conn = hub.accept().await;
    conn.reject(
        &identity,
        RejectReason::AgentTooOld {
            minimum: Release {
                major: 0,
                minor: 0,
                patch: 5,
            },
        },
    )
    .await;
    settle().await;
    assert!(!link.task.is_finished(), "the agent stays up idle");
    let err = LinkError::AgentTooOld {
        agent_version: "0.0.1".to_owned(),
        minimum: Release {
            major: 0,
            minor: 0,
            patch: 5,
        },
    };
    assert!(err.is_idle() && !err.is_final());
    let said = err.to_string();
    assert!(said.contains("update the agent"), "{said}");
    shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

#[tokio::test]
async fn incompatible_reject_is_idle_not_final() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let shutdown = link.shutdown.clone();
    let mut conn = hub.accept().await;
    let hub_version = ProtocolVersion { major: 2, minor: 0 };
    conn.reject(&identity, RejectReason::Incompatible { hub: hub_version })
        .await;
    settle().await;
    assert!(!link.task.is_finished(), "the agent stays up idle");
    let err = LinkError::Incompatible {
        hub: hub_version,
        agent: PROTOCOL_VERSION,
    };
    assert!(err.is_idle());
    assert!(!err.is_final(), "an upgrade and restart ends the state");
    assert!(err.to_string().contains("upgrade"), "{err}");
    shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

#[tokio::test]
async fn incompatible_agent_idles_without_reconnecting() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let shutdown = link.shutdown.clone();
    let mut conn = hub.accept().await;
    conn.reject(
        &identity,
        RejectReason::Incompatible {
            hub: ProtocolVersion { major: 2, minor: 0 },
        },
    )
    .await;
    settle().await;
    assert!(!link.task.is_finished(), "the link did not exit");
    for _ in 0..3 {
        clock.advance(SignedDuration::from_hours(1));
        settle().await;
    }
    assert_eq!(hub.accepted(), 1, "an idle agent never reconnects");
    assert!(!link.task.is_finished());
    shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

#[tokio::test]
async fn idle_incompatible_agent_logs_and_updates_status_hourly() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let shutdown = link.shutdown.clone();
    let mut conn = hub.accept().await;
    conn.reject(
        &identity,
        RejectReason::Incompatible {
            hub: ProtocolVersion { major: 2, minor: 0 },
        },
    )
    .await;
    settle().await;
    let before = status::read(dir.path()).unwrap().expect("status written");
    assert_eq!(before.state, "incompatible");
    assert_eq!(before.agent_protocol.as_deref(), Some("1.1"));
    assert_eq!(before.hub_protocol.as_deref(), Some("2.0"));
    let first = before.last_notice_at.expect("the first notice is timed");
    clock.advance(SignedDuration::from_hours(1));
    settle().await;
    let after = status::read(dir.path()).unwrap().expect("status rewritten");
    assert!(
        after.last_notice_at.expect("the hourly notice is timed") > first,
        "the idle agent says hourly that it is still waiting"
    );
    shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

/// A hub release is not a compatibility dimension: any welcome is accepted.
#[tokio::test]
async fn welcome_from_any_hub_release_is_accepted() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let mut conn = hub.accept().await;
    conn.welcome_from_release(&identity, 3600, "0.0.1").await;
    let (first, _) = advancing(&clock, SignedDuration::from_secs(1), conn.expect()).await;
    assert!(
        matches!(first.message, Message::Ping(Ping { .. })),
        "the session starts and heartbeats as usual"
    );
    link.shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

#[tokio::test]
async fn link_status_written_on_connect_and_drop() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let shutdown = link.shutdown.clone();
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;
    settle().await;
    let connected = status::read(dir.path()).unwrap().expect("status written");
    assert_eq!(connected.state, "connected");
    assert!(connected.connected_since.is_some());

    drop(conn);
    settle().await;
    let dropped = status::read(dir.path()).unwrap().expect("status rewritten");
    assert_eq!(dropped.state, "reconnecting");
    assert!(dropped.connected_since.is_none());
    shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

/// A hub whose release does not parse, as a development build's does, is never locked out.
#[tokio::test]
async fn unparsable_hub_release_still_connects() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let mut conn = hub.accept().await;
    conn.welcome_from_release(&identity, 2, "dev").await;
    let (first, _) = advancing(&clock, SignedDuration::from_secs(1), conn.expect()).await;
    let kind = first.message;
    assert!(
        matches!(kind, Message::Ping(Ping { .. })),
        "an unparsable release is unknown, not old: the agent heartbeats as usual: {kind:?}",
    );
    link.shutdown.cancel();
    assert!(
        guard(link.task).await.unwrap().is_ok(),
        "and the agent keeps running"
    );
}

#[tokio::test]
async fn reconnects_with_backoff_after_drop() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let step = SignedDuration::from_millis(100);
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;
    let mut delays = Vec::new();
    for _ in 0..3 {
        drop(conn);
        let (next, waited) = advancing(&clock, step, hub.accept()).await;
        delays.push(waited);
        conn = next;
        conn.welcome(&identity, 3600).await;
    }
    for (waited, expected) in delays.iter().zip([1, 2, 4]) {
        let expected = SignedDuration::from_secs(expected);
        assert!(*waited >= expected, "{delays:?}");
        assert!(
            *waited <= expected + SignedDuration::from_millis(500),
            "{delays:?}"
        );
    }
    assert_eq!(hub.accepted(), 4);
    link.shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}

#[tokio::test]
async fn backoff_resets_after_stable_connection() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let step = SignedDuration::from_millis(100);
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;
    drop(conn);
    let (mut conn, _) = advancing(&clock, step, hub.accept()).await;
    conn.welcome(&identity, 3600).await;
    // This connection stays up for two minutes, so the next delay starts at 1 s again.
    settle().await;
    clock.advance(SignedDuration::from_secs(120));
    settle().await;
    drop(conn);
    let (_conn, waited) = advancing(&clock, step, hub.accept()).await;
    assert!(waited <= SignedDuration::from_millis(1500), "{waited:?}");
    link.shutdown.cancel();
}

#[tokio::test]
async fn heartbeat_drops_link_after_three_missed_pongs() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let clock = clock();
    let link = start_link(&hub, &identity, &clock, &dir);
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 10).await;
    let step = SignedDuration::from_secs(1);

    // An answered ping keeps the link up.
    let (ping, _) = advancing(&clock, step, conn.expect()).await;
    assert!(matches!(ping.message, Message::Ping(_)));
    conn.send(
        Message::Pong(domovoi_proto::messages::Pong {
            at: clock_now(&clock),
        }),
        Some(ping.id),
    )
    .await;

    for _ in 0..3 {
        let (ping, _) = advancing(&clock, step, conn.expect()).await;
        assert!(matches!(ping.message, Message::Ping(Ping { .. })));
    }
    let (closed, _) = advancing(&clock, step, conn.recv()).await;
    assert!(
        closed.is_none(),
        "the link should give up after three missed pongs"
    );
    link.shutdown.cancel();
}

fn clock_now(clock: &FakeClock) -> jiff::Timestamp {
    domovoi_core::ports::Clock::now(clock)
}

#[tokio::test]
async fn link_forwards_requests_and_answers_unknown_kinds() {
    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    let mut link = start_link(&hub, &identity, &clock(), &dir);
    let mut conn = hub.accept().await;
    conn.welcome(&identity, 3600).await;

    let run = RunTool {
        call: ReadToolCall::HostOverview,
        deadline: T0,
    };
    conn.send(Message::RunTool(run.clone()), None).await;
    let request = guard(link.requests.recv()).await.unwrap();
    assert_eq!(request.kind, RequestKind::Tool(run));

    // Replies go out through the handle with the request's id.
    let reply = Message::ToolResult(ToolResult {
        outcome: ToolOutcome::Err(ToolFailure {
            code: ToolFailureCode::Unavailable,
            message: "test".to_owned(),
        }),
    });
    assert!(link.handle.send(reply, Some(request.id)));
    let answer = conn.expect().await;
    assert_eq!(answer.reply_to, Some(request.id));

    // A kind from a newer hub gets `unsupported`, and the link stays up.
    let frame =
        r#"{"v":1,"id":"0190a0a0-0000-7000-8000-000000000001","kind":"teleport","payload":{}}"#;
    conn.transport.send(frame.to_owned()).await.unwrap();
    let Message::Unsupported(unsupported) = conn.expect().await.message else {
        panic!("expected unsupported");
    };
    assert_eq!(unsupported.kind, "teleport");
    link.shutdown.cancel();
    assert!(guard(link.task).await.unwrap().is_ok());
}
