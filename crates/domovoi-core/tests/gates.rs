//! Gate tests for the human-only approval invariant (INV-1, INV-6, ADR 0007).
//!
//! Never delete or weaken these tests; extend them.

use domovoi_core::{
    ActionChange, ActionError, ActionRequest, ApprovalChannel, ApprovalProvenance, Authorization,
    CheckKey, HostId, HumanApproval, Incident, IncidentId, KeyFingerprint, ManifestHash,
    PlanBinding, PolicyPermit, PolicyPermitParts, Principal, ReadTool, ReadToolCall, Risk,
    Severity, Signal, SkillName, SkillSummary, SourceKind, TargetKey, ToolArgError, UserId,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

#[cfg(test)]
fn incident(id: IncidentId, host: HostId, check: &str, target: Option<&str>) -> Incident {
    let signal = Signal::firing(
        SourceKind::BuiltIn,
        host,
        CheckKey::parse(check).unwrap(),
        target.map(|t| TargetKey::parse(t).unwrap()),
        Severity::Warning,
        "test",
        Timestamp::UNIX_EPOCH,
    );
    Incident::new_pending(&signal, Timestamp::UNIX_EPOCH).with_id(id)
}

#[test]
fn read_tool_catalogue_is_read_only() {
    let expected = [
        "docker_ps",
        "docker_logs",
        "docker_inspect",
        "docker_events",
        "host_overview",
        "journal",
        "listening_ports",
        "smart_report",
        "incident_history",
        "notes_search",
        "run_read_skill",
    ];
    let names: Vec<&str> = ReadTool::ALL.iter().map(ReadTool::name).collect();
    assert_eq!(names, expected, "the read tool catalogue changed");
    let forbidden = [
        "approve",
        "exec",
        "write",
        "delete",
        "restart",
        "run_skill",
        "shell",
        "command",
    ];
    for name in names {
        for word in forbidden {
            assert!(!name.contains(word), "tool `{name}` contains `{word}`");
        }
    }
}

#[test]
fn read_derived_contracts_are_read_only() {
    let contracts = domovoi_core::capability::builtin::builtin_contracts();
    assert_eq!(
        contracts.len(),
        10,
        "the built-in read contract set changed"
    );
    for contract in &contracts {
        assert_eq!(
            contract.effect,
            domovoi_core::capability::EffectClass::Read,
            "{} is not a read",
            contract.id
        );
        assert_eq!(
            contract.source,
            domovoi_core::capability::ContractSource::BuiltIn,
            "{} is not a built-in contract",
            contract.id
        );
        assert!(
            !contract.id.as_str().starts_with("skill."),
            "{} claims the skill family",
            contract.id
        );
    }
    assert!(
        domovoi_core::capability::builtin::contract_id_of(ReadTool::RunReadSkill).is_none(),
        "the skill entry point must not map to a built-in contract"
    );
}

#[test]
fn read_tool_call_rejects_unknown_tool_names() {
    for name in [
        "approve_action",
        "run_skill",
        "execute",
        "run_write_skill",
        "shell",
        "DockerPs",
        "",
    ] {
        assert_eq!(
            ReadToolCall::from_json(name, json!({})),
            Err(ToolArgError::UnknownTool(name.to_owned())),
            "`{name}` was accepted"
        );
    }
}

#[test]
fn no_approval_without_human() {
    // `approve` must take an `Authorization`; if its signature changes, this stops compiling.
    let approve: fn(
        &mut ActionRequest,
        Authorization,
        ApprovalProvenance,
        &PlanBinding,
        Timestamp,
    ) -> Result<ActionChange, ActionError> = ActionRequest::approve;
    let _ = approve;

    // A policy authorization is refused at M1 even though the type can produce one in a test.
    let ceiling = "cd".repeat(32);
    let permit = PolicyPermit::issue(PolicyPermitParts {
        decision_id: "dec-1",
        rule_ids: Vec::new(),
        policy_version: "1",
        plan_hash: "ab".repeat(32).parse().unwrap(),
        issued_at: Timestamp::UNIX_EPOCH,
        expires_at: Timestamp::UNIX_EPOCH + SignedDuration::from_hours(1),
        ceiling_hash: &ceiling,
        safety_identity: None,
    })
    .unwrap();
    let skill = SkillSummary {
        name: SkillName::parse("restart_container").unwrap(),
        description: "Restart a container".to_owned(),
        risk: Risk::Write,
        params: Vec::new(),
        applies_to: Vec::new(),
        manifest_hash: "00".repeat(32).parse::<ManifestHash>().unwrap(),
        entry_script_digest: domovoi_core::ScriptDigest::of_bytes(b"entry"),
        timeout: SignedDuration::from_secs(60),
        verify: None,
        readme: None,
    };
    let args = skill.validate_args(&Default::default()).unwrap();
    let host = HostId::new();
    let incident = incident(IncidentId::new(), host, "disk.usage", None);
    let mut req = ActionRequest::propose(
        &incident,
        &skill,
        &args,
        Some(KeyFingerprint::of_public_key(&[7u8; 32])),
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        Timestamp::UNIX_EPOCH,
    )
    .unwrap();
    let binding = PlanBinding::Full(req.plan_hash_for_approval());
    assert_eq!(
        req.approve(
            Authorization::policy(permit),
            ApprovalProvenance::new(None, "test"),
            &binding,
            Timestamp::UNIX_EPOCH,
        ),
        Err(ActionError::NotAllowed {
            status: domovoi_core::ActionStatus::Proposed,
            operation: "approve",
        }),
    );
}

#[test]
fn destructive_actions_cannot_be_approved_from_chat() {
    let skill = SkillSummary {
        name: SkillName::parse("wipe_volume").unwrap(),
        description: "Delete a volume".to_owned(),
        risk: Risk::Destructive,
        params: Vec::new(),
        applies_to: Vec::new(),
        manifest_hash: "00".repeat(32).parse::<ManifestHash>().unwrap(),
        entry_script_digest: domovoi_core::ScriptDigest::of_bytes(b"#!/bin/sh\nexit 0\n"),
        timeout: jiff::SignedDuration::from_secs(60),
        verify: None,
        readme: None,
    };
    let args = skill.validate_args(&Default::default()).unwrap();
    let now = Timestamp::UNIX_EPOCH;
    let host = HostId::new();
    let incident = incident(IncidentId::new(), host, "disk.usage", None);
    let mut req = ActionRequest::propose(
        &incident,
        &skill,
        &args,
        Some(KeyFingerprint::of_public_key(&[7u8; 32])),
        Principal::System("triage".into()),
        SignedDuration::from_hours(1),
        now,
    )
    .unwrap();
    let chat = HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Chat {
            platform: "discord".to_owned(),
            platform_user: "1".to_owned(),
        },
        now,
    );
    let binding = PlanBinding::Full(req.plan_hash_for_approval());
    assert_eq!(
        req.approve(
            Authorization::human(chat),
            ApprovalProvenance::new(None, "test"),
            &binding,
            now,
        ),
        Err(ActionError::DestructiveNeedsTypedConfirmation)
    );
}

/// `AuditLog` exposes only `append` and `list` (INV-7). This impl defines exactly those two
/// methods, so adding any required method to the trait (an update, a delete) stops this
/// test compiling. Adding a provided (default) method would not; reviewers must reject that
/// too.
#[test]
fn audit_log_has_no_mutation_api() {
    use async_trait::async_trait;
    use domovoi_core::ports::{AuditEvent, AuditFilter, AuditLog, Page, Paged, StoreError};

    struct OnlyAppendAndList;

    #[async_trait]
    impl AuditLog for OnlyAppendAndList {
        async fn append(&self, _event: &AuditEvent) -> Result<u64, StoreError> {
            Ok(1)
        }

        async fn list(
            &self,
            _filter: &AuditFilter,
            _page: &Page,
        ) -> Result<Paged<AuditEvent>, StoreError> {
            Ok(Paged {
                items: Vec::new(),
                next_cursor: None,
            })
        }
    }

    let log: &dyn AuditLog = &OnlyAppendAndList;
    let _ = log;
}

/// Approvals can't be deserialized (INV-1, ADR 0014). They come only from
/// `HumanApproval::new` after authentication and `HumanApproval::restore` when the store
/// loads one; arch-check restricts both call sites. This probe resolves to `true` only if
/// the type implements `Deserialize`.
#[test]
fn approvals_and_actions_cannot_be_deserialized() {
    use std::marker::PhantomData;

    use domovoi_core::{ActionRequest, HumanApproval, Principal};

    struct Probe<T>(PhantomData<T>);
    trait Deserializable {
        fn deserializable(&self) -> bool {
            true
        }
    }
    impl<T: serde::de::DeserializeOwned> Deserializable for &Probe<T> {}
    trait NotDeserializable {
        fn deserializable(&self) -> bool {
            false
        }
    }
    impl<T> NotDeserializable for Probe<T> {}

    // The double reference picks the `Deserializable` impl when it applies.
    macro_rules! deserializable {
        ($t:ty) => {
            (&&Probe::<$t>(PhantomData)).deserializable()
        };
    }
    assert!(!deserializable!(HumanApproval));
    assert!(!deserializable!(Authorization));
    assert!(!deserializable!(PolicyPermit));
    assert!(!deserializable!(ActionRequest));
    assert!(!deserializable!(Principal));
    // The probe does detect a type that can be deserialized.
    assert!(deserializable!(domovoi_core::Severity));
}
