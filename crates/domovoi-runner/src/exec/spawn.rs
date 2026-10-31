//! Starting the child process.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use domovoi_core::ports::{ProcessError, ProcessSpec};
use nix::unistd::{User, geteuid};
use tokio::process::{Child, Command};

/// A uid and primary gid to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SwitchTo {
    uid: u32,
    gid: u32,
}

/// Resolves `run_as`. Returns `None` when the agent already runs as that user.
pub(super) fn resolve_user(name: &str) -> Result<Option<SwitchTo>, ProcessError> {
    let user = User::from_name(name)
        .map_err(|err| ProcessError::UserSwitch(format!("cannot look up user {name}: {err}")))?
        .ok_or_else(|| ProcessError::UserSwitch(format!("unknown user {name}")))?;
    let current = geteuid();
    if user.uid == current {
        return Ok(None);
    }
    if !current.is_root() {
        return Err(ProcessError::UserSwitch(
            "agent must run as root to switch users".to_owned(),
        ));
    }
    Ok(Some(SwitchTo {
        uid: user.uid.as_raw(),
        gid: user.gid.as_raw(),
    }))
}

/// Spawns `program` with exactly `env`, in its own process group.
// ADR 0004: this is the only `Command::new` in Domovoi. Everything else goes through
// the `ProcessExecutor` port, which this module implements.
#[allow(clippy::disallowed_methods)]
pub(super) fn spawn(
    program: &Path,
    spec: &ProcessSpec,
    env: &BTreeMap<String, String>,
    user: Option<SwitchTo>,
) -> Result<Child, ProcessError> {
    let mut command = Command::new(program);
    command
        .args(&spec.args)
        .env_clear()
        .envs(env)
        .current_dir(spec.cwd.as_deref().unwrap_or("/"))
        .process_group(0)
        .stdin(if spec.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(user) = user {
        command.uid(user.uid).gid(user.gid);
    }
    command.spawn().map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => ProcessError::NotFound,
        std::io::ErrorKind::PermissionDenied => ProcessError::NotExecutable,
        _ => ProcessError::Spawn(err.to_string()),
    })
}
