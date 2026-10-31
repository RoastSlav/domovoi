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
    step(
        root,
        "xtask tests",
        "cargo",
        &["test", "--package", "xtask", "--locked"],
    )?;
    super::arch::run(root, None)?;
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
        step(&web_dir, "web install", "npm", &["ci"])?;
        step(&web_dir, "web lint", "npm", &["run", "lint"])?;
        step(&web_dir, "web typecheck", "npm", &["run", "typecheck"])?;
        step(&web_dir, "web test", "npm", &["test"])?;
        step(&web_dir, "web build", "npm", &["run", "build"])?;
    }
    println!("ci: all gates passed");
    Ok(())
}

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
