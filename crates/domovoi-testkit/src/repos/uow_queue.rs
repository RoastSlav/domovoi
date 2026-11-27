//! The in-memory effect queue: claiming due work and reverting claims at start.

use async_trait::async_trait;
use domovoi_core::EffectId;
use domovoi_core::ports::{
    AggregateKind, ClaimBatch, ClaimToken, ClaimedEffect, EffectKind, EffectQueue, EffectRecord,
    EffectStatus, StoreError,
};
use jiff::{SignedDuration, Timestamp};

use super::{InMemoryStore, State};

/// The stored kind that a lease expiry never reclaims: its outcome is settled by reconciliation.
const DISPATCH: &str = "dispatch";

/// Effects in `state` that a claim pass may take: due `pending` rows first, then `claimed` rows
/// whose lease expired, each group ordered by `not_before` and then insertion order.
fn candidates(
    state: &State,
    kinds: &[EffectKind],
    aggregates: &[AggregateKind],
    now: Timestamp,
) -> (Vec<EffectId>, Vec<EffectId>) {
    let wanted = |kind: &str| kinds.iter().any(|candidate| candidate.as_str() == kind);
    let mut pending: Vec<&EffectRecord> = state
        .effect_order
        .iter()
        .filter_map(|id| state.effects.get(id))
        .filter(|effect| {
            effect.status == EffectStatus::Pending.as_str()
                && effect.not_before <= now
                && wanted(&effect.kind)
                && aggregates.contains(&effect.aggregate.kind)
        })
        .collect();
    pending.sort_by_key(|effect| effect.not_before);
    let mut expired: Vec<&EffectRecord> = state
        .effect_order
        .iter()
        .filter_map(|id| state.effects.get(id))
        .filter(|effect| {
            effect.status == EffectStatus::Claimed.as_str()
                && effect.kind != DISPATCH
                && wanted(&effect.kind)
                && aggregates.contains(&effect.aggregate.kind)
                && effect.lease_until.is_some_and(|until| until <= now)
        })
        .collect();
    expired.sort_by_key(|effect| effect.not_before);
    (
        pending.into_iter().map(|effect| effect.id).collect(),
        expired.into_iter().map(|effect| effect.id).collect(),
    )
}

/// Moves a row that has used up its attempts to `dead` and returns the stored record.
fn dead_letter(
    state: &mut State,
    id: EffectId,
    error: &str,
    now: Timestamp,
) -> Option<EffectRecord> {
    let effect = state.effects.get_mut(&id)?;
    effect.status = EffectStatus::Dead.as_str().to_owned();
    effect.last_error = Some(error.to_owned());
    effect.completed_at = Some(now);
    effect.claim_token = None;
    effect.lease_until = None;
    effect.runner_id = None;
    Some(effect.clone())
}

/// Claims a row under a fresh token and returns the stored record with that token.
fn claim(
    state: &mut State,
    id: EffectId,
    runner: &str,
    now: Timestamp,
    lease: SignedDuration,
) -> Option<ClaimedEffect> {
    let token = ClaimToken::new();
    let effect = state.effects.get_mut(&id)?;
    effect.status = EffectStatus::Claimed.as_str().to_owned();
    effect.claim_token = Some(token);
    effect.claim_generation = effect.claim_generation.saturating_add(1);
    effect.lease_until = Some(now.checked_add(lease).unwrap_or(now));
    effect.runner_id = Some(runner.to_owned());
    effect.attempt = effect.attempt.saturating_add(1);
    Some(ClaimedEffect {
        record: effect.clone(),
        token,
    })
}

#[async_trait]
impl EffectQueue for InMemoryStore {
    async fn claim_due(
        &self,
        runner: &str,
        kinds: &[EffectKind],
        aggregates: &[AggregateKind],
        now: Timestamp,
        lease: SignedDuration,
        limit: u16,
    ) -> Result<ClaimBatch, StoreError> {
        let mut batch = ClaimBatch::default();
        if kinds.is_empty() || aggregates.is_empty() || limit == 0 {
            return Ok(batch);
        }
        let mut state = self.begin()?;
        let (pending, expired) = candidates(&state, kinds, aggregates, now);
        let mut budget = usize::from(limit);
        for (group, error) in [
            (pending, "attempt limit reached"),
            (expired, "lease expired too many times"),
        ] {
            for id in group {
                if budget == 0 {
                    break;
                }
                let Some(row) = state.effects.get(&id) else {
                    continue;
                };
                let exhausted = row.attempt >= row.max_attempts;
                if exhausted {
                    if let Some(record) = dead_letter(&mut state, id, error, now) {
                        batch.dead.push(record);
                        budget = budget.saturating_sub(1);
                    }
                    continue;
                }
                if let Some(claimed) = claim(&mut state, id, runner, now, lease) {
                    batch.claimed.push(claimed);
                    budget = budget.saturating_sub(1);
                }
            }
        }
        Ok(batch)
    }

    async fn revert_claimed_at_start(
        &self,
        aggregates: &[AggregateKind],
    ) -> Result<u64, StoreError> {
        let mut state = self.begin()?;
        let mut reverted = 0;
        for id in state.effect_order.clone() {
            let Some(effect) = state.effects.get_mut(&id) else {
                continue;
            };
            if effect.status != EffectStatus::Claimed.as_str()
                || effect.kind == DISPATCH
                || !aggregates.contains(&effect.aggregate.kind)
            {
                continue;
            }
            effect.status = EffectStatus::Pending.as_str().to_owned();
            effect.claim_token = None;
            effect.claim_generation = effect.claim_generation.saturating_add(1);
            effect.lease_until = None;
            effect.runner_id = None;
            reverted += 1;
        }
        Ok(reverted)
    }

    async fn pull_forward_retry(&self, now: Timestamp) -> Result<u64, StoreError> {
        let mut state = self.begin()?;
        let next = state
            .effect_order
            .iter()
            .filter_map(|id| state.effects.get(id))
            .filter(|e| {
                e.status == EffectStatus::Pending.as_str()
                    && e.kind == EffectKind::BuildContext.as_str()
                    && e.aggregate.kind == AggregateKind::Operation
                    && e.not_before > now
                    && e.dedup_key
                        .as_deref()
                        .is_some_and(|k| k.starts_with("build:") && k.contains(":retry:"))
            })
            .min_by_key(|e| e.not_before)
            .map(|e| e.id);
        let Some(id) = next else { return Ok(0) };
        if let Some(effect) = state.effects.get_mut(&id) {
            effect.not_before = now;
        }
        Ok(1)
    }
}
