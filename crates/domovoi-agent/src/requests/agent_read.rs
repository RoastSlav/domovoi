//! The built-in reads the agent answers in place of a skill's entry script when the skill
//! runs behind the skill runner, whose account and sandbox cannot see their sources. The set
//! is closed ([`AgentRead`]), each runs fixed commands only, none changes the host, and none
//! gives a skill anything the hub's read tools do not already reach through this account.
//!
//! A source that cannot be read ends the read with exit status 3 and the reason; an empty
//! section is written as `(none)`, only after its read succeeded.

use std::collections::BTreeMap;
use std::fmt::Write;

use domovoi_checks::commands::{CommandError, DayPriority, SUMMARY_LINES, ServiceState, UnitName};
use domovoi_checks::smart::{parse_report, parse_scan};
use domovoi_core::helper::read::DeviceRef;
use domovoi_core::ports::{ProcessOutput, ProcessSpec, SkillOutput};
use domovoi_core::{AgentRead, ScriptDigest, SkillManifest, ValidatedArgs};

use super::tools::{journal_access_refused, journal_asked, journal_result};
use super::{RequestCtx, cap};

/// Exit status of a read whose source could not be read.
const HIDDEN: i32 = 3;
/// Most processes listed per order.
const PROCESSES: usize = 15;
/// Most devices whose SMART health is read.
const DEVICES: usize = 16;

/// Whether this account may read the system journal. Asked with one line of output; only a
/// refusal that journalctl names, or a failed run, counts as no.
pub(super) async fn journal_is_readable(ctx: &RequestCtx) -> bool {
    let Ok(spec) = ctx.commands.journal(None, 60, 1, false, None) else {
        return false;
    };
    match ctx.exec.run(spec).await {
        Ok(output) => !journal_access_refused(&output.stderr) && output.exit_code == Some(0),
        Err(_) => false,
    }
}

/// Runs the built-in `read` for the skill `manifest`. The result has the shape of a skill
/// run, so every caller treats it as one; its first line says who answered.
pub(super) async fn run(
    ctx: &RequestCtx,
    read: AgentRead,
    manifest: &SkillManifest,
    args: &ValidatedArgs,
) -> SkillOutput {
    let started = ctx.clock.now();
    let args = args.to_map();
    let result = match read {
        AgentRead::Services => services(ctx).await,
        AgentRead::Processes => processes(ctx).await,
        AgentRead::LogSummary => log_summary(ctx).await,
        AgentRead::JournalErrors => journal_errors(ctx, &args).await,
        AgentRead::Hardware => Ok(hardware(ctx).await),
    };
    let (exit_code, stdout, stderr) = match result {
        Ok(text) => (
            0,
            format!(
                "(read by the agent's built-in `{}`; the skill runner cannot see {}, so the entry script did not run)\n{text}",
                read.name(),
                read.covers().describe()
            ),
            String::new(),
        ),
        Err(why) => (HIDDEN, String::new(), format!("host view hidden: {why}")),
    };
    let (stdout, truncated) = cap(stdout, manifest.max_output_bytes as usize);
    let invoked = format!("agent_read {} {args:?}", read.name());
    SkillOutput {
        exit_code: Some(exit_code),
        stdout,
        stderr,
        truncated,
        duration_ms: u64::try_from(ctx.clock.now().duration_since(started).as_millis())
            .unwrap_or(0),
        argv_hash: *ScriptDigest::of_bytes(invoked.as_bytes()).as_bytes(),
    }
}

/// Runs one fixed command that has to succeed.
async fn command(
    ctx: &RequestCtx,
    spec: Result<ProcessSpec, CommandError>,
) -> Result<ProcessOutput, String> {
    let spec = spec.map_err(|e| e.to_string())?;
    let program = spec.program.clone();
    let output = ctx.exec.run(spec).await.map_err(|e| e.to_string())?;
    if output.timed_out {
        return Err(format!("{program} timed out"));
    }
    // A command stopped at the output limit did answer; the caller sees the cut.
    if output.exit_code != Some(0) && !output.truncated {
        return Err(format!("{program} failed: {}", output.stderr.trim()));
    }
    Ok(output)
}

/// Appends `== title`, then `body` or `(none)`.
fn section(out: &mut String, title: &str, body: &str) {
    let body = body.trim_end();
    let body = if body.is_empty() { "(none)" } else { body };
    _ = write!(out, "== {title}\n{body}\n\n");
}

async fn services(ctx: &RequestCtx) -> Result<String, String> {
    let running = command(ctx, ctx.commands.systemctl_services(ServiceState::Running)).await?;
    // A systemd host always runs some service, so nothing here means nothing was seen.
    if running.stdout.trim().is_empty() {
        return Err("systemctl listed no running service at all".to_owned());
    }
    let failed = command(ctx, ctx.commands.systemctl_services(ServiceState::Failed)).await?;
    let timers = command(ctx, ctx.commands.systemctl_timers()).await?;
    let mut out = String::new();
    section(&mut out, "running services", &running.stdout);
    section(&mut out, "failed services", &failed.stdout);
    section(&mut out, "timers", &timers.stdout);
    Ok(out)
}

async fn processes(ctx: &RequestCtx) -> Result<String, String> {
    command(ctx, ctx.commands.ps_first())
        .await
        .map_err(|why| format!("process 1 is not visible to the agent ({why})"))?;
    let mut out = String::new();
    for (title, memory) in [("by CPU", false), ("by memory", true)] {
        let listed = command(ctx, ctx.commands.ps_by(memory)).await?;
        let top: Vec<&str> = listed.stdout.lines().take(PROCESSES + 1).collect();
        if top.len() < 2 {
            return Err("ps listed no process".to_owned());
        }
        section(&mut out, title, &top.join("\n"));
    }
    Ok(out)
}

/// The lines of one day read, refusing a journal this account may not read.
async fn day(ctx: &RequestCtx, priority: DayPriority, lines: u16) -> Result<String, String> {
    let output = command(ctx, ctx.commands.journal_day(priority, lines)).await?;
    if journal_access_refused(&output.stderr) {
        return Err(format!(
            "the agent account may not read the system journal: {}",
            output.stderr.trim()
        ));
    }
    let mut text = output
        .stdout
        .lines()
        .filter(|line| !line.starts_with("-- "))
        .fold(String::new(), |mut text, line| {
            text.push_str(line);
            text.push('\n');
            text
        });
    if output.truncated {
        text.push_str("(cut at the output limit)\n");
    }
    Ok(text)
}

/// Entries per program from a list of program names, one per line, most first, at most 30.
fn per_program(names: &str) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for name in names.lines().map(str::trim) {
        if !name.is_empty() && !name.starts_with('(') {
            *counts.entry(name).or_default() += 1;
        }
    }
    let mut counts: Vec<(&str, usize)> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    counts
        .iter()
        .take(30)
        .map(|(name, count)| format!("{count:7} {name}\n"))
        .collect()
}

async fn log_summary(ctx: &RequestCtx) -> Result<String, String> {
    let warnings = day(ctx, DayPriority::Warnings, SUMMARY_LINES).await?;
    let errors = day(ctx, DayPriority::Errors, 20).await?;
    let mut out = String::new();
    if warnings.lines().count() >= usize::from(SUMMARY_LINES) {
        _ = writeln!(out, "(only the newest {SUMMARY_LINES} entries are counted)");
    }
    section(
        &mut out,
        "warnings and errors per program, last 24 h",
        &per_program(&warnings),
    );
    section(&mut out, "most recent errors", &errors);
    Ok(out)
}

async fn journal_errors(
    ctx: &RequestCtx,
    args: &BTreeMap<String, String>,
) -> Result<String, String> {
    let unit = args.get("unit").ok_or("the `unit` parameter is missing")?;
    let unit = UnitName::parse(unit).map_err(|e| e.to_string())?;
    let minutes: u32 = args
        .get("minutes")
        .and_then(|text| text.parse().ok())
        .ok_or("the `minutes` parameter is not a number")?;
    let spec = ctx
        .commands
        .journal(Some(&unit), minutes, 200, false, Some("err"))
        .map_err(|e| e.to_string())?;
    let output = ctx.exec.run(spec).await.map_err(|e| e.to_string())?;
    if output.timed_out {
        return Err("journalctl timed out".to_owned());
    }
    let name = unit.to_string();
    journal_result(output, &journal_asked(Some(&name), minutes, Some("err")))
        .map(|(text, _)| text)
        .map_err(|fail| fail.message)
}

async fn temperatures(ctx: &RequestCtx) -> String {
    let mut zones = match ctx.fs.read_dir("/sys/class/thermal").await {
        Ok(zones) => zones,
        Err(err) => return format!("not read: {err}"),
    };
    zones.sort();
    let mut out = String::new();
    for zone in zones.iter().filter(|z| z.starts_with("thermal_zone")) {
        let base = format!("/sys/class/thermal/{zone}");
        let Ok(temp) = ctx.fs.read_to_string(&format!("{base}/temp")).await else {
            continue;
        };
        let kind = ctx
            .fs
            .read_to_string(&format!("{base}/type"))
            .await
            .unwrap_or_else(|_| "unknown".to_owned());
        _ = writeln!(
            out,
            "{zone} {}: {} millidegrees C",
            kind.trim(),
            temp.trim()
        );
    }
    if out.is_empty() {
        out.push_str("no thermal zones\n");
    }
    out
}

/// Kernel errors of the last two hours: the journal, else the helper's kernel log.
async fn kernel_errors(ctx: &RequestCtx) -> String {
    let direct = match ctx.commands.journal(None, 120, 100, true, Some("err")) {
        Ok(spec) => match ctx.exec.run(spec).await {
            Ok(output) => journal_result(output, &journal_asked(None, 120, Some("err")))
                .map(|(text, _)| text)
                .map_err(|fail| fail.message),
            Err(err) => Err(err.to_string()),
        },
        Err(err) => Err(err.to_string()),
    };
    let why = match direct {
        Ok(text) => return text,
        Err(why) => why,
    };
    if let Some(port) = &ctx.privileged
        && let Ok(log) = port.kernel_log(120, 100).await
    {
        let lines = if log.text.trim().is_empty() {
            "no kernel log lines in the last 120 minutes".to_owned()
        } else {
            log.text
        };
        return format!(
            "[read through the helper, which does not filter by priority: every priority is listed]\n{lines}"
        );
    }
    format!("not read: {why}")
}

async fn edac(ctx: &RequestCtx) -> String {
    const ROOT: &str = "/sys/devices/system/edac/mc";
    let mut controllers = ctx.fs.read_dir(ROOT).await.unwrap_or_default();
    controllers.sort();
    let mut out = String::new();
    for mc in controllers.iter().filter(|name| name.starts_with("mc")) {
        for counter in ["ce_count", "ue_count"] {
            if let Ok(value) = ctx
                .fs
                .read_to_string(&format!("{ROOT}/{mc}/{counter}"))
                .await
            {
                _ = writeln!(out, "{mc}.{counter} = {}", value.trim());
            }
        }
    }
    if out.is_empty() {
        out.push_str("no edac counters\n");
    }
    out
}

/// One health line from `smartctl --json` text.
fn health_line(device: &str, json: &str) -> String {
    match parse_report(json) {
        Some(report) => {
            let health = match report.smart_status {
                Some(status) if status.passed => "PASSED",
                Some(_) => "FAILED",
                None => "no health status reported",
            };
            let model = report
                .model_name
                .unwrap_or_else(|| "unknown model".to_owned());
            format!("{device} {model}: {health}\n")
        }
        None => format!("{device}: not read: smartctl printed no readable report\n"),
    }
}

/// Each disk with its `smartctl --json` text, or why it could not be read.
type Disks = Vec<(String, Result<String, String>)>;

/// Disk health. Opening a disk needs root, so it is asked of the helper.
async fn smart(ctx: &RequestCtx) -> String {
    let devices: Result<Disks, String> = match &ctx.privileged {
        Some(port) => match port.smart_scan().await {
            Ok(found) => {
                let mut reports = Vec::new();
                for device in found.iter().take(DEVICES) {
                    let report = port
                        .smart_report(device)
                        .await
                        .map(|report| report.json)
                        .map_err(|e| e.to_string());
                    reports.push((device.as_str().to_owned(), report));
                }
                Ok(reports)
            }
            Err(err) => Err(format!("the helper could not list the disks: {err}")),
        },
        None => direct_smart(ctx).await,
    };
    match devices {
        Err(why) => format!("not read: {why}"),
        Ok(devices) if devices.is_empty() => "no SMART capable disk was found".to_owned(),
        Ok(devices) => devices
            .iter()
            .map(|(device, report)| match report {
                Ok(json) => health_line(device, json),
                Err(why) => format!("{device}: not read: {why}\n"),
            })
            .collect(),
    }
}

/// Disk health without a helper: `smartctl` run by the agent itself.
async fn direct_smart(ctx: &RequestCtx) -> Result<Disks, String> {
    let scan = command(ctx, ctx.commands.smartctl_scan()).await?;
    let scan = parse_scan(&scan.stdout).ok_or("smartctl printed no readable device list")?;
    let mut reports = Vec::new();
    for found in scan.devices.iter().take(DEVICES) {
        let Ok(device) = DeviceRef::parse(&found.name) else {
            continue;
        };
        let path = domovoi_checks::commands::DevicePath::parse(device.as_str())
            .map_err(|e| e.to_string())?;
        let spec = ctx
            .commands
            .smartctl_all(&path)
            .map_err(|e| e.to_string())?;
        // smartctl's exit status is a bit mask, so the report text is judged, not the status.
        let report = ctx
            .exec
            .run(spec)
            .await
            .map(|output| output.stdout)
            .map_err(|e| e.to_string());
        reports.push((found.name.clone(), report));
    }
    Ok(reports)
}

/// Temperatures, kernel errors, memory error counters and disk health. Each section says
/// when its own source could not be read.
async fn hardware(ctx: &RequestCtx) -> String {
    let mut out = String::new();
    section(&mut out, "temperatures", &temperatures(ctx).await);
    section(
        &mut out,
        "kernel errors (2 hours)",
        &kernel_errors(ctx).await,
    );
    section(&mut out, "edac counters", &edac(ctx).await);
    section(&mut out, "smart", &smart(ctx).await);
    out
}

#[cfg(test)]
mod tests {
    use super::per_program;

    #[test]
    fn entries_are_counted_per_program_most_first() {
        let names = "sshd\nkernel\nsshd\n\n(cut at the output limit)\n";
        assert_eq!(per_program(names), "      2 sshd\n      1 kernel\n");
    }
}
