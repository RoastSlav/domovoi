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

use crate::framework::CheckError;
pub(crate) use crate::framework::{key, target};
use domovoi_core::ports::{FsError, HostFs};

pub use disk::{DiskFree, FsStats, FsUsage, RealFsStats};
pub use kernel::KernelErrors;
pub use memory::MemoryAvailable;
pub use power::PowerSupply;
pub use reboot::Reboot;
pub use reboot_required::RebootRequired;
pub use temperature::CpuTemperature;

/// Bytes in one GiB.
pub(crate) const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

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
