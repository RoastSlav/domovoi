//! One fixed sample per message kind, with fixed ids and times so snapshots are stable.

use std::collections::BTreeMap;

use domovoi_core::{
    ActionId, AgentId, CheckKey, DigestFact, Evidence, EvidenceValue, FactValue, HostId, HostName,
    PlanHash, ReadToolCall, RequestId, Severity, Signal, SignalId, SignalStatus, SkillName,
    SkillSummary, SourceKind, TargetKey,
};
use domovoi_proto::messages::*;
use domovoi_proto::{
    CertFingerprint, EnrollToken, Message, Nonce, PROTOCOL_VERSION, PublicKey, Sha256Bytes,
    Signature,
};
use jiff::Timestamp;
use uuid::Uuid;

pub const T0: Timestamp = Timestamp::constant(1_700_000_000, 0);
pub const DEADLINE: Timestamp = Timestamp::constant(1_700_000_090, 0);

pub fn request_id(n: u128) -> RequestId {
    RequestId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
    ))
}

pub fn agent_id() -> AgentId {
    AgentId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00a1))
}

pub fn host_id() -> HostId {
    HostId::from_uuid(Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00b1))
}

fn facts() -> HostFacts {
    HostFacts {
        hostname: "nas".to_owned(),
        os: "Debian GNU/Linux 13".to_owned(),
        kernel: "6.12.0-1-amd64".to_owned(),
        arch: "x86_64".to_owned(),
        cpu_count: 4,
        mem_total_bytes: 16 * 1024 * 1024 * 1024,
        boot_time: Timestamp::constant(1_699_000_000, 0),
        boot_id: "5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f".to_owned(),
    }
}

fn settings() -> AgentSettings {
    let mut thresholds = BTreeMap::new();
    thresholds.insert("critical_percent".to_owned(), serde_json::json!(95));
    let mut check_overrides = BTreeMap::new();
    check_overrides.insert(
        CheckKey::parse("disk.usage").unwrap(),
        CheckOverride {
            enabled: Some(true),
            interval_seconds: Some(300),
            thresholds,
        },
    );
    AgentSettings {
        heartbeat_seconds: 30,
        check_overrides,
    }
}

fn skill_summary() -> SkillSummary {
    serde_json::from_value(serde_json::json!({
        "name": "restart_container",
        "description": "Restart one container.",
        "risk": "write",
        "params": [{
            "name": "container",
            "description": "Container name.",
            "kind": { "type": "string", "pattern": "^[a-z0-9_.-]+$", "max_len": 64 }
        }],
        "applies_to": ["container.restart_loop"],
        "manifest_hash": "ab".repeat(32),
    }))
    .unwrap()
}

fn evidence() -> Evidence {
    let mut evidence = Evidence::new();
    evidence.insert("used_percent", EvidenceValue::Float(97.5));
    evidence
}

fn signal() -> Signal {
    Signal::firing(
        SourceKind::BuiltIn,
        host_id(),
        CheckKey::parse("disk.usage").unwrap(),
        Some(TargetKey::parse("/srv").unwrap()),
        Severity::Critical,
        "/srv is 97% full",
        T0,
    )
    .with_evidence(evidence())
    .with_id(SignalId::from_uuid(Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_00c1,
    )))
}

fn plan_hash() -> PlanHash {
    "cd".repeat(32).parse().unwrap()
}

/// One message of every kind, in [`Message::KINDS`] order.
pub fn all() -> Vec<Message> {
    vec![
        Enroll {
            token: EnrollToken::new("dmv_enroll_0123456789abcdef"),
            public_key: PublicKey([7; 32]),
            host_name: HostName::parse("nas").unwrap(),
            facts: facts(),
            agent_version: "0.1.0".to_owned(),
        }
        .into(),
        Enrolled {
            agent_id: agent_id(),
            host_id: host_id(),
            hub_cert_sha256: CertFingerprint([9; 32]),
        }
        .into(),
        Hello {
            agent_id: agent_id(),
            protocol: PROTOCOL_VERSION,
            agent_version: "0.1.0".to_owned(),
            facts: facts(),
            capabilities: vec!["docker".to_owned(), "journal".to_owned()],
            skills: vec![skill_summary()],
        }
        .into(),
        Challenge {
            nonce: Nonce([1; 32]),
        }
        .into(),
        ChallengeResponse {
            signature: Signature([2; 64]),
        }
        .into(),
        Welcome {
            session_id: SessionId::from_uuid(Uuid::from_u128(
                0x0190_0000_0000_7000_8000_0000_0000_00d1,
            )),
            hub_version: "0.1.0".to_owned(),
            config: settings(),
        }
        .into(),
        Reject {
            reason: RejectReason::Incompatible {
                hub: PROTOCOL_VERSION,
            },
        }
        .into(),
        Ping { at: T0 }.into(),
        Pong { at: T0 }.into(),
        Signals {
            batch_seq: 42,
            signals: vec![signal()],
        }
        .into(),
        SignalsAck { up_to_seq: 42 }.into(),
        DigestFacts {
            facts: vec![DigestFact {
                host: host_id(),
                key: "zfs.tank.used_pct".to_owned(),
                label: "tank used".to_owned(),
                value: FactValue::Number(61.0),
                unit: Some("%".to_owned()),
                observed_at: T0,
            }],
        }
        .into(),
        RunTool {
            call: ReadToolCall::from_json(
                "docker_logs",
                serde_json::json!({ "container": "postgres", "lines": 50 }),
            )
            .unwrap(),
            deadline: DEADLINE,
        }
        .into(),
        ToolResult {
            outcome: ToolOutcome::Ok(ToolSuccess {
                text: "ready to accept connections".to_owned(),
                truncated: false,
            }),
        }
        .into(),
        RunSkill {
            action_id: ActionId::from_uuid(Uuid::from_u128(
                0x0190_0000_0000_7000_8000_0000_0000_00e1,
            )),
            skill: SkillName::parse("restart_container").unwrap(),
            args: BTreeMap::from([("container".to_owned(), "postgres".to_owned())]),
            plan_hash: plan_hash(),
            deadline: DEADLINE,
        }
        .into(),
        SkillResult {
            outcome: SkillOutcome::Err(SkillFailure {
                code: SkillFailureCode::PlanMismatch,
                message: "manifest changed since approval".to_owned(),
            }),
        }
        .into(),
        RunCheck {
            check: CheckKey::parse("disk.usage").unwrap(),
            target: Some(TargetKey::parse("/srv").unwrap()),
            deadline: DEADLINE,
        }
        .into(),
        CheckResult {
            observations: vec![CheckObservation {
                target: Some(TargetKey::parse("/srv").unwrap()),
                status: SignalStatus::Firing,
                summary: "/srv is 97% full".to_owned(),
                evidence: evidence(),
                observed_at: T0,
            }],
        }
        .into(),
        ConfigUpdate { config: settings() }.into(),
        ErrorMessage {
            code: "invalid_payload".to_owned(),
            message: "signals: missing field `batch_seq`".to_owned(),
        }
        .into(),
        Unsupported {
            kind: "future_kind".to_owned(),
        }
        .into(),
    ]
}

/// A successful skill result, which [`all`] does not otherwise cover.
pub fn skill_success() -> Message {
    SkillResult {
        outcome: SkillOutcome::Ok(SkillSuccess {
            exit_code: Some(0),
            stdout: "postgres\n".to_owned(),
            stderr: String::new(),
            truncated: false,
            duration_ms: 1_250,
            argv_hash: Sha256Bytes([3; 32]),
        }),
    }
    .into()
}
