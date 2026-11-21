//! The checks a queued target passes before the worker claims it: is its approval still
//! usable, does the hub offer the package contract on the host, and is there a live session.
//! A target that cannot start yet stays queued; one that can never start is settled as not
//! run straight from queued.

use domovoi_core::capability::Availability;
use domovoi_core::{InstallJob, InstallTarget};

use super::deps::Shared;
use super::error::PackageExecError;
use super::plan::contract_id;

/// What the worker does with a queued target.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Gate {
    /// Claim it and start.
    Go,
    /// Leave it queued; look again on the next wake.
    Wait,
    /// It can never start; settle it as not run.
    Settle(PackageExecError),
}

/// Runs the checks that need no claim.
pub(super) async fn check(shared: &Shared, job: &InstallJob, target: &InstallTarget) -> Gate {
    let deps = &shared.deps;
    if deps.clock.now() >= target.expires_at {
        return Gate::Settle(PackageExecError::Expired);
    }
    let Some(contract) = shared.contract(contract_id(job.kind)) else {
        return Gate::Settle(PackageExecError::NotAdvertised(
            "the hub does not know this package contract".to_owned(),
        ));
    };
    match deps.registry.availability(target.host, &contract.id).await {
        Availability::Available => {}
        Availability::HostUnknown | Availability::HostAbsent { .. } => return Gate::Wait,
        Availability::HostIncompatible { .. } => {
            return Gate::Settle(PackageExecError::AgentIncompatible);
        }
        other => {
            let reason = other
                .reason_code()
                .map_or_else(String::new, |r| format!(": {r}"));
            return Gate::Settle(PackageExecError::NotAdvertised(format!(
                "the host's agent does not offer this ({}{reason})",
                other.code()
            )));
        }
    }
    if deps.caps.observer.observe_snapshot(target.host).is_none() {
        return Gate::Settle(PackageExecError::HostUnavailable);
    }
    Gate::Go
}
