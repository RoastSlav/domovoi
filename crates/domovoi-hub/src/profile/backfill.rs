//! Copies a host profile's machine description into the knowledge store as a `host_platform`
//! fact.
//!
//! Containers, ports and skills in a profile are inventory and stay there; only the machine
//! description is knowledge.

use std::sync::Arc;

use domovoi_core::knowledge::{FactSource, Predicate, SubjectRef};
use domovoi_core::ports::{FactFilter, Page};
use domovoi_core::{Actor, HostId, SYSTEM_PROFILE};
use domovoi_engine::knowledge::{FactDraft, KnowledgeService};
use serde_json::json;

use super::{HostProfile, ProfileDeps, stored};

/// The platform fact a profile describes.
fn platform_draft(host: HostId, profile: &HostProfile) -> FactDraft {
    let facts = &profile.facts;
    FactDraft {
        subject: SubjectRef::Host(host),
        predicate: Predicate::HostPlatform,
        value: json!({
            "hostname": facts.hostname,
            "os": facts.os,
            "kernel": facts.kernel,
            "arch": facts.arch,
            "cpu_count": facts.cpu_count,
            "mem_total_bytes": facts.mem_total_bytes,
        }),
        source: FactSource::Discovered,
        observed_at: Some(profile.collected_at),
        valid_until: None,
        evidence: None,
    }
}

/// Records the platform fact of `profile`. A failure is logged and never returned: the
/// profile itself is already saved and the next refresh tries again.
pub(crate) async fn observe(
    knowledge: &Arc<KnowledgeService>,
    host: HostId,
    profile: &HostProfile,
) {
    let actor = Actor::System(SYSTEM_PROFILE.to_owned());
    if let Err(err) = knowledge
        .record(platform_draft(host, profile), &actor)
        .await
    {
        tracing::warn!(%err, %host, "could not record the host platform fact");
    }
}

/// Whether the host already has a live `host_platform` fact. A read that fails counts as
/// "yes": the back-fill must not guess, and a later profile refresh records the fact anyway.
async fn has_platform_fact(knowledge: &Arc<KnowledgeService>, host: HostId) -> bool {
    let filter = FactFilter {
        subject: Some(SubjectRef::Host(host)),
        predicate: Some(Predicate::HostPlatform),
        ..FactFilter::default()
    };
    knowledge
        .facts(&filter, &Page::first(1))
        .await
        .map_or(true, |page| !page.items.is_empty())
}

/// Writes a `host_platform` fact for every host with a stored profile that has none yet.
/// Running it again changes nothing.
pub async fn backfill(deps: &ProfileDeps) {
    let Some(knowledge) = deps.knowledge.clone() else {
        return;
    };
    let hosts = match deps.hosts.list_hosts().await {
        Ok(hosts) => hosts,
        Err(err) => {
            tracing::warn!(%err, "could not list the hosts to back-fill facts for");
            return;
        }
    };
    for host in hosts {
        let profile = match stored(deps, host.id).await {
            Ok(Some(profile)) => profile,
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!(%err, "could not read a stored host profile");
                continue;
            }
        };
        if !has_platform_fact(&knowledge, host.id).await {
            observe(&knowledge, host.id, &profile).await;
        }
    }
}
