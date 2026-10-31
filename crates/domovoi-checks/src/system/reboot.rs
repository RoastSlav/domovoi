//! `system.reboot`: the host rebooted since the last run.
//!
//! - **Source:** `/proc/sys/kernel/random/boot_id` compared with the boot id kept in
//!   state; `/proc/uptime`.
//! - **Target:** none. **Class:** immediate. **Severity:** info.
//! - Fires once when the boot id changes. The first run only records the boot id.
//! - **Evidence:** `uptime_s`, `boot_id`, `previous_boot_id`.
//! - **Facts:** `system.uptime_days` (days), every run.

use async_trait::async_trait;
use domovoi_core::{CheckKey, EvidenceValue, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{key, read};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, number_fact};

/// The state key holding the last boot id seen.
pub const BOOT_ID_STATE: &str = "boot_id";

/// Parses the first field of `/proc/uptime`, in seconds.
#[must_use]
pub fn parse_uptime(text: &str) -> Option<f64> {
    let secs: f64 = text.split_whitespace().next()?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then_some(secs)
}

/// The `system.reboot` check.
pub struct Reboot {
    key: CheckKey,
}

impl Reboot {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("system.reboot")?,
        })
    }
}

#[async_trait]
impl Check for Reboot {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Immediate
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let boot_id = read(ctx.fs, "/proc/sys/kernel/random/boot_id")
            .await?
            .trim()
            .to_owned();
        let uptime = parse_uptime(&read(ctx.fs, "/proc/uptime").await?);
        let mut report = CheckReport::default();
        if let Some(uptime) = uptime {
            report.facts.push(number_fact(
                ctx.host,
                "system.uptime_days",
                "uptime",
                uptime / 86_400.0,
                "d",
                ctx.clock.now(),
            ));
        }
        if boot_id.is_empty() {
            return Ok(report);
        }
        let previous = ctx.state.get(BOOT_ID_STATE).map(str::to_owned);
        ctx.state.set(BOOT_ID_STATE, boot_id.clone());
        if let Some(previous) = previous.filter(|p| *p != boot_id) {
            let uptime_s = uptime.map_or(0, |u| u as i64);
            report.observations.push(
                Observation::firing(None, Severity::Info, "Host rebooted")
                    .with("uptime_s", EvidenceValue::Int(uptime_s))
                    .with_text("boot_id", boot_id)
                    .with_text("previous_boot_id", previous),
            );
        }
        Ok(report)
    }
}
