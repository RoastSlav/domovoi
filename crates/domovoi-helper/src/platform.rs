//! The host's platform id and the record of its tier that `selftest --record` leaves for the helper.
//!
//! The helper never trusts a tier a caller claims; `platform.json`, written by root under the
//! state lock, is its only view. The tier decision itself is `classify_platform` in core.

use std::path::Path;

use domovoi_core::isolation::{
    PLATFORM_RECORD_FORMAT, PlatformId, PlatformRecord, SelftestReport, TierDecision,
};
use domovoi_core::ports::{Clock, HostInspect, InspectError};

use crate::state::{LockError, StateLock, StateWriteError, WriteFaults, write_atomic_locked};

/// The record's file name inside the helper state directory.
pub const PLATFORM_FILE: &str = "platform.json";
/// The record's mode: root only.
pub const PLATFORM_FILE_MODE: u32 = 0o600;

const UNKNOWN: &str = "unknown";

fn unquote(value: &str) -> &str {
    let value = value.trim();
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|v| v.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

/// Reads `ID` and `VERSION_ID` from the text of an os-release file.
#[must_use]
pub fn parse_os_release(text: &str) -> (Option<String>, Option<String>) {
    let find = |key: &str| {
        text.lines()
            .filter_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .map(|v| unquote(v).to_owned())
            .find(|v| !v.is_empty())
    };
    (find("ID"), find("VERSION_ID"))
}

/// The host's operating system release and architecture.
///
/// Reads `/etc/os-release`, then `/usr/lib/os-release`, through the inspection port; the
/// architecture is the one this binary was built for. A missing field is `unknown`.
#[must_use]
pub fn platform_id(inspect: &dyn HostInspect) -> PlatformId {
    let text = ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|p| inspect.read_text(Path::new(p)).ok())
        .unwrap_or_default();
    let (os_id, version_id) = parse_os_release(&text);
    PlatformId {
        os_id: os_id.unwrap_or_else(|| UNKNOWN.to_owned()),
        version_id: version_id.unwrap_or_else(|| UNKNOWN.to_owned()),
        arch: std::env::consts::ARCH.to_owned(),
    }
}

/// The record `--record` writes for `report` and the decision made from it.
#[must_use]
pub fn platform_record(
    report: &SelftestReport,
    decision: &TierDecision,
    kernel_release: &str,
    now: jiff::Timestamp,
) -> PlatformRecord {
    PlatformRecord {
        format: PLATFORM_RECORD_FORMAT,
        recorded_at: now,
        kernel_release: kernel_release.to_owned(),
        platform: report.platform.clone(),
        tier: decision.tier,
        blocking: decision.blocking.clone(),
    }
}

/// Why the record could not be written.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// The running kernel's release could not be read.
    #[error("cannot read the kernel release: {0}")]
    Kernel(InspectError),
    /// The record could not be serialised.
    #[error("cannot encode the platform record: {0}")]
    Encode(#[from] serde_json::Error),
    /// The state lock could not be opened or taken.
    #[error(transparent)]
    Lock(#[from] LockError),
    /// The file could not be written, or its durability is unknown.
    #[error(transparent)]
    Write(#[from] StateWriteError),
}

/// Writes `platform.json` in `dir` atomically, under the state lock.
///
/// `dir` is the helper state directory with its `state.lock`; the file is created with mode 0600.
/// `recorded_at` comes from `clock`, never from the system.
pub fn record_platform(
    inspect: &dyn HostInspect,
    clock: &dyn Clock,
    dir: &Path,
    report: &SelftestReport,
    decision: &TierDecision,
) -> Result<PlatformRecord, RecordError> {
    let kernel = inspect.kernel_release().map_err(RecordError::Kernel)?;
    let record = platform_record(report, decision, &kernel, clock.now());
    let bytes = serde_json::to_vec_pretty(&record)?;
    let lock = StateLock::open(dir)?;
    let guard = lock.acquire()?;
    write_atomic_locked(
        &guard,
        dir,
        PLATFORM_FILE,
        &bytes,
        PLATFORM_FILE_MODE,
        WriteFaults::default(),
    )?;
    Ok(record)
}
