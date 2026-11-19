//! One decision, one commit: load a fresh snapshot, apply the writer's own row changes, run
//! the pure decision, build the transaction and commit it. A conflicting commit reloads and
//! decides again once; a second conflict is an error.

use domovoi_core::ports::{
    AggregateRef, AuditDraft, CommitError, EffectCancel, EffectCompletion, EffectDraft, EffectKind,
    EffectReschedule, ExecutionGuard, Expected, JournalDraft, OperationWrite, StateWrite,
    Transaction,
};
use domovoi_core::{
    Actor, ApprovalChannel, ApprovalProvenance, Authorization, EffectId, ExecutionId,
    ExecutionRecord, IncidentId, JournalEpoch, OperationId, OperationSnapshot, PlanBinding, StepId,
    StepOutcome,
};
use jiff::Timestamp;

use super::OpShared;
use super::commands::Held;
use crate::chat_token::chat_token_ok;
use crate::operations::decide::{
    DecideError, Decision, EffectSpec, IdSource, OperationEvent, RECONCILE_ATTEMPTS, decide,
    reconcile_key, step_payload,
};
use crate::operations::error::OperationError;
use crate::operations::state::OperationState;

/// Fresh random ids for steps and effects.
pub(crate) struct UuidIds;

impl IdSource for UuidIds {
    fn step(&mut self) -> StepId {
        StepId::new()
    }

    fn effect(&mut self) -> EffectId {
        EffectId::new()
    }
}

/// A person's approval to record on an execution row.
pub(crate) struct AuthorizeEdit {
    pub(crate) step: StepId,
    pub(crate) authorization: Authorization,
    pub(crate) binding: PlanBinding,
    pub(crate) provenance: ApprovalProvenance,
    /// The chat token a chat approval must carry; a web approval ignores it.
    pub(crate) chat_token: Option<String>,
}

/// A change to an execution row that is the writer's to make, not the decision's. Applied to
/// the loaded snapshot before `decide` runs, so `decide` sees the row as it will be stored.
pub(crate) enum Edit {
    /// No row changes.
    None,
    /// Records a person's approval on the row.
    Authorize(Box<AuthorizeEdit>),
    /// Refuses the change unless the step's outstanding chat token matches. A chat denial
    /// carries one; the decision then clears it in the same commit.
    CheckChatToken(StepId, String),
    /// Records when the hub began waiting for the agent, once.
    Waiting(StepId),
    /// Records the journal epoch and time of the hand-off.
    Dispatched(StepId, JournalEpoch),
    /// Records when the result was recorded.
    Settled(StepId),
}

impl Edit {
    /// Applies the change to `snapshot`; returns the Execution steps whose row changed.
    fn apply(
        &self,
        snapshot: &mut OperationSnapshot,
        now: Timestamp,
    ) -> Result<Vec<StepId>, OperationError> {
        let step = match self {
            Self::None => return Ok(Vec::new()),
            Self::Authorize(edit) => edit.step,
            Self::CheckChatToken(step, _) => *step,
            Self::Waiting(step) | Self::Dispatched(step, _) | Self::Settled(step) => *step,
        };
        let row = snapshot
            .executions
            .iter_mut()
            .find(|row| row.value.step == step)
            .ok_or(DecideError::UnknownStep(step))?;
        let record = &mut row.value;
        if let Self::CheckChatToken(_, token) = self {
            if !chat_token_ok(Some(token), record.chat_token_hash()) {
                return Err(OperationError::BadToken);
            }
            return Ok(Vec::new());
        }
        match self {
            Self::None | Self::CheckChatToken(..) => {}
            Self::Authorize(edit) => {
                let chat = edit
                    .authorization
                    .human_approval()
                    .is_some_and(|human| matches!(human.channel(), ApprovalChannel::Chat { .. }));
                if chat && !chat_token_ok(edit.chat_token.as_deref(), record.chat_token_hash()) {
                    return Err(OperationError::BadToken);
                }
                record.authorize(
                    edit.authorization.clone(),
                    edit.provenance.clone(),
                    &edit.binding,
                    now,
                )?;
                record.clear_chat_token_hash();
            }
            Self::Waiting(_) => {
                if record.waiting_since.is_some() {
                    return Ok(Vec::new());
                }
                record.waiting_since = Some(now);
            }
            Self::Dispatched(_, epoch) => record.dispatched = Some((*epoch, now)),
            Self::Settled(_) => {
                if record.settled_at.is_none() {
                    record.settled_at = Some(now);
                }
            }
        }
        record.updated_at = now;
        Ok(vec![step])
    }
}

/// One change to one operation.
pub(crate) struct Change {
    /// The operation.
    pub(crate) operation: OperationId,
    /// Who it is for the journal and the audit log.
    pub(crate) actor: Actor,
    /// What happened.
    pub(crate) event: OperationEvent,
    /// The writer's own row change.
    pub(crate) edit: Edit,
    /// The effect this change answers, completed in the same commit.
    pub(crate) held: Option<Held>,
    /// An effect that must still be held by this claim for the change to apply, and stays
    /// held: a step that starts is not the end of its effect.
    pub(crate) fence: Option<Held>,
    /// Effects to create beyond what the decision creates.
    pub(crate) effects: Vec<EffectSpec>,
}

impl Change {
    /// A change with no row edit, no held effect and no extra effects.
    pub(crate) fn new(operation: OperationId, actor: Actor, event: OperationEvent) -> Self {
        Self {
            operation,
            actor,
            event,
            edit: Edit::None,
            held: None,
            fence: None,
            effects: Vec::new(),
        }
    }
}

/// What a commit did, for what the caller does next.
pub(crate) struct Applied {
    /// Whether the operation became terminal in this commit, which frees a slot.
    pub(crate) became_terminal: bool,
    /// Executions that left "awaiting approval" in this commit.
    pub(crate) left_approval: Vec<ExecutionId>,
    /// The first incident the operation is about.
    pub(crate) incident: Option<IncidentId>,
}

/// The error a caller sees for a failed commit.
pub(crate) fn commit_error(err: CommitError) -> OperationError {
    match err {
        CommitError::Conflict { .. } => OperationError::Conflict,
        CommitError::StaleEffect { .. } => OperationError::StaleEffect,
        CommitError::Constraint(message) | CommitError::Backend(message) => {
            OperationError::Store(message)
        }
    }
}

/// A draft of an effect created by this commit.
pub(crate) fn draft(spec: &EffectSpec, operation: OperationId, now: Timestamp) -> EffectDraft {
    EffectDraft {
        id: spec.id,
        kind: spec.kind,
        aggregate: AggregateRef::operation(operation),
        payload: spec.payload.clone(),
        not_before: spec.not_before,
        max_attempts: spec.max_attempts,
        dedup_key: spec.dedup_key.clone(),
        created_at: now,
    }
}

/// Adds everything `decision` changes, except the operation row, to `tx`: steps, born and
/// later progress, edges, execution rows (new, then `updated` at their loaded versions),
/// journal, audit and effects.
pub(crate) fn push_decision(
    mut tx: Transaction,
    decision: &Decision,
    updated: &[(ExecutionRecord, u64)],
    actor: &Actor,
    now: Timestamp,
) -> Transaction {
    let operation = decision.operation.id;
    for record in &decision.steps {
        tx = tx.write(StateWrite::Operation(OperationWrite::Step(
            record.step.clone(),
        )));
    }
    for record in &decision.steps {
        if record.progress.outcome != StepOutcome::Pending {
            tx = tx.write(StateWrite::Operation(OperationWrite::Progress {
                step: record.step.id,
                from: StepOutcome::Pending,
                to: record.progress.clone(),
            }));
        }
    }
    for change in &decision.progress {
        tx = tx.write(StateWrite::Operation(OperationWrite::Progress {
            step: change.step,
            from: change.from,
            to: change.to.clone(),
        }));
    }
    for edge in &decision.edges {
        tx = tx.write(StateWrite::Operation(OperationWrite::Edge(*edge)));
    }
    for row in &decision.executions {
        tx = tx.write(StateWrite::Operation(OperationWrite::Execution {
            execution: row.clone(),
            expected: ExecutionGuard::New,
        }));
    }
    for (row, version) in updated {
        tx = tx.write(StateWrite::Operation(OperationWrite::Execution {
            execution: row.clone(),
            expected: ExecutionGuard::Version(*version),
        }));
    }
    for note in &decision.journal {
        tx = tx.journal(JournalDraft {
            aggregate: AggregateRef::operation(operation),
            kind: note.kind,
            payload_v: 1,
            payload: note.payload.clone(),
            actor: actor.clone(),
            occurred_at: now,
        });
    }
    for note in &decision.audit {
        tx = tx.audit(AuditDraft::new(
            actor.clone(),
            now,
            note.kind.to_owned(),
            note.subject.clone(),
            note.detail.clone(),
        ));
    }
    for spec in &decision.effects {
        tx = tx.effect(draft(spec, operation, now));
    }
    tx
}

/// Applies `change` and commits it. A conflict reloads and decides again once.
pub(crate) async fn apply(shared: &OpShared, change: Change) -> Result<Applied, OperationError> {
    let mut result = attempt(shared, &change).await;
    if matches!(result, Err(OperationError::Conflict)) {
        result = attempt(shared, &change).await;
        if matches!(result, Err(OperationError::Conflict)) {
            tracing::error!(
                operation = %change.operation,
                "the operation changed under the writer twice; the change was dropped",
            );
        }
    }
    if let (Err(err), Some(held)) = (&result, change.held) {
        release(shared, held, err).await;
    }
    if let Ok(applied) = &result {
        super::announce::left_approval(shared, applied).await;
    }
    result
}

/// Lets go of a held effect after its change was refused: a refusal will not pass on a
/// retry, so the effect is completed; a paused operation is retried later.
pub(crate) async fn release(shared: &OpShared, held: Held, err: &OperationError) {
    let now = shared.deps.clock.now();
    let tx = match err {
        OperationError::Decide(DecideError::Paused) => {
            let not_before = now
                .checked_add(shared.settings.retry_interval)
                .unwrap_or(now);
            Transaction::new().reschedule(EffectReschedule {
                id: held.effect,
                claim_token: held.claim,
                not_before,
                error: "the operation is paused".to_owned(),
                at: now,
            })
        }
        OperationError::Decide(_) | OperationError::NotFound(_) => {
            Transaction::new().complete(completion(held, now))
        }
        OperationError::StaleEffect => {
            tracing::warn!(effect = %held.effect, "a handler answered with a stale claim");
            return;
        }
        _ => return,
    };
    if let Err(err) = shared.deps.uow.commit(tx).await {
        tracing::warn!(%err, effect = %held.effect, "could not release a held effect");
    }
}

/// The completion of a held effect.
pub(crate) fn completion(held: Held, now: Timestamp) -> EffectCompletion {
    EffectCompletion {
        id: held.effect,
        claim_token: Some(held.claim),
        at: now,
    }
}

/// One attempt: load, edit, decide, build, commit.
async fn attempt(shared: &OpShared, change: &Change) -> Result<Applied, OperationError> {
    let deps = &shared.deps;
    let now = deps.clock.now();
    let loaded = deps
        .operations
        .load(change.operation)
        .await
        .map_err(|err| OperationError::Store(err.to_string()))?
        .ok_or(OperationError::NotFound(change.operation))?;
    if let Some(fence) = change.fence {
        super::support::check_claim(shared, fence).await?;
    }
    let mut snapshot = loaded.clone();
    let changed = change.edit.apply(&mut snapshot, now)?;
    let mut updated: Vec<(ExecutionRecord, u64)> = changed
        .iter()
        .filter_map(|step| {
            let row = snapshot.execution_of(*step)?;
            let version = loaded.execution_of(*step).map_or(0, |r| r.version);
            Some((row.value.clone(), version))
        })
        .collect();
    let state = OperationState::new(snapshot);
    let decision = decide(&state, change.event.clone(), now, &mut UuidIds)?;
    let after = state.apply(&decision);
    let left_approval = super::support::spend_tokens(&loaded, &after, &mut updated);
    let mut tx = Transaction::new();
    let mut applied = Applied {
        became_terminal: false,
        left_approval,
        incident: loaded.incidents.first().copied(),
    };
    if !(decision.is_noop(&state) && updated.is_empty()) {
        tx = tx.write(StateWrite::Operation(OperationWrite::Operation {
            operation: decision.operation.clone(),
            expected: Expected::Version(loaded.version),
        }));
        tx = push_decision(tx, &decision, &updated, &change.actor, now);
        if !loaded.operation.status.is_terminal() && decision.operation.status.is_terminal() {
            applied.became_terminal = true;
            tx = tx.cancel_effects(EffectCancel {
                aggregate: AggregateRef::operation(change.operation),
                at: now,
            });
            tx = super::support::keep_reconciling(tx, &state.apply(&decision), now);
        }
    }
    for spec in &change.effects {
        tx = tx.effect(draft(spec, change.operation, now));
    }
    if let Some(held) = change.held {
        tx = tx.complete(completion(held, now));
    }
    if tx.is_empty() {
        return Ok(applied);
    }
    let created = !tx.new_effects.is_empty();
    deps.uow.commit(tx).await.map_err(commit_error)?;
    if created {
        shared.nudge.notify_one();
    }
    Ok(applied)
}

/// Queues one reconcile effect for Execution step `step`; already queued is not an error.
pub(crate) async fn queue_reconcile(
    shared: &OpShared,
    operation: OperationId,
    step: StepId,
) -> Result<(), OperationError> {
    let now = shared.deps.clock.now();
    let tx = Transaction::new().effect(EffectDraft {
        id: EffectId::new(),
        kind: EffectKind::Reconcile,
        aggregate: AggregateRef::operation(operation),
        payload: step_payload(operation, step),
        not_before: now,
        max_attempts: RECONCILE_ATTEMPTS,
        dedup_key: Some(reconcile_key(step)),
        created_at: now,
    });
    match shared.deps.uow.commit(tx).await {
        Ok(_) => {
            shared.nudge.notify_one();
            Ok(())
        }
        Err(CommitError::Constraint(message)) if message.contains("duplicate effect") => Ok(()),
        Err(err) => Err(commit_error(err)),
    }
}

/// `spec` without its dedup key when it equals the held effect's own: the held effect is
/// still claimed when the successor is created, so the same key would collide with it.
pub(crate) async fn without_held_key(
    shared: &OpShared,
    held: Held,
    mut spec: EffectSpec,
) -> Result<EffectSpec, OperationError> {
    let existing = shared
        .deps
        .effect_read
        .effect(held.effect)
        .await
        .map_err(|err| OperationError::Store(err.to_string()))?;
    if existing.is_some_and(|record| record.dedup_key == spec.dedup_key) {
        spec.dedup_key = None;
    }
    Ok(spec)
}

/// Completes a held effect that has nothing left to do, and creates `successor` when given,
/// in one commit.
pub(crate) async fn finish_held(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    successor: Option<EffectSpec>,
) -> Result<(), OperationError> {
    finish_held_many(shared, held, operation, successor.into_iter().collect()).await
}

/// Completes a held effect and creates every effect of `successors`, in one commit.
pub(crate) async fn finish_held_many(
    shared: &OpShared,
    held: Held,
    operation: OperationId,
    successors: Vec<EffectSpec>,
) -> Result<(), OperationError> {
    let now = shared.deps.clock.now();
    let mut tx = Transaction::new();
    for spec in successors {
        let spec = without_held_key(shared, held, spec).await?;
        tx = tx.effect(draft(&spec, operation, now));
    }
    let created = !tx.new_effects.is_empty();
    tx = tx.complete(completion(held, now));
    shared.deps.uow.commit(tx).await.map_err(commit_error)?;
    if created {
        shared.nudge.notify_one();
    }
    Ok(())
}

/// Commits `tx` as one unit: the writer's only commit for changes that carry no decision.
pub(super) async fn commit_plain(shared: &OpShared, tx: Transaction) -> Result<(), OperationError> {
    shared.deps.uow.commit(tx).await.map_err(commit_error)?;
    Ok(())
}
