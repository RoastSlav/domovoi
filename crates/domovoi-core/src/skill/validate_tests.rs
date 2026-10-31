use std::collections::BTreeMap;

use super::{ArgError, ParamKind, ParamName, ParamSpec};
use crate::skill::validate::validate_params;

fn string_param(name: &str, pattern: &str, max_len: u16) -> ParamSpec {
    ParamSpec {
        name: ParamName::parse(name).unwrap(),
        description: "test".to_owned(),
        required: true,
        kind: ParamKind::String {
            pattern: pattern.to_owned(),
            max_len,
        },
    }
}

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn args_reject_injection_strings() {
    let params = [string_param("container", "^[a-zA-Z0-9_.-]{1,64}$", 64)];
    let long = "x".repeat(65);
    let hostile = [
        "plex; rm -rf /",
        "$(id)",
        "a\nb",
        "../../etc",
        long.as_str(),
    ];
    for value in hostile {
        let result = validate_params(&params, &args(&[("container", value)]));
        assert!(
            matches!(result, Err(ArgError::Invalid { .. })),
            "{value:?} was accepted"
        );
    }
    assert!(validate_params(&params, &args(&[("container", "plex")])).is_ok());
}

#[test]
fn args_reject_missing_and_unknown_params() {
    let params = [string_param("container", "^[a-z]+$", 32)];
    assert_eq!(
        validate_params(&params, &BTreeMap::new()),
        Err(ArgError::Missing("container".to_owned()))
    );
    assert_eq!(
        validate_params(&params, &args(&[("container", "plex"), ("extra", "1")])),
        Err(ArgError::Unknown("extra".to_owned()))
    );
}

#[test]
fn optional_params_may_be_omitted() {
    let mut spec = string_param("tag", "^[a-z]+$", 32);
    spec.required = false;
    let validated = validate_params(&[spec], &BTreeMap::new()).unwrap();
    assert_eq!(validated.iter().count(), 0);
}

#[test]
fn unanchored_summary_patterns_still_match_the_whole_value() {
    let params = [string_param("name", "[a-z]+", 32)];
    assert!(validate_params(&params, &args(&[("name", "ok; rm")])).is_err());
}

#[test]
fn validated_args_canonical_is_order_independent() {
    let params = [
        string_param("b", "^[a-z]+$", 32),
        string_param("a", "^[a-z]+$", 32),
    ];
    let mut first = BTreeMap::new();
    first.insert("a".to_owned(), "one".to_owned());
    first.insert("b".to_owned(), "two".to_owned());
    let mut second = BTreeMap::new();
    second.insert("b".to_owned(), "two".to_owned());
    second.insert("a".to_owned(), "one".to_owned());
    let x = validate_params(&params, &first).unwrap();
    let y = validate_params(&params, &second).unwrap();
    assert_eq!(x.canonical(), y.canonical());
    assert_eq!(x.canonical(), "b:3:two\na:3:one\n");
    let order: Vec<&str> = x.iter().map(|(k, _)| k).collect();
    assert_eq!(order, ["b", "a"]);
}

#[test]
fn integers_are_range_checked_and_normalised() {
    let params = [ParamSpec {
        name: ParamName::parse("n").unwrap(),
        description: "count".to_owned(),
        required: true,
        kind: ParamKind::Integer { min: 0, max: 300 },
    }];
    let ok = validate_params(&params, &args(&[("n", "007")])).unwrap();
    assert_eq!(ok.get("n"), Some("7"));
    for bad in ["301", "-1", "1.5", " 5", "five", ""] {
        assert!(
            validate_params(&params, &args(&[("n", bad)])).is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn booleans_and_enums_match_exactly() {
    let params = [
        ParamSpec {
            name: ParamName::parse("force").unwrap(),
            description: "flag".to_owned(),
            required: true,
            kind: ParamKind::Boolean,
        },
        ParamSpec {
            name: ParamName::parse("mode").unwrap(),
            description: "mode".to_owned(),
            required: true,
            kind: ParamKind::Enum {
                values: vec!["soft".to_owned(), "hard".to_owned()],
            },
        },
    ];
    assert!(validate_params(&params, &args(&[("force", "true"), ("mode", "soft")])).is_ok());
    assert!(validate_params(&params, &args(&[("force", "True"), ("mode", "soft")])).is_err());
    assert!(validate_params(&params, &args(&[("force", "1"), ("mode", "soft")])).is_err());
    assert!(validate_params(&params, &args(&[("force", "false"), ("mode", "Soft")])).is_err());
}
