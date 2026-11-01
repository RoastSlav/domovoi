//! Live sessions: streaming, requests through the link, replacement, revocation, limits.

use domovoi_core::ports::{AgentLink, Clock, FactRepo, LinkError, Presence};
use domovoi_core::{
    CheckKey, DigestFact, FactValue, HostId, ReadToolCall, Severity, Signal, SourceKind,
};
use domovoi_hub::agent_server::{LOCAL_SESSION_HINT, MAX_FRAMES_PER_SECOND, MemoryTransport};
use domovoi_proto::messages::{
    Challenge, ChallengeResponse, DigestFacts, Ping, RejectReason, Signals, ToolOutcome,
    ToolResult, ToolSuccess,
};
use domovoi_proto::{
    MAX_FRAME_BYTES, Message, PROTOCOL_VERSION, Signature, Transport, challenge_message, decode,
    encode,
};
use ed25519_dalek::Signer;
use jiff::SignedDuration;

use crate::agent::{FakeAgent, enroll, hello, online, rejected};
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
        matches!(agent.expect().await.message, Message::Unsupported(u) if u.kind == "telepathy")
    );
}

#[tokio::test]
async fn run_tool_round_trips_and_times_out() {
    let hub = Hub::start().await;
    let link = hub.server.link();
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
async fn new_session_replaces_old_session_for_same_agent() {
    let hub = Hub::start().await;
    let (ident, mut first) = online(&hub, "nas").await;
    let mut second = FakeAgent::connect(hub.addr).await;
    second.authenticate(&ident).await;
    first.closed().await;
    assert!(matches!(
        hub.server.link().presence(ident.host),
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
    let link = hub.server.link();
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
        hub.server.link().presence(ident.host),
        Presence::Connected { .. }
    ));
}
