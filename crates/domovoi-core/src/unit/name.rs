//! Unit names and canonical ids, validated before any manager call.

use serde::{Deserialize, Serialize};

use crate::capability::TargetIdentity;
use crate::error::ValidationError;
use crate::host::KeyFingerprint;
use crate::id::HostId;
use crate::sha256::sha256_newtype;

/// The longest unit name or id, in characters. It keeps `unit:{host}/{key}/{Id}`
/// (36 + 1 + 64 + 1 + 128 = 230) inside the 300 characters a target identity allows.
pub const MAX_UNIT_NAME: usize = 128;

sha256_newtype!(
    /// SHA-256 over the encoded [`super::UnitIdentityV1`]: the loaded configuration of a unit
    /// plus the bytes of every file it loads.
    IdentityHash
);

impl IdentityHash {
    /// Hashes the already-encoded identity text.
    #[must_use]
    pub fn of_encoded(encoded: &[u8]) -> Self {
        Self::digest(encoded)
    }
}

fn check(text: &str, what: &'static str) -> Result<(), ValidationError> {
    if text.is_empty() || text.chars().count() > MAX_UNIT_NAME {
        return Err(ValidationError::new(
            what,
            format!("must be 1 to {MAX_UNIT_NAME} characters"),
        ));
    }
    if text.starts_with('-') {
        return Err(ValidationError::new(what, "must not start with `-`"));
    }
    if !text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.' | b'\\' | b'@' | b'-'))
    {
        return Err(ValidationError::new(
            what,
            "may hold only letters, digits and `: _ . \\ @ -`",
        ));
    }
    let Some(stem) = text.strip_suffix(".service") else {
        return Err(ValidationError::new(what, "must end in `.service`"));
    };
    if stem.is_empty() || stem.ends_with('@') {
        return Err(ValidationError::new(
            what,
            "must not be empty or a bare template",
        ));
    }
    Ok(())
}

/// A unit name as typed: `[A-Za-z0-9:_.\@-]`, ends in `.service`, at most 128 characters, no
/// leading `-`, no glob character and not a bare template (`x@.service`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct UnitName(String);

impl UnitName {
    /// Validates `input`.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        check(input, "unit name")?;
        Ok(Self(input.to_owned()))
    }

    /// The name as typed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UnitName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UnitName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// The canonical `Id` systemd reports for a unit. Same grammar as [`UnitName`]; an instance is
/// an exact string such as `foo@bar.service`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct UnitId(String);

impl UnitId {
    /// Validates `input`.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        check(input, "unit id")?;
        Ok(Self(input.to_owned()))
    }

    /// The id text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UnitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for UnitId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// The canonical unit identity, `unit:{host}/{key}/{Id}`.
///
/// It is stable across configuration changes: pinning and rule resolution use it. The
/// configuration enters through [`IdentityHash`], bound separately in the plan hash.
pub fn canonical_unit_identity(
    host: HostId,
    key: &KeyFingerprint,
    id: &UnitId,
) -> Result<TargetIdentity, ValidationError> {
    TargetIdentity::parse(&format!("unit:{host}/{key}/{id}"))
}
