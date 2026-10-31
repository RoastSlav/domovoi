//! Skills: manifests that describe a script an agent may run, and validation of its arguments.

mod manifest;
mod params;
mod validate;
#[cfg(test)]
mod validate_tests;

pub use manifest::{
    ManifestHash, RevertSpec, Risk, SkillManifest, SkillName, SkillSummary, VerifySpec,
};
pub use params::{ParamKind, ParamName, ParamSpec};
pub use validate::{ArgError, ManifestError, ValidatedArgs};

/// Defines a validated identifier newtype matching `^[a-z][a-z0-9_]{0,MAX-1}$`.
macro_rules! ident_newtype {
    ($(#[$meta:meta])* $name:ident, $field:literal, $max:literal) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Validates `input`.
            pub fn parse(input: &str) -> Result<Self, $crate::error::ValidationError> {
                $crate::skill::check_ident(input, $field, $max)?;
                Ok(Self(input.to_owned()))
            }

            /// The identifier as a string slice.
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

        impl TryFrom<String> for $name {
            type Error = $crate::error::ValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(&value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

pub(crate) use ident_newtype;

/// Checks `input` against `^[a-z][a-z0-9_]{0,max-1}$`.
pub(crate) fn check_ident(
    input: &str,
    field: &'static str,
    max: usize,
) -> Result<(), crate::error::ValidationError> {
    use crate::error::ValidationError;
    let bytes = input.as_bytes();
    let Some(first) = bytes.first() else {
        return Err(ValidationError::new(field, "must not be empty"));
    };
    if bytes.len() > max {
        return Err(ValidationError::new(
            field,
            format!("must be at most {max} characters"),
        ));
    }
    if !first.is_ascii_lowercase() {
        return Err(ValidationError::new(
            field,
            "must start with a lowercase letter",
        ));
    }
    let rest_ok = bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_');
    if !rest_ok {
        return Err(ValidationError::new(
            field,
            "may contain only a-z, 0-9 and underscore",
        ));
    }
    Ok(())
}
