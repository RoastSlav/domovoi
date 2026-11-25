//! Journal questions only containment asks: how many unattended blocks were applied this hour,
//! and whether a block's apply finished.

use std::collections::BTreeSet;

use domovoi_proto::messages::{AuthorizationKind, CapabilityOutcome};
use uuid::Uuid;

use super::limiter;
use super::record::Record;
use super::{Journal, JournalError};

impl Journal {
    /// How many policy-labelled, *mutating* executions of `contract` were accepted inside the last
    /// hour. A block is three executions of one contract; only its `apply` is journaled as
    /// mutating, so `confirm` and `rollback` never spend the hourly budget.
    pub fn policy_accepted_mutating(
        &self,
        contract: &str,
        now_mono_ns: u64,
        boot_id: &str,
    ) -> Result<u32, JournalError> {
        let state = self.lock()?;
        let first_load = state.meta.boot.first_load_mono_ns;
        let ids: BTreeSet<Uuid> = state
            .records
            .iter()
            .filter_map(|record| match record {
                Record::Accepted {
                    execution_id,
                    contract: accepted,
                    authorization_kind: AuthorizationKind::Policy,
                    mutating: true,
                    boot_id: entry_boot,
                    mono_ns,
                    ..
                } if accepted == contract
                    && limiter::active_until(entry_boot, *mono_ns, boot_id, first_load)
                        > now_mono_ns =>
                {
                    Some(*execution_id)
                }
                _ => None,
            })
            .collect();
        Ok(u32::try_from(ids.len()).unwrap_or(u32::MAX))
    }

    /// Whether the journal holds an accepted execution `execution_id` of `contract` whose
    /// durable result is a success: the proof that a block's `apply` ran here.
    pub fn finished_ok(&self, contract: &str, execution_id: Uuid) -> Result<bool, JournalError> {
        let state = self.lock()?;
        let accepted = state.records.iter().any(|record| {
            matches!(record, Record::Accepted { execution_id: id, contract: c, .. }
                if *id == execution_id && c == contract)
        });
        let succeeded = state.records.iter().any(|record| {
            matches!(record, Record::Finished { execution_id: id, result, .. }
                if *id == execution_id
                    && matches!(result.outcome, CapabilityOutcome::Succeeded { .. }))
        });
        Ok(accepted && succeeded)
    }
}
