//! Manifest rules and argument validation.

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;

use super::manifest::{Risk, SkillManifest, SkillSummary};
use super::params::{ParamKind, ParamSpec};

/// Entry file names that are shells; skills must be scripts or binaries run directly.
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "fish", "ash", "ksh"];

/// Cap on compiled regex size, so a manifest cannot make validation expensive.
const REGEX_SIZE_LIMIT: usize = 1 << 20;

/// One broken manifest rule.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// Two params share a name.
    #[error("param `{0}` is declared more than once")]
    DuplicateParam(String),
    /// A param's kind is malformed.
    #[error("param `{param}`: {reason}")]
    InvalidParam {
        /// The param name.
        param: String,
        /// What is wrong.
        reason: String,
    },
    /// `timeout` is outside 1 second to 1 hour.
    #[error("timeout must be between 1s and 1h")]
    TimeoutOutOfRange,
    /// `max_output_bytes` is outside 1 KiB to 1 MiB.
    #[error("max_output_bytes must be between 1024 and 1048576")]
    MaxOutputOutOfRange,
    /// An entry path is unsafe.
    #[error("entry `{entry}`: {reason}")]
    InvalidEntry {
        /// The rejected path.
        entry: String,
        /// What is wrong.
        reason: &'static str,
    },
    /// `revert` is set on a skill whose risk is not `write`.
    #[error("revert is only allowed for write skills")]
    RevertNotAllowed,
    /// `verify` is set on a read skill.
    #[error("read skills cannot have verify")]
    VerifyNotAllowed,
    /// A verify delay or revert window is out of range.
    #[error("{field}: {reason}")]
    InvalidDuration {
        /// The field.
        field: &'static str,
        /// What is wrong.
        reason: &'static str,
    },
}

/// Why a set of arguments was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArgError {
    /// A required param has no value.
    #[error("missing required argument `{0}`")]
    Missing(String),
    /// A value was given for an undeclared param.
    #[error("unknown argument `{0}`")]
    Unknown(String),
    /// A value does not satisfy its param's kind.
    #[error("argument `{param}`: {reason}")]
    Invalid {
        /// The param name.
        param: String,
        /// What is wrong.
        reason: String,
    },
}

/// Arguments that passed validation against a manifest's params.
///
/// Values are in declared param order and normalised (integers in canonical decimal form).
/// The only way to obtain one is [`SkillManifest::validate_args`] or
/// [`SkillSummary::validate_args`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedArgs {
    values: Vec<(String, String)>,
}

impl ValidatedArgs {
    /// Iterates `(name, value)` pairs in declared param order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Looks up one value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    /// The values as a map, for storing on an action request.
    #[must_use]
    pub fn to_map(&self) -> BTreeMap<String, String> {
        self.values.iter().cloned().collect()
    }

    /// A stable, unambiguous string for hashing: one `name:byte_len:value` line per value.
    #[must_use]
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for (name, value) in &self.values {
            out.push_str(&format!("{name}:{}:{value}\n", value.len()));
        }
        out
    }
}

impl SkillManifest {
    /// Checks every manifest rule and returns all violations.
    pub fn validate(&self) -> Result<(), Vec<ManifestError>> {
        let mut errors = Vec::new();
        let mut seen = BTreeSet::new();
        for spec in &self.params {
            if !seen.insert(spec.name.as_str()) {
                errors.push(ManifestError::DuplicateParam(spec.name.to_string()));
            }
            if let Err(reason) = check_kind(&spec.kind) {
                errors.push(ManifestError::InvalidParam {
                    param: spec.name.to_string(),
                    reason,
                });
            }
        }
        let timeout_ok = self.timeout >= SignedDuration::from_secs(1)
            && self.timeout <= SignedDuration::from_hours(1);
        if !timeout_ok {
            errors.push(ManifestError::TimeoutOutOfRange);
        }
        if !(1024..=1024 * 1024).contains(&self.max_output_bytes) {
            errors.push(ManifestError::MaxOutputOutOfRange);
        }
        errors.extend(check_entry(&self.entry).err());
        if let Some(revert) = &self.revert {
            if self.risk != Risk::Write {
                errors.push(ManifestError::RevertNotAllowed);
            }
            errors.extend(check_entry(&revert.entry).err());
            if revert.confirm_window <= SignedDuration::ZERO {
                errors.push(ManifestError::InvalidDuration {
                    field: "revert.confirm_window",
                    reason: "must be positive",
                });
            }
        }
        if let Some(verify) = &self.verify {
            if self.risk == Risk::Read {
                errors.push(ManifestError::VerifyNotAllowed);
            }
            if verify.delay < SignedDuration::ZERO || verify.delay > SignedDuration::from_hours(1) {
                errors.push(ManifestError::InvalidDuration {
                    field: "verify.delay",
                    reason: "must be between 0s and 1h",
                });
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validates `args` against this manifest's params.
    pub fn validate_args(
        &self,
        args: &BTreeMap<String, String>,
    ) -> Result<ValidatedArgs, ArgError> {
        validate_params(&self.params, args)
    }
}

impl SkillSummary {
    /// Validates `args` against the summarised params, with the same rules as the manifest.
    pub fn validate_args(
        &self,
        args: &BTreeMap<String, String>,
    ) -> Result<ValidatedArgs, ArgError> {
        validate_params(&self.params, args)
    }
}

fn check_kind(kind: &ParamKind) -> Result<(), String> {
    match kind {
        ParamKind::String { pattern, max_len } => {
            if !(1..=1024).contains(max_len) {
                return Err("max_len must be between 1 and 1024".to_owned());
            }
            let anchored =
                pattern.starts_with('^') && pattern.ends_with('$') && !pattern.ends_with("\\$");
            if !anchored {
                return Err("pattern must be anchored with ^ and $".to_owned());
            }
            compile(pattern).map(|_| ())
        }
        ParamKind::Integer { min, max } => {
            if min > max {
                return Err("min must not exceed max".to_owned());
            }
            Ok(())
        }
        ParamKind::Boolean => Ok(()),
        ParamKind::Enum { values } => {
            if values.is_empty() {
                return Err("enum must have at least one value".to_owned());
            }
            let unique: BTreeSet<&String> = values.iter().collect();
            if unique.len() != values.len() {
                return Err("enum values must be unique".to_owned());
            }
            Ok(())
        }
    }
}

fn compile(pattern: &str) -> Result<regex::Regex, String> {
    regex::RegexBuilder::new(&format!("^(?:{pattern})$"))
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|e| format!("pattern does not compile: {e}"))
}

fn check_entry(entry: &str) -> Result<(), ManifestError> {
    let fail = |reason| ManifestError::InvalidEntry {
        entry: entry.to_owned(),
        reason,
    };
    if entry.is_empty() {
        return Err(fail("must not be empty"));
    }
    if entry.starts_with('/') || entry.starts_with('\\') || entry.as_bytes().get(1) == Some(&b':') {
        return Err(fail("must be relative to the skill folder"));
    }
    let components: Vec<&str> = entry.split(['/', '\\']).collect();
    if components.contains(&"..") {
        return Err(fail("must not contain `..`"));
    }
    let file = components.last().copied().unwrap_or_default();
    if file.is_empty() || file == "." {
        return Err(fail("must name a file"));
    }
    let lower = file.to_ascii_lowercase();
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    if SHELLS.contains(&stem) {
        return Err(fail("must not be a shell; run the script directly"));
    }
    Ok(())
}

/// Validates `args` against `params`: every required param present, no unknown params,
/// and every value satisfying its kind.
pub(crate) fn validate_params(
    params: &[ParamSpec],
    args: &BTreeMap<String, String>,
) -> Result<ValidatedArgs, ArgError> {
    if let Some(unknown) = args
        .keys()
        .find(|k| !params.iter().any(|p| p.name.as_str() == k.as_str()))
    {
        return Err(ArgError::Unknown(unknown.clone()));
    }
    let mut values = Vec::new();
    for spec in params {
        let name = spec.name.as_str();
        let Some(raw) = args.get(name) else {
            if spec.required {
                return Err(ArgError::Missing(name.to_owned()));
            }
            continue;
        };
        let value = check_value(&spec.kind, raw).map_err(|reason| ArgError::Invalid {
            param: name.to_owned(),
            reason,
        })?;
        values.push((name.to_owned(), value));
    }
    Ok(ValidatedArgs { values })
}

fn check_value(kind: &ParamKind, raw: &str) -> Result<String, String> {
    match kind {
        ParamKind::String { pattern, max_len } => {
            if raw.chars().count() > usize::from(*max_len) {
                return Err(format!("longer than {max_len} characters"));
            }
            if !compile(pattern)?.is_match(raw) {
                return Err("does not match the declared pattern".to_owned());
            }
            Ok(raw.to_owned())
        }
        ParamKind::Integer { min, max } => {
            let n: i64 = raw.parse().map_err(|_| "not an integer".to_owned())?;
            if n < *min || n > *max {
                return Err(format!("must be between {min} and {max}"));
            }
            Ok(n.to_string())
        }
        ParamKind::Boolean => match raw {
            "true" | "false" => Ok(raw.to_owned()),
            _ => Err("must be `true` or `false`".to_owned()),
        },
        ParamKind::Enum { values } => {
            if values.iter().any(|v| v == raw) {
                Ok(raw.to_owned())
            } else {
                Err(format!("must be one of: {}", values.join(", ")))
            }
        }
    }
}
