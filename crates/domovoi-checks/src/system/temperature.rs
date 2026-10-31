//! `temperature.cpu`: the hottest CPU sensor.
//!
//! - **Source:** `/sys/class/hwmon/*/{name,temp*_input,temp*_label}` for CPU drivers
//!   ([`CPU_HWMON`]); without any, `/sys/class/thermal/thermal_zone*/{type,temp}`, preferring
//!   CPU zones and otherwise using every zone.
//! - **Target:** none (the whole host).
//! - **Thresholds:** `warning_c` (88), `critical_c` (95).
//! - **Evidence:** `max_c`, `sensor` (driver and label), `sensors` (how many were read).
//! - **Facts:** `temperature.cpu.max_c` (°C).

use async_trait::async_trait;
use domovoi_core::ports::HostFs;
use domovoi_core::{CheckKey, EvidenceValue, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{key, list_optional, read_optional};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, number_fact};

/// hwmon driver names that report CPU temperatures.
pub const CPU_HWMON: &[&str] = &[
    "coretemp",
    "k10temp",
    "zenpower",
    "cpu_thermal",
    "cpu-thermal",
    "soc_thermal",
    "x86_pkg_temp",
];

const CPU_ZONES: &[&str] = &[
    "x86_pkg_temp",
    "cpu-thermal",
    "cpu_thermal",
    "soc_thermal",
    "acpitz",
];

/// One sensor reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// Driver or zone name, plus the label when there is one.
    pub sensor: String,
    /// Degrees Celsius.
    pub celsius: f64,
}

/// Parses a sysfs temperature in millidegrees Celsius. Values outside -50..200 °C are
/// treated as broken sensors.
#[must_use]
pub fn parse_millidegrees(text: &str) -> Option<f64> {
    let milli: i64 = text.trim().parse().ok()?;
    let celsius = milli as f64 / 1000.0;
    (-50.0..=200.0).contains(&celsius).then_some(celsius)
}

async fn hwmon_readings(fs: &dyn HostFs) -> Result<Vec<Reading>, CheckError> {
    let mut readings = Vec::new();
    for dev in list_optional(fs, "/sys/class/hwmon").await? {
        let base = format!("/sys/class/hwmon/{dev}");
        let Some(name) = read_optional(fs, &format!("{base}/name")).await? else {
            continue;
        };
        let name = name.trim().to_owned();
        if !CPU_HWMON.contains(&name.as_str()) {
            continue;
        }
        for file in list_optional(fs, &base).await? {
            let Some(sensor) = file
                .strip_suffix("_input")
                .filter(|s| s.starts_with("temp"))
            else {
                continue;
            };
            let Some(value) = read_optional(fs, &format!("{base}/{file}")).await? else {
                continue;
            };
            let Some(celsius) = parse_millidegrees(&value) else {
                continue;
            };
            let label = read_optional(fs, &format!("{base}/{sensor}_label")).await?;
            let sensor = match label {
                Some(label) => format!("{name} {}", label.trim()),
                None => format!("{name} {sensor}"),
            };
            readings.push(Reading { sensor, celsius });
        }
    }
    Ok(readings)
}

async fn thermal_zone_readings(fs: &dyn HostFs) -> Result<Vec<Reading>, CheckError> {
    let mut cpu = Vec::new();
    let mut other = Vec::new();
    for zone in list_optional(fs, "/sys/class/thermal").await? {
        if !zone.starts_with("thermal_zone") {
            continue;
        }
        let base = format!("/sys/class/thermal/{zone}");
        let Some(celsius) = read_optional(fs, &format!("{base}/temp"))
            .await?
            .as_deref()
            .and_then(parse_millidegrees)
        else {
            continue;
        };
        let kind = read_optional(fs, &format!("{base}/type"))
            .await?
            .map(|t| t.trim().to_owned())
            .unwrap_or_else(|| zone.clone());
        let reading = Reading {
            sensor: kind.clone(),
            celsius,
        };
        if CPU_ZONES.contains(&kind.as_str()) {
            cpu.push(reading);
        } else {
            other.push(reading);
        }
    }
    Ok(if cpu.is_empty() { other } else { cpu })
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Thresholds {
    warning_c: f64,
    critical_c: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            warning_c: 88.0,
            critical_c: 95.0,
        }
    }
}

/// The `temperature.cpu` check.
pub struct CpuTemperature {
    key: CheckKey,
}

impl CpuTemperature {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("temperature.cpu")?,
        })
    }
}

#[async_trait]
impl Check for CpuTemperature {
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
        let mut readings = hwmon_readings(ctx.fs).await?;
        if readings.is_empty() {
            readings = thermal_zone_readings(ctx.fs).await?;
        }
        let count = readings.len();
        let Some(hottest) = readings
            .into_iter()
            .max_by(|a, b| a.celsius.total_cmp(&b.celsius))
        else {
            return Ok(CheckReport::default());
        };
        let max = hottest.celsius;
        let summary = format!("CPU at {max:.0} °C ({})", hottest.sensor);
        let observation = if max >= thresholds.critical_c {
            Observation::firing(None, Severity::Critical, summary)
        } else if max >= thresholds.warning_c {
            Observation::firing(None, Severity::Warning, summary)
        } else {
            Observation::ok(None, summary)
        };
        let sensors = i64::try_from(count).unwrap_or(i64::MAX);
        Ok(CheckReport {
            observations: vec![
                observation
                    .with_number("max_c", max)
                    .with_text("sensor", hottest.sensor)
                    .with("sensors", EvidenceValue::Int(sensors)),
            ],
            facts: vec![number_fact(
                ctx.host,
                "temperature.cpu.max_c",
                "hottest CPU sensor",
                max,
                "°C",
                ctx.clock.now(),
            )],
        })
    }
}
