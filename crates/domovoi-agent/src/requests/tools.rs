//! `run_tool`: the closed set of read tools, served from local sources.

use std::fmt::Write;

use domovoi_checks::commands::{CommandError, DevicePath, UnitName};
use domovoi_checks::containers::{ContainerSource, EventLog, RunState};
use domovoi_checks::security::ports::listeners;
use domovoi_core::ReadToolCall;
use domovoi_core::ports::ProcessSpec;
use domovoi_proto::messages::ToolFailureCode;
use jiff::SignedDuration;

use super::overview::host_overview;
use super::skills::run_read_skill;
use super::{RequestCtx, ToolFail};

/// The check whose state holds the 24 h container event log.
pub const EVENTS_CHECK: &str = "container.expected_down";

fn docker(ctx: &RequestCtx) -> Result<&dyn ContainerSource, ToolFail> {
    ctx.containers.as_deref().ok_or_else(|| {
        ToolFail::new(
            ToolFailureCode::Unavailable,
            "Docker is not enabled on this host",
        )
    })
}

fn docker_err(err: impl std::fmt::Display) -> ToolFail {
    ToolFail::new(ToolFailureCode::Unavailable, format!("Docker: {err}"))
}

/// A template that could not be built: the tool is missing or unusable here, or an
/// argument is invalid.
fn template(err: CommandError) -> ToolFail {
    if err.is_unavailable() {
        ToolFail::new(ToolFailureCode::Unavailable, err.to_string())
    } else {
        invalid(err)
    }
}

fn invalid(err: impl std::fmt::Display) -> ToolFail {
    ToolFail::new(ToolFailureCode::InvalidArgs, err.to_string())
}

async fn command(ctx: &RequestCtx, spec: ProcessSpec) -> Result<(String, bool), ToolFail> {
    let output = ctx
        .exec
        .run(spec)
        .await
        .map_err(|e| ToolFail::new(ToolFailureCode::Exec, e.to_string()))?;
    if output.timed_out {
        return Err(ToolFail::new(
            ToolFailureCode::Timeout,
            "the command timed out",
        ));
    }
    let mut text = output.stdout;
    if output.exit_code != Some(0) && !output.stderr.is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&output.stderr);
    }
    Ok((text, output.truncated))
}

/// Runs one read tool and returns its text and whether the source cut it short.
pub async fn run_tool(ctx: &RequestCtx, call: &ReadToolCall) -> Result<(String, bool), ToolFail> {
    match call {
        ReadToolCall::DockerPs { all } => {
            let containers = docker(ctx)?.list().await.map_err(docker_err)?;
            let mut out = String::from("name\timage\tstate\thealth\trestarts\n");
            for c in containers
                .iter()
                .filter(|c| *all || c.state == RunState::Running)
            {
                let health = c
                    .health
                    .map_or("-".to_owned(), |h| format!("{h:?}").to_lowercase());
                _ = writeln!(
                    out,
                    "{}\t{}\t{}\t{health}\t{}",
                    c.name,
                    c.image,
                    c.state.as_str(),
                    c.restart_count
                );
            }
            Ok((out, false))
        }
        ReadToolCall::DockerLogs { container, lines } => {
            let text = docker(ctx)?
                .logs(container, *lines)
                .await
                .map_err(docker_err)?;
            Ok((text, false))
        }
        ReadToolCall::DockerInspect { container } => {
            let inspect = docker(ctx)?.inspect(container).await.map_err(docker_err)?;
            let text = serde_json::to_string_pretty(&inspect).map_err(docker_err)?;
            Ok((text, false))
        }
        ReadToolCall::DockerEvents {
            container,
            since_minutes,
        } => {
            docker(ctx)?;
            let key = domovoi_core::CheckKey::parse(EVENTS_CHECK).map_err(invalid)?;
            let log = EventLog::from_state(&ctx.store.state(&key));
            let since = ctx.clock.now() - SignedDuration::from_mins(i64::from(*since_minutes));
            let mut out = String::new();
            for event in log.events.iter().filter(|e| e.at >= since) {
                if container.as_ref().is_some_and(|name| *name != event.name) {
                    continue;
                }
                _ = writeln!(out, "{} {} {}", event.at, event.name, event.action);
            }
            if out.is_empty() {
                out.push_str("no container events in that window\n");
            }
            Ok((out, false))
        }
        ReadToolCall::HostOverview => Ok((host_overview(ctx).await, false)),
        ReadToolCall::Journal {
            unit,
            since_minutes,
            lines,
            kernel,
        } => {
            let unit = unit
                .as_deref()
                .map(UnitName::parse)
                .transpose()
                .map_err(invalid)?;
            let spec = ctx
                .commands
                .journal(unit.as_ref(), *since_minutes, *lines, *kernel)
                .map_err(template)?;
            command(ctx, spec).await
        }
        ReadToolCall::ListeningPorts => {
            let mut out = String::new();
            for listener in listeners(&*ctx.fs).await {
                _ = writeln!(out, "{listener}");
            }
            if out.is_empty() {
                out.push_str("no listeners outside loopback\n");
            }
            Ok((out, false))
        }
        ReadToolCall::SmartReport { device } => {
            let device = DevicePath::parse(device).map_err(invalid)?;
            let spec = ctx.commands.smartctl_all(&device).map_err(template)?;
            command(ctx, spec).await
        }
        ReadToolCall::RunReadSkill { skill, args } => run_read_skill(ctx, skill, args).await,
        ReadToolCall::IncidentHistory { .. } | ReadToolCall::NotesSearch { .. } => {
            Err(ToolFail::new(
                ToolFailureCode::UnknownTool,
                "this tool is answered by the hub, not the agent",
            ))
        }
    }
}
