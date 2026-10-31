//! `smart.health`: disk health from SMART, with counters tracked as deltas.
//!
//! - **Source:** `smartctl --scan-open`, then `smartctl --all` per device, as JSON. Disks
//!   in standby are not woken and report nothing that run.
//! - **Counters** fire on an **increase** over the baseline kept in state, never on a
//!   non-zero value alone. The first sight of a counter records it without firing; a drop
//!   (a replaced disk) re-baselines silently. A counter keeps firing until
//!   [`acknowledge`] moves its baseline.
//!   - ATA 197 (pending) and 198 (offline uncorrectable): critical.
//!   - ATA 5 (reallocated) and 187 (reported uncorrect): warning.
//!   - ATA 199 (CRC errors): warning, with a `hint` to check the cable.
//!   - NVMe `media_errors`: critical.
//! - **Device conditions** (target: the device): NVMe `critical_warning != 0` or a failed
//!   overall assessment → critical; NVMe `percentage_used` ≥ 90 or temperature ≥ 70 °C →
//!   warning.
//! - **Targets:** the device (`/dev/sda`) and `<device>:<counter>`
//!   (`/dev/sda:current_pending_sector`).
//! - **Evidence:** `value`, `previous`, `delta` for counters; `model`, `serial`,
//!   `power_on_hours`, `temperature_c` on every observation.
//! - **Facts:** `smart.<disk>.health` (text) and `smart.<disk>.temperature_c` (°C).

mod parse;

use std::collections::BTreeMap;

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

pub use parse::{NvmeHealth, Report, Scan, ScanDevice, parse_report, parse_scan};

use crate::commands::DevicePath;
use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, CheckState, Observation, int, key, number_fact,
    run_command, target, text_fact,
};
use crate::learn::{load_json, save_json};

/// The state key of the counter baselines.
pub const COUNTERS_STATE: &str = "counters";
/// Most devices checked in one run.
pub const MAX_DEVICES: usize = 32;
/// NVMe wear at which the check warns, in percent.
pub const WEAR_WARNING_PCT: u64 = 90;
/// NVMe temperature at which the check warns.
pub const NVME_TEMP_WARNING_C: i64 = 70;

/// A tracked SMART counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counter {
    /// The ATA attribute id; `None` for NVMe `media_errors`.
    pub ata_id: Option<u16>,
    /// The name used in targets.
    pub name: &'static str,
    /// How bad an increase is.
    pub severity: Severity,
    /// Advice shown with an increase.
    pub hint: Option<&'static str>,
}

/// Every counter the check tracks.
pub const COUNTERS: &[Counter] = &[
    Counter {
        ata_id: Some(5),
        name: "reallocated_sector_ct",
        severity: Severity::Warning,
        hint: None,
    },
    Counter {
        ata_id: Some(187),
        name: "reported_uncorrect",
        severity: Severity::Warning,
        hint: None,
    },
    Counter {
        ata_id: Some(197),
        name: "current_pending_sector",
        severity: Severity::Critical,
        hint: None,
    },
    Counter {
        ata_id: Some(198),
        name: "offline_uncorrectable",
        severity: Severity::Critical,
        hint: None,
    },
    Counter {
        ata_id: Some(199),
        name: "udma_crc_error_count",
        severity: Severity::Warning,
        hint: Some("CRC errors usually mean a loose or bad SATA cable; check the cable"),
    },
    Counter {
        ata_id: None,
        name: "media_errors",
        severity: Severity::Critical,
        hint: None,
    },
];

/// Counter baselines keyed by `<device>:<counter>`.
pub type Baselines = BTreeMap<String, u64>;

fn counter_key(device: &str, counter: &str) -> String {
    format!("{device}:{counter}")
}

/// Moves the baseline of `counter` on `device` to `value`, so the check stops firing until
/// the counter grows again. Called when a human acknowledges the increase.
pub fn acknowledge(state: &mut CheckState, device: &str, counter: &str, value: u64) {
    let mut baselines: Baselines = load_json(state, COUNTERS_STATE);
    baselines.insert(counter_key(device, counter), value);
    save_json(state, COUNTERS_STATE, &baselines);
}

/// The `smart.health` check.
pub struct SmartHealth {
    key: CheckKey,
}

impl SmartHealth {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("smart.health")?,
        })
    }
}

fn with_identity(obs: Observation, report: &Report) -> Observation {
    let mut obs = obs;
    if let Some(model) = &report.model_name {
        obs = obs.with_text("model", model.clone());
    }
    if let Some(serial) = &report.serial_number {
        obs = obs.with_text("serial", serial.clone());
    }
    if let Some(hours) = report.power_on_hours() {
        obs = obs.with("power_on_hours", int(hours));
    }
    if let Some(temp) = report.temperature_c() {
        obs = obs.with("temperature_c", domovoi_core::EvidenceValue::Int(temp));
    }
    obs
}

/// Problems that concern the whole device, worst first.
fn device_problems(report: &Report) -> Vec<(Severity, String)> {
    let mut problems = Vec::new();
    if report.smart_status.is_some_and(|s| !s.passed) {
        problems.push((
            Severity::Critical,
            "SMART overall assessment failed".to_owned(),
        ));
    }
    if let Some(nvme) = report.nvme_smart_health_information_log {
        if nvme.critical_warning != 0 {
            problems.push((
                Severity::Critical,
                format!("NVMe critical warning 0x{:02x}", nvme.critical_warning),
            ));
        }
        if let Some(used) = nvme.percentage_used.filter(|u| *u >= WEAR_WARNING_PCT) {
            problems.push((Severity::Warning, format!("{used}% of rated life used")));
        }
        if let Some(temp) = nvme.temperature.filter(|t| *t >= NVME_TEMP_WARNING_C) {
            problems.push((Severity::Warning, format!("running at {temp} °C")));
        }
    }
    problems.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    problems
}

/// Observations for one device's report, updating `baselines`.
#[must_use]
pub fn evaluate(device: &str, report: &Report, baselines: &mut Baselines) -> Vec<Observation> {
    let mut out = Vec::new();
    let problems = device_problems(report);
    let device_obs = match problems.first() {
        Some((severity, _)) => {
            let summary = problems
                .iter()
                .map(|(_, p)| p.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            Observation::firing(target(device), *severity, format!("{device}: {summary}"))
        }
        None => Observation::ok(target(device), format!("{device} is healthy")),
    };
    out.push(with_identity(device_obs, report));

    for counter in COUNTERS {
        let current = match counter.ata_id {
            Some(id) => report.ata_raw(id),
            None => report
                .nvme_smart_health_information_log
                .and_then(|n| n.media_errors),
        };
        let Some(current) = current else { continue };
        let key = counter_key(device, counter.name);
        let previous = *baselines.entry(key.clone()).or_insert(current);
        if current < previous {
            baselines.insert(key.clone(), current);
        }
        let obs = if current > previous {
            let delta = current - previous;
            let mut obs = Observation::firing(
                target(&key),
                counter.severity,
                format!("{device} {} rose by {delta} to {current}", counter.name),
            )
            .with("value", int(current))
            .with("previous", int(previous))
            .with("delta", int(delta));
            if let Some(hint) = counter.hint {
                obs = obs.with_text("hint", hint);
            }
            obs
        } else {
            Observation::ok(target(&key), format!("{device} {} is steady", counter.name))
                .with("value", int(current))
        };
        out.push(with_identity(obs, report));
    }
    out
}

#[async_trait]
impl Check for SmartHealth {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_hours(1)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let scan_output = run_command(ctx, ctx.commands.smartctl_scan()?).await?;
        let scan = parse_scan(&scan_output.stdout).ok_or_else(|| CheckError::Parse {
            what: "smartctl --scan-open",
            reason: "not the expected JSON".to_owned(),
        })?;
        let now = ctx.clock.now();
        let mut baselines: Baselines = load_json(ctx.state, COUNTERS_STATE);
        let mut out = CheckReport::default();
        for device in scan.devices.iter().take(MAX_DEVICES) {
            let Ok(path) = DevicePath::parse(&device.name) else {
                tracing::warn!(device = device.name, "skipping a device with an odd name");
                continue;
            };
            let output = run_command(ctx, ctx.commands.smartctl_all(&path)?).await?;
            let Some(report) = parse_report(&output.stdout).filter(Report::has_data) else {
                tracing::debug!(
                    device = path.as_str(),
                    "no SMART data (standby or unreadable)"
                );
                continue;
            };
            out.observations
                .extend(evaluate(path.as_str(), &report, &mut baselines));
            let disk = path.as_str().trim_start_matches("/dev/");
            let health = match report.smart_status {
                Some(s) if s.passed => "PASSED",
                Some(_) => "FAILED",
                None => "unknown",
            };
            out.facts.push(text_fact(
                ctx.host,
                format!("smart.{disk}.health"),
                format!("SMART health of {disk}"),
                health,
                now,
            ));
            if let Some(temp) = report.temperature_c() {
                let temp = temp as f64;
                out.facts.push(number_fact(
                    ctx.host,
                    format!("smart.{disk}.temperature_c"),
                    format!("Temperature of {disk}"),
                    temp,
                    "°C",
                    now,
                ));
            }
        }
        save_json(ctx.state, COUNTERS_STATE, &baselines);
        Ok(out)
    }
}
