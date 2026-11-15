//! The agent-side mutation rate limiter.
//!
//! It reads `accepted` records, so a crash cannot desynchronise the count from the journal.
//! It applies to every mutating execution, human- or policy-labelled; read-only contracts are
//! not counted.

use std::collections::BTreeMap;

use domovoi_config::MutationLimits;
use domovoi_proto::Open;
use domovoi_proto::messages::{RefusalScope, Refused};
use uuid::Uuid;

/// One mutation the journal accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acceptance {
    /// The execution id; counting is by distinct id.
    pub execution_id: Uuid,
    /// The contract id.
    pub contract: String,
    /// The boot the acceptance happened on.
    pub boot_id: String,
    /// Monotonic time at acceptance.
    pub mono_ns: u64,
}

/// One hour in nanoseconds.
pub const WINDOW_NS: u64 = 3_600 * 1_000_000_000;

/// The limiter's answer: allowed, or the refusal to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitCheck {
    /// The mutation may proceed.
    Allowed,
    /// The mutation would exceed a limit.
    Refused(Refused),
}

/// Whether a mutating `contract` may run now.
///
/// `accepted` holds every accepted record, including read-only ones; only entries of mutating
/// contracts are considered. `boot_first_load_mono_ns` is the current boot's first journal
/// load, so an entry from an earlier boot is retained for at least a full window after it.
#[must_use]
pub fn check(
    limits: &MutationLimits,
    accepted: &[Acceptance],
    contract: &str,
    now_mono_ns: u64,
    boot_id: &str,
    boot_first_load_mono_ns: u64,
) -> LimitCheck {
    let mut global: BTreeMap<Uuid, u64> = BTreeMap::new();
    let mut per_contract: BTreeMap<Uuid, u64> = BTreeMap::new();
    for entry in accepted {
        let leave_at = leave_at(entry, boot_id, boot_first_load_mono_ns);
        if leave_at <= now_mono_ns {
            continue;
        }
        global.insert(entry.execution_id, leave_at);
        if entry.contract == contract {
            per_contract.insert(entry.execution_id, leave_at);
        }
    }

    let cap_limit = limits.per_capability.get(contract).copied();
    let cap_exceeds =
        cap_limit.is_some_and(|limit| limit == 0 || per_contract.len() as u64 >= u64::from(limit));
    if cap_exceeds {
        let retry = retry_after(&per_contract, now_mono_ns, cap_limit);
        return LimitCheck::Refused(refusal(
            RefusalScope::Capability {
                contract: contract.to_owned(),
            },
            retry,
        ));
    }

    let global_limit = limits.per_hour;
    if global_limit == 0 || global.len() as u64 >= u64::from(global_limit) {
        let retry = retry_after(&global, now_mono_ns, Some(global_limit));
        return LimitCheck::Refused(refusal(RefusalScope::Global, retry));
    }

    LimitCheck::Allowed
}

fn leave_at(entry: &Acceptance, boot_id: &str, boot_first_load_mono_ns: u64) -> u64 {
    if entry.boot_id == boot_id {
        // A negative age is a corrupted clock, not a fresh window: treat it as age 0.
        entry.mono_ns.saturating_add(WINDOW_NS)
    } else {
        // The entry's age is unknown; it leaves one window after this boot's first load.
        boot_first_load_mono_ns.saturating_add(WINDOW_NS)
    }
}

fn retry_after(counted: &BTreeMap<Uuid, u64>, now_mono_ns: u64, limit: Option<u32>) -> Option<u32> {
    if limit == Some(0) {
        return None;
    }
    let oldest = counted.values().copied().min()?;
    let nanos = oldest.saturating_sub(now_mono_ns);
    let secs = nanos.div_ceil(1_000_000_000).max(1);
    Some(u32::try_from(secs).unwrap_or(u32::MAX))
}

fn refusal(scope: RefusalScope, retry_after: Option<u32>) -> Refused {
    Refused {
        code: Open::new(
            "rate_limited",
            Some(domovoi_proto::messages::RefusalCode::RateLimited),
        ),
        detail: match &scope {
            RefusalScope::Global => "the agent's hourly mutation limit was reached".to_owned(),
            RefusalScope::Capability { contract } => {
                format!("the hourly limit for {contract} was reached")
            }
        },
        retry_after,
        scope: Some(scope),
    }
}
