//! The state of one block while it runs: committing each phase change before the request that
//! causes it, sending one phase, and the failure paths.

use domovoi_core::action::AuthorizationKind;
use domovoi_core::attest::ExpectedSafety;
use domovoi_core::containment::{BlockSpec, ContainCall, ContainOutputs, Net, Token, keys};
use domovoi_core::ports::{
    BlockPhase, BlockRow, CommitError, ContainmentWrite, PhaseGuard, PhaseUpdate, StateWrite,
    Transaction, phase_journal,
};
use domovoi_core::{
    Actor, AgentSessionId, ExecutionId, HostId, JournalEpoch, PlanHash, StepId, StepOutcome,
};
use jiff::{SignedDuration, Timestamp};

use super::phases::{
    Answer, ContainEnd, UNRECOGNISED_MAX, is_refusal, send_raw, settled, token_of, token_text,
};
use super::{CONFIRM_WITHIN, ContainDeps, STEP_ONE_SECS, audit};

/// The mutable state of one run.
pub(super) struct Run<'a> {
    pub(super) deps: &'a ContainDeps,
    pub(super) host: HostId,
    pub(super) block: BlockRow,
    pub(super) net: Net,
    pub(super) ttl: SignedDuration,
    pub(super) plan: PlanHash,
    pub(super) authorization: AuthorizationKind,
    pub(super) expires_at: Timestamp,
    pub(super) expected: ExpectedSafety,
}

/// Commits `block` moving to `update.phase`, with its journal row and one audit row, guarded by
/// the phase and version the caller read. Returns the row as it now stands.
pub(super) async fn commit_phase(
    deps: &ContainDeps,
    block: &BlockRow,
    update: PhaseUpdate,
    kind: &str,
    actor: Actor,
) -> Result<BlockRow, CommitError> {
    let from = block.phase;
    let mut next = block.clone();
    next.phase = update.phase;
    next.version += 1;
    next.updated_at = update.at;
    next.confirm_execution = update.confirm_execution.or(next.confirm_execution);
    next.rollback_execution = update.rollback_execution.or(next.rollback_execution);
    next.applied_at = update.applied_at.or(next.applied_at);
    next.deadline = update.deadline.or(next.deadline);
    next.existing_flows = update.existing_flows.clone().or(next.existing_flows);
    next.bridge_gap = update.bridge_gap.or(next.bridge_gap);
    next.failure_token = update.failure_token.clone().or(next.failure_token);
    next.failure_message = update.failure_message.clone().or(next.failure_message);
    let token = update.failure_token.clone();
    let tx = Transaction::new()
        .write(StateWrite::Containment(ContainmentWrite::Phase {
            block: block.block_id,
            expected: PhaseGuard {
                phase: from,
                version: block.version,
            },
            update: update.clone(),
        }))
        .journal(phase_journal(
            &next,
            from,
            update.phase,
            token.as_deref(),
            actor.clone(),
            update.at,
        ))
        .audit(audit::draft(
            kind,
            &next,
            actor,
            update.at,
            token.as_deref(),
        ));
    deps.uow.commit(tx).await?;
    Ok(next)
}

impl Run<'_> {
    pub(super) fn now(&self) -> Timestamp {
        self.deps.clock.now()
    }

    fn spec(&self) -> BlockSpec {
        BlockSpec {
            block: self.block.block_id,
            net: self.net,
            ttl: self.ttl,
        }
    }

    /// The session and epoch of the host's live session now, or `None` with none.
    pub(super) fn live(&self) -> Option<(AgentSessionId, JournalEpoch)> {
        let session = self.deps.caps.observer.observe_snapshot(self.host)?.session;
        let epoch = self.deps.caps.journal.epoch_of(self.host)?;
        Some((session, epoch))
    }

    /// Commits the block moving to `update.phase`; only on success does the local row change.
    pub(super) async fn move_to(
        &mut self,
        update: PhaseUpdate,
        kind: &str,
        actor: Actor,
    ) -> Result<(), CommitError> {
        self.block = commit_phase(self.deps, &self.block, update, kind, actor).await?;
        Ok(())
    }

    /// Sends one phase and waits for its answer. The phase was committed before this call.
    async fn send(
        &self,
        call: ContainCall,
        execution: ExecutionId,
        session: AgentSessionId,
        epoch: JournalEpoch,
    ) -> Answer {
        let Ok(request) = domovoi_core::containment::ContainDispatch::phase(
            execution,
            call,
            self.plan,
            self.authorization,
            self.expires_at,
            self.expected.clone(),
        ) else {
            return Answer::NotHandedOver;
        };
        send_raw(self.deps, self.host, request, session, epoch).await
    }

    fn update(&self, phase: BlockPhase) -> PhaseUpdate {
        PhaseUpdate::to(phase, self.now())
    }

    /// Settles the block in a final phase with a token, and returns the step's ending.
    async fn finish(
        &mut self,
        phase: BlockPhase,
        token: Option<&str>,
        message: Option<String>,
        end: ContainEnd,
    ) -> ContainEnd {
        let mut update = self.update(phase);
        update.failure_token = token.map(str::to_owned);
        update.failure_message = message.map(|m| m.chars().take(UNRECOGNISED_MAX).collect());
        match self
            .move_to(update, audit::kind_of(phase), audit::system())
            .await
        {
            Ok(()) => end,
            Err(err) => {
                tracing::warn!(block = %self.block.block_id, %err, "could not settle a containment block");
                ContainEnd::Unknown
            }
        }
    }

    /// The failure path after `apply` succeeded and the confirmation did not: roll back when the
    /// session is alive, otherwise leave the block to its 30 second timer.
    async fn abandon(&mut self, why: String) -> ContainEnd {
        let Some((session, epoch)) = self.live() else {
            return self
                .finish(
                    BlockPhase::Expired,
                    None,
                    Some(why.clone()),
                    settled(
                        StepOutcome::Failed,
                        Some(format!(
                            "{why}; the block expires on its own in at most {STEP_ONE_SECS} seconds"
                        )),
                        None,
                    ),
                )
                .await;
        };
        let rollback = ExecutionId::from_step(StepId::new());
        let mut update = self.update(BlockPhase::RollingBack);
        update.rollback_execution = Some(rollback);
        if self
            .move_to(update, audit::ROLLED_BACK, audit::system())
            .await
            .is_err()
        {
            return ContainEnd::Unknown;
        }
        match self
            .send(ContainCall::Rollback(self.spec()), rollback, session, epoch)
            .await
        {
            Answer::Done(_) => {
                self.finish(
                    BlockPhase::RolledBack,
                    None,
                    Some(why.clone()),
                    settled(
                        StepOutcome::Failed,
                        Some(format!("{why}; the block was removed")),
                        None,
                    ),
                )
                .await
            }
            Answer::Rejected(message) => self.after_rollback_rejected(&why, &message).await,
            Answer::NotHandedOver => {
                self.finish(
                    BlockPhase::Expired,
                    None,
                    Some(why.clone()),
                    settled(
                        StepOutcome::Failed,
                        Some(format!(
                            "{why}; the block expires on its own in at most {STEP_ONE_SECS} seconds"
                        )),
                        None,
                    ),
                )
                .await
            }
            Answer::Unknown => ContainEnd::Unknown,
        }
    }

    async fn after_rollback_rejected(&mut self, why: &str, message: &str) -> ContainEnd {
        let token = token_text(message).to_owned();
        let phase = match token_of(message) {
            Some(Token::ContainmentLost) => BlockPhase::Lost,
            Some(Token::Expired) => BlockPhase::Expired,
            Some(Token::RolledBack | Token::AbsentCovered) => BlockPhase::RolledBack,
            _ => BlockPhase::Failed,
        };
        let text = format!("{why}; rollback answered {token}");
        self.finish(
            phase,
            Some(&token),
            Some(message.to_owned()),
            settled(StepOutcome::Failed, Some(text), None),
        )
        .await
    }

    pub(super) async fn run(&mut self, session: AgentSessionId, epoch: JournalEpoch) -> ContainEnd {
        // Step one. The Applying row is committed before the frame is sent.
        let apply_id = self.block.block_id;
        match self
            .send(ContainCall::Apply(self.spec()), apply_id, session, epoch)
            .await
        {
            Answer::Done(outputs) => self.applied(&outputs).await,
            Answer::Rejected(message) => {
                let token = token_text(&message).to_owned();
                let (phase, outcome) = if is_refusal(&message) {
                    (BlockPhase::NotRun, StepOutcome::Refused)
                } else {
                    (BlockPhase::Failed, StepOutcome::Failed)
                };
                let reason: String = message.chars().take(UNRECOGNISED_MAX).collect();
                let kind = if phase == BlockPhase::NotRun {
                    audit::REFUSED
                } else {
                    audit::kind_of(phase)
                };
                let mut update = self.update(phase);
                update.failure_token = Some(token.clone());
                update.failure_message = Some(reason.clone());
                match self.move_to(update, kind, audit::system()).await {
                    Ok(()) => settled(outcome, Some(reason), None),
                    Err(_) => ContainEnd::Unknown,
                }
            }
            Answer::NotHandedOver => {
                self.finish(
                    BlockPhase::NotRun,
                    Some("hub_not_handed_off"),
                    None,
                    settled(
                        StepOutcome::NotRun,
                        Some("hub_not_handed_off".to_owned()),
                        None,
                    ),
                )
                .await
            }
            Answer::Unknown => {
                self.note_unknown().await;
                ContainEnd::Unknown
            }
        }
    }

    /// An audit-only row: the block stays where it is and a reconcile settles it.
    async fn note_unknown(&self) {
        let tx = Transaction::new().audit(audit::draft(
            audit::OUTCOME_UNKNOWN,
            &self.block,
            audit::system(),
            self.now(),
            None,
        ));
        if let Err(err) = self.deps.uow.commit(tx).await {
            tracing::warn!(%err, "could not record an unknown containment outcome");
        }
    }

    async fn applied(&mut self, outputs: &ContainOutputs) -> ContainEnd {
        let at = self.now();
        let mut update = PhaseUpdate::to(BlockPhase::Applied, at);
        update.applied_at = Some(at);
        update.existing_flows = outputs.get(keys::OUT_EXISTING_FLOWS).map(str::to_owned);
        update.bridge_gap = outputs.get(keys::OUT_BRIDGE_GAP).map(|v| v == "true");
        if self
            .move_to(update, audit::APPLIED, audit::system())
            .await
            .is_err()
        {
            return ContainEnd::Unknown;
        }
        // The confirmation is the hub's own reachability check: it must be sent in time.
        let late = self
            .now()
            .duration_since(at)
            .checked_sub(CONFIRM_WITHIN)
            .is_some_and(|over| over.is_positive());
        if late {
            return self
                .abandon("the confirmation could not be sent in time".to_owned())
                .await;
        }
        let Some((session, epoch)) = self.live() else {
            return self
                .abandon("the agent's session ended before the confirmation".to_owned())
                .await;
        };
        let confirm = ExecutionId::from_step(StepId::new());
        let mut update = self.update(BlockPhase::Confirming);
        update.confirm_execution = Some(confirm);
        if self
            .move_to(update, audit::CONFIRMING, audit::system())
            .await
            .is_err()
        {
            return ContainEnd::Unknown;
        }
        match self
            .send(ContainCall::Confirm(self.spec()), confirm, session, epoch)
            .await
        {
            Answer::Done(outputs) => self.confirmed(&outputs).await,
            Answer::Rejected(message) => self.confirm_rejected(&message).await,
            Answer::NotHandedOver => {
                self.abandon("the confirmation never reached the agent".to_owned())
                    .await
            }
            Answer::Unknown => {
                self.note_unknown().await;
                ContainEnd::Unknown
            }
        }
    }

    async fn confirmed(&mut self, outputs: &ContainOutputs) -> ContainEnd {
        let at = self.now();
        let mut update = PhaseUpdate::to(BlockPhase::Confirmed, at);
        update.deadline = at.checked_add(self.ttl).ok();
        update.existing_flows = outputs.get(keys::OUT_EXISTING_FLOWS).map(str::to_owned);
        update.bridge_gap = outputs.get(keys::OUT_BRIDGE_GAP).map(|v| v == "true");
        let flows = self.block.existing_flows.clone();
        let text = format!(
            "blocked {} for {} seconds{}",
            self.block.source,
            self.ttl.as_secs(),
            match update.existing_flows.as_deref().or(flows.as_deref()) {
                Some("may_persist") => "; existing connections may persist",
                _ => "",
            }
        );
        match self
            .move_to(update, audit::CONFIRMED, audit::system())
            .await
        {
            Ok(()) => settled(StepOutcome::Succeeded, None, Some(text)),
            Err(_) => ContainEnd::Unknown,
        }
    }

    async fn confirm_rejected(&mut self, message: &str) -> ContainEnd {
        let token = token_text(message).to_owned();
        match token_of(message) {
            // The element was gone: nothing is re-applied, a new block needs a new request.
            Some(Token::Expired) => {
                self.finish(
                    BlockPhase::Expired,
                    Some(&token),
                    Some(message.to_owned()),
                    settled(
                        StepOutcome::Failed,
                        Some("the block expired before it was confirmed".to_owned()),
                        None,
                    ),
                )
                .await
            }
            Some(Token::ContainmentLost) => {
                self.finish(
                    BlockPhase::Lost,
                    Some(&token),
                    Some(message.to_owned()),
                    settled(
                        StepOutcome::Failed,
                        Some("the helper's table was lost; nothing is blocked".to_owned()),
                        None,
                    ),
                )
                .await
            }
            Some(Token::RolledBack) => {
                self.finish(
                    BlockPhase::RolledBack,
                    Some(&token),
                    Some(message.to_owned()),
                    settled(
                        StepOutcome::Failed,
                        Some("the block was removed".to_owned()),
                        None,
                    ),
                )
                .await
            }
            _ => {
                self.abandon(format!("the confirmation was refused: {token}"))
                    .await
            }
        }
    }
}

impl Run<'_> {
    /// The owner's early release: removes a block that holds. The caller has checked the phase and
    /// that the session is alive; the rolling-back row is committed before the frame.
    pub(super) async fn release(
        &mut self,
        session: AgentSessionId,
        epoch: JournalEpoch,
    ) -> ContainEnd {
        let rollback = ExecutionId::from_step(StepId::new());
        let mut update = self.update(BlockPhase::RollingBack);
        update.rollback_execution = Some(rollback);
        if self
            .move_to(update, audit::RELEASED, audit::system())
            .await
            .is_err()
        {
            return ContainEnd::Unknown;
        }
        match self
            .send(ContainCall::Rollback(self.spec()), rollback, session, epoch)
            .await
        {
            Answer::Done(_) => {
                self.finish(
                    BlockPhase::RolledBack,
                    None,
                    None,
                    settled(
                        StepOutcome::Succeeded,
                        None,
                        Some("the block was released".to_owned()),
                    ),
                )
                .await
            }
            Answer::Rejected(message) => {
                self.after_rollback_rejected("the release", &message).await
            }
            Answer::NotHandedOver | Answer::Unknown => ContainEnd::Unknown,
        }
    }
}
