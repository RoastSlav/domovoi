//! A fake target resolver, so the operation service can bind plans without a hub.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::capability::{
    ResolvedTarget, TargetError, TargetKind, TargetName, TargetQuery, TargetUnresolved,
    canonical_host_identity, canonical_network_identity,
};
use domovoi_core::ports::TargetResolver;
use domovoi_core::unit::{
    ResolvedUnit, UnitRefusal, canonical_unit_identity, card_of, id_and_hash,
};
use domovoi_core::{HostId, KeyFingerprint};

use crate::lock;

/// A resolver that answers `TargetQuery::Host` with the fixed key every testkit builder
/// enrolls, unless a test scripts another key or a failure for a host.
pub struct FakeTargetResolver {
    keys: Mutex<BTreeMap<HostId, Option<KeyFingerprint>>>,
    units: Mutex<BTreeMap<(HostId, String), Result<ResolvedUnit, UnitRefusal>>>,
}

impl FakeTargetResolver {
    /// A resolver that knows every host under the testkit's enrolled key.
    #[must_use]
    pub fn new() -> Self {
        Self {
            keys: Mutex::new(BTreeMap::new()),
            units: Mutex::new(BTreeMap::new()),
        }
    }

    /// Makes the unit typed as `typed` resolve to the canonical `id` with an identity hash of 64
    /// `hash_char` characters, on a platform of `tier`. Calling it again with another hash is a
    /// configuration change of that unit.
    pub fn set_unit(&self, host: HostId, typed: &str, id: &str, hash_char: char, tier: &str) {
        let key = match lock(&self.keys).get(&host) {
            Some(Some(key)) => *key,
            _ => crate::build::key_fingerprint(),
        };
        let outputs = crate::unit::identity_outputs(id, hash_char, tier, true);
        let resolved = (|| {
            let (unit_id, identity_hash) = id_and_hash(&outputs).ok()?;
            let identity = canonical_unit_identity(host, &key, &unit_id).ok()?;
            Some(ResolvedUnit {
                target: ResolvedTarget {
                    identity,
                    name: TargetName::parse(typed).ok()?,
                    kind: TargetKind::Unit,
                    host_key: key,
                },
                id: unit_id,
                identity_hash,
                card: card_of(&outputs).ok()?,
            })
        })();
        if let Some(resolved) = resolved {
            lock(&self.units).insert((host, typed.to_owned()), Ok(resolved));
        }
    }

    /// Makes the unit typed as `typed` be refused by the helper with `refusal`.
    pub fn refuse_unit(&self, host: HostId, typed: &str, refusal: UnitRefusal) {
        lock(&self.units).insert((host, typed.to_owned()), Err(refusal));
    }

    /// Makes `host` resolve under `key`, as a re-enrolment would.
    pub fn set_key(&self, host: HostId, key: KeyFingerprint) {
        lock(&self.keys).insert(host, Some(key));
    }

    /// Makes `host` unresolvable, as a host with no usable enrolled key is.
    pub fn fail(&self, host: HostId) {
        lock(&self.keys).insert(host, None);
    }
}

impl Default for FakeTargetResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn unresolved(query: TargetQuery, reason: TargetUnresolved) -> TargetError {
    TargetError::Unresolved { query, reason }
}

#[async_trait]
impl TargetResolver for FakeTargetResolver {
    async fn resolve_unit(
        &self,
        host: HostId,
        name: &TargetName,
    ) -> Result<ResolvedUnit, TargetError> {
        let query = TargetQuery::Unit {
            host,
            name: name.clone(),
        };
        match lock(&self.units).get(&(host, name.as_str().to_owned())) {
            Some(Ok(unit)) => Ok(unit.clone()),
            Some(Err(refusal)) => Err(TargetError::UnitRefused {
                query,
                refusal: refusal.clone(),
            }),
            None => Err(unresolved(query, TargetUnresolved::UnitIdentityUnproven)),
        }
    }

    async fn resolve(&self, query: TargetQuery) -> Result<ResolvedTarget, TargetError> {
        if let TargetQuery::NetworkSource { host, source } = &query {
            let key = match lock(&self.keys).get(host) {
                Some(Some(key)) => *key,
                Some(None) => {
                    return Err(unresolved(query, TargetUnresolved::HostIdentityUnproven));
                }
                None => crate::build::key_fingerprint(),
            };
            let identity = canonical_network_identity(*host, &key, source)
                .map_err(|_| unresolved(query.clone(), TargetUnresolved::HostIdentityUnproven))?;
            let name = TargetName::parse(&source.canonical()).map_err(|reason| {
                TargetError::InvalidName {
                    query: query.clone(),
                    reason,
                }
            })?;
            return Ok(ResolvedTarget {
                identity,
                name,
                kind: TargetKind::NetworkSource,
                host_key: key,
            });
        }
        let TargetQuery::Host { host } = &query else {
            return Err(unresolved(query, TargetUnresolved::ContainerVanished));
        };
        let host = *host;
        let key = match lock(&self.keys).get(&host) {
            Some(Some(key)) => *key,
            Some(None) => return Err(unresolved(query, TargetUnresolved::HostIdentityUnproven)),
            None => crate::build::key_fingerprint(),
        };
        let identity = canonical_host_identity(host, &key)
            .map_err(|_| unresolved(query.clone(), TargetUnresolved::HostIdentityUnproven))?;
        let name = TargetName::parse(&host.to_string())
            .map_err(|reason| TargetError::InvalidName { query, reason })?;
        Ok(ResolvedTarget {
            identity,
            name,
            kind: TargetKind::Host,
            host_key: key,
        })
    }
}
