//! Where environment variables come from, and `DOMOVOI_*` overrides.

use std::collections::BTreeMap;

use toml::{Table, Value};

use crate::issue::ConfigIssue;

/// The prefix of override variables.
pub const OVERRIDE_PREFIX: &str = "DOMOVOI_";

/// The top-level tables of `hub.toml`.
pub const HUB_SECTIONS: [&str; 13] = [
    "hub",
    "web",
    "agent_server",
    "llm",
    "triage",
    "incidents",
    "notify",
    "discord",
    "webhooks",
    "probes",
    "heartbeat",
    "backup",
    "secrets",
];

/// The top-level tables of `agent.toml`.
pub const AGENT_SECTIONS: [&str; 6] = [
    "agent",
    "policy",
    "docker",
    "checks",
    "containers",
    "local_webhooks",
];

/// Which `DOMOVOI_*` variables a config reads: one process can run the hub and an agent
/// (single-box mode) with one environment, so each skips the other's sections. Any
/// other unknown section is still reported, so typos are not silently ignored.
pub trait OverrideScope {
    /// Top-level sections that belong to the other role.
    fn foreign_sections() -> &'static [&'static str];
}

/// A source of environment variables, so loading never depends on the process
/// environment in tests.
pub trait EnvSource {
    /// The value of `name`, if set and valid Unicode.
    fn var(&self, name: &str) -> Option<String>;
    /// Every variable, in any order.
    fn vars(&self) -> Vec<(String, String)>;
}

/// The process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl EnvSource for SystemEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn vars(&self) -> Vec<(String, String)> {
        std::env::vars().collect()
    }
}

/// A fixed set of variables, for tests and for validating a file without the process
/// environment.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    vars: BTreeMap<String, String>,
}

impl MapEnv {
    /// An empty environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The same environment with `name` set to `value`.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(name.into(), value.into());
        self
    }
}

impl EnvSource for MapEnv {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn vars(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// Applies `DOMOVOI_<SECTION>__<KEY>` overrides to `table`, skipping variables for the
/// `foreign` top-level sections.
///
/// Only variables with at least one `__` after the prefix are overrides; segments are
/// lowercased and name nested keys. Only scalar values can be set. An existing value
/// keeps its type (the variable is parsed as a bool, integer, float or string to match);
/// a new value is inferred as bool, then integer, then float, then string.
pub(crate) fn apply_scoped(
    table: &mut Table,
    env: &dyn EnvSource,
    foreign: &[&str],
) -> Vec<ConfigIssue> {
    apply_overrides_tracked(table, env, foreign).0
}

/// Like [`apply_scoped`], and also returns which variable set each dotted path.
pub(crate) fn apply_overrides_tracked(
    table: &mut Table,
    env: &dyn EnvSource,
    foreign: &[&str],
) -> (Vec<ConfigIssue>, BTreeMap<String, String>) {
    let mut vars = env.vars();
    vars.sort();
    let mut issues = Vec::new();
    let mut set = BTreeMap::new();
    for (name, raw) in vars {
        let Some(rest) = name.strip_prefix(OVERRIDE_PREFIX) else {
            continue;
        };
        if !rest.contains("__") {
            continue;
        }
        let segments: Vec<String> = rest.split("__").map(str::to_ascii_lowercase).collect();
        let path = segments.join(".");
        if segments
            .first()
            .is_some_and(|first| foreign.contains(&first.as_str()))
        {
            continue;
        }
        if segments.iter().any(String::is_empty) {
            issues.push(ConfigIssue::error(
                path,
                format!("{name} has an empty segment; use DOMOVOI_SECTION__KEY"),
            ));
            continue;
        }
        match set_scalar(table, &segments, &raw) {
            Ok(()) => {
                set.insert(path, name);
            }
            Err(message) => issues.push(ConfigIssue::error(path, format!("{name}: {message}"))),
        }
    }
    (issues, set)
}

fn set_scalar(table: &mut Table, segments: &[String], raw: &str) -> Result<(), String> {
    let Some((last, parents)) = segments.split_last() else {
        return Ok(());
    };
    let mut current = table;
    for segment in parents {
        let entry = current
            .entry(segment.clone())
            .or_insert_with(|| Value::Table(Table::new()));
        current = match entry {
            Value::Table(inner) => inner,
            _ => {
                return Err(format!(
                    "`{segment}` is not a table, so it has no keys to set"
                ));
            }
        };
    }
    let value = match current.get(last) {
        Some(existing) => parse_like(existing, raw)?,
        None => infer(raw),
    };
    current.insert(last.clone(), value);
    Ok(())
}

fn parse_like(existing: &Value, raw: &str) -> Result<Value, String> {
    match existing {
        Value::String(_) => Ok(Value::String(raw.to_owned())),
        Value::Boolean(_) => raw
            .parse()
            .map(Value::Boolean)
            .map_err(|_| "expected true or false".to_owned()),
        Value::Integer(_) => raw
            .parse()
            .map(Value::Integer)
            .map_err(|_| "expected an integer".to_owned()),
        Value::Float(_) => raw
            .parse()
            .map(Value::Float)
            .map_err(|_| "expected a number".to_owned()),
        Value::Datetime(_) => Err("dates cannot be overridden; set this in the file".to_owned()),
        Value::Array(_) | Value::Table(_) => Err(
            "only scalar values can be overridden from the environment; set this in the file"
                .to_owned(),
        ),
    }
}

fn infer(raw: &str) -> Value {
    if let Ok(b) = raw.parse::<bool>() {
        Value::Boolean(b)
    } else if let Ok(i) = raw.parse::<i64>() {
        Value::Integer(i)
    } else if let Ok(f) = raw.parse::<f64>()
        && f.is_finite()
    {
        Value::Float(f)
    } else {
        Value::String(raw.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> Table {
        text.parse().unwrap()
    }

    #[test]
    fn existing_values_keep_their_type() {
        let mut t = table("[triage]\nconcurrency = 2\n[discord]\nname = \"x\"\n");
        let env = MapEnv::new()
            .with("DOMOVOI_TRIAGE__CONCURRENCY", "4")
            .with("DOMOVOI_DISCORD__NAME", "12");
        assert!(apply_scoped(&mut t, &env, &[]).is_empty());
        assert_eq!(t["triage"]["concurrency"], Value::Integer(4));
        assert_eq!(t["discord"]["name"], Value::String("12".into()));
    }

    #[test]
    fn new_values_are_inferred_and_unrelated_vars_ignored() {
        let mut t = Table::new();
        let env = MapEnv::new()
            .with("DOMOVOI_A__B", "true")
            .with("DOMOVOI_A__C", "1.5")
            .with("DOMOVOI_A__D", "text")
            .with("DOMOVOI_LOG", "debug")
            .with("PATH", "/bin");
        assert!(apply_scoped(&mut t, &env, &[]).is_empty());
        assert_eq!(t["a"]["b"], Value::Boolean(true));
        assert_eq!(t["a"]["c"], Value::Float(1.5));
        assert_eq!(t["a"]["d"], Value::String("text".into()));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn variables_for_the_other_role_are_skipped_but_typos_are_kept() {
        let mut t = Table::new();
        let env = MapEnv::new()
            .with("DOMOVOI_AGENT_SERVER__BIND", "0.0.0.0:1")
            .with("DOMOVOI_WEB__BIND", "0.0.0.0:2")
            .with("DOMOVOI_AGNET__HOST_NAME", "typo");
        assert!(apply_scoped(&mut t, &env, &HUB_SECTIONS).is_empty());
        assert!(t.get("agent_server").is_none() && t.get("web").is_none());
        assert_eq!(t["agnet"]["host_name"], Value::String("typo".into()));
    }

    #[test]
    fn mistyped_value_names_path_and_variable() {
        let mut t = table("[triage]\nenabled = true\n");
        let env = MapEnv::new().with("DOMOVOI_TRIAGE__ENABLED", "yes");
        let issues = apply_scoped(&mut t, &env, &[]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "triage.enabled");
        assert!(issues[0].message.contains("DOMOVOI_TRIAGE__ENABLED"));
    }
}
