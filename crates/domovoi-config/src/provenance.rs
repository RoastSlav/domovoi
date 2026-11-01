//! Which settings layer each field's value came from.

use std::collections::BTreeMap;

use toml::{Table, Value};

/// The layer that set a field, lowest to highest precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Nothing set it; the built-in default applies.
    Default,
    /// Saved from the web UI.
    Ui,
    /// `hub.toml`.
    File,
    /// A `DOMOVOI_*` variable, by name.
    Env(String),
}

impl Source {
    /// Whether the UI may change the field: only defaults and UI values are editable.
    #[must_use]
    pub fn editable(&self) -> bool {
        matches!(self, Self::Default | Self::Ui)
    }

    /// `default`, `ui`, `file` or `env:<VAR>`, as the API reports it.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Default => "default".to_owned(),
            Self::Ui => "ui".to_owned(),
            Self::File => "file".to_owned(),
            Self::Env(var) => format!("env:{var}"),
        }
    }
}

/// The source of every field some layer set, keyed by dotted path. Arrays and secret
/// tables are recorded as one field; tables are recorded per key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provenance(BTreeMap<String, Source>);

impl Provenance {
    /// Records every field of `layer` as coming from `source`, replacing lower layers.
    pub(crate) fn record_layer(&mut self, layer: &Table, source: &Source) {
        let mut paths = Vec::new();
        leaf_paths(layer, "", &mut paths);
        for path in paths {
            self.0.insert(path, source.clone());
        }
    }

    /// Records `path` as set by `source`.
    pub(crate) fn record(&mut self, path: String, source: Source) {
        self.0.insert(path, source);
    }

    /// Where the value at `path` comes from: the field itself, else the nearest
    /// recorded parent (a whole array or secret), else [`Source::Default`].
    #[must_use]
    pub fn source_of(&self, path: &str) -> Source {
        let mut candidate = path;
        loop {
            if let Some(source) = self.0.get(candidate) {
                return source.clone();
            }
            match candidate.rfind('.') {
                Some(dot) => candidate = &candidate[..dot],
                None => return Source::Default,
            }
        }
    }

    /// Every recorded field and its source.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Source)> {
        self.0.iter().map(|(path, source)| (path.as_str(), source))
    }
}

/// Whether `table` is a secret reference (`{ env }`, `{ file }` or `{ stored }`), which is
/// one value, not a table of settings.
pub(crate) fn is_secret_table(table: &Table) -> bool {
    !table.is_empty()
        && table
            .keys()
            .all(|key| matches!(key.as_str(), "env" | "file" | "stored"))
}

/// Dotted paths of every leaf value under `table`.
pub(crate) fn leaf_paths(table: &Table, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in table {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Table(inner) if !is_secret_table(inner) => leaf_paths(inner, &path, out),
            _ => out.push(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_recorded_parent_wins_and_unknown_is_default() {
        let mut p = Provenance::default();
        p.record("probes".to_owned(), Source::File);
        p.record("hub.name".to_owned(), Source::Ui);
        assert_eq!(p.source_of("probes.0.target"), Source::File);
        assert_eq!(p.source_of("hub.name"), Source::Ui);
        assert_eq!(p.source_of("hub.timezone"), Source::Default);
        assert!(!Source::Env("X".to_owned()).editable());
        assert_eq!(Source::Env("X".to_owned()).label(), "env:X");
    }
}
