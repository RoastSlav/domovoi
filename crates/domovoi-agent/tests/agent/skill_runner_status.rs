//! The runner status probe, the probe loop and the status in `hello`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use domovoi_agent::helper_client::{HelperClient, keep_status_with};
use domovoi_agent::link::STATUS_PROBE_INTERVAL;
use domovoi_agent::link::handshake::AgentStatusSource;
use domovoi_agent::skill_runner::{ExpectedDirs, RunnerClient, probe_runner_status};
use domovoi_core::skill_runner::{HealthReply, RUNNER_PROTOCOL, RefusalCode, RunnerReply};
use domovoi_proto::messages::{HelperStatus, SkillRunnerStatus};
use tokio_util::sync::CancellationToken;

use crate::harness::clock;
use crate::runner_rig::{Action, FakeRunner, always, health, refused, run_reply};
use crate::skill_runner::{ok_status, rig, until};

const SKILLS: &str = "/etc/domovoi/skills";
const PACKAGES: &str = "/var/lib/domovoi-packages";

fn expected() -> ExpectedDirs {
    ExpectedDirs {
        skills: PathBuf::from(SKILLS),
        packages: PathBuf::from(PACKAGES),
    }
}

fn good_health() -> RunnerReply {
    health(Path::new(SKILLS), Path::new(PACKAGES))
}

async fn status_of(runner: &FakeRunner) -> SkillRunnerStatus {
    let clock = clock();
    probe_runner_status(&runner.client(clock.clone()), &expected(), &*clock).await
}

fn error(reason: &str) -> SkillRunnerStatus {
    SkillRunnerStatus::Error {
        reason: reason.to_owned(),
    }
}

#[tokio::test]
async fn status_absent_when_socket_missing() {
    let dir = tempfile::tempdir().unwrap();
    let clock = clock();
    let client = RunnerClient::new(dir.path().join("none.sock"), clock.clone());
    assert_eq!(
        probe_runner_status(&client, &expected(), &*clock).await,
        SkillRunnerStatus::Absent
    );
}

#[tokio::test]
async fn status_error_on_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    if nix::unistd::geteuid().is_root() {
        eprintln!("skipped: root can open a socket whatever its mode");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let runner = FakeRunner::start(dir.path(), always(Action::Close));
    std::fs::set_permissions(&runner.socket, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(status_of(&runner).await, error("runner_socket_permission"));
}

#[tokio::test]
async fn status_ok_requires_equal_roots() {
    let dir = tempfile::tempdir().unwrap();
    let runner = FakeRunner::start(dir.path(), always(Action::Reply(good_health())));
    assert_eq!(status_of(&runner).await, ok_status());

    let other = tempfile::tempdir().unwrap();
    for (skills, packages) in [
        ("/srv/skills", PACKAGES),
        (SKILLS, "/srv/packages"),
        ("/etc/domovoi/skills/", PACKAGES),
    ] {
        let reply = health(Path::new(skills), Path::new(packages));
        let runner = FakeRunner::start(other.path(), always(Action::Reply(reply)));
        assert_eq!(status_of(&runner).await, error("runner_roots_differ"));
        drop(runner);
        std::fs::remove_file(other.path().join("runner.sock")).ok();
    }
}

#[tokio::test]
async fn status_unsupported_on_start_check_refusals() {
    for code in [
        RefusalCode::RunnerGroupsExtra,
        RefusalCode::RunnerPrivileges,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::start(dir.path(), always(Action::Reply(refused(code, "no"))));
        assert_eq!(
            status_of(&runner).await,
            SkillRunnerStatus::Unsupported {
                reason: code.as_str().to_owned()
            }
        );
    }
}

#[tokio::test]
async fn status_error_on_protocol_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let runner = FakeRunner::start(
        dir.path(),
        always(Action::Reply(refused(RefusalCode::ProtocolMismatch, "old"))),
    );
    assert_eq!(status_of(&runner).await, error("runner_protocol_mismatch"));

    let dir = tempfile::tempdir().unwrap();
    let newer = RunnerReply::Health(HealthReply {
        protocol: RUNNER_PROTOCOL + 1,
        version: "9".to_owned(),
        skills_root: SKILLS.to_owned(),
        packages_root: PACKAGES.to_owned(),
    });
    let runner = FakeRunner::start(dir.path(), always(Action::Reply(newer)));
    assert_eq!(status_of(&runner).await, error("runner_protocol_mismatch"));
}

#[tokio::test]
async fn status_error_when_the_runner_does_not_answer_properly() {
    let dir = tempfile::tempdir().unwrap();
    let closing = FakeRunner::start(dir.path(), always(Action::Close));
    assert_eq!(status_of(&closing).await, error("runner_unreachable"));

    // A reply of the wrong kind is malformed, not a status.
    let dir = tempfile::tempdir().unwrap();
    let wrong = FakeRunner::start(
        dir.path(),
        always(Action::Reply(RunnerReply::Run(run_reply("x")))),
    );
    assert_eq!(status_of(&wrong).await, error("runner_unreachable"));

    // A socket file nobody listens on refuses the connection.
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("dead.sock");
    drop(tokio::net::UnixListener::bind(&socket).unwrap());
    let clock = clock();
    let client = RunnerClient::new(socket, clock.clone());
    assert_eq!(
        probe_runner_status(&client, &expected(), &*clock).await,
        error("runner_unreachable")
    );
}

#[tokio::test]
async fn status_probe_runs_at_start_every_interval_and_after_error() {
    let dir = tempfile::tempdir().unwrap();
    let current = Arc::new(Mutex::new(good_health()));
    let answer = current.clone();
    let script: crate::runner_rig::Script = Arc::new(move |request| match request {
        domovoi_core::skill_runner::RunnerRequest::Run(_) => Action::Close,
        _ => Action::Reply(answer.lock().unwrap().clone()),
    });
    let runner = FakeRunner::start(dir.path(), script);
    let clock = clock();
    let route = runner.route(clock.clone(), Path::new(SKILLS), Path::new(PACKAGES));

    // At start: one probe before anything else.
    assert_eq!(route.refresh().await, ok_status());
    let status = AgentStatusSource::default();
    let stop = CancellationToken::new();
    let task = tokio::spawn(keep_status_with(
        HelperClient::new(dir.path().join("no-helper.sock")),
        Some(route.clone()),
        clock.clone(),
        status.clone(),
        stop.clone(),
    ));
    crate::harness::settle().await;
    assert_eq!(
        status.current().unwrap().skill_runner_status,
        SkillRunnerStatus::Absent,
        "the loop waits an interval before its first probe"
    );

    // Every interval.
    clock.advance(STATUS_PROBE_INTERVAL);
    until(|| status.current().unwrap().skill_runner_status == ok_status()).await;

    // After a transport error: no clock advance, and the runner now refuses to start.
    *current.lock().unwrap() = refused(RefusalCode::RunnerPrivileges, "capabilities");
    let (skill, args) = skill_and_args(dir.path());
    assert!(route.run(&skill, &args).await.is_err());
    let unsupported = SkillRunnerStatus::Unsupported {
        reason: "runner_privileges".to_owned(),
    };
    until(|| status.current().unwrap().skill_runner_status == unsupported).await;
    assert_eq!(
        status.current().unwrap().helper_status,
        HelperStatus::Absent,
        "every other field is kept"
    );
    stop.cancel();
    task.await.unwrap();
}

/// A loaded skill and its arguments, for provoking a run through the route.
fn skill_and_args(dir: &Path) -> (domovoi_runner::LoadedSkill, domovoi_core::ValidatedArgs) {
    let skills = dir.join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    crate::common::install(&skills, "reader", "read");
    let (catalog, _) = domovoi_runner::SkillCatalog::load(&skills);
    let skill = catalog
        .get(&domovoi_core::SkillName::parse("reader").unwrap())
        .unwrap()
        .clone();
    let args = skill
        .manifest()
        .validate_args(&std::collections::BTreeMap::from([(
            "service".to_owned(),
            "plex".to_owned(),
        )]))
        .unwrap();
    (skill, args)
}

#[tokio::test]
async fn route_rig_status_is_shared_with_the_probe() {
    // The route built by the rig starts `Ok`; a refresh against a failing runner replaces it.
    let rig = rig(always(Action::Close));
    assert!(rig.route.state().is_ok());
    assert_eq!(rig.route.refresh().await, error("runner_unreachable"));
    assert!(!rig.route.state().is_ok());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn hello_carries_the_real_runner_status() {
    use domovoi_agent::skill_runner::{RunnerRoute, SkillRoute};
    use domovoi_agent::{AgentDeps, AgentRuntime};
    use domovoi_testkit::{FakeProcessExecutor, ListRedactor};

    use crate::common::NoStats;
    use crate::harness::{empty_fs, identity};
    use crate::hub::TestHub;
    use crate::link::config;

    let mut hub = TestHub::start().await;
    let dir = tempfile::tempdir().unwrap();
    let identity = identity(&hub);
    identity.save(dir.path()).unwrap();
    domovoi_agent::Journal::init(
        dir.path(),
        jiff::Timestamp::constant(1_700_000_000, 0),
        "018f0000-0000-7000-8000-000000000001",
        1,
    )
    .unwrap();
    let config = config(&dir, &hub);
    let sock_dir = tempfile::tempdir().unwrap();
    let runner = FakeRunner::start(
        sock_dir.path(),
        always(Action::Reply(health(
            &config.agent.skills_dir,
            &config.agent.packages_dir,
        ))),
    );
    let clock = clock();
    let route: Arc<RunnerRoute> = runner.route(
        clock.clone(),
        &config.agent.skills_dir,
        &config.agent.packages_dir,
    );
    let deps = AgentDeps {
        clock,
        executor: Arc::new(FakeProcessExecutor::new()),
        hostfs: empty_fs(&dir),
        toolfs: empty_fs(&dir),
        fs_stats: Arc::new(NoStats),
        container_source: None,
        docker: None,
        redactor: Arc::new(ListRedactor::new(Vec::<String>::new())),
        logs: None,
        boundary_in_effect: true,
        skills: SkillRoute::Runner(route),
    };
    let runtime = AgentRuntime::new(config, deps).await.unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(runtime.run(shutdown.clone()));
    let mut conn = hub.accept().await;
    let hello = conn.welcome(&identity, 10).await;
    assert_eq!(hello.skill_runner_status, ok_status());
    shutdown.cancel();
    task.await.unwrap().unwrap();
}
