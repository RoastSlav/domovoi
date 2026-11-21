//! What the hub believes a host has installed: updated by finished jobs and replaced by a
//! refresh that asks the agent to list its packages.

use std::collections::BTreeMap;

use domovoi_core::capability::Availability;
use domovoi_core::package::{PackageListEntry, PackageStatus, PrivilegeBoundary, keys};
use domovoi_core::ports::{InstallWrite, LinkError, StateWrite, Transaction};
use domovoi_core::{
    Actor, ExecutionId, HostId, HostPackage, InstallJob, InstallTarget, ObservedBy, PackageCall,
    PackageDispatch, PackagePlan, UserId,
};
use jiff::Timestamp;
use serde_json::{Value, json};

use super::audit;
use super::deps::Shared;
use super::error::PackageError;
use super::{execute, plan, settle};

const LIST_CONTRACT: &str = "agent.package.list";

/// The privilege boundary an output reports; anything but `in_effect` is read as not in effect.
fn boundary(outputs: &BTreeMap<String, String>) -> PrivilegeBoundary {
    match outputs
        .get(keys::OUT_PRIVILEGE_BOUNDARY)
        .map(String::as_str)
    {
        Some("in_effect") => PrivilegeBoundary::InEffect,
        _ => PrivilegeBoundary::NotInEffect,
    }
}

/// The inventory row an install's success outputs describe, or `None` for a job that has no
/// digest or key (an uninstall).
pub(super) fn from_install(
    job: &InstallJob,
    target: &InstallTarget,
    outputs: &BTreeMap<String, String>,
    now: Timestamp,
) -> Option<HostPackage> {
    let digest = job.archive_sha256?;
    let key_id = job.key_id.clone()?;
    let installed_at = outputs
        .get(keys::OUT_INSTALLED_AT)
        .and_then(|text| text.parse::<Timestamp>().ok())
        .unwrap_or(now);
    let skills = outputs
        .get(keys::OUT_SKILLS)
        .map(|text| {
            text.split(',')
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(HostPackage {
        host: target.host,
        name: job.name.clone(),
        version: job.version.clone(),
        key_id,
        archive_sha256: digest,
        installed_at,
        status: PackageStatus::Active,
        key_state: domovoi_core::package::KeyState::Trusted,
        skills,
        privilege_boundary: boundary(outputs),
        observed_at: now,
        observed_by: ObservedBy::Job,
    })
}

/// Reads the `packages` output of a list answer.
fn parse_list(outputs: &BTreeMap<String, String>) -> Result<Vec<PackageListEntry>, PackageError> {
    let text = outputs
        .get(keys::OUT_PACKAGES)
        .ok_or_else(|| PackageError::BadAnswer("no package list in the answer".to_owned()))?;
    serde_json::from_str(text).map_err(|err| PackageError::BadAnswer(err.to_string()))
}

/// Asks `host`'s agent what it has installed and replaces the hub's record of it, noting each
/// package whose signer is newly untrusted. Started by a person only.
pub(super) async fn refresh(
    shared: &Shared,
    host: HostId,
    user: UserId,
) -> Result<Vec<HostPackage>, PackageError> {
    let deps = &shared.deps;
    let contract = shared
        .contract(LIST_CONTRACT)
        .ok_or_else(|| PackageError::Conflict("the list contract is not known".to_owned()))?;
    match deps.registry.availability(host, &contract.id).await {
        Availability::Available => {}
        Availability::HostUnknown | Availability::HostAbsent { .. } => {
            return Err(PackageError::Link(LinkError::NotConnected));
        }
        Availability::HostIncompatible { .. } => {
            return Err(PackageError::Link(LinkError::Incompatible));
        }
        other => {
            return Err(PackageError::Conflict(format!(
                "the host does not offer package listing: {}",
                other.code()
            )));
        }
    }
    let snapshot = deps
        .caps
        .observer
        .observe_snapshot(host)
        .ok_or(PackageError::Link(LinkError::NotConnected))?;
    let key = plan::host_key(shared, host).await?;
    let plan_hash = PackagePlan::list(host, &key).hash();
    let now = deps.clock.now();
    let timeout = contract.timeout;
    let dispatch = PackageDispatch {
        execution: ExecutionId::new(),
        call: PackageCall::List,
        plan_hash,
        expires_at: now.checked_add(timeout).unwrap_or(now),
    };
    let execution = dispatch.execution;
    let deadline = now
        .checked_add(timeout)
        .and_then(|at| at.checked_add(shared.settings.result_grace))
        .unwrap_or(now);
    let answer = execute::list_packages(shared, host, &snapshot, dispatch, deadline)
        .await
        .map_err(PackageError::Link)?;
    let entries = parse_list(&answer.0)?;
    let privilege = boundary(&answer.0);
    let previous = deps
        .installs
        .inventory(host)
        .await
        .map_err(|err| PackageError::Store(err.to_string()))?;
    let at = deps.clock.now();
    let rows: Vec<HostPackage> = entries
        .iter()
        .map(|entry| HostPackage::from_entry(host, entry, privilege, at, ObservedBy::Refresh))
        .collect();
    let actor = Actor::User(user);
    let mut tx = Transaction::new()
        .write(StateWrite::Install(InstallWrite::Inventory {
            host,
            entries: rows.clone(),
            replace: true,
        }))
        .audit(audit::host_event(
            audit::INVENTORY_REFRESHED,
            actor.clone(),
            at,
            host,
            json!({
                "count": rows.len(),
                "execution": execution.to_string(),
                "plan_hash": plan_hash.to_string(),
            }),
        ));
    for row in &rows {
        if row.status == PackageStatus::DisabledUntrustedKey && !was_untrusted(&previous, row) {
            tx = tx.audit(untrusted(row, actor.clone(), at));
        }
    }
    settle::commit(shared, tx).await.map_err(|err| match err {
        settle::SettleError::Conflict => PackageError::Conflict("the inventory changed".to_owned()),
        settle::SettleError::Store(text) => PackageError::Store(text),
    })?;
    Ok(rows)
}

/// Whether the row for the same package was already untrusted before this refresh.
fn was_untrusted(previous: &[HostPackage], row: &HostPackage) -> bool {
    previous
        .iter()
        .any(|old| old.name == row.name && old.status == PackageStatus::DisabledUntrustedKey)
}

/// The audit row for a package whose signer the host no longer trusts.
fn untrusted(row: &HostPackage, actor: Actor, at: Timestamp) -> domovoi_core::ports::AuditDraft {
    let detail: Value = json!({
        "package": row.name.to_string(),
        "version": row.version.to_string(),
        "key_id": row.key_id.to_string(),
        "host": row.host.to_string(),
        "key_state": format!("{:?}", row.key_state).to_lowercase(),
    });
    domovoi_core::ports::AuditDraft::new(
        actor,
        at,
        audit::KEY_UNTRUSTED.to_owned(),
        format!("package:{}@{}", row.name, row.version),
        detail,
    )
}
