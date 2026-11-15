//! Host facts the agent reports in `hello` and `enroll`.

use domovoi_checks::system::memory::parse_meminfo;
use domovoi_core::ports::HostFs;
use domovoi_proto::messages::HostFacts;
use jiff::Timestamp;

async fn read(fs: &dyn HostFs, path: &str) -> String {
    fs.read_to_string(path).await.unwrap_or_default()
}

/// `PRETTY_NAME` from an `os-release` file, else `NAME`.
#[must_use]
pub fn parse_os_release(text: &str) -> Option<String> {
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').to_owned())
            .filter(|v| !v.is_empty())
    };
    value("PRETTY_NAME").or_else(|| value("NAME"))
}

/// The `btime` line of `/proc/stat`: when the host booted.
#[must_use]
pub fn parse_boot_time(stat: &str) -> Option<Timestamp> {
    let secs: i64 = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    Timestamp::from_second(secs).ok()
}

/// How many `processor` entries `/proc/cpuinfo` lists.
#[must_use]
pub fn parse_cpu_count(cpuinfo: &str) -> u32 {
    let count = cpuinfo
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(key, _)| key.trim() == "processor")
        })
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// `/proc/uptime` in nanoseconds: seconds with hundredth-of-a-second resolution.
///
/// This is monotonic across processes on the same boot, which `std::time::Instant` is not.
#[must_use]
pub fn parse_uptime_ns(text: &str) -> u64 {
    let Some(secs) = text.split_whitespace().next() else {
        return 0;
    };
    let (whole, frac) = secs.split_once('.').unwrap_or((secs, ""));
    let whole: u64 = whole.parse().unwrap_or(0);
    let mut hundredths = String::new();
    for ch in frac.chars().take(2) {
        if ch.is_ascii_digit() {
            hundredths.push(ch);
        }
    }
    while hundredths.len() < 2 {
        hundredths.push('0');
    }
    let frac_ns = hundredths.parse::<u64>().unwrap_or(0) * 10_000_000;
    whole.saturating_mul(1_000_000_000).saturating_add(frac_ns)
}

/// Reads the host's facts; anything unreadable is left empty or zero.
pub async fn host_facts(fs: &dyn HostFs) -> HostFacts {
    let os_release = match fs.read_to_string("/etc/os-release").await {
        Ok(text) => text,
        Err(_) => read(fs, "/usr/lib/os-release").await,
    };
    let meminfo = parse_meminfo(&read(fs, "/proc/meminfo").await);
    HostFacts {
        hostname: read(fs, "/proc/sys/kernel/hostname")
            .await
            .trim()
            .to_owned(),
        os: parse_os_release(&os_release).unwrap_or_default(),
        kernel: read(fs, "/proc/sys/kernel/osrelease")
            .await
            .trim()
            .to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        cpu_count: parse_cpu_count(&read(fs, "/proc/cpuinfo").await),
        mem_total_bytes: meminfo.get("MemTotal").copied().unwrap_or(0),
        boot_time: parse_boot_time(&read(fs, "/proc/stat").await).unwrap_or(Timestamp::UNIX_EPOCH),
        boot_id: read(fs, "/proc/sys/kernel/random/boot_id")
            .await
            .trim()
            .to_owned(),
    }
}
