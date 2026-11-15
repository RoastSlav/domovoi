//! Unit-of-work contracts about the effect queue: claiming, leases and recovery.

use domovoi_core::ports::{
    AggregateRef, ClaimBatch, ClaimToken, CommitError, EffectCompletion, EffectDeath, EffectDraft,
    EffectKind, EffectRecord, EffectReschedule, Expected, StateWrite, Transaction,
};
use domovoi_core::{EffectId, IncidentId, Note, NoteId};
use jiff::{SignedDuration, Timestamp};
use serde_json::json;

use super::super::{ContractError, ContractResult};
use super::{UowSubject, lease};
use crate::build::{T0, at, host};

/// A five-minute lease, the runner's default in these contracts.
pub(super) const LEASE: i64 = 300;

pub(super) fn draft(kind: EffectKind, not_before_secs: i64) -> EffectDraft {
    EffectDraft {
        id: EffectId::new(),
        kind,
        aggregate: AggregateRef::incident(IncidentId::new()),
        payload: json!({}),
        not_before: at(not_before_secs),
        max_attempts: 3,
        dedup_key: None,
        created_at: at(not_before_secs),
    }
}

/// Commits one new effect and returns its id.
pub(super) async fn store(
    subject: &dyn UowSubject,
    effect: EffectDraft,
) -> Result<EffectId, ContractError> {
    let id = effect.id;
    subject
        .uow()
        .commit(Transaction::new().effect(effect))
        .await?;
    Ok(id)
}

/// Runs one claim pass with the contract's five-minute lease and a limit of ten.
pub(super) async fn claim(
    subject: &dyn UowSubject,
    runner: &str,
    kinds: &[EffectKind],
    now: Timestamp,
) -> Result<ClaimBatch, ContractError> {
    Ok(subject
        .queue()
        .claim_due(runner, kinds, now, SignedDuration::from_secs(LEASE), 10)
        .await?)
}

/// The stored row, or a setup failure when it is missing.
pub(super) async fn fetch(
    subject: &dyn UowSubject,
    id: EffectId,
) -> Result<EffectRecord, ContractError> {
    subject
        .effects()
        .effect(id)
        .await?
        .ok_or_else(|| ContractError::Setup("effect missing".to_owned()))
}

/// A runbook note owned by `host`, for proving a stale effect write rolls the whole commit back.
fn note(host_id: domovoi_core::HostId) -> Note {
    Note {
        id: NoteId::new(),
        host: Some(host_id),
        title: "Runbook".to_owned(),
        body_markdown: "Steps".to_owned(),
        tags: Vec::new(),
        updated_at: T0,
    }
}

/// A claim gives the row a token, the next generation, a lease and the runner's id.
pub async fn claim_sets_token_generation_lease_and_attempt(
    subject: &dyn UowSubject,
) -> ContractResult {
    let id = store(subject, draft(EffectKind::Wake, 0)).await?;
    let batch = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    assert_eq!(batch.dead.len(), 0);
    assert_eq!(batch.claimed.len(), 1);
    assert_eq!(batch.claimed[0].record.id, id);

    let record = fetch(subject, id).await?;
    assert_eq!(record.status, "claimed");
    assert_eq!(record.attempt, 1);
    assert_eq!(record.claim_generation, 1);
    assert_eq!(record.lease_until, Some(lease(LEASE)));
    assert_eq!(record.runner_id.as_deref(), Some("r1"));
    assert_eq!(record.claim_token, Some(batch.claimed[0].token));

    let again = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    assert_eq!(again.claimed.len(), 0);
    assert_eq!(again.dead.len(), 0);
    Ok(())
}

/// An effect that is not due, or of another kind, is not claimed.
pub async fn claim_skips_effects_not_yet_due_and_other_kinds(
    subject: &dyn UowSubject,
) -> ContractResult {
    let later = store(subject, draft(EffectKind::Wake, 60)).await?;
    let other = store(subject, draft(EffectKind::Notify, 0)).await?;

    let wake = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    assert_eq!(wake.claimed.len(), 0, "the wake effect is not due yet");
    assert_eq!(fetch(subject, later).await?.status, "pending");
    assert_eq!(fetch(subject, other).await?.status, "pending");

    let notify = claim(subject, "r1", &[EffectKind::Notify], at(0)).await?;
    assert_eq!(notify.claimed.len(), 1);
    assert_eq!(notify.claimed[0].record.id, other);
    Ok(())
}

/// Due pending work comes before work whose claim expired, however old that claim's due time.
pub async fn claim_prioritizes_pending_effects_before_expired_claims(
    subject: &dyn UowSubject,
) -> ContractResult {
    // A is due first and is claimed under a short lease, so its claim expires early.
    let expired = store(subject, draft(EffectKind::Wake, 0)).await?;
    let first = subject
        .queue()
        .claim_due(
            "r1",
            &[EffectKind::Wake],
            at(0),
            SignedDuration::from_secs(5),
            10,
        )
        .await?;
    assert_eq!(first.claimed.len(), 1);
    let stale = first.claimed[0].token;

    // B becomes due later than A was, but it has never been claimed.
    let pending = store(subject, draft(EffectKind::Wake, 10)).await?;

    // One slot at a time: the pending effect takes it, and the expired claim waits.
    let one = subject
        .queue()
        .claim_due(
            "r2",
            &[EffectKind::Wake],
            at(11),
            SignedDuration::from_secs(LEASE),
            1,
        )
        .await?;
    assert_eq!(one.dead.len(), 0);
    assert_eq!(one.claimed.len(), 1);
    assert_eq!(
        one.claimed[0].record.id, pending,
        "the due pending effect is claimed before the expired claim"
    );
    let kept = fetch(subject, expired).await?;
    assert_eq!(kept.status, "claimed", "the expired claim is left alone");
    assert_eq!(kept.claim_token, Some(stale));
    assert_eq!(kept.attempt, 1);

    // With room for both, the expired claim is reclaimed under a new token and generation.
    let both = claim(subject, "r2", &[EffectKind::Wake], at(11)).await?;
    let reclaimed = both
        .claimed
        .iter()
        .find(|claimed| claimed.record.id == expired)
        .ok_or_else(|| ContractError::Setup("the expired claim was not reclaimed".to_owned()))?;
    assert_ne!(reclaimed.token, stale, "the reclaim uses a fresh token");
    let record = fetch(subject, expired).await?;
    assert_eq!(record.claim_generation, 2);
    assert_eq!(record.attempt, 2);
    Ok(())
}

/// After its lease expires, a claimed effect is reclaimed under a fresh token and generation.
pub async fn claimed_effect_reclaimed_after_lease_expiry(
    subject: &dyn UowSubject,
) -> ContractResult {
    let id = store(subject, draft(EffectKind::Wake, 0)).await?;
    let first = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    assert_eq!(first.claimed.len(), 1);
    let first_token = first.claimed[0].token;

    let second = claim(subject, "r2", &[EffectKind::Wake], at(LEASE + 1)).await?;
    assert_eq!(second.claimed.len(), 1);
    assert_eq!(second.claimed[0].record.id, id);
    let second_token = second.claimed[0].token;
    assert_ne!(second_token, first_token, "every claim gets a fresh token");

    let record = fetch(subject, id).await?;
    assert_eq!(record.status, "claimed");
    assert_eq!(record.claim_generation, 2);
    assert_eq!(record.attempt, 2);
    assert_eq!(record.runner_id.as_deref(), Some("r2"));
    assert_eq!(record.claim_token, Some(second_token));
    Ok(())
}

/// A stale lease holder's completion is refused and rolls back the state it carried.
pub async fn stale_lease_holder_cannot_complete_reclaimed_effect(
    subject: &dyn UowSubject,
) -> ContractResult {
    let id = store(subject, draft(EffectKind::Wake, 0)).await?;
    let first = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    let stale = first.claimed[0].token;
    let second = claim(subject, "r2", &[EffectKind::Wake], at(LEASE + 1)).await?;
    let current = second.claimed[0].token;

    let host = host("nas")?;
    let note = note(host.id);
    let note_id = note.id;
    let failed = subject
        .uow()
        .commit(
            Transaction::new()
                .write(StateWrite::Note {
                    note,
                    expected: Expected::New,
                })
                .complete(EffectCompletion {
                    id,
                    claim_token: Some(stale),
                    at: at(LEASE + 2),
                }),
        )
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::StaleEffect { effect }) if effect == id
    ));
    assert_eq!(subject.notes().get(note_id).await?, None);
    assert_eq!(fetch(subject, id).await?.status, "claimed");

    subject
        .uow()
        .commit(Transaction::new().complete(EffectCompletion {
            id,
            claim_token: Some(current),
            at: at(LEASE + 3),
        }))
        .await?;
    let record = fetch(subject, id).await?;
    assert_eq!(record.status, "done");
    assert_eq!(record.completed_at, Some(at(LEASE + 3)));
    Ok(())
}

/// A stale lease holder can neither reschedule nor dead-letter the effect it lost.
pub async fn stale_holder_cannot_reschedule_or_dead_letter(
    subject: &dyn UowSubject,
) -> ContractResult {
    let id = store(subject, draft(EffectKind::Wake, 0)).await?;
    let first = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    let stale = first.claimed[0].token;
    claim(subject, "r2", &[EffectKind::Wake], at(LEASE + 1)).await?;
    let before = fetch(subject, id).await?;

    let failed = subject
        .uow()
        .commit(Transaction::new().reschedule(EffectReschedule {
            id,
            claim_token: stale,
            not_before: at(LEASE + 90),
            error: "offline".to_owned(),
            at: at(LEASE + 2),
        }))
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::StaleEffect { effect }) if effect == id
    ));
    assert_eq!(fetch(subject, id).await?, before);

    let failed = subject
        .uow()
        .commit(Transaction::new().kill(EffectDeath {
            id,
            claim_token: stale,
            error: "gave up".to_owned(),
            at: at(LEASE + 2),
        }))
        .await;
    assert!(matches!(
        failed,
        Err(CommitError::StaleEffect { effect }) if effect == id
    ));
    assert_eq!(fetch(subject, id).await?, before);
    Ok(())
}

/// Claiming never takes a dispatch effect back from an expired lease.
pub async fn dispatch_effects_are_never_reclaimed_after_lease_expiry(
    subject: &dyn UowSubject,
) -> ContractResult {
    let id = store(subject, draft(EffectKind::Dispatch, 0)).await?;
    let first = claim(subject, "r1", &[EffectKind::Dispatch], at(0)).await?;
    assert_eq!(first.claimed.len(), 1);

    let later = claim(subject, "r2", &[EffectKind::Dispatch], at(LEASE + 1)).await?;
    assert_eq!(later.claimed.len(), 0);
    assert_eq!(later.dead.len(), 0);
    let record = fetch(subject, id).await?;
    assert_eq!(record.status, "claimed");
    assert_eq!(record.claim_generation, 1);
    assert_eq!(record.runner_id.as_deref(), Some("r1"));
    Ok(())
}

/// At start only non-dispatch claims are put back, each with a new generation and no token.
pub async fn restart_reverts_claimed_effects_except_dispatch(
    subject: &dyn UowSubject,
) -> ContractResult {
    let wake = store(subject, draft(EffectKind::Wake, 0)).await?;
    let dispatch = store(subject, draft(EffectKind::Dispatch, 0)).await?;
    let batch = claim(
        subject,
        "r1",
        &[EffectKind::Wake, EffectKind::Dispatch],
        at(0),
    )
    .await?;
    assert_eq!(batch.claimed.len(), 2);

    let reverted = subject.queue().revert_claimed_at_start().await?;
    assert_eq!(reverted, 1);

    let wake = fetch(subject, wake).await?;
    assert_eq!(wake.status, "pending");
    assert_eq!(wake.claim_generation, 2);
    assert_eq!(wake.claim_token, None);
    assert_eq!(wake.lease_until, None);
    assert_eq!(wake.runner_id, None);
    assert_eq!(wake.attempt, 1, "the attempt already made is kept");

    let dispatch = fetch(subject, dispatch).await?;
    assert_eq!(dispatch.status, "claimed");
    assert_eq!(dispatch.claim_generation, 1);
    assert!(dispatch.claim_token.is_some());
    Ok(())
}

/// A rescheduled effect keeps its dedup key, so no second one may be created meanwhile.
pub async fn dedup_key_blocks_a_second_pending_effect(subject: &dyn UowSubject) -> ContractResult {
    let aggregate = AggregateRef::incident(IncidentId::new());
    let mut first = draft(EffectKind::Verify, 0);
    first.aggregate = aggregate.clone();
    first.dedup_key = Some("verify:x".to_owned());
    let id = first.id;
    store(subject, first).await?;
    let held = claim(subject, "r1", &[EffectKind::Verify], at(0)).await?;
    let token: ClaimToken = held.claimed[0].token;
    subject
        .uow()
        .commit(Transaction::new().reschedule(EffectReschedule {
            id,
            claim_token: token,
            not_before: at(60),
            error: "offline".to_owned(),
            at: at(1),
        }))
        .await?;
    assert_eq!(fetch(subject, id).await?.status, "pending");

    let mut second = draft(EffectKind::Verify, 70);
    second.aggregate = aggregate;
    second.dedup_key = Some("verify:x".to_owned());
    let failed = subject
        .uow()
        .commit(Transaction::new().effect(second.clone()))
        .await;
    assert!(matches!(failed, Err(CommitError::Constraint(_))));
    assert_eq!(subject.effects().effect(second.id).await?, None);
    Ok(())
}
