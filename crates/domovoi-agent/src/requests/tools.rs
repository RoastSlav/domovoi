//! `run_tool`: the closed set of read tools, served from local sources.

use std::fmt::Write;

use domovoi_checks::commands::{CommandError, DevicePath, UnitName};
use domovoi_checks::containers::{ContainerSource, EventLog, RunState};
use domovoi_checks::security::ports::{Listener, listeners};
use domovoi_core::ReadToolCall;
use domovoi_core::helper::read::{
    DeviceRef, ListenerWire, MAX_KERNEL_MINUTES, MAX_LOG_LINES, WireProto,
};
use domovoi_core::ports::{ProcessOutput, ProcessSpec};
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
            "Docker is not enabled on this host; set [docker] enabled = true in agent.toml",
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

async fn execute(ctx: &RequestCtx, spec: ProcessSpec) -> Result<ProcessOutput, ToolFail> {
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
    Ok(output)
}

fn render(output: ProcessOutput) -> (String, bool) {
    let mut text = output.stdout;
    if output.exit_code != Some(0) && !output.stderr.is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&output.stderr);
    }
    (text, output.truncated)
}

async fn command(ctx: &RequestCtx, spec: ProcessSpec) -> Result<(String, bool), ToolFail> {
    Ok(render(execute(ctx, spec).await?))
}

/// Whether `journalctl --dmesg` could not read the journal, so the kernel ring buffer may be
/// read through the helper instead.
fn journal_unreadable(output: &ProcessOutput) -> bool {
    output.exit_code != Some(0)
        || journal_access_refused(&output.stderr)
        || output.stderr.contains("could not be opened")
        || output.stderr.contains("No journal files")
}

/// Whether journalctl says this account may not read the system journal. It says so on its
/// error output, and not at all with `--quiet`, so the journal tool never passes that.
pub(super) fn journal_access_refused(stderr: &str) -> bool {
    stderr.contains("not seeing messages") || stderr.contains("insufficient permissions")
}

/// The entries of a `journalctl` output, without its own `-- ... --` marker lines.
fn journal_entries(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| !(line.starts_with("-- ") && line.ends_with(" --")))
        .fold(String::new(), |mut text, line| {
            text.push_str(line);
            text.push('\n');
            text
        })
}

/// A journal read that ran. An account that may not read the system journal, or a failed
/// read with nothing to show, is a failure that says why; "no entries" is said only when the
/// read succeeded. `asked` names the filters, so an empty answer can be widened.
pub(super) fn journal_result(
    output: ProcessOutput,
    asked: &str,
) -> Result<(String, bool), ToolFail> {
    let entries = journal_entries(&output.stdout);
    let why = output.stderr.trim();
    if journal_access_refused(&output.stderr) {
        return Err(ToolFail::new(
            ToolFailureCode::Unavailable,
            format!(
                "the journal could not be read: this agent account may not read the system journal (add it to the systemd-journal group and restart the agent); this is not an empty result. {why}"
            ),
        ));
    }
    if entries.trim().is_empty() {
        if journal_unreadable(&output) {
            return Err(ToolFail::new(
                ToolFailureCode::Unavailable,
                format!("the journal could not be read; this is not an empty result. {why}"),
            ));
        }
        return Ok((
            format!("no journal entries matched ({asked}); the read succeeded\n"),
            false,
        ));
    }
    Ok(render(ProcessOutput {
        stdout: entries,
        ..output
    }))
}

/// The words for a journal query's filters.
pub(super) fn journal_asked(unit: Option<&str>, minutes: u32, priority: Option<&str>) -> String {
    let unit = unit.map_or(String::new(), |unit| format!("unit {unit}, "));
    let priority = priority.map_or(String::new(), |p| format!("priority {p} or worse, "));
    format!("{unit}{priority}last {minutes} minutes")
}

/// The kernel log: `journalctl --dmesg` first, and once through the helper when that cannot
/// read the journal and the helper serves `kernel.read`. A failed helper read keeps the direct
/// result.
async fn kernel_journal(
    ctx: &RequestCtx,
    spec: ProcessSpec,
    since_minutes: u32,
    lines: u16,
    priority: Option<&str>,
) -> Result<(String, bool), ToolFail> {
    let output = execute(ctx, spec).await?;
    if let Some(port) = &ctx.privileged
        && journal_unreadable(&output)
    {
        let minutes = since_minutes.clamp(1, MAX_KERNEL_MINUTES);
        let lines = lines.clamp(1, MAX_LOG_LINES);
        if let Ok(log) = port.kernel_log(minutes, lines).await {
            let mut text = log.text;
            if text.trim().is_empty() {
                text = format!(
                    "no kernel log lines in the last {minutes} minutes; the read succeeded\n"
                );
            } else if priority.is_some() {
                text.insert_str(
                    0,
                    "[read through the helper, which does not filter by priority: every priority is listed]\n",
                );
            }
            return Ok((text, log.truncated));
        }
    }
    journal_result(output, &journal_asked(None, since_minutes, priority))
}

/// One listener line, with the owning process when the helper saw it.
fn listener_line(listener: &Listener, owners: &[ListenerWire]) -> String {
    let proto = listener.proto.as_str();
    let addr = listener.addr.to_string();
    let same_socket = |o: &&ListenerWire| {
        o.process.is_some() && o.port == listener.port && wire_proto(o.proto) == proto
    };
    let owner = owners
        .iter()
        .filter(same_socket)
        .find(|o| o.addr == addr)
        .or_else(|| owners.iter().find(same_socket));
    match owner.and_then(|o| o.process.as_deref().map(|name| (name, o.pid))) {
        Some((name, Some(pid))) => format!("{listener} {name} (pid {pid})"),
        Some((name, None)) => format!("{listener} {name}"),
        None => listener.to_string(),
    }
}

fn wire_proto(proto: WireProto) -> &'static str {
    match proto {
        WireProto::Tcp => "tcp",
        WireProto::Udp => "udp",
    }
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
            priority,
        } => {
            let unit = unit
                .as_deref()
                .map(UnitName::parse)
                .transpose()
                .map_err(invalid)?;
            let spec = ctx
                .commands
                .journal(
                    unit.as_ref(),
                    *since_minutes,
                    *lines,
                    *kernel,
                    priority.as_deref(),
                )
                .map_err(template)?;
            if *kernel && unit.is_none() {
                kernel_journal(ctx, spec, *since_minutes, *lines, priority.as_deref()).await
            } else {
                let unit = unit.as_ref().map(ToString::to_string);
                let asked = journal_asked(unit.as_deref(), *since_minutes, priority.as_deref());
                journal_result(execute(ctx, spec).await?, &asked)
            }
        }
        ReadToolCall::ListeningPorts => {
            let direct = listeners(&*ctx.fs).await;
            // Owning processes come from the helper only; a failed helper read leaves the
            // direct result as it is.
            let owners = match &ctx.privileged {
                Some(port) if !direct.is_empty() => port.net_listeners().await.unwrap_or_default(),
                _ => Vec::new(),
            };
            let mut out = String::new();
            for listener in &direct {
                _ = writeln!(out, "{}", listener_line(listener, &owners));
            }
            if out.is_empty() {
                out.push_str("no listeners outside loopback\n");
            }
            Ok((out, false))
        }
        ReadToolCall::SmartReport { device } => {
            let device = DevicePath::parse(device).map_err(invalid)?;
            if let Some(port) = &ctx.privileged {
                let slot = DeviceRef::parse(device.as_str()).map_err(invalid)?;
                let report = port.smart_report(&slot).await.map_err(|err| {
                    ToolFail::new(ToolFailureCode::Unavailable, format!("SMART: {err}"))
                })?;
                return Ok((report.json, report.truncated));
            }
            let spec = ctx.commands.smartctl_all(&device).map_err(template)?;
            command(ctx, spec).await
        }
        ReadToolCall::HostBinary { name, package } => Ok((
            super::binary::host_binary(&*ctx.fs, name, package.as_deref()).await,
            false,
        )),
        ReadToolCall::RunReadSkill { skill, args } => run_read_skill(ctx, skill, args).await,
        ReadToolCall::IncidentHistory { .. } | ReadToolCall::NotesSearch { .. } => {
            Err(ToolFail::new(
                ToolFailureCode::UnknownTool,
                "this tool is answered by the hub, not the agent",
            ))
        }
    }
}

#[cfg(test)]
mod journal_tests {
    use super::*;

    fn out(code: i32, stdout: &str, stderr: &str) -> ProcessOutput {
        ProcessOutput {
            exit_code: Some(code),
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            truncated: false,
            ..Default::default()
        }
    }

    #[test]
    fn an_unreadable_journal_is_an_error_not_an_empty_answer() {
        let hint = "Hint: You are currently not seeing messages from other users and the system.";
        assert!(journal_result(out(0, "", hint), "x").is_err());
        assert!(journal_result(out(0, "-- No entries --\n", hint), "x").is_err());
        // The account's own lines are not the system journal.
        assert!(journal_result(out(0, "own line\n", hint), "x").is_err());
        assert!(journal_result(out(1, "", "No journal files were found."), "x").is_err());
    }

    #[test]
    fn a_readable_empty_journal_says_what_was_asked() {
        let asked = journal_asked(Some("a.service"), 60, Some("err"));
        let text = journal_result(out(0, "-- No entries --\n", ""), &asked)
            .unwrap()
            .0;
        assert_eq!(
            text,
            "no journal entries matched (unit a.service, priority err or worse, last 60 minutes); the read succeeded\n"
        );
        let text = journal_result(out(0, "-- Boot ab --\nline\n", ""), "x")
            .unwrap()
            .0;
        assert_eq!(text, "line\n");
    }
}
