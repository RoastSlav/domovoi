//! `disk.free`: free space per real filesystem.
//!
//! - **Source:** `/proc/self/mounts`, then `statvfs` per mount point. Pseudo and
//!   read-only image filesystems ([`PSEUDO_FS`], `cgroup*`) are skipped, as are duplicate
//!   mount points.
//! - **Targets:** the mount point, such as `/srv`.
//! - **Thresholds:** `warning_free_pct` (15), `critical_free_pct` (6), per-mount overrides
//!   in `mounts.<mount point>` with the same keys, `ignore` (mount points to skip).
//! - **Evidence:** `free_pct`, `free_gib`, `total_gib`, `fstype`, `device`.
//! - **Facts:** `disk.<mount>.free_gib` (GiB).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use domovoi_core::ports::FsError;
use domovoi_core::{CheckKey, Severity, SignalClass, ValidationError};
use jiff::SignedDuration;

use super::{GIB, key, read, target};
use crate::framework::{Check, CheckContext, CheckError, CheckReport, Observation, number_fact};

/// Filesystem types that never hold user data.
pub const PSEUDO_FS: &[&str] = &[
    "autofs",
    "binfmt_misc",
    "bpf",
    "configfs",
    "debugfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "nsfs",
    "overlay",
    "proc",
    "pstore",
    "ramfs",
    "rpc_pipefs",
    "securityfs",
    "squashfs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

/// One line of `/proc/self/mounts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// The source device, such as `/dev/sda1`.
    pub device: String,
    /// Where it is mounted, with `\040`-style escapes decoded.
    pub mount_point: String,
    /// The filesystem type.
    pub fstype: String,
}

/// Size and free space of one filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsUsage {
    /// Total size in bytes.
    pub total_bytes: u64,
    /// Bytes available to unprivileged users.
    pub available_bytes: u64,
}

/// Reads filesystem usage; [`RealFsStats`] in production, a fake in tests.
#[async_trait]
pub trait FsStats: Send + Sync {
    /// Usage of the filesystem mounted at the host-absolute `mount_point`.
    async fn usage(&self, mount_point: &str) -> Result<FsUsage, FsError>;
}

/// `statvfs` on `<root><mount_point>`.
#[derive(Debug, Clone)]
pub struct RealFsStats {
    root: PathBuf,
}

impl RealFsStats {
    /// Stats under `root` (`/` normally, `/host` in a container).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl FsStats for RealFsStats {
    async fn usage(&self, mount_point: &str) -> Result<FsUsage, FsError> {
        let path = self.root.join(mount_point.trim_start_matches('/'));
        #[cfg(unix)]
        {
            let stats =
                nix::sys::statvfs::statvfs(&path).map_err(|e| FsError::Io(e.to_string()))?;
            let fragment = widen(stats.fragment_size());
            Ok(FsUsage {
                total_bytes: widen(stats.blocks()).saturating_mul(fragment),
                available_bytes: widen(stats.blocks_available()).saturating_mul(fragment),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(FsError::Io("statvfs is only available on Unix".to_owned()))
        }
    }
}

/// `statvfs` field widths differ between Unix platforms; this takes any of them.
#[cfg(unix)]
fn widen<T: Into<u64>>(value: T) -> u64 {
    value.into()
}

/// Decodes the octal escapes the kernel uses for spaces and tabs in mount paths.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes
            .get(i + 1..i + 4)
            .filter(|d| bytes[i] == b'\\' && d.iter().all(|b| (b'0'..=b'7').contains(b)));
        match octal {
            Some(digits) => {
                let value = digits
                    .iter()
                    .fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0'));
                out.push(u8::try_from(value).unwrap_or(b'?'));
                i += 4;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses `/proc/self/mounts`, skipping malformed lines.
#[must_use]
pub fn parse_mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(Mount {
                device: unescape(fields.next()?),
                mount_point: unescape(fields.next()?),
                fstype: fields.next()?.to_owned(),
            })
        })
        .collect()
}

/// Whether `fstype` is a pseudo filesystem the check skips.
#[must_use]
pub fn is_pseudo(fstype: &str) -> bool {
    PSEUDO_FS.contains(&fstype) || fstype.starts_with("cgroup")
}

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MountThresholds {
    warning_free_pct: Option<f64>,
    critical_free_pct: Option<f64>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Thresholds {
    warning_free_pct: f64,
    critical_free_pct: f64,
    mounts: BTreeMap<String, MountThresholds>,
    ignore: Vec<String>,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            warning_free_pct: 15.0,
            critical_free_pct: 6.0,
            mounts: BTreeMap::new(),
            ignore: Vec::new(),
        }
    }
}

/// The `disk.free` check.
pub struct DiskFree {
    key: CheckKey,
    stats: Arc<dyn FsStats>,
}

impl DiskFree {
    /// The check, reading usage through `stats`.
    pub fn new(stats: Arc<dyn FsStats>) -> Result<Self, ValidationError> {
        Ok(Self {
            key: key("disk.free")?,
            stats,
        })
    }
}

#[async_trait]
impl Check for DiskFree {
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
        let thresholds: Thresholds = ctx.settings.thresholds()?;
        let mounts = parse_mounts(&read(ctx.fs, "/proc/self/mounts").await?);
        let now = ctx.clock.now();
        let mut seen = BTreeSet::new();
        let mut report = CheckReport::default();
        for mount in mounts {
            if is_pseudo(&mount.fstype)
                || thresholds.ignore.contains(&mount.mount_point)
                || !seen.insert(mount.mount_point.clone())
            {
                continue;
            }
            let usage = match self.stats.usage(&mount.mount_point).await {
                Ok(usage) if usage.total_bytes > 0 => usage,
                Ok(_) => continue,
                Err(err) => {
                    tracing::debug!(mount = %mount.mount_point, %err, "skipping a mount without stats");
                    continue;
                }
            };
            let Some(target) = target(&mount.mount_point) else {
                continue;
            };
            let free_pct = usage.available_bytes as f64 / usage.total_bytes as f64 * 100.0;
            let free_gib = usage.available_bytes as f64 / GIB;
            let limits = thresholds
                .mounts
                .get(&mount.mount_point)
                .copied()
                .unwrap_or_default();
            let critical = limits
                .critical_free_pct
                .unwrap_or(thresholds.critical_free_pct);
            let warning = limits
                .warning_free_pct
                .unwrap_or(thresholds.warning_free_pct);
            let observation = if free_pct < critical {
                Observation::firing(
                    Some(target),
                    Severity::Critical,
                    format!("{} has {free_pct:.1}% free", mount.mount_point),
                )
            } else if free_pct < warning {
                Observation::firing(
                    Some(target),
                    Severity::Warning,
                    format!("{} has {free_pct:.1}% free", mount.mount_point),
                )
            } else {
                Observation::ok(
                    Some(target),
                    format!("{} has {free_pct:.1}% free", mount.mount_point),
                )
            };
            report.observations.push(
                observation
                    .with_number("free_pct", free_pct)
                    .with_number("free_gib", free_gib)
                    .with_number("total_gib", usage.total_bytes as f64 / GIB)
                    .with_text("fstype", mount.fstype.clone())
                    .with_text("device", mount.device.clone()),
            );
            report.facts.push(number_fact(
                ctx.host,
                format!("disk.{}.free_gib", mount.mount_point),
                format!("free on {}", mount.mount_point),
                free_gib,
                "GiB",
                now,
            ));
        }
        Ok(report)
    }
}
