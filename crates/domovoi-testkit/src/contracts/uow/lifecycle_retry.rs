//! Unit-of-work contracts about putting a claimed effect back or giving up on it.

use domovoi_core::ports::{EffectKind, EffectReschedule, Transaction};

use super::super::ContractResult;
use super::UowSubject;
use super::lifecycle::{LEASE, claim, draft, fetch, store};
use crate::build::at;

/// A reschedule puts the effect back to `pending` at the given time and keeps its attempt count.
pub async fn reschedule_returns_effect_to_pending_with_backoff_time(
    subject: &dyn UowSubject,
) -> ContractResult {
    let id = store(subject, draft(EffectKind::Wake, 0)).await?;
    let held = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    let token = held.claimed[0].token;

    subject
        .uow()
        .commit(Transaction::new().reschedule(EffectReschedule {
            id,
            claim_token: token,
            not_before: at(120),
            error: "agent offline".to_owned(),
            at: at(10),
        }))
        .await?;

    let record = fetch(subject, id).await?;
    assert_eq!(record.status, "pending");
    assert_eq!(record.not_before, at(120));
    assert_eq!(record.last_error.as_deref(), Some("agent offline"));
    assert_eq!(record.claim_token, None);
    assert_eq!(record.lease_until, None);
    assert_eq!(record.runner_id, None);
    assert_eq!(record.attempt, 1);
    assert_eq!(record.claim_generation, 1);

    let early = claim(subject, "r1", &[EffectKind::Wake], at(119)).await?;
    assert_eq!(early.claimed.len(), 0);
    let due = claim(subject, "r1", &[EffectKind::Wake], at(120)).await?;
    assert_eq!(due.claimed.len(), 1);
    assert_eq!(due.claimed[0].record.id, id);
    assert_eq!(due.claimed[0].record.attempt, 2);
    Ok(())
}

/// An effect at its attempt limit is dead-lettered by the next claim instead of being taken.
pub async fn attempt_limit_dead_letters_at_claim(subject: &dyn UowSubject) -> ContractResult {
    let mut effect = draft(EffectKind::Wake, 0);
    effect.max_attempts = 2;
    let id = store(subject, effect).await?;

    let first = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    subject
        .uow()
        .commit(Transaction::new().reschedule(EffectReschedule {
            id,
            claim_token: first.claimed[0].token,
            not_before: at(60),
            error: "offline".to_owned(),
            at: at(1),
        }))
        .await?;
    let second = claim(subject, "r1", &[EffectKind::Wake], at(60)).await?;
    assert_eq!(second.claimed[0].record.attempt, 2);
    subject
        .uow()
        .commit(Transaction::new().reschedule(EffectReschedule {
            id,
            claim_token: second.claimed[0].token,
            not_before: at(120),
            error: "offline".to_owned(),
            at: at(61),
        }))
        .await?;

    let third = claim(subject, "r1", &[EffectKind::Wake], at(120)).await?;
    assert_eq!(third.claimed.len(), 0);
    assert_eq!(third.dead.len(), 1);
    assert_eq!(third.dead[0].id, id);

    let record = fetch(subject, id).await?;
    assert_eq!(record.status, "dead");
    assert_eq!(record.last_error.as_deref(), Some("attempt limit reached"));
    assert_eq!(record.completed_at, Some(at(120)));
    assert_eq!(record.claim_token, None);
    assert_eq!(record.lease_until, None);
    assert_eq!(record.runner_id, None);
    Ok(())
}

/// An expired lease on an effect with no attempts left dies instead of being reclaimed.
pub async fn expired_lease_beyond_attempt_limit_dies_instead_of_reclaim(
    subject: &dyn UowSubject,
) -> ContractResult {
    let mut effect = draft(EffectKind::Wake, 0);
    effect.max_attempts = 1;
    let id = store(subject, effect).await?;
    let held = claim(subject, "r1", &[EffectKind::Wake], at(0)).await?;
    assert_eq!(held.claimed.len(), 1);

    let later = claim(subject, "r2", &[EffectKind::Wake], at(LEASE + 1)).await?;
    assert_eq!(later.claimed.len(), 0);
    assert_eq!(later.dead.len(), 1);
    assert_eq!(later.dead[0].id, id);

    let record = fetch(subject, id).await?;
    assert_eq!(record.status, "dead");
    assert!(
        record
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("lease expired")),
        "the row says why it died: {:?}",
        record.last_error
    );
    assert_eq!(record.claim_token, None);
    assert_eq!(record.lease_until, None);
    assert_eq!(record.runner_id, None);
    Ok(())
}
