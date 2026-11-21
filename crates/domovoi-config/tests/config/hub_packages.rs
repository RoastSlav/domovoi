use crate::helpers::{assert_issue, hub, hub_errors};

const KEY: &str =
    "id = \"a1b2c3d4e5f60718\"\npublic_key = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"\n";

#[test]
fn hub_packages_defaults_and_bounds() {
    let (config, issues) = hub("").unwrap();
    let packages = config.packages;
    assert!(issues.is_empty(), "{issues:?}");
    assert!(!packages.enabled);
    assert_eq!(packages.max_artifact_bytes, 20_971_520);
    assert_eq!(packages.max_store_bytes, 536_870_912);
    assert_eq!(packages.max_artifacts, 64);
    assert_eq!(packages.approval_window.as_signed().as_secs(), 600);
    assert_eq!(packages.job_history_days, 180);
    assert!(packages.revoked.is_empty() && packages.owner_keys.is_empty());

    let errors = hub_errors(
        "[packages]\nmax_artifact_bytes = 10\nmax_store_bytes = 10\nmax_artifacts = 0\napproval_window = \"30s\"\njob_history_days = 3\n",
    );
    for field in [
        "max_artifact_bytes",
        "max_store_bytes",
        "max_artifacts",
        "approval_window",
        "job_history_days",
    ] {
        assert_issue(&errors, &format!("packages.{field}"), "must be");
    }
    let errors = hub_errors(
        "[packages]\nmax_artifact_bytes = 999999999999\nmax_store_bytes = 99999999999999\nmax_artifacts = 1001\napproval_window = \"2h\"\njob_history_days = 3651\n",
    );
    assert_eq!(errors.len(), 5, "{errors:?}");
    assert!(hub("[packages]\nmax_artifact_bytes = 1024\nmax_artifacts = 1000\napproval_window = \"1h\"\njob_history_days = 7\n").is_ok());
}

#[test]
fn hub_owner_keys_are_checked_for_shape_and_duplicates() {
    let text = format!("[[packages.owner_keys]]\n{KEY}[[packages.owner_keys]]\n{KEY}");
    assert_issue(&hub_errors(&text), "packages.owner_keys[1].id", "twice");
    let text = "[[packages.owner_keys]]\nid = \"A1\"\npublic_key = \"short\"\nlabel = \"x\"\n";
    let errors = hub_errors(text);
    assert_issue(&errors, "packages.owner_keys[0].id", "16 lowercase hex");
    assert_issue(&errors, "packages.owner_keys[0].public_key", "base64");
    assert_issue(
        &hub_errors("[packages]\nrevoked = [\"nope\"]\n"),
        "packages.revoked[0]",
        "16 lowercase hex",
    );
}

#[test]
fn hub_packages_warns_when_enabled_without_keys() {
    let (_, issues) = hub("[packages]\nenabled = true\n").unwrap();
    assert!(
        issues
            .iter()
            .any(|i| !i.is_error() && i.path == "packages.owner_keys")
    );
    let (_, issues) = hub(&format!(
        "[packages]\nenabled = true\nrevoked = [\"a1b2c3d4e5f60718\"]\n[[packages.owner_keys]]\n{KEY}"
    ))
    .unwrap();
    assert!(
        !issues.iter().any(|i| i.path == "packages.owner_keys"),
        "{issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|i| !i.is_error() && i.path == "packages.revoked[0]")
    );
}
