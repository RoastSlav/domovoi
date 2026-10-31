//! JSON Schema for fields whose types live in `domovoi-core`, which does not depend on
//! `schemars`.

use schemars::{Schema, SchemaGenerator};

use crate::agent::CheckConfig;

fn string(description: &str, pattern: Option<&str>) -> Schema {
    let mut schema = Schema::default();
    schema.insert("type".to_owned(), "string".into());
    schema.insert("description".to_owned(), description.into());
    if let Some(pattern) = pattern {
        schema.insert("pattern".to_owned(), pattern.into());
    }
    schema
}

fn one_of_strings(description: &str, values: &[&str]) -> Schema {
    let mut schema = string(description, None);
    schema.insert("enum".to_owned(), values.to_vec().into());
    schema
}

const HOST_NAME: &str = "A host slug: lowercase letters, digits and '-', at most 63 characters.";
const HOST_NAME_PATTERN: &str = "^[a-z0-9][a-z0-9-]{0,62}$";
const CHECK_KEY_PATTERN: &str = "^[a-z0-9_]+(\\.[a-z0-9_]+){0,7}$";

/// A `HostName`.
pub(crate) fn host_name(_: &mut SchemaGenerator) -> Schema {
    string(HOST_NAME, Some(HOST_NAME_PATTERN))
}

/// A `Risk`.
fn risk() -> Schema {
    one_of_strings(
        "What a skill may do: read, write or destructive.",
        &["read", "write", "destructive"],
    )
}

/// A list of `Risk`.
pub(crate) fn risk_list(_: &mut SchemaGenerator) -> Schema {
    let mut schema = Schema::default();
    schema.insert("type".to_owned(), "array".into());
    schema.insert("items".to_owned(), risk().into());
    schema
}

/// A `Criticality`.
pub(crate) fn criticality(_: &mut SchemaGenerator) -> Schema {
    one_of_strings(
        "How bad a failing check is: critical or warning.",
        &["critical", "warning"],
    )
}

/// A thresholds table: check-specific keys with scalar values.
pub(crate) fn thresholds(_: &mut SchemaGenerator) -> Schema {
    let mut value = Schema::default();
    value.insert(
        "type".to_owned(),
        vec!["string", "number", "integer", "boolean"].into(),
    );
    let mut schema = Schema::default();
    schema.insert("type".to_owned(), "object".into());
    schema.insert(
        "description".to_owned(),
        "Check-specific thresholds; each check documents its keys.".into(),
    );
    schema.insert("additionalProperties".to_owned(), value.into());
    schema
}

/// The `[checks.<check_key>]` map.
pub(crate) fn checks(generator: &mut SchemaGenerator) -> Schema {
    let mut schema = Schema::default();
    schema.insert("type".to_owned(), "object".into());
    schema.insert(
        "propertyNames".to_owned(),
        string(
            "A dotted check key such as `disk.usage`.",
            Some(CHECK_KEY_PATTERN),
        )
        .into(),
    );
    schema.insert(
        "additionalProperties".to_owned(),
        generator.subschema_for::<CheckConfig>().into(),
    );
    schema
}
