//! System checks: disk, memory, temperature, reboots, kernel errors and power.
//!
//! Each module has pure parsers (exposed so they can be fuzzed) and one [`Check`]
//! implementation. Module docs list the check's thresholds, targets and evidence keys.
//!
//! [`Check`]: crate::framework::Check

pub mod disk;
pub mod kernel;
pub mod memory;
pub mod power;
pub mod reboot;
pub mod reboot_required;
pub mod temperature;

use domovoi_core::ports::{FsError, HostFs};
use domovoi_core::{CheckKey, TargetKey, ValidationError};

use crate::framework::CheckError;

pub use disk::{DiskFree, FsStats, FsUsage, RealFsStats};
pub use kernel::KernelErrors;
pub use memory::MemoryAvailable;
pub use power::PowerSupply;
pub use reboot::Reboot;
pub use reboot_required::RebootRequired;
pub use temperature::CpuTemperature;

/// Bytes in one GiB.
pub(crate) const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

pub(crate) fn key(name: &str) -> Result<CheckKey, ValidationError> {
    CheckKey::parse(name)
}

/// Reads a whole file, as a check error naming the path on failure.
pub(crate) async fn read(fs: &dyn HostFs, path: &str) -> Result<String, CheckError> {
    fs.read_to_string(path)
        .await
        .map_err(|e| CheckError::fs(path, e))
}

/// Reads a file that may be absent.
pub(crate) async fn read_optional(
    fs: &dyn HostFs,
    path: &str,
) -> Result<Option<String>, CheckError> {
    match fs.read_to_string(path).await {
        Ok(text) => Ok(Some(text)),
        Err(FsError::NotFound) => Ok(None),
        Err(e) => Err(CheckError::fs(path, e)),
    }
}

/// Lists a directory that may be absent; absent means empty.
pub(crate) async fn list_optional(fs: &dyn HostFs, path: &str) -> Result<Vec<String>, CheckError> {
    match fs.read_dir(path).await {
        Ok(names) => Ok(names),
        Err(FsError::NotFound) => Ok(Vec::new()),
        Err(e) => Err(CheckError::fs(path, e)),
    }
}

/// A target key, or `None` (with a log line) when `name` is not a valid one.
pub(crate) fn target(name: &str) -> Option<TargetKey> {
    let parsed = TargetKey::parse(name);
    if parsed.is_err() {
        tracing::warn!(target_name = name, "skipping a target with an invalid name");
    }
    parsed.ok()
}
