//! Typed configuration for the Domovoi hub and agent.
//!
//! Loads `hub.toml` and `agent.toml`, applies `DOMOVOI_*` environment
//! overrides, resolves secret indirection (`{ env = ".." }` or
//! `{ file = ".." }`) and reports every validation problem with its TOML path.
//! See `ARCHITECTURE.md` §14.

pub mod agent;
pub mod duration;
pub mod env;
pub mod helper;
pub mod helper_endpoints;
pub mod helper_safety;
pub mod hub;
pub mod issue;
pub mod layered;
pub mod packages;
pub mod provenance;
mod schema;
pub mod secret;
mod snowflake;
pub mod time;
pub mod validate;
mod validate_packages;

use std::path::Path;

use serde::de::DeserializeOwned;

pub use agent::{AgentConfig, DockerMode, MutationLimits, UnattendedAllow, UnattendedSection};
pub use duration::Duration;
pub use env::{EnvSource, MapEnv, OverrideScope, SystemEnv};
pub use helper::{
    FRESH_INSTALL_GROUPS, HelperConfig, load_helper, parse_helper, valid_device_pattern,
};
pub use helper_endpoints::hub_url_ip;
pub use helper_safety::{HelperSafetyError, effective_config_lines, parse_safety_input};
pub use hub::{FALLBACK_KEEP_PRE_MIGRATE, HubConfig, PackagesHubSection};
pub use issue::{ConfigIssue, IssueSeverity};
pub use layered::{
    BOOTSTRAP_KEYS, Layered, LayeredError, LayeredResult, bootstrap_data_dir,
    bootstrap_keep_pre_migrate, load_hub_layered,
};
pub use packages::{PackageLimits, PackagesSection, TrustedKey};
pub use provenance::{Provenance, Source};
pub use secret::{NoStoredSecrets, Secret, SecretSource, Secrets, StoredSecrets};
pub use time::{LocalTime, TimeZone};
pub use validate::Validate;

/// A loaded config with its warnings, or every problem that stopped it loading
/// (errors, plus any warnings found before them).
pub type LoadResult<T> = Result<(T, Vec<ConfigIssue>), Vec<ConfigIssue>>;

/// Loads and validates `hub.toml` from `path`, reading overrides and secrets from `env`.
pub fn load_hub(path: &Path, env: &dyn EnvSource) -> LoadResult<HubConfig> {
    load(path, env)
}

/// Loads and validates `agent.toml` from `path`, reading overrides and secrets from `env`.
pub fn load_agent(path: &Path, env: &dyn EnvSource) -> LoadResult<AgentConfig> {
    load(path, env)
}

/// Parses, validates and resolves a config from TOML text, as [`load_hub`] and
/// [`load_agent`] do after reading the file.
pub fn parse<T>(text: &str, env: &dyn EnvSource) -> LoadResult<T>
where
    T: DeserializeOwned + Validate + Secrets + env::OverrideScope,
{
    let mut table: toml::Table = text.parse().map_err(|err: toml::de::Error| {
        vec![ConfigIssue::error("", format!("invalid TOML: {err}"))]
    })?;
    let issues = env::apply_scoped(&mut table, env, T::foreign_sections());
    finish(table, env, None, issues)
}

/// Deserializes, resolves secrets and validates a table that already has its overrides.
pub(crate) fn finish<T>(
    table: toml::Table,
    env: &dyn EnvSource,
    stored: Option<&dyn StoredSecrets>,
    mut issues: Vec<ConfigIssue>,
) -> LoadResult<T>
where
    T: DeserializeOwned + Validate + Secrets,
{
    issues.extend(missing_keys(&table, T::required_keys()));
    if issues.iter().any(ConfigIssue::is_error) {
        return Err(issues);
    }
    let mut config: T = match deserialize_collecting(table, &mut issues) {
        Some(config) => config,
        None => return Err(issues),
    };
    config.secrets_mut(&mut |path, secret| secret.resolve(path, env, stored, &mut issues));
    config.validate(&mut issues);
    // Errors found while deserializing count even when the rest then validates.
    if issues.iter().any(ConfigIssue::is_error) {
        Err(issues)
    } else {
        Ok((config, issues))
    }
}

/// One error for every key of `required` that `table` lacks.
/// The most keys dropped to see past a wrong value; a file with more mistakes than this is
/// reported with what was found so far.
const MAX_DESERIALIZE_ISSUES: usize = 64;

/// Deserializes `table`, and when a value is wrong records that and carries on without that
/// key (its default applies), so one run reports every wrong value and unknown key, not
/// only the first. `None` when no config could be built (a required key is missing, or an
/// error cannot be located); `issues` then holds everything found.
fn deserialize_collecting<T>(mut table: toml::Table, issues: &mut Vec<ConfigIssue>) -> Option<T>
where
    T: DeserializeOwned,
{
    let mut dropped: Vec<String> = Vec::new();
    for _ in 0..MAX_DESERIALIZE_ISSUES {
        let err = match serde_path_to_error::deserialize(toml::Value::Table(table.clone())) {
            Ok(config) => return Some(config),
            Err(err) => err,
        };
        let path = err.path().to_string();
        let path = if path == "." { String::new() } else { path };
        let raw = err.inner().message().trim().to_owned();
        let field = quoted_field(&raw, "unknown field ");
        let missing = quoted_field(&raw, "missing field ");
        let key_path = match (&field, &missing) {
            // The path of an unknown key already ends with it.
            (Some(field), _) if path.rsplit('.').next() == Some(field.as_str()) => path.clone(),
            (Some(field), _) | (None, Some(field)) => join_path(&path, field),
            (None, None) => path.clone(),
        };
        // A required key that was just dropped for being wrong is reported once, as wrong.
        if missing.is_some() && dropped.contains(&key_path) {
            return None;
        }
        issues.push(issue::from_path_error(&err));
        if missing.is_some() || !remove_key(&mut table, &key_path) {
            return None;
        }
        dropped.push(key_path);
    }
    None
}

/// The name in a message such as ``unknown field `bnd`, expected ...`` for `prefix`.
fn quoted_field(message: &str, prefix: &str) -> Option<String> {
    let rest = message.strip_prefix(prefix)?.strip_prefix('`')?;
    rest.split_once('`').map(|(name, _)| name.to_owned())
}

fn join_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

/// Removes the key at dotted `path`; false when the path goes through a list or is absent.
fn remove_key(table: &mut toml::Table, path: &str) -> bool {
    let Some((head, rest)) = path.split_once('.') else {
        return table.remove(path).is_some();
    };
    match table.get_mut(head) {
        Some(toml::Value::Table(inner)) => remove_key(inner, rest),
        _ => false,
    }
}

fn missing_keys(table: &toml::Table, required: &[&str]) -> Vec<ConfigIssue> {
    required
        .iter()
        .filter(|path| {
            let (section, key) = path.split_once('.').unwrap_or(("", path));
            match table.get(section) {
                None => true,
                // A section of the wrong type is left to the deserializer to name.
                Some(value) => value.as_table().is_some_and(|t| !t.contains_key(key)),
            }
        })
        .map(|path| {
            let key = path.rsplit('.').next().unwrap_or(path);
            ConfigIssue::error(*path, format!("missing required key `{key}`; add it"))
        })
        .collect()
}

fn load<T>(path: &Path, env: &dyn EnvSource) -> LoadResult<T>
where
    T: DeserializeOwned + Validate + Secrets + env::OverrideScope,
{
    let text = std::fs::read_to_string(path).map_err(|err| {
        vec![ConfigIssue::error(
            "",
            format!("cannot read {}: {err}", path.display()),
        )]
    })?;
    parse(&text, env)
}
