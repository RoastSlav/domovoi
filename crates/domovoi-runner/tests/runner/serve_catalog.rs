//! `load_one`, the cancellable run and the executor's cancel input. Linux only.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use domovoi_core::SkillName;
use domovoi_core::ports::ProcessExecutor;
use domovoi_runner::{RunCaps, RunEnd, RunError, SandboxExecutor, SkillCatalog};
use domovoi_testkit::ListRedactor;

use crate::helpers::{any_bin, bin, executor, spec};
use crate::serve_common::{Env, SkillSpec, write_skill};

fn name(n: &str) -> SkillName {
    SkillName::parse(n).unwrap()
}

fn plain_executor() -> SandboxExecutor {
    SandboxExecutor::new(Arc::new(ListRedactor::new(["unused-secret"])))
}

#[tokio::test]
async fn load_one_loads_exactly_one_skill() {
    let env = Env::new();
    write_skill(env.skills.path(), "alpha", &SkillSpec::new("echo a")).await;
    write_skill(env.skills.path(), "beta", &SkillSpec::new("echo b")).await;
    std::fs::create_dir(env.skills.path().join("broken")).unwrap();

    let (catalog, skill) = SkillCatalog::load_one(env.skills.path(), &name("alpha")).unwrap();
    assert_eq!(skill.manifest().name.as_str(), "alpha");
    assert_eq!(catalog.summaries().len(), 1);
    assert!(catalog.get(&name("beta")).is_none());

    // A broken folder is its own issue, and a missing one another.
    let issue = SkillCatalog::load_one(env.skills.path(), &name("broken")).unwrap_err();
    assert!(issue.message.contains("skill.toml"), "{issue}");
    let issue = SkillCatalog::load_one(env.skills.path(), &name("nothing")).unwrap_err();
    assert!(issue.message.contains("no skill folder"), "{issue}");
    let missing_root = env.skills.path().join("absent");
    let issue = SkillCatalog::load_one(&missing_root, &name("alpha")).unwrap_err();
    assert!(issue.message.contains("no skill folder"), "{issue}");

    // A folder that is a symlink to another folder is refused, even inside the root.
    std::os::unix::fs::symlink(
        env.skills.path().join("beta"),
        env.skills.path().join("alias"),
    )
    .unwrap();
    let issue = SkillCatalog::load_one(env.skills.path(), &name("alias")).unwrap_err();
    assert!(issue.message.contains("outside"), "{issue}");
}

#[tokio::test]
async fn runner_refuses_entry_changed_since_load() {
    let env = Env::new();
    let dir = write_skill(env.skills.path(), "mutable", &SkillSpec::new("echo before")).await;
    let (catalog, skill) = SkillCatalog::load_one(env.skills.path(), &name("mutable")).unwrap();
    std::fs::write(dir.join("run"), "#!/bin/sh\necho after\n").unwrap();
    let args = skill.manifest().validate_args(&BTreeMap::new()).unwrap();
    let err = catalog
        .run_skill_cancellable(
            &skill,
            &args,
            &plain_executor(),
            RunCaps::default(),
            std::future::pending(),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, RunError::EntryChanged(_)), "{err:?}");
}

#[tokio::test]
async fn run_cancellable_without_cancel_behaves_like_run() {
    let Some(printf) = any_bin(&["/usr/bin/printf"]) else {
        return;
    };
    let plain = executor()
        .run(spec(&printf, &["%s", "same"]))
        .await
        .unwrap();
    let end = plain_executor()
        .run_cancellable(spec(&printf, &["%s", "same"]), std::future::pending())
        .await
        .unwrap();
    let RunEnd::Finished(out) = end else {
        panic!("cancelled without a cancel");
    };
    assert_eq!(out.stdout, plain.stdout);
    assert_eq!(out.exit_code, plain.exit_code);
}

#[tokio::test]
async fn run_cancellable_cancels_and_collects_the_child() {
    let Some(sleep) = bin("/bin/sleep") else {
        return;
    };
    let started = Instant::now();
    let end = plain_executor()
        .run_cancellable(
            spec(&sleep, &["29.7641"]),
            tokio::time::sleep(Duration::from_millis(200)),
        )
        .await
        .unwrap();
    assert_eq!(end, RunEnd::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(5));
}
