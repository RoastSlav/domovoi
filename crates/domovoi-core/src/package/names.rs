//! Grammar-checked package name, version and signing key id.

use sha2::{Digest as _, Sha256};

use crate::error::ValidationError;

macro_rules! text_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// The validated text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = ValidationError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::parse(s)
            }
        }

        impl TryFrom<String> for $name {
            type Error = ValidationError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::parse(&text).map_err(serde::de::Error::custom)
            }
        }
    };
}

text_newtype!(
    /// A package name: `^[a-z][a-z0-9]*(-[a-z0-9]+)*$`, 2 to 48 characters.
    PackageName
);
text_newtype!(
    /// A package version: `MAJOR.MINOR.PATCH` without leading zeros, optionally followed by
    /// `-` and dot-separated `[0-9a-z]+` pre-release parts; at most 32 characters, no `+`.
    /// Compared as exact text; this type does not order versions.
    PackageVersion
);
text_newtype!(
    /// The id of a signing key: 16 lowercase hex characters, the start of the SHA-256 of the
    /// key's 32 public bytes.
    KeyId
);

impl PackageName {
    /// Validates `input` as a package name.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        let bad = |reason: &str| ValidationError::new("package name", reason);
        if !(2..=48).contains(&input.len()) {
            return Err(bad("must be 2 to 48 characters"));
        }
        if !input.as_bytes()[0].is_ascii_lowercase() {
            return Err(bad("must start with a lowercase letter"));
        }
        if !input
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(bad("may contain only a-z, 0-9 and hyphen"));
        }
        if input.ends_with('-') || input.contains("--") {
            return Err(bad("hyphens must separate non-empty parts"));
        }
        Ok(Self(input.to_owned()))
    }
}

impl PackageVersion {
    /// Validates `input` as a package version.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        let bad = |reason: &str| ValidationError::new("package version", reason);
        if input.is_empty() || input.len() > 32 {
            return Err(bad("must be 1 to 32 characters"));
        }
        let (core, pre) = match input.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (input, None),
        };
        let numbers: Vec<&str> = core.split('.').collect();
        if numbers.len() != 3 {
            return Err(bad("must be MAJOR.MINOR.PATCH"));
        }
        for number in numbers {
            let digits = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
            if !digits || (number.len() > 1 && number.starts_with('0')) {
                return Err(bad("numbers must be digits without leading zeros"));
            }
        }
        if let Some(pre) = pre {
            let ok = pre.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            });
            if !ok {
                return Err(bad("pre-release parts must be [0-9a-z]+ separated by dots"));
            }
        }
        Ok(Self(input.to_owned()))
    }
}

impl KeyId {
    /// Validates `input` as a key id.
    pub fn parse(input: &str) -> Result<Self, ValidationError> {
        let ok = input.len() == 16
            && input
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !ok {
            return Err(ValidationError::new(
                "key id",
                "must be 16 lowercase hex characters",
            ));
        }
        Ok(Self(input.to_owned()))
    }

    /// The first 16 hex characters of the SHA-256 of the 32 raw public-key bytes.
    #[must_use]
    pub fn of_public_key(public_key: &[u8; 32]) -> KeyId {
        let digest = Sha256::digest(public_key);
        KeyId(hex::encode(&digest[..8]))
    }
}
