//! `power.supply`: mains power, battery level, battery health and charge limits.
//!
//! - **Source:** `/sys/class/power_supply/*/{type,online,status,capacity}`, battery
//!   capacity from `energy_full`/`energy_full_design` or `charge_full`/`charge_full_design`,
//!   and `charge_control_end_threshold`.
//! - **Targets and rules:**
//!   - `ac`: firing **critical, immediate** when no mains supply is online and a battery is
//!     discharging (running on battery).
//!   - `<battery>` (such as `BAT0`): firing critical when discharging below
//!     `low_battery_pct` (30).
//!   - `<battery>.charge_limit`: with `charge_limit_pct` set, firing warning when the
//!     battery reports a different `charge_control_end_threshold`.
//! - A host with no power supplies (most desktops and servers) reports nothing.
//! - **Evidence:** `capacity_pct`, `status`, `health_pct`; `charge_limit_pct` and
//!   `applied_pct` for the charge limit.
//! - **Facts:** `power.<battery>.capacity_pct` and `power.<battery>.health_pct` (%).

use async_trait::async_trait;
use domovoi_core::ports::HostFs;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{key, list_optional, read_optional, target};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, number_fact};

const ROOT: &str = "/sys/class/power_supply";

/// What one `/sys/class/power_supply/<name>` directory reports.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Supply {
    /// Directory name, such as `AC` or `BAT0`.
    pub name: String,
    /// `Mains`, `Battery`, `USB`, ...
    pub kind: String,
    /// Whether a mains supply is online.
    pub online: Option<bool>,
    /// Battery status, such as `Discharging`.
    pub status: Option<String>,
    /// Battery charge in percent.
    pub capacity: Option<f64>,
    /// Full capacity as a share of design capacity, in percent.
    pub health: Option<f64>,
    /// The charge limit the battery applies, in percent.
    pub charge_end: Option<f64>,
}

/// Parses a numeric sysfs value.
#[must_use]
pub fn parse_number(text: &str) -> Option<f64> {
    let value: f64 = text.trim().parse().ok()?;
    value.is_finite().then_some(value)
}

/// Full capacity over design capacity, in percent, when both are sane.
#[must_use]
pub fn health_pct(full: f64, design: f64) -> Option<f64> {
    (full >= 0.0 && design > 0.0).then(|| (full / design * 100.0).min(200.0))
}

async fn number(fs: &dyn HostFs, path: &str) -> Result<Option<f64>, CheckError> {
    Ok(read_optional(fs, path)
        .await?
        .as_deref()
        .and_then(parse_number))
}

async fn read_supply(fs: &dyn HostFs, name: &str) -> Result<Supply, CheckError> {
    let base = format!("{ROOT}/{name}");
    let text = |file: &str| format!("{base}/{file}");
    let kind = read_optional(fs, &text("type")).await?.unwrap_or_default();
    let mut health = None;
    for (full, design) in [
        ("energy_full", "energy_full_design"),
        ("charge_full", "charge_full_design"),
    ] {
        if let (Some(f), Some(d)) = (
            number(fs, &text(full)).await?,
            number(fs, &text(design)).await?,
        ) {
            health = health_pct(f, d);
            break;
        }
    }
    Ok(Supply {
        name: name.to_owned(),
        kind: kind.trim().to_owned(),
        online: number(fs, &text("online")).await?.map(|v| v > 0.0),
        status: read_optional(fs, &text("status"))
            .await?
            .map(|s| s.trim().to_owned()),
        capacity: number(fs, &text("capacity")).await?,
        health,
        charge_end: number(fs, &text("charge_control_end_threshold")).await?,
    })
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Thresholds {
    low_battery_pct: f64,
    charge_limit_pct: Option<f64>,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            low_battery_pct: 30.0,
            charge_limit_pct: None,
        }
    }
}

/// The `power.supply` check.
pub struct PowerSupply {
    key: CheckKey,
}

impl PowerSupply {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("power.supply")?,
        })
    }
}

fn is_discharging(supply: &Supply) -> bool {
    supply.status.as_deref() == Some("Discharging")
}

#[async_trait]
impl Check for PowerSupply {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_secs(30)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let thresholds: Thresholds = ctx.settings.thresholds()?;
        let mut supplies = Vec::new();
        for name in list_optional(ctx.fs, ROOT).await? {
            supplies.push(read_supply(ctx.fs, &name).await?);
        }
        let batteries: Vec<&Supply> = supplies.iter().filter(|s| s.kind == "Battery").collect();
        let mut report = CheckReport::default();
        if batteries.is_empty() {
            return Ok(report);
        }
        let mains_online = supplies
            .iter()
            .any(|s| s.kind == "Mains" && s.online == Some(true));
        let on_battery = !mains_online && batteries.iter().any(|b| is_discharging(b));
        report.observations.push(if on_battery {
            Observation::firing(target("ac"), Severity::Critical, "Running on battery").immediate()
        } else {
            Observation::ok(target("ac"), "On mains power")
        });
        let now = ctx.clock.now();
        for battery in batteries {
            let capacity = battery.capacity.unwrap_or(100.0);
            let status = battery.status.clone().unwrap_or_default();
            let low = is_discharging(battery) && capacity < thresholds.low_battery_pct;
            let summary = format!("{} at {capacity:.0}% ({status})", battery.name);
            let mut observation = if low {
                Observation::firing(target(&battery.name), Severity::Critical, summary)
            } else {
                Observation::ok(target(&battery.name), summary)
            };
            observation = observation
                .with_number("capacity_pct", capacity)
                .with_text("status", status);
            if let Some(health) = battery.health {
                observation = observation.with_number("health_pct", health);
                report.facts.push(number_fact(
                    ctx.host,
                    format!("power.{}.health_pct", battery.name),
                    format!("{} health", battery.name),
                    health,
                    "%",
                    now,
                ));
            }
            report.observations.push(observation);
            report.facts.push(number_fact(
                ctx.host,
                format!("power.{}.capacity_pct", battery.name),
                format!("{} charge", battery.name),
                capacity,
                "%",
                now,
            ));
            if let (Some(limit), Some(applied)) = (thresholds.charge_limit_pct, battery.charge_end)
            {
                let name = format!("{}.charge_limit", battery.name);
                let observation = if (applied - limit).abs() >= 1.0 {
                    Observation::firing(
                        target(&name),
                        Severity::Warning,
                        format!("{} charges to {applied:.0}%, not {limit:.0}%", battery.name),
                    )
                } else {
                    Observation::ok(
                        target(&name),
                        format!("{} charge limit is {limit:.0}%", battery.name),
                    )
                };
                report.observations.push(
                    observation
                        .with_number("charge_limit_pct", limit)
                        .with_number("applied_pct", applied),
                );
            }
        }
        Ok(report)
    }
}
