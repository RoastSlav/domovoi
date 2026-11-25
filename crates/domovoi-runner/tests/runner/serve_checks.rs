//! Frames, the peer rule, the start checks, health and probe. Linux only.

use std::time::Duration;

use domovoi_core::skill_runner::{
    Header, ProbeRequest, RUNNER_PROTOCOL, RefusalCode, RunnerReply, RunnerRequest,
};
use domovoi_runner::serve::{RequestKind, peer_allowed};
use tokio::io::AsyncWriteExt;

use crate::serve_common::{
    AGENT_UID, Env, SKILL_ID, SkillSpec, StubProbe, connect, finish, good_inspect, inspect_with,
    proc_status, read_reply, roundtrip, roundtrip_as, run_request, sample_report, send,
    write_skill,
};

const ZERO: &str = "0000000000000000";

fn health() -> RunnerRequest {
    RunnerRequest::Health(Header {
        protocol: RUNNER_PROTOCOL,
    })
}

fn probe(helper_pid: Option<u32>) -> RunnerRequest {
    RunnerRequest::Probe(ProbeRequest {
        protocol: RUNNER_PROTOCOL,
        helper_pid,
    })
}

fn code_of(reply: RunnerReply) -> RefusalCode {
    match reply {
        RunnerReply::Refused(refusal) => refusal.code,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Serves `bytes` written raw by a client that then keeps the connection open.
async fn raw_bad_request(env: &Env, bytes: &[u8], half_close: bool) -> RunnerReply {
    let mut conn = connect(env.ctx(), Some(AGENT_UID));
    conn.client.write_all(bytes).await.unwrap();
    if half_close {
        conn.client.shutdown().await.unwrap();
    }
    let reply = read_reply(&mut conn.client).await.expect("no reply");
    finish(conn.server).await;
    reply
}

fn frame_of(body: &[u8]) -> Vec<u8> {
    let mut frame = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
    frame.extend_from_slice(body);
    frame
}

#[tokio::test]
async fn runner_refuses_bad_frames_oversize_unknown_field_and_slow_requests() {
    let env = Env::new();
    // Empty frame.
    let reply = raw_bad_request(&env, &0_u32.to_be_bytes(), false).await;
    assert_eq!(code_of(reply), RefusalCode::BadRequest);
    // Over MAX_REQUEST_BYTES, refused before any body is read.
    let reply = raw_bad_request(&env, &70_000_u32.to_be_bytes(), false).await;
    assert_eq!(code_of(reply), RefusalCode::BadRequest);
    // Unknown field.
    let unknown = frame_of(br#"{"type":"health","protocol":1,"extra":true}"#);
    assert_eq!(
        code_of(raw_bad_request(&env, &unknown, false).await),
        RefusalCode::BadRequest
    );
    // Not JSON.
    let junk = frame_of(b"this is not json");
    assert_eq!(
        code_of(raw_bad_request(&env, &junk, false).await),
        RefusalCode::BadRequest
    );
    // A body shorter than the header announced.
    let mut short = 50_u32.to_be_bytes().to_vec();
    short.extend_from_slice(b"{\"type\"");
    assert_eq!(
        code_of(raw_bad_request(&env, &short, true).await),
        RefusalCode::BadRequest
    );
    // Slow: nothing arrives within the (short, test-sized) read deadline.
    let mut ctx = env.ctx();
    ctx.request_timeout = Duration::from_millis(200);
    let mut conn = connect(ctx, Some(AGENT_UID));
    let reply = read_reply(&mut conn.client).await.expect("no reply");
    assert_eq!(code_of(reply), RefusalCode::BadRequest);
    finish(conn.server).await;
}

#[tokio::test]
async fn runner_refuses_protocol_mismatch() {
    let env = Env::new();
    let reply = roundtrip(&env, RunnerRequest::Health(Header { protocol: 99 })).await;
    assert_eq!(code_of(reply), RefusalCode::ProtocolMismatch);
    write_skill(env.skills.path(), "versioned", &SkillSpec::new("echo hi")).await;
    let mut request = run_request(env.skills.path(), "versioned", &[]);
    request.protocol = RUNNER_PROTOCOL + 1;
    let reply = roundtrip(&env, RunnerRequest::Run(request)).await;
    assert_eq!(code_of(reply), RefusalCode::ProtocolMismatch);
}

#[test]
fn peer_rule_allows_agent_for_run_and_root_only_for_health_and_probe() {
    let agent = Some(AGENT_UID);
    for kind in [RequestKind::Run, RequestKind::Health, RequestKind::Probe] {
        assert!(peer_allowed(Some(AGENT_UID), kind, agent), "{kind:?}");
        assert!(!peer_allowed(Some(1000), kind, agent), "{kind:?} other uid");
        assert!(!peer_allowed(None, kind, agent), "{kind:?} unknown peer");
    }
    assert!(!peer_allowed(Some(0), RequestKind::Run, agent));
    assert!(peer_allowed(Some(0), RequestKind::Health, agent));
    assert!(peer_allowed(Some(0), RequestKind::Probe, agent));
    // When the agent account cannot be resolved nobody may run, root may still look.
    assert!(!peer_allowed(Some(AGENT_UID), RequestKind::Run, None));
    assert!(!peer_allowed(Some(0), RequestKind::Run, None));
    assert!(peer_allowed(Some(0), RequestKind::Health, None));
    // An agent account that resolves to uid 0 is not trusted to run skills.
    assert!(!peer_allowed(Some(0), RequestKind::Run, Some(0)));
}

#[tokio::test]
async fn peer_rule_is_applied_to_every_request_kind_on_the_wire() {
    let env = Env::new();
    write_skill(env.skills.path(), "guarded", &SkillSpec::new("echo hi")).await;
    let request = run_request(env.skills.path(), "guarded", &[]);
    for peer in [Some(0), Some(1000), None] {
        let reply = roundtrip_as(&env, peer, RunnerRequest::Run(request.clone())).await;
        assert_eq!(code_of(reply), RefusalCode::PeerNotAllowed, "{peer:?}");
    }
    let reply = roundtrip_as(&env, Some(1000), health()).await;
    assert_eq!(code_of(reply), RefusalCode::PeerNotAllowed);
    let reply = roundtrip_as(&env, Some(1000), probe(None)).await;
    assert_eq!(code_of(reply), RefusalCode::PeerNotAllowed);
    assert!(matches!(
        roundtrip_as(&env, Some(0), health()).await,
        RunnerReply::Health(_)
    ));
    assert!(matches!(
        roundtrip_as(&env, Some(0), probe(None)).await,
        RunnerReply::Probe(_)
    ));
}

async fn health_reply_with(env: &Env, groups: &[u32], status: &str) -> RunnerReply {
    let ctx = env.ctx_with(inspect_with(groups, status), StubProbe::new());
    let mut conn = connect(ctx, Some(AGENT_UID));
    send(&mut conn.client, &health()).await;
    let reply = read_reply(&mut conn.client).await.unwrap();
    finish(conn.server).await;
    reply
}

#[tokio::test]
async fn runner_start_check_accepts_primary_group_only() {
    let env = Env::new();
    for groups in [vec![SKILL_ID], vec![]] {
        let reply = health_reply_with(&env, &groups, &proc_status(ZERO, ZERO, 1)).await;
        assert!(
            matches!(reply, RunnerReply::Health(_)),
            "{groups:?}: {reply:?}"
        );
    }
}

#[tokio::test]
async fn runner_start_check_rejects_extra_group() {
    let env = Env::new();
    let reply = health_reply_with(&env, &[SKILL_ID, 4242], &proc_status(ZERO, ZERO, 1)).await;
    let RunnerReply::Refused(refusal) = reply else {
        panic!("not refused: {reply:?}");
    };
    assert_eq!(refusal.code, RefusalCode::RunnerGroupsExtra);
    assert!(refusal.detail.contains("4242"), "{}", refusal.detail);
}

#[tokio::test]
async fn runner_start_check_rejects_nonzero_capabilities() {
    let env = Env::new();
    for status in [
        proc_status("0000000000000400", ZERO, 1),
        proc_status(ZERO, "000001ffffffffff", 1),
    ] {
        let reply = health_reply_with(&env, &[SKILL_ID], &status).await;
        assert_eq!(code_of(reply), RefusalCode::RunnerPrivileges);
    }
}

#[tokio::test]
async fn runner_start_check_requires_no_new_privs() {
    let env = Env::new();
    let reply = health_reply_with(&env, &[SKILL_ID], &proc_status(ZERO, ZERO, 0)).await;
    assert_eq!(code_of(reply), RefusalCode::RunnerPrivileges);
    // A status the runner cannot read is not a pass.
    let reply = health_reply_with(&env, &[SKILL_ID], "Name:\trunner\n").await;
    assert_eq!(code_of(reply), RefusalCode::RunnerPrivileges);
}

#[tokio::test]
async fn failed_start_check_refuses_every_request_including_health_and_exits_zero() {
    let env = Env::new();
    let marker = env.marker("start_check_ran");
    write_skill(
        env.skills.path(),
        "never_runs",
        &SkillSpec::new(&format!("echo ran > {}", marker.display())),
    )
    .await;
    let requests = [
        health(),
        probe(Some(1)),
        RunnerRequest::Run(run_request(env.skills.path(), "never_runs", &[])),
    ];
    for request in requests {
        let ctx = env.ctx_with(
            inspect_with(&[SKILL_ID, 4242], &proc_status(ZERO, ZERO, 1)),
            StubProbe::new(),
        );
        let mut conn = connect(ctx, Some(AGENT_UID));
        send(&mut conn.client, &request).await;
        let reply = read_reply(&mut conn.client).await.unwrap();
        assert_eq!(code_of(reply), RefusalCode::RunnerGroupsExtra);
        // `finish` asserts the serve future returned `Ok`: the process would exit 0.
        finish(conn.server).await;
    }
    assert!(
        !marker.exists(),
        "a skill ran despite the failed start check"
    );
}

#[tokio::test]
async fn health_returns_the_roots_it_was_started_with() {
    let env = Env::new();
    let RunnerReply::Health(reply) = roundtrip(&env, health()).await else {
        panic!("not a health reply");
    };
    assert_eq!(reply.protocol, RUNNER_PROTOCOL);
    assert_eq!(reply.skills_root, env.skills.path().to_string_lossy());
    assert_eq!(reply.packages_root, env.packages.path().to_string_lossy());
    assert!(!reply.version.is_empty());
}

#[tokio::test]
async fn probe_returns_the_collector_report_unchanged() {
    let env = Env::new();
    let stub = StubProbe::new();
    let ctx = env.ctx_with(good_inspect(), stub.clone());
    let mut conn = connect(ctx, Some(0));
    send(&mut conn.client, &probe(Some(4242))).await;
    let reply = read_reply(&mut conn.client).await.unwrap();
    finish(conn.server).await;
    let RunnerReply::Probe(report) = reply else {
        panic!("not a probe reply: {reply:?}");
    };
    assert_eq!(*report, sample_report());
    assert_eq!(*stub.asked.lock().unwrap(), vec![Some(4242)]);
}
