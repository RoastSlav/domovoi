//! Binding a step's host and target to identities the resolver can prove.

use std::collections::BTreeMap;

use domovoi_core::capability::validate_container_name;
use domovoi_core::capability::{TargetName, TargetUnresolved};
use domovoi_core::ports::TargetResolver;
use domovoi_core::{
    CapabilityContract, HostId, ResolvedTarget, TargetError, TargetKind, TargetQuery,
};

use super::refusal::PlanRefusalCode;

/// A refusal in the making: its code and message.
pub(super) type Refuse = (PlanRefusalCode, String);

/// Resolves the host's own identity.
pub(super) async fn resolve_host(
    resolver: &dyn TargetResolver,
    host: HostId,
    label: &str,
) -> Result<ResolvedTarget, Refuse> {
    resolver
        .resolve(TargetQuery::Host { host })
        .await
        .map_err(|error| map_error(&error, label))
}

/// Resolves the container or unit a contract names in one of its arguments.
///
/// `args[target_arg]` is the name; a `step_target`, when present, must equal it. A container
/// name must pass the name grammar before the resolver is asked.
pub async fn resolve_named_target(
    resolver: &dyn TargetResolver,
    host: HostId,
    contract: &CapabilityContract,
    args: &BTreeMap<String, String>,
    step_target: Option<&str>,
) -> Result<ResolvedTarget, (PlanRefusalCode, String)> {
    let Some(arg) = contract.target_arg.as_deref() else {
        return Err((
            PlanRefusalCode::TargetInvalid,
            "the capability names no target argument".to_owned(),
        ));
    };
    let Some(name) = args.get(arg) else {
        return Err((
            PlanRefusalCode::BadArguments,
            format!("bad arguments: missing required `{arg}`"),
        ));
    };
    if let Some(given) = step_target
        && given != name
    {
        return Err((
            PlanRefusalCode::TargetMismatch,
            format!("target `{given}` does not match `{arg}` (`{name}`)"),
        ));
    }
    let parsed = match contract.target_kind {
        TargetKind::Container => validate_container_name(name),
        _ => TargetName::parse(name),
    };
    let parsed = parsed.map_err(|error| (PlanRefusalCode::TargetInvalid, error.reason))?;
    let query = match contract.target_kind {
        TargetKind::Unit => TargetQuery::Unit { host, name: parsed },
        _ => TargetQuery::Container { host, name: parsed },
    };
    resolver
        .resolve(query)
        .await
        .map_err(|error| map_error(&error, name))
}

/// Maps a resolver error to its fixed refusal; `label` is the name asked about.
pub(super) fn map_error(error: &TargetError, label: &str) -> Refuse {
    match error {
        TargetError::Unresolved { reason, .. } => (
            PlanRefusalCode::TargetUnresolved,
            match reason {
                TargetUnresolved::HostIdentityUnproven => format!(
                    "host `{label}` has no enrolled agent key, so a plan cannot be bound to it"
                ),
                TargetUnresolved::ContainerVanished => {
                    format!("no container named `{label}` on that host now")
                }
                TargetUnresolved::ContainerIdentityUnproven => {
                    format!("container `{label}` has no provable full id")
                }
                TargetUnresolved::UnitIdentityUnproven => {
                    "systemd units cannot be targeted yet".to_owned()
                }
            },
        ),
        TargetError::Ambiguous { count, .. } => (
            PlanRefusalCode::TargetAmbiguous,
            format!("{count} containers are named `{label}`; none was chosen"),
        ),
        TargetError::InvalidName { reason, .. } => {
            (PlanRefusalCode::TargetInvalid, reason.reason.clone())
        }
        TargetError::HostUnavailable { state, .. } => {
            (PlanRefusalCode::HostOffline, state.describe())
        }
    }
}
