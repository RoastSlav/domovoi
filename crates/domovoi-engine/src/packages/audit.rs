//! Audit kinds and journal payloads of the install service.
//!
//! The subject of a package row is `package:<name>@<version>`; list and refresh rows use
//! `host:<id>`. A detail never holds archive contents or a signature.

use domovoi_core::ports::{AggregateRef, AuditDraft, JournalDraft, JournalKind};
use domovoi_core::{Actor, HostId, InstallJob, InstallKind, InstallTarget, TargetStatus, UserId};
use jiff::Timestamp;
use serde_json::{Map, Value, json};

/// An install was requested for one host.
pub(super) const INSTALL_REQUESTED: &str = "package.install_requested";
/// An uninstall was requested for one host.
pub(super) const UNINSTALL_REQUESTED: &str = "package.uninstall_requested";
/// The archive and its signature file reached the agent.
pub(super) const RELAYED: &str = "package.relayed";
/// The request was committed as handed to the agent.
pub(super) const INSTALL_DISPATCHED: &str = "package.install_dispatched";
/// The agent installed the package.
pub(super) const INSTALLED: &str = "package.installed";
/// The agent removed the package.
pub(super) const UNINSTALLED: &str = "package.uninstalled";
/// The agent refused or failed the request.
pub(super) const REJECTED: &str = "package.rejected";
/// The request never reached the agent.
pub(super) const NOT_RUN: &str = "package.not_run";
/// The request may have run; nobody knows.
pub(super) const OUTCOME_UNKNOWN: &str = "package.outcome_unknown";
/// The agent's journal says the install finished; its outputs were not seen.
pub(super) const CONFIRMED_BY_JOURNAL: &str = "package.install_confirmed_by_journal";
/// The agent's success outputs differ from what the job asked for.
pub(super) const MISMATCH: &str = "package.install_mismatch";
/// The owner cancelled a queued target.
pub(super) const CANCELLED: &str = "package.cancelled";
/// A host's inventory was replaced by an `agent.package.list` answer.
pub(super) const INVENTORY_REFRESHED: &str = "package.inventory_refreshed";
/// A person enabled a stored package's skills on a host.
pub(super) const ENABLED: &str = "package.enabled";
/// A person disabled a stored package's skills on a host.
pub(super) const DISABLED: &str = "package.disabled";
/// The agent refused to enable or disable.
pub(super) const ENABLE_REFUSED: &str = "package.enable_refused";
/// A listed package's signer is newly untrusted on the host.
pub(super) const KEY_UNTRUSTED: &str = "package.key_untrusted_detected";

/// The actor of what the worker records.
pub(super) fn worker() -> Actor {
    Actor::System(super::SYSTEM_PACKAGES.to_owned())
}

/// The audit subject of a job's package.
pub(super) fn subject(job: &InstallJob) -> String {
    format!("package:{}@{}", job.name, job.version)
}

/// What every row about one target says: the package, the host, the execution and the plan.
pub(super) fn detail(job: &InstallJob, target: &InstallTarget) -> Map<String, Value> {
    let channel =
        serde_json::to_value(target.authorization.approval().channel()).unwrap_or(Value::Null);
    let mut map = Map::new();
    map.insert("package".to_owned(), json!(job.name.to_string()));
    map.insert("version".to_owned(), json!(job.version.to_string()));
    map.insert("kind".to_owned(), json!(job.kind.as_str()));
    map.insert(
        "digest".to_owned(),
        json!(job.archive_sha256.map(|d| d.to_string())),
    );
    map.insert(
        "key_id".to_owned(),
        json!(job.key_id.as_ref().map(ToString::to_string)),
    );
    map.insert("host".to_owned(), json!(target.host.to_string()));
    map.insert("attempt".to_owned(), json!(target.attempt));
    map.insert("execution".to_owned(), json!(target.execution.to_string()));
    map.insert("plan_hash".to_owned(), json!(target.plan_hash.to_string()));
    map.insert("approval_channel".to_owned(), channel);
    map
}

/// One audit row about `target`, with `extra` fields added to the common detail.
pub(super) fn event(
    kind: &str,
    actor: Actor,
    at: Timestamp,
    job: &InstallJob,
    target: &InstallTarget,
    extra: &[(&str, Value)],
) -> AuditDraft {
    let mut map = detail(job, target);
    for (key, value) in extra {
        map.insert((*key).to_owned(), value.clone());
    }
    AuditDraft::new(actor, at, kind.to_owned(), subject(job), Value::Object(map))
}

/// The row that records an owner's request for one target.
pub(super) fn requested(
    job: &InstallJob,
    target: &InstallTarget,
    user: UserId,
    at: Timestamp,
) -> AuditDraft {
    let kind = match job.kind {
        InstallKind::Install => INSTALL_REQUESTED,
        InstallKind::Uninstall => UNINSTALL_REQUESTED,
    };
    event(kind, Actor::User(user), at, job, target, &[])
}

/// An audit row about a host rather than a package.
pub(super) fn host_event(
    kind: &str,
    actor: Actor,
    at: Timestamp,
    host: HostId,
    detail: Value,
) -> AuditDraft {
    AuditDraft::new(actor, at, kind.to_owned(), format!("host:{host}"), detail)
}

/// The journal row of a target's change from `from` (`None` for a new target) to `to`.
pub(super) fn journal(
    target: &InstallTarget,
    from: Option<TargetStatus>,
    to: TargetStatus,
    code: Option<&str>,
    actor: Actor,
    at: Timestamp,
) -> JournalDraft {
    let mut payload = json!({
        "target": target.id.to_string(),
        "host": target.host.to_string(),
        "attempt": target.attempt,
        "from": from.map_or("new", TargetStatus::as_str),
        "to": to.as_str(),
    });
    if let Some(code) = code {
        payload["code"] = json!(code);
    }
    JournalDraft {
        aggregate: AggregateRef::install_job(target.job),
        kind: JournalKind::InstallTarget,
        payload_v: 1,
        payload,
        actor,
        occurred_at: at,
    }
}
