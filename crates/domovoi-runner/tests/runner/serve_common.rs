//! Shared pieces for the serve tests: a fake host, skill folders, and the client side of a
//! `UnixStream::pair()` connection.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use domovoi_core::SkillName;
use domovoi_core::action::ImplHash;
use domovoi_core::id::ExecutionId;
use domovoi_core::identity::{AGENT_ACCOUNT, HostDirs, SKILL_ACCOUNT, UserEntry};
use domovoi_core::isolation::{IsolationReport, ProcStatus};
use domovoi_core::package::{PackageName, PackageVersion};
use domovoi_core::ports::ProcessExecutor;
use domovoi_core::skill_runner::{
    MAX_FRAME_BYTES, RUNNER_PROTOCOL, RunRequest, RunnerReply, RunnerRequest, SkillSource,
    decode_payload, encode_frame, frame_len,
};
use domovoi_runner::SkillCatalog;
use domovoi_runner::serve::{ProbeCollector, ServeContext, ServeError, serve_connection_with_peer};
use domovoi_testkit::FakeHostInspect;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::task::JoinHandle;

use crate::helpers::{chmod, executor, spec};

/// The uid the fake host gives `domovoi-agent`.
pub const AGENT_UID: u32 = 991;
/// The uid and gid the fake host gives `domovoi-skill`.
pub const SKILL_ID: u32 = 990;

/// A well-formed `/proc/self/status` excerpt for the fake host.
pub fn proc_status(cap_eff: &str, cap_bnd: &str, no_new_privs: u8) -> String {
    format!(
        "Name:\trunner\nUid:\t{SKILL_ID}\t{SKILL_ID}\t{SKILL_ID}\t{SKILL_ID}\n\
         Gid:\t{SKILL_ID}\t{SKILL_ID}\t{SKILL_ID}\t{SKILL_ID}\nGroups:\t{SKILL_ID}\n\
         CapEff:\t{cap_eff}\nCapBnd:\t{cap_bnd}\nNoNewPrivs:\t{no_new_privs}\n"
    )
}

/// A host where both accounts exist and the runner is clean.
pub fn good_inspect() -> FakeHostInspect {
    inspect_with(
        &[SKILL_ID],
        &proc_status("0000000000000000", "0000000000000000", 1),
    )
}

/// A host with the given supplementary groups and `/proc/self/status` text.
pub fn inspect_with(groups: &[u32], status: &str) -> FakeHostInspect {
    let account = |name: &str, id: u32| UserEntry {
        name: name.to_owned(),
        uid: id,
        gid: id,
        shell: "/usr/sbin/nologin".to_owned(),
    };
    FakeHostInspect::new()
        .ids(SKILL_ID, SKILL_ID)
        .groups(groups)
        .user(account(SKILL_ACCOUNT, SKILL_ID), &[SKILL_ID])
        .user(account(AGENT_ACCOUNT, AGENT_UID), &[AGENT_UID])
        .text("/proc/self/status", status)
}

/// An isolation report with nothing special in it.
pub fn sample_report() -> IsolationReport {
    IsolationReport {
        dirs: HostDirs::default(),
        connects: vec![],
        hidden_state: vec![],
        etc_listing: Some(vec!["skills".to_owned()]),
        var_lib_listing: None,
        config_files: vec![],
        run: None,
        writes: vec![],
        homes: vec![],
        run_user_exists: false,
        proc_hidden: vec![],
        status: ProcStatus {
            uid: SKILL_ID,
            gid: SKILL_ID,
            groups: vec![],
            cap_eff: 0,
            cap_bnd: 0,
            no_new_privs: true,
        },
        mounts: vec![],
        apparmor_label: None,
        shadow: None,
    }
}

/// A probe collector that returns a fixed report and records the pids it was asked about.
pub struct StubProbe {
    pub report: IsolationReport,
    pub asked: Mutex<Vec<Option<u32>>>,
}

impl StubProbe {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            report: sample_report(),
            asked: Mutex::new(Vec::new()),
        })
    }
}

impl ProbeCollector for StubProbe {
    fn collect(&self, helper_pid: Option<u32>) -> IsolationReport {
        self.asked.lock().unwrap().push(helper_pid);
        self.report.clone()
    }
}

/// Two roots and a scratch directory, all temporary.
pub struct Env {
    pub skills: TempDir,
    pub packages: TempDir,
    pub scratch: TempDir,
}

impl Env {
    pub fn new() -> Self {
        Self {
            skills: tempfile::tempdir().unwrap(),
            packages: tempfile::tempdir().unwrap(),
            scratch: tempfile::tempdir().unwrap(),
        }
    }

    pub fn ctx_with(&self, inspect: FakeHostInspect, probe: Arc<StubProbe>) -> ServeContext {
        ServeContext::new(
            self.skills.path().to_path_buf(),
            self.packages.path().to_path_buf(),
            Arc::new(inspect),
            probe,
        )
    }

    pub fn ctx(&self) -> ServeContext {
        self.ctx_with(good_inspect(), StubProbe::new())
    }

    /// `<packages>/<name>/<version>/skills`.
    pub fn package_root(&self, name: &str, version: &str) -> PathBuf {
        self.packages.path().join(name).join(version).join("skills")
    }

    pub fn marker(&self, name: &str) -> PathBuf {
        self.scratch.path().join(name)
    }
}

/// What a test skill's manifest says besides its name.
pub struct SkillSpec<'a> {
    pub run_as: Option<&'a str>,
    pub timeout: &'a str,
    pub max_output_bytes: u32,
    pub body: &'a str,
}

impl<'a> SkillSpec<'a> {
    pub fn new(body: &'a str) -> Self {
        Self {
            run_as: None,
            timeout: "30s",
            max_output_bytes: 4096,
            body,
        }
    }
}

/// Writes skill `name` under `root` with one optional parameter `first`.
///
/// The script is run once with `WARM=1` (it exits at once) before the test uses it, so that a
/// concurrent fork cannot leave it open for writing when the real run starts ("Text file busy").
pub async fn write_skill(root: &Path, name: &str, skill: &SkillSpec<'_>) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let run_as = skill
        .run_as
        .map(|user| format!("run_as = \"{user}\"\n"))
        .unwrap_or_default();
    let manifest = format!(
        "name = \"{name}\"\ndescription = \"A serve test skill.\"\nrisk = \"read\"\nentry = \"run\"\n\
         timeout = \"{}\"\nmax_output_bytes = {}\n{run_as}\n[[params]]\nname = \"first\"\n\
         description = \"First value\"\nrequired = false\n\
         kind = {{ type = \"string\", pattern = '^[ -~]{{0,64}}$', max_len = 64 }}\n",
        skill.timeout, skill.max_output_bytes
    );
    std::fs::write(dir.join("skill.toml"), manifest).unwrap();
    std::fs::write(
        dir.join("run"),
        format!("#!/bin/sh\n[ \"$WARM\" = 1 ] && exit 0\n{}\n", skill.body),
    )
    .unwrap();
    chmod(&dir, 0o755);
    chmod(&dir.join("skill.toml"), 0o644);
    chmod(&dir.join("run"), 0o755);
    let mut warm = spec(dir.join("run").to_str().unwrap(), &[]);
    warm.env = BTreeMap::from([("WARM".to_owned(), "1".to_owned())]);
    executor().run(warm).await.unwrap();
    dir
}

/// The `ImplHash` the agent would send for `<root>/<name>`.
pub fn impl_hash_of(root: &Path, name: &str) -> ImplHash {
    let (_, skill) = SkillCatalog::load_one(root, &SkillName::parse(name).unwrap()).unwrap();
    ImplHash::from_parts(skill.manifest_hash(), skill.entry_script_digest())
}

/// A run request for skill `name` under the skills root, with the right hash.
pub fn run_request(root: &Path, name: &str, args: &[(&str, &str)]) -> RunRequest {
    RunRequest {
        protocol: RUNNER_PROTOCOL,
        execution_id: ExecutionId::new(),
        source: SkillSource::SkillsDir,
        skill: SkillName::parse(name).unwrap(),
        impl_hash: impl_hash_of(root, name),
        args: args
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        timeout_secs: 30,
        max_output_bytes: 65_536,
    }
}

/// A run request for a package skill.
pub fn package_request(env: &Env, pkg: &str, version: &str, name: &str) -> RunRequest {
    let mut request = run_request(&env.package_root(pkg, version), name, &[]);
    request.source = SkillSource::Package {
        name: PackageName::parse(pkg).unwrap(),
        version: PackageVersion::parse(version).unwrap(),
    };
    request
}

/// The client end of a connection and the task serving the other end.
pub struct Conn {
    pub client: UnixStream,
    pub server: JoinHandle<Result<(), ServeError>>,
}

/// Connects a client to a new server task with `ctx`, as `peer`.
pub fn connect(ctx: ServeContext, peer: Option<u32>) -> Conn {
    let (client, server) = UnixStream::pair().unwrap();
    Conn {
        client,
        server: tokio::spawn(serve_connection_with_peer(server, peer, ctx)),
    }
}

/// Writes `request` as one frame.
pub async fn send(client: &mut UnixStream, request: &RunnerRequest) {
    let frame = encode_frame(request, MAX_FRAME_BYTES).unwrap();
    client.write_all(&frame).await.unwrap();
}

/// Reads one reply frame; `None` when the server closed without writing one.
pub async fn read_reply(client: &mut UnixStream) -> Option<RunnerReply> {
    let mut header = [0_u8; 4];
    client.read_exact(&mut header).await.ok()?;
    let len = frame_len(header, MAX_FRAME_BYTES).unwrap();
    let mut body = vec![0_u8; len];
    client.read_exact(&mut body).await.unwrap();
    Some(decode_payload(&body).unwrap())
}

/// Waits for the server task and asserts it returned `Ok`, which is the process exiting 0.
pub async fn finish(server: JoinHandle<Result<(), ServeError>>) {
    let done = tokio::time::timeout(Duration::from_secs(15), server)
        .await
        .expect("the serve future did not return");
    done.unwrap().unwrap();
}

/// One request on a fresh connection as `peer`; the server must return `Ok`.
pub async fn roundtrip_as(env: &Env, peer: Option<u32>, request: RunnerRequest) -> RunnerReply {
    let mut conn = connect(env.ctx(), peer);
    send(&mut conn.client, &request).await;
    let reply = read_reply(&mut conn.client).await.expect("no reply");
    finish(conn.server).await;
    reply
}

/// One request as the agent.
pub async fn roundtrip(env: &Env, request: RunnerRequest) -> RunnerReply {
    roundtrip_as(env, Some(AGENT_UID), request).await
}

/// Polls until `path` exists, or fails after five seconds.
pub async fn wait_for_file(path: &Path) -> String {
    for _ in 0..250 {
        if let Ok(text) = std::fs::read_to_string(path)
            && text.ends_with('\n')
        {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("{} never appeared", path.display());
}

/// Whether `pid` is a live process (a zombie is gone for this purpose).
pub fn alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_some_and(|state| state != "Z"),
        Err(_) => false,
    }
}

/// Polls until `pid` is gone, or fails after five seconds.
pub async fn wait_gone(pid: u32) {
    for _ in 0..250 {
        if !alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("process {pid} survived the cancel");
}
