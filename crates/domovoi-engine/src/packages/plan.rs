//! What an install or uninstall request would do on each host, and the hashes an owner's
//! confirmation binds. Read-only: nothing here writes.

use std::collections::BTreeSet;

use domovoi_core::action::job_plan_hash;
use domovoi_core::capability::{Availability, TargetQuery};
use domovoi_core::package::{ArchiveDigest, KeyId, PackageName, PackageVersion};
use domovoi_core::{
    HostId, HostName, HostPackage, InstallJob, InstallKind, KeyFingerprint, PackageArtifact,
    PackagePlan, PlanHash,
};

use super::deps::Shared;
use super::error::PackageError;

/// The most hosts one job may name.
pub const MAX_HOSTS: usize = 16;

/// What the owner asks for: an install or uninstall of one package on one or more hosts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRequest {
    /// Install or uninstall.
    pub kind: InstallKind,
    /// The package.
    pub name: PackageName,
    /// The version.
    pub version: PackageVersion,
    /// The hosts, 1 to 16, each once.
    pub hosts: Vec<HostId>,
}

/// What a request would do on one host.
#[derive(Debug, Clone, PartialEq)]
pub struct HostPlan {
    /// The host.
    pub host: HostId,
    /// Its name, for display.
    pub host_name: HostName,
    /// Whether the hub would offer the package contract on it now.
    pub availability: Availability,
    /// What the hub last saw of this package on the host.
    pub installed: Option<HostPackage>,
    /// The plan hash an approval for this host binds.
    pub plan_hash: PlanHash,
    /// Short plain sentences about what the owner should know before confirming.
    pub warnings: Vec<String>,
}

/// The plan of a whole request: every host, and the hash that covers them all.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallPlan {
    /// Install or uninstall.
    pub kind: InstallKind,
    /// The package.
    pub name: PackageName,
    /// The version.
    pub version: PackageVersion,
    /// The archive digest, for an install.
    pub archive_sha256: Option<ArchiveDigest>,
    /// The signing key, for an install.
    pub key_id: Option<KeyId>,
    /// The owner's label for the signing key, when the hub's list has one.
    pub signer_label: Option<String>,
    /// The hash over every host's plan; what the owner confirms.
    pub job_plan_hash: PlanHash,
    /// One entry per host, in the order the request named them.
    pub hosts: Vec<HostPlan>,
}

/// The contract id a job of `kind` runs.
pub(super) const fn contract_id(kind: InstallKind) -> &'static str {
    match kind {
        InstallKind::Install => "agent.package.install",
        InstallKind::Uninstall => "agent.package.uninstall",
    }
}

/// The enrolled key fingerprint a plan for `host` binds.
pub(super) async fn host_key(
    shared: &Shared,
    host: HostId,
) -> Result<KeyFingerprint, PackageError> {
    shared
        .deps
        .resolver
        .resolve(TargetQuery::Host { host })
        .await
        .map(|resolved| resolved.host_key)
        .map_err(|_| PackageError::NotEnrolled(host))
}

/// The plan of one host of a job over this package.
pub(super) fn host_plan(
    kind: InstallKind,
    name: &PackageName,
    version: &PackageVersion,
    install: Option<(&ArchiveDigest, &KeyId)>,
    host: HostId,
    key: &KeyFingerprint,
) -> PackagePlan {
    match (kind, install) {
        (InstallKind::Install, Some((digest, key_id))) => {
            PackagePlan::install(name, version, digest, key_id, host, key)
        }
        _ => PackagePlan::uninstall(name, version, host, key),
    }
}

/// The per-host hash a job's target is bound to, recomputed from what the hub knows now.
pub(super) async fn current_hash(
    shared: &Shared,
    job: &InstallJob,
    host: HostId,
) -> Result<PlanHash, PackageError> {
    let key = host_key(shared, host).await?;
    let install = job.archive_sha256.as_ref().zip(job.key_id.as_ref());
    Ok(host_plan(job.kind, &job.name, &job.version, install, host, &key).hash())
}

/// Plans `request` from current data.
pub(super) async fn build(
    shared: &Shared,
    request: &PlanRequest,
) -> Result<InstallPlan, PackageError> {
    check_hosts(&request.hosts)?;
    let store = |err: domovoi_core::ports::StoreError| PackageError::Store(err.to_string());
    let artifact = match request.kind {
        InstallKind::Install => Some(
            shared
                .deps
                .installs
                .artifact(&request.name, &request.version)
                .await
                .map_err(store)?
                .ok_or_else(|| PackageError::NoSuchPackage {
                    name: request.name.clone(),
                    version: request.version.clone(),
                })?,
        ),
        InstallKind::Uninstall => None,
    };
    let install = artifact
        .as_ref()
        .map(|a: &PackageArtifact| (&a.archive_sha256, &a.key_id));
    let mut hosts = Vec::with_capacity(request.hosts.len());
    for host in &request.hosts {
        hosts.push(plan_host(shared, request, install, *host).await?);
    }
    let hashes: Vec<PlanHash> = hosts.iter().map(|h| h.plan_hash).collect();
    Ok(InstallPlan {
        kind: request.kind,
        name: request.name.clone(),
        version: request.version.clone(),
        archive_sha256: artifact.as_ref().map(|a| a.archive_sha256),
        key_id: artifact.as_ref().map(|a| a.key_id.clone()),
        signer_label: artifact.as_ref().and_then(|a| a.signer_label.clone()),
        job_plan_hash: job_plan_hash(request.kind, &hashes),
        hosts,
    })
}

fn check_hosts(hosts: &[HostId]) -> Result<(), PackageError> {
    if hosts.is_empty() || hosts.len() > MAX_HOSTS {
        return Err(PackageError::HostCount);
    }
    let mut seen = BTreeSet::new();
    for host in hosts {
        if !seen.insert(*host) {
            return Err(PackageError::DuplicateHost(*host));
        }
    }
    Ok(())
}

async fn plan_host(
    shared: &Shared,
    request: &PlanRequest,
    install: Option<(&ArchiveDigest, &KeyId)>,
    host: HostId,
) -> Result<HostPlan, PackageError> {
    let deps = &shared.deps;
    let store = |err: domovoi_core::ports::StoreError| PackageError::Store(err.to_string());
    let registered = deps
        .hosts
        .get_host(host)
        .await
        .map_err(store)?
        .ok_or(PackageError::UnknownHost(host))?;
    let key = host_key(shared, host).await?;
    let plan_hash = host_plan(
        request.kind,
        &request.name,
        &request.version,
        install,
        host,
        &key,
    )
    .hash();
    let availability = match shared.contract(contract_id(request.kind)) {
        Some(contract) => deps.registry.availability(host, &contract.id).await,
        None => Availability::NotInstalled,
    };
    let installed = deps
        .installs
        .inventory(host)
        .await
        .map_err(store)?
        .into_iter()
        .find(|row| row.name == request.name);
    let warnings = warnings(request, install, &availability, installed.as_ref());
    Ok(HostPlan {
        host,
        host_name: registered.name,
        availability,
        installed,
        plan_hash,
        warnings,
    })
}

fn warnings(
    request: &PlanRequest,
    install: Option<(&ArchiveDigest, &KeyId)>,
    availability: &Availability,
    installed: Option<&HostPackage>,
) -> Vec<String> {
    let mut out = Vec::new();
    if !matches!(availability, Availability::Available) {
        out.push(format!(
            "this host cannot take package changes now ({})",
            availability.code()
        ));
    }
    match (request.kind, installed) {
        (InstallKind::Install, Some(row)) => {
            if row.version == request.version && Some(&row.archive_sha256) == install.map(|i| i.0) {
                out.push("this exact package is already installed here".to_owned());
            } else {
                out.push(format!(
                    "version {} is installed here and will be replaced",
                    row.version
                ));
            }
            if row.status == domovoi_core::package::PackageStatus::DisabledUntrustedKey {
                out.push("the installed copy's signer is no longer trusted".to_owned());
            }
        }
        (InstallKind::Uninstall, None) => {
            out.push("the hub has no record of this package on the host".to_owned());
        }
        (InstallKind::Uninstall, Some(row)) if row.version != request.version => {
            out.push(format!(
                "version {} is installed here, not {}",
                row.version, request.version
            ));
        }
        _ => {}
    }
    out
}
