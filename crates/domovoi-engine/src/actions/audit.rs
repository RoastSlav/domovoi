//! Audit entries for every action transition, with consistent kinds and details
//! (ARCHITECTURE.md §12): who proposed and approved, plan hash, argv hash, exit code,
//! output hash, duration and the verification result.

use domovoi_core::ports::AuditEvent;
use domovoi_core::{ActionRequest, ApprovalChannel, HumanApproval};
use jiff::Timestamp;
use serde_json::{Value, json};

/// A triage or system component proposed the action.
pub const PROPOSED: &str = "action.proposed";
/// A human approved it.
pub const APPROVED: &str = "action.approved";
/// A human denied it.
pub const DENIED: &str = "action.denied";
/// Its approval window closed.
pub const EXPIRED: &str = "action.expired";
/// Re-validation at dispatch failed.
pub const REJECTED: &str = "action.rejected";
/// It was sent to the agent.
pub const DISPATCHED: &str = "action.dispatched";
/// The skill exited 0.
pub const SUCCEEDED: &str = "action.succeeded";
/// The skill failed or could not run.
pub const FAILED: &str = "action.failed";
/// The originating check passed afterwards.
pub const VERIFIED: &str = "action.verified";
/// The originating check still failed afterwards.
pub const VERIFY_FAILED: &str = "action.verify_failed";
/// The loop guard stopped it.
pub const BLOCKED: &str = "action.blocked";

/// The actor for system steps of the action lifecycle.
pub const SYSTEM_ACTOR: &str = "system:actions";

/// What a skill run produced, as recorded in the audit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRecord {
    /// Exit code, or `None` when killed by a signal or never run.
    pub exit_code: Option<i32>,
    /// SHA-256 of the argv the agent ran, hex.
    pub argv_hash: String,
    /// SHA-256 of stdout and stderr, hex.
    pub output_sha256: String,
    /// Run time in milliseconds.
    pub duration_ms: u64,
    /// Whether the agent cut the output.
    pub truncated: bool,
    /// Redacted output, at most 4 KiB.
    pub excerpt: String,
}

/// The audit actor for `approval`: `user:<id>` on the web, `chat:<platform>:<user>` in chat.
#[must_use]
pub fn actor(approval: &HumanApproval) -> String {
    match approval.channel() {
        ApprovalChannel::Web { .. } => format!("user:{}", approval.user()),
        ApprovalChannel::Chat {
            platform,
            platform_user,
        } => format!("chat:{platform}:{platform_user}"),
    }
}

fn event(
    kind: &str,
    actor: String,
    action: &ActionRequest,
    at: Timestamp,
    extra: Value,
) -> AuditEvent {
    let mut detail = json!({
        "incident": action.incident().to_string(),
        "host": action.host().to_string(),
        "skill": action.skill().as_str(),
        "risk": action.risk(),
        "plan_hash": action.plan_hash().to_string(),
        "status": action.status(),
    });
    if let (Value::Object(base), Value::Object(more)) = (&mut detail, extra) {
        base.extend(more);
    }
    AuditEvent {
        at,
        actor,
        kind: kind.to_owned(),
        subject: format!("action:{}", action.id()),
        detail,
    }
}

/// `action.proposed`, by whoever proposed it.
#[must_use]
pub fn proposed(action: &ActionRequest, at: Timestamp) -> AuditEvent {
    let actor = match action.proposed_by() {
        domovoi_core::Principal::Human(user) => format!("user:{user}"),
        domovoi_core::Principal::System(component) => format!("system:{component}"),
    };
    let args = json!({ "args": action.args(), "expires_at": action.expires_at().to_string() });
    event(PROPOSED, actor, action, at, args)
}

/// `action.approved`, by the approving human.
#[must_use]
pub fn approved(action: &ActionRequest, approval: &HumanApproval, at: Timestamp) -> AuditEvent {
    let extra = json!({ "user": approval.user().to_string(), "channel": approval.channel() });
    event(APPROVED, actor(approval), action, at, extra)
}

/// `action.denied`, by the denying human.
#[must_use]
pub fn denied(action: &ActionRequest, approval: &HumanApproval, at: Timestamp) -> AuditEvent {
    let extra = json!({ "user": approval.user().to_string(), "channel": approval.channel() });
    event(DENIED, actor(approval), action, at, extra)
}

/// `action.expired`.
#[must_use]
pub fn expired(action: &ActionRequest, at: Timestamp) -> AuditEvent {
    event(EXPIRED, SYSTEM_ACTOR.to_owned(), action, at, Value::Null)
}

/// `action.rejected`, with why.
#[must_use]
pub fn rejected(action: &ActionRequest, reason: &str, at: Timestamp) -> AuditEvent {
    event(
        REJECTED,
        SYSTEM_ACTOR.to_owned(),
        action,
        at,
        json!({ "reason": reason }),
    )
}

/// `action.blocked` by the loop guard.
#[must_use]
pub fn blocked(action: &ActionRequest, reason: &str, at: Timestamp) -> AuditEvent {
    event(
        BLOCKED,
        SYSTEM_ACTOR.to_owned(),
        action,
        at,
        json!({ "reason": reason }),
    )
}

/// `action.dispatched`, with the plan hash that matched.
#[must_use]
pub fn dispatched(action: &ActionRequest, at: Timestamp) -> AuditEvent {
    event(DISPATCHED, SYSTEM_ACTOR.to_owned(), action, at, Value::Null)
}

/// `action.succeeded` or `action.failed`, with what the run produced, if it ran.
#[must_use]
pub fn finished(
    action: &ActionRequest,
    run: Option<&RunRecord>,
    reason: Option<&str>,
    at: Timestamp,
) -> AuditEvent {
    let kind = if reason.is_none() { SUCCEEDED } else { FAILED };
    let mut extra = json!({ "reason": reason });
    if let (Value::Object(map), Some(run)) = (&mut extra, run) {
        map.insert("exit_code".to_owned(), json!(run.exit_code));
        map.insert("argv_hash".to_owned(), json!(run.argv_hash));
        map.insert("output_sha256".to_owned(), json!(run.output_sha256));
        map.insert("duration_ms".to_owned(), json!(run.duration_ms));
        map.insert("truncated".to_owned(), json!(run.truncated));
        map.insert("output".to_owned(), json!(run.excerpt));
    }
    event(kind, SYSTEM_ACTOR.to_owned(), action, at, extra)
}

/// `action.verified` or `action.verify_failed`, with what the re-check said.
#[must_use]
pub fn verification(
    action: &ActionRequest,
    passed: bool,
    summary: &str,
    at: Timestamp,
) -> AuditEvent {
    let kind = if passed { VERIFIED } else { VERIFY_FAILED };
    event(
        kind,
        SYSTEM_ACTOR.to_owned(),
        action,
        at,
        json!({ "passed": passed, "check": summary }),
    )
}
