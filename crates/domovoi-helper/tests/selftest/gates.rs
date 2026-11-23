//! Source scans for the promises about repair.

use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
fn helper_repair_never_runs_at_start() {
    let mut files = vec![crate_dir().join("src/start_check.rs")];
    rust_files(&crate_dir().join("src/serve"), &mut files);
    for file in files {
        let code = code_only(&std::fs::read_to_string(&file).expect("read"));
        assert!(
            !code.contains("repair"),
            "{} must not mention repair: start never repairs",
            file.display()
        );
    }
}

#[test]
fn helper_repair_is_not_reachable_from_start_agent_hub_or_chat() {
    let crates = crate_dir().join("..");
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("crates") {
        let krate = entry.expect("entry").path();
        let name = krate
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        if name == "domovoi-helper" || !krate.join("src").is_dir() {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&krate.join("src"), &mut files);
        for file in files {
            let code = code_only(&std::fs::read_to_string(&file).expect("read"));
            let uses = code.contains("domovoi_helper::repair") || code.contains("run_repair");
            let allowed = file.ends_with("domovoi/src/cmd/helper.rs");
            if uses && !allowed {
                offenders.push(file);
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "repair is invoked only from cmd/helper.rs: {offenders:?}"
    );
}
