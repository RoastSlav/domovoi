//! Scripted hosts and executors shared by the self-test tests.

use domovoi_core::identity::{
    AGENT_ACCOUNT, GroupEntry, HostDirs, JOURNAL_GROUP, PathFacts, PathKind, SKILL_ACCOUNT,
    UserEntry, expected_paths,
};
use domovoi_core::isolation::{CheckId, CheckResult, Outcome};
use domovoi_core::ports::{ProcessOutput, ProcessSpec};
use domovoi_helper::selftest::{
    AgentChecksOutput, HealthAnswer, HealthProbe, SELFTEST_INSTANCE, SYSTEMCTL, SelftestEnv,
};
use domovoi_testkit::{FakeHostInspect, FakeProcessExecutor};

pub const AGENT_UID: u32 = 990;
pub const AGENT_GID: u32 = 990;
pub const SKILL_UID: u32 = 991;
pub const SKILL_GID: u32 = 991;
pub const JOURNAL_GID: u32 = 190;
pub const EXE: &str = "/usr/local/bin/domovoi";
pub const HELPER_PID: u32 = 4242;

pub const SOCKET_FRAGMENT: &str = "/etc/systemd/system/domovoi-helper.socket";
pub const SERVICE_FRAGMENT: &str = "/etc/systemd/system/domovoi-helper@.service";
pub const AGENT_FRAGMENT: &str = "/etc/systemd/system/domovoi-agent.service";

pub fn dirs() -> HostDirs {
    HostDirs::default()
}

pub fn agent_user() -> UserEntry {
    UserEntry {
        name: AGENT_ACCOUNT.to_owned(),
        uid: AGENT_UID,
        gid: AGENT_GID,
        shell: "/usr/sbin/nologin".to_owned(),
    }
}

pub fn skill_user() -> UserEntry {
    UserEntry {
        name: SKILL_ACCOUNT.to_owned(),
        uid: SKILL_UID,
        gid: SKILL_GID,
        shell: "/usr/sbin/nologin".to_owned(),
    }
}

pub fn dir_facts(mode: u32, uid: u32, gid: u32) -> PathFacts {
    PathFacts {
        kind: Some(PathKind::Dir),
        symlink: false,
        mode,
        uid,
        gid,
    }
}

/// A root host that matches the path table, has systemd and clean unit files.
pub fn healthy_host() -> FakeHostInspect {
    let mut host = FakeHostInspect::new()
        .ids(0, 0)
        .user(agent_user(), &[AGENT_GID, JOURNAL_GID])
        .user(skill_user(), &[SKILL_GID])
        .group(GroupEntry {
            name: JOURNAL_GROUP.to_owned(),
            gid: JOURNAL_GID,
            members: vec![AGENT_ACCOUNT.to_owned()],
        })
        .path("/run/systemd/system", dir_facts(0o755, 0, 0))
        .text("/etc/os-release", "ID=ubuntu\nVERSION_ID=\"24.04\"\n")
        .kernel_release_text("6.8.0-test")
        .timings(&[900, 1000, 1100, 1000, 950, 1000, 1000, 1000, 1000, 1000])
        .dir("/etc/domovoi", &["agent.toml", "helper.toml", "skills"])
        .text(SOCKET_FRAGMENT, "[Socket]\nSocketUser=root\nSocketGroup=domovoi-agent\nSocketMode=0660\nAccept=yes\n")
        .text(
            SERVICE_FRAGMENT,
            "[Service]\nType=simple\nExecStart=/usr/local/bin/domovoi helper serve\nRuntimeMaxSec=60s\n",
        )
        .text(
            AGENT_FRAGMENT,
            "[Service]\nUser=domovoi-agent\nGroup=domovoi-agent\nExecStart=/usr/local/bin/domovoi agent run\n",
        );
    for spec in expected_paths(&dirs()) {
        let facts = spec.expected_facts((AGENT_UID, AGENT_GID));
        host = host.path(spec.path.clone(), facts);
    }
    host.tree("/etc/domovoi/skills", Vec::new())
        .tree("/var/lib/domovoi-packages", Vec::new())
}

/// The same host seen by the unprivileged agent: it may not reach helper state.
pub fn agent_host() -> FakeHostInspect {
    let host = healthy_host()
        .ids(AGENT_UID, AGENT_GID)
        .groups(&[AGENT_GID, JOURNAL_GID]);
    let helper = dirs().helper_dir;
    host.any_access(
        helper.join("attest.key"),
        domovoi_core::ports::Access::Denied,
    )
    .any_access(
        helper.join("state.lock"),
        domovoi_core::ports::Access::Denied,
    )
    .any_access(helper.clone(), domovoi_core::ports::Access::Denied)
    .any_access("/proc/1/environ", domovoi_core::ports::Access::Denied)
    .any_access(
        format!("/proc/{HELPER_PID}/environ"),
        domovoi_core::ports::Access::Denied,
    )
}

pub struct FakeHealth(pub Result<HealthAnswer, String>);

impl FakeHealth {
    pub fn ok() -> Self {
        Self(Ok(HealthAnswer { pid: HELPER_PID }))
    }
}

impl HealthProbe for FakeHealth {
    fn health(&self, _socket: &std::path::Path) -> Result<HealthAnswer, String> {
        self.0.clone()
    }
}

pub fn ok_output(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: Some(0),
        stdout: stdout.to_owned(),
        ..ProcessOutput::default()
    }
}

pub fn socket_show() -> String {
    format!(
        "LoadState=loaded\nFragmentPath={SOCKET_FRAGMENT}\nDropInPaths=\nNeedDaemonReload=no\nAccept=yes\nSocketMode=0660\nSocketUser=root\nSocketGroup={AGENT_ACCOUNT}\n"
    )
}

pub fn service_show() -> String {
    format!(
        "LoadState=loaded\nFragmentPath={SERVICE_FRAGMENT}\nDropInPaths=\nNeedDaemonReload=no\nType=simple\nUser=\nRuntimeMaxUSec=1min\n"
    )
}

pub fn agent_show() -> String {
    format!(
        "LoadState=loaded\nFragmentPath={AGENT_FRAGMENT}\nDropInPaths=\nNeedDaemonReload=no\nUser={AGENT_ACCOUNT}\nGroup={AGENT_ACCOUNT}\n"
    )
}

const COMMON: &str = "LoadState,FragmentPath,DropInPaths,NeedDaemonReload";

pub fn expect_show(exec: &FakeProcessExecutor, unit: &str, extra: &str, stdout: &str) {
    let property = format!("--property={COMMON},{extra}");
    exec.expect(
        SYSTEMCTL,
        &["show", "--no-pager", &property, unit],
        ok_output(stdout),
    );
}

pub const SOCKET_UNIT: &str = "domovoi-helper.socket";
pub const SERVICE_UNIT: &str = "domovoi-helper@selftest.service";
pub const AGENT_UNIT: &str = "domovoi-agent.service";
pub const SOCKET_PROPS: &str = "Accept,SocketMode,SocketUser,SocketGroup";
pub const SERVICE_PROPS: &str = "Type,User,RuntimeMaxUSec";
pub const AGENT_PROPS: &str = "User,Group";

pub fn executor_with(socket: &str, service: &str, agent: &str) -> FakeProcessExecutor {
    let exec = FakeProcessExecutor::new();
    exec.expect(
        SYSTEMCTL,
        &["--version"],
        ok_output("systemd 255 (255.4-1ubuntu8)\n+PAM +AUDIT\n"),
    );
    expect_show(&exec, SOCKET_UNIT, SOCKET_PROPS, socket);
    expect_show(&exec, SERVICE_UNIT, SERVICE_PROPS, service);
    expect_show(&exec, AGENT_UNIT, AGENT_PROPS, agent);
    exec
}

pub fn healthy_executor() -> FakeProcessExecutor {
    executor_with(&socket_show(), &service_show(), &agent_show())
}

/// What the agent-checks child prints when everything passes.
pub fn child_json() -> String {
    let results = [
        CheckId::St01,
        CheckId::St02,
        CheckId::St12,
        CheckId::St14,
        CheckId::St15,
        CheckId::St16,
    ]
    .into_iter()
    .map(|id| CheckResult::new(id, Outcome::Pass))
    .collect();
    serde_json::to_string(&AgentChecksOutput {
        boundary: domovoi_core::isolation::BoundaryState::InEffect,
        results,
    })
    .expect("json")
}

pub fn expect_child(exec: &FakeProcessExecutor, stdout: &str) {
    exec.expect(
        EXE,
        &["agent", "selftest", "--agent-checks", "--json"],
        ok_output(stdout),
    );
}

pub fn env<'a>(
    inspect: &'a FakeHostInspect,
    executor: &'a FakeProcessExecutor,
    health: &'a FakeHealth,
    dirs: &'a HostDirs,
) -> SelftestEnv<'a> {
    SelftestEnv {
        inspect,
        executor,
        health,
        dirs,
        exe: EXE,
        systemctl: SYSTEMCTL,
    }
}

pub fn result(results: &[CheckResult], id: CheckId) -> &CheckResult {
    results.iter().find(|r| r.id == id).expect("result present")
}

pub fn programs(exec: &FakeProcessExecutor) -> Vec<ProcessSpec> {
    exec.calls()
}

pub const INSTANCE: &str = SELFTEST_INSTANCE;
