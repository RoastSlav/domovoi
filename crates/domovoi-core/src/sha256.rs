//! A macro for SHA-256 newtypes that display and serialize as lowercase hex.

/// Defines a `Copy` newtype over `[u8; 32]` with hex `Display`, `FromStr`, `Debug` and serde.
macro_rules! sha256_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Hashes `input` with SHA-256.
            #[must_use]
            pub(crate) fn digest(input: &[u8]) -> Self {
                use sha2::Digest as _;
                Self(sha2::Sha256::digest(input).into())
            }

            /// The raw 32 bytes of the hash.
            #[must_use]
            pub fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&hex::encode(self.0))
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({self})", stringify!($name))
            }
        }

        impl std::str::FromStr for $name {
            type Err = hex::FromHexError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let mut bytes = [0u8; 32];
                hex::decode_to_slice(s, &mut bytes)?;
                Ok(Self(bytes))
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use sha256_newtype;
