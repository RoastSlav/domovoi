//! Validated keys that name what a signal is about.

use std::fmt;

use crate::error::ValidationError;

/// A dotted check name such as `container.restart_loop`.
///
/// One to eight segments of `[a-z0-9_]+`, at most 64 characters in total.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct CheckKey(String);

impl CheckKey {
    /// Validates `input` as a check key.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        if input.is_empty() || input.len() > 64 {
            return Err(ValidationError::new(
                "check key",
                "must be 1 to 64 characters",
            ));
        }
        let mut segments = 0usize;
        for segment in input.split('.') {
            segments += 1;
            let ok = !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            if !ok {
                return Err(ValidationError::new(
                    "check key",
                    "segments must be non-empty and use only a-z, 0-9 and underscore",
                ));
            }
        }
        if segments > 8 {
            return Err(ValidationError::new(
                "check key",
                "must have at most 8 segments",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The key as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CheckKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for CheckKey {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<CheckKey> for String {
    fn from(value: CheckKey) -> Self {
        value.0
    }
}

/// What a check looked at, such as a container name or a mount point.
///
/// 1 to 128 characters, trimmed, with no control characters.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct TargetKey(String);

impl TargetKey {
    /// Validates `input` as a target key.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        if input.trim() != input {
            return Err(ValidationError::new(
                "target key",
                "must not have surrounding whitespace",
            ));
        }
        let len = input.chars().count();
        if len == 0 || len > 128 {
            return Err(ValidationError::new(
                "target key",
                "must be 1 to 128 characters",
            ));
        }
        if input.chars().any(char::is_control) {
            return Err(ValidationError::new(
                "target key",
                "must not contain control characters",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The key as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TargetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TargetKey {
    type Error = ValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<TargetKey> for String {
    fn from(value: TargetKey) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_key_validation_table() {
        let max_segments = ["a"; 8].join(".");
        let nine_segments = ["a"; 9].join(".");
        let sixty_four = "a".repeat(64);
        let sixty_five = "a".repeat(65);
        let valid = [
            "disk.free",
            "container.restart_loop",
            "a",
            "a1.b_2",
            &max_segments,
            &sixty_four,
        ];
        for ok in valid {
            assert!(CheckKey::parse(ok).is_ok(), "{ok} should be valid");
        }
        let invalid = [
            "",
            "Disk.free",
            "disk..free",
            ".disk",
            "disk.",
            "disk-free",
            &nine_segments,
            &sixty_five,
        ];
        for bad in invalid {
            assert!(CheckKey::parse(bad).is_err(), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn target_key_rejects_control_characters_and_length() {
        for ok in ["plex", "/dev/sdc", "/mnt/data", "a b"] {
            assert!(TargetKey::parse(ok).is_ok(), "{ok} should be valid");
        }
        let long_ok = "x".repeat(128);
        let long_bad = "x".repeat(129);
        assert!(TargetKey::parse(&long_ok).is_ok());
        for bad in ["", " plex", "plex ", "pl\nex", "pl\u{0}ex", &long_bad] {
            assert!(TargetKey::parse(bad).is_err(), "{bad:?} should be invalid");
        }
    }
}
