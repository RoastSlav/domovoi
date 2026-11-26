//! Resolving a target name to the canonical identity a plan binds.
//!
//! A plan hash binds the identity, so a resolution must be exact and never optimistic: a host
//! identity carries the fingerprint of its enrolled agent key, a container identity carries the
//! full container id, and a unit has no canonical identity at all.
//!
//! Every read goes through the one read boundary, [`Evidence::collect_evidence`], because the
//! agent's `docker_ps` and `docker_inspect` are reads like any other: this module never reaches
//! the link itself. Nothing here writes, approves or dispatches.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::capability::{
    Availability, ResolvedTarget, TargetError, TargetKind, TargetName, TargetQuery,
    TargetUnresolved, canonical_container_identity, canonical_host_identity,
    canonical_network_identity, parse_inspect_id, parse_ps, validate_container_name,
};
use domovoi_core::host::KeyFingerprint;
use domovoi_core::ports::{HostAvailability, HostRepo, TargetResolver};
use domovoi_core::ports::{LinkError, UnitCaps};
use domovoi_core::unit::{
    ResolvedUnit, UnitName, UnitRefusal, canonical_unit_identity, card_of, id_and_hash,
};
use domovoi_core::{HostId, ReadToolCall};
use jiff::SignedDuration;

use crate::evidence::{Evidence, EvidenceRequest};

/// How long one resolution's reads may take: one `docker_ps` plus one `docker_inspect`.
const RESOLVE_TIMEOUT: SignedDuration = SignedDuration::from_secs(30);

/// The [`TargetResolver`] port over the hub's enrolled records and the one read boundary.
pub struct TargetService {
    evidence: Arc<Evidence>,
    hosts: Arc<dyn HostRepo>,
    units: Option<UnitCaps>,
}

impl TargetService {
    /// A resolver that reads through `evidence` and takes the enrolled key from `hosts`.
    #[must_use]
    pub fn new(evidence: Arc<Evidence>, hosts: Arc<dyn HostRepo>) -> Self {
        Self {
            evidence,
            hosts,
            units: None,
        }
    }

    /// The same resolver, able to resolve a systemd unit through the host's helper. Without
    /// it a unit never resolves.
    #[must_use]
    pub fn with_units(mut self, units: UnitCaps) -> Self {
        self.units = Some(units);
        self
    }

    /// The enrolled key fingerprint of `host`: the only source of a canonical identity.
    ///
    /// A host that was never enrolled, or whose agent was revoked, has no identity to bind, so
    /// nothing is bound rather than a placeholder being invented.
    async fn host_key(&self, query: &TargetQuery) -> Result<KeyFingerprint, TargetError> {
        let host = query.host();
        let agent = self
            .hosts
            .live_agent(host)
            .await
            .map_err(|_| unresolved(query, TargetUnresolved::HostIdentityUnproven))?;
        let Some(agent) = agent else {
            return Err(unresolved(query, TargetUnresolved::HostIdentityUnproven));
        };
        Ok(KeyFingerprint::of_public_key(&agent.public_key))
    }

    /// Whether `host` can be read at all, as the exact availability state.
    async fn reachable(&self, query: &TargetQuery) -> Result<(), TargetError> {
        let host = query.host();
        match self.evidence.availability(host).await {
            HostAvailability::Known { .. } => Ok(()),
            HostAvailability::Unknown => Err(TargetError::HostUnavailable {
                host,
                state: Availability::HostUnknown,
            }),
            HostAvailability::Absent { since } => Err(TargetError::HostUnavailable {
                host,
                state: Availability::HostAbsent { since },
            }),
            HostAvailability::Incompatible { reported } => Err(TargetError::HostUnavailable {
                host,
                state: Availability::HostIncompatible { reported },
            }),
        }
    }

    /// One read through the boundary, or "the target does not resolve now".
    async fn read(
        &self,
        query: &TargetQuery,
        call: ReadToolCall,
    ) -> Result<domovoi_core::ports::ToolOutput, TargetError> {
        let deadline = self
            .evidence
            .now()
            .saturating_add(RESOLVE_TIMEOUT)
            .unwrap_or_else(|_| self.evidence.now());
        self.evidence
            .collect_evidence(EvidenceRequest::for_host(query.host(), call, deadline))
            .await
            // A read that failed proves nothing about the target, so the least-claiming
            // answer is that its identity could not be established; `ContainerVanished`
            // is reserved for a list that arrived and did not name it.
            .map_err(|_| unresolved(query, TargetUnresolved::ContainerIdentityUnproven))
    }
}

/// A target that does not resolve now, carrying the query it was asked about.
fn unresolved(query: &TargetQuery, reason: TargetUnresolved) -> TargetError {
    TargetError::Unresolved {
        query: query.clone(),
        reason,
    }
}

#[async_trait]
impl TargetResolver for TargetService {
    async fn resolve(&self, query: TargetQuery) -> Result<ResolvedTarget, TargetError> {
        match &query {
            TargetQuery::Host { host } => {
                let key = self.host_key(&query).await?;
                let identity = canonical_host_identity(*host, &key)
                    .map_err(|_| unresolved(&query, TargetUnresolved::HostIdentityUnproven))?;
                let name = match self.hosts.get_host(*host).await {
                    Ok(Some(host)) => TargetName::parse(host.name.as_str()),
                    _ => TargetName::parse(&host.to_string()),
                }
                .map_err(|reason| TargetError::InvalidName {
                    query: query.clone(),
                    reason,
                })?;
                Ok(ResolvedTarget {
                    identity,
                    name,
                    kind: TargetKind::Host,
                    host_key: key,
                })
            }
            TargetQuery::Container { host, name } => {
                let typed = validate_container_name(name.as_str()).map_err(|reason| {
                    TargetError::InvalidName {
                        query: query.clone(),
                        reason,
                    }
                })?;
                self.reachable(&query).await?;
                let key = self.host_key(&query).await?;
                let listed = self
                    .read(&query, ReadToolCall::DockerPs { all: true })
                    .await?;
                let names = parse_ps(&listed.text)
                    .map_err(|_| unresolved(&query, TargetUnresolved::ContainerVanished))?;
                let mut matches = names.iter().filter(|listed| **listed == typed);
                let Some(_only) = matches.next() else {
                    return Err(unresolved(&query, TargetUnresolved::ContainerVanished));
                };
                let count = 1 + matches.count();
                if count > 1 {
                    return Err(TargetError::Ambiguous {
                        query: query.clone(),
                        count,
                    });
                }
                let inspect = self
                    .read(
                        &query,
                        ReadToolCall::DockerInspect {
                            container: typed.as_str().to_owned(),
                        },
                    )
                    .await?;
                let id = parse_inspect_id(&inspect.text)
                    .map_err(|_| unresolved(&query, TargetUnresolved::ContainerIdentityUnproven))?;
                let identity = canonical_container_identity(*host, &key, &id)
                    .map_err(|_| unresolved(&query, TargetUnresolved::ContainerIdentityUnproven))?;
                Ok(ResolvedTarget {
                    identity,
                    name: name.clone(),
                    kind: TargetKind::Container,
                    host_key: key,
                })
            }
            // A unit resolves only through the host's helper, which reads its loaded
            // configuration and files; without one it has no provable identity.
            TargetQuery::Unit { host, name } => {
                self.resolve_unit(*host, name).await.map(|unit| unit.target)
            }
            // A network source needs no read: its identity is the canonical network plus
            // the enrolled key of the host that would hold the block.
            TargetQuery::NetworkSource { host, source } => {
                let key = self.host_key(&query).await?;
                let identity = canonical_network_identity(*host, &key, source)
                    .map_err(|_| unresolved(&query, TargetUnresolved::HostIdentityUnproven))?;
                let name = TargetName::parse(&source.canonical()).map_err(|reason| {
                    TargetError::InvalidName {
                        query: query.clone(),
                        reason,
                    }
                })?;
                Ok(ResolvedTarget {
                    identity,
                    name,
                    kind: TargetKind::NetworkSource,
                    host_key: key,
                })
            }
        }
    }

    async fn resolve_unit(
        &self,
        host: HostId,
        name: &TargetName,
    ) -> Result<ResolvedUnit, TargetError> {
        let query = TargetQuery::Unit {
            host,
            name: name.clone(),
        };
        let Some(units) = &self.units else {
            return Err(unresolved(&query, TargetUnresolved::UnitIdentityUnproven));
        };
        let typed = UnitName::parse(name.as_str()).map_err(|reason| TargetError::InvalidName {
            query: query.clone(),
            reason,
        })?;
        self.reachable(&query).await?;
        let key = self.host_key(&query).await?;
        let Some(snapshot) = units.observer.observe_snapshot(host) else {
            return Err(unresolved(&query, TargetUnresolved::UnitIdentityUnproven));
        };
        let deadline = self
            .evidence
            .now()
            .saturating_add(RESOLVE_TIMEOUT)
            .unwrap_or_else(|_| self.evidence.now());
        let refused = |refusal| TargetError::UnitRefused {
            query: query.clone(),
            refusal,
        };
        let outputs = units
            .relay
            .read_identity(host, &typed, snapshot.session, deadline)
            .await
            .map_err(|error| match error {
                // The agent relays the helper's refusal as `<token>: <detail>`.
                LinkError::Rejected(text) => UnitRefusal::from_message(&text).map_or_else(
                    || unresolved(&query, TargetUnresolved::UnitIdentityUnproven),
                    refused,
                ),
                _ => unresolved(&query, TargetUnresolved::UnitIdentityUnproven),
            })?;
        let (id, identity_hash) = id_and_hash(&outputs).map_err(refused)?;
        let card = card_of(&outputs).map_err(refused)?;
        let identity = canonical_unit_identity(host, &key, &id)
            .map_err(|_| unresolved(&query, TargetUnresolved::UnitIdentityUnproven))?;
        Ok(ResolvedUnit {
            target: ResolvedTarget {
                identity,
                name: name.clone(),
                kind: TargetKind::Unit,
                host_key: key,
            },
            id,
            identity_hash,
            card,
        })
    }
}

/// The host a query reads, for the callers that hold only the query.
#[must_use]
pub fn query_host(query: &TargetQuery) -> HostId {
    query.host()
}
