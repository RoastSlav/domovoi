//! The effective helper configuration as report lines, and `config_hash`.
//!
//! TOML parsing is not here: the config crate reads `helper.toml` into a [`SafetyInput`] and
//! this module validates and canonicalises it, so the same input always gives the same lines.

use std::collections::BTreeSet;

use super::CONFIG_PREFIX;
use super::hash::{Hash32, domain_hash};
use super::scalar::Cidr;

/// Units that are always protected; configuration can add to them but never remove one.
pub const MANDATORY_UNITS: [&str; 4] = [
    "domovoi-agent.service",
    "domovoi-helper.socket",
    "domovoi-skill-runner.socket",
    "sshd.service",
];

/// The longest a containment may last when `max_ttl` is not configured, in seconds.
const DEFAULT_MAX_TTL_S: u64 = 3600;

/// The systemd unit types a name may end with.
const UNIT_SUFFIXES: [&str; 11] = [
    "service",
    "socket",
    "target",
    "mount",
    "automount",
    "swap",
    "timer",
    "path",
    "slice",
    "scope",
    "device",
];

/// Why a configured value produced no hash. The helper signs an incomplete report with the
/// reason `config_invalid` for any of these.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// A configured value fails its grammar.
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

fn invalid(what: &str, value: &str) -> ConfigError {
    ConfigError::Invalid(format!("{what} {value:?}"))
}

/// The raw `[safety]` values as written, before validation. Nothing here is trusted yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SafetyInput {
    /// `management_cidrs`: networks containment must never cut off.
    pub management_cidrs: Vec<String>,
    /// `protected_units`: units no write may target.
    pub protected_units: Vec<String>,
    /// `protected_containers`: containers no write may target.
    pub protected_containers: Vec<String>,
    /// `[safety.containment] max_ttl`, for example `24h`; `None` means the default.
    pub max_ttl: Option<String>,
    /// `[safety.containment] max_active`; `None` means the default of zero.
    pub max_active: Option<u64>,
}

/// The config half of a report: encoded values, sorted bytewise and de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigLines {
    /// `cidr` values.
    pub cidr: Vec<String>,
    /// `protected_unit` values.
    pub protected_unit: Vec<String>,
    /// `protected_container` values.
    pub protected_container: Vec<String>,
    /// `max_ttl_s`, in seconds.
    pub max_ttl_s: u64,
    /// `max_active`; zero means no containment.
    pub max_active: u64,
}

impl ConfigLines {
    /// The effective configuration of `input`: CIDRs normalised, unit names canonicalised and
    /// unioned with [`MANDATORY_UNITS`], everything de-duplicated and sorted. `max_ttl`
    /// defaults to 3600 seconds and `max_active` to zero.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for a CIDR, unit, container or duration that fails its grammar, or a
    /// `max_ttl` of zero. No hash exists then.
    pub fn effective(input: &SafetyInput) -> Result<Self, ConfigError> {
        let mut cidrs = BTreeSet::new();
        for text in &input.management_cidrs {
            let cidr = Cidr::parse_config(text).map_err(|_| invalid("management cidr", text))?;
            cidrs.insert(cidr.render());
        }
        let mut units: BTreeSet<String> = MANDATORY_UNITS
            .iter()
            .map(|unit| (*unit).to_owned())
            .collect();
        for name in &input.protected_units {
            units.insert(canonical_unit(name)?);
        }
        let mut containers = BTreeSet::new();
        for name in &input.protected_containers {
            check_container(name)?;
            containers.insert(name.clone());
        }
        let max_ttl_s = match &input.max_ttl {
            Some(text) => parse_max_ttl(text)?,
            None => DEFAULT_MAX_TTL_S,
        };
        Ok(Self {
            cidr: cidrs.into_iter().collect(),
            protected_unit: units.into_iter().collect(),
            protected_container: containers.into_iter().collect(),
            max_ttl_s,
            max_active: input.max_active.unwrap_or(0),
        })
    }

    /// The `key=value` lines in report order.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        lines.extend(self.cidr.iter().map(|v| format!("cidr={v}")));
        lines.extend(
            self.protected_unit
                .iter()
                .map(|v| format!("protected_unit={v}")),
        );
        lines.extend(
            self.protected_container
                .iter()
                .map(|v| format!("protected_container={v}")),
        );
        lines.push(format!("max_ttl_s={}", self.max_ttl_s));
        lines.push(format!("max_active={}", self.max_active));
        lines
    }

    /// The `config_hash`: SHA-256 of the config prefix and the lines.
    #[must_use]
    pub fn hash(&self) -> Hash32 {
        domain_hash(CONFIG_PREFIX, &self.lines())
    }
}

/// Parses `max_ttl`: digits and one lowercase unit `s`, `m`, `h` or `d`, no sign, no sum, a
/// value of at least one, in seconds.
///
/// # Errors
///
/// [`ConfigError::Invalid`] for anything else, zero, or a result that overflows `u64`.
pub fn parse_max_ttl(text: &str) -> Result<u64, ConfigError> {
    let bad = || invalid("max_ttl", text);
    let split = text.len().checked_sub(1).ok_or_else(bad)?;
    let (digits, unit) = text.split_at_checked(split).ok_or_else(bad)?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let count: u64 = digits.parse().map_err(|_| bad())?;
    let factor = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return Err(bad()),
    };
    match count.checked_mul(factor) {
        Some(seconds) if seconds >= 1 => Ok(seconds),
        _ => Err(bad()),
    }
}

/// The canonical form of a unit name: `.service` is appended when no known unit type follows
/// the last dot. Case is kept.
///
/// # Errors
///
/// [`ConfigError::Invalid`] for an empty name, one over 255 bytes, or any character outside
/// `[A-Za-z0-9:_.@-]` (a backslash is refused until the owner widens the grammar).
pub fn canonical_unit(name: &str) -> Result<String, ConfigError> {
    let valid = !name.is_empty()
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.' | b'@' | b'-'));
    if !valid {
        return Err(invalid("unit name", name));
    }
    let typed = name
        .rsplit_once('.')
        .is_some_and(|(stem, suffix)| !stem.is_empty() && UNIT_SUFFIXES.contains(&suffix));
    Ok(if typed {
        name.to_owned()
    } else {
        format!("{name}.service")
    })
}

/// Checks a container name against `[A-Za-z0-9][A-Za-z0-9_.-]{0,254}`.
///
/// # Errors
///
/// [`ConfigError::Invalid`] when the name does not match.
pub fn check_container(name: &str) -> Result<(), ConfigError> {
    let bytes = name.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= 255
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(invalid("container name", name))
    }
}
