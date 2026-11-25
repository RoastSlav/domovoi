//! Source scans that pin where the attestation key is made and how processes are started.

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
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

fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn signing_key_is_constructed_only_in_keys() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut makers = Vec::new();
    for file in files {
        let code = code_only(&std::fs::read_to_string(&file).expect("read"));
        let makes = code.contains("SigningKey") || code.contains("rand::random");
        if makes {
            makers.push(
                file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_owned(),
            );
        }
    }
    assert_eq!(
        makers,
        ["keys.rs"],
        "only keys.rs makes or generates a signing key"
    );
}

#[test]
fn the_helper_starts_no_process_itself() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    for file in files {
        let code = code_only(&std::fs::read_to_string(&file).expect("read"));
        for word in ["process::Command", "tokio::process", "Command::new"] {
            assert!(
                !code.contains(word),
                "{} must not use `{word}`: processes run through the executor port",
                file.display()
            );
        }
    }
}

#[test]
fn only_the_ip_source_names_the_ip_program() {
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut namers = Vec::new();
    for file in files {
        let code = code_only(&std::fs::read_to_string(&file).expect("read"));
        if code.contains("/usr/sbin/ip") {
            namers.push(
                file.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_owned(),
            );
        }
    }
    assert_eq!(namers, ["ip_source.rs"]);
}
