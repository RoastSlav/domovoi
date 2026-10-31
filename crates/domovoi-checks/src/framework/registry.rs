//! Building the set of enabled checks.

use std::collections::BTreeMap;
use std::sync::Arc;

use domovoi_core::{CheckKey, ValidationError};

use super::{Check, CheckSettings};
use crate::system::{
    CpuTemperature, DiskFree, FsStats, KernelErrors, MemoryAvailable, PowerSupply, Reboot,
    RebootRequired,
};

/// Host services some checks need beyond the [`CheckContext`](super::CheckContext).
#[derive(Clone)]
pub struct HostDeps {
    /// Filesystem usage for `disk.free`.
    pub fs_stats: Arc<dyn FsStats>,
}

/// The enabled checks, in key order.
pub struct Registry {
    checks: Vec<Box<dyn Check>>,
}

impl Registry {
    /// Every built-in check except those whose settings disable them. Checks without
    /// settings are enabled.
    pub fn build(
        settings: &BTreeMap<CheckKey, CheckSettings>,
        deps: HostDeps,
    ) -> Result<Self, ValidationError> {
        let all: Vec<Box<dyn Check>> = vec![
            Box::new(DiskFree::new(deps.fs_stats)?),
            Box::new(KernelErrors::new()?),
            Box::new(MemoryAvailable::new()?),
            Box::new(PowerSupply::new()?),
            Box::new(Reboot::new()?),
            Box::new(RebootRequired::new()?),
            Box::new(CpuTemperature::new()?),
        ];
        let mut checks: Vec<Box<dyn Check>> = all
            .into_iter()
            .filter(|c| settings.get(&c.key()).is_none_or(|s| s.enabled))
            .collect();
        checks.sort_by_key(|c| c.key());
        Ok(Self { checks })
    }

    /// The checks, in key order.
    pub fn checks(&self) -> impl Iterator<Item = &dyn Check> {
        self.checks.iter().map(AsRef::as_ref)
    }

    /// The check with `key`, if it is enabled.
    #[must_use]
    pub fn get(&self, key: &CheckKey) -> Option<&dyn Check> {
        self.checks().find(|c| &c.key() == key)
    }
}
