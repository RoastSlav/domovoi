//! The closed set of operations and the helper's view of the platform record.

use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::attest::Nonce32;
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse, UnavailableReason};
use domovoi_core::identity::HostDirs;
use domovoi_core::isolation::{
    PLATFORM_RECORD_FORMAT, PlatformRecord, PlatformRecordState, PlatformTier,
};
use domovoi_core::ports::{HostInspect, ProcessError, ProcessExecutor, ProcessOutput, ProcessSpec};

use super::ServeContext;
use crate::attest::{AttestEnv, AttestOutcome, public_key, safety_attest};

/// The platform record's file name inside the helper's state directory.
const PLATFORM_FILE: &str = "platform.json";

/// Reads `platform.json` and says whether it can be trusted.
///
/// `Missing` covers an absent, unreadable or unparsable file and a format this build does not
/// know; `Stale` means the record was written on a different kernel than the one running now (or
/// the running kernel cannot be determined). Only `Ok` carries the record.
pub fn read_platform_record(
    inspect: &dyn HostInspect,
    dirs: &HostDirs,
) -> (PlatformRecordState, Option<PlatformRecord>) {
    let path = dirs.helper_dir.join(PLATFORM_FILE);
    let record = inspect
        .read_text(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<PlatformRecord>(&text).ok())
        .filter(|record| record.format == PLATFORM_RECORD_FORMAT);
    let Some(record) = record else {
        return (PlatformRecordState::Missing, None);
    };
    match inspect.kernel_release() {
        Ok(running) if running.trim() == record.kernel_release.trim() => {
            (PlatformRecordState::Ok, Some(record))
        }
        _ => (PlatformRecordState::Stale, None),
    }
}

/// Why `group` is not served, given the enabled set, the recorded platform and the owner's switch.
///
/// A group that is not enabled is `GroupDisabled`. A write group is `PlatformBlocked` unless the
/// record is `Ok` and the tier is not Blocked, and `WriteGroupNeedsOptIn` on an untested platform
/// without the opt-in. Everything else is `NotImplemented`.
#[must_use]
pub fn unavailable_reason(
    group: HelperGroup,
    enabled: &[HelperGroup],
    record: PlatformRecordState,
    tier: PlatformTier,
    write_opt_in: bool,
) -> UnavailableReason {
    if group.is_write() {
        return match (record, tier) {
            (PlatformRecordState::Ok, PlatformTier::Untested) if !write_opt_in => {
                UnavailableReason::WriteGroupNeedsOptIn
            }
            (PlatformRecordState::Ok, PlatformTier::Supported | PlatformTier::Untested) => {
                UnavailableReason::NotImplemented
            }
            _ => UnavailableReason::PlatformBlocked,
        };
    }
    if enabled.contains(&group) {
        UnavailableReason::NotImplemented
    } else {
        UnavailableReason::GroupDisabled
    }
}

/// Answers one decoded request. The request type has no write variant and no free-form field.
pub(super) async fn dispatch(
    request: &HelperRequest,
    ctx: &ServeContext,
    peer_uid: Option<u32>,
) -> HelperResponse {
    match request {
        HelperRequest::Health => HelperResponse::Health {
            protocol: domovoi_core::helper::HELPER_PROTOCOL,
            build: ctx.build.clone(),
            pid: std::process::id(),
        },
        HelperRequest::Groups => groups(ctx),
        HelperRequest::SafetyAttest { nonce } => attest(*nonce, ctx).await,
        HelperRequest::SafetyPublicKey => attest_public_key(ctx),
        HelperRequest::UnitIdentity { .. } | HelperRequest::UnitRestart { .. } => {
            crate::systemd::serve(request, ctx, peer_uid).await
        }
        HelperRequest::FirewallApply { .. }
        | HelperRequest::FirewallConfirm { .. }
        | HelperRequest::FirewallRollback { .. }
        | HelperRequest::FirewallListBlocks => crate::nft::serve(request, ctx, peer_uid).await,
        read => crate::reads::serve(read, ctx, peer_uid).await,
    }
}

/// An executor for a context that was given none: every program is missing, so every route read
/// fails and the attestation is a signed incomplete report rather than a guess.
struct NoProcess;

#[async_trait]
impl ProcessExecutor for NoProcess {
    async fn run(&self, _spec: ProcessSpec) -> Result<ProcessOutput, ProcessError> {
        Err(ProcessError::NotFound)
    }
}

/// The attestation environment of this connection, or the answer to give when the group is off.
fn attest_env(ctx: &ServeContext) -> Result<AttestEnv, HelperResponse> {
    match &ctx.config {
        Ok(config)
            if config
                .effective_groups()
                .contains(&HelperGroup::SafetyAttest) => {}
        _ => return Err(unavailable(HelperGroup::SafetyAttest, ctx)),
    }
    let exec: Arc<dyn ProcessExecutor> =
        ctx.executor.clone().unwrap_or_else(|| Arc::new(NoProcess));
    Ok(AttestEnv::new(
        ctx.dirs.clone(),
        ctx.inspect.clone(),
        ctx.clock.clone(),
        exec,
    ))
}

async fn attest(nonce: domovoi_core::helper::Nonce, ctx: &ServeContext) -> HelperResponse {
    let env = match attest_env(ctx) {
        Ok(env) => env,
        Err(response) => return response,
    };
    match safety_attest(&env, Nonce32::from(nonce)).await {
        AttestOutcome::Signed(attestation) => HelperResponse::SafetyAttested { attestation },
        AttestOutcome::Unsigned(token) => HelperResponse::SafetyUnsigned { token },
    }
}

fn attest_public_key(ctx: &ServeContext) -> HelperResponse {
    let env = match attest_env(ctx) {
        Ok(env) => env,
        Err(response) => return response,
    };
    match public_key(&env) {
        Ok(key) => HelperResponse::SafetyPublicKey {
            public_key: domovoi_core::attest::Hash32::from_bytes(key).to_string(),
        },
        Err(token) => HelperResponse::SafetyUnsigned { token },
    }
}

fn groups(ctx: &ServeContext) -> HelperResponse {
    let Ok(config) = &ctx.config else {
        return HelperResponse::Refused {
            token: domovoi_core::identity::RefusalToken::HelperConfigInvalid,
        };
    };
    let (record_state, record) = read_platform_record(ctx.inspect.as_ref(), &ctx.dirs);
    HelperResponse::Groups {
        enabled: config.effective_groups(),
        ignored: config.ignored_groups(),
        write_opt_in: config.write_opt_in(),
        platform_tier: record.map_or(PlatformTier::Blocked, |r| r.tier),
        platform_record: record_state,
    }
}

fn unavailable(group: HelperGroup, ctx: &ServeContext) -> HelperResponse {
    let Ok(config) = &ctx.config else {
        return HelperResponse::Refused {
            token: domovoi_core::identity::RefusalToken::HelperConfigInvalid,
        };
    };
    let (record_state, record) = read_platform_record(ctx.inspect.as_ref(), &ctx.dirs);
    let tier = record.map_or(PlatformTier::Blocked, |r| r.tier);
    HelperResponse::Unavailable {
        group,
        reason: unavailable_reason(
            group,
            &config.effective_groups(),
            record_state,
            tier,
            config.write_opt_in(),
        ),
    }
}
