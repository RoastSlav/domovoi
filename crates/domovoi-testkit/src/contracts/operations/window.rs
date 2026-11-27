//! Contract for the dispatch-window read of the operation repository.

use domovoi_core::HostId;

use super::reads::{epoch, new_execution};
use super::{commit, execution, operation, seed};
use crate::build::at;
use crate::contracts::{ContractResult, UowSubject};

/// The window includes both ends, is per host, ignores executions never dispatched and lists
/// the oldest dispatch first.
pub async fn dispatched_between_lists_one_hosts_dispatches_in_the_window(
    subject: &dyn UowSubject,
) -> ContractResult {
    let (here, there) = (HostId::new(), HostId::new());
    let op = operation(here, 1)?;
    let mut steps = Vec::new();
    let mut writes = Vec::new();
    let plan = [
        (here, Some(300)),
        (here, Some(100)),
        (here, Some(200)),
        (here, Some(400)),
        (here, None),
        (there, Some(200)),
    ];
    for (ordinal, (host, secs)) in plan.into_iter().enumerate() {
        let ordinal = u32::try_from(ordinal).map_err(super::setup)?;
        let (step, mut row) = execution(op.id, ordinal, host)?;
        row.dispatched = match secs {
            Some(secs) => Some((epoch(1)?, at(secs))),
            None => None,
        };
        steps.push(step);
        writes.push(new_execution(row));
    }
    seed(subject, &op, &steps).await?;
    commit(subject, writes).await?;

    let found = subject
        .operations()
        .dispatched_between(here, at(100), at(300))
        .await?;
    let times: Vec<_> = found
        .iter()
        .filter_map(|row| row.dispatched.map(|(_, when)| when))
        .collect();
    assert_eq!(times, vec![at(100), at(200), at(300)]);
    assert!(found.iter().all(|row| row.host == here));
    let none = subject
        .operations()
        .dispatched_between(here, at(401), at(500))
        .await?;
    assert!(none.is_empty());
    Ok(())
}
