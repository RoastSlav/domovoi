//! Short phrases used in thread replies.

use jiff::SignedDuration;

/// A duration as a person would say it: `45m`, `3h`, `2d 4h`.
#[must_use]
pub fn human(duration: SignedDuration) -> String {
    let mins = duration.as_secs().max(0) / 60;
    let (days, hours, mins) = (mins / 1440, mins / 60 % 24, mins % 60);
    match (days, hours) {
        (0, 0) if mins == 0 => "under a minute".to_owned(),
        (0, 0) => format!("{mins}m"),
        (0, _) if mins == 0 => format!("{hours}h"),
        (0, _) => format!("{hours}h {mins}m"),
        (_, 0) => format!("{days}d"),
        _ => format!("{days}d {hours}h"),
    }
}

/// `1st`, `2nd`, `3rd`, `4th`, `11th`, `21st`.
#[must_use]
pub fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases() {
        assert_eq!(human(SignedDuration::from_secs(30)), "under a minute");
        assert_eq!(human(SignedDuration::from_mins(45)), "45m");
        assert_eq!(human(SignedDuration::from_mins(180)), "3h");
        assert_eq!(human(SignedDuration::from_mins(195)), "3h 15m");
        assert_eq!(human(SignedDuration::from_hours(52)), "2d 4h");
        assert_eq!(human(SignedDuration::from_hours(48)), "2d");
        let ords: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 102, 111].map(ordinal).to_vec();
        assert_eq!(
            ords,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "102nd", "111th"
            ]
        );
    }
}
