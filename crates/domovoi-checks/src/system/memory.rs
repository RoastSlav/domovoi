//! `memory.available`: available memory and swap use.
//!
//! - **Source:** `/proc/meminfo` (`MemAvailable`, `MemTotal`, `SwapTotal`, `SwapFree`).
//! - **Targets:** `available`; `swap` when the host has swap.
//! - **Thresholds:** `warning_available_gib` (1.5), `critical_available_gib` (none),
//!   `warning_swap_used_pct` (50).
//! - **Evidence:** `available_gib`, `total_gib`; for swap `swap_used_pct`, `swap_total_gib`.
//! - **Facts:** `memory.available_gib` (GiB).

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{GIB, key, read, target};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, number_fact};

/// Parses `/proc/meminfo` into byte counts keyed by field name. Malformed lines are
/// skipped.
#[must_use]
pub fn parse_meminfo(text: &str) -> BTreeMap<String, u64> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            let mut parts = rest.split_whitespace();
            let value: u64 = parts.next()?.parse().ok()?;
            let bytes = match parts.next() {
                Some("kB") => value.checked_mul(1024)?,
                None => value,
                Some(_) => return None,
            };
            Some((name.trim().to_owned(), bytes))
        })
        .collect()
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Thresholds {
    warning_available_gib: f64,
    critical_available_gib: Option<f64>,
    warning_swap_used_pct: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            warning_available_gib: 1.5,
            critical_available_gib: None,
            warning_swap_used_pct: 50.0,
        }
    }
}

/// The `memory.available` check.
pub struct MemoryAvailable {
    key: CheckKey,
}

impl MemoryAvailable {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("memory.available")?,
        })
    }
}

#[async_trait]
impl Check for MemoryAvailable {
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
        let thresholds: Thresholds = ctx.settings.thresholds()?;
        let info = parse_meminfo(&read(ctx.fs, "/proc/meminfo").await?);
        let mut report = CheckReport::default();
        if let Some(&available) = info.get("MemAvailable") {
            let gib = available as f64 / GIB;
            let summary = format!("{gib:.2} GiB of memory available");
            let critical = thresholds.critical_available_gib.is_some_and(|c| gib < c);
            let observation = if critical {
                Observation::firing(target("available"), Severity::Critical, summary)
            } else if gib < thresholds.warning_available_gib {
                Observation::firing(target("available"), Severity::Warning, summary)
            } else {
                Observation::ok(target("available"), summary)
            };
            let total = info.get("MemTotal").copied().unwrap_or(0) as f64 / GIB;
            report.observations.push(
                observation
                    .with_number("available_gib", gib)
                    .with_number("total_gib", total),
            );
            report.facts.push(number_fact(
                ctx.host,
                "memory.available_gib",
                "memory available",
                gib,
                "GiB",
                ctx.clock.now(),
            ));
        }
        let swap_total = info.get("SwapTotal").copied().unwrap_or(0);
        if let (true, Some(&swap_free)) = (swap_total > 0, info.get("SwapFree")) {
            let used_pct = swap_total.saturating_sub(swap_free) as f64 / swap_total as f64 * 100.0;
            let summary = format!("{used_pct:.0}% of swap in use");
            let observation = if used_pct > thresholds.warning_swap_used_pct {
                Observation::firing(target("swap"), Severity::Warning, summary)
            } else {
                Observation::ok(target("swap"), summary)
            };
            report.observations.push(
                observation
                    .with_number("swap_used_pct", used_pct)
                    .with_number("swap_total_gib", swap_total as f64 / GIB),
            );
        }
        Ok(report)
    }
}
