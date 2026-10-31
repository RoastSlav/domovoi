//! Fingerprints: the identity of "the same problem" across signals.

use crate::id::HostId;
use crate::sha256::sha256_newtype;
use crate::signal::{CheckKey, TargetKey};

sha256_newtype!(
    /// A stable hash of host, check and target.
    ///
    /// Evidence, counts, ids and timestamps never enter a fingerprint. Anything that changes
    /// from one observation to the next (measurements, retry counts, signal ids, observation
    /// times, summaries, severities) belongs in evidence, otherwise every observation would
    /// look like a new problem and incidents could not fold recurrences.
    Fingerprint
);

impl Fingerprint {
    /// Computes the fingerprint as SHA-256 over `"fp:v1\n{host}\n{check}\n{target or empty}"`.
    #[must_use]
    pub fn of(host: HostId, check: &CheckKey, target: Option<&TargetKey>) -> Self {
        let text = format!(
            "fp:v1\n{host}\n{check}\n{}",
            target.map_or("", TargetKey::as_str)
        );
        Self::digest(text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn host(n: u128) -> HostId {
        HostId::from_uuid(Uuid::from_u128(n))
    }

    fn check(s: &str) -> CheckKey {
        CheckKey::parse(s).unwrap()
    }

    fn target(s: &str) -> TargetKey {
        TargetKey::parse(s).unwrap()
    }

    #[test]
    fn fingerprint_differs_by_host_check_and_target() {
        let base = Fingerprint::of(host(1), &check("disk.free"), Some(&target("/mnt/a")));
        assert_ne!(
            base,
            Fingerprint::of(host(2), &check("disk.free"), Some(&target("/mnt/a")))
        );
        assert_ne!(
            base,
            Fingerprint::of(host(1), &check("disk.smart"), Some(&target("/mnt/a")))
        );
        assert_ne!(
            base,
            Fingerprint::of(host(1), &check("disk.free"), Some(&target("/mnt/b")))
        );
        assert_ne!(base, Fingerprint::of(host(1), &check("disk.free"), None));
    }

    #[test]
    fn fingerprint_round_trips_through_hex() {
        let fp = Fingerprint::of(host(1), &check("disk.free"), None);
        assert_eq!(fp.to_string().parse::<Fingerprint>().unwrap(), fp);
        let json = serde_json::to_string(&fp).unwrap();
        assert_eq!(serde_json::from_str::<Fingerprint>(&json).unwrap(), fp);
    }

    // Guards against accidental algorithm changes; verified against an independent SHA-256.
    const EXPECTED: &str = "fd6884a3716024e4ce757dd3fa236707e1506f1b8f28e287904d48f30065f994";

    #[test]
    fn fingerprint_is_stable() {
        let fp = Fingerprint::of(host(1), &check("disk.free"), Some(&target("/mnt/data")));
        assert_eq!(fp.to_string(), EXPECTED);
    }
}
