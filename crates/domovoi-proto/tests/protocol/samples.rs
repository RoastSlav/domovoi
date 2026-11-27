//! One fixed sample per message kind, with fixed ids and times so snapshots are stable.

use std::collections::BTreeMap;

use domovoi_core::attest::{BootId, Hash32, UnverifiedReason};
use domovoi_core::ports::LogLine;
use domovoi_core::{
    AgentId, CheckKey, DigestFact, Evidence, EvidenceValue, ExecutionId, FactValue, HostId,
    HostName, PlanHash, ReadToolCall, RequestId, ScriptDigest, Severity, Signal, SignalId,
    SignalStatus, SkillName, SkillSummary, SourceKind, TargetKey,
};
use domovoi_proto::messages::*;
use domovoi_proto::{
    Base64Bytes, CertFingerprint, EnrollToken, Message, Nonce, Open, PROTOCOL_VERSION, PublicKey,
    Release, Sha256Bytes, Signature,
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
        docker: Some(false),
    }
}

fn skill_summary() -> SkillSummary {
    // A real computed digest of the bytes the sample entry stands for, not a literal.
    let digest = ScriptDigest::of_bytes(b"#!/bin/sh\nexec /usr/local/bin/restart \"$1\"\n");
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
        "entry_script_digest": digest.to_string(),
        "readme": "# Restart container\n\nRestarts one container managed by compose.",
    }))
    .unwrap()
}

fn execution_id() -> Uuid {
    Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00f1)
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
            facts: Box::new(facts()),
            capabilities: vec!["docker".to_owned(), "journal".to_owned()],
            features: Vec::new(),
            journal_epoch: Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00f0),
            catalog_hash: None,
            ceiling_hash: None,
            ceiling_summary: None,
            skill_runner_status: SkillRunnerStatus::Absent,
            helper_status: HelperStatus::Absent,
            runs_as_root: true,
            in_container: true,
            helper_identity: None,
            skills: vec![skill_summary()],
            checks: vec![CheckInfo {
                key: CheckKey::parse("disk.usage").unwrap(),
                enabled: true,
                interval_seconds: 300,
                thresholds: BTreeMap::from([("warn_percent".to_owned(), serde_json::json!(85))]),
            }],
            docker: Some(DockerReport {
                engine: "Podman".to_owned(),
                endpoint: "unix:///run/podman/podman.sock".to_owned(),
                available: true,
                locked: false,
                enabled: true,
                problem: None,
            }),
            package_skills: vec![PackageSkillState {
                package: "nginx-tools".to_owned(),
                version: "1.2.0".to_owned(),
                skill: "nginx_reload".to_owned(),
                enabled: false,
                reason: Some("stored, not enabled".to_owned()),
            }],
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
            retired: vec!["disk./old.free_gib".to_owned()],
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
        GetLogs {
            lines: 50,
            min_level: "info".to_owned(),
            offset: 0,
            deadline: DEADLINE,
        }
        .into(),
        Logs {
            lines: vec![
                LogLine {
                    at: T0,
                    level: "warn".to_owned(),
                    target: "agent.checks".to_owned(),
                    message: "/srv is 97% full".to_owned(),
                },
                LogLine {
                    at: T0,
                    level: "error".to_owned(),
                    target: "agent.link".to_owned(),
                    message: "agent not connected; retrying in 4s".to_owned(),
                },
            ],
            has_more: false,
        }
        .into(),
        RunSkill {
            action_id: ExecutionId::from_uuid(Uuid::from_u128(
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
            type_name: "future_kind".to_owned(),
        }
        .into(),
        AgentStatus {
            helper_status: HelperStatus::Absent,
            helper_identity: None,
            skill_runner_status: SkillRunnerStatus::Absent,
            runs_as_root: true,
            in_container: true,
            ceiling_hash: None,
            ceiling_summary: None,
        }
        .into(),
        RunCapability {
            execution_id: execution_id(),
            contract: "container.restart".to_owned(),
            version: 1,
            target: TargetKey::parse("container:plex").unwrap(),
            args: BTreeMap::from([("container".to_owned(), "plex".to_owned())]),
            plan_hash: plan_hash(),
            authorization_kind: AuthorizationKind::Human,
            expires_at: DEADLINE,
            expected_safety: None,
        }
        .into(),
        CapabilityResult {
            outcome: CapabilityOutcome::Succeeded {
                outputs: BTreeMap::from([("previous_state".to_owned(), "running".to_owned())]),
            },
        }
        .into(),
        Refused {
            code: Open::new("rate_limited", Some(RefusalCode::RateLimited)),
            detail: "too many mutations this hour".to_owned(),
            retry_after: Some(30),
            scope: Some(RefusalScope::Global),
        }
        .into(),
        ActionStatusQuery {
            execution_ids: vec![execution_id()],
            expected_epoch: Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00f0),
        }
        .into(),
        ActionStatusAnswers {
            epoch: Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_00f0),
            answers: vec![ActionStatusAnswer {
                execution_id: execution_id(),
                state: ActionState::Finished {
                    result: CapabilityResult {
                        outcome: CapabilityOutcome::Succeeded {
                            outputs: BTreeMap::new(),
                        },
                    },
                },
            }],
        }
        .into(),
        GetCatalog {
            page: 0,
            page_size: 50,
        }
        .into(),
        CatalogPage {
            entries: vec![CatalogEntry {
                contract: "container.restart".to_owned(),
                version: 1,
                descriptor: serde_json::json!({"family": "container", "verb": "restart"}),
            }],
            next_page: None,
        }
        .into(),
        TransferBegin {
            transfer_id: execution_id(),
            kind: Open::new("package_artifact", Some(TransferKind::PackageArtifact)),
            size: 3,
            sha256: Sha256Bytes([4; 32]),
        }
        .into(),
        TransferChunk {
            transfer_id: execution_id(),
            seq: 0,
            bytes: Base64Bytes(vec![1, 2, 3]),
        }
        .into(),
        TransferEnd {
            transfer_id: execution_id(),
        }
        .into(),
        TransferResult {
            transfer_id: execution_id(),
            outcome: TransferOutcome::Stored,
        }
        .into(),
        AttestSafety {
            nonce: Nonce([5; 32]),
        }
        .into(),
        SafetyAttestation {
            public_key: PublicKey([6; 32]),
            report: Base64Bytes(b"domovoi-attestation".to_vec()),
            signature: Signature([7; 64]),
        }
        .into(),
        SkillsUpdate {
            skills: vec![skill_summary()],
            package_skills: vec![PackageSkillState {
                package: "media-tools".to_owned(),
                version: "1.2.0".to_owned(),
                skill: "media_status".to_owned(),
                enabled: false,
                reason: Some("not enabled".to_owned()),
            }],
            catalog_hash: Some(Sha256Bytes([0xab; 32])),
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

/// A `run_capability` for `contain` that carries a verified safety identity.
pub fn contain_verified() -> Message {
    contain(ExpectedSafety::Verified {
        fingerprint: Hash32::from_bytes([0xab; 32]),
        boot_id: BootId::parse("5f0c1d2e-3a4b-4c5d-8e9f-0a1b2c3d4e5f").unwrap(),
        generation: 7,
        config_hash: Hash32::from_bytes([1; 32]),
        route_state_hash: Hash32::from_bytes([2; 32]),
    })
}

/// A `run_capability` for `contain` whose helper is unverified: it carries a reason and no
/// identity.
pub fn contain_unverified() -> Message {
    contain(ExpectedSafety::Unverified {
        reason: UnverifiedReason::KeyNotPinned,
    })
}

fn contain(expected_safety: ExpectedSafety) -> Message {
    RunCapability {
        execution_id: execution_id(),
        contract: "firewall.block_source".to_owned(),
        version: 1,
        target: TargetKey::parse("host:nas").unwrap(),
        args: BTreeMap::from([("source".to_owned(), "203.0.113.9/32".to_owned())]),
        plan_hash: plan_hash(),
        authorization_kind: AuthorizationKind::Human,
        expires_at: DEADLINE,
        expected_safety: Some(expected_safety),
    }
    .into()
}

/// A `reject` about the agent's release: no longer sent, but frames from older hubs still
/// have to decode, so its wire shape stays pinned.
pub fn agent_too_old() -> Message {
    Reject {
        reason: RejectReason::AgentTooOld {
            minimum: Release {
                major: 0,
                minor: 0,
                patch: 5,
            },
        },
    }
    .into()
}
