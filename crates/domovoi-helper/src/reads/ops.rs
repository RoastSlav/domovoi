//! Runs one read operation and turns its result into a response.

use domovoi_config::HelperConfig;
use domovoi_core::helper::read::{ReadFailure, ReadToken};
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse};
use domovoi_core::ports::DockerReadError;
use jiff::{SignedDuration, Timestamp};

use super::tools::{ToolEnv, kernel, net, smart};
use crate::serve::ServeContext;

/// The answer for a failed read.
pub(super) fn failed(group: HelperGroup, failure: ReadFailure) -> HelperResponse {
    HelperResponse::ReadFailed {
        group,
        token: failure.token,
        detail: failure.detail,
    }
}

/// Maps an engine error to the token the agent sees.
fn docker_failure(err: &DockerReadError) -> ReadFailure {
    ReadFailure::bare(match err {
        DockerReadError::Absent => ReadToken::EngineAbsent,
        DockerReadError::Denied => ReadToken::EngineDenied,
        DockerReadError::Refused => ReadToken::EngineApiRefused,
        DockerReadError::NotFound => ReadToken::TargetNotFound,
        DockerReadError::Engine(_) => ReadToken::EngineError,
        DockerReadError::TimedOut => ReadToken::TimedOut,
    })
}

/// Runs `request`, which belongs to `group` and has passed the gate.
pub(super) async fn run(
    request: &HelperRequest,
    group: HelperGroup,
    config: &HelperConfig,
    ctx: &ServeContext,
) -> HelperResponse {
    let outcome = match group {
        HelperGroup::DockerRead => docker(request, ctx).await,
        _ => tools(request, config, ctx).await,
    };
    outcome.unwrap_or_else(|failure| failed(group, failure))
}

async fn tools(
    request: &HelperRequest,
    config: &HelperConfig,
    ctx: &ServeContext,
) -> Result<HelperResponse, ReadFailure> {
    let Some(exec) = ctx.executor.as_deref() else {
        return Err(ReadFailure::bare(ReadToken::ToolMissing));
    };
    let env = ToolEnv {
        inspect: ctx.inspect.as_ref(),
        exec,
    };
    let patterns = config.smart_device_patterns();
    Ok(match request {
        HelperRequest::SmartScan => HelperResponse::SmartScan {
            devices: smart::scan(env, patterns).await?,
        },
        HelperRequest::SmartReport { device } => {
            let report = smart::report(env, patterns, device).await?;
            HelperResponse::SmartReport {
                json: report.json,
                exit_code: report.exit_code,
                truncated: report.truncated,
            }
        }
        HelperRequest::KernelLog {
            since_minutes,
            lines,
        } => {
            let log = kernel::log(env, ctx.clock.now(), *since_minutes, *lines).await?;
            HelperResponse::KernelLog {
                text: log.text,
                truncated: log.truncated,
            }
        }
        HelperRequest::NetListeners => {
            let (listeners, truncated) = net::listeners(env).await?;
            HelperResponse::NetListeners {
                listeners,
                truncated,
            }
        }
        _ => return Err(ReadFailure::bare(ReadToken::CommandFailed)),
    })
}

async fn docker(
    request: &HelperRequest,
    ctx: &ServeContext,
) -> Result<HelperResponse, ReadFailure> {
    use domovoi_core::helper::read::{MAX_CONTAINERS, MAX_EVENTS, MAX_LOG_TEXT_BYTES};

    let Some(api) = ctx.docker.as_deref() else {
        return Err(ReadFailure::bare(ReadToken::EngineAbsent));
    };
    let fail = |err: DockerReadError| docker_failure(&err);
    Ok(match request {
        HelperRequest::DockerProbe => HelperResponse::DockerEngine {
            engine: Some(api.engine().await.map_err(fail)?),
        },
        HelperRequest::DockerList => {
            let mut containers = api.list().await.map_err(fail)?;
            let truncated = containers.len() > MAX_CONTAINERS;
            containers.truncate(MAX_CONTAINERS);
            HelperResponse::DockerContainers {
                containers,
                truncated,
            }
        }
        HelperRequest::DockerEvents {
            since_secs,
            until_secs,
        } => {
            let (since, until) = window(ctx.clock.now(), *since_secs, *until_secs);
            let mut events = api.events(since, until).await.map_err(fail)?;
            let truncated = events.len() > MAX_EVENTS;
            events.truncate(MAX_EVENTS);
            HelperResponse::DockerEvents { events, truncated }
        }
        HelperRequest::DockerInspect { container } => HelperResponse::DockerInspect {
            inspect: api.inspect(container).await.map_err(fail)?,
        },
        HelperRequest::DockerLogs { container, lines } => {
            let text = api.logs(container, *lines).await.map_err(fail)?;
            let kept = super::limits::cut_utf8(&text, MAX_LOG_TEXT_BYTES).len();
            HelperResponse::DockerLogs {
                // The adapter cuts at the cap on a character boundary and cannot say so: a text within
                // three bytes of the cap (the longest character) was cut.
                truncated: kept < text.len() || text.len() + 3 >= MAX_LOG_TEXT_BYTES,
                text: text[..kept].to_owned(),
            }
        }
        _ => return Err(ReadFailure::bare(ReadToken::EngineError)),
    })
}

/// The window of an events request: `since_secs` ago to `until_secs` ago.
fn window(now: Timestamp, since_secs: u32, until_secs: u32) -> (Timestamp, Timestamp) {
    let ago = |secs: u32| {
        now.saturating_sub(SignedDuration::from_secs(i64::from(secs)))
            .unwrap_or(now)
    };
    (ago(since_secs), ago(until_secs))
}
