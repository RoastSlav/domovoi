//! Settings from four layers, lowest first: built-in defaults, values saved in the web
//! UI, `hub.toml`, `DOMOVOI_*` variables (ADR 0017).
//!
//! Layers are merged on the TOML value tree before deserializing, so the result goes
//! through the same deserialization, secret resolution, validation and error paths as a
//! file (ADR 0013).

use std::path::Path;

use toml::{Table, Value};

use crate::env::{self, EnvSource};
use crate::hub::HubConfig;
use crate::issue::ConfigIssue;
use crate::provenance::{Provenance, Source, is_secret_table};
use crate::secret::{STORED_OUTSIDE_UI, StoredSecrets};

/// Settings that are needed before the store opens, or could lock people out of the UI.
/// They come only from the file and the environment.
pub const BOOTSTRAP_KEYS: [&str; 3] = ["hub.data_dir", "web.bind", "agent_server.bind"];

/// The merged settings, their warnings, where each field came from, and the merged
/// value tree (overrides applied, secrets as references) for showing in the UI.
#[derive(Debug)]
pub struct Layered {
    /// The validated configuration.
    pub config: HubConfig,
    /// Warnings.
    pub issues: Vec<ConfigIssue>,
    /// The layer each field came from.
    pub provenance: Provenance,
    /// The merged value tree before deserializing.
    pub merged: Table,
}

/// Why the layers did not make a valid configuration: every issue, with provenance so a
/// caller can still tell which fields are locked.
#[derive(Debug)]
pub struct LayeredError {
    /// Errors, plus warnings found before them.
    pub issues: Vec<ConfigIssue>,
    /// The layer each field came from, as far as the layers were read.
    pub provenance: Provenance,
}

/// The outcome of [`load_hub_layered`].
pub type LayeredResult = Result<Layered, LayeredError>;

/// Merges the stored document, `file` (when given) and `env` overrides, then loads the
/// result as `hub.toml` is loaded. Stored secrets (`{ stored = true }`) resolve through
/// `stored_secrets`; one that is missing there leaves its setting unset with a warning.
pub fn load_hub_layered(
    file: Option<&Path>,
    stored: &Table,
    stored_secrets: &dyn StoredSecrets,
    env: &dyn EnvSource,
) -> LayeredResult {
    let mut issues = Vec::new();
    let mut provenance = Provenance::default();

    let mut merged = stored.clone();
    for key in BOOTSTRAP_KEYS {
        if remove_path(&mut merged, key).is_some() {
            issues.push(ConfigIssue::warning(
                key,
                "this setting is only read from hub.toml or the environment; the saved value is ignored",
            ));
        }
    }
    provenance.record_layer(&merged, &Source::Ui);

    if let Some(path) = file {
        let file_table = match read_file(path) {
            Ok(table) => table,
            Err(issue) => {
                issues.push(issue);
                return Err(LayeredError { issues, provenance });
            }
        };
        let mut markers = Vec::new();
        stored_markers(&file_table, "", &mut markers);
        for marker in markers {
            issues.push(ConfigIssue::error(marker, STORED_OUTSIDE_UI));
        }
        provenance.record_layer(&file_table, &Source::File);
        merge(&mut merged, file_table);
    }
    if issues.iter().any(ConfigIssue::is_error) {
        return Err(LayeredError { issues, provenance });
    }

    prune_missing_secrets(&mut merged, stored_secrets, &mut issues);

    let (env_issues, overridden) = env::apply_overrides_tracked(&mut merged, env);
    issues.extend(env_issues);
    for (path, var) in overridden {
        provenance.record(path, Source::Env(var));
    }

    match crate::finish::<HubConfig>(merged.clone(), env, Some(stored_secrets), issues) {
        Ok((config, issues)) => Ok(Layered {
            config,
            issues,
            provenance,
            merged,
        }),
        Err(issues) => Err(LayeredError { issues, provenance }),
    }
}

fn read_file(path: &Path) -> Result<Table, ConfigIssue> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| ConfigIssue::error("", format!("cannot read {}: {err}", path.display())))?;
    text.parse()
        .map_err(|err: toml::de::Error| ConfigIssue::error("", format!("invalid TOML: {err}")))
}

/// Deep-merges `upper` over `lower`: tables merge per key; everything else (scalars,
/// arrays, secret references) replaces.
fn merge(lower: &mut Table, upper: Table) {
    for (key, value) in upper {
        match (lower.get_mut(&key), value) {
            (Some(Value::Table(low)), Value::Table(up))
                if !is_secret_table(low) && !is_secret_table(&up) =>
            {
                merge(low, up);
            }
            (_, value) => {
                lower.insert(key, value);
            }
        }
    }
}

fn remove_path(table: &mut Table, path: &str) -> Option<Value> {
    let (parents, last) = match path.rsplit_once('.') {
        Some((parents, last)) => (Some(parents), last),
        None => (None, path),
    };
    let mut current = table;
    if let Some(parents) = parents {
        for segment in parents.split('.') {
            current = current.get_mut(segment)?.as_table_mut()?;
        }
    }
    current.remove(last)
}

fn stored_markers(table: &Table, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if let Value::Table(inner) = value {
            if inner.contains_key("stored") && is_secret_table(inner) {
                out.push(path);
            } else {
                stored_markers(inner, &path, out);
            }
        }
    }
}

/// A stored secret with no readable value leaves its setting unset: an optional secret
/// is removed; a required one removes the whole thing it belongs to (a webhook, the
/// heartbeat, Discord), so nothing ever runs with an empty secret.
fn prune_missing_secrets(
    merged: &mut Table,
    stored_secrets: &dyn StoredSecrets,
    issues: &mut Vec<ConfigIssue>,
) {
    let mut markers = Vec::new();
    stored_markers(merged, "", &mut markers);
    for path in markers {
        if stored_secrets.get(&path).is_some() {
            continue;
        }
        let owner = required_secret_owner(&path).unwrap_or(&path);
        remove_path(merged, owner);
        issues.push(ConfigIssue::warning(
            path.as_str(),
            "the stored secret is missing or unreadable, so this setting is off; enter it again in Settings",
        ));
    }
}

/// For a required secret, the setting that cannot exist without it.
fn required_secret_owner(path: &str) -> Option<&str> {
    match path {
        "discord.bot_token" => Some("discord"),
        "heartbeat.url" => Some("heartbeat"),
        _ => path
            .strip_suffix(".secret")
            .filter(|owner| owner.starts_with("webhooks.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Table {
        text.parse().unwrap()
    }

    #[test]
    fn merge_replaces_arrays_and_secrets_but_merges_tables() {
        let mut low = table("a = [1]\n[t]\nx = 1\ny = 2\n[s]\nk = { stored = true }\n");
        merge(
            &mut low,
            table("a = [2]\n[t]\ny = 3\n[s]\nk = { env = \"K\" }\n"),
        );
        assert_eq!(low["a"], Value::Array(vec![Value::Integer(2)]));
        assert_eq!(low["t"]["x"], Value::Integer(1));
        assert_eq!(low["t"]["y"], Value::Integer(3));
        assert_eq!(low["s"]["k"], table("k = { env = \"K\" }")["k"]);
    }

    #[test]
    fn required_secrets_take_their_owner_with_them() {
        assert_eq!(required_secret_owner("discord.bot_token"), Some("discord"));
        assert_eq!(
            required_secret_owner("webhooks.alerts.secret"),
            Some("webhooks.alerts")
        );
        assert_eq!(required_secret_owner("llm.primary.api_key"), None);
    }
}
