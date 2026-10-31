//! The parts of `smartctl --json` output the SMART check reads.

use serde::Deserialize;

/// `smartctl --scan-open --json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Scan {
    /// The devices smartctl can open.
    #[serde(default)]
    pub devices: Vec<ScanDevice>,
}

/// One scanned device.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScanDevice {
    /// The device path, such as `/dev/sda`.
    pub name: String,
    /// The protocol, such as `ATA` or `NVMe`.
    #[serde(default)]
    pub protocol: String,
}

/// `smartctl --all --json <device>`.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Report {
    /// The drive model.
    #[serde(default)]
    pub model_name: Option<String>,
    /// The drive serial number.
    #[serde(default)]
    pub serial_number: Option<String>,
    /// The drive's own overall assessment.
    #[serde(default)]
    pub smart_status: Option<SmartStatus>,
    /// Current temperature.
    #[serde(default)]
    pub temperature: Option<Temperature>,
    /// Power-on time.
    #[serde(default)]
    pub power_on_time: Option<PowerOnTime>,
    /// ATA attributes.
    #[serde(default)]
    pub ata_smart_attributes: Option<AtaAttributes>,
    /// The NVMe health log.
    #[serde(default)]
    pub nvme_smart_health_information_log: Option<NvmeHealth>,
}

/// `smart_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct SmartStatus {
    /// Whether the drive says it is healthy.
    pub passed: bool,
}

/// `temperature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Temperature {
    /// Degrees Celsius.
    pub current: Option<i64>,
}

/// `power_on_time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct PowerOnTime {
    /// Hours powered on.
    pub hours: Option<u64>,
}

/// `ata_smart_attributes`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct AtaAttributes {
    /// The attribute table.
    #[serde(default)]
    pub table: Vec<AtaAttribute>,
}

/// One ATA attribute.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AtaAttribute {
    /// The attribute id, such as 5.
    pub id: u16,
    /// The raw counter.
    pub raw: AtaRaw,
}

/// An attribute's raw value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct AtaRaw {
    /// The raw counter.
    pub value: u64,
}

/// `nvme_smart_health_information_log`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub struct NvmeHealth {
    /// Critical warning bits; any bit set is critical.
    #[serde(default)]
    pub critical_warning: u64,
    /// Composite temperature in degrees Celsius.
    #[serde(default)]
    pub temperature: Option<i64>,
    /// Estimated share of the drive's rated life used.
    #[serde(default)]
    pub percentage_used: Option<u64>,
    /// Unrecovered data integrity errors.
    #[serde(default)]
    pub media_errors: Option<u64>,
    /// Hours powered on.
    #[serde(default)]
    pub power_on_hours: Option<u64>,
}

impl Report {
    /// Whether the report carries health data; a disk in standby returns none.
    #[must_use]
    pub fn has_data(&self) -> bool {
        self.ata_smart_attributes.is_some() || self.nvme_smart_health_information_log.is_some()
    }

    /// The temperature, from the NVMe log or the generic field.
    #[must_use]
    pub fn temperature_c(&self) -> Option<i64> {
        self.nvme_smart_health_information_log
            .and_then(|n| n.temperature)
            .or_else(|| self.temperature.and_then(|t| t.current))
    }

    /// Hours powered on.
    #[must_use]
    pub fn power_on_hours(&self) -> Option<u64> {
        self.power_on_time
            .and_then(|p| p.hours)
            .or_else(|| self.nvme_smart_health_information_log?.power_on_hours)
    }

    /// The raw value of ATA attribute `id`.
    #[must_use]
    pub fn ata_raw(&self, id: u16) -> Option<u64> {
        self.ata_smart_attributes
            .as_ref()?
            .table
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.raw.value)
    }
}

/// Parses `smartctl --scan-open --json`.
#[must_use]
pub fn parse_scan(output: &str) -> Option<Scan> {
    serde_json::from_str(output).ok()
}

/// Parses `smartctl --all --json`.
#[must_use]
pub fn parse_report(output: &str) -> Option<Report> {
    serde_json::from_str(output).ok()
}
