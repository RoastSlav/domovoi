//! Tests for the closed-vocabulary shape of a contract's argument schema and
//! for the cross-checks other rules read it through.

use serde_json::{Map, Value, json};

use super::schema::{
    SchemaError, declared_type, declares_argument, requires_argument, stated_bounds,
    validate_schema,
};

/// A schema of one property, for the tests that break one argument shape.
fn one_argument(name: &str, description: Value) -> Value {
    let mut properties = Map::new();
    properties.insert(name.to_string(), description);
    json!({
        "type": "object",
        "properties": properties,
        "required": [],
        "additionalProperties": false,
    })
}

#[test]
fn schema_accepts_read_tool_and_skill_vocabularies() {
    let schema = json!({
        "type": "object",
        "properties": {
            "device": {
                "type": "string",
                "pattern": "^/dev/",
                "minLength": 1,
                "maxLength": 32,
                "description": "Which device to read",
            },
            "limit": {
                "type": "integer",
                "pattern": "^-?[0-9]+$",
                "minimum": 0,
                "maximum": 100,
                "default": 10,
                "description": "How many lines",
            },
            "force": {
                "type": "boolean",
                "pattern": "^(true|false)$",
                "description": "Whether to force",
            },
            "mode": {
                "type": "string",
                "enum": ["fast", "safe"],
                "default": "safe",
                "description": "Which mode to use",
            },
        },
        "required": ["device"],
        "additionalProperties": false,
    });
    validate_schema(&schema).unwrap();
    assert!(declares_argument(&schema, "device"));
    assert_eq!(declared_type(&schema, "device"), Some("string".to_string()));
    assert!(requires_argument(&schema, "device"));
    assert_eq!(stated_bounds(&schema, "limit"), (Some(0), Some(100)));
    assert!(
        !declares_argument(&schema, "ghost"),
        "an undeclared argument reads as declared"
    );
    assert_eq!(declared_type(&schema, "ghost"), None);
    assert!(
        !requires_argument(&schema, "mode"),
        "an optional argument reads as required"
    );
    assert_eq!(stated_bounds(&schema, "device"), (None, None));
}

#[test]
fn schema_refuses_bad_root_shape() {
    assert_eq!(validate_schema(&json!([])), Err(SchemaError::NotObject));
    assert_eq!(
        validate_schema(&json!({
            "type": "string",
            "properties": {},
            "required": [],
            "additionalProperties": false,
        })),
        Err(SchemaError::RootTypeNotObject)
    );
    assert_eq!(
        validate_schema(&json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false,
            "version": 2,
        })),
        Err(SchemaError::RootKey {
            key: "version".to_string()
        },)
    );
    assert_eq!(
        validate_schema(&json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": true,
        })),
        Err(SchemaError::OpenAdditionalProperties)
    );
    assert_eq!(
        validate_schema(&json!({
            "type": "object",
            "properties": [],
            "required": [],
            "additionalProperties": false,
        })),
        Err(SchemaError::PropertiesNotObject)
    );
    assert_eq!(
        validate_schema(&json!({
            "type": "object",
            "properties": {},
            "required": {},
            "additionalProperties": false,
        })),
        Err(SchemaError::RequiredNotArray)
    );
}

#[test]
fn schema_refuses_bad_argument_shape() {
    assert_eq!(
        validate_schema(&one_argument("n", json!(42))),
        Err(SchemaError::PropertyNotObject {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "Wait It",
            json!({"type": "string", "description": "x"})
        )),
        Err(SchemaError::ArgumentName {
            name: "Wait It".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "array", "description": "x"}),
        )),
        Err(SchemaError::ArgumentTypeNotTransportable {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "string", "description": "x", "weird": 1})
        )),
        Err(SchemaError::PropertyKeyUnknown {
            name: "n".to_string(),
            key: "weird".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "boolean", "minimum": 1, "description": "x"})
        )),
        Err(SchemaError::ConstraintForType {
            name: "n".to_string(),
            key: "minimum".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "integer", "minimum": 0.5, "description": "x"})
        )),
        Err(SchemaError::BoundNotWhole {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "string", "minLength": -1, "description": "x"})
        )),
        Err(SchemaError::LengthNegative {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "integer", "minimum": 10, "maximum": 5, "description": "x"})
        )),
        Err(SchemaError::InvertedRange {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "string", "minLength": 8, "maxLength": 5, "description": "x"})
        )),
        Err(SchemaError::InvertedLength {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "string", "enum": ["a", "a"], "description": "x"})
        )),
        Err(SchemaError::BadEnum {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "integer", "enum": [1, 2], "description": "x"})
        )),
        Err(SchemaError::EnumNotTextType {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "string", "pattern": "", "description": "x"})
        )),
        Err(SchemaError::BlankPattern {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "string", "description": ""})
        )),
        Err(SchemaError::BlankDescription {
            name: "n".to_string()
        })
    );
}

#[test]
fn schema_refuses_bad_required_list() {
    assert_eq!(
        validate_schema(&json!({
            "type": "object",
            "properties": {},
            "required": ["ghost"],
            "additionalProperties": false,
        })),
        Err(SchemaError::UnknownRequired {
            name: "ghost".to_string()
        })
    );
    assert_eq!(
        validate_schema(&json!({
            "type": "object",
            "properties": {
                "n": {"type": "string", "description": "x"},
            },
            "required": ["n", "n"],
            "additionalProperties": false,
        })),
        Err(SchemaError::DuplicateRequired {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&json!({
            "type": "object",
            "properties": {},
            "required": [1],
            "additionalProperties": false,
        }),),
        Err(SchemaError::RequiredNotText)
    );
}

#[test]
fn schema_refuses_incoherent_default() {
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "integer", "minimum": 0, "maximum": 5, "default": 9, "description": "x"}),
        )),
        Err(SchemaError::DefaultOutsideBounds {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "integer", "default": true, "description": "x"})
        )),
        Err(SchemaError::DefaultTypeMismatch {
            name: "n".to_string()
        })
    );
    assert_eq!(
        validate_schema(&one_argument(
            "n",
            json!({"type": "string", "enum": ["a", "b"], "default": "c", "description": "x"})
        )),
        Err(SchemaError::DefaultOutsideEnum {
            name: "n".to_string()
        })
    );
}
