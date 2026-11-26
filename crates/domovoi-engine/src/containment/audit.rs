//! Audit kinds and details of containment. The subject is `host:<id>`; a detail holds the block
//! id, the canonical source, the TTL, the plan hash, the kind of safety assurance and the
//! execution ids, never a password, a key or a signature value.

use domovoi_core::Actor;
use domovoi_core::ports::{AuditDraft, BlockPhase, BlockRow};
use jiff::Timestamp;
use serde_json::{Map, Value, json};

/// A block was requested and approved.
pub const REQUESTED: &str = "containment.requested";
/// The apply was committed as about to be sent.
pub const APPLYING: &str = "containment.applying";
/// The helper applied the block with the kernel's short timeout.
pub const APPLIED: &str = "containment.applied";
/// The confirmation was committed as about to be sent.
pub const CONFIRMING: &str = "containment.confirming";
/// The block holds for its TTL.
pub const CONFIRMED: &str = "containment.confirmed";
/// The block was removed on purpose.
pub const ROLLED_BACK: &str = "containment.rolled_back";
/// The block went away on its own timer.
pub const EXPIRED: &str = "containment.expired";
/// The block was lost with the helper's table.
pub const LOST: &str = "containment.lost";
/// Nothing reached the kernel.
pub const NOT_RUN: &str = "containment.not_run";
/// A request was handed over and nobody knows what happened.
pub const OUTCOME_UNKNOWN: &str = "containment.outcome_unknown";
/// The agent or the helper refused; the detail names the token.
pub const REFUSED: &str = "containment.refused";
/// A person approved a block on the unverified path.
pub const UNVERIFIED_APPROVED: &str = "containment.unverified_approved";
/// The owner released a block early.
pub const RELEASED: &str = "containment.released";
/// The owner refreshed a host's block list.
pub const LISTED: &str = "containment.listed";

/// The actor of what the driver records on its own.
#[must_use]
pub fn system() -> Actor {
    Actor::System("containment".to_owned())
}

/// The audit kind that records a block reaching `phase`.
#[must_use]
pub const fn kind_of(phase: BlockPhase) -> &'static str {
    match phase {
        BlockPhase::Queued => REQUESTED,
        BlockPhase::Applying => APPLYING,
        BlockPhase::Applied => APPLIED,
        BlockPhase::Confirming => CONFIRMING,
        BlockPhase::Confirmed => CONFIRMED,
        BlockPhase::RollingBack | BlockPhase::RolledBack => ROLLED_BACK,
        BlockPhase::Expired => EXPIRED,
        BlockPhase::Lost => LOST,
        BlockPhase::NotRun | BlockPhase::Failed => NOT_RUN,
        BlockPhase::OutcomeUnknown => OUTCOME_UNKNOWN,
    }
}

/// The kind of assurance the stored assurance JSON records: `verified` or `unverified`.
#[must_use]
pub fn assurance_kind(assurance: &str) -> String {
    serde_json::from_str::<Value>(assurance)
        .ok()
        .and_then(|value| value.get("kind").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// One audit row about `block`, with `token` when a refusal or failure has one.
#[must_use]
pub fn draft(
    kind: &str,
    block: &BlockRow,
    actor: Actor,
    at: Timestamp,
    token: Option<&str>,
) -> AuditDraft {
    let mut detail = Map::new();
    detail.insert("block".to_owned(), json!(block.block_id.to_string()));
    detail.insert("source".to_owned(), json!(block.source));
    detail.insert("ttl_s".to_owned(), json!(block.ttl_s));
    detail.insert("plan_hash".to_owned(), json!(block.plan_hash));
    detail.insert(
        "assurance".to_owned(),
        json!(assurance_kind(&block.assurance)),
    );
    detail.insert("authorization".to_owned(), json!(block.authorization_kind));
    detail.insert("phase".to_owned(), json!(block.phase.as_str()));
    if let Some(id) = block.confirm_execution {
        detail.insert("confirm_execution".to_owned(), json!(id.to_string()));
    }
    if let Some(id) = block.rollback_execution {
        detail.insert("rollback_execution".to_owned(), json!(id.to_string()));
    }
    if let Some(token) = token {
        detail.insert("token".to_owned(), json!(token));
    }
    AuditDraft::new(
        actor,
        at,
        kind.to_owned(),
        format!("host:{}", block.host_id),
        Value::Object(detail),
    )
}

/// One audit row about a host rather than a block, such as a refresh of its block list.
#[must_use]
pub fn host_draft(
    kind: &str,
    host: domovoi_core::HostId,
    actor: Actor,
    at: Timestamp,
    detail: Value,
) -> AuditDraft {
    AuditDraft::new(actor, at, kind.to_owned(), format!("host:{host}"), detail)
}
