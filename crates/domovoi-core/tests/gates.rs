//! Gate tests for the human-only approval invariant (INV-1, INV-6, ADR 0007).
//!
//! Never delete or weaken these tests; extend them.

use domovoi_core::{
    ActionChange, ActionError, ActionRequest, ApprovalChannel, HostId, HumanApproval, IncidentId,
    ManifestHash, Principal, ReadTool, ReadToolCall, Risk, SkillName, SkillSummary, ToolArgError,
    UserId,
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
    // `approve` must take a `HumanApproval`; if its signature changes, this stops compiling.
    let approve: fn(
        &mut ActionRequest,
        HumanApproval,
        Timestamp,
    ) -> Result<ActionChange, ActionError> = ActionRequest::approve;
    let _ = approve;
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
    };
    let args = skill.validate_args(&Default::default()).unwrap();
    let now = Timestamp::UNIX_EPOCH;
    let mut req = ActionRequest::propose(
        IncidentId::new(),
        HostId::new(),
        &skill,
        &args,
        Principal::System("triage"),
        SignedDuration::from_hours(1),
        now,
    );
    let chat = HumanApproval::new(
        UserId::new(),
        ApprovalChannel::Chat {
            platform: "discord".to_owned(),
            platform_user: "1".to_owned(),
        },
        now,
    );
    assert_eq!(
        req.approve(chat, now),
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
