//! The checks only the unprivileged agent can make: ST-01, ST-02, ST-12, ST-14, ST-15, ST-16.
//!
//! They run in the agent's own process (`domovoi agent selftest`), or in a child that
//! `selftest --installer` starts as the agent account. A check that needs the agent's identity
//! reports `nd` when the process is somebody else: root reaching the helper state proves nothing.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use domovoi_core::helper::{
    HELPER_PROTOCOL, HelperRequest, HelperRequestFrame, HelperResponse, decode_response,
    encode_frame, frame_len,
};
use domovoi_core::identity::{
    AGENT_ACCOUNT, CheckToken, DOCKER_GROUP, HostDirs, JOURNAL_GROUP, PathId, PathSpec, UserEntry,
    evaluate_agent_groups, evaluate_path, expected_paths,
};
use domovoi_core::isolation::{BoundaryReason, BoundaryState, CheckId, CheckResult, Outcome};
use domovoi_core::ports::{Access, HostInspect, InspectError, Probe};
use domovoi_core::skill_runner::RUNNER_SOCKET_PATH;
use uuid::Uuid;

/// Where Docker listens on a host install.
pub const DOCKER_SOCKET: &str = "/var/run/docker.sock";

/// What a helper that answered `health` said about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthAnswer {
    /// The pid of the serving process.
    pub pid: u32,
}

/// Connects to the helper socket and sends `health` (ST-14).
///
/// A port so the check can be tested without a socket; [`UnixHealthProbe`] is the real one.
pub trait HealthProbe: Send + Sync {
    /// Asks the helper at `socket` for its health; the error is a short reason.
    fn health(&self, socket: &Path) -> Result<HealthAnswer, String>;

    /// What the skill runner answered to `health` on its socket. The runner speaks another
    /// protocol and the client is async, so the binary asks beforehand and reports here; the
    /// default is a host without a runner.
    fn runner_health(&self) -> RunnerHealth {
        RunnerHealth::NotInstalled
    }
}

/// The runner half of ST-14: whether a `health` connect to the runner socket worked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerHealth {
    /// No runner socket exists (or nobody asked).
    NotInstalled,
    /// The runner answered, or refused with its own token: the socket connects.
    Answered,
    /// The socket exists but the connect or the exchange failed; the text is why.
    Unreachable(String),
}

/// The real [`HealthProbe`]: one blocking request over a Unix socket with socket timeouts.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnixHealthProbe;

const IO_TIMEOUT: Duration = Duration::from_secs(5);

fn exchange(socket: &Path) -> Result<HealthAnswer, String> {
    let mut stream = UnixStream::connect(socket).map_err(|e| format!("connect: {e}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .map_err(|e| format!("socket timeout: {e}"))?;
    let frame = HelperRequestFrame {
        v: HELPER_PROTOCOL,
        id: Uuid::now_v7(),
        request: HelperRequest::Health,
    };
    let bytes = encode_frame(&frame).map_err(|e| e.to_string())?;
    stream
        .write_all(&bytes)
        .map_err(|e| format!("write: {e}"))?;
    let mut header = [0_u8; 4];
    stream
        .read_exact(&mut header)
        .map_err(|e| format!("read: {e}"))?;
    let len = frame_len(header).map_err(|e| e.to_string())?;
    let mut body = vec![0_u8; len];
    stream
        .read_exact(&mut body)
        .map_err(|e| format!("read: {e}"))?;
    match decode_response(&body).map_err(|e| e.to_string())?.response {
        HelperResponse::Health { protocol, pid, .. } if protocol == HELPER_PROTOCOL => {
            Ok(HealthAnswer { pid })
        }
        HelperResponse::Health { protocol, .. } => Err(format!("protocol {protocol}")),
        HelperResponse::Refused { token } | HelperResponse::StartRefused { token, .. } => {
            Err(format!("refused: {token}"))
        }
        _ => Err("unexpected answer".to_owned()),
    }
}

impl HealthProbe for UnixHealthProbe {
    fn health(&self, socket: &Path) -> Result<HealthAnswer, String> {
        exchange(socket)
    }
}

fn not_agent() -> Outcome {
    Outcome::nd("not_running_as_agent: this check needs the domovoi-agent account")
}

fn boundary_detail(reason: &BoundaryReason) -> String {
    match reason {
        BoundaryReason::AgentIsRoot => "the agent runs as root".to_owned(),
        BoundaryReason::AgentNotDomovoiAgent { uid } => {
            format!("uid {uid} is not the domovoi-agent account")
        }
        BoundaryReason::AgentGroupsUnexpected { gids } => format!("unexpected groups {gids:?}"),
        BoundaryReason::AccountMissing => "the domovoi-agent account does not exist".to_owned(),
    }
}

/// ST-01: the process is the `domovoi-agent` account, not root.
fn st01(boundary: &BoundaryState) -> CheckResult {
    let outcome = match boundary {
        BoundaryState::InEffect
        | BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentGroupsUnexpected { .. },
        } => Outcome::Pass,
        BoundaryState::NotInEffect { reason } => Outcome::fail(
            CheckToken::PrivilegeBoundaryNotInEffect,
            boundary_detail(reason),
        ),
    };
    CheckResult::new(CheckId::St01, outcome)
}

fn st02(inspect: &dyn HostInspect, agent: Option<&UserEntry>) -> CheckResult {
    let Some(agent) = agent else {
        return CheckResult::new(CheckId::St02, not_agent());
    };
    let look = || -> Result<Result<(), Vec<u32>>, InspectError> {
        let groups = inspect.supplementary_groups()?;
        let journal = inspect.group(JOURNAL_GROUP)?.map(|g| g.gid);
        let docker = inspect.group(DOCKER_GROUP)?.map(|g| g.gid);
        Ok(evaluate_agent_groups(&groups, agent.gid, journal, docker))
    };
    let outcome = match look() {
        Ok(Ok(())) => Outcome::Pass,
        Ok(Err(gids)) => Outcome::fail(
            CheckToken::AgentGroupsUnexpected,
            format!("the agent's groups are {gids:?}, expected its own and systemd-journal"),
        ),
        Err(e) => Outcome::nd(format!("groups could not be read: {e}")),
    };
    CheckResult::new(CheckId::St02, outcome)
}

fn st12(inspect: &dyn HostInspect, dirs: &HostDirs, agent: Option<&UserEntry>) -> CheckResult {
    let Some(agent) = agent else {
        return CheckResult::new(
            CheckId::St12,
            Outcome::nd("agent_account_missing: ownership cannot be resolved"),
        );
    };
    let Some(spec) = expected_paths(dirs)
        .into_iter()
        .find(|s| s.id == PathId::HelperSocket)
    else {
        return CheckResult::new(
            CheckId::St12,
            Outcome::nd("the path table has no socket row"),
        );
    };
    // The runner's node has the helper's expectations: socket, root:domovoi-agent, 0660.
    let runner_spec = PathSpec {
        path: PathBuf::from(RUNNER_SOCKET_PATH),
        ..spec.clone()
    };
    let owner = (agent.uid, agent.gid);
    let helper = inspect.lstat(&spec.path).map_err(Into::into);
    let runner = inspect.lstat(&runner_spec.path).map_err(Into::into);
    let helper_absent = matches!(helper, Err(domovoi_core::identity::FactsError::NotFound));
    let runner_absent = matches!(runner, Err(domovoi_core::identity::FactsError::NotFound));
    let mut faults = Vec::new();
    let mut wrong = Vec::new();
    for (spec, found, absent) in [
        (&spec, helper, helper_absent),
        (&runner_spec, runner, runner_absent),
    ] {
        if absent {
            continue;
        }
        if let Some(fault) = evaluate_path(spec, found, owner) {
            wrong.push(spec.path.display().to_string());
            faults.push(fault);
        }
    }
    if !faults.is_empty() {
        return CheckResult::new(
            CheckId::St12,
            Outcome::fail(
                CheckToken::SocketModeWrong,
                format!("{} is not root:domovoi-agent 0660", wrong.join(", ")),
            ),
        )
        .with_faults(faults);
    }
    if helper_absent {
        return CheckResult::new(
            CheckId::St12,
            Outcome::nd("helper_socket_absent: the socket unit is not running"),
        );
    }
    CheckResult::new(CheckId::St12, Outcome::Pass)
}

/// The outcome once the helper answered: the runner half decides between `nd`, pass and fail.
fn st14_runner_half(health: &dyn HealthProbe, answer: HealthAnswer) -> Outcome {
    match health.runner_health() {
        RunnerHealth::NotInstalled => Outcome::nd(format!(
            "runner_socket_not_installed; helper socket answered health (pid {})",
            answer.pid
        )),
        RunnerHealth::Answered => Outcome::Pass,
        RunnerHealth::Unreachable(why) => Outcome::fail(
            CheckToken::SocketUnreachable,
            format!("runner socket: {why}"),
        ),
    }
}

fn st14(
    health: &dyn HealthProbe,
    dirs: &HostDirs,
    is_agent: bool,
) -> (CheckResult, Option<HealthAnswer>) {
    if !is_agent {
        return (CheckResult::new(CheckId::St14, not_agent()), None);
    }
    match health.health(&dirs.helper_socket) {
        Err(e) => (
            CheckResult::new(
                CheckId::St14,
                Outcome::fail(CheckToken::SocketUnreachable, format!("helper socket: {e}")),
            ),
            None,
        ),
        Ok(answer) => (
            CheckResult::new(CheckId::St14, st14_runner_half(health, answer)),
            Some(answer),
        ),
    }
}

fn st15(
    inspect: &dyn HostInspect,
    dirs: &HostDirs,
    is_agent: bool,
    helper: Option<HealthAnswer>,
) -> CheckResult {
    if !is_agent {
        return CheckResult::new(CheckId::St15, not_agent());
    }
    let state: [(PathBuf, Probe); 3] = [
        (dirs.helper_dir.join("attest.key"), Probe::Open),
        (dirs.helper_dir.clone(), Probe::List),
        (dirs.helper_dir.join("state.lock"), Probe::Open),
    ];
    let mut procs = vec![PathBuf::from("/proc/1/environ")];
    procs.extend(helper.map(|h| PathBuf::from(format!("/proc/{}/environ", h.pid))));
    let mut reached = Vec::new();
    let mut denied = 0;
    let mut unclear = Vec::new();
    let all = state
        .iter()
        .map(|(p, how)| (p, *how, true))
        .chain(procs.iter().map(|p| (p, Probe::Open, false)));
    for (path, how, is_state) in all {
        match inspect.probe(path, how) {
            Access::Allowed => reached.push(path.display().to_string()),
            Access::Denied => denied += usize::from(is_state),
            Access::NotFound if !is_state => {}
            other => unclear.push(format!("{} gave {other:?}", path.display())),
        }
    }
    let outcome = if !reached.is_empty() {
        Outcome::fail(
            CheckToken::AgentReachesHelperState,
            format!("the agent can reach {}", reached.join(", ")),
        )
    } else if !unclear.is_empty() || denied == 0 {
        Outcome::nd(format!(
            "helper state could not be confirmed closed: {}",
            if unclear.is_empty() {
                "nothing answered EACCES".to_owned()
            } else {
                unclear.join(", ")
            }
        ))
    } else {
        Outcome::Pass
    };
    CheckResult::new(CheckId::St15, outcome)
}

fn st16(inspect: &dyn HostInspect, is_agent: bool) -> CheckResult {
    if !is_agent {
        return CheckResult::new(CheckId::St16, not_agent());
    }
    let outcome = match inspect.probe(Path::new(DOCKER_SOCKET), Probe::Connect) {
        Access::Allowed => Outcome::fail(
            CheckToken::AgentReachesDocker,
            format!("the agent can connect to {DOCKER_SOCKET}"),
        ),
        Access::Denied | Access::NotFound => Outcome::Pass,
        other => Outcome::nd(format!("{DOCKER_SOCKET} gave {other:?}")),
    };
    CheckResult::new(CheckId::St16, outcome)
}

/// Runs ST-01, ST-02, ST-12, ST-14, ST-15 and ST-16 in this process, in that order.
///
/// ST-02, ST-14, ST-15 and ST-16 are `nd` unless this process is the `domovoi-agent` account.
pub fn run_agent_checks(
    inspect: &dyn HostInspect,
    health: &dyn HealthProbe,
    dirs: &HostDirs,
) -> Vec<CheckResult> {
    let account = inspect.user(AGENT_ACCOUNT).ok().flatten();
    let euid = inspect.euid();
    let agent = account.as_ref().filter(|a| euid != 0 && a.uid == euid);
    let boundary = domovoi_core::isolation::inspect_boundary(inspect);
    let (reach, answer) = st14(health, dirs, agent.is_some());
    vec![
        st01(&boundary),
        st02(inspect, agent),
        st12(inspect, dirs, account.as_ref()),
        reach,
        st15(inspect, dirs, agent.is_some(), answer),
        st16(inspect, agent.is_some()),
    ]
}
