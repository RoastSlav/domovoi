//! The `host_overview` tool: uptime, load, memory and disks in a few lines.

use std::fmt::Write;

use domovoi_checks::system::disk::{is_pseudo, parse_mounts};
use domovoi_checks::system::memory::parse_meminfo;
use domovoi_checks::system::reboot::parse_uptime;

use super::RequestCtx;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// Most mounts listed.
pub const MAX_MOUNTS: usize = 64;

/// The three load averages from `/proc/loadavg`.
#[must_use]
pub fn parse_loadavg(text: &str) -> Option<[f64; 3]> {
    let mut fields = text.split_whitespace().map(str::parse::<f64>);
    Some([
        fields.next()?.ok()?,
        fields.next()?.ok()?,
        fields.next()?.ok()?,
    ])
}

fn duration(secs: f64) -> String {
    let secs = secs.max(0.0) as u64;
    let (days, hours, mins) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else {
        format!("{hours}h {mins}m")
    }
}

/// Renders the overview; unreadable parts are reported as such.
pub async fn host_overview(ctx: &RequestCtx) -> String {
    let fs = &*ctx.fs;
    let mut out = String::new();
    match fs
        .read_to_string("/proc/uptime")
        .await
        .ok()
        .as_deref()
        .and_then(parse_uptime)
    {
        Some(secs) => _ = writeln!(out, "uptime: {}", duration(secs)),
        None => out.push_str("uptime: unknown\n"),
    }
    match fs
        .read_to_string("/proc/loadavg")
        .await
        .ok()
        .as_deref()
        .and_then(parse_loadavg)
    {
        Some([one, five, fifteen]) => _ = writeln!(out, "load: {one:.2} {five:.2} {fifteen:.2}"),
        None => out.push_str("load: unknown\n"),
    }
    let mem = parse_meminfo(&fs.read_to_string("/proc/meminfo").await.unwrap_or_default());
    match (mem.get("MemTotal"), mem.get("MemAvailable")) {
        (Some(&total), Some(&available)) => {
            _ = writeln!(
                out,
                "memory: {:.1} GiB available of {:.1} GiB",
                available as f64 / GIB,
                total as f64 / GIB
            );
        }
        _ => out.push_str("memory: unknown\n"),
    }
    if let (Some(&total), Some(&free)) = (mem.get("SwapTotal"), mem.get("SwapFree"))
        && total > 0
    {
        let used = total.saturating_sub(free) as f64 / GIB;
        _ = writeln!(
            out,
            "swap: {used:.1} GiB used of {:.1} GiB",
            total as f64 / GIB
        );
    }
    let mounts = parse_mounts(
        &fs.read_to_string("/proc/self/mounts")
            .await
            .unwrap_or_default(),
    );
    out.push_str("disks:\n");
    for mount in mounts
        .iter()
        .filter(|m| !is_pseudo(&m.fstype))
        .take(MAX_MOUNTS)
    {
        match ctx.fs_stats.usage(&mount.mount_point).await {
            Ok(usage) if usage.total_bytes > 0 => {
                let free_pct = usage.available_bytes as f64 / usage.total_bytes as f64 * 100.0;
                _ = writeln!(
                    out,
                    "  {} ({}): {:.1} GiB free of {:.1} GiB ({free_pct:.0}%)",
                    mount.mount_point,
                    mount.fstype,
                    usage.available_bytes as f64 / GIB,
                    usage.total_bytes as f64 / GIB,
                );
            }
            Ok(_) => {}
            Err(err) => _ = writeln!(out, "  {}: unreadable ({err})", mount.mount_point),
        }
    }
    out
}
