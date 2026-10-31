//! Smoke tests for the `domovoi` command line.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn version_prints_package_version() {
    Command::cargo_bin("domovoi")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_version_subcommand() {
    Command::cargo_bin("domovoi")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("version"));
}
