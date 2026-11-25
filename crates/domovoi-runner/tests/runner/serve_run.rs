//! Validation and execution of `Run` requests through the serve path. Linux only.

use std::time::{Duration, Instant};

use domovoi_core::action::ImplHash;
use domovoi_core::identity::SKILL_ACCOUNT;
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::skill_runner::{RefusalCode, RunReply, RunnerReply, RunnerRequest, SkillSource};
use domovoi_core::{ScriptDigest, SkillName};

use crate::serve_common::{Env, SkillSpec, package_request, roundtrip, run_request, write_skill};

fn run_reply(reply: RunnerReply) -> RunReply {
    match reply {
        RunnerReply::Run(run) => run,
        other => panic!("expected a run reply, got {other:?}"),
    }
}

fn refusal(reply: RunnerReply) -> (RefusalCode, String) {
    match reply {
        RunnerReply::Refused(refusal) => (refusal.code, refusal.detail),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn runner_puts_skill_in_its_own_process_group() {
    let env = Env::new();
    write_skill(
        env.skills.path(),
        "pgid_probe",
        &SkillSpec::new("echo \"$$ $(cut -d' ' -f5 /proc/$$/stat)\""),
    )
    .await;
    let request = run_request(env.skills.path(), "pgid_probe", &[]);
    let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(run.exit_code, Some(0), "{run:?}");
    let (pid, pgid) = run.stdout.trim().split_once(' ').unwrap();
    let own = nix::unistd::getpgrp().as_raw().to_string();
    assert_ne!(pgid, own, "the skill shares the runner's process group");
    assert_eq!(pgid, pid, "the skill leads its own process group");
    assert_eq!(run.argv_hash.len(), 64);
}

#[tokio::test]
async fn runner_enforces_request_timeout_before_runtime_max() {
    let env = Env::new();
    write_skill(
        env.skills.path(),
        "slow",
        &SkillSpec {
            timeout: "60s",
            ..SkillSpec::new("/bin/sleep 29.7611")
        },
    )
    .await;
    let mut request = run_request(env.skills.path(), "slow", &[]);
    request.timeout_secs = 1;
    let started = Instant::now();
    let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert!(run.timed_out, "{run:?}");
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[tokio::test]
async fn request_caps_lower_but_never_raise_manifest_limits() {
    let env = Env::new();
    // The manifest allows 2 s; a request asking for 100 s does not get them.
    write_skill(
        env.skills.path(),
        "short",
        &SkillSpec {
            timeout: "2s",
            ..SkillSpec::new("/bin/sleep 29.7612")
        },
    )
    .await;
    let mut request = run_request(env.skills.path(), "short", &[]);
    request.timeout_secs = 100;
    let started = Instant::now();
    let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert!(run.timed_out, "{run:?}");
    assert!(started.elapsed() < Duration::from_secs(10));

    // The manifest allows 4096 bytes; a request for a million gets 4096, one for ten gets ten.
    write_skill(
        env.skills.path(),
        "loud",
        &SkillSpec::new("head -c 10000 /dev/zero | tr '\\000' a"),
    )
    .await;
    let mut request = run_request(env.skills.path(), "loud", &[]);
    request.max_output_bytes = 1_000_000;
    let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert!(run.truncated);
    assert_eq!(run.stdout.len(), 4096);
    let mut request = run_request(env.skills.path(), "loud", &[]);
    request.max_output_bytes = 10;
    let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert!(run.truncated);
    assert_eq!(run.stdout.len(), 10);
}

#[tokio::test]
async fn runner_refuses_run_as_other_user() {
    let env = Env::new();
    let marker = env.marker("ran");
    write_skill(
        env.skills.path(),
        "as_other",
        &SkillSpec {
            run_as: Some("nobody"),
            ..SkillSpec::new(&format!("echo ran > {}", marker.display()))
        },
    )
    .await;
    let request = run_request(env.skills.path(), "as_other", &[]);
    let (code, _) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::RunAsUnsupported);
    assert!(!marker.exists(), "the skill ran");
}

#[tokio::test]
async fn runner_accepts_run_as_absent_or_skill_account() {
    let env = Env::new();
    write_skill(env.skills.path(), "plain", &SkillSpec::new("echo ok")).await;
    write_skill(
        env.skills.path(),
        "named",
        &SkillSpec {
            run_as: Some(SKILL_ACCOUNT),
            ..SkillSpec::new("echo ok")
        },
    )
    .await;
    for name in ["plain", "named"] {
        let request = run_request(env.skills.path(), name, &[]);
        let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
        assert_eq!(run.exit_code, Some(0), "{name}: {run:?}");
        assert_eq!(run.stdout, "ok\n");
    }
}

#[tokio::test]
async fn runner_never_sets_spec_run_as() {
    // The account does not exist on a development host; a spec that asked for a switch would
    // fail to spawn. The skill instead runs as whoever runs the runner.
    let env = Env::new();
    write_skill(
        env.skills.path(),
        "whoami_probe",
        &SkillSpec {
            run_as: Some(SKILL_ACCOUNT),
            ..SkillSpec::new("id -u")
        },
    )
    .await;
    let request = run_request(env.skills.path(), "whoami_probe", &[]);
    let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(run.exit_code, Some(0), "{run:?}");
    assert_eq!(
        run.stdout.trim(),
        nix::unistd::geteuid().as_raw().to_string()
    );
}

#[tokio::test]
async fn package_source_refuses_any_run_as() {
    let env = Env::new();
    let root = env.package_root("mail-tools", "1.0.0");
    let marker = env.marker("pkg_ran");
    write_skill(
        &root,
        "with_run_as",
        &SkillSpec {
            run_as: Some(SKILL_ACCOUNT),
            ..SkillSpec::new(&format!("echo ran > {}", marker.display()))
        },
    )
    .await;
    write_skill(&root, "without", &SkillSpec::new("echo packaged")).await;

    let request = package_request(&env, "mail-tools", "1.0.0", "with_run_as");
    let (code, _) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::RunAsUnsupported);
    assert!(!marker.exists());

    let request = package_request(&env, "mail-tools", "1.0.0", "without");
    let run = run_reply(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(run.stdout, "packaged\n");
}

#[tokio::test]
async fn runner_refuses_impl_hash_mismatch_without_executing() {
    let env = Env::new();
    let marker = env.marker("hash_ran");
    write_skill(
        env.skills.path(),
        "hashed",
        &SkillSpec::new(&format!("echo ran > {}", marker.display())),
    )
    .await;
    let mut request = run_request(env.skills.path(), "hashed", &[]);
    let other = SkillName::parse("hashed").unwrap();
    let (_, skill) = domovoi_runner::SkillCatalog::load_one(env.skills.path(), &other).unwrap();
    request.impl_hash = ImplHash::from_parts(
        skill.manifest_hash(),
        ScriptDigest::of_bytes(b"another entry"),
    );
    let (code, _) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::ImplHashMismatch);
    assert!(!marker.exists(), "the skill ran");
}

#[tokio::test]
async fn runner_refuses_invalid_args() {
    let env = Env::new();
    let marker = env.marker("args_ran");
    write_skill(
        env.skills.path(),
        "with_args",
        &SkillSpec::new(&format!("echo ran > {}", marker.display())),
    )
    .await;
    // Not printable ASCII, so it fails the pattern; the refusal must not repeat it.
    let value = "s3cr3t-\u{e9}-value";
    let request = run_request(env.skills.path(), "with_args", &[("first", value)]);
    let (code, detail) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::ArgsInvalid);
    assert!(detail.contains("first"), "{detail}");
    assert!(!detail.contains("s3cr3t"), "{detail}");

    let request = run_request(env.skills.path(), "with_args", &[("undeclared", "x")]);
    let (code, _) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::ArgsInvalid);
    assert!(!marker.exists());
}

#[tokio::test]
async fn runner_refuses_folder_symlinked_outside_root() {
    let env = Env::new();
    let elsewhere = tempfile::tempdir().unwrap();
    write_skill(elsewhere.path(), "evil", &SkillSpec::new("echo escaped")).await;
    std::os::unix::fs::symlink(
        elsewhere.path().join("evil"),
        env.skills.path().join("evil"),
    )
    .unwrap();
    let request = run_request(elsewhere.path(), "evil", &[]);
    let (code, _) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::SkillNotFound);
}

#[tokio::test]
async fn skill_runner_refuses_path_outside_skill_roots() {
    // Names cannot carry a separator, so a request cannot even name a path.
    for name in ["../etc", "a/b", "..", "/etc/passwd"] {
        assert!(SkillName::parse(name).is_err(), "{name} parsed");
    }
    let env = Env::new();
    // A package folder that is a symlink out of the packages root is not a root at all.
    let elsewhere = tempfile::tempdir().unwrap();
    write_skill(
        &elsewhere.path().join("1.0.0").join("skills"),
        "evil",
        &SkillSpec::new("echo escaped"),
    )
    .await;
    std::os::unix::fs::symlink(elsewhere.path(), env.packages.path().join("mail-tools")).unwrap();
    let mut request = run_request(&elsewhere.path().join("1.0.0").join("skills"), "evil", &[]);
    request.source = SkillSource::Package {
        name: PackageName::parse("mail-tools").unwrap(),
        version: PackageVersion::parse("1.0.0").unwrap(),
    };
    let (code, _) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::SkillNotFound);

    // A name that does not exist is the same answer.
    write_skill(env.skills.path(), "present", &SkillSpec::new("echo hi")).await;
    let mut request = run_request(env.skills.path(), "present", &[]);
    request.skill = SkillName::parse("absent").unwrap();
    let (code, _) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::SkillNotFound);
}

#[tokio::test]
async fn runner_refuses_invalid_skill_folders_with_the_catalogue_message() {
    let env = Env::new();
    let dir = write_skill(env.skills.path(), "loose", &SkillSpec::new("echo hi")).await;
    let request = run_request(env.skills.path(), "loose", &[]);
    crate::helpers::chmod(&dir.join("run"), 0o757);
    let (code, detail) = refusal(roundtrip(&env, RunnerRequest::Run(request)).await);
    assert_eq!(code, RefusalCode::SkillInvalid);
    assert!(detail.contains("writable"), "{detail}");
}
