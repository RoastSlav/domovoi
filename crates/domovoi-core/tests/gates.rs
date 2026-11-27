//! Gate tests for the human-only approval invariant (INV-1, INV-6, ADR 0007).
//!
//! Never delete or weaken these tests; extend them.

use std::collections::BTreeMap;

use domovoi_core::capability::ContractId;
use domovoi_core::{
    ApprovalChannel, ApprovalProvenance, Authorization, CheckKey, ExecutionError, ExecutionIntent,
    ExecutionRecord, HostId, HumanApproval, OperationId, PlanBinding, PolicyPermit,
    PolicyPermitParts, ReadTool, ReadToolCall, Risk, StepId, ToolArgError, UserId,
};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

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
        "host_binary",
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
        11,
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

/// A proposed execution of risk `risk`, for the approval gates.
#[cfg(test)]
fn proposed_execution(risk: Risk) -> ExecutionRecord {
    let intent = ExecutionIntent {
        contract: ContractId::parse("skill.restart_container").unwrap(),
        contract_version: 1,
        host: HostId::new(),
        check: CheckKey::parse("disk.usage").unwrap(),
        target: None,
        args: BTreeMap::new(),
        retry_of: None,
    };
    ExecutionRecord::propose(
        &intent,
        StepId::new(),
        OperationId::new(),
        "ab".repeat(32).parse().unwrap(),
        Timestamp::UNIX_EPOCH + SignedDuration::from_hours(1),
        risk,
        Timestamp::UNIX_EPOCH,
    )
}

#[test]
fn no_approval_without_human() {
    // `authorize` must take an `Authorization`; if its signature changes, this stops compiling.
    let authorize: fn(
        &mut ExecutionRecord,
        Authorization,
        ApprovalProvenance,
        &PlanBinding,
        Timestamp,
    ) -> Result<(), ExecutionError> = ExecutionRecord::authorize;
    let _ = authorize;

    // A policy authorization is refused for a destructive execution even though the type can
    // produce one in a test.
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
    let mut record = proposed_execution(Risk::Destructive);
    let binding = PlanBinding::Full("ab".repeat(32).parse().unwrap());
    assert_eq!(
        record.authorize(
            Authorization::policy(permit),
            ApprovalProvenance::new(None, "test"),
            &binding,
            Timestamp::UNIX_EPOCH,
        ),
        Err(ExecutionError::PermitNotAllowed),
    );
    assert!(!record.is_authorized());
}

#[test]
fn destructive_actions_cannot_be_approved_from_chat() {
    let now = Timestamp::UNIX_EPOCH;
    let mut record = proposed_execution(Risk::Destructive);
    let chat = HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Chat {
            platform: "discord".to_owned(),
            platform_user: "1".to_owned(),
        },
        now,
    );
    let binding = PlanBinding::Full("ab".repeat(32).parse().unwrap());
    assert_eq!(
        record.authorize(
            Authorization::human(chat),
            ApprovalProvenance::new(None, "test"),
            &binding,
            now,
        ),
        Err(ExecutionError::NeedsTypedConfirmation)
    );
    assert!(!record.is_authorized());
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

    use domovoi_core::{ExecutionRecord, HumanApproval, Principal};

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
    assert!(!deserializable!(ExecutionRecord));
    assert!(!deserializable!(Principal));
    // The probe does detect a type that can be deserialized.
    assert!(deserializable!(domovoi_core::Severity));
}

#[test]
fn model_meta_tools_are_closed() {
    use domovoi_core::MetaTool;
    use domovoi_core::meta::{MetaToolCall, ToolSet};

    let names: Vec<&str> = MetaTool::ALL.iter().map(MetaTool::name).collect();
    assert_eq!(
        names,
        [
            "list_capabilities",
            "describe_capability",
            "request_evidence",
            "search_experience",
            "propose_plan",
            "ask_user",
            "finish"
        ],
        "the meta-tool catalogue changed"
    );
    assert_eq!(MetaTool::ALL.len(), 7);
    let forbidden = [
        "approve",
        "exec",
        "write",
        "delete",
        "restart",
        "run_skill",
        "shell",
        "command",
        "authorize",
        "permit",
        "dispatch",
    ];
    for tool in MetaTool::ALL {
        for word in forbidden {
            assert!(
                !tool.name().contains(word),
                "{} contains {word}",
                tool.name()
            );
        }
        let schema = tool.json_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
    }
    let read = ToolSet::Read.tools();
    for tool in [MetaTool::ProposePlan, MetaTool::AskUser, MetaTool::Finish] {
        assert!(!read.contains(&tool));
    }
    assert!(read.iter().all(|tool| MetaTool::ALL.contains(tool)));

    // Adding a variant breaks this exhaustive match, which forces a visible review of the gate.
    fn variants(call: &MetaToolCall) -> usize {
        match call {
            MetaToolCall::ListCapabilities { .. } => 1,
            MetaToolCall::DescribeCapability { .. } => 2,
            MetaToolCall::RequestEvidence(_) => 3,
            MetaToolCall::SearchExperience { .. } => 4,
            MetaToolCall::ProposePlan(_) => 5,
            MetaToolCall::AskUser { .. } => 6,
            MetaToolCall::Finish(_) => 7,
        }
    }
    let _ = variants;
}

/// ATTEST-8: freshness is measured from the nonce issue time, and the gate names neither the
/// receipt nor the acceptance time.
#[test]
fn attest_8_gate_reads_nonce_issued_at_only() {
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/attest/gate.rs"))
            .expect("the gate source");
    assert!(source.contains("nonce_issued_at"));
    for forbidden in ["received_at", "accepted_at"] {
        assert!(
            !source.contains(forbidden),
            "the gate must not read `{forbidden}`"
        );
    }
}

/// ATTEST-10: the guard value and its reason refuse extra fields, unknown tags and unknown
/// reasons, and the reason is a closed enum.
#[test]
fn attest_10_expected_safety_is_strict() {
    use domovoi_core::attest::{ExpectedSafety, UnverifiedReason};
    let refused = |json: &str| serde_json::from_str::<ExpectedSafety>(json).is_err();
    assert!(refused(
        r#"{"state":"unverified","reason":"key_not_pinned","extra":1}"#
    ));
    assert!(refused(r#"{"state":"maybe","reason":"key_not_pinned"}"#));
    assert!(refused(r#"{"state":"unverified","reason":"not_a_reason"}"#));
    assert!(serde_json::from_str::<UnverifiedReason>(r#""not_a_reason""#).is_err());
    // Adding a reason breaks this exhaustive match, which forces a visible review of the gate.
    fn closed(reason: UnverifiedReason) -> usize {
        match reason {
            UnverifiedReason::KeyNotPinned => 1,
            UnverifiedReason::KeyMismatch => 2,
            UnverifiedReason::AttestationMissing => 3,
            UnverifiedReason::AttestationStale => 4,
            UnverifiedReason::SignatureInvalid => 5,
            UnverifiedReason::ReportIncomplete => 6,
            UnverifiedReason::GenerationRegressed => 7,
            UnverifiedReason::PathUncovered => 8,
            UnverifiedReason::ConfirmationStale => 9,
        }
    }
    assert_eq!(
        UnverifiedReason::ALL.map(closed),
        [1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
}
