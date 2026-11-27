//! The read path's deadline contract at the production adapter: the `NotConnected` fallback's
//! store lookups are bounded by the request deadline, and no request stays registered however
//! the call ends. The fake clock and pending futures decide every ordering; nothing sleeps.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::{
    AgentLink, Clock, EnrollmentToken, HostRepo, LinkError, StoreError, ToolOutput,
};
use domovoi_core::{
    AgentId, AgentIdentity, AgentReport, AgentSessionId, AuthenticatedReport, ExecutionId, Host,
    HostId, HostName, PlanHash, ReadToolCall, ReportOutcome, Risk, SkillName,
};
use domovoi_proto::messages::{HostFacts, SessionId, ToolOutcome, ToolResult, ToolSuccess};
use domovoi_proto::{Message, decode};
use domovoi_testkit::build::manifest;
use domovoi_testkit::{FakeClock, InMemoryStore};
use jiff::{SignedDuration, Timestamp};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::super::registry::{Live, Registry};
use super::WsAgentLink;

const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);

/// A host repository whose `live_agent` never answers, standing in for a stalled store.
struct StalledHosts {
    inner: InMemoryStore,
}

#[async_trait]
impl HostRepo for StalledHosts {
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
    async fn live_agent(&self, _host: HostId) -> Result<Option<AgentIdentity>, StoreError> {
        std::future::pending().await
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

struct World {
    link: Arc<WsAgentLink>,
    registry: Arc<Registry>,
    clock: Arc<FakeClock>,
    host: HostId,
}

fn world(hosts: Arc<dyn HostRepo>) -> World {
    let registry = Arc::new(Registry::default());
    let clock = Arc::new(FakeClock::new(T0));
    let dynamic: Arc<dyn Clock> = clock.clone();
    let link = Arc::new(WsAgentLink::new(registry.clone(), dynamic, hosts));
    World {
        link,
        registry,
        clock,
        host: HostId::new(),
    }
}

fn live_session(outbox: mpsc::Sender<String>) -> (Live, SessionId) {
    let session = SessionId::new();
    let live = Live {
        session,
        identity: AgentSessionId::new(),
        agent: AgentId::new(),
        since: T0,
        version: "0.1.0".to_owned(),
        facts: HostFacts {
            hostname: "nas".to_owned(),
            os: "Debian GNU/Linux 13".to_owned(),
            kernel: "6.12.0".to_owned(),
            arch: "x86_64".to_owned(),
            cpu_count: 4,
            mem_total_bytes: 8 << 30,
            boot_time: T0,
            boot_id: "boot-1".to_owned(),
        },
        skills: Vec::new(),
        package_skills: Vec::new(),
        catalog_hash: None,
        runs_as_root: false,
        in_container: false,
        skill_runner_status: domovoi_proto::messages::SkillRunnerStatus::Absent,
        helper_status: domovoi_proto::messages::HelperStatus::Absent,
        helper_identity: None,
        ceiling_summary: None,
        ceiling_hash: None,
        checks: Vec::new(),
        docker: None,
        journal_epoch: Uuid::nil(),
        authoritative_absence: true,
        agent_status: None,
        override_refusals: Vec::new(),
        outbox,
        pending: HashMap::new(),
        cancel: CancellationToken::new(),
    };
    (live, session)
}

fn call() -> ReadToolCall {
    ReadToolCall::DockerPs { all: false }
}

fn later(by: i64) -> Timestamp {
    T0 + SignedDuration::from_secs(by)
}

/// Lets every spawned task run until it waits on something.
async fn settle() {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_stalled_not_connected_lookup_times_out_at_the_deadline() {
    let hosts = Arc::new(StalledHosts {
        inner: InMemoryStore::new(),
    });
    let w = world(hosts);
    let (link, host) = (w.link.clone(), w.host);
    let task = tokio::spawn(async move { link.run_tool(host, call(), later(5)).await });
    settle().await;
    assert!(!task.is_finished(), "the lookup is still pending");
    w.clock.advance(SignedDuration::from_secs(10));
    let result = task.await.unwrap();
    assert_eq!(result.unwrap_err(), LinkError::Timeout);
    assert_eq!(w.registry.pending_len(w.host), 0);
}

#[tokio::test]
async fn a_stalled_lookup_in_start_skill_is_bounded_by_the_deadline_too() {
    use domovoi_core::ports::SkillDispatch;
    let hosts = Arc::new(StalledHosts {
        inner: InMemoryStore::new(),
    });
    let w = world(hosts);
    let (link, host) = (w.link.clone(), w.host);
    let task = tokio::spawn(async move {
        let skill = SkillName::parse("prune_logs").unwrap();
        let built = manifest(skill.as_str(), Risk::Write).unwrap();
        let args = built.validate_args(&BTreeMap::new()).unwrap();
        let req = SkillDispatch {
            execution: ExecutionId::new(),
            plan_hash: PlanHash::agent_check(&skill, built.manifest_hash(), host, &args),
            skill,
            args,
        };
        link.start_skill(
            host,
            req,
            AgentSessionId::new(),
            domovoi_core::JournalEpoch::from_uuid(Uuid::nil()),
            later(5),
        )
        .await
    });
    settle().await;
    w.clock.advance(SignedDuration::from_secs(10));
    assert_eq!(task.await.unwrap().unwrap_err(), LinkError::Timeout);
}

#[tokio::test]
async fn a_host_with_no_session_still_reports_not_connected_before_the_deadline() {
    let w = world(Arc::new(InMemoryStore::new()));
    let result = w.link.run_tool(w.host, call(), later(5)).await;
    assert_eq!(result.unwrap_err(), LinkError::NotConnected);
}

#[tokio::test]
async fn a_deadline_already_passed_answers_timeout_before_anything_is_queued() {
    let w = world(Arc::new(InMemoryStore::new()));
    w.clock.advance(SignedDuration::from_secs(10));
    let result = w.link.run_tool(w.host, call(), later(5)).await;
    assert_eq!(result.unwrap_err(), LinkError::Timeout);
}

#[tokio::test]
async fn a_read_is_answered_with_the_agents_output_unchanged_and_released() {
    let w = world(Arc::new(InMemoryStore::new()));
    let (tx, mut outbox) = mpsc::channel(4);
    let (live, session) = live_session(tx);
    w.registry.register(w.host, live, None).unwrap();

    let (link, host) = (w.link.clone(), w.host);
    let task = tokio::spawn(async move { link.run_tool(host, call(), later(5)).await });
    let frame = outbox.recv().await.expect("the request was queued");
    let sent = decode(&frame).unwrap();
    assert_eq!(w.registry.pending_len(w.host), 1);
    let reply = Message::ToolResult(ToolResult {
        outcome: ToolOutcome::Ok(ToolSuccess {
            text: "CONTAINER ID  IMAGE\n".to_owned(),
            truncated: true,
        }),
    });
    assert!(w.registry.deliver(w.host, session, sent.id, reply));
    let out = task.await.unwrap().unwrap();
    assert_eq!(
        out,
        ToolOutput {
            text: "CONTAINER ID  IMAGE\n".to_owned(),
            truncated: true
        }
    );
    assert_eq!(w.registry.pending_len(w.host), 0);
}

#[tokio::test]
async fn an_unanswered_read_times_out_and_leaves_nothing_registered() {
    let w = world(Arc::new(InMemoryStore::new()));
    let (tx, mut outbox) = mpsc::channel(4);
    let (live, _session) = live_session(tx);
    w.registry.register(w.host, live, None).unwrap();

    let (link, host) = (w.link.clone(), w.host);
    let task = tokio::spawn(async move { link.run_tool(host, call(), later(5)).await });
    outbox.recv().await.expect("the request was queued");
    assert_eq!(w.registry.pending_len(w.host), 1);
    w.clock.advance(SignedDuration::from_secs(10));
    assert_eq!(task.await.unwrap().unwrap_err(), LinkError::Timeout);
    assert_eq!(w.registry.pending_len(w.host), 0);
}

#[tokio::test]
async fn dropping_a_read_midway_releases_its_registered_request() {
    let w = world(Arc::new(InMemoryStore::new()));
    let (tx, mut outbox) = mpsc::channel(4);
    let (live, _session) = live_session(tx);
    w.registry.register(w.host, live, None).unwrap();

    let (link, host) = (w.link.clone(), w.host);
    let task = tokio::spawn(async move { link.run_tool(host, call(), later(5)).await });
    outbox.recv().await.expect("the request was queued");
    assert_eq!(w.registry.pending_len(w.host), 1);
    task.abort();
    let _ = task.await;
    assert_eq!(
        w.registry.pending_len(w.host),
        0,
        "an outer cancellation must not leave a stale request"
    );
}

#[path = "link_package_tests.rs"]
mod package;

#[tokio::test]
async fn received_key_from_hello_is_stored_and_fingerprinted_by_the_hub() {
    use domovoi_core::KeyFingerprint;
    use domovoi_core::ports::{AgentLink as _, HelperLive};
    use domovoi_proto::PublicKey;
    use domovoi_proto::messages::{AgentStatus, HelperIdentity, HelperStatus, SkillRunnerStatus};

    let w = world(Arc::new(InMemoryStore::new()));
    assert!(
        w.link.helper_report(w.host).is_none(),
        "no session, no report"
    );
    let (mut live, session) = live_session(mpsc::channel(1).0);
    live.helper_identity = Some([7; 32]);
    live.helper_status = HelperStatus::Ok {
        ops: vec!["safety.attest".to_owned()],
    };
    w.registry.register(w.host, live, None).unwrap();
    let report = w.link.helper_report(w.host).unwrap();
    assert_eq!(report.public_key, Some([7; 32]));
    assert_eq!(
        report.live,
        HelperLive::Ok {
            ops: vec!["safety.attest".to_owned()]
        }
    );
    // The hub's fingerprint is its own hash of the relayed bytes; nothing else is read.
    let fingerprint = KeyFingerprint::of_public_key(&report.public_key.unwrap());
    assert_eq!(fingerprint.to_string().len(), 64);
    // A later status report replaces what the session says about its helper.
    w.registry.set_agent_status(
        w.host,
        session,
        AgentStatus {
            helper_status: HelperStatus::Error {
                reason: "restarting".to_owned(),
            },
            helper_identity: Some(HelperIdentity {
                public_key: PublicKey([8; 32]),
            }),
            skill_runner_status: SkillRunnerStatus::Absent,
            runs_as_root: false,
            in_container: false,
            ceiling_hash: None,
            ceiling_summary: None,
        },
    );
    let report = w.link.helper_report(w.host).unwrap();
    assert_eq!(report.public_key, Some([8; 32]));
    assert_eq!(
        report.live,
        HelperLive::Error {
            reason: "restarting".to_owned()
        }
    );
}

#[tokio::test]
async fn session_carries_in_container_from_hello_and_agent_status() {
    use domovoi_proto::messages::{AgentStatus, HelperStatus, SkillRunnerStatus};

    let w = world(Arc::new(InMemoryStore::new()));
    let (mut live, session) = live_session(mpsc::channel(1).0);
    live.in_container = true;
    w.registry.register(w.host, live, None).unwrap();
    assert!(w.registry.host_view(w.host).unwrap().words.in_container);
    w.registry.set_agent_status(
        w.host,
        session,
        AgentStatus {
            helper_status: HelperStatus::Absent,
            helper_identity: None,
            skill_runner_status: SkillRunnerStatus::Absent,
            runs_as_root: false,
            in_container: false,
            ceiling_hash: None,
            ceiling_summary: None,
        },
    );
    assert!(!w.registry.host_view(w.host).unwrap().words.in_container);
}
