//! The restart steps and the identity read, against the scripted manager.

use std::collections::BTreeMap;

use domovoi_core::helper::{HelperRequest, HelperResponse};
use domovoi_core::ports::ProcessOutput;
use domovoi_core::unit::{IdentityHash, UnitId, UnitName, UnitRuntime};
use domovoi_helper::systemd::{Budget, RESTARTS_FILE, UnitEnv, restart_argv, run};
use jiff::SignedDuration;

use crate::world::Fixture;

const TIMEOUT: SignedDuration = SignedDuration::from_secs(30);

#[cfg(test)]
async fn identity(env: &UnitEnv<'_>) -> BTreeMap<String, String> {
    let request = HelperRequest::UnitIdentity {
        name: UnitName::parse("example.service").expect("name"),
    };
    match run(&request, env, TIMEOUT, None, "unit_identity").await {
        HelperResponse::Outputs { outputs } => outputs,
        other => panic!("not served: {other:?}"),
    }
}

#[cfg(test)]
async fn hash(env: &UnitEnv<'_>) -> IdentityHash {
    identity(env).await["identity_hash"].parse().expect("hash")
}

#[cfg(test)]
async fn restart_with(env: &UnitEnv<'_>, hash: IdentityHash) -> HelperResponse {
    let request = HelperRequest::UnitRestart {
        id: UnitId::parse("example.service").expect("id"),
        identity_hash: hash,
    };
    run(&request, env, TIMEOUT, None, "unit_restart").await
}

#[cfg(test)]
fn refused(response: &HelperResponse) -> (&str, Option<&str>) {
    match response {
        HelperResponse::UnitRefused { token, detail } => (token.as_str(), detail.as_deref()),
        other => panic!("expected a refusal: {other:?}"),
    }
}

#[tokio::test]
async fn unit_identity_is_served_when_the_unit_resolves() {
    let f = Fixture::new();
    let out = identity(&f.env(10)).await;
    assert_eq!(out["id"], "example.service");
    assert_eq!(out["unit_file_state"], "enabled");
    assert_eq!(out["identity_hash"].len(), 64);
    assert_eq!(out["active_state"], "active");
    assert!(out["fragment"].contains("example.service"));
    assert!(out["loaded"].contains("--serve"));
    assert!(
        !out["loaded"].contains("PATH"),
        "only declared environment is shown"
    );
}

#[tokio::test]
async fn restart_is_issued_by_canonical_id_and_nothing_else_runs() {
    let f = Fixture::new();
    let env = f.env(10);
    let hash = hash(&env).await;
    let response = restart_with(&env, hash).await;
    let HelperResponse::Outputs { outputs } = response else {
        panic!("not restarted: {response:?}");
    };
    assert_eq!(outputs["restarted"], "true");
    assert_eq!(outputs["verification"], "verified");
    assert_eq!(outputs["invocation_before"], "aa");
    assert_eq!(outputs["invocation_after"], "bb");
    assert_eq!(outputs["stale_start_suspected"], "false");

    let calls = f.world.calls();
    assert_eq!(
        calls.len(),
        1,
        "one process: the restart, no reload, stop or reset"
    );
    assert_eq!(calls[0].program, "/usr/bin/systemctl");
    assert_eq!(calls[0].args, ["restart", "--", "example.service"]);
    assert!(calls[0].env.is_empty() && calls[0].stdin.is_none());
    assert_eq!(
        restart_argv(&UnitId::parse("example.service").unwrap()),
        calls[0].args
    );
}

#[tokio::test]
async fn post_check_does_not_reread_loaded_properties() {
    let f = Fixture::new();
    let env = f.env(10);
    let hash = hash(&env).await;
    let before = f.world.with(|s| s.service_reads);
    restart_with(&env, hash).await;
    // The restart reads the loaded properties twice before it acts (resolution, and the
    // bound copy for the check) and never again afterwards.
    assert_eq!(f.world.with(|s| s.service_reads) - before, 2);
}

#[tokio::test]
async fn unit_config_change_invalidates_approval_and_runs_nothing() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| s.loaded.user = "someone-else".to_owned());
    let response = restart_with(&env, approved).await;
    assert_eq!(refused(&response).0, "target_changed");
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn a_drop_in_edited_after_approval_is_target_changed() {
    let mut f = Fixture::new();
    let approved = hash(&f.env(10)).await;
    f.disk
        .files
        .insert(crate::world::DROP_IN.to_owned(), b"changed".to_vec());
    let response = restart_with(&f.env(10), approved).await;
    assert_eq!(refused(&response).0, "target_changed");
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn daemon_reload_pending_refuses_target_stale_and_never_reloads() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| s.props.need_daemon_reload = true);
    let response = restart_with(&env, approved).await;
    assert_eq!(refused(&response).0, "target_stale");
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn unit_restart_refused_when_unit_not_in_helper_allowlist() {
    let mut f = Fixture::new();
    let approved = hash(&f.env(10)).await;
    f.policy.allow.clear();
    let response = restart_with(&f.env(10), approved).await;
    assert_eq!(refused(&response).0, "target_not_allowlisted");
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn protected_unit_is_refused_by_id_and_by_name() {
    let mut f = Fixture::new();
    let approved = hash(&f.env(10)).await;
    f.policy.protected.insert("alias.service".to_owned());
    f.world
        .with(|s| s.props.names.insert("alias.service".to_owned()));
    let response = restart_with(&f.env(10), approved).await;
    assert_eq!(refused(&response).0, "target_protected");
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn alias_retarget_after_approval_refuses_with_target_changed_on_id() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    // A request that names an alias of the unit is never restarted by that name.
    f.world
        .with(|s| s.props.names.insert("alias.service".to_owned()));
    let request = HelperRequest::UnitRestart {
        id: UnitId::parse("alias.service").expect("id"),
        identity_hash: approved,
    };
    let response = run(&request, &env, TIMEOUT, None, "unit_restart").await;
    assert_eq!(refused(&response), ("target_changed", Some("id")));
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn helper_restart_budget_prevents_start_limit() {
    let f = Fixture::new();
    f.world.with(|s| s.loaded.start_limit_burst = 3);
    let env = f.env(100);
    let approved = hash(&env).await;
    assert!(matches!(
        restart_with(&env, approved).await,
        HelperResponse::Outputs { .. }
    ));
    f.world.with(|s| s.restarted = false);
    assert!(matches!(
        restart_with(&env, approved).await,
        HelperResponse::Outputs { .. }
    ));
    f.world.with(|s| s.restarted = false);
    let third = restart_with(&env, approved).await;
    assert_eq!(refused(&third).0, "restart_budget_exhausted");
    assert_eq!(
        f.world.calls().len(),
        2,
        "the third never reached systemctl"
    );

    // Later, outside the start-limit interval, a restart fits again.
    f.clock.advance(SignedDuration::from_secs(11));
    f.world.with(|s| s.restarted = false);
    assert!(matches!(
        restart_with(&env, approved).await,
        HelperResponse::Outputs { .. }
    ));
}

#[tokio::test]
async fn mutation_limit_survives_helper_restart() {
    let f = Fixture::new();
    let approved = hash(&f.env(1)).await;
    assert!(matches!(
        restart_with(&f.env(1), approved).await,
        HelperResponse::Outputs { .. }
    ));
    // A fresh connection builds a fresh environment and reads the persisted file.
    let file = std::fs::read_to_string(f.dir.path().join(RESTARTS_FILE)).expect("file");
    assert!(file.contains("r\texample.service\t") && file.contains("w\t"));
    assert_eq!(
        Budget::load(f.dir.path()).expect("loads"),
        Budget::load(f.dir.path()).expect("loads again")
    );
    f.world.with(|s| s.restarted = false);
    let second = restart_with(&f.env(1), approved).await;
    assert_eq!(refused(&second).0, "restart_budget_exhausted");
    assert_eq!(f.world.calls().len(), 1);
}

#[tokio::test]
async fn zero_mutations_per_hour_accepts_no_write() {
    let f = Fixture::new();
    let approved = hash(&f.env(0)).await;
    let response = restart_with(&f.env(0), approved).await;
    assert_eq!(refused(&response).0, "restart_budget_exhausted");
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn a_corrupt_budget_file_refuses_read_failed() {
    let f = Fixture::new();
    std::fs::write(f.dir.path().join(RESTARTS_FILE), "garbage\n").expect("write");
    let approved = hash(&f.env(10)).await;
    let response = restart_with(&f.env(10), approved).await;
    assert_eq!(refused(&response).0, "read_failed");
    assert!(f.world.calls().is_empty());
}

#[tokio::test]
async fn post_restart_environ_mismatch_reports_wrong_start_and_does_not_restart_again() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| {
        s.process = Some(domovoi_core::unit::ProcessView {
            environ: vec!["A=2".to_owned()],
            cmdline: vec!["/usr/bin/example-daemon".to_owned(), "--serve".to_owned()],
            uid: 0,
        });
    });
    let response = restart_with(&env, approved).await;
    let (token, detail) = refused(&response);
    assert_eq!(token, "wrong_start");
    assert_eq!(
        detail,
        Some("environment"),
        "field names only, never values"
    );
    assert_eq!(f.world.calls().len(), 1, "no corrective restart or stop");
}

#[tokio::test]
async fn start_limit_refusal_is_detected_by_result_not_message_text() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| {
        s.restart_exit = ProcessOutput {
            exit_code: Some(1),
            stderr: "some unrelated wording".to_owned(),
            ..ProcessOutput::default()
        };
        s.after = UnitRuntime {
            result: "start-limit-hit".to_owned(),
            active_state: "failed".to_owned(),
            ..s.after.clone()
        };
    });
    let response = restart_with(&env, approved).await;
    assert_eq!(refused(&response).0, "start_limit_hit");
    assert_eq!(f.world.calls().len(), 1, "never retried, never reset");
}

#[tokio::test]
async fn a_failed_restart_without_the_limit_is_restart_failed() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| {
        s.restart_exit = ProcessOutput {
            exit_code: Some(1),
            ..ProcessOutput::default()
        };
    });
    assert_eq!(
        refused(&restart_with(&env, approved).await).0,
        "restart_failed"
    );
}

#[tokio::test]
async fn an_unchanged_invocation_is_no_restart_observed() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| s.after = s.before.clone());
    assert_eq!(
        refused(&restart_with(&env, approved).await).0,
        "no_restart_observed"
    );
}

#[tokio::test]
async fn restart_stderr_changed_on_disk_flags_stale_start() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| {
        s.restart_exit = ProcessOutput {
            exit_code: Some(0),
            stderr: "Warning: The unit file changed on disk".to_owned(),
            ..ProcessOutput::default()
        };
    });
    let HelperResponse::Outputs { outputs } = restart_with(&env, approved).await else {
        panic!("restart refused");
    };
    assert_eq!(outputs["stale_start_suspected"], "true");
}

#[tokio::test]
async fn a_process_that_cannot_be_read_is_unverified_never_verified() {
    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world.with(|s| s.process = None);
    let HelperResponse::Outputs { outputs } = restart_with(&env, approved).await else {
        panic!("restart refused");
    };
    assert_eq!(outputs["verification"], "unverified");

    let f = Fixture::new();
    let env = f.env(10);
    let approved = hash(&env).await;
    f.world
        .with(|s| s.loaded.service_type = "oneshot".to_owned());
    // The approved identity covered the old Type; a oneshot is checked at its own hash.
    let approved_oneshot = hash(&env).await;
    assert_ne!(approved, approved_oneshot);
    let HelperResponse::Outputs { outputs } = restart_with(&env, approved_oneshot).await else {
        panic!("restart refused");
    };
    assert_eq!(outputs["verification"], "unverified");
}

#[tokio::test]
async fn allowlist_entry_that_is_an_alias_is_a_config_error() {
    let f = Fixture::new();
    f.world.with(|s| s.props.id = "real.service".to_owned());
    let response = run(
        &HelperRequest::UnitIdentity {
            name: UnitName::parse("example.service").unwrap(),
        },
        &f.env(10),
        TIMEOUT,
        None,
        "unit_identity",
    )
    .await;
    assert!(
        matches!(response, HelperResponse::Refused { .. }),
        "{response:?}"
    );
}
