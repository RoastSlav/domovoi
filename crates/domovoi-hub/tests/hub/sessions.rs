//! Live sessions: streaming, requests through the link, replacement, revocation, limits.

use domovoi_core::ports::{AgentLink, Clock, LinkError, LogLine, MetricSampleRepo, Presence};
use domovoi_core::{
    CheckKey, DigestFact, FactValue, HostId, ReadToolCall, Severity, Signal, SourceKind,
};
use domovoi_hub::agent_server::{LOCAL_SESSION_HINT, MAX_FRAMES_PER_SECOND, MemoryTransport};
use domovoi_proto::messages::{
    AgentStatus, Challenge, ChallengeResponse, DigestFacts, Enrolled, ErrorMessage, HelperStatus,
    Logs, Ping, RejectReason, Signals, SkillRunnerStatus, ToolOutcome, ToolResult, ToolSuccess,
    Unsupported,
};
use domovoi_proto::{
    MAX_FRAME_BYTES, Message, PROTOCOL_VERSION, Signature, Transport, challenge_message, decode,
    encode,
};
use ed25519_dalek::Signer;
use jiff::SignedDuration;

use crate::agent::{FakeAgent, Ident, enroll, hello, online, rejected};
use crate::harness::{Hub, guard, pump, t0};

fn signal(host: HostId, check: &str) -> Signal {
    Signal::firing(
        SourceKind::Agent,
        host,
        CheckKey::parse(check).unwrap(),
        None,
        Severity::Warning,
        "disk almost full",
        t0(),
    )
}

#[tokio::test]
async fn signals_are_submitted_and_acknowledged_and_facts_recorded() {
    let hub = Hub::start().await;
    let (ident, mut agent) = online(&hub, "nas").await;
    let foreign = signal(HostId::new(), "disk.free");
    let batch = Message::Signals(Signals {
        batch_seq: 7,
        signals: vec![signal(ident.host, "disk.free"), foreign],
    });
    let id = agent.send(batch, None).await;
    let ack = agent.expect().await;
    assert_eq!(ack.reply_to, Some(id));
    assert!(matches!(ack.message, Message::SignalsAck(a) if a.up_to_seq == 7));
    let got = hub.signals();
    assert_eq!(got.len(), 1, "signals about another host are dropped");
    assert_eq!(got[0].host(), ident.host);

    let fact = DigestFact {
        host: ident.host,
        key: "disk.root.used_pct".to_owned(),
        label: "Root used".to_owned(),
        value: FactValue::Number(41.0),
        unit: Some("%".to_owned()),
        observed_at: t0(),
    };
    agent
        .send(
            Message::DigestFacts(DigestFacts {
                facts: vec![fact.clone()],
            }),
            None,
        )
        .await;
    let ping = agent.send(Message::Ping(Ping { at: t0() }), None).await;
    let pong = agent.expect().await;
    assert_eq!(pong.reply_to, Some(ping));
    let stored = hub
        .store
        .list(
            ident.host,
            t0() - SignedDuration::from_secs(1),
            t0() + SignedDuration::from_secs(1),
        )
        .await
        .unwrap();
    assert_eq!(stored, vec![fact]);

    agent
        .send(
            Message::Unknown {
                kind: "telepathy".to_owned(),
            },
            None,
        )
        .await;
    assert!(
        matches!(agent.expect().await.message, Message::Unsupported(u) if u.type_name == "telepathy")
    );
}

#[tokio::test]
async fn unsupported_message_does_not_end_session() {
    let hub = Hub::start().await;
    let (_ident, mut agent) = online(&hub, "nas").await;
    agent
        .send(
            Message::Unknown {
                kind: "telepathy".to_owned(),
            },
            None,
        )
        .await;
    assert!(
        matches!(agent.expect().await.message, Message::Unsupported(u) if u.type_name == "telepathy")
    );
    // The same session is still usable afterwards.
    agent.send(Message::Ping(Ping { at: t0() }), None).await;
    assert!(matches!(agent.expect().await.message, Message::Pong(_)));
}

#[tokio::test]
async fn run_tool_round_trips_and_times_out() {
    let hub = Hub::start().await;
    let link = hub.server.link_for_tests();
    let (ident, mut agent) = online(&hub, "nas").await;
    assert!(matches!(
        link.presence(ident.host),
        Presence::Connected { .. }
    ));

    let deadline = t0() + SignedDuration::from_secs(30);
    let call = tokio::spawn({
        let link = link.clone();
        async move {
            link.run_tool(ident.host, ReadToolCall::DockerPs { all: true }, deadline)
                .await
        }
    });
    let request = agent.expect().await;
    let Message::RunTool(run) = request.message else {
        panic!("expected run_tool");
    };
    assert_eq!(run.call, ReadToolCall::DockerPs { all: true });
    assert_eq!(run.deadline, deadline);
    let result = Message::ToolResult(ToolResult {
        outcome: ToolOutcome::Ok(ToolSuccess {
            text: "plex  Up 3 hours".to_owned(),
            truncated: false,
        }),
    });
    agent.send(result, Some(request.id)).await;
    let out = guard(call).await.unwrap().unwrap();
    assert_eq!(out.text, "plex  Up 3 hours");

    // No answer: the deadline passes on the clock.
    let deadline = hub.clock.now() + SignedDuration::from_secs(5);
    let waiting = link.run_tool(ident.host, ReadToolCall::DockerPs { all: false }, deadline);
    let err = pump(&hub.clock, SignedDuration::from_millis(500), waiting).await;
    assert_eq!(err, Err(LinkError::Timeout));
    assert!(matches!(agent.expect().await.message, Message::RunTool(_)));

    // A host without a session fails fast.
    let deadline = hub.clock.now() + SignedDuration::from_secs(5);
    let other = link
        .run_tool(
            HostId::new(),
            ReadToolCall::DockerPs { all: false },
            deadline,
        )
        .await;
    assert_eq!(other, Err(LinkError::NotConnected));
}

#[tokio::test]
async fn api_host_state_is_read_without_the_link() {
    use domovoi_hub::api::AgentAdmin;

    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let skill = domovoi_testkit::build::manifest("disk_report", domovoi_core::Risk::Read)
        .unwrap()
        .summary(domovoi_core::ScriptDigest::of_bytes(b"fixture entry"));
    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Hello(mut greeting) = hello(ident.agent, PROTOCOL_VERSION) else {
        panic!("expected a hello");
    };
    greeting.skills = vec![skill];
    agent.send(Message::Hello(greeting), None).await;
    let Message::Challenge(challenge) = agent.expect().await.message else {
        panic!("expected a challenge");
    };
    assert!(matches!(
        agent.answer(&challenge, ident.agent, &ident.key).await,
        Message::Welcome(_)
    ));

    // Presence and skills are what the agent reported on its handshake.
    assert!(
        matches!(
            AgentAdmin::presence(&hub.server, ident.host),
            Presence::Connected { .. }
        ),
        "the API reads presence through AgentAdmin"
    );
    let announced: Vec<String> = AgentAdmin::skills(&hub.server, ident.host)
        .iter()
        .map(|skill| skill.name.to_string())
        .collect();
    assert_eq!(
        announced,
        ["disk_report"],
        "the agent's catalogue, unmodified"
    );

    // Logs are answered once, through the same seam.
    let asked = tokio::spawn({
        let server = hub.server.clone();
        async move { server.logs(ident.host, 1, "warn".to_owned()).await }
    });
    let request = agent.expect().await;
    let Message::GetLogs(get) = request.message else {
        panic!("expected get_logs");
    };
    assert_eq!(get.min_level, "warn");
    let mine = [LogLine {
        at: t0(),
        level: "warn".to_owned(),
        target: "agent.checks".to_owned(),
        message: "/srv is 97% full".to_owned(),
    }];
    agent
        .send(
            Message::Logs(Logs {
                lines: mine.to_vec(),
                has_more: false,
            }),
            Some(request.id),
        )
        .await;
    assert_eq!(guard(asked).await.unwrap().unwrap(), mine.to_vec());
}

#[tokio::test]
async fn logs_are_requested_and_answered_or_rejected() {
    let hub = Hub::start().await;
    let link = hub.server.link_for_tests();
    let (ident, mut agent) = online(&hub, "nas").await;

    let asked = tokio::spawn({
        let link = link.clone();
        async move { link.logs(ident.host, 2, "warn".to_owned()).await }
    });
    let request = agent.expect().await;
    let Message::GetLogs(get) = request.message else {
        panic!("expected get_logs");
    };
    assert_eq!(get.lines, 2);
    assert_eq!(get.min_level, "warn");
    assert_eq!(get.offset, 0);
    let mine = [LogLine {
        at: t0(),
        level: "warn".to_owned(),
        target: "agent.checks".to_owned(),
        message: "/srv is 97% full".to_owned(),
    }];
    agent
        .send(
            Message::Logs(Logs {
                lines: mine.to_vec(),
                has_more: false,
            }),
            Some(request.id),
        )
        .await;
    let got = guard(asked).await.unwrap().unwrap();
    assert_eq!(
        got,
        mine.to_vec(),
        "the agent's lines come back through the link"
    );

    // An agent from a release before get_logs replies unsupported.
    let stale = tokio::spawn({
        let link = link.clone();
        async move { link.logs(ident.host, 5, "info".to_owned()).await }
    });
    let request = agent.expect().await;
    let Message::GetLogs(get) = request.message else {
        panic!("expected get_logs");
    };
    assert_eq!(get.min_level, "info");
    agent
        .send(
            Message::Unsupported(Unsupported {
                type_name: "get_logs".to_owned(),
            }),
            Some(request.id),
        )
        .await;
    let err = guard(stale).await.unwrap().unwrap_err();
    assert_eq!(
        err,
        LinkError::Rejected("the agent is too old to send logs; update it".to_owned()),
        "an agent too old to send logs says so, so the person knows to update it",
    );
}

#[tokio::test]
async fn a_log_window_too_big_for_one_frame_is_asked_for_in_chunks() {
    let hub = Hub::start().await;
    let link = hub.server.link_for_tests();
    let (ident, mut agent) = online(&hub, "nas").await;

    let asked = tokio::spawn({
        let link = link.clone();
        async move { link.logs(ident.host, 3, "info".to_owned()).await }
    });

    let all = [
        LogLine {
            at: t0(),
            level: "info".to_owned(),
            target: "agent.link".to_owned(),
            message: "first".to_owned(),
        },
        LogLine {
            at: t0(),
            level: "info".to_owned(),
            target: "agent.link".to_owned(),
            message: "second".to_owned(),
        },
        LogLine {
            at: t0(),
            level: "info".to_owned(),
            target: "agent.link".to_owned(),
            message: "third".to_owned(),
        },
    ];
    let request = agent.expect().await;
    let Message::GetLogs(get) = request.message else {
        panic!("expected get_logs");
    };
    assert_eq!(
        get.offset, 0,
        "the hub starts at the oldest end of the window"
    );
    agent
        .send(
            Message::Logs(Logs {
                lines: all[..2].to_vec(),
                has_more: true,
            }),
            Some(request.id),
        )
        .await;

    let again = agent.expect().await;
    let Message::GetLogs(get) = again.message else {
        panic!("expected the hub to ask again for the rest of the window");
    };
    assert_eq!(get.offset, 2, "the hub asks after the lines it already got");
    agent
        .send(
            Message::Logs(Logs {
                lines: all[2..].to_vec(),
                has_more: false,
            }),
            Some(again.id),
        )
        .await;

    let got = guard(asked).await.unwrap().unwrap();
    assert_eq!(
        got,
        all.to_vec(),
        "the chunks come back joined, oldest first",
    );
}

#[tokio::test]
async fn a_busy_agent_answers_the_log_request_right_away() {
    let hub = Hub::start().await;
    let link = hub.server.link_for_tests();
    let (ident, mut agent) = online(&hub, "nas").await;

    let asked = tokio::spawn({
        let link = link.clone();
        async move { link.logs(ident.host, 5, "info".to_owned()).await }
    });
    let request = agent.expect().await;
    let Message::GetLogs(get) = request.message else {
        panic!("expected get_logs");
    };
    assert_eq!(get.lines, 5);
    agent
        .send(
            Message::Error(ErrorMessage {
                code: "busy".to_owned(),
                message: "the agent is busy; try again shortly".to_owned(),
            }),
            Some(request.id),
        )
        .await;
    let err = guard(asked).await.unwrap().unwrap_err();
    assert_eq!(
        err,
        LinkError::Rejected("the agent is busy; try again shortly".to_owned()),
        "the busy answer is delivered, so the hub does not wait out the deadline",
    );
}

#[tokio::test]
async fn new_session_replaces_old_session_for_same_agent() {
    let hub = Hub::start().await;
    let (ident, mut first) = online(&hub, "nas").await;
    let mut second = FakeAgent::connect(hub.addr).await;
    second.authenticate(&ident).await;
    first.closed().await;
    assert!(matches!(
        hub.server.link_for_tests().presence(ident.host),
        Presence::Connected { .. }
    ));
    let ping = second.send(Message::Ping(Ping { at: t0() }), None).await;
    assert_eq!(
        second.expect().await.reply_to,
        Some(ping),
        "the new session is live"
    );
}

#[tokio::test]
async fn revocation_closes_live_session() {
    let hub = Hub::start().await;
    let (ident, mut agent) = online(&hub, "nas").await;
    assert!(hub.server.revoke_agent(ident.host).await.unwrap());
    agent.closed().await;
    let link = hub.server.link_for_tests();
    assert!(matches!(
        link.presence(ident.host),
        Presence::Disconnected { .. }
    ));
    let mut again = FakeAgent::connect(hub.addr).await;
    assert_eq!(
        rejected(&again.hello(ident.agent, PROTOCOL_VERSION).await),
        Some(RejectReason::Revoked)
    );
}

#[tokio::test]
async fn oversized_frame_and_flooding_close_connection() {
    let hub = Hub::start().await;
    let (ident, mut agent) = online(&hub, "nas").await;
    let padding = "x".repeat(MAX_FRAME_BYTES + 1);
    agent.try_send_raw(padding).await;
    agent.closed().await;

    let mut flood = FakeAgent::connect(hub.addr).await;
    flood.authenticate(&ident).await;
    let ping = encode(
        &Message::Ping(Ping { at: t0() }),
        domovoi_core::RequestId::new(),
        None,
    )
    .unwrap();
    let mut sent = 0;
    while sent <= MAX_FRAMES_PER_SECOND && flood.try_send_raw(ping.clone()).await {
        sent += 1;
    }
    flood.closed().await;
    assert!(sent <= MAX_FRAMES_PER_SECOND + 1, "closed by frame {sent}");
}

#[tokio::test]
async fn local_attach_runs_the_same_handshake() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let (mut ours, theirs) = MemoryTransport::pair();
    let server = hub.server.clone();
    tokio::spawn(async move { server.attach_local(Box::new(theirs)).await });

    let frame = encode(
        &hello(ident.agent, PROTOCOL_VERSION),
        domovoi_core::RequestId::new(),
        None,
    )
    .unwrap();
    ours.send(frame).await.unwrap();
    let challenge = decode(&guard(ours.recv()).await.unwrap().unwrap()).unwrap();
    let Message::Challenge(Challenge { nonce }) = challenge.message else {
        panic!("expected a challenge");
    };
    let signed = challenge_message(&nonce, ident.agent, LOCAL_SESSION_HINT);
    let response = Message::ChallengeResponse(ChallengeResponse {
        signature: Signature(ident.key.sign(&signed).to_bytes()),
    });
    ours.send(encode(&response, domovoi_core::RequestId::new(), None).unwrap())
        .await
        .unwrap();
    let welcome = decode(&guard(ours.recv()).await.unwrap().unwrap()).unwrap();
    assert!(matches!(welcome.message, Message::Welcome(w) if w.config.heartbeat_seconds == 10));
    assert!(matches!(
        hub.server.link_for_tests().presence(ident.host),
        Presence::Connected { .. }
    ));
}

#[tokio::test]
async fn agent_status_is_recorded_per_session() {
    let hub = Hub::start().await;
    let (ident, mut agent) = online(&hub, "nas").await;
    let status = AgentStatus {
        helper_status: HelperStatus::Absent,
        helper_identity: None,
        skill_runner_status: SkillRunnerStatus::Absent,
        runs_as_root: true,
        in_container: false,
        ceiling_hash: None,
        ceiling_summary: None,
    };
    agent.send(Message::AgentStatus(status.clone()), None).await;
    // A ping answers after the status in the same loop, proving it was processed.
    agent.send(Message::Ping(Ping { at: t0() }), None).await;
    assert!(matches!(agent.expect().await.message, Message::Pong(_)));
    assert_eq!(hub.server.agent_status(ident.host), Some(status));

    // A replacement session starts with no status inherited from the old one.
    let mut second = FakeAgent::connect(hub.addr).await;
    second.authenticate(&ident).await;
    assert_eq!(hub.server.agent_status(ident.host), None);
}

#[tokio::test]
async fn a_handshake_that_finishes_after_reenrollment_is_refused() {
    let hub = Hub::start().await;
    let old = enroll(&hub, "nas").await;
    let mut stale = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = stale.hello(old.agent, PROTOCOL_VERSION).await else {
        panic!("expected a challenge");
    };
    // The host is re-enrolled while the old agent is still answering the challenge.
    let new = enroll(&hub, "nas").await;
    assert_ne!(new.agent, old.agent, "re-enrollment issues a new identity");

    // The old signature is genuine and its key was never changed, but the identity is gone.
    let reply = stale.answer(&challenge, old.agent, &old.key).await;
    assert_eq!(rejected(&reply), Some(RejectReason::Revoked));
    stale.closed().await;
    assert!(
        hub.server.link_for_tests().snapshot(old.host).is_none(),
        "the stale handshake never became the live session"
    );
    assert!(matches!(
        hub.server.link_for_tests().presence(old.host),
        Presence::Disconnected { .. }
    ));
}

#[tokio::test]
async fn replacing_a_live_enrollment_ends_its_session_and_pending_work() {
    let hub = Hub::start().await;
    let (old, mut stale) = online(&hub, "nas").await;
    let link = hub.server.link_for_tests();
    let deadline = t0() + SignedDuration::from_secs(30);
    let asked = tokio::spawn({
        let link = link.clone();
        async move {
            link.run_tool(old.host, ReadToolCall::DockerPs { all: true }, deadline)
                .await
        }
    });
    assert!(
        matches!(stale.expect().await.message, Message::RunTool(_)),
        "the old session has the request in flight"
    );

    // A second token re-enrolls the host: the old identity is retired and its session ends.
    let (new, _) = online(&hub, "nas").await;
    assert_ne!(new.agent, old.agent);
    stale.closed().await;
    assert!(
        guard(asked).await.unwrap().is_err(),
        "pending work on the retired session does not survive"
    );
    assert!(
        link.snapshot(old.host).is_some(),
        "the replacement enrollment is the live one"
    );
}

#[tokio::test]
async fn revocation_refuses_a_handshake_already_answering() {
    let hub = Hub::start().await;
    let ident = enroll(&hub, "nas").await;
    let mut agent = FakeAgent::connect(hub.addr).await;
    let Message::Challenge(challenge) = agent.hello(ident.agent, PROTOCOL_VERSION).await else {
        panic!("expected a challenge");
    };
    assert!(hub.server.revoke_agent(ident.host).await.unwrap());

    let reply = agent.answer(&challenge, ident.agent, &ident.key).await;
    assert_eq!(rejected(&reply), Some(RejectReason::Revoked));
    agent.closed().await;
    assert!(hub.server.link_for_tests().snapshot(ident.host).is_none());
}

#[tokio::test]
async fn reenrollment_with_the_same_key_still_retires_the_old_agent() {
    let hub = Hub::start().await;
    let (old, mut first) = online(&hub, "nas").await;
    let (_, token) = hub.token("nas").await;
    let mut enroll_again = FakeAgent::connect(hub.addr).await;
    let Message::Enrolled(Enrolled { agent_id, .. }) = enroll_again.enroll(&token, &old.key).await
    else {
        panic!("enrollment failed");
    };
    assert_ne!(
        agent_id, old.agent,
        "the same key still gets a new identity"
    );
    first.closed().await;

    let mut fresh = FakeAgent::connect(hub.addr).await;
    let ident = Ident {
        key: old.key.clone(),
        agent: agent_id,
        host: old.host,
    };
    fresh.authenticate(&ident).await;
    assert!(
        hub.server.link_for_tests().snapshot(old.host).is_some(),
        "the new identity is live"
    );

    // The old identity cannot come back, even though the key still exists.
    let mut stale = FakeAgent::connect(hub.addr).await;
    assert_eq!(
        rejected(&stale.hello(old.agent, PROTOCOL_VERSION).await),
        Some(RejectReason::Revoked)
    );
}
