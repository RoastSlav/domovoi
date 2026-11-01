//! Build information for `domovoi version`: git commit, build date and target.
//!
//! Reads `.git` directly instead of running `git`: only the runner spawns processes
//! (ARCHITECTURE.md R3). Missing information prints as `unknown`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let commit = std::env::var("DOMOVOI_GIT_COMMIT")
        .ok()
        .filter(|c| !c.is_empty())
        .or_else(|| git_commit(&manifest))
        .unwrap_or_else(|| "unknown".to_owned());
    emit(&format!("cargo:rustc-env=DOMOVOI_GIT_COMMIT={commit}"));
    emit(&format!(
        "cargo:rustc-env=DOMOVOI_BUILD_DATE={}",
        build_date()
    ));
    emit(&format!(
        "cargo:rustc-env=DOMOVOI_TARGET={}",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned())
    ));
    emit("cargo:rerun-if-env-changed=DOMOVOI_GIT_COMMIT");
    emit("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    if let Some(git) = find_git_dir(&manifest) {
        emit(&format!(
            "cargo:rerun-if-changed={}",
            git.join("HEAD").display()
        ));
    }
}

/// `SOURCE_DATE_EPOCH` for reproducible builds, otherwise today (UTC).
fn build_date() -> String {
    let epoch = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok());
    let now = match epoch.and_then(|s| jiff::Timestamp::from_second(s).ok()) {
        Some(t) => t,
        None => jiff::Timestamp::now(),
    };
    now.strftime("%Y-%m-%d").to_string()
}

fn find_git_dir(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|git| git.join("HEAD").is_file())
}

/// The commit HEAD points at, following one symbolic ref and `packed-refs`.
fn git_commit(manifest: &Path) -> Option<String> {
    let git = find_git_dir(manifest)?;
    let head = fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    let Some(reference) = head.strip_prefix("ref: ") else {
        return Some(short(head));
    };
    emit(&format!(
        "cargo:rerun-if-changed={}",
        git.join(reference).display()
    ));
    if let Ok(id) = fs::read_to_string(git.join(reference)) {
        return Some(short(id.trim()));
    }
    let packed = fs::read_to_string(git.join("packed-refs")).ok()?;
    packed
        .lines()
        .filter_map(|line| line.split_once(' '))
        .find(|(_, name)| *name == reference)
        .map(|(id, _)| short(id))
}

fn short(id: &str) -> String {
    id.chars().take(12).collect()
}

/// One instruction to Cargo on standard output.
fn emit(line: &str) {
    let _ = writeln!(std::io::stdout(), "{line}");
}
