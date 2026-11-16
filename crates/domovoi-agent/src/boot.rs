//! The boot identity and monotonic clock the journal and limiter rely on.

use domovoi_core::ports::{FsError, HostFs};
use uuid::Uuid;

/// The kernel's boot identity and the monotonic time at which it was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootSnapshot {
    /// `/proc/sys/kernel/random/boot_id`, parsed and non-nil.
    pub boot_id: Uuid,
    /// `/proc/uptime`, in nanoseconds with hundredth-of-a-second precision.
    pub monotonic_ns: u64,
}

/// Why a boot snapshot could not be taken.
#[derive(Debug, thiserror::Error)]
pub enum BootSnapshotError {
    /// A source file could not be read.
    #[error("cannot read {path}: {reason}")]
    Read {
        /// The file that failed.
        path: &'static str,
        /// What the filesystem reported.
        reason: String,
    },
    /// The boot id is empty, not a UUID or nil.
    #[error("invalid boot id: {0}")]
    BootId(String),
    /// The uptime is empty, malformed, negative, over-precise or overflows.
    #[error("invalid uptime: {0}")]
    Uptime(String),
}

/// Reads and validates the boot id and uptime from `fs`.
///
/// A read failure, an empty or malformed value, a nil boot UUID and an overflow are all
/// errors. A genuine uptime of zero is valid.
pub async fn boot_snapshot(fs: &dyn HostFs) -> Result<BootSnapshot, BootSnapshotError> {
    let boot_id = read(fs, "/proc/sys/kernel/random/boot_id").await?;
    let boot_id = parse_boot_id(&boot_id).map_err(BootSnapshotError::BootId)?;
    let uptime = read(fs, "/proc/uptime").await?;
    let monotonic_ns = parse_uptime_ns(&uptime).map_err(BootSnapshotError::Uptime)?;
    Ok(BootSnapshot {
        boot_id,
        monotonic_ns,
    })
}

async fn read(fs: &dyn HostFs, path: &'static str) -> Result<String, BootSnapshotError> {
    fs.read_to_string(path).await.map_err(|err| match err {
        FsError::NotFound => BootSnapshotError::Read {
            path,
            reason: "not found".to_owned(),
        },
        other => BootSnapshotError::Read {
            path,
            reason: other.to_string(),
        },
    })
}

/// Parses a non-empty, non-nil UUID boot id.
pub(crate) fn parse_boot_id(text: &str) -> Result<Uuid, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty".to_owned());
    }
    let id: Uuid = text
        .parse()
        .map_err(|_| format!("`{text}` is not a UUID"))?;
    if id.is_nil() {
        return Err("nil UUID".to_owned());
    }
    Ok(id)
}

/// Parses the first `/proc/uptime` field as nanoseconds.
///
/// The kernel writes hundredths of a second, so more than two fractional digits is refused
/// rather than rounded. The whole and fractional parts are combined with checked arithmetic.
pub(crate) fn parse_uptime_ns(text: &str) -> Result<u64, String> {
    let first = text
        .split_whitespace()
        .next()
        .ok_or_else(|| "empty".to_owned())?;
    if first.starts_with('-') {
        return Err(format!("`{first}` is negative"));
    }
    let (whole, fraction) = first.split_once('.').unwrap_or((first, ""));
    if first.contains('.') && fraction.is_empty() {
        return Err(format!("`{first}` ends with a decimal point"));
    }
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("`{first}` has an invalid whole part"));
    }
    if fraction.len() > 2 {
        return Err(format!("`{first}` has more than hundredths of a second"));
    }
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("`{first}` has an invalid fractional part"));
    }
    let whole: u64 = whole
        .parse()
        .map_err(|_| format!("`{whole}` does not fit in 64 bits"))?;
    let fraction_ns = match fraction.len() {
        0 => 0,
        1 => {
            let digit = u64::from(fraction.as_bytes()[0] - b'0');
            digit * 100_000_000
        }
        _ => {
            let hundredths: u64 = fraction
                .parse()
                .map_err(|_| format!("`{fraction}` is not a number"))?;
            hundredths * 10_000_000
        }
    };
    whole
        .checked_mul(1_000_000_000)
        .and_then(|ns| ns.checked_add(fraction_ns))
        .ok_or_else(|| format!("`{first}` overflows nanoseconds"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use domovoi_testkit::FixtureHostFs;

    fn snapshot(boot_id: Option<&str>, uptime: Option<&str>) -> FixtureHostFs {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.keep();
        let proc = root.join("proc");
        std::fs::create_dir_all(proc.join("sys/kernel/random")).unwrap();
        if let Some(value) = boot_id {
            std::fs::write(proc.join("sys/kernel/random/boot_id"), value).unwrap();
        }
        if let Some(value) = uptime {
            std::fs::write(proc.join("uptime"), value).unwrap();
        }
        FixtureHostFs::new(root)
    }

    #[test]
    fn normal_uptime_keeps_hundredths() {
        assert_eq!(
            parse_uptime_ns("1845.67 1234.00").unwrap(),
            1845 * 1_000_000_000 + 67 * 10_000_000
        );
        assert_eq!(parse_uptime_ns("0").unwrap(), 0);
        assert_eq!(parse_uptime_ns("0.00 0.00").unwrap(), 0);
    }

    #[test]
    fn malformed_uptime_is_refused() {
        for value in [
            "",
            "abc.00",
            "1.xx",
            "-1.00",
            "1.",
            "1.234",
            "99999999999999999999",
        ] {
            assert!(parse_uptime_ns(value).is_err(), "{value}");
        }
    }

    #[test]
    fn boot_id_is_strict() {
        assert!(parse_boot_id("").is_err());
        assert!(parse_boot_id("not-a-uuid").is_err());
        assert!(parse_boot_id(&Uuid::nil().to_string()).is_err());
        assert!(parse_boot_id("018f0000-0000-7000-8000-000000000001").is_ok());
    }

    #[tokio::test]
    async fn missing_or_invalid_sources_fail() {
        assert!(boot_snapshot(&snapshot(None, Some("1.00"))).await.is_err());
        assert!(
            boot_snapshot(&snapshot(Some(""), Some("1.00")))
                .await
                .is_err()
        );
        assert!(
            boot_snapshot(&snapshot(Some("bad"), Some("1.00")))
                .await
                .is_err()
        );
        assert!(
            boot_snapshot(&snapshot(Some(&Uuid::nil().to_string()), Some("1.00")))
                .await
                .is_err()
        );
        assert!(
            boot_snapshot(&snapshot(
                Some("018f0000-0000-7000-8000-000000000001"),
                None
            ))
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn valid_snapshot_is_returned() {
        let snapshot = boot_snapshot(&snapshot(
            Some("018f0000-0000-7000-8000-000000000001"),
            Some("12.34 5.00"),
        ))
        .await
        .unwrap();
        assert_eq!(
            snapshot.boot_id,
            Uuid::parse_str("018f0000-0000-7000-8000-000000000001").unwrap()
        );
        assert_eq!(snapshot.monotonic_ns, 12 * 1_000_000_000 + 34 * 10_000_000);
    }
}
