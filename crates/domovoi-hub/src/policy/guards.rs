//! The permit rate limits: per rule, per host and contract, and for the whole hub.

use domovoi_core::HostId;
use domovoi_core::capability::ContractId;
use domovoi_core::ports::{PermitRefusal, PermitRepo, RateScope};
use jiff::{SignedDuration, Timestamp};

/// The window every permit rate counts in.
const HOUR: SignedDuration = SignedDuration::from_hours(1);

/// Whether issuing one more permit would break a rate limit.
///
/// `rule_max` is the lowest `max_per_hour` the winning rules carry (the evaluator's
/// constraint); `global_max` is `[policy] max_permits_per_hour`, where `0` means no permit is
/// ever issued. A count that cannot be read is `InputsUnavailable`: an unknown rate is not room.
pub(super) async fn check(
    permits: &dyn PermitRepo,
    now: Timestamp,
    host: HostId,
    contract: &ContractId,
    winners: &[String],
    rule_max: Option<u32>,
    global_max: u32,
) -> Result<(), PermitRefusal> {
    let since = now.checked_sub(HOUR).unwrap_or(now);
    let unreadable = |_| PermitRefusal::InputsUnavailable;
    let issued = permits
        .permits_issued_since(since, None)
        .await
        .map_err(unreadable)?;
    if issued >= global_max {
        return Err(PermitRefusal::RateLimited(RateScope::Global));
    }
    let Some(limit) = rule_max else {
        return Ok(());
    };
    let on_host = permits
        .permits_issued_since(since, Some((host, contract)))
        .await
        .map_err(unreadable)?;
    if on_host >= limit {
        return Err(PermitRefusal::RateLimited(RateScope::HostContract));
    }
    for rule in winners {
        let named = permits
            .rule_permits_since(rule, since)
            .await
            .map_err(unreadable)?;
        if named >= limit {
            return Err(PermitRefusal::RateLimited(RateScope::Rule));
        }
    }
    Ok(())
}
