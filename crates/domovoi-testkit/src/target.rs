//! A fake target resolver, so the operation service can bind plans without a hub.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use domovoi_core::capability::{
    ResolvedTarget, TargetError, TargetKind, TargetName, TargetQuery, TargetUnresolved,
    canonical_host_identity,
};
use domovoi_core::ports::TargetResolver;
use domovoi_core::{HostId, KeyFingerprint};

use crate::lock;

/// A resolver that answers `TargetQuery::Host` with the fixed key every testkit builder
/// enrolls, unless a test scripts another key or a failure for a host.
pub struct FakeTargetResolver {
    keys: Mutex<BTreeMap<HostId, Option<KeyFingerprint>>>,
}

impl FakeTargetResolver {
    /// A resolver that knows every host under the testkit's enrolled key.
    #[must_use]
    pub fn new() -> Self {
        Self {
            keys: Mutex::new(BTreeMap::new()),
        }
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
    async fn resolve(&self, query: TargetQuery) -> Result<ResolvedTarget, TargetError> {
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
