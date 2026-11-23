//! Effective helper configuration to config lines and `config_hash` (04b section 3).

use std::collections::BTreeSet;

use crate::hash::{CONFIG_PREFIX, domain_hash};
use crate::lines::{canonical_unit, check_container};
use crate::encoding::parse_cidr_config;

/// Units that are always protected and cannot be removed by configuration.
pub const MANDATORY_UNITS: [&str; 4] = [
    "domovoi-agent.service",
    "domovoi-helper.socket",
    "domovoi-skill-runner.socket",
    "sshd.service",
];

/// Why the configuration produced no hash.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The text is not valid TOML.
    #[error("toml parse error: {0}")]
    Parse(String),
    /// A value is invalid (CIDR, name, type, unknown key, duration).
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

impl ConfigError {
    /// The `incomplete_reason` token the report carries for it.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        "config_invalid"
    }
}

/// The config part of the report: encoded values per key, already sorted and de-duplicated.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigLines {
    /// `cidr` values.
    pub cidr: Vec<String>,
    /// `protected_unit` values.
    pub protected_unit: Vec<String>,
    /// `protected_container` values.
    pub protected_container: Vec<String>,
    /// `max_ttl_s`.
    pub max_ttl_s: u64,
    /// `max_active`.
    pub max_active: u64,
}

impl ConfigLines {
    /// Full `key=value` lines in report order.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        v.extend(self.cidr.iter().map(|x| format!("cidr={x}")));
        v.extend(self.protected_unit.iter().map(|x| format!("protected_unit={x}")));
        v.extend(self.protected_container.iter().map(|x| format!("protected_container={x}")));
        v.push(format!("max_ttl_s={}", self.max_ttl_s));
        v.push(format!("max_active={}", self.max_active));
        v
    }

    /// `config_hash`.
    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        domain_hash(CONFIG_PREFIX, &self.lines())
    }
}

/// Duration grammar of `max_ttl`: `<digits><s|m|h|d>`. The contract gives only examples
/// (`24h`, `1440m`); this grammar is the harness's reading.
///
/// # Errors
/// `Invalid` for anything else or on overflow.
pub fn parse_duration_secs(s: &str) -> Result<u64, ConfigError> {
    let bad = || ConfigError::Invalid(format!("max_ttl {s:?}"));
    let (num, unit) = s.split_at(s.len().checked_sub(1).ok_or_else(bad)?);
    if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let n: u64 = num.parse().map_err(|_| bad())?;
    let mul = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return Err(bad()),
    };
    n.checked_mul(mul).ok_or_else(bad)
}

fn string_list(t: &toml::Table, key: &str) -> Result<Vec<String>, ConfigError> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| ConfigError::Invalid(format!("{key}: not a string")))
            })
            .collect(),
        Some(_) => Err(ConfigError::Invalid(format!("{key}: not an array"))),
    }
}

/// Parse `helper.toml` text into the effective configuration lines.
///
/// A missing `[safety]` table is treated like an empty one (defaults); unknown keys inside
/// `[safety]` or `[safety.containment]` are refused. Both are harness readings.
///
/// # Errors
/// [`ConfigError`] when the text does not parse or holds an invalid value; no hash exists then.
pub fn effective_config(text: &str) -> Result<ConfigLines, ConfigError> {
    let root: toml::Table = toml::from_str(text).map_err(|e| ConfigError::Parse(e.message().to_owned()))?;
    let empty = toml::Table::new();
    let safety = match root.get("safety") {
        None => &empty,
        Some(toml::Value::Table(t)) => t,
        Some(_) => return Err(ConfigError::Invalid("safety: not a table".into())),
    };
    for k in safety.keys() {
        if !["management_cidrs", "protected_units", "protected_containers", "containment"].contains(&k.as_str()) {
            return Err(ConfigError::Invalid(format!("unknown key safety.{k}")));
        }
    }
    let mut cidrs = BTreeSet::new();
    for c in string_list(safety, "management_cidrs")? {
        let cidr = parse_cidr_config(&c).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        cidrs.insert(cidr.render());
    }
    let mut units: BTreeSet<String> = MANDATORY_UNITS.iter().map(|s| (*s).to_owned()).collect();
    for u in string_list(safety, "protected_units")? {
        units.insert(canonical_unit(&u).map_err(|e| ConfigError::Invalid(e.to_string()))?);
    }
    let mut containers = BTreeSet::new();
    for c in string_list(safety, "protected_containers")? {
        check_container(&c).map_err(|e| ConfigError::Invalid(e.to_string()))?;
        containers.insert(c);
    }
    let (mut max_ttl_s, mut max_active) = (3600u64, 0u64);
    match safety.get("containment") {
        None => {}
        Some(toml::Value::Table(t)) => {
            for k in t.keys() {
                if k != "max_ttl" && k != "max_active" {
                    return Err(ConfigError::Invalid(format!("unknown key safety.containment.{k}")));
                }
            }
            if let Some(v) = t.get("max_ttl") {
                let s = v.as_str().ok_or_else(|| ConfigError::Invalid("max_ttl: not a string".into()))?;
                max_ttl_s = parse_duration_secs(s)?;
            }
            if let Some(v) = t.get("max_active") {
                let i = v.as_integer().ok_or_else(|| ConfigError::Invalid("max_active: not an integer".into()))?;
                max_active = u64::try_from(i).map_err(|_| ConfigError::Invalid("max_active: negative".into()))?;
            }
        }
        Some(_) => return Err(ConfigError::Invalid("safety.containment: not a table".into())),
    }
    // BTreeSet iteration is bytewise ascending on the encoded value; every key shares the
    // "key=" prefix so this equals sorting the full line.
    Ok(ConfigLines {
        cidr: cidrs.into_iter().collect(),
        protected_unit: units.into_iter().collect(),
        protected_container: containers.into_iter().collect(),
        max_ttl_s,
        max_active,
    })
}
