//! The strict `[safety]` reader behind the helper's safety attestation.
//!
//! [`crate::helper::HelperConfig`] reads the same section for the helper's own use and accepts
//! friendlier spellings (a `max_ttl` of `1h30m`, for example). The attestation signs a hash of
//! the effective set, so it reads the section here with the exact grammar of the report and
//! hands the raw values to `domovoi_core::attest::ConfigLines::effective`, which validates and
//! canonicalises them. A value this reader or that function refuses gives no hash.

use domovoi_core::attest::{ConfigError, ConfigLines, IncompleteReason, SafetyInput};
use serde::Deserialize;

/// Why `helper.toml` gave no effective safety set. Each variant is one signed incomplete
/// reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HelperSafetyError {
    /// The text is not TOML. The reason is `config_unreadable`.
    #[error("helper.toml is not valid TOML: {0}")]
    Unreadable(String),
    /// The text is TOML but `[safety]` holds an unknown key, a value of the wrong type or a
    /// value that fails its grammar. The reason is `config_invalid`.
    #[error("invalid [safety] configuration: {0}")]
    Invalid(String),
}

impl HelperSafetyError {
    /// The `incomplete_reason` token the helper signs for this error.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Unreadable(_) => IncompleteReason::CONFIG_UNREADABLE,
            Self::Invalid(_) => IncompleteReason::CONFIG_INVALID,
        }
    }
}

impl From<ConfigError> for HelperSafetyError {
    fn from(err: ConfigError) -> Self {
        Self::Invalid(err.to_string())
    }
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawSafety {
    #[serde(default)]
    management_cidrs: Vec<String>,
    #[serde(default)]
    protected_units: Vec<String>,
    #[serde(default)]
    protected_containers: Vec<String>,
    #[serde(default)]
    containment: RawContainment,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawContainment {
    max_ttl: Option<String>,
    max_active: Option<u64>,
}

/// Reads the `[safety]` and `[safety.containment]` tables of `helper.toml` text into raw values.
///
/// Every other table is ignored here. A missing `[safety]` equals an empty one. Unknown keys
/// under `[safety]` and `[safety.containment]` are refused.
///
/// # Errors
///
/// [`HelperSafetyError::Unreadable`] when the text is not TOML,
/// [`HelperSafetyError::Invalid`] for an unknown key or a value of the wrong type.
pub fn parse_safety_input(text: &str) -> Result<SafetyInput, HelperSafetyError> {
    let table: toml::Table = text
        .parse()
        .map_err(|err: toml::de::Error| HelperSafetyError::Unreadable(err.message().to_owned()))?;
    let raw: RawSafety = match table.get("safety") {
        None => RawSafety::default(),
        Some(value) => value
            .clone()
            .try_into()
            .map_err(|err: toml::de::Error| HelperSafetyError::Invalid(err.message().to_owned()))?,
    };
    Ok(SafetyInput {
        management_cidrs: raw.management_cidrs,
        protected_units: raw.protected_units,
        protected_containers: raw.protected_containers,
        max_ttl: raw.containment.max_ttl,
        max_active: raw.containment.max_active,
    })
}

/// The effective config lines of `helper.toml` text: [`parse_safety_input`] then
/// [`ConfigLines::effective`].
///
/// # Errors
///
/// [`HelperSafetyError`] from either step; no hash exists then.
pub fn effective_config_lines(text: &str) -> Result<ConfigLines, HelperSafetyError> {
    Ok(ConfigLines::effective(&parse_safety_input(text)?)?)
}
