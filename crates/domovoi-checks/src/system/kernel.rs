//! `kernel.errors`: hardware and memory errors in the kernel log.
//!
//! - **Source:** the kernel journal since the last cursor, through
//!   [`Commands::journal_since_cursor`](crate::commands::Commands::journal_since_cursor).
//!   The cursor is kept in state, so every line is classified once.
//! - **Targets:** one per pattern ([`KernelPattern`]), so each fingerprints separately.
//!   Counts and lines are evidence, never part of a target.
//! - **Evidence:** `count`, `examples` (at most [`MAX_EXAMPLES`] lines, already redacted by
//!   the executor); `device` for `io_error`, `process` for `oom_kill`.
//! - Only patterns seen in this run are reported; there is nothing to resolve.

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::{CheckKey, EvidenceValue, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{key, target};
use crate::commands::{JournalCursor, JournalScope};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation};

/// The state key holding the journal cursor.
pub const CURSOR_STATE: &str = "cursor";
/// Most example lines kept per pattern.
pub const MAX_EXAMPLES: usize = 5;
/// Longest example line kept, in characters.
pub const MAX_EXAMPLE_CHARS: usize = 300;

/// A kernel log pattern the check recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KernelPattern {
    /// `I/O error` on a block device.
    IoError,
    /// The OOM killer ended a process.
    OomKill,
    /// An NVMe controller reset or timed out.
    NvmeReset,
    /// An uncorrected PCIe bus error.
    PcieUncorrected,
    /// A corrected PCIe bus error.
    PcieCorrected,
}

impl KernelPattern {
    /// The target name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::IoError => "io_error",
            Self::OomKill => "oom_kill",
            Self::NvmeReset => "nvme_reset",
            Self::PcieUncorrected => "pcie_uncorrected",
            Self::PcieCorrected => "pcie_corrected",
        }
    }

    /// How serious one occurrence is.
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            Self::IoError | Self::OomKill => Severity::Warning,
            Self::NvmeReset | Self::PcieUncorrected => Severity::Critical,
            Self::PcieCorrected => Severity::Info,
        }
    }

    fn summary(self, count: u64) -> String {
        let what = match self {
            Self::IoError => "I/O error",
            Self::OomKill => "out-of-memory kill",
            Self::NvmeReset => "NVMe reset or timeout",
            Self::PcieUncorrected => "uncorrected PCIe error",
            Self::PcieCorrected => "corrected PCIe error",
        };
        format!("{count} {what}(s) in the kernel log")
    }
}

/// Whether `needle` appears in `haystack` after `from`, returning the index after it.
fn find_after(haystack: &str, from: usize, needle: &str) -> Option<usize> {
    haystack
        .get(from..)?
        .find(needle)
        .map(|i| from + i + needle.len())
}

/// Classifies one kernel log line, case-insensitively.
#[must_use]
pub fn classify(line: &str) -> Option<KernelPattern> {
    let lower = line.to_ascii_lowercase();
    if let Some(at) = find_after(&lower, 0, "pcie bus error") {
        if find_after(&lower, at, "severity=uncorrected").is_some() {
            return Some(KernelPattern::PcieUncorrected);
        }
        if find_after(&lower, at, "severity=corrected").is_some() {
            return Some(KernelPattern::PcieCorrected);
        }
    }
    if let Some(at) = find_after(&lower, 0, "nvme")
        && (find_after(&lower, at, "reset").is_some()
            || find_after(&lower, at, "timeout").is_some())
    {
        return Some(KernelPattern::NvmeReset);
    }
    if lower.contains("out of memory: killed process") || lower.contains("oom-kill:") {
        return Some(KernelPattern::OomKill);
    }
    if lower.contains("i/o error") {
        return Some(KernelPattern::IoError);
    }
    None
}

/// The device named in an `I/O error, dev sda, ...` line.
#[must_use]
pub fn io_device(line: &str) -> Option<String> {
    let rest = &line[line.find("dev ")? + 4..];
    let device: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    (!device.is_empty()).then_some(device)
}

/// The process named in a `Killed process 1234 (name)` line.
#[must_use]
pub fn oom_process(line: &str) -> Option<String> {
    let start = line.find('(')? + 1;
    let end = start + line.get(start..)?.find(')')?;
    let name = line.get(start..end)?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// The cursor from a `-- cursor: <cursor>` line.
#[must_use]
pub fn parse_cursor_line(line: &str) -> Option<JournalCursor> {
    JournalCursor::parse(line.strip_prefix("-- cursor: ")?.trim()).ok()
}

#[derive(Default)]
struct Tally {
    count: u64,
    examples: Vec<String>,
    detail: Option<String>,
}

/// The `kernel.errors` check.
pub struct KernelErrors {
    key: CheckKey,
}

impl KernelErrors {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("kernel.errors")?,
        })
    }
}

/// Classifies journal output: tallies per pattern and the trailing cursor, if any.
fn scan(output: &str) -> (BTreeMap<KernelPattern, Tally>, Option<JournalCursor>) {
    let mut tallies: BTreeMap<KernelPattern, Tally> = BTreeMap::new();
    let mut cursor = None;
    for line in output.lines() {
        if let Some(found) = parse_cursor_line(line) {
            cursor = Some(found);
            continue;
        }
        let Some(pattern) = classify(line) else {
            continue;
        };
        let tally = tallies.entry(pattern).or_default();
        tally.count += 1;
        if tally.examples.len() < MAX_EXAMPLES {
            tally
                .examples
                .push(line.chars().take(MAX_EXAMPLE_CHARS).collect());
        }
        if tally.detail.is_none() {
            tally.detail = match pattern {
                KernelPattern::IoError => io_device(line),
                KernelPattern::OomKill => oom_process(line),
                _ => None,
            };
        }
    }
    (tallies, cursor)
}

#[async_trait]
impl Check for KernelErrors {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let previous = ctx
            .state
            .get(CURSOR_STATE)
            .and_then(|c| JournalCursor::parse(c).ok());
        let spec = ctx
            .commands
            .journal_since_cursor(previous.as_ref(), JournalScope::Kernel)?;
        let program = spec.program.clone();
        let output = ctx
            .exec
            .run(spec)
            .await
            .map_err(|source| CheckError::Process { program, source })?;
        let (tallies, cursor) = scan(&output.stdout);
        match cursor {
            Some(cursor) => ctx.state.set(CURSOR_STATE, cursor.as_str()),
            // Cut output loses the cursor line; start again from a recent window rather
            // than re-reading the same lines forever.
            None if output.truncated => ctx.state.remove(CURSOR_STATE),
            None => {}
        }
        let mut report = CheckReport::default();
        for (pattern, tally) in tallies {
            let count = i64::try_from(tally.count).unwrap_or(i64::MAX);
            let mut observation = Observation::firing(
                target(pattern.name()),
                pattern.severity(),
                pattern.summary(tally.count),
            )
            .with("count", EvidenceValue::Int(count))
            .with_text("examples", tally.examples.join("\n"));
            if let Some(detail) = tally.detail {
                let field = if pattern == KernelPattern::OomKill {
                    "process"
                } else {
                    "device"
                };
                observation = observation.with_text(field, detail);
            }
            report.observations.push(observation);
        }
        Ok(report)
    }
}
