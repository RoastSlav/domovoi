use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use domovoi_config::{HelperConfig, parse_helper};
use domovoi_core::helper::{
    HELPER_PROTOCOL, HelperGroup, HelperRequest, HelperRequestFrame, HelperResponse,
    HelperResponseFrame, Nonce, ReadGroup, UnavailableReason, decode_response, encode_frame,
};
use domovoi_core::identity::{FaultKind, HostDirs, PathId, RefusalToken};
use domovoi_core::isolation::{
    PLATFORM_RECORD_FORMAT, PlatformId, PlatformRecord, PlatformRecordState, PlatformTier,
};
use domovoi_helper::serve::{
    Connection, ServeContext, ServeOutcome, serve_connection, unavailable_reason,
};
use domovoi_testkit::{FakeClock, FakeHostInspect};
use jiff::{SignedDuration, Timestamp};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::UnixStream;
use uuid::Uuid;

use crate::common::{AGENT_ID, healthy, root_dir, root_file};

/// A socket that reports a chosen peer uid, standing in for `SO_PEERCRED`.
struct TestConn {
    stream: UnixStream,
    uid: u32,
}

impl AsyncRead for TestConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for TestConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

impl Connection for TestConn {
    fn peer_uid(&self) -> std::io::Result<u32> {
        Ok(self.uid)
    }

    fn discard_unread(&mut self) {
        self.stream.discard_unread();
    }
}

fn config(text: &str) -> Result<HelperConfig, Vec<domovoi_config::ConfigIssue>> {
    parse_helper(text).map(|(config, _)| config)
}

fn context_with(
    inspect: FakeHostInspect,
    config_text: &str,
    clock: Arc<FakeClock>,
) -> ServeContext {
    ServeContext::new(
        config(config_text),
        Arc::new(inspect),
        clock,
        HostDirs::default(),
        "test-build",
    )
}

fn context(inspect: FakeHostInspect, config_text: &str) -> ServeContext {
    context_with(
        inspect,
        config_text,
        Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH)),
    )
}

fn default_context() -> ServeContext {
    context(healthy(&HostDirs::default()), "")
}

fn request_bytes(request: HelperRequest) -> Vec<u8> {
    let frame = HelperRequestFrame {
        v: HELPER_PROTOCOL,
        id: Uuid::now_v7(),
        request,
    };
    encode_frame(&frame).expect("encode")
}

fn raw_frame(json: &str) -> Vec<u8> {
    let mut out = u32::try_from(json.len())
        .expect("len")
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(json.as_bytes());
    out
}

async fn exchange(
    ctx: &ServeContext,
    uid: u32,
    bytes: &[u8],
) -> (ServeOutcome, Option<HelperResponseFrame>) {
    let (mut client, server) = UnixStream::pair().expect("pair");
    client.write_all(bytes).await.expect("send");
    let outcome = serve_connection(
        TestConn {
            stream: server,
            uid,
        },
        ctx,
    )
    .await;
    let mut answer = Vec::new();
    client.read_to_end(&mut answer).await.expect("read answer");
    let frame = (answer.len() > 4).then(|| decode_response(&answer[4..]).expect("answer decodes"));
    (outcome, frame)
}

async fn ask(ctx: &ServeContext, request: HelperRequest) -> HelperResponse {
    let (outcome, frame) = exchange(ctx, AGENT_ID, &request_bytes(request)).await;
    assert_eq!(outcome, ServeOutcome::Answered);
    frame.expect("an answer").response
}

fn nonce() -> Nonce {
    Nonce::from_bytes([7; 32])
}

#[tokio::test]
async fn serve_answers_health_and_groups() {
    let ctx = default_context();
    match ask(&ctx, HelperRequest::Health).await {
        HelperResponse::Health {
            protocol,
            build,
            pid,
        } => {
            assert_eq!(protocol, HELPER_PROTOCOL);
            assert_eq!(build, "test-build");
            assert_eq!(pid, std::process::id());
        }
        other => panic!("unexpected {other:?}"),
    }
    match ask(&ctx, HelperRequest::Groups).await {
        HelperResponse::Groups {
            enabled,
            ignored,
            write_opt_in,
            platform_tier,
            platform_record,
        } => {
            assert_eq!(enabled.len(), 6);
            assert!(enabled.iter().all(|g| !g.is_write()));
            assert!(ignored.is_empty());
            assert!(!write_opt_in);
            assert_eq!(platform_tier, PlatformTier::Blocked);
            assert_eq!(platform_record, PlatformRecordState::Missing);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn serve_echoes_the_request_id() {
    let ctx = default_context();
    let id = Uuid::now_v7();
    let frame = HelperRequestFrame {
        v: HELPER_PROTOCOL,
        id,
        request: HelperRequest::Health,
    };
    let (_, answer) = exchange(&ctx, AGENT_ID, &encode_frame(&frame).expect("encode")).await;
    assert_eq!(answer.expect("answer").id, id);
}

#[tokio::test]
async fn serve_answers_read_and_safety_attest_as_unavailable() {
    let ctx = default_context();
    assert_eq!(
        ask(
            &ctx,
            HelperRequest::Read {
                group: ReadGroup::DockerRead
            }
        )
        .await,
        HelperResponse::Unavailable {
            group: HelperGroup::DockerRead,
            reason: UnavailableReason::NotImplemented,
        }
    );
    assert_eq!(
        ask(&ctx, HelperRequest::SafetyAttest { nonce: nonce() }).await,
        HelperResponse::Unavailable {
            group: HelperGroup::SafetyAttest,
            reason: UnavailableReason::NotImplemented,
        }
    );
}

#[tokio::test]
async fn serve_marks_a_disabled_group_group_disabled() {
    let ctx = context(
        healthy(&HostDirs::default()),
        "groups = [\"docker.read\"]\n",
    );
    assert_eq!(
        ask(
            &ctx,
            HelperRequest::Read {
                group: ReadGroup::SmartRead
            }
        )
        .await,
        HelperResponse::Unavailable {
            group: HelperGroup::SmartRead,
            reason: UnavailableReason::GroupDisabled,
        }
    );
    assert_eq!(
        ask(&ctx, HelperRequest::SafetyAttest { nonce: nonce() }).await,
        HelperResponse::Unavailable {
            group: HelperGroup::SafetyAttest,
            reason: UnavailableReason::GroupDisabled,
        }
    );
}

// covers: peer_not_agent
#[tokio::test]
async fn helper_rejects_wrong_peer_uid() {
    let ctx = default_context();
    let (outcome, frame) = exchange(&ctx, 1234, &request_bytes(HelperRequest::Health)).await;
    assert_eq!(outcome, ServeOutcome::Answered);
    assert_eq!(
        frame.expect("answer").response,
        HelperResponse::Refused {
            token: RefusalToken::PeerNotAgent
        }
    );
}

// covers: peer_not_agent
#[tokio::test]
async fn helper_does_not_serve_root_peer() {
    let ctx = default_context();
    let (_, frame) = exchange(&ctx, 0, &request_bytes(HelperRequest::Groups)).await;
    assert_eq!(
        frame.expect("answer").response,
        HelperResponse::Refused {
            token: RefusalToken::PeerNotAgent
        }
    );
}

#[tokio::test]
async fn helper_does_not_serve_anyone_when_the_agent_account_is_missing() {
    let ctx = context(FakeHostInspect::new(), "");
    let (_, frame) = exchange(&ctx, AGENT_ID, &request_bytes(HelperRequest::Health)).await;
    assert_eq!(
        frame.expect("answer").response,
        HelperResponse::Refused {
            token: RefusalToken::PeerNotAgent
        }
    );
}

// covers: frame_too_large
#[tokio::test]
async fn serve_refuses_oversized_frame_without_reading_body() {
    let ctx = default_context();
    // Only the header is sent and the peer stays connected: reading the body would block forever.
    let header = 70_000_u32.to_be_bytes();
    let (outcome, frame) = exchange(&ctx, AGENT_ID, &header).await;
    assert_eq!(outcome, ServeOutcome::Answered);
    assert_eq!(
        frame.expect("answer").response,
        HelperResponse::Refused {
            token: RefusalToken::FrameTooLarge
        }
    );
}

// covers: unknown_operation
// covers: frame_malformed
// covers: unsupported_version
#[tokio::test]
async fn serve_refuses_unknown_operation_and_unknown_field() {
    let ctx = default_context();
    let id = Uuid::now_v7();
    let cases = [
        (
            format!(r#"{{"v":1,"id":"{id}","request":{{"op":"restart"}}}}"#),
            RefusalToken::UnknownOperation,
        ),
        (
            format!(r#"{{"v":1,"id":"{id}","request":{{"op":"health","path":"/x"}}}}"#),
            RefusalToken::FrameMalformed,
        ),
        (
            format!(r#"{{"v":1,"id":"{id}","extra":1,"request":{{"op":"health"}}}}"#),
            RefusalToken::FrameMalformed,
        ),
        (
            format!(r#"{{"v":2,"id":"{id}","request":{{"op":"health"}}}}"#),
            RefusalToken::UnsupportedVersion,
        ),
        ("not json".to_owned(), RefusalToken::FrameMalformed),
    ];
    for (json, token) in cases {
        let (outcome, frame) = exchange(&ctx, AGENT_ID, &raw_frame(&json)).await;
        assert_eq!(outcome, ServeOutcome::Answered, "{json}");
        assert_eq!(
            frame.expect("answer").response,
            HelperResponse::Refused { token },
            "{json}"
        );
    }
}

#[tokio::test]
async fn serve_closes_a_silent_peer_at_the_deadline_on_the_fake_clock() {
    let clock = Arc::new(FakeClock::new(Timestamp::UNIX_EPOCH));
    let ctx = Arc::new(context_with(
        healthy(&HostDirs::default()),
        "",
        Arc::clone(&clock),
    ));
    let (client, server) = UnixStream::pair().expect("pair");
    let served = Arc::clone(&ctx);
    let task = tokio::spawn(async move {
        serve_connection(
            TestConn {
                stream: server,
                uid: AGENT_ID,
            },
            &served,
        )
        .await
    });
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(!task.is_finished(), "the helper waits for the peer");
    clock.advance(SignedDuration::from_secs(9));
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert!(!task.is_finished(), "nine seconds is inside the deadline");
    clock.advance(SignedDuration::from_secs(1));
    assert_eq!(task.await.expect("task"), ServeOutcome::TimedOut);
    drop(client);
}

// covers: helper_state_permissions
#[tokio::test]
async fn serve_answers_start_refused_with_every_fault_after_the_peer_check() {
    let d = HostDirs::default();
    let host = healthy(&d)
        .path(&d.helper_dir, root_dir(0o755))
        .path(d.helper_dir.join("state.lock"), root_file(0o644));
    let ctx = context(host, "");
    let expected = |response: HelperResponse| match response {
        HelperResponse::StartRefused { token, faults } => {
            assert_eq!(token, RefusalToken::HelperStatePermissions);
            let ids: Vec<PathId> = faults.iter().map(|f| f.id).collect();
            assert_eq!(ids, [PathId::HelperDir, PathId::HelperLock]);
            assert!(faults.iter().all(|f| f.why == FaultKind::Mode));
        }
        other => panic!("unexpected {other:?}"),
    };
    expected(ask(&ctx, HelperRequest::Health).await);
    expected(ask(&ctx, HelperRequest::Groups).await);

    // A request that is not even understood gets the same answer.
    let (_, frame) = exchange(&ctx, AGENT_ID, &raw_frame("not json")).await;
    expected(frame.expect("answer").response);

    // The peer check still comes first and reveals nothing.
    let (_, frame) = exchange(&ctx, 1234, &request_bytes(HelperRequest::Health)).await;
    assert_eq!(
        frame.expect("answer").response,
        HelperResponse::Refused {
            token: RefusalToken::PeerNotAgent
        }
    );
}

#[tokio::test]
async fn serve_answers_start_refused_for_an_unusable_configuration() {
    let ctx = context(healthy(&HostDirs::default()), "bogus = true\n");
    assert_eq!(
        ask(&ctx, HelperRequest::Health).await,
        HelperResponse::StartRefused {
            token: RefusalToken::HelperConfigInvalid,
            faults: Vec::new(),
        }
    );
}

#[tokio::test]
async fn helper_ignores_write_groups_named_in_config() {
    let ctx = context(
        healthy(&HostDirs::default()),
        "groups = [\"docker.read\", \"docker.write\", \"systemd.write\", \"nft\"]\n",
    );
    match ask(&ctx, HelperRequest::Groups).await {
        HelperResponse::Groups {
            enabled, ignored, ..
        } => {
            assert_eq!(enabled, [HelperGroup::DockerRead]);
            assert_eq!(
                ignored,
                [
                    HelperGroup::DockerWrite,
                    HelperGroup::SystemdWrite,
                    HelperGroup::Nft
                ]
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn record(kernel: &str, tier: PlatformTier) -> String {
    let record = PlatformRecord {
        format: PLATFORM_RECORD_FORMAT,
        recorded_at: Timestamp::UNIX_EPOCH,
        kernel_release: kernel.to_owned(),
        platform: PlatformId {
            os_id: "ubuntu".to_owned(),
            version_id: "24.04".to_owned(),
            arch: "x86_64".to_owned(),
        },
        tier,
        blocking: Vec::new(),
    };
    serde_json::to_string(&record).expect("encode record")
}

async fn groups_with_record(text: Option<String>) -> (PlatformRecordState, PlatformTier) {
    let d = HostDirs::default();
    let mut host = healthy(&d).kernel_release_text("6.8.0-test");
    if let Some(text) = text {
        host = host.text(d.helper_dir.join("platform.json"), text);
    }
    match ask(&context(host, ""), HelperRequest::Groups).await {
        HelperResponse::Groups {
            platform_tier,
            platform_record,
            ..
        } => (platform_record, platform_tier),
        other => panic!("unexpected {other:?}"),
    }
}

// covers: platform_record_missing
#[tokio::test]
async fn helper_groups_report_platform_record_missing() {
    for text in [None, Some("{not json".to_owned())] {
        assert_eq!(
            groups_with_record(text).await,
            (PlatformRecordState::Missing, PlatformTier::Blocked)
        );
    }
}

// covers: platform_record_stale
#[tokio::test]
async fn helper_groups_report_platform_record_stale() {
    assert_eq!(
        groups_with_record(Some(record("6.1.0-old", PlatformTier::Untested))).await,
        (PlatformRecordState::Stale, PlatformTier::Blocked)
    );
}

#[tokio::test]
async fn helper_groups_report_the_recorded_tier_when_the_kernel_matches() {
    assert_eq!(
        groups_with_record(Some(record("6.8.0-test", PlatformTier::Untested))).await,
        (PlatformRecordState::Ok, PlatformTier::Untested)
    );
}

#[test]
fn write_groups_are_refused_by_record_and_opt_in() {
    use PlatformRecordState::{Missing, Ok as RecordOk, Stale};
    use PlatformTier::{Blocked, Supported, Untested};
    let reason = |record, tier, opt_in| {
        unavailable_reason(HelperGroup::DockerWrite, &[], record, tier, opt_in)
    };
    assert_eq!(
        reason(Missing, Untested, true),
        UnavailableReason::PlatformBlocked
    );
    assert_eq!(
        reason(Stale, Supported, true),
        UnavailableReason::PlatformBlocked
    );
    assert_eq!(
        reason(RecordOk, Blocked, true),
        UnavailableReason::PlatformBlocked
    );
    assert_eq!(
        reason(RecordOk, Untested, false),
        UnavailableReason::WriteGroupNeedsOptIn
    );
    assert_eq!(
        reason(RecordOk, Untested, true),
        UnavailableReason::NotImplemented
    );
    assert_eq!(
        reason(RecordOk, Supported, false),
        UnavailableReason::NotImplemented
    );
}
