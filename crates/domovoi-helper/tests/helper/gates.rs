//! Source scans that pin the helper's structural promises.

use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The code of `source` with comment text removed, so a doc comment may name what is forbidden.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn adopt_has_no_unsafe_and_no_env_mutation() {
    let mut files = Vec::new();
    rust_files(&crate_dir().join("src"), &mut files);
    assert!(!files.is_empty());
    for file in files {
        let code = code_only(&std::fs::read_to_string(&file).expect("read"));
        for word in ["unsafe", "remove_var", "set_var"] {
            assert!(
                !code.contains(word),
                "{} must not contain `{word}`",
                file.display()
            );
        }
    }
}

#[test]
fn helper_start_never_chmods_an_existing_directory() {
    let source = std::fs::read_to_string(crate_dir().join("src/start_check.rs")).expect("read");
    let code = code_only(&source);
    for word in [
        "set_permissions",
        "chmod",
        "chown",
        "fchmod",
        "fchown",
        "create_dir",
        "remove_",
        "rename",
    ] {
        assert!(!code.contains(word), "start_check.rs must not use `{word}`");
    }
    let port =
        std::fs::read_to_string(crate_dir().join("../domovoi-core/src/ports/host_inspect.rs"))
            .expect("read port");
    let body = port
        .split("pub trait HostInspect")
        .nth(1)
        .expect("the trait is declared");
    let read_only = [
        "euid",
        "egid",
        "supplementary_groups",
        "user",
        "group",
        "groups_of",
        "lstat",
        "open_facts",
        "list_dir",
        "walk_tree",
        "probe",
        "read_text",
        "kernel_release",
        "state_write_timings",
    ];
    for line in body.lines() {
        if let Some(rest) = line.trim().strip_prefix("fn ") {
            let name = rest.split('(').next().unwrap_or("");
            assert!(
                read_only.contains(&name),
                "HostInspect has a method `{name}` that is not on the read-only list"
            );
        }
    }
}
