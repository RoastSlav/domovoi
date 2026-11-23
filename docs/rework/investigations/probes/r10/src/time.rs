//! Minimal RFC 3339 UTC seconds handling for `computed_at` (`YYYY-MM-DDTHH:MM:SSZ`).

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Format unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
#[must_use]
pub fn format_unix(secs: u64) -> String {
    let s = i64::try_from(secs).unwrap_or(0);
    let (y, m, d) = civil_from_days(s.div_euclid(86_400));
    let r = s.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", r / 3600, (r % 3600) / 60, r % 60)
}

/// Strict parse back to unix seconds; `None` unless the text is exactly the canonical form.
#[must_use]
pub fn parse_unix(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z' {
        return None;
    }
    let num = |a: usize, z: usize| -> Option<i64> {
        let t = &s[a..z];
        t.bytes().all(|c| c.is_ascii_digit()).then(|| t.parse().ok()).flatten()
    };
    let (y, mo, d, h, mi, se) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || h > 23 || mi > 59 || se > 59 || d < 1 {
        return None;
    }
    let days = civil_from_days(days_from_civil(y, mo, d));
    if days != (y, mo, d) {
        return None; // day out of range for the month
    }
    u64::try_from(days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se).ok()
}
