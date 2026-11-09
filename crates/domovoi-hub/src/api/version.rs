//! Comparing agent versions with the hub's, for the "outdated" flag.

use domovoi_proto::parse_release;

/// The hub's own version.
pub const HUB_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether agent `version` is older than `hub`. Versions that do not parse are never
/// called outdated.
#[must_use]
pub fn is_older(version: &str, hub: &str) -> bool {
    match (parse_release(version), parse_release(hub)) {
        (Some(agent), Some(hub)) => agent < hub,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_older;

    #[test]
    fn compares_numerically_and_ignores_suffixes() {
        assert!(is_older("0.9.0", "0.10.0"));
        assert!(is_older("1.2.3", "1.2.4"));
        assert!(is_older("v1.2.3-rc.1", "1.3.0"));
        assert!(!is_older("1.2.3", "1.2.3"));
        assert!(!is_older("1.3.0", "1.2.9"));
        assert!(!is_older("dev", "1.0.0"), "unparseable is not outdated");
        assert!(!is_older("1.2", "1.3.0"));
    }
}
