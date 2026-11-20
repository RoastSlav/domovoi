use std::collections::BTreeMap;

use domovoi_core::capability::builtin_contracts;
use serde_json::{Value, json};

use super::schema::{SUPPORTED_KEYWORDS, check_schema};

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn schema_check_supports_each_keyword_and_no_more() {
    let schema = json!({
        "type": "object",
        "required": ["name"],
        "additionalProperties": false,
        "properties": {
            "name": { "type": "string", "minLength": 2, "maxLength": 4, "pattern": "^[a-z]+$" },
            "keep": { "type": "integer", "minimum": 1, "maximum": 30 },
            "force": { "type": "boolean" },
            "mode": { "enum": ["a", "b"] },
        },
    });
    assert!(
        check_schema(
            &schema,
            &args(&[
                ("name", "plex"),
                ("keep", "14"),
                ("force", "true"),
                ("mode", "a")
            ])
        )
        .is_ok()
    );
    for bad in [
        args(&[]),
        args(&[("name", "p")]),
        args(&[("name", "plexx")]),
        args(&[("name", "PLEX")]),
        args(&[("name", "plex"), ("keep", "0")]),
        args(&[("name", "plex"), ("keep", "31")]),
        args(&[("name", "plex"), ("keep", "x")]),
        args(&[("name", "plex"), ("force", "yes")]),
        args(&[("name", "plex"), ("mode", "c")]),
        args(&[("name", "plex"), ("other", "1")]),
    ] {
        assert!(check_schema(&schema, &bad).is_err(), "{bad:?}");
    }
}

#[test]
fn a_bad_pattern_is_a_problem_not_a_panic() {
    let schema = json!({ "properties": { "a": { "pattern": "(" } } });
    let problems = check_schema(&schema, &args(&[("a", "x")])).unwrap_err();
    assert!(problems[0].problem.contains("unusable pattern"));
}

fn keywords(value: &Value, out: &mut Vec<String>) {
    if let Value::Object(map) = value {
        for (key, inner) in map {
            out.push(key.clone());
            // `properties` holds argument names, not keywords.
            if key == "properties" {
                if let Value::Object(props) = inner {
                    props.values().for_each(|spec| keywords(spec, out));
                }
            } else {
                keywords(inner, out);
            }
        }
    }
}

#[test]
fn schema_keywords_in_use_are_all_supported() {
    let mut found = Vec::new();
    for contract in builtin_contracts() {
        keywords(&contract.input_schema, &mut found);
    }
    found.sort_unstable();
    found.dedup();
    for keyword in found {
        assert!(
            SUPPORTED_KEYWORDS.contains(&keyword.as_str()),
            "schema keyword `{keyword}` is not supported by check_schema"
        );
    }
}
