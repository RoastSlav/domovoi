//! Which sources the agent builds from the boundary, and how the helper answers show up in
//! `hello.docker`, the kernel-log tool and the listening-ports tool.

#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;

use domovoi_agent::AgentDeps;
use domovoi_agent::helper_client::HelperReads;
use domovoi_agent::production::{deps, deps_via_helper, deps_with_boundary};
use domovoi_agent::requests::run_tool;
use domovoi_checks::commands::{Commands, ToolHost};
use domovoi_config::{AgentConfig, MapEnv};
use domovoi_core::ReadToolCall;
use domovoi_core::helper::read::{KernelLogOutput, ReadToken};
use domovoi_core::helper::{HelperGroup, HelperRequest, HelperResponse, UnavailableReason};
use domovoi_core::identity::RefusalToken;
use domovoi_core::ports::{PrivilegedError, ProcessOutput};
use domovoi_testkit::{
    FakePrivilegedReads, FakeProcessExecutor, FixtureHostFs, ListRedactor, PrivilegedCall,
    PrivilegedScript,
};

use crate::common::Fixture;
use crate::harness::clock;
use crate::helper_reads::{answers, listener};
use crate::helper_stub::{Stub, serve};

fn config(dir: &tempfile::TempDir, docker: &str) -> AgentConfig {
    let text = format!(
        "[agent]\nhost_name = \"nas\"\nhub_url = \"wss://hub.example:7421\"\nstate_dir = '{}'\nskills_dir = '{}'\nhost_root = '/'\n[docker]\n{docker}\n",
        dir.path().display(),
        dir.path().join("skills").display(),
    );
    domovoi_config::parse::<AgentConfig>(&text, &MapEnv::new())
        .unwrap()
        .0
}

async fn build(config: &AgentConfig, helper: &HelperReads) -> AgentDeps {
    let executor = Arc::new(FakeProcessExecutor::new());
    deps_via_helper(
        config,
        executor,
        clock(),
        Arc::new(ListRedactor::new(Vec::<String>::new())),
        None,
        helper,
    )
    .await
    .unwrap()
}

fn helper(socket: &Path) -> HelperReads {
    HelperReads::new(socket, clock())
}

fn engines(stub_socket: &Path) -> Stub {
    serve(stub_socket, answers())
}

#[tokio::test]
async fn boundary_in_effect_never_builds_a_direct_docker_source_for_a_unix_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = engines(&socket);
    let configured = dir.path().join("docker.sock");
    // The owner's unix endpoint must not be opened by the agent: nothing listens there, and
    // the source still answers, from the helper.
    let config = config(
        &dir,
        &format!(
            "enabled = true\nendpoint = \"unix://{}\"",
            configured.display()
        ),
    );
    let deps = build(&config, &helper(&socket)).await;

    let report = deps.docker.as_ref().unwrap().report();
    assert_eq!(
        report.endpoint, "/var/run/docker.sock",
        "the helper's engine, not the owner's socket"
    );
    assert!(deps.boundary_in_effect && deps.privileged.is_some());
    let source = deps.container_source.as_ref().unwrap();
    assert_eq!(source.list().await.unwrap().len(), 1);
    assert_eq!(
        stub.requests(),
        vec![HelperRequest::DockerProbe, HelperRequest::DockerList]
    );
}

#[tokio::test]
async fn boundary_in_effect_keeps_an_owner_set_tcp_endpoint_direct() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = engines(&socket);
    let config = config(&dir, "enabled = true\nendpoint = \"tcp://127.0.0.1:9\"");
    let deps = build(&config, &helper(&socket)).await;

    let report = deps.docker.as_ref().unwrap().report();
    assert!(report.endpoint.starts_with("tcp://"), "{report:?}");
    assert!(deps.container_source.is_some());
    assert_eq!(stub.count(), 0, "Docker was not asked of the helper");
    assert!(
        deps.privileged.is_some(),
        "SMART still goes through the helper"
    );
}

#[tokio::test]
async fn boundary_not_in_effect_keeps_the_direct_sources() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let stub = engines(&socket);
    let configured = dir.path().join("docker.sock");
    let _engine = std::os::unix::net::UnixListener::bind(&configured).unwrap();
    let config = config(
        &dir,
        &format!(
            "enabled = true\nendpoint = \"unix://{}\"",
            configured.display()
        ),
    );
    let executor = Arc::new(FakeProcessExecutor::new());
    let redactor = Arc::new(ListRedactor::new(Vec::<String>::new()));

    for deps in [
        deps(&config, executor.clone(), clock(), redactor.clone(), None).unwrap(),
        deps_with_boundary(
            &config,
            executor.clone(),
            clock(),
            redactor.clone(),
            None,
            false,
        )
        .await
        .unwrap(),
    ] {
        let report = deps.docker.as_ref().unwrap().report();
        assert_eq!(report.endpoint, format!("unix://{}", configured.display()));
        assert!(deps.container_source.is_some());
        assert!(deps.privileged.is_none() && !deps.boundary_in_effect);
    }
    assert_eq!(stub.count(), 0);
}

#[tokio::test]
async fn docker_problem_text_follows_the_helper_answer() {
    let dir = tempfile::tempdir().unwrap();
    let failed = |token| HelperResponse::ReadFailed {
        group: HelperGroup::DockerRead,
        token,
        detail: None,
    };
    let cases: Vec<(&str, Option<HelperResponse>, &str)> = vec![
        (
            "absent",
            None,
            "the helper is not installed; Docker is read through it",
        ),
        (
            "refused",
            Some(HelperResponse::Refused {
                token: RefusalToken::PeerNotAgent,
            }),
            "the helper is not usable (peer_not_agent)",
        ),
        (
            "start",
            Some(HelperResponse::StartRefused {
                token: RefusalToken::HelperStatePermissions,
                faults: Vec::new(),
            }),
            "the helper is not usable (helper_state_permissions)",
        ),
        (
            "off",
            Some(HelperResponse::Unavailable {
                group: HelperGroup::DockerRead,
                reason: UnavailableReason::GroupDisabled,
            }),
            "docker.read is switched off in helper.toml",
        ),
        (
            "blocked",
            Some(HelperResponse::Unavailable {
                group: HelperGroup::DockerRead,
                reason: UnavailableReason::PlatformBlocked,
            }),
            "this platform's isolation checks failed; run domovoi agent selftest",
        ),
        (
            "noengine",
            Some(failed(ReadToken::EngineAbsent)),
            "no Docker or Podman socket was found",
        ),
        (
            "none",
            Some(HelperResponse::DockerEngine { engine: None }),
            "no Docker or Podman socket was found",
        ),
        (
            "denied",
            Some(failed(ReadToken::EngineDenied)),
            "the helper cannot use the container engine socket",
        ),
        (
            "error",
            Some(failed(ReadToken::EngineError)),
            "the helper cannot use the container engine socket",
        ),
    ];
    for (name, response, expected) in cases {
        let socket = dir.path().join(format!("{name}.sock"));
        let _stub = response.map(|response| serve(&socket, Arc::new(move |_| response.clone())));
        let config = config(&dir, "enabled = \"auto\"");
        let deps = build(&config, &helper(&socket)).await;
        let report = deps.docker.as_ref().unwrap().report();
        assert_eq!(report.problem.as_deref(), Some(expected), "{name}");
        assert!(
            !report.available && deps.container_source.is_none(),
            "{name}"
        );
    }

    // The owner's word that Docker is there keeps the source, and its errors say why.
    let socket = dir.path().join("missing.sock");
    let config = config(&dir, "enabled = true");
    let deps = build(&config, &helper(&socket)).await;
    let err = deps.container_source.unwrap().list().await.unwrap_err();
    assert_eq!(
        err.0,
        "the helper is not installed; Docker is read through it"
    );
}

#[tokio::test]
async fn hub_docker_switch_still_gates_the_helper_source() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("h.sock");
    let _stub = engines(&socket);
    let config = config(&dir, "enabled = \"auto\"");
    let deps = build(&config, &helper(&socket)).await;
    let control = deps.docker.as_ref().unwrap();
    let source = deps.container_source.as_ref().unwrap();

    assert!(control.is_on());
    assert_eq!(source.list().await.unwrap().len(), 1);
    control.choose(Some(false));
    let err = source.list().await.unwrap_err();
    assert!(err.0.contains("switched off"), "{err:?}");
    control.choose(Some(true));
    assert_eq!(source.list().await.unwrap().len(), 1);
}

const DMESG_FAILED: &str = "Failed to open journal: not readable";

async fn journal_ctx(
    fixture: &Fixture,
    port: Arc<FakePrivilegedReads>,
) -> domovoi_agent::requests::RequestCtx {
    let tools = dir_with_tools(fixture);
    let mut ctx = fixture.ctx(Vec::new(), None, Vec::new());
    ctx.commands = Arc::new(Commands::resolve(&FixtureHostFs::new(tools), ToolHost::Host).await);
    ctx.privileged = Some(port);
    ctx
}

fn dir_with_tools(fixture: &Fixture) -> std::path::PathBuf {
    let root = fixture.dir.path().join("tools");
    std::fs::create_dir_all(root.join("usr/bin")).unwrap();
    std::fs::write(root.join("usr/bin/journalctl"), "").unwrap();
    root
}

const JOURNAL: &str = "/usr/bin/journalctl";
const KERNEL_ARGS: [&str; 6] = [
    "--no-pager",
    "--quiet",
    "--output=short-iso",
    "--since=-60min",
    "--lines=100",
    "--dmesg",
];

fn kernel_call() -> ReadToolCall {
    ReadToolCall::Journal {
        unit: None,
        since_minutes: 60,
        lines: 100,
        kernel: true,
    }
}

#[tokio::test]
async fn kernel_tool_falls_back_to_the_helper_when_journalctl_cannot_open_the_journal() {
    let fixture = Fixture::new();
    let port = Arc::new(FakePrivilegedReads::new(PrivilegedScript {
        kernel: Ok(KernelLogOutput {
            text: "kernel: oom\n".to_owned(),
            truncated: false,
        }),
        ..PrivilegedScript::default()
    }));
    let ctx = journal_ctx(&fixture, port.clone()).await;
    fixture.exec.expect(
        JOURNAL,
        &KERNEL_ARGS,
        ProcessOutput {
            exit_code: Some(1),
            stderr: DMESG_FAILED.to_owned(),
            ..ProcessOutput::default()
        },
    );
    let (text, truncated) = run_tool(&ctx, &kernel_call()).await.unwrap();
    assert_eq!((text.as_str(), truncated), ("kernel: oom\n", false));
    assert_eq!(port.calls(), vec![PrivilegedCall::KernelLog(60, 100)]);
    assert_eq!(fixture.exec.calls().len(), 1, "journalctl first, once");

    // A helper that fails leaves the direct answer.
    port.set(PrivilegedScript {
        kernel: Err(PrivilegedError::HelperMissing),
        ..PrivilegedScript::default()
    });
    let (text, _) = run_tool(&ctx, &kernel_call()).await.unwrap();
    assert!(text.contains(DMESG_FAILED), "{text}");
}

#[tokio::test]
async fn kernel_tool_does_not_use_the_helper_when_journalctl_succeeds() {
    let fixture = Fixture::new();
    let port = Arc::new(FakePrivilegedReads::new(PrivilegedScript::default()));
    let ctx = journal_ctx(&fixture, port.clone()).await;
    fixture.exec.expect(
        JOURNAL,
        &KERNEL_ARGS,
        ProcessOutput {
            exit_code: Some(0),
            stdout: "direct line\n".to_owned(),
            ..ProcessOutput::default()
        },
    );
    let (text, _) = run_tool(&ctx, &kernel_call()).await.unwrap();
    assert_eq!(text, "direct line\n");
    assert!(port.calls().is_empty());
}

fn ports_fixture(fixture: &Fixture) {
    let net = fixture.dir.path().join("host/proc/net");
    std::fs::create_dir_all(&net).unwrap();
    std::fs::write(
        net.join("tcp"),
        "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 10000 1 0000000000000000 100 0 0 10 0\n",
    )
    .unwrap();
}

#[tokio::test]
async fn listening_ports_tool_adds_owning_processes_only_through_the_helper() {
    let fixture = Fixture::new();
    ports_fixture(&fixture);
    let direct = fixture.ctx(Vec::new(), None, Vec::new());
    let (text, _) = run_tool(&direct, &ReadToolCall::ListeningPorts)
        .await
        .unwrap();
    assert_eq!(text, "0.0.0.0:22/tcp\n");

    let port = Arc::new(FakePrivilegedReads::new(PrivilegedScript {
        listeners: Ok(vec![listener()]),
        ..PrivilegedScript::default()
    }));
    let mut ctx = fixture.ctx(Vec::new(), None, Vec::new());
    ctx.privileged = Some(port.clone());
    let (text, _) = run_tool(&ctx, &ReadToolCall::ListeningPorts).await.unwrap();
    assert_eq!(text, "0.0.0.0:22/tcp sshd (pid 812)\n");
    assert_eq!(port.calls(), vec![PrivilegedCall::NetListeners]);
}

#[tokio::test]
async fn listening_ports_tool_keeps_the_direct_result_when_the_helper_fails() {
    let fixture = Fixture::new();
    ports_fixture(&fixture);
    let mut ctx = fixture.ctx(Vec::new(), None, Vec::new());
    ctx.privileged = Some(Arc::new(FakePrivilegedReads::new(PrivilegedScript {
        listeners: Err(PrivilegedError::Failed(ReadToken::CommandFailed)),
        ..PrivilegedScript::default()
    })));
    let (text, _) = run_tool(&ctx, &ReadToolCall::ListeningPorts).await.unwrap();
    assert_eq!(text, "0.0.0.0:22/tcp\n");
}
