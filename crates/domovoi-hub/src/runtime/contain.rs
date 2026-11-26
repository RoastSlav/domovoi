//! The containment driver, built once at start-up over the one link.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::HostId;
use domovoi_core::attest::Cidr;
use domovoi_core::ports::{Clock, ContainCaps, HelperTrustRead, TargetResolver};
use domovoi_engine::Evidence;
use domovoi_engine::containment::{ContainDeps, ContainmentDriver, PathSource};
use domovoi_engine::knowledge::KnowledgeService;
use domovoi_engine::targets::TargetService;
use domovoi_store::Store;
use jiff::SignedDuration;

/// Extra time beyond a phase's contract timeout to wait for an agent's answer.
const RESULT_GRACE: SignedDuration = SignedDuration::from_secs(30);

/// The host's management paths as the knowledge store holds them: the candidates a verified
/// report must cover before a block counts as verified.
struct KnowledgePaths(Arc<KnowledgeService>);

#[async_trait]
impl PathSource for KnowledgePaths {
    async fn discovered(&self, host: HostId) -> Result<Vec<Cidr>, String> {
        let paths = self
            .0
            .management_paths(host)
            .await
            .map_err(|err| err.to_string())?;
        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let Some(text) = path
                .fact
                .value
                .value
                .as_ref()
                .and_then(|value| value.get("cidr"))
                .and_then(|cidr| cidr.as_str())
            else {
                return Err("a management path has no readable range".to_owned());
            };
            out.push(Cidr::parse_config(text).map_err(|err| err.to_string())?);
        }
        Ok(out)
    }
}

/// What the driver is built from: values the runtime already holds.
pub(super) struct ContainBuild<'a> {
    pub(super) store: &'a Arc<Store>,
    pub(super) caps: ContainCaps,
    pub(super) trust: Arc<dyn HelperTrustRead>,
    pub(super) knowledge: Arc<KnowledgeService>,
    pub(super) evidence: &'a Evidence,
    pub(super) units: domovoi_core::ports::UnitCaps,
    pub(super) clock: &'a Arc<dyn Clock>,
}

/// The containment driver. It runs whether or not orchestration is on, so a block left in flight
/// is reconciled and the owner can always read and refresh a host's blocks.
pub(super) fn build(input: ContainBuild<'_>) -> Arc<ContainmentDriver> {
    let ContainBuild {
        store,
        caps,
        trust,
        knowledge,
        evidence,
        units,
        clock,
    } = input;
    let resolver: Arc<dyn TargetResolver> =
        Arc::new(TargetService::new(Arc::new(evidence.clone()), store.clone()).with_units(units));
    ContainmentDriver::new(ContainDeps {
        caps,
        uow: store.clone(),
        blocks: store.clone(),
        trust,
        paths: Arc::new(KnowledgePaths(knowledge)),
        resolver,
        clock: clock.clone(),
        result_grace: RESULT_GRACE,
    })
}

/// The containment use cases' inputs, over the driver the runtime built.
pub(super) fn services(
    driver: &Arc<ContainmentDriver>,
    store: &Arc<Store>,
    auth: &Arc<crate::auth::AuthState>,
    clock: &Arc<dyn Clock>,
) -> crate::services::ContainmentDeps {
    crate::services::ContainmentDeps {
        driver: Some(driver.clone()),
        repo: store.clone(),
        hosts: store.clone(),
        auth: auth.clone(),
        clock: clock.clone(),
    }
}
