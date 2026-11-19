//! The in-memory policy repository and the applier for `StateWrite::Policy`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::policy_store::{
    AckRecord, DraftGuard, PolicyStatus, PolicyVersionRecord, PolicyWrite, ShadowCandidate,
    ShadowFilter, ShadowRecord, ShadowState, ShadowStatus, ShadowSummary, StoredRule,
};
use domovoi_core::ports::{
    AggregateRef, CommitError, Expectation, Observed, Page, Paged, PolicyRepo, StoreError,
};
use domovoi_core::{ExecutionId, PolicyVersionId};
use jiff::Timestamp;

use super::{InMemoryStore, Key, State, paginate};

fn conflict(id: PolicyVersionId, expected: Expectation, found: Observed) -> CommitError {
    CommitError::Conflict {
        aggregate: AggregateRef::policy(id),
        expected,
        found,
    }
}

fn nanos(at: Timestamp) -> i128 {
    at.as_nanosecond()
}

fn count(rules: &[StoredRule], pick: impl Fn(&StoredRule) -> bool) -> u32 {
    u32::try_from(rules.iter().filter(|rule| pick(rule)).count()).unwrap_or(u32::MAX)
}

/// The position and revision of the stored draft.
fn draft_index(state: &State) -> Option<(usize, u64)> {
    state
        .policy_versions
        .iter()
        .position(|(record, _)| record.status == PolicyStatus::Draft)
        .map(|index| (index, state.policy_versions[index].0.revision))
}

/// Requires the stored draft to be `id` at `revision`; returns its position.
fn require_draft(state: &State, id: PolicyVersionId, revision: u64) -> Result<usize, CommitError> {
    match draft_index(state) {
        Some((index, current))
            if state.policy_versions[index].0.id == id && current == revision =>
        {
            Ok(index)
        }
        Some((_, current)) => Err(conflict(
            id,
            Expectation::Version(revision),
            Observed::Version(current),
        )),
        None => Err(conflict(
            id,
            Expectation::Version(revision),
            Observed::Missing,
        )),
    }
}

/// Stores one policy write, with the same refusals as the SQLite applier.
pub(super) fn apply(state: &mut State, write: &PolicyWrite) -> Result<(), CommitError> {
    match write {
        PolicyWrite::SaveDraft {
            id,
            expected,
            based_on,
            rules,
            rules_hash,
            actor,
            at,
        } => {
            let total = count(rules, |_| true);
            let locked = count(rules, |rule| rule.locked);
            let shadow = count(rules, |rule| rule.shadow);
            let mut ordinals: Vec<u32> = rules.iter().map(|rule| rule.ordinal).collect();
            ordinals.sort_unstable();
            ordinals.dedup();
            let mut ids: Vec<&str> = rules.iter().map(|rule| rule.rule_id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            if ordinals.len() != rules.len() || ids.len() != rules.len() {
                return Err(CommitError::Constraint(
                    "policy rule ids and ordinals must be unique".to_owned(),
                ));
            }
            match expected {
                DraftGuard::Absent => {
                    if let Some((_, current)) = draft_index(state) {
                        return Err(conflict(*id, Expectation::New, Observed::Version(current)));
                    }
                    state.policy_versions.push((
                        PolicyVersionRecord {
                            id: *id,
                            status: PolicyStatus::Draft,
                            number: None,
                            revision: 1,
                            based_on: *based_on,
                            rules_hash: rules_hash.clone(),
                            rule_count: total,
                            locked_count: locked,
                            shadow_count: shadow,
                            created_at: *at,
                            created_by: actor.clone(),
                            updated_at: *at,
                            updated_by: actor.clone(),
                            published_at: None,
                            published_by: None,
                        },
                        rules.clone(),
                    ));
                }
                DraftGuard::Revision(revision) => {
                    let index = require_draft(state, *id, *revision)?;
                    let (record, stored) = &mut state.policy_versions[index];
                    record.revision += 1;
                    // A save re-bases the draft on the version active now.
                    record.based_on = based_on.or(record.based_on);
                    record.rules_hash.clone_from(rules_hash);
                    record.rule_count = total;
                    record.locked_count = locked;
                    record.shadow_count = shadow;
                    record.updated_at = *at;
                    record.updated_by = actor.clone();
                    *stored = rules.clone();
                }
            }
            Ok(())
        }
        PolicyWrite::DiscardDraft {
            id,
            expected_revision,
        } => {
            let index = require_draft(state, *id, *expected_revision)?;
            state.policy_versions.remove(index);
            Ok(())
        }
        PolicyWrite::Publish {
            id,
            expected_revision,
            number,
            acknowledgements,
            actor,
            at,
        } => {
            let index = require_draft(state, *id, *expected_revision)?;
            let highest = state
                .policy_versions
                .iter()
                .filter_map(|(record, _)| record.number)
                .max()
                .unwrap_or(0);
            if *number != highest + 1 {
                return Err(conflict(
                    *id,
                    Expectation::Version(u64::from(*number).saturating_sub(1)),
                    Observed::Version(u64::from(highest)),
                ));
            }
            let (record, _) = &mut state.policy_versions[index];
            record.status = PolicyStatus::Published;
            record.number = Some(*number);
            record.published_at = Some(*at);
            record.published_by = Some(actor.clone());
            record.updated_at = *at;
            record.updated_by = actor.clone();
            let mut seen = Vec::new();
            for ack in acknowledgements {
                if seen.contains(&&ack.finding_key) {
                    return Err(CommitError::Constraint(
                        "duplicate policy acknowledgement".to_owned(),
                    ));
                }
                seen.push(&ack.finding_key);
            }
            state.policy_acks.insert(*id, acknowledgements.clone());
            Ok(())
        }
        PolicyWrite::RecordShadow {
            records, at, error, ..
        } => {
            let mut inserted = [0_u64; 3];
            for record in records {
                if state.shadow.contains_key(&record.execution) {
                    continue;
                }
                state.shadow.insert(record.execution, record.clone());
                inserted[match record.status {
                    ShadowStatus::Evaluated => 0,
                    ShadowStatus::Unevaluable => 1,
                    ShadowStatus::Skipped => 2,
                }] += 1;
            }
            if let Some(error) = error {
                state.shadow_state.last_error = Some(error.clone());
            } else {
                let bookkeeping = &mut state.shadow_state;
                bookkeeping.last_sweep_at = Some(*at);
                bookkeeping.last_error = None;
                bookkeeping.total_evaluated += inserted[0];
                bookkeeping.total_unevaluable += inserted[1];
                bookkeeping.total_skipped += inserted[2];
            }
            Ok(())
        }
    }
}

fn differs(record: &ShadowRecord) -> bool {
    record.enforced_outcome != record.shadow_outcome
}

fn matches(filter: &ShadowFilter, record: &ShadowRecord) -> bool {
    filter.host.is_none_or(|host| record.host == host)
        && filter
            .contract
            .as_deref()
            .is_none_or(|contract| record.contract == contract)
        && filter.status.is_none_or(|status| record.status == status)
        && filter
            .enforced_outcome
            .as_deref()
            .is_none_or(|outcome| record.enforced_outcome.as_deref() == Some(outcome))
        && (!filter.differing || differs(record))
        && filter
            .since
            .is_none_or(|since| record.evaluated_at >= since)
        && filter.until.is_none_or(|until| record.evaluated_at < until)
}

#[async_trait]
impl PolicyRepo for InMemoryStore {
    async fn active(&self) -> Result<Option<PolicyVersionRecord>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .policy_versions
            .iter()
            .filter(|(record, _)| record.number.is_some())
            .max_by_key(|(record, _)| record.number)
            .map(|(record, _)| record.clone()))
    }

    async fn version(&self, number: u32) -> Result<Option<PolicyVersionRecord>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .policy_versions
            .iter()
            .find(|(record, _)| record.number == Some(number))
            .map(|(record, _)| record.clone()))
    }

    async fn by_id(&self, id: PolicyVersionId) -> Result<Option<PolicyVersionRecord>, StoreError> {
        let state = self.begin()?;
        Ok(state
            .policy_versions
            .iter()
            .find(|(record, _)| record.id == id)
            .map(|(record, _)| record.clone()))
    }

    async fn versions(&self, page: &Page) -> Result<Paged<PolicyVersionRecord>, StoreError> {
        let state = self.begin()?;
        let items: Vec<(Key, PolicyVersionRecord)> = state
            .policy_versions
            .iter()
            .filter_map(|(record, _)| {
                record
                    .number
                    .map(|number| ((i128::from(number), String::new()), record.clone()))
            })
            .collect();
        paginate(items, page)
    }

    async fn draft(&self) -> Result<Option<PolicyVersionRecord>, StoreError> {
        let state = self.begin()?;
        Ok(draft_index(&state).map(|(index, _)| state.policy_versions[index].0.clone()))
    }

    async fn rules(&self, id: PolicyVersionId) -> Result<Vec<StoredRule>, StoreError> {
        let state = self.begin()?;
        let mut rules = state
            .policy_versions
            .iter()
            .find(|(record, _)| record.id == id)
            .map(|(_, rules)| rules.clone())
            .unwrap_or_default();
        rules.sort_by_key(|rule| rule.ordinal);
        Ok(rules)
    }

    async fn acknowledgements(&self, id: PolicyVersionId) -> Result<Vec<AckRecord>, StoreError> {
        let state = self.begin()?;
        let mut acks = state.policy_acks.get(&id).cloned().unwrap_or_default();
        acks.sort_by(|a, b| a.finding_key.cmp(&b.finding_key));
        Ok(acks)
    }

    async fn shadow_candidates(&self, limit: u16) -> Result<Vec<ShadowCandidate>, StoreError> {
        let state = self.begin()?;
        let mut candidates: Vec<ShadowCandidate> = state
            .executions
            .values()
            .filter(|(record, _)| !state.shadow.contains_key(&record.execution))
            .filter_map(|(record, _)| {
                let operation = state.operations.get(&record.operation)?;
                Some(ShadowCandidate {
                    execution: record.execution,
                    operation: record.operation,
                    host: record.host,
                    contract: record.contract.clone(),
                    contract_version: record.contract_version,
                    check: record.check.clone(),
                    target: record.target.clone(),
                    args: record.args.clone(),
                    pinned: operation.pinned.clone(),
                    created_at: record.created_at,
                })
            })
            .collect();
        candidates.sort_by(|a, b| {
            (a.created_at, a.execution.to_string()).cmp(&(b.created_at, b.execution.to_string()))
        });
        candidates.truncate(usize::from(limit));
        Ok(candidates)
    }

    async fn shadow(&self, execution: ExecutionId) -> Result<Option<ShadowRecord>, StoreError> {
        let state = self.begin()?;
        Ok(state.shadow.get(&execution).cloned())
    }

    async fn shadow_records(
        &self,
        filter: &ShadowFilter,
        page: &Page,
    ) -> Result<Paged<ShadowRecord>, StoreError> {
        let state = self.begin()?;
        let items: Vec<(Key, ShadowRecord)> = state
            .shadow
            .values()
            .filter(|record| matches(filter, record))
            .map(|record| {
                (
                    (nanos(record.evaluated_at), record.execution.to_string()),
                    record.clone(),
                )
            })
            .collect();
        paginate(items, page)
    }

    async fn shadow_summary(&self, since: Timestamp) -> Result<ShadowSummary, StoreError> {
        let state = self.begin()?;
        let mut summary = ShadowSummary {
            since,
            total: 0,
            by_status: BTreeMap::new(),
            by_enforced_outcome: BTreeMap::new(),
            degraded: 0,
            differing: 0,
        };
        for record in state.shadow.values().filter(|r| r.evaluated_at >= since) {
            summary.total += 1;
            *summary
                .by_status
                .entry(record.status.as_str().to_owned())
                .or_insert(0) += 1;
            if let Some(outcome) = &record.enforced_outcome {
                *summary
                    .by_enforced_outcome
                    .entry(outcome.clone())
                    .or_insert(0) += 1;
            }
            summary.degraded += u64::from(record.degraded);
            summary.differing += u64::from(differs(record));
        }
        Ok(summary)
    }

    async fn shadow_state(&self) -> Result<ShadowState, StoreError> {
        let state = self.begin()?;
        Ok(state.shadow_state.clone())
    }
}
