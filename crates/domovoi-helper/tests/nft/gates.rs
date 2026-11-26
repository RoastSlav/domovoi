//! Source scans that protect the invariants of the `nft` group. Extend them; never weaken them.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn sources(sub: &str) -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(sub);
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).expect("read");
            (p, text)
        })
        .collect()
}

#[test]
fn nft_and_conntrack_are_spawned_only_through_the_process_port() {
    for (path, text) in sources("nft") {
        for needle in [
            "std::process::Command",
            "tokio::process",
            "Command::new",
            "libc::exec",
            "Stdio",
        ] {
            assert!(!text.contains(needle), "{} uses {needle}", path.display());
        }
    }
}

#[test]
fn no_sh_c_anywhere_in_the_helper() {
    for (path, text) in sources("") {
        for needle in ["\"sh\"", "\"-c\"", "sh -c", "\"bash\"", "/bin/sh"] {
            assert!(
                !text.contains(needle),
                "{} names a shell: {needle}",
                path.display()
            );
        }
    }
}

#[test]
fn containment_runs_under_the_single_state_lock() {
    let mut openers = Vec::new();
    for (path, text) in sources("nft") {
        if text.contains("StateLock::open") {
            openers.push(
                path.file_name()
                    .expect("name")
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    openers.sort();
    // The request path opens the lock once. The two subcommands take it only to store a result,
    // and the drills take their own scratch directory's lock, never the helper's.
    assert_eq!(openers, ["command.rs", "drill.rs", "serve.rs"]);
    let current = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/attest/current.rs"),
    )
    .expect("read");
    assert!(
        !current.contains("acquire"),
        "the identity computation takes the caller's guard"
    );
}

#[test]
fn only_the_template_module_renders_nft_text() {
    for (path, text) in sources("nft") {
        for needle in [
            "create element",
            "delete element",
            "add element",
            "flush ruleset",
            "destroy",
        ] {
            let in_comment_or_test = text
                .lines()
                .filter(|l| l.contains(needle))
                .all(|l| l.trim_start().starts_with("//") || l.contains("starts_with"));
            let file = path.file_name().expect("name").to_string_lossy();
            // The self-test and the drills name no script text of their own either.
            assert!(
                in_comment_or_test || file == "fake.rs",
                "{} writes `{needle}` itself",
                path.display()
            );
        }
    }
}

#[test]
fn only_the_selftest_table_text_is_written_outside_the_templates() {
    let scenario = sources("nft/selftest")
        .into_iter()
        .find(|(p, _)| p.ends_with("scenario.rs"))
        .map(|(_, t)| t)
        .expect("scenario.rs");
    let rule_lines: Vec<&str> = scenario
        .lines()
        .filter(|l| l.contains("add rule inet"))
        .collect();
    assert_eq!(
        rule_lines.len(),
        1,
        "one extra counter rule, in the self-test's own table"
    );
    assert!(rule_lines[0].contains("{TABLE}"));
}

#[test]
fn the_long_lived_helper_never_creates_a_namespace() {
    for (path, text) in sources("") {
        let file = path.to_string_lossy();
        if file.contains("nft/selftest")
            || file.ends_with("nft/drill.rs")
            || file.ends_with("nft/command.rs")
            || file.ends_with("nft/exec.rs")
            || file.ends_with("nft/serve.rs")
        {
            continue;
        }
        for needle in ["netns", "unshare", "CLONE_NEWNET", "setns"] {
            assert!(
                !text.contains(needle),
                "{} creates or enters a namespace: {needle}",
                path.display()
            );
        }
    }
}

#[test]
fn the_request_path_never_sets_a_namespace_for_nft() {
    let serve =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/nft/serve.rs"))
            .expect("read");
    assert!(serve.contains("netns: None"));
    assert_eq!(serve.matches("netns").count(), 1);
}
