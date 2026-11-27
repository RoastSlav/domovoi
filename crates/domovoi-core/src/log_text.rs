//! Small text helpers for log lines: a span of time people can read, and library error
//! text with the documentation links cut off.

/// A length of time as people say it: `45s`, `5m10s`, `3h12m`, `2d4h`. Two units at most,
/// so a line stays short and still tells minutes from hours.
#[must_use]
pub fn human_duration(secs: u64) -> String {
    let (days, hours, mins, rest) = (
        secs / 86_400,
        secs % 86_400 / 3600,
        secs % 3600 / 60,
        secs % 60,
    );
    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{rest:02}s")
    } else {
        format!("{rest}s")
    }
}

/// `text` without a trailing link. Library errors end with text such as
/// `...: https://docs.rs/rustls/...`, which says nothing a person can act on.
#[must_use]
pub fn without_urls(text: &str) -> String {
    let cut = ["https://", "http://"]
        .iter()
        .filter_map(|scheme| text.find(scheme))
        .min()
        .unwrap_or(text.len());
    text[..cut]
        .trim_end_matches(|c: char| c == ':' || c.is_whitespace())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_use_at_most_two_units() {
        assert_eq!(human_duration(45), "45s");
        assert_eq!(human_duration(310), "5m10s");
        assert_eq!(human_duration(3 * 3600 + 12 * 60 + 9), "3h12m");
        assert_eq!(human_duration(2 * 86_400 + 4 * 3600), "2d4h");
    }

    #[test]
    fn a_trailing_link_is_cut_off() {
        assert_eq!(
            without_urls(
                "peer closed connection without sending TLS close_notify: https://docs.rs/rustls/x"
            ),
            "peer closed connection without sending TLS close_notify"
        );
        assert_eq!(without_urls("plain"), "plain");
    }
}
