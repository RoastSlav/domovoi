//! Typed configuration for the Domovoi hub and agent.
//!
//! Loads `hub.toml` and `agent.toml`, applies `DOMOVOI_*` environment
//! overrides, resolves secret indirection (`{ env = ".." }` or
//! `{ file = ".." }`) and reports every validation problem with its TOML path.
//! See `ARCHITECTURE.md` §14.

pub mod agent;
pub mod duration;
pub mod env;
pub mod hub;
pub mod issue;
pub mod layered;
pub mod provenance;
mod schema;
pub mod secret;
pub mod time;
pub mod validate;

use std::path::Path;

use serde::de::DeserializeOwned;

pub use agent::AgentConfig;
pub use duration::Duration;
pub use env::{EnvSource, MapEnv, SystemEnv};
pub use hub::HubConfig;
pub use issue::{ConfigIssue, IssueSeverity};
pub use layered::{Layered, LayeredError, LayeredResult, load_hub_layered};
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
    T: DeserializeOwned + Validate + Secrets,
{
    let mut table: toml::Table = text.parse().map_err(|err: toml::de::Error| {
        vec![ConfigIssue::error("", format!("invalid TOML: {err}"))]
    })?;
    let issues = env::apply_overrides(&mut table, env);
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
    if issues.iter().any(ConfigIssue::is_error) {
        return Err(issues);
    }
    let mut config: T =
        serde_path_to_error::deserialize(toml::Value::Table(table)).map_err(|err| {
            issues.push(issue::from_path_error(&err));
            issues.clone()
        })?;
    config.secrets_mut(&mut |path, secret| secret.resolve(path, env, stored, &mut issues));
    config.validate(&mut issues);
    if issues.iter().any(ConfigIssue::is_error) {
        Err(issues)
    } else {
        Ok((config, issues))
    }
}

fn load<T>(path: &Path, env: &dyn EnvSource) -> LoadResult<T>
where
    T: DeserializeOwned + Validate + Secrets,
{
    let text = std::fs::read_to_string(path).map_err(|err| {
        vec![ConfigIssue::error(
            "",
            format!("cannot read {}: {err}", path.display()),
        )]
    })?;
    parse(&text, env)
}
