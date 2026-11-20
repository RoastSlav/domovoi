use crate::helpers::{AGENT, agent, agent_errors, assert_issue};

const KEY: &str =
    "id = \"a1b2c3d4e5f60718\"\npublic_key = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"\n";

fn with_packages(extra: &str) -> String {
    format!("{AGENT}{extra}")
}

#[test]
fn packages_section_absent_means_no_installs() {
    let (config, _) = agent(AGENT).unwrap();
    assert!(config.packages.is_none());
}

#[test]
fn packages_defaults_match_the_contract() {
    let (config, _) = agent(&with_packages("[packages]\n")).unwrap();
    let packages = config.packages.unwrap();
    assert!(!packages.allow_install);
    assert!(packages.revoked.is_empty() && packages.trusted_keys.is_empty());
    let limits = packages.limits;
    assert_eq!(limits.compressed_bytes, 20_971_520);
    assert_eq!(limits.uncompressed_bytes, 104_857_600);
    assert_eq!(limits.file_bytes, 20_971_520);
    assert_eq!(
        (limits.entries, limits.path_depth, limits.path_bytes),
        (2000, 8, 255)
    );
    assert_eq!(limits.extraction_time.as_signed().as_secs(), 60);
    assert_eq!(
        config.agent.packages_dir,
        std::path::PathBuf::from("/var/lib/domovoi-packages")
    );
}

#[test]
fn trusted_key_id_must_be_sixteen_lower_hex() {
    for id in ["A1B2C3D4E5F60718", "a1b2", "zzzzzzzzzzzzzzzz"] {
        let text = with_packages(&format!(
            "[[packages.trusted_keys]]\n{}",
            KEY.replace("a1b2c3d4e5f60718", id)
        ));
        assert_issue(
            &agent_errors(&text),
            "packages.trusted_keys[0].id",
            "16 lowercase hex",
        );
    }
    let text = with_packages(&format!(
        "[[packages.trusted_keys]]\n{}",
        KEY.replace("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", "short")
    ));
    assert_issue(
        &agent_errors(&text),
        "packages.trusted_keys[0].public_key",
        "base64",
    );
}

#[test]
fn duplicate_trusted_key_ids_are_rejected() {
    let text = with_packages(&format!(
        "[[packages.trusted_keys]]\n{KEY}[[packages.trusted_keys]]\n{KEY}"
    ));
    assert_issue(&agent_errors(&text), "packages.trusted_keys[1].id", "twice");
}

#[test]
fn limits_outside_bounds_are_rejected() {
    let text = with_packages(
        "[packages.limits]\ncompressed_bytes = 10\nentries = 0\npath_depth = 1\npath_bytes = 8\nextraction_time = \"1h\"\nfile_bytes = 999999999999\n",
    );
    let errors = agent_errors(&text);
    for field in [
        "compressed_bytes",
        "entries",
        "path_depth",
        "path_bytes",
        "extraction_time",
        "file_bytes",
    ] {
        assert_issue(&errors, &format!("packages.limits.{field}"), "must be");
    }
}

#[test]
fn revoked_entry_must_be_sixteen_hex() {
    let errors = agent_errors(&with_packages("[packages]\nrevoked = [\"nope\"]\n"));
    assert_issue(&errors, "packages.revoked[0]", "16 lowercase hex");
}

#[test]
fn packages_dir_must_be_absolute() {
    let text = AGENT.to_owned() + "packages_dir = \"relative/dir\"\n";
    assert_issue(&agent_errors(&text), "agent.packages_dir", "absolute");
}

#[test]
fn allow_install_without_keys_warns() {
    let (_, issues) = agent(&with_packages("[packages]\nallow_install = true\n")).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| !i.is_error() && i.path == "packages.allow_install")
    );
}

#[test]
fn key_trusted_and_revoked_warns() {
    let text = with_packages(&format!(
        "[packages]\nrevoked = [\"a1b2c3d4e5f60718\"]\n[[packages.trusted_keys]]\n{KEY}"
    ));
    let (_, issues) = agent(&text).unwrap();
    assert!(
        issues
            .iter()
            .any(|i| !i.is_error() && i.path == "packages.revoked[0]")
    );
}
