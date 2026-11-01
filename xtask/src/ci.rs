//! `cargo xtask ci`: run the same gates as CI, in order, stopping at the first failure.

use std::path::Path;
use std::process::Command;

/// Run the local pipeline.
pub fn run(root: &Path, fast: bool, web: bool) -> anyhow::Result<()> {
    step(root, "fmt", "cargo", &["fmt", "--all", "--check"])?;
    step(
        root,
        "clippy",
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    if has_nextest(root) {
        step(
            root,
            "test",
            "cargo",
            &["nextest", "run", "--workspace", "--locked"],
        )?;
        step(
            root,
            "doctest",
            "cargo",
            &["test", "--workspace", "--doc", "--locked"],
        )?;
    } else {
        step(root, "test", "cargo", &["test", "--workspace", "--locked"])?;
    }
    // Feature-gated code is not built by the workspace steps above; `--all-features`
    // on the workspace would also turn on `domovoi/embed-ui`, which needs the web build.
    step(
        root,
        "checks features clippy",
        "cargo",
        &[
            "clippy",
            "--package",
            "domovoi-checks",
            "--all-features",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    step(
        root,
        "checks features test",
        "cargo",
        &[
            "test",
            "--package",
            "domovoi-checks",
            "--all-features",
            "--locked",
        ],
    )?;
    // Network probes are also used by the hub, which does not enable `host`.
    step(
        root,
        "checks net-only clippy",
        "cargo",
        &[
            "clippy",
            "--package",
            "domovoi-checks",
            "--no-default-features",
            "--features",
            "net",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    step(
        root,
        "xtask tests",
        "cargo",
        &["test", "--package", "xtask", "--locked"],
    )?;
    super::arch::run(root, None)?;
    super::openapi::run(root, true)?;
    if !fast {
        if Command::new("cargo")
            .current_dir(root)
            .args(["deny", "--version"])
            .output()
            .is_ok_and(|o| o.status.success())
        {
            step(root, "deny", "cargo", &["deny", "check"])?;
        } else {
            println!("skip deny: install with `cargo install cargo-deny --locked`");
        }
    }
    if web && !fast {
        let web_dir = root.join("web");
        step(&web_dir, "web install", NPM, &["ci"])?;
        step(&web_dir, "web lint", NPM, &["run", "lint"])?;
        step(&web_dir, "web typecheck", NPM, &["run", "typecheck"])?;
        step(&web_dir, "web test", NPM, &["test"])?;
        step(&web_dir, "web build", NPM, &["run", "build"])?;
        // The hub with the built UI embedded, now that web/dist exists.
        step(
            root,
            "hub embed-ui clippy",
            "cargo",
            &[
                "clippy",
                "--package",
                "domovoi-hub",
                "--features",
                "embed-ui",
                "--all-targets",
                "--locked",
                "--",
                "-D",
                "warnings",
            ],
        )?;
        step(
            root,
            "hub embed-ui test",
            "cargo",
            &[
                "test",
                "--package",
                "domovoi-hub",
                "--features",
                "embed-ui",
                "--locked",
            ],
        )?;
    }
    println!("ci: all gates passed");
    Ok(())
}

/// npm's launcher: a batch file on Windows, which `Command` does not find by bare name.
const NPM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

fn has_nextest(root: &Path) -> bool {
    Command::new("cargo")
        .current_dir(root)
        .args(["nextest", "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn step(dir: &Path, name: &str, program: &str, args: &[&str]) -> anyhow::Result<()> {
    println!("==> {name}: {program} {}", args.join(" "));
    let status = Command::new(program).current_dir(dir).args(args).status()?;
    if !status.success() {
        anyhow::bail!("step `{name}` failed");
    }
    Ok(())
}
