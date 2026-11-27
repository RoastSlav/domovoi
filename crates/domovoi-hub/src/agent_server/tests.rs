//! Agent-server lifecycle tests that need the test-only registration gate.
//!
//! They drive the same handshake and session loop as a network agent, over an in-memory
//! transport, and reach the registry's test hook between authentication and registration.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use domovoi_core::ports::{AgentLink, Clock, EnrollmentToken, HostRepo, StoreError};
use domovoi_core::{
    AgentId, AgentIdentity, AgentReport, AuthenticatedReport, Host, HostId, HostKind, HostName,
    ReportOutcome, RequestId,
};
use domovoi_proto::messages::{
    Challenge, ChallengeResponse, Hello, HelperStatus, HostFacts, Message, SkillRunnerStatus,
};
use domovoi_proto::{
    Decoded, PROTOCOL_VERSION, Signature, Transport, challenge_message, decode, encode,
};
use domovoi_testkit::{FakeClock, InMemoryStore, RecordingSignalSink};
use ed25519_dalek::{Signer, SigningKey};
use jiff::Timestamp;

use super::{
    AgentServer, AgentServerDeps, LOCAL_SESSION_HINT, MemoryTransport, NetProbes, ServerSettings,
};

pub(super) const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);

pub(super) struct NoProbes;

#[async_trait]
impl NetProbes for NoProbes {
    async fn gateway_reachable(&self) -> bool {
        true
    }

    async fn tcp_reachable(&self, _target: &str) -> bool {
        true
    }
}

pub(super) fn settings(dir: &std::path::Path) -> ServerSettings {
    ServerSettings {
        hub_name: "testhub".to_owned(),
        data_dir: dir.to_path_buf(),
        bind: "127.0.0.1:0".parse().unwrap(),
        public_url: url::Url::parse("wss://testhub:7421").unwrap(),
        self_check: "1.1.1.1:53".to_owned(),
        heartbeat: jiff::SignedDuration::from_secs(10),
        probes: BTreeMap::new(),
    }
}

fn host() -> Host {
    Host {
        id: HostId::new(),
        name: HostName::parse("nas").unwrap(),
        kind: HostKind::Remote,
        labels: BTreeMap::new(),
        created_at: T0,
    }
}

fn enrolled(host: HostId, key: &SigningKey) -> AgentIdentity {
    AgentIdentity {
        agent: AgentId::new(),
        host,
        public_key: key.verifying_key().to_bytes(),
        version: "0.1.0".to_owned(),
        enrolled_at: T0,
        revoked_at: None,
    }
}

async fn server_over(
    hosts: Arc<dyn HostRepo>,
    facts: Arc<InMemoryStore>,
    host: HostId,
) -> AgentServer {
    let dir = tempfile::tempdir().unwrap();
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(T0));
    let audit: Arc<dyn domovoi_core::ports::AuditLog> = facts.clone();
    let deps = AgentServerDeps {
        clock,
        hosts,
        facts,
        sink: Arc::new(RecordingSignalSink::new()),
        audit,
        probes: Arc::new(NoProbes),
        hub_host: host,
    };
    AgentServer::new(settings(dir.path()), deps).unwrap()
}

/// A server with one enrolled, non-revoked agent, and that agent's signing key.
async fn world() -> (AgentServer, HostId, AgentId, SigningKey) {
    let store = Arc::new(InMemoryStore::new());
    let host = host();
    HostRepo::upsert_host(&*store, &host).await.unwrap();
    let key = SigningKey::from_bytes(&rand::random());
    let agent = enrolled(host.id, &key);
    HostRepo::put_agent(&*store, &agent).await.unwrap();
    let server = server_over(store.clone(), store.clone(), host.id).await;
    (server, host.id, agent.agent, key)
}

fn hello(agent: AgentId) -> Message {
    Message::Hello(Hello {
        agent_build: None,
        agent_id: agent,
        protocol: PROTOCOL_VERSION,
        agent_version: "0.1.0".to_owned(),
        facts: Box::new(HostFacts {
            hostname: "nas".to_owned(),
            os: "Debian GNU/Linux 13".to_owned(),
            kernel: "6.12.0".to_owned(),
            arch: "x86_64".to_owned(),
            cpu_count: 4,
            mem_total_bytes: 8 << 30,
            boot_time: T0,
            boot_id: "boot-1".to_owned(),
        }),
        capabilities: Vec::new(),
        features: Vec::new(),
        journal_epoch: Default::default(),
        catalog_hash: None,
        ceiling_hash: None,
        ceiling_summary: None,
        skill_runner_status: SkillRunnerStatus::Absent,
        helper_status: HelperStatus::Absent,
        runs_as_root: false,
        in_container: false,
        helper_identity: None,
        skills: Vec::new(),
        checks: Vec::new(),
        docker: None,
        package_skills: Vec::new(),
    })
}

async fn next(transport: &mut MemoryTransport) -> Decoded {
    let frame = transport
        .recv()
        .await
        .unwrap()
        .expect("the hub sent a frame");
    decode(&frame).unwrap()
}

/// Runs the agent half of the local handshake and returns the hub's last message.
async fn authenticate(
    transport: &mut MemoryTransport,
    agent: AgentId,
    key: &SigningKey,
) -> Message {
    let frame = encode(&hello(agent), RequestId::new(), None).unwrap();
    transport.send(frame).await.unwrap();
    let message = next(transport).await;
    let Message::Challenge(Challenge { nonce }) = message.message else {
        panic!("expected a challenge, got {:?}", message.message);
    };
    let signed = challenge_message(&nonce, agent, LOCAL_SESSION_HINT);
    let response = Message::ChallengeResponse(ChallengeResponse {
        signature: Signature(key.sign(&signed).to_bytes()),
    });
    transport
        .send(encode(&response, RequestId::new(), None).unwrap())
        .await
        .unwrap();
    next(transport).await.message
}

#[tokio::test]
async fn a_registration_after_retirement_is_refused() {
    let (server, host, agent, key) = world().await;
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    server.inner.registry.hold_registrations(gate.clone());

    let (mut client, theirs) = MemoryTransport::pair();
    let serving = server.clone();
    let task = tokio::spawn(async move { serving.attach_local(Box::new(theirs)).await });

    assert!(
        matches!(
            authenticate(&mut client, agent, &key).await,
            Message::Welcome(_)
        ),
        "the handshake finished; registration is held"
    );

    // Retire the identity between the successful handshake and its registration.
    assert!(server.revoke_agent(host).await.unwrap());
    gate.add_permits(1);

    assert!(
        client.recv().await.unwrap().is_none(),
        "the hub closes the connection"
    );
    assert!(
        server.link().snapshot(host).is_none(),
        "a retired agent never becomes the live session"
    );
    let _ = task.await;
}

#[derive(Debug, Clone, Copy)]
enum Mutation {
    PublicKey,
    Host,
}

/// A host repository that lies about the live identity on its second `live_agent` call,
/// which is the one registration makes.
struct MutatingHosts {
    inner: Arc<InMemoryStore>,
    calls: AtomicUsize,
    mutate_on: usize,
    mutation: Mutation,
}

#[async_trait]
impl HostRepo for MutatingHosts {
    async fn upsert_host(&self, host: &Host) -> Result<(), StoreError> {
        self.inner.upsert_host(host).await
    }

    async fn get_host(&self, id: HostId) -> Result<Option<Host>, StoreError> {
        self.inner.get_host(id).await
    }

    async fn get_host_by_name(&self, name: &HostName) -> Result<Option<Host>, StoreError> {
        self.inner.get_host_by_name(name).await
    }

    async fn list_hosts(&self) -> Result<Vec<Host>, StoreError> {
        self.inner.list_hosts().await
    }

    async fn put_agent(&self, agent: &AgentIdentity) -> Result<(), StoreError> {
        self.inner.put_agent(agent).await
    }

    async fn get_agent(&self, id: AgentId) -> Result<Option<AgentIdentity>, StoreError> {
        self.inner.get_agent(id).await
    }

    async fn live_agent(&self, host: HostId) -> Result<Option<AgentIdentity>, StoreError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let mut live = self.inner.live_agent(host).await?;
        if call == self.mutate_on
            && let Some(agent) = live.as_mut()
        {
            match self.mutation {
                Mutation::PublicKey => agent.public_key = [9; 32],
                Mutation::Host => agent.host = HostId::new(),
            }
        }
        Ok(live)
    }

    async fn revoke_agent(&self, id: AgentId, at: Timestamp) -> Result<(), StoreError> {
        self.inner.revoke_agent(id, at).await
    }

    async fn agent_report(&self, agent: AgentId) -> Result<Option<AgentReport>, StoreError> {
        self.inner.agent_report(agent).await
    }

    async fn record_agent_report(
        &self,
        agent: AgentId,
        seen: &AuthenticatedReport,
        now: Timestamp,
    ) -> Result<ReportOutcome, StoreError> {
        self.inner.record_agent_report(agent, seen, now).await
    }

    async fn create_enrollment_token(&self, token: &EnrollmentToken) -> Result<(), StoreError> {
        self.inner.create_enrollment_token(token).await
    }

    async fn consume_enrollment_token(
        &self,
        token_hash: &[u8; 32],
        now: Timestamp,
    ) -> Result<HostId, StoreError> {
        self.inner.consume_enrollment_token(token_hash, now).await
    }
}

async fn mutating_world(mutation: Mutation) -> (AgentServer, HostId, AgentId, SigningKey) {
    let store = Arc::new(InMemoryStore::new());
    let host = host();
    HostRepo::upsert_host(&*store, &host).await.unwrap();
    let key = SigningKey::from_bytes(&rand::random());
    let agent = enrolled(host.id, &key);
    HostRepo::put_agent(&*store, &agent).await.unwrap();
    let hosts = Arc::new(MutatingHosts {
        inner: store.clone(),
        calls: AtomicUsize::new(0),
        mutate_on: 1,
        mutation,
    });
    let server = server_over(hosts, store.clone(), host.id).await;
    (server, host.id, agent.agent, key)
}

#[tokio::test]
async fn an_identity_changed_before_registration_is_refused() {
    for mutation in [Mutation::PublicKey, Mutation::Host] {
        let (server, host, agent, key) = mutating_world(mutation).await;
        let (mut client, theirs) = MemoryTransport::pair();
        let serving = server.clone();
        let task = tokio::spawn(async move { serving.attach_local(Box::new(theirs)).await });

        assert!(
            matches!(
                authenticate(&mut client, agent, &key).await,
                Message::Welcome(_)
            ),
            "authentication succeeds before the identity changes"
        );
        assert!(
            client.recv().await.unwrap().is_none(),
            "{mutation:?}: the changed identity is refused before registration"
        );
        assert!(
            server.link().snapshot(host).is_none(),
            "{mutation:?}: the changed identity never becomes the live session"
        );
        let _ = task.await;
    }
}
