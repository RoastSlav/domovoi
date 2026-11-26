//! The read operations of the helper: `docker.read`, `smart.read`, `kernel.read` and `net.read`.
//!
//! [`serve`] is the only entry point. It checks the group and the platform, runs the operation
//! under the request deadline on the injected clock, maps every failure to a token, keeps the
//! answer inside one frame and writes one log line. It holds no state between requests.

pub mod docker;
pub mod gate;
pub mod limits;
mod ops;
pub mod tools;

use domovoi_core::helper::read::{ReadFailure, ReadToken};
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse};

use crate::serve::ServeContext;

/// The operation name logged for `request`; a fixed word, never a slot value.
fn op_name(request: &HelperRequest) -> &'static str {
    match request {
        HelperRequest::DockerProbe => "docker_probe",
        HelperRequest::DockerList => "docker_list",
        HelperRequest::DockerEvents { .. } => "docker_events",
        HelperRequest::DockerInspect { .. } => "docker_inspect",
        HelperRequest::DockerLogs { .. } => "docker_logs",
        HelperRequest::SmartScan => "smart_scan",
        HelperRequest::SmartReport { .. } => "smart_report",
        HelperRequest::KernelLog { .. } => "kernel_log",
        HelperRequest::NetListeners => "net_listeners",
        _ => "other",
    }
}

/// Answers one read request. `peer_uid` is only logged.
pub async fn serve(
    request: &HelperRequest,
    ctx: &ServeContext,
    peer_uid: Option<u32>,
) -> HelperResponse {
    let Some(group) = request.group() else {
        return HelperResponse::Unavailable {
            group: HelperGroup::DockerRead,
            reason: domovoi_core::helper::UnavailableReason::NotImplemented,
        };
    };
    let Ok(config) = &ctx.config else {
        return HelperResponse::Refused {
            token: domovoi_core::identity::RefusalToken::HelperConfigInvalid,
        };
    };
    let op = op_name(request);
    if let Some(reason) = gate::refusal(group, config, ctx.inspect.as_ref(), &ctx.dirs) {
        tracing::info!(op, group = %group, outcome = %reason_word(reason), peer_uid, "read refused");
        return HelperResponse::Unavailable { group, reason };
    }
    let started = ctx.clock.now();
    let limit = limits::request_deadline(config);
    let answer = limits::within(
        ctx.clock.as_ref(),
        limit,
        ops::run(request, group, config, ctx),
    )
    .await
    .unwrap_or_else(|| ops::failed(group, ReadFailure::bare(ReadToken::TimedOut)));
    let answer = limits::fit_frame(answer);
    log_outcome(
        op,
        group,
        &answer,
        ctx.clock.now().duration_since(started),
        peer_uid,
    );
    answer
}

fn reason_word(reason: domovoi_core::helper::UnavailableReason) -> &'static str {
    use domovoi_core::helper::UnavailableReason as R;
    match reason {
        R::GroupDisabled => "group_disabled",
        R::NotImplemented => "not_implemented",
        R::PlatformBlocked => "platform_blocked",
        R::WriteGroupNeedsOptIn => "write_group_needs_opt_in",
    }
}

/// One line per request: operation, group, outcome token or `ok`, duration, truncated flag and the
/// peer's uid. No body, no slot value, no detail.
fn log_outcome(
    op: &'static str,
    group: HelperGroup,
    answer: &HelperResponse,
    took: jiff::SignedDuration,
    peer_uid: Option<u32>,
) {
    let (outcome, truncated) = match answer {
        HelperResponse::ReadFailed { token, .. } => (token.as_str(), false),
        HelperResponse::DockerContainers { truncated, .. }
        | HelperResponse::DockerEvents { truncated, .. }
        | HelperResponse::DockerLogs { truncated, .. }
        | HelperResponse::SmartReport { truncated, .. }
        | HelperResponse::KernelLog { truncated, .. }
        | HelperResponse::NetListeners { truncated, .. } => ("ok", *truncated),
        _ => ("ok", false),
    };
    let millis = took.as_millis();
    if matches!(
        answer,
        HelperResponse::ReadFailed {
            token: ReadToken::DeviceNotAllowed | ReadToken::EngineApiRefused,
            ..
        }
    ) {
        tracing::warn!(op, group = %group, outcome, millis = %millis, truncated, peer_uid, "read refused");
    } else {
        tracing::info!(op, group = %group, outcome, millis = %millis, truncated, peer_uid, "read served");
    }
}
