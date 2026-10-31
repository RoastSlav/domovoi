//! `system.reboot_required`: installed updates need a reboot.
//!
//! - **Source:** `/var/run/reboot-required` (present means a reboot is needed) and
//!   `/var/run/reboot-required.pkgs` (one package per line), as written by Debian and Ubuntu.
//! - **Target:** none. **Severity:** warning while the file exists; resolves when it is gone.
//! - **Evidence:** `packages` (first [`MAX_PACKAGES`], comma-separated), `package_count`.

use async_trait::async_trait;
use domovoi_core::{CheckKey, EvidenceValue, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{key, read_optional};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation};

/// Most package names listed in evidence.
pub const MAX_PACKAGES: usize = 10;

/// Unique package names from `reboot-required.pkgs`, in file order.
#[must_use]
pub fn parse_packages(text: &str) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if !packages.iter().any(|p| p == line) {
            packages.push(line.to_owned());
        }
    }
    packages
}

/// The `system.reboot_required` check.
pub struct RebootRequired {
    key: CheckKey,
}

impl RebootRequired {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("system.reboot_required")?,
        })
    }
}

#[async_trait]
impl Check for RebootRequired {
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
        if !ctx.fs.exists("/var/run/reboot-required").await {
            return Ok(CheckReport {
                observations: vec![Observation::ok(None, "No reboot required")],
                facts: Vec::new(),
            });
        }
        let packages = read_optional(ctx.fs, "/var/run/reboot-required.pkgs")
            .await?
            .map(|text| parse_packages(&text))
            .unwrap_or_default();
        let count = i64::try_from(packages.len()).unwrap_or(i64::MAX);
        let listed: Vec<&str> = packages
            .iter()
            .take(MAX_PACKAGES)
            .map(String::as_str)
            .collect();
        let summary = if packages.is_empty() {
            "Reboot required".to_owned()
        } else {
            format!("Reboot required by {} package(s)", packages.len())
        };
        Ok(CheckReport {
            observations: vec![
                Observation::firing(None, Severity::Warning, summary)
                    .with_text("packages", listed.join(", "))
                    .with("package_count", EvidenceValue::Int(count)),
            ],
            facts: Vec::new(),
        })
    }
}
