//! `systemd.read` and `systemd.write`: the identity of a unit and its restart.
//!
//! The group gates, the allowlist and the protected set live here and in `domovoi-core`'s pure
//! identity algorithm. This module owns the only `systemctl` call the helper makes.

mod adapter;
mod budget;
mod identity;
mod parse;
mod restart;

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_config::HelperConfig;
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse, UnavailableReason};
use domovoi_core::identity::RefusalToken;
use domovoi_core::isolation::{PlatformRecordState, PlatformTier};
use domovoi_core::ports::{Clock, HostInspect, ProcessExecutor};
use domovoi_core::unit::{SystemdView, UnitFs, UnitId, UnitName, UnitPolicy, UnitRefusal};
use jiff::SignedDuration;

pub use adapter::{BusctlView, DiskFs, SEARCH_DIRS, dropin_dir_names};
pub use budget::{Budget, RESTARTS_FILE};
pub use restart::restart_argv;

use crate::reads::{gate, limits};
use crate::serve::{ServeContext, read_platform_record};

/// The largest answer of a unit operation, in bytes of output text.
pub const MAX_OUTPUT_BYTES: usize = 16 * 1024;

/// What a unit operation needs: the manager, the disk, a process runner and the rules.
pub struct UnitEnv<'a> {
    /// Reads the manager.
    pub sys: &'a dyn SystemdView,
    /// Reads unit files.
    pub fs: &'a dyn UnitFs,
    /// Runs `systemctl restart`.
    pub exec: &'a dyn ProcessExecutor,
    /// Vets programs and resolves users.
    pub inspect: &'a dyn HostInspect,
    /// The clock the budget runs on.
    pub clock: &'a dyn Clock,
    /// The helper's state directory.
    pub state_dir: &'a Path,
    /// The allowlist and the protected set.
    pub policy: &'a UnitPolicy,
    /// The longest one restart command may run.
    pub timeout: SignedDuration,
    /// `[limits] mutations_per_hour`; `0` accepts no write.
    pub mutations_per_hour: u32,
}

/// The helper's rules for units, from `helper.toml`.
///
/// # Errors
/// A `restart_units` entry that is not a valid unit id.
pub fn policy_of(config: &HelperConfig) -> Result<UnitPolicy, UnitRefusal> {
    let allow = config
        .systemd
        .restart_units
        .iter()
        .map(|entry| UnitId::parse(entry).map_err(|_| UnitRefusal::ReadFailed))
        .collect::<Result<_, _>>()?;
    Ok(UnitPolicy {
        allow,
        protected: config.protected_unit_set(),
    })
}

/// The tier gate for a write: the group must be configured, the platform record must be sound
/// and not blocked, and an untested platform needs the owner's per-host opt-in.
pub fn write_gate(
    config: &HelperConfig,
    state: PlatformRecordState,
    tier: PlatformTier,
) -> Result<(), UnavailableReason> {
    if !config
        .configured_groups()
        .contains(&HelperGroup::SystemdWrite)
    {
        return Err(UnavailableReason::GroupDisabled);
    }
    match (state, tier) {
        (PlatformRecordState::Ok, PlatformTier::Supported) => Ok(()),
        (PlatformRecordState::Ok, PlatformTier::Untested) if config.write_opt_in() => Ok(()),
        (PlatformRecordState::Ok, PlatformTier::Untested) => {
            Err(UnavailableReason::WriteGroupNeedsOptIn)
        }
        _ => Err(UnavailableReason::PlatformBlocked),
    }
}

/// An allowlist entry whose canonical id differs from what was written is a config error
/// naming the id. Returns the first such entry as `(written, canonical)`.
pub async fn alias_entry(
    sys: &dyn SystemdView,
    policy: &UnitPolicy,
) -> Result<Option<(String, String)>, UnitRefusal> {
    for entry in &policy.allow {
        let name = UnitName::parse(entry.as_str()).map_err(|_| UnitRefusal::ReadFailed)?;
        let props = sys
            .load_unit(&name)
            .await
            .map_err(|_| UnitRefusal::ReadFailed)?;
        if props.id != entry.as_str() {
            return Ok(Some((entry.to_string(), props.id)));
        }
    }
    Ok(None)
}

fn refused(refusal: &UnitRefusal) -> HelperResponse {
    HelperResponse::UnitRefused {
        token: refusal.token().to_owned(),
        detail: refusal
            .detail()
            .map(|d| limits::cut_utf8(&d, 200).to_owned()),
    }
}

fn answer(result: Result<BTreeMap<String, String>, UnitRefusal>) -> HelperResponse {
    match result {
        Ok(outputs) => {
            let size: usize = outputs.iter().map(|(k, v)| k.len() + v.len()).sum();
            if size > MAX_OUTPUT_BYTES {
                refused(&UnitRefusal::ReadFailed)
            } else {
                HelperResponse::Outputs { outputs }
            }
        }
        Err(refusal) => refused(&refusal),
    }
}

/// Answers one unit request. Reads pass the read gate; a restart passes the tier gate.
pub async fn serve(
    request: &HelperRequest,
    ctx: &ServeContext,
    peer_uid: Option<u32>,
) -> HelperResponse {
    let Ok(config) = &ctx.config else {
        return HelperResponse::Refused {
            token: RefusalToken::HelperConfigInvalid,
        };
    };
    let (group, op) = match request {
        HelperRequest::UnitIdentity { .. } => (HelperGroup::SystemdRead, "unit_identity"),
        HelperRequest::UnitRestart { .. } => (HelperGroup::SystemdWrite, "unit_restart"),
        _ => {
            return HelperResponse::Unavailable {
                group: HelperGroup::SystemdRead,
                reason: UnavailableReason::NotImplemented,
            };
        }
    };
    let (record_state, record) = read_platform_record(ctx.inspect.as_ref(), &ctx.dirs);
    let gated = if group.is_write() {
        write_gate(
            config,
            record_state,
            record.map_or(PlatformTier::Blocked, |r| r.tier),
        )
        .err()
    } else {
        gate::refusal(group, config, ctx.inspect.as_ref(), &ctx.dirs)
    };
    if let Some(reason) = gated {
        tracing::info!(op, group = %group, peer_uid, "unit operation refused by the gate");
        return HelperResponse::Unavailable { group, reason };
    }
    let Some(exec) = ctx.executor.clone() else {
        return refused(&UnitRefusal::ReadFailed);
    };
    let Ok(policy) = policy_of(config) else {
        return HelperResponse::Refused {
            token: RefusalToken::HelperConfigInvalid,
        };
    };
    let view = BusctlView::new(exec.clone(), ctx.inspect.clone());
    let timeout = limits::request_deadline(config);
    let env = UnitEnv {
        sys: &view,
        fs: &DiskFs,
        exec: exec.as_ref(),
        inspect: ctx.inspect.as_ref(),
        clock: ctx.clock.as_ref(),
        state_dir: &ctx.dirs.helper_dir,
        policy: &policy,
        timeout,
        mutations_per_hour: config.limits.mutations_per_hour,
    };
    run(request, &env, timeout, peer_uid, op).await
}

/// Runs the operation under the request deadline and logs one line.
pub async fn run(
    request: &HelperRequest,
    env: &UnitEnv<'_>,
    timeout: SignedDuration,
    peer_uid: Option<u32>,
    op: &'static str,
) -> HelperResponse {
    let work = async {
        match alias_entry(env.sys, env.policy).await {
            Ok(None) => {}
            Ok(Some((written, canonical))) => {
                tracing::error!(%written, %canonical, "restart_units holds an alias; use the canonical id");
                return Err(None);
            }
            Err(refusal) => return Err(Some(refusal)),
        }
        match request {
            HelperRequest::UnitIdentity { name } => {
                identity::identity(env, name).await.map_err(Some)
            }
            HelperRequest::UnitRestart { id, identity_hash } => {
                restart::restart(env, id, *identity_hash)
                    .await
                    .map_err(Some)
            }
            _ => Err(Some(UnitRefusal::ReadFailed)),
        }
    };
    let result = limits::within(env.clock, timeout, work).await;
    let response = match result {
        None => refused(&UnitRefusal::ReadFailed),
        Some(Err(None)) => HelperResponse::Refused {
            token: RefusalToken::HelperConfigInvalid,
        },
        Some(Err(Some(refusal))) => refused(&refusal),
        Some(Ok(outputs)) => answer(Ok(outputs)),
    };
    let outcome = match &response {
        HelperResponse::UnitRefused { token, .. } => token.as_str(),
        _ => "ok",
    };
    tracing::info!(op, outcome, peer_uid, "unit operation served");
    response
}
