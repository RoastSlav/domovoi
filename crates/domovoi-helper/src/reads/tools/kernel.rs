//! `kernel.read`: the last lines of the kernel ring buffer within a time window.

use domovoi_core::helper::read::{KernelLogOutput, ReadFailure};
use jiff::{SignedDuration, Timestamp};

use super::{Tool, ToolEnv, command_failed, kernel_args, run};

/// Lines of `dmesg --time-format=iso` output at or after `now - since_minutes`, then the last
/// `lines` of them. A line without a readable timestamp (a continuation) follows the line before
/// it.
pub async fn log(
    env: ToolEnv<'_>,
    now: Timestamp,
    since_minutes: u32,
    lines: u16,
) -> Result<KernelLogOutput, ReadFailure> {
    let output = run(env, Tool::Dmesg, kernel_args()).await?;
    if output.exit_code != Some(0) && output.stdout.trim().is_empty() {
        return Err(command_failed(&output));
    }
    let window = SignedDuration::from_mins(i64::from(since_minutes));
    let from = now.saturating_sub(window).unwrap_or(now);
    let kept = filter_since(&output.stdout, from);
    let start = kept.len().saturating_sub(usize::from(lines));
    Ok(KernelLogOutput {
        text: kept[start..].join("\n"),
        truncated: output.truncated,
    })
}

/// The lines of `text` that are at or after `from`, with continuation lines kept or dropped with
/// their parent.
#[must_use]
pub fn filter_since(text: &str, from: Timestamp) -> Vec<&str> {
    let mut keeping = false;
    let mut kept = Vec::new();
    for line in text.lines() {
        if let Some(at) = line_time(line) {
            keeping = at >= from;
        }
        if keeping {
            kept.push(line);
        }
    }
    kept
}

/// The leading ISO timestamp of a `dmesg` line, `YYYY-MM-DDTHH:MM:SS,ffffff+hh:mm`.
fn line_time(line: &str) -> Option<Timestamp> {
    let token = line.split_whitespace().next()?;
    if token.len() < 20 || !token.as_bytes()[..4].iter().all(u8::is_ascii_digit) {
        return None;
    }
    token.replacen(',', ".", 1).parse().ok()
}
