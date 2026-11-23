//! Parses the duration text `systemctl show` prints for time properties.

/// A systemd time span: a finite number of microseconds, or no limit at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemdDuration {
    /// This many microseconds.
    Micros(u64),
    /// `infinity`: the property is unset.
    Infinity,
}

impl SystemdDuration {
    /// A span of `secs` whole seconds.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self::Micros(secs.saturating_mul(1_000_000))
    }
}

/// Microseconds in one unit, for the unit spellings `systemd.time(7)` accepts. `M` (month) is
/// case-sensitive and `m` is minutes, as in systemd.
fn unit_micros(unit: &str) -> Option<u64> {
    const SEC: u64 = 1_000_000;
    Some(match unit {
        "usec" | "us" | "μs" => 1,
        "msec" | "ms" => 1_000,
        "seconds" | "second" | "sec" | "s" => SEC,
        "minutes" | "minute" | "min" | "m" => 60 * SEC,
        "hours" | "hour" | "hr" | "h" => 3_600 * SEC,
        "days" | "day" | "d" => 86_400 * SEC,
        "weeks" | "week" | "w" => 7 * 86_400 * SEC,
        "months" | "month" | "M" => 2_629_800 * SEC,
        "years" | "year" | "y" => 31_557_600 * SEC,
        _ => return None,
    })
}

/// One `<number><unit>` term, in microseconds. A number with no unit is seconds, the systemd rule.
fn term_micros(number: &str, unit: &str) -> Option<u64> {
    let per = if unit.is_empty() {
        1_000_000
    } else {
        unit_micros(unit)?
    };
    let (whole, frac) = match number.split_once('.') {
        Some((w, f)) => (w, f),
        None => (number, ""),
    };
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let mut micros = whole.checked_mul(per)?;
    if !frac.is_empty() {
        let digits: String = frac.chars().take(9).collect();
        let scale = 10_u64.checked_pow(u32::try_from(digits.len()).ok()?)?;
        let part: u64 = digits.parse().ok()?;
        micros = micros.checked_add(part.checked_mul(per)? / scale)?;
    }
    Some(micros)
}

/// Parses `6s`, `1min`, `infinity`, `1min 30s`, `1h 2min`, `500ms` and the like.
///
/// Terms may be separated by spaces or run together (`1min30s`); a bare number is seconds.
/// Returns `None` for anything it does not understand, which a caller treats as a mismatch, never
/// as a pass.
#[must_use]
pub fn parse_systemd_duration(text: &str) -> Option<SystemdDuration> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text == "infinity" {
        return Some(SystemdDuration::Infinity);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut total: u64 = 0;
    let mut terms = 0;
    while i < chars.len() {
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        while chars
            .get(i)
            .is_some_and(|c| c.is_ascii_digit() || *c == '.')
        {
            i += 1;
        }
        let number: String = chars[start..i].iter().collect();
        if number.is_empty() {
            return None;
        }
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        let unit_start = i;
        while chars
            .get(i)
            .is_some_and(|c| !c.is_whitespace() && !c.is_ascii_digit() && *c != '.')
        {
            i += 1;
        }
        let unit: String = chars[unit_start..i].iter().collect();
        total = total.checked_add(term_micros(&number, &unit)?)?;
        terms += 1;
    }
    (terms > 0).then_some(SystemdDuration::Micros(total))
}
