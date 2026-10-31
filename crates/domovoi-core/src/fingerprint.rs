//! Fingerprints: the identity of "the same problem" across signals.

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::id::HostId;
use crate::signal::{CheckKey, TargetKey};

/// A stable hash of host, check and target.
///
/// Evidence, counts, ids and timestamps never enter a fingerprint. Anything that changes
/// from one observation to the next (measurements, retry counts, signal ids, observation
/// times, summaries, severities) belongs in evidence, otherwise every observation would
/// look like a new problem and incidents could not fold recurrences.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    /// Computes the fingerprint as SHA-256 over `"fp:v1\n{host}\n{check}\n{target or empty}"`.
    #[must_use]
    pub fn of(host: HostId, check: &CheckKey, target: Option<&TargetKey>) -> Self {
        let text = format!(
            "fp:v1\n{host}\n{check}\n{}",
            target.map_or("", TargetKey::as_str)
        );
        let digest = Sha256::digest(text.as_bytes());
        Self(digest.into())
    }

    /// The raw 32 bytes of the hash.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl FromStr for Fingerprint {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(s, &mut bytes)?;
        Ok(Self(bytes))
    }
}

impl serde::Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
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
