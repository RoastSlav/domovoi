//! Secrets referenced by environment variable or file, never written inline.

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use secrecy::{ExposeSecret, SecretString};
use serde::de::{self, MapAccess, Visitor};

use crate::env::EnvSource;
use crate::issue::ConfigIssue;

/// The message for a secret written inline.
pub const INLINE_SECRET: &str =
    "secrets must use { env = .. } or { file = .. }; move the value out of the file";

/// Where a secret's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    /// An environment variable name: `{ env = "NAME" }`.
    Env(String),
    /// A file whose contents are the value: `{ file = "/path" }`.
    File(PathBuf),
}

/// A secret configuration value.
///
/// Deserialization reads only the source; loading then resolves it into a
/// [`SecretString`]. `Debug` shows the source, never the value.
#[derive(Clone)]
pub struct Secret {
    source: SecretSource,
    value: SecretString,
}

impl Secret {
    /// A secret with its value already known, for building configs in code.
    #[must_use]
    pub fn resolved(source: SecretSource, value: SecretString) -> Self {
        Self { source, value }
    }

    /// Where the value comes from.
    #[must_use]
    pub fn source(&self) -> &SecretSource {
        &self.source
    }

    /// The resolved value. Empty only if the secret was never resolved, which cannot
    /// happen for a config returned by `load_hub` or `load_agent`.
    #[must_use]
    pub fn value(&self) -> &SecretString {
        &self.value
    }

    /// Reads the value from its source. Problems are pushed onto `issues` at `path`.
    pub(crate) fn resolve(
        &mut self,
        path: &str,
        env: &dyn EnvSource,
        issues: &mut Vec<ConfigIssue>,
    ) {
        match &self.source {
            SecretSource::Env(name) => match env.var(name) {
                Some(value) if !value.is_empty() => self.value = SecretString::from(value),
                Some(_) => issues.push(ConfigIssue::error(
                    path,
                    format!("environment variable {name} is empty; set it to the secret value"),
                )),
                None => issues.push(ConfigIssue::error(
                    path,
                    format!(
                        "environment variable {name} is not set; set it or use {{ file = .. }}"
                    ),
                )),
            },
            SecretSource::File(file) => {
                let file = file.clone();
                self.resolve_file(path, &file, issues);
            }
        }
    }

    fn resolve_file(&mut self, path: &str, file: &Path, issues: &mut Vec<ConfigIssue>) {
        match std::fs::read_to_string(file) {
            Ok(text) => {
                let text = text
                    .strip_suffix("\r\n")
                    .or_else(|| text.strip_suffix('\n'))
                    .unwrap_or(&text);
                if text.is_empty() {
                    issues.push(ConfigIssue::error(
                        path,
                        format!("secret file {} is empty", file.display()),
                    ));
                    return;
                }
                self.value = SecretString::from(text.to_owned());
                if let Some(warning) = permission_warning(path, file) {
                    issues.push(warning);
                }
            }
            Err(err) => issues.push(ConfigIssue::error(
                path,
                format!("cannot read secret file {}: {err}", file.display()),
            )),
        }
    }
}

#[cfg(unix)]
fn permission_warning(path: &str, file: &Path) -> Option<ConfigIssue> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(file).ok()?.permissions().mode();
    (mode & 0o077 != 0).then(|| {
        ConfigIssue::warning(
            path,
            format!(
                "secret file {} is readable by group or others (mode {:o}); run chmod 600 on it",
                file.display(),
                mode & 0o777
            ),
        )
    })
}

#[cfg(not(unix))]
fn permission_warning(_path: &str, _file: &Path) -> Option<ConfigIssue> {
    None
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.value.expose_secret() == other.value.expose_secret()
    }
}

impl<'de> serde::Deserialize<'de> for Secret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SecretVisitor)
    }
}

struct SecretVisitor;

impl<'de> Visitor<'de> for SecretVisitor {
    type Value = Secret;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a table with exactly one of `env` or `file`")
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<Secret, E> {
        Err(E::custom(INLINE_SECRET))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Secret, A::Error> {
        let mut source = None;
        while let Some(key) = map.next_key::<String>()? {
            let next = match key.as_str() {
                "env" => SecretSource::Env(map.next_value()?),
                "file" => SecretSource::File(map.next_value()?),
                other => {
                    return Err(de::Error::custom(format!(
                        "unknown key `{other}` in a secret; use {{ env = .. }} or {{ file = .. }}"
                    )));
                }
            };
            if source.replace(next).is_some() {
                return Err(de::Error::custom(
                    "a secret takes exactly one of `env` or `file`",
                ));
            }
        }
        match source {
            Some(SecretSource::Env(name)) if name.is_empty() => {
                Err(de::Error::custom("`env` must name an environment variable"))
            }
            Some(source) => Ok(Secret {
                source,
                value: SecretString::from(String::new()),
            }),
            None => Err(de::Error::custom(INLINE_SECRET)),
        }
    }
}

impl JsonSchema for Secret {
    fn schema_name() -> Cow<'static, str> {
        "Secret".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let mut schema = Schema::default();
        schema.insert(
            "description".to_owned(),
            "A secret read from an environment variable or a file; never written inline.".into(),
        );
        schema.insert(
            "oneOf".to_owned(),
            vec![
                source_schema("env", "Name of the environment variable holding the value."),
                source_schema(
                    "file",
                    "Path of a file, readable only by its owner, holding the value.",
                ),
            ]
            .into(),
        );
        schema
    }
}

fn source_schema(key: &str, description: &str) -> Schema {
    let mut property = Schema::default();
    property.insert("type".to_owned(), "string".into());
    property.insert("description".to_owned(), description.into());
    let mut properties = Schema::default();
    properties.insert(key.to_owned(), property.into());
    let mut schema = Schema::default();
    schema.insert("type".to_owned(), "object".into());
    schema.insert("properties".to_owned(), properties.into());
    schema.insert("required".to_owned(), vec![key].into());
    schema.insert("additionalProperties".to_owned(), false.into());
    schema
}

/// Visits every secret in a config with its TOML path, so loading can resolve them.
pub trait Secrets {
    /// Calls `visit` once per secret field that is present.
    fn secrets_mut(&mut self, visit: &mut dyn FnMut(&str, &mut Secret));
}
