//! Unit-of-work contracts about aggregate scope: which effects a runner may claim, and
//! cancelling an aggregate's pending effects.

use domovoi_core::OperationId;
use domovoi_core::ports::{
    AggregateKind, AggregateRef, ClaimBatch, EffectCancel, EffectCompletion, EffectDraft,
    EffectKind, Transaction,
};
use jiff::SignedDuration;

use super::super::ContractResult;
use super::UowSubject;
use super::lifecycle::{LEASE, draft, fetch, store};
use crate::build::at;

/// A wake effect owned by `aggregate`, due at the start of time.
fn owned_by(aggregate: AggregateRef) -> EffectDraft {
    EffectDraft {
        aggregate,
        ..draft(EffectKind::Wake, 0)
    }
}

async fn claim_in(
    subject: &dyn UowSubject,
    kinds: &[EffectKind],
    aggregates: &[AggregateKind],
) -> Result<ClaimBatch, super::super::ContractError> {
    Ok(subject
        .queue()
        .claim_due(
            "r1",
            kinds,
            aggregates,
            at(0),
            SignedDuration::from_secs(LEASE),
            10,
        )
        .await?)
}

/// A claim takes only effects of the aggregate kinds it names, even when the effect kind is
/// the same: two services that both own `wake` never take each other's rows.
pub async fn claim_filters_by_aggregate_kind(subject: &dyn UowSubject) -> ContractResult {
    let legacy = store(subject, draft(EffectKind::Wake, 0)).await?;
    let operation = store(
        subject,
        owned_by(AggregateRef::operation(OperationId::new())),
    )
    .await?;

    let ops = claim_in(subject, &[EffectKind::Wake], &[AggregateKind::Operation]).await?;
    assert_eq!(ops.claimed.len(), 1);
    assert_eq!(ops.claimed[0].record.id, operation);
    assert_eq!(fetch(subject, legacy).await?.status, "pending");

    let old = claim_in(subject, &[EffectKind::Wake], &AggregateKind::LEGACY).await?;
    assert_eq!(old.claimed.len(), 1);
    assert_eq!(old.claimed[0].record.id, legacy);
    Ok(())
}

/// An empty aggregate scope claims and reverts nothing.
pub async fn an_empty_aggregate_scope_claims_nothing(subject: &dyn UowSubject) -> ContractResult {
    let id = store(subject, draft(EffectKind::Wake, 0)).await?;
    let none = claim_in(subject, &[EffectKind::Wake], &[]).await?;
    assert!(none.claimed.is_empty() && none.dead.is_empty());
    assert_eq!(fetch(subject, id).await?.status, "pending");

    let taken = claim_in(subject, &[EffectKind::Wake], &AggregateKind::LEGACY).await?;
    assert_eq!(taken.claimed.len(), 1);
    assert_eq!(subject.queue().revert_claimed_at_start(&[]).await?, 0);
    assert_eq!(fetch(subject, id).await?.status, "claimed");
    Ok(())
}

/// Reverting claims at start touches only the aggregate kinds it is given.
pub async fn revert_claimed_at_start_is_scoped_to_aggregates(
    subject: &dyn UowSubject,
) -> ContractResult {
    let legacy = store(subject, draft(EffectKind::Wake, 0)).await?;
    let operation = store(
        subject,
        owned_by(AggregateRef::operation(OperationId::new())),
    )
    .await?;
    let both = [AggregateKind::Incident, AggregateKind::Operation];
    let batch = claim_in(subject, &[EffectKind::Wake], &both).await?;
    assert_eq!(batch.claimed.len(), 2);

    let reverted = subject
        .queue()
        .revert_claimed_at_start(&[AggregateKind::Operation])
        .await?;
    assert_eq!(reverted, 1);
    assert_eq!(fetch(subject, operation).await?.status, "pending");
    assert_eq!(fetch(subject, legacy).await?.status, "claimed");
    Ok(())
}

/// Cancelling an aggregate turns its pending effects to `cancelled` and leaves claimed
/// effects, other aggregates and other kinds alone.
pub async fn cancel_effects_cancels_only_pending_effects_of_the_aggregate(
    subject: &dyn UowSubject,
) -> ContractResult {
    let target = AggregateRef::operation(OperationId::new());
    let held = store(subject, owned_by(target.clone())).await?;
    let claimed = claim_in(subject, &[EffectKind::Wake], &[AggregateKind::Operation]).await?;
    assert_eq!(claimed.claimed[0].record.id, held);
    let waiting = store(subject, owned_by(target.clone())).await?;
    let other = store(
        subject,
        owned_by(AggregateRef::operation(OperationId::new())),
    )
    .await?;
    let legacy = store(subject, draft(EffectKind::Wake, 0)).await?;

    subject
        .uow()
        .commit(Transaction::new().cancel_effects(EffectCancel {
            aggregate: target,
            at: at(5),
        }))
        .await?;

    let cancelled = fetch(subject, waiting).await?;
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(cancelled.completed_at, Some(at(5)));
    assert_eq!(cancelled.claim_token, None);
    assert_eq!(fetch(subject, held).await?.status, "claimed");
    assert_eq!(fetch(subject, other).await?.status, "pending");
    assert_eq!(fetch(subject, legacy).await?.status, "pending");

    // The held effect still finishes under its own token.
    subject
        .uow()
        .commit(Transaction::new().complete(EffectCompletion {
            id: held,
            claim_token: Some(claimed.claimed[0].token),
            at: at(6),
        }))
        .await?;
    assert_eq!(fetch(subject, held).await?.status, "done");
    Ok(())
}

/// `claimed_of` lists only claimed effects of the kind and the aggregate kind it is given, in
/// creation order, and stops at its limit.
pub async fn claimed_of_filters_by_kind_aggregate_and_status(
    subject: &dyn UowSubject,
) -> ContractResult {
    let dispatch = |aggregate: AggregateRef| EffectDraft {
        aggregate,
        ..draft(EffectKind::Dispatch, 0)
    };
    let first = store(
        subject,
        dispatch(AggregateRef::operation(OperationId::new())),
    )
    .await?;
    let second = store(
        subject,
        dispatch(AggregateRef::operation(OperationId::new())),
    )
    .await?;
    let mut later = dispatch(AggregateRef::operation(OperationId::new()));
    later.not_before = at(100);
    let unclaimed = store(subject, later).await?;
    let legacy = store(subject, draft(EffectKind::Dispatch, 0)).await?;
    let wake = store(
        subject,
        owned_by(AggregateRef::operation(OperationId::new())),
    )
    .await?;
    // Claim everything except `unclaimed`, which is due later.
    let claimed = claim_in(
        subject,
        &[EffectKind::Dispatch, EffectKind::Wake],
        &[AggregateKind::Operation, AggregateKind::Incident],
    )
    .await?;
    assert_eq!(claimed.claimed.len(), 4, "every due effect is claimed");
    for id in [first, second, legacy, wake] {
        assert_eq!(fetch(subject, id).await?.status, "claimed");
    }
    assert_eq!(fetch(subject, unclaimed).await?.status, "pending");
    let listed = subject
        .effects()
        .claimed_of(EffectKind::Dispatch, AggregateKind::Operation, 10)
        .await?;
    let found: Vec<_> = listed.iter().map(|record| record.id).collect();
    assert_eq!(found, vec![first, second]);
    let limited = subject
        .effects()
        .claimed_of(EffectKind::Dispatch, AggregateKind::Operation, 1)
        .await?;
    assert_eq!(limited.len(), 1);
    Ok(())
}
