//! The containment use cases: what a host's blocks look like, the owner's refresh, the approval
//! of a block (with the unverified variant) and its early release.
//!
//! Every command takes a [`Caller`] and is made on the web surface only. A block is approved by
//! the signed-in owner after their password, the source typed out and, when coverage cannot be
//! verified, an explicit acknowledgement. The approval is made by
//! [`crate::auth::approval::web_containment_authorization`] and handed to the operation service.
//! Nothing here sends a frame: the driver in the engine does, and no model, chat or background
//! path reaches it (ADR 0038).

use std::sync::Arc;

use domovoi_core::attest::{Authority, Cidr};
use domovoi_core::capability::containment::BLOCK_SOURCE_ID;
use domovoi_core::ports::{BlockRow, Clock, HostRepo, OperationRepo, StoreError};
use domovoi_core::{ExecutionId, HostId, OperationId, PlanBinding, PlanHash, StepId, StepKind};
use domovoi_engine::containment::input::{assess, parse_request};
use domovoi_engine::containment::{
    ContainError, ContainmentDriver, HostContainment, MIN_WINDOW, input::Assessment,
};
use domovoi_engine::operations::OperationError;

use super::operations::OperationCommandError;
use super::{Caller, OperationCommands, StepUp, Surface};
use crate::auth::approval::web_containment_authorization;
use crate::auth::{AuthError, AuthState};

/// What the containment use cases are built from.
pub struct ContainmentDeps {
    /// The driver; `None` when orchestration is off.
    pub driver: Option<Arc<ContainmentDriver>>,
    /// Stored operations, read only.
    pub repo: Arc<dyn OperationRepo>,
    /// Hosts, read only.
    pub hosts: Arc<dyn HostRepo>,
    /// Re-asks for the password.
    pub auth: Arc<AuthState>,
    /// Time.
    pub clock: Arc<dyn Clock>,
}

/// The containment use cases.
#[derive(Clone)]
pub struct ContainmentCommands {
    driver: Option<Arc<ContainmentDriver>>,
    operations: OperationCommands,
    repo: Arc<dyn OperationRepo>,
    hosts: Arc<dyn HostRepo>,
    auth: Arc<AuthState>,
    clock: Arc<dyn Clock>,
}

/// Why a containment command was refused. Nothing changed unless the variant says so.
#[derive(Debug, thiserror::Error)]
pub enum ContainmentCommandError {
    /// Orchestration is off, so there is no driver.
    #[error("containment is not set up on this hub")]
    Disabled,
    /// A block is approved, refreshed and released on the web only.
    #[error("containment is handled in the web UI only")]
    WebOnly,
    /// No such host, operation, step or block.
    #[error("not found")]
    NotFound,
    /// The step is not a containment step.
    #[error("the step is not a containment step")]
    NotContainment,
    /// The request carried no password.
    #[error("blocking a source needs your password")]
    PasswordRequired,
    /// The password is wrong.
    #[error("wrong password")]
    BadPassword,
    /// Too many wrong passwords from this address.
    #[error("too many attempts; try again in {retry_after_secs} s")]
    TooMany {
        /// Seconds until the next attempt.
        retry_after_secs: i64,
    },
    /// The typed text is not the canonical source.
    #[error("type the source to confirm")]
    TypedSourceMismatch,
    /// Coverage cannot be verified and the person did not acknowledge it.
    #[error("the helper could not verify coverage; acknowledge it to continue")]
    UnverifiedNotAcknowledged,
    /// The approval window is too short for a block's three phases.
    #[error("the approval window is too short to start a block")]
    ApprovalWindowTooShort,
    /// The host's agent is not connected.
    #[error("the host's agent is not connected")]
    NotConnected,
    /// The agent or the helper refused, as `"<token>: <detail>"`.
    #[error("{0}")]
    Refused(String),
    /// The block cannot be released in its phase.
    #[error("the block is {0} and cannot be released")]
    NotReleasable(&'static str),
    /// The request is malformed.
    #[error("{0}")]
    BadRequest(String),
    /// The step's state does not allow this.
    #[error("{0}")]
    Conflict(String),
    /// The operation service refused the approval.
    #[error(transparent)]
    Operation(#[from] OperationError),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// A request was handed over and no answer came.
    #[error("no answer arrived; the outcome is unknown")]
    Unknown,
}

impl From<ContainError> for ContainmentCommandError {
    fn from(err: ContainError) -> Self {
        match err {
            ContainError::NotConnected => Self::NotConnected,
            ContainError::UnknownBlock => Self::NotFound,
            ContainError::NotReleasable(phase) => Self::NotReleasable(phase),
            ContainError::Refused(message) => Self::Refused(message),
            ContainError::Unknown => Self::Unknown,
            ContainError::Store(text) => Self::Store(StoreError::Backend(text)),
        }
    }
}

/// How a containment step stands, for the step detail: the block when there is one, otherwise
/// what the approval would show now.
#[derive(Debug, Clone)]
pub struct StepView {
    /// The block row, once the step was dispatched.
    pub block: Option<BlockRow>,
    /// The assessment a person would be shown now, while the step awaits approval.
    pub preview: Option<Preview>,
}

/// What the approval card shows before a block exists.
#[derive(Debug, Clone)]
pub struct Preview {
    /// The canonical source.
    pub source: String,
    /// The TTL in seconds.
    pub ttl_s: i64,
    /// The reason coverage could not be verified, when it could not.
    pub unverified_reason: Option<&'static str>,
    /// The discovered paths the verified report does not cover.
    pub unprotected: Vec<String>,
}

/// What a person typed and ticked to approve a block.
#[derive(Debug, Clone, Copy)]
pub struct ApproveInput<'a> {
    /// The plan hash shown on the step.
    pub plan_hash: PlanHash,
    /// The source typed out.
    pub typed_source: Option<&'a str>,
    /// Whether the person acknowledged that coverage could not be verified.
    pub acknowledged: bool,
}

/// What a host's blocks look like now.
#[derive(Debug, Clone)]
pub struct HostView {
    /// Whether the driver exists.
    pub available: bool,
    /// What the helper said at the last refresh, if there was one since this start.
    pub status: Option<HostContainment>,
    /// The blocks the hub stored, newest first.
    pub blocks: Vec<BlockRow>,
}

fn cidr_text(cidr: &Cidr) -> String {
    format!("{}/{}", cidr.addr, cidr.prefix)
}

impl ContainmentCommands {
    /// The use cases over `deps`.
    #[must_use]
    pub fn new(deps: ContainmentDeps, operations: OperationCommands) -> Self {
        Self {
            driver: deps.driver,
            operations,
            repo: deps.repo,
            hosts: deps.hosts,
            auth: deps.auth,
            clock: deps.clock,
        }
    }

    fn driver(&self) -> Result<&Arc<ContainmentDriver>, ContainmentCommandError> {
        self.driver
            .as_ref()
            .ok_or(ContainmentCommandError::Disabled)
    }

    fn web(caller: &Caller) -> Result<&[u8; 32], ContainmentCommandError> {
        match caller.surface() {
            Surface::Web { session_hash } => Ok(session_hash),
            Surface::Chat { .. } => Err(ContainmentCommandError::WebOnly),
        }
    }

    /// Asks for the password again. It shares the login limiter.
    async fn step_up(
        &self,
        caller: &Caller,
        proof: StepUp<'_>,
    ) -> Result<(), ContainmentCommandError> {
        let Some(password) = proof.password else {
            return Err(ContainmentCommandError::PasswordRequired);
        };
        match self
            .auth
            .confirm_password(proof.ip, caller.user(), password)
            .await
        {
            Ok(()) => Ok(()),
            Err(AuthError::LockedOut { retry_after_secs }) => {
                Err(ContainmentCommandError::TooMany { retry_after_secs })
            }
            Err(_) => Err(ContainmentCommandError::BadPassword),
        }
    }

    /// The host named `name`, or why not.
    ///
    /// # Errors
    /// [`ContainmentCommandError::NotFound`] or a store failure.
    pub async fn host_id(&self, name: &str) -> Result<HostId, ContainmentCommandError> {
        let name =
            domovoi_core::HostName::parse(name).map_err(|_| ContainmentCommandError::NotFound)?;
        self.hosts
            .get_host_by_name(&name)
            .await?
            .map(|host| host.id)
            .ok_or(ContainmentCommandError::NotFound)
    }

    /// What `host`'s blocks look like now. Works when containment is unavailable: the answer then
    /// says so and holds nothing.
    ///
    /// # Errors
    /// A store failure.
    pub async fn host_view(&self, host: HostId) -> Result<HostView, ContainmentCommandError> {
        let Some(driver) = &self.driver else {
            return Ok(HostView {
                available: false,
                status: None,
                blocks: Vec::new(),
            });
        };
        Ok(HostView {
            available: true,
            status: driver.status(host),
            blocks: driver.blocks(host, 50).await?,
        })
    }

    /// The owner's refresh of a host's block list, gate and drills.
    ///
    /// # Errors
    /// Web only; [`ContainmentCommandError::NotConnected`] with no live session, or the helper's
    /// refusal.
    pub async fn refresh(
        &self,
        caller: &Caller,
        host: HostId,
    ) -> Result<HostView, ContainmentCommandError> {
        Self::web(caller)?;
        let driver = self.driver()?;
        driver.refresh(host, caller.actor()).await?;
        self.host_view(host).await
    }

    /// How a containment step stands now.
    ///
    /// # Errors
    /// [`ContainmentCommandError::NotFound`], [`ContainmentCommandError::NotContainment`] or a
    /// store failure.
    pub async fn step_view(
        &self,
        op: OperationId,
        step: StepId,
    ) -> Result<StepView, ContainmentCommandError> {
        let driver = self.driver()?;
        let row = self.execution(op, step).await?;
        if let Some(block) = driver.block(ExecutionId::from_step(step)).await? {
            return Ok(StepView {
                block: Some(block),
                preview: None,
            });
        }
        let Ok((net, ttl)) = parse_request(&row.args) else {
            return Ok(StepView {
                block: None,
                preview: None,
            });
        };
        let deps = driver.deps();
        let assessed = assess(
            &*deps.trust,
            &*deps.paths,
            row.host,
            Authority::Human,
            self.clock.now(),
        )
        .await
        .ok();
        Ok(StepView {
            block: None,
            preview: Some(Preview {
                source: net.canonical(),
                ttl_s: ttl.as_secs(),
                unverified_reason: assessed
                    .as_ref()
                    .and_then(Assessment::reason)
                    .map(|r| r.token()),
                unprotected: assessed
                    .map(|a| a.unprotected.iter().map(cidr_text).collect())
                    .unwrap_or_default(),
            }),
        })
    }

    async fn execution(
        &self,
        op: OperationId,
        step: StepId,
    ) -> Result<domovoi_core::ExecutionRecord, ContainmentCommandError> {
        let snapshot = self
            .repo
            .load(op)
            .await?
            .ok_or(ContainmentCommandError::NotFound)?;
        snapshot
            .step(step)
            .filter(|record| record.step.kind == StepKind::Execution)
            .ok_or(ContainmentCommandError::NotFound)?;
        let row = snapshot
            .execution_of(step)
            .map(|stored| stored.value.clone())
            .ok_or(ContainmentCommandError::NotFound)?;
        if row.contract.as_str() != BLOCK_SOURCE_ID {
            return Err(ContainmentCommandError::NotContainment);
        }
        Ok(row)
    }

    /// Approves a containment step: the person's password, the source typed out and, when the
    /// helper's coverage cannot be verified, an explicit acknowledgement.
    ///
    /// # Errors
    /// See [`ContainmentCommandError`]; nothing is stored unless the call returns `Ok`.
    pub async fn approve(
        &self,
        caller: &Caller,
        op: OperationId,
        step: StepId,
        input: ApproveInput<'_>,
        proof: StepUp<'_>,
    ) -> Result<(), ContainmentCommandError> {
        let session_hash = Self::web(caller)?;
        let driver = self.driver()?;
        let row = self.execution(op, step).await?;
        let (net, _ttl) = parse_request(&row.args).map_err(ContainmentCommandError::BadRequest)?;
        let now = self.clock.now();
        if row.expires_at.duration_since(now) < MIN_WINDOW {
            return Err(ContainmentCommandError::ApprovalWindowTooShort);
        }
        if input.typed_source != Some(net.canonical().as_str()) {
            return Err(ContainmentCommandError::TypedSourceMismatch);
        }
        let deps = driver.deps();
        let assessed = assess(&*deps.trust, &*deps.paths, row.host, Authority::Human, now)
            .await
            .map_err(|err| ContainmentCommandError::Conflict(err.to_string()))?;
        if !assessed.is_verified() && !input.acknowledged {
            return Err(ContainmentCommandError::UnverifiedNotAcknowledged);
        }
        self.step_up(caller, proof).await?;
        let (authorization, provenance) =
            web_containment_authorization(caller.user(), session_hash, now);
        self.operations
            .authorize_prepared(
                op,
                step,
                authorization,
                PlanBinding::Full(input.plan_hash),
                provenance,
            )
            .await
            .map_err(|err| match err {
                OperationCommandError::Operation(err) => ContainmentCommandError::Operation(err),
                OperationCommandError::NotEnabled => ContainmentCommandError::Disabled,
                OperationCommandError::Invalid(text) => ContainmentCommandError::BadRequest(text),
                OperationCommandError::Conflict(text) => ContainmentCommandError::Conflict(text),
                OperationCommandError::NotFound => ContainmentCommandError::NotFound,
                OperationCommandError::TypedConfirmationRequired
                | OperationCommandError::WebOnly => ContainmentCommandError::WebOnly,
            })
    }

    /// The owner's early release of a block that holds: password, the source typed out, then a
    /// `rollback` under the block's stored plan hash.
    ///
    /// # Errors
    /// See [`ContainmentCommandError`].
    pub async fn release(
        &self,
        caller: &Caller,
        op: OperationId,
        step: StepId,
        typed_source: Option<&str>,
        proof: StepUp<'_>,
    ) -> Result<BlockRow, ContainmentCommandError> {
        Self::web(caller)?;
        let driver = self.driver()?;
        self.execution(op, step).await?;
        let block = driver
            .block(ExecutionId::from_step(step))
            .await?
            .ok_or(ContainmentCommandError::NotFound)?;
        if typed_source != Some(block.source.as_str()) {
            return Err(ContainmentCommandError::TypedSourceMismatch);
        }
        self.step_up(caller, proof).await?;
        Ok(driver.release(block.block_id).await?)
    }
}
