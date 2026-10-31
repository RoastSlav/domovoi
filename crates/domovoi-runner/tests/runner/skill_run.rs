//! Building the argv and environment for a skill run.

use std::collections::BTreeMap;
use std::path::Path;

use domovoi_core::SkillManifest;
use domovoi_runner::skill_run::{argv_hash, argv_hash_hex, build_spec};
use jiff::SignedDuration;

const MANIFEST: &str = r#"
name = "ordered"
description = "Three params, one optional."
risk = "write"
entry = "run"
timeout = "45s"
run_as = "domovoi-skills"
max_output_bytes = 8192

[[params]]
name = "alpha"
description = "a"
kind = { type = "string", pattern = '^[ -~]{0,32}$', max_len = 32 }

[[params]]
name = "beta"
description = "b"
required = false
kind = { type = "integer", min = 0, max = 10 }

[[params]]
name = "gamma"
description = "c"
kind = { type = "string", pattern = '^[ -~]{0,32}$', max_len = 32 }
"#;

fn manifest() -> SkillManifest {
    let manifest: SkillManifest = toml::from_str(MANIFEST).unwrap();
    manifest.validate().unwrap();
    manifest
}

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[test]
fn skill_env_and_argv_follow_declared_order() {
    let m = manifest();
    let validated = m
        .validate_args(&args(&[("gamma", "; rm -rf /"), ("alpha", "$(id)")]))
        .unwrap();
    let spec = build_spec(
        &m,
        Path::new("/skills/ordered/run"),
        Path::new("/skills/ordered"),
        &validated,
    );
    assert_eq!(spec.program, "/skills/ordered/run");
    assert_eq!(spec.args, ["$(id)", "", "; rm -rf /"]);
    assert_eq!(
        spec.env,
        args(&[
            ("DOMOVOI_PARAM_ALPHA", "$(id)"),
            ("DOMOVOI_PARAM_GAMMA", "; rm -rf /"),
            ("DOMOVOI_SKILL", "ordered"),
        ])
    );
    assert_eq!(spec.cwd.as_deref(), Some("/skills/ordered"));
    assert_eq!(spec.timeout, SignedDuration::from_secs(45));
    assert_eq!(spec.max_output_bytes, 8192);
    assert_eq!(spec.run_as.as_deref(), Some("domovoi-skills"));
    assert_eq!(spec.stdin, None);
}

#[test]
fn argv_hash_is_stable_and_changes_with_args() {
    let m = manifest();
    let entry = Path::new("/skills/ordered/run");
    let dir = Path::new("/skills/ordered");
    let spec_for = |pairs: &[(&str, &str)]| {
        build_spec(&m, entry, dir, &m.validate_args(&args(pairs)).unwrap())
    };
    let a = spec_for(&[("alpha", "x"), ("gamma", "y")]);
    let again = spec_for(&[("gamma", "y"), ("alpha", "x")]);
    assert_eq!(argv_hash(&a), argv_hash(&again));
    assert_eq!(argv_hash_hex(&a).len(), 64);

    let different = spec_for(&[("alpha", "x"), ("gamma", "z")]);
    assert_ne!(argv_hash(&a), argv_hash(&different));
    // The separator keeps ("ab", "") and ("a", "b") apart.
    let joined = spec_for(&[("alpha", "ab"), ("gamma", "")]);
    let split = spec_for(&[("alpha", "a"), ("gamma", "b")]);
    assert_ne!(argv_hash(&joined), argv_hash(&split));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn run_skill_end_to_end() {
    use domovoi_core::SkillName;
    use domovoi_runner::SkillCatalog;

    use crate::helpers::{executor, install_skill};

    if !Path::new("/bin/sh").exists() {
        eprintln!("skipping: the fixture script needs /bin/sh");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    install_skill(dir.path(), "echo_args", "echo_args");
    let (catalog, issues) = SkillCatalog::load(dir.path());
    assert!(issues.is_empty(), "{issues:#?}");
    let skill = catalog
        .get(&SkillName::parse("echo_args").unwrap())
        .unwrap();
    let validated = skill
        .manifest()
        .validate_args(&args(&[("first", "a b; $(id)"), ("second", "7")]))
        .unwrap();
    let out = catalog
        .run_skill(skill, &validated, &executor())
        .await
        .unwrap();
    assert_eq!(out.exit_code, Some(0), "{out:?}");
    assert_eq!(out.stdout, "a b; $(id)\n7\nskill=echo_args\n");
    let spec = build_spec(
        skill.manifest(),
        skill.entry_path(),
        skill.dir(),
        &validated,
    );
    assert_eq!(out.argv_hash, argv_hash(&spec));
}
