//! The containment service's handle: the owner's refresh and early release, the in-memory view of
//! each host's last refresh, and the policy gate that needs passed drills.
//!
//! Nothing here polls. A refresh is owner-initiated (`firewall.list_blocks` is a read through
//! `run_capability`, which needs a human authorization), and a release is a `rollback` phase
//! under the stored plan hash of the block.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use domovoi_core::action::AuthorizationKind;
use domovoi_core::containment::{ContainDispatch, Net};
use domovoi_core::ports::{BlockPhase, BlockRow, StoreError, Transaction};
use domovoi_core::{Actor, ExecutionId, HostId, PlanHash, StepId};
use jiff::SignedDuration;
use serde_json::json;

use super::input::expected_from_assurance;
use super::phases::{Answer, ContainEnd, send_raw};
use super::run::Run;
use super::status::HostContainment;
use super::{ContainDeps, MIN_WINDOW, audit};

/// Why a driver call did not do what the owner asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContainError {
    /// The host's agent has no live session.
    #[error("the host's agent is not connected")]
    NotConnected,
    /// No block has this id.
    #[error("there is no such block")]
    UnknownBlock,
    /// The block is in a phase that cannot be released.
    #[error("the block is {0} and cannot be released")]
    NotReleasable(&'static str),
    /// The agent or the helper refused, as `"<token>: <detail>"`.
    #[error("{0}")]
    Refused(String),
    /// The request was handed over and no usable answer came back.
    #[error("no answer arrived; the outcome is unknown")]
    Unknown,
    /// The store failed.
    #[error("the store failed: {0}")]
    Store(String),
}

impl From<StoreError> for ContainError {
    fn from(err: StoreError) -> Self {
        Self::Store(err.to_string())
    }
}

/// The containment service: built once and shared.
pub struct ContainmentDriver {
    deps: ContainDeps,
    status: Mutex<BTreeMap<HostId, HostContainment>>,
}

impl ContainmentDriver {
    /// A driver over `deps`, with no refresh seen yet.
    #[must_use]
    pub fn new(deps: ContainDeps) -> Arc<Self> {
        Arc::new(Self {
            deps,
            status: Mutex::new(BTreeMap::new()),
        })
    }

    /// What the driver is built from.
    #[must_use]
    pub fn deps(&self) -> &ContainDeps {
        &self.deps
    }

    /// What the helper said at the last refresh of `host`, if there was one since this start.
    #[must_use]
    pub fn status(&self, host: HostId) -> Option<HostContainment> {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&host)
            .cloned()
    }

    /// Whether the drills passed on `host` at the last refresh. Never true before a refresh: a
    /// policy run is refused until the owner has looked.
    #[must_use]
    pub fn drills_passed(&self, host: HostId) -> bool {
        self.status(host).is_some_and(|s| s.drills_passed())
    }

    /// The blocks stored for `host`, newest first.
    ///
    /// # Errors
    /// [`ContainError::Store`] when the read failed.
    pub async fn blocks(&self, host: HostId, limit: u32) -> Result<Vec<BlockRow>, ContainError> {
        Ok(self.deps.blocks.blocks_for_host(host, limit).await?)
    }

    /// The block `id`, if there is one.
    ///
    /// # Errors
    /// [`ContainError::Store`] when the read failed.
    pub async fn block(&self, id: ExecutionId) -> Result<Option<BlockRow>, ContainError> {
        Ok(self.deps.blocks.get_block(id).await?)
    }

    /// The owner's refresh: asks the helper for its blocks, gate and drills with a human
    /// authorization, keeps the answer in memory and records an audit row.
    ///
    /// # Errors
    /// [`ContainError::NotConnected`] with no live session, [`ContainError::Refused`] when the
    /// agent refused, [`ContainError::Unknown`] when no answer came.
    pub async fn refresh(
        &self,
        host: HostId,
        actor: Actor,
    ) -> Result<HostContainment, ContainError> {
        let (session, epoch) = self.live(host).ok_or(ContainError::NotConnected)?;
        let now = self.deps.clock.now();
        let expires_at = now.checked_add(MIN_WINDOW).unwrap_or(now);
        // A read binds nothing: its plan hash is a fixed placeholder the agent only journals.
        let plan: PlanHash = "00"
            .repeat(32)
            .parse()
            .map_err(|_| ContainError::Store("the read's plan placeholder is invalid".into()))?;
        let request =
            ContainDispatch::list(ExecutionId::from_step(StepId::new()), plan, expires_at);
        match send_raw(&self.deps, host, request, session, epoch).await {
            Answer::Done(outputs) => {
                let seen = HostContainment::from_outputs(self.deps.clock.now(), &outputs);
                self.status
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(host, seen.clone());
                let tx = Transaction::new().audit(audit::host_draft(
                    audit::LISTED,
                    host,
                    actor,
                    self.deps.clock.now(),
                    json!({ "gate": seen.gate, "drills": seen.drills, "active": seen.active }),
                ));
                if let Err(err) = self.deps.uow.commit(tx).await {
                    tracing::warn!(%err, "could not record a containment refresh");
                }
                Ok(seen)
            }
            Answer::Rejected(message) => Err(ContainError::Refused(message)),
            Answer::NotHandedOver => Err(ContainError::NotConnected),
            Answer::Unknown => Err(ContainError::Unknown),
        }
    }

    /// The owner's early release: sends `rollback` under the block's stored plan hash and
    /// assurance, with a fresh execution id. The step-up that authorizes it is the caller's.
    ///
    /// # Errors
    /// [`ContainError::UnknownBlock`], [`ContainError::NotReleasable`] for a block that does not
    /// hold, [`ContainError::NotConnected`] with no live session, [`ContainError::Refused`] or
    /// [`ContainError::Unknown`] from the answer.
    pub async fn release(&self, id: ExecutionId) -> Result<BlockRow, ContainError> {
        let row = self.block(id).await?.ok_or(ContainError::UnknownBlock)?;
        if !matches!(row.phase, BlockPhase::Applied | BlockPhase::Confirmed) {
            return Err(ContainError::NotReleasable(row.phase.as_str()));
        }
        let (session, epoch) = self.live(row.host_id).ok_or(ContainError::NotConnected)?;
        let (Ok(net), Ok(plan), Some(expected)) = (
            Net::parse(&row.source),
            row.plan_hash.parse::<PlanHash>(),
            expected_from_assurance(&row.assurance),
        ) else {
            return Err(ContainError::Store("the stored block is unreadable".into()));
        };
        let now = self.deps.clock.now();
        let mut run = Run {
            deps: &self.deps,
            host: row.host_id,
            net,
            ttl: SignedDuration::from_secs(i64::from(row.ttl_s)),
            plan,
            authorization: AuthorizationKind::Human,
            expires_at: now.checked_add(MIN_WINDOW).unwrap_or(now),
            expected,
            block: row,
        };
        match run.release(session, epoch).await {
            ContainEnd::Settled { .. } => Ok(run.block),
            ContainEnd::Unknown => match run.block.phase {
                BlockPhase::RollingBack => Err(ContainError::Unknown),
                _ => Err(ContainError::Refused(
                    run.block.failure_token.clone().unwrap_or_default(),
                )),
            },
        }
    }

    fn live(
        &self,
        host: HostId,
    ) -> Option<(domovoi_core::AgentSessionId, domovoi_core::JournalEpoch)> {
        let session = self.deps.caps.observer.observe_snapshot(host)?.session;
        let epoch = self.deps.caps.journal.epoch_of(host)?;
        Some((session, epoch))
    }
}
