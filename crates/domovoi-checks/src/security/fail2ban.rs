//! `security.fail2ban`: a jail is banning a lot.
//!
//! - **Settings:** `jails` (the jails to read; none means the check does nothing) and
//!   `bans_per_hour` (default 20).
//! - **Source:** `fail2ban-client status <jail>`. Bans in the last hour come from the
//!   growth of `Total banned` in a one-hour window kept in state; a fail2ban restart
//!   (the total drops) counts from zero.
//! - **Target:** the jail. **Severity:** info. Counts are evidence only.
//! - **Evidence:** `bans_last_hour`, `currently_banned`, `total_banned`, `total_failed`.

use async_trait::async_trait;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use crate::commands::JailName;
use crate::containers::RESTART_WINDOW;
use crate::framework::{
    Check, CheckContext, CheckError, CheckReport, Observation, int, key, run_command, target,
};
use crate::learn::{Windows, load_json, save_json};

/// The state key of the per-jail ban windows.
pub const WINDOWS_STATE: &str = "windows";
/// Most jails one check reads.
pub const MAX_JAILS: usize = 32;

/// `[checks."security.fail2ban"]` thresholds.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fail2banThresholds {
    /// The jails to read.
    pub jails: Vec<String>,
    /// Bans within an hour that are worth a note.
    pub bans_per_hour: u64,
}

impl Default for Fail2banThresholds {
    fn default() -> Self {
        Self {
            jails: Vec::new(),
            bans_per_hour: 20,
        }
    }
}

/// The counters in `fail2ban-client status <jail>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JailStatus {
    /// `Currently failed`.
    pub currently_failed: u64,
    /// `Total failed`.
    pub total_failed: u64,
    /// `Currently banned`.
    pub currently_banned: u64,
    /// `Total banned`.
    pub total_banned: u64,
}

/// Parses `fail2ban-client status <jail>`; `None` without a `Total banned` line.
#[must_use]
pub fn parse_jail_status(output: &str) -> Option<JailStatus> {
    let mut status = JailStatus::default();
    let mut found = false;
    for line in output.lines() {
        let line = line.trim_start_matches(['|', '`', '-', ' ']);
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<u64>() else {
            continue;
        };
        match name.trim() {
            "Currently failed" => status.currently_failed = value,
            "Total failed" => status.total_failed = value,
            "Currently banned" => status.currently_banned = value,
            "Total banned" => {
                status.total_banned = value;
                found = true;
            }
            _ => {}
        }
    }
    found.then_some(status)
}

/// The `security.fail2ban` check.
pub struct Fail2banBans {
    key: CheckKey,
}

impl Fail2banBans {
    /// The check.
    pub fn new() -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("security.fail2ban")?,
        })
    }
}

#[async_trait]
impl Check for Fail2banBans {
    fn key(&self) -> CheckKey {
        self.key.clone()
    }

    fn default_cadence(&self) -> SignedDuration {
        SignedDuration::from_mins(5)
    }

    fn class(&self) -> SignalClass {
        SignalClass::Normal
    }

    async fn run(&self, ctx: &mut CheckContext<'_>) -> Result<CheckReport, CheckError> {
        let thresholds: Fail2banThresholds = ctx.settings.thresholds()?;
        if thresholds.jails.len() > MAX_JAILS {
            return Err(CheckError::Settings(format!(
                "at most {MAX_JAILS} jails can be read"
            )));
        }
        let jails = thresholds
            .jails
            .iter()
            .map(|j| JailName::parse(j))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CheckError::Settings(e.to_string()))?;
        let now = ctx.clock.now();
        let mut windows: Windows = load_json(ctx.state, WINDOWS_STATE);
        let mut report = CheckReport::default();
        for jail in &jails {
            let name = jail.as_str();
            let output = run_command(ctx, ctx.commands.fail2ban_status(jail)?).await?;
            let status = parse_jail_status(&output.stdout).ok_or_else(|| CheckError::Parse {
                what: "fail2ban-client status",
                reason: format!("no ban counts for jail {name}"),
            })?;
            let window = windows.entry(name.to_owned()).or_default();
            window.record(now, status.total_banned, RESTART_WINDOW);
            let bans = window.increase();
            let observation = if bans >= thresholds.bans_per_hour {
                Observation::firing(
                    target(name),
                    Severity::Info,
                    format!("fail2ban jail {name} banned {bans} addresses in the last hour"),
                )
            } else {
                Observation::ok(target(name), format!("fail2ban jail {name} is quiet"))
            };
            report.observations.push(
                observation
                    .with("bans_last_hour", int(bans))
                    .with("currently_banned", int(status.currently_banned))
                    .with("total_banned", int(status.total_banned))
                    .with("total_failed", int(status.total_failed)),
            );
        }
        windows.retain(|name, _| jails.iter().any(|j| j.as_str() == name));
        save_json(ctx.state, WINDOWS_STATE, &windows);
        Ok(report)
    }
}
