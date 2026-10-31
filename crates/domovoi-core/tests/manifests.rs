//! Skill manifest rules, hashing and the `skill.toml` wire format.

use domovoi_core::{
    ManifestError, ParamKind, ParamName, RevertSpec, Risk, SkillManifest, SkillName, VerifySpec,
};
use jiff::SignedDuration;
use sha2::{Digest, Sha256};

const SAMPLE_TOML: &str = r#"
name = "restart_container"
description = "Restart a container managed by compose"
risk = "write"
entry = "bin/restart"
timeout = "90s"
run_as = "domovoi-skills"
hosts = ["nas"]
applies_to = ["container.restart_loop"]
max_output_bytes = 65536

[[params]]
name = "container"
description = "Container name"
kind = { type = "string", pattern = "^[a-z0-9_.-]{1,64}$", max_len = 64 }

[[params]]
name = "wait_seconds"
description = "Seconds to wait before reporting"
required = false
kind = { type = "integer", min = 0, max = 300 }

[[params]]
name = "pull"
description = "Pull the image first"
kind = { type = "boolean" }

[[params]]
name = "mode"
description = "Restart mode"
kind = { type = "enum", values = ["soft", "hard"] }

[verify]
recheck = true
delay = "30s"

[revert]
entry = "bin/start"
confirm_window = "10m"
"#;

#[cfg(test)]
fn sample() -> SkillManifest {
    toml::from_str(SAMPLE_TOML).unwrap()
}

fn string_kind(pattern: &str, max_len: u16) -> ParamKind {
    ParamKind::String {
        pattern: pattern.to_owned(),
        max_len,
    }
}

fn secs(s: i64) -> SignedDuration {
    SignedDuration::from_secs(s)
}

type Edit = fn(&mut SkillManifest);

#[test]
fn manifest_validation_table() {
    let rows: Vec<(&str, Edit, bool)> = vec![
        ("sample is valid", |_| {}, true),
        (
            "duplicate param",
            |m| m.params.push(m.params[0].clone()),
            false,
        ),
        (
            "pattern must compile",
            |m| m.params[0].kind = string_kind("^([a-z$", 10),
            false,
        ),
        (
            "pattern must be anchored",
            |m| m.params[0].kind = string_kind("[a-z]+", 10),
            false,
        ),
        (
            "pattern must end anchored",
            |m| m.params[0].kind = string_kind("^[a-z]+", 10),
            false,
        ),
        (
            "escaped dollar is not an anchor",
            |m| m.params[0].kind = string_kind("^[a-z]+\\$", 10),
            false,
        ),
        (
            "max_len 0",
            |m| m.params[0].kind = string_kind("^[a-z]+$", 0),
            false,
        ),
        (
            "max_len 1024",
            |m| m.params[0].kind = string_kind("^[a-z]+$", 1024),
            true,
        ),
        (
            "max_len 1025",
            |m| m.params[0].kind = string_kind("^[a-z]+$", 1025),
            false,
        ),
        (
            "integer min > max",
            |m| m.params[1].kind = ParamKind::Integer { min: 5, max: 4 },
            false,
        ),
        (
            "integer min == max",
            |m| m.params[1].kind = ParamKind::Integer { min: 5, max: 5 },
            true,
        ),
        (
            "enum empty",
            |m| m.params[3].kind = ParamKind::Enum { values: vec![] },
            false,
        ),
        (
            "enum duplicate values",
            |m| {
                m.params[3].kind = ParamKind::Enum {
                    values: vec!["a".into(), "a".into()],
                }
            },
            false,
        ),
        ("timeout 0s", |m| m.timeout = secs(0), false),
        ("timeout 1s", |m| m.timeout = secs(1), true),
        ("timeout 1h", |m| m.timeout = secs(3600), true),
        ("timeout over 1h", |m| m.timeout = secs(3601), false),
        ("max_output 1023", |m| m.max_output_bytes = 1023, false),
        ("max_output 1 KiB", |m| m.max_output_bytes = 1024, true),
        (
            "max_output 1 MiB",
            |m| m.max_output_bytes = 1024 * 1024,
            true,
        ),
        (
            "max_output over 1 MiB",
            |m| m.max_output_bytes = 1024 * 1024 + 1,
            false,
        ),
        (
            "entry nested relative",
            |m| m.entry = "bin/run.py".into(),
            true,
        ),
        ("entry empty", |m| m.entry = String::new(), false),
        ("entry absolute", |m| m.entry = "/usr/bin/run".into(), false),
        (
            "entry backslash absolute",
            |m| m.entry = "\\run".into(),
            false,
        ),
        (
            "entry drive letter",
            |m| m.entry = "C:\\run.exe".into(),
            false,
        ),
        ("entry parent", |m| m.entry = "../run".into(), false),
        (
            "entry inner parent",
            |m| m.entry = "bin/../../run".into(),
            false,
        ),
        ("entry directory", |m| m.entry = "bin/".into(), false),
        ("entry is sh", |m| m.entry = "sh".into(), false),
        (
            "entry is nested bash",
            |m| m.entry = "bin/bash".into(),
            false,
        ),
        ("entry is zsh", |m| m.entry = "zsh".into(), false),
        ("entry is fish", |m| m.entry = "fish".into(), false),
        ("entry is dash", |m| m.entry = "dash".into(), false),
        ("entry is ash", |m| m.entry = "ash".into(), false),
        ("entry is ksh", |m| m.entry = "ksh".into(), false),
        ("entry is BASH.exe", |m| m.entry = "BASH.exe".into(), false),
        (
            "entry script named after shell",
            |m| m.entry = "bash.py".into(),
            true,
        ),
        (
            "revert entry checked too",
            |m| m.revert.as_mut().unwrap().entry = "../x".into(),
            false,
        ),
        (
            "revert window must be positive",
            |m| m.revert.as_mut().unwrap().confirm_window = secs(0),
            false,
        ),
        (
            "revert on destructive",
            |m| m.risk = Risk::Destructive,
            false,
        ),
        (
            "destructive without revert",
            |m| {
                m.risk = Risk::Destructive;
                m.revert = None;
            },
            true,
        ),
        (
            "read with revert",
            |m| {
                m.risk = Risk::Read;
                m.verify = None;
            },
            false,
        ),
        (
            "read with verify",
            |m| {
                m.risk = Risk::Read;
                m.revert = None;
            },
            false,
        ),
        (
            "read without verify or revert",
            |m| {
                m.risk = Risk::Read;
                m.revert = None;
                m.verify = None;
            },
            true,
        ),
        (
            "verify delay negative",
            |m| {
                m.verify = Some(VerifySpec {
                    recheck: true,
                    delay: secs(-1),
                })
            },
            false,
        ),
        (
            "write with revert",
            |m| {
                m.revert = Some(RevertSpec {
                    entry: "bin/undo".into(),
                    confirm_window: secs(60),
                })
            },
            true,
        ),
    ];
    for (name, edit, valid) in rows {
        let mut manifest = sample();
        edit(&mut manifest);
        assert_eq!(
            manifest.validate().is_ok(),
            valid,
            "{name}: {:?}",
            manifest.validate()
        );
    }
}

#[test]
fn manifest_validation_reports_every_problem() {
    let mut manifest = sample();
    manifest.timeout = secs(0);
    manifest.max_output_bytes = 1;
    manifest.entry = "sh".into();
    let errors = manifest.validate().unwrap_err();
    assert_eq!(errors.len(), 3, "{errors:?}");
    assert!(errors.contains(&ManifestError::TimeoutOutOfRange));
    assert!(errors.contains(&ManifestError::MaxOutputOutOfRange));
}

#[test]
fn manifest_parsing_rejects_bad_names_and_unknown_keys() {
    let cases = [
        (
            "uppercase skill name",
            SAMPLE_TOML.replace("\"restart_container\"", "\"Restart\""),
        ),
        (
            "skill name with dash",
            SAMPLE_TOML.replace("\"restart_container\"", "\"restart-container\""),
        ),
        (
            "param name starting with digit",
            SAMPLE_TOML.replace("\"container\"\ndescription", "\"1container\"\ndescription"),
        ),
        (
            "unknown top-level key",
            SAMPLE_TOML.replace("risk = \"write\"", "risk = \"write\"\nshell = true"),
        ),
        (
            "unknown kind key",
            SAMPLE_TOML.replace("max_len = 64", "max_len = 64, flags = \"i\""),
        ),
        (
            "unknown risk",
            SAMPLE_TOML.replace("risk = \"write\"", "risk = \"admin\""),
        ),
    ];
    for (name, text) in cases {
        assert_ne!(text, SAMPLE_TOML, "{name}: replacement did not apply");
        assert!(
            toml::from_str::<SkillManifest>(&text).is_err(),
            "{name} parsed"
        );
    }
    assert!(SkillName::parse(&"a".repeat(64)).is_ok());
    assert!(SkillName::parse(&"a".repeat(65)).is_err());
    assert!(ParamName::parse(&"a".repeat(32)).is_ok());
    assert!(ParamName::parse(&"a".repeat(33)).is_err());
}

#[test]
fn params_are_required_by_default() {
    let manifest = sample();
    assert!(manifest.params[0].required);
    assert!(!manifest.params[1].required);
}

#[test]
fn manifest_hash_is_sha256_of_canonical_json_and_changes_on_edit() {
    let manifest = sample();
    let expected = hex::encode(Sha256::digest(serde_json::to_vec(&manifest).unwrap()));
    assert_eq!(manifest.manifest_hash().to_string(), expected);

    let reparsed = sample();
    assert_eq!(reparsed.manifest_hash(), manifest.manifest_hash());

    let mut edited = sample();
    edited.params[0].kind = string_kind("^[a-z0-9_.-]{1,63}$", 63);
    assert_ne!(edited.manifest_hash(), manifest.manifest_hash());
}

#[test]
fn summary_validates_args_like_the_manifest() {
    let manifest = sample();
    let summary = manifest.summary();
    assert_eq!(summary.manifest_hash, manifest.manifest_hash());
    let mut args = std::collections::BTreeMap::new();
    args.insert("container".to_owned(), "plex".to_owned());
    args.insert("pull".to_owned(), "true".to_owned());
    args.insert("mode".to_owned(), "soft".to_owned());
    assert_eq!(manifest.validate_args(&args), summary.validate_args(&args));
    assert!(summary.validate_args(&args).is_ok());
    args.insert("container".to_owned(), "plex; reboot".to_owned());
    assert!(summary.validate_args(&args).is_err());
}

#[test]
fn skill_manifest_json_snapshot() {
    insta::assert_json_snapshot!(sample());
}

#[test]
fn skill_manifest_round_trips_through_toml() {
    let manifest = sample();
    let text = toml::to_string(&manifest).unwrap();
    let back: SkillManifest = toml::from_str(&text).unwrap();
    assert_eq!(back, manifest);
}
