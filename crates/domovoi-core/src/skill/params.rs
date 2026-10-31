//! Parameter declarations in a skill manifest.

use super::ident_newtype;

ident_newtype!(
    /// A parameter name matching `^[a-z][a-z0-9_]{0,31}$`.
    ParamName,
    "param name",
    32
);

/// The type and constraints of one parameter value.
///
/// Every value reaches the skill as a separate argv element and a `DOMOVOI_PARAM_<NAME>`
/// variable, never through a shell.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParamKind {
    /// Text that must fully match an anchored regex and stay within `max_len` characters.
    String {
        /// Anchored regex (`^...$`) the whole value must match.
        pattern: String,
        /// Maximum length in characters, 1 to 1024.
        max_len: u16,
    },
    /// A whole number within an inclusive range.
    Integer {
        /// Smallest allowed value.
        min: i64,
        /// Largest allowed value.
        max: i64,
    },
    /// Exactly `true` or `false`.
    Boolean,
    /// One of a fixed set of strings.
    Enum {
        /// The allowed values.
        values: Vec<String>,
    },
}

fn default_required() -> bool {
    true
}

/// One declared parameter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamSpec {
    /// Name used as the argument key.
    pub name: ParamName,
    /// What the parameter means, shown to humans and the model.
    pub description: String,
    /// Whether the argument must be supplied. Defaults to `true`.
    #[serde(default = "default_required")]
    pub required: bool,
    /// Type and constraints.
    pub kind: ParamKind,
}
