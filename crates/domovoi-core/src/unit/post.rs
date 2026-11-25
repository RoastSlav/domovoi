//! The check after a restart: does the new process run what was approved?
//!
//! It reads the process, never the loaded properties again: re-reading them after a restart
//! raised false alarms in every measured run. A mismatch names fields, never values.

use super::props::{LoadedProps, ProcessView, UnitRuntime};
use super::refusal::UnitRefusal;

/// How the new process compared with what was approved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostCheck {
    /// The process matches the bound environment, command line and user.
    Verified,
    /// The process could not be checked (a one-shot unit, or it exited before the read).
    Unverified,
}

/// Checks a finished restart.
///
/// `expected_uid` is the uid of the bound `User`, when the unit names one. `process` is `None`
/// when no process could be read.
pub fn post_restart_check(
    bound: &LoadedProps,
    expected_uid: Option<u32>,
    before: &UnitRuntime,
    after: &UnitRuntime,
    process: Option<&ProcessView>,
) -> Result<PostCheck, UnitRefusal> {
    if after.invocation_id == before.invocation_id {
        return Err(UnitRefusal::NoRestartObserved);
    }
    let oneshot = bound.service_type == "oneshot";
    if !oneshot && after.active_state != "active" {
        return Err(UnitRefusal::RestartFailed);
    }
    let Some(process) = process.filter(|_| !oneshot && after.main_pid != 0) else {
        return Ok(PostCheck::Unverified);
    };
    let mut fields = Vec::new();
    if !bound
        .environment
        .iter()
        .all(|e| process.environ.contains(e))
    {
        fields.push("environment".to_owned());
    }
    if let Some(start) = bound.exec_start.first() {
        let argv_matches = process.cmdline.len() >= start.argv.len()
            && process
                .cmdline
                .iter()
                .zip(&start.argv)
                .skip(1)
                .all(|(a, b)| a == b);
        if !argv_matches {
            fields.push("exec_start".to_owned());
        }
    }
    if expected_uid.is_some_and(|uid| uid != process.uid) {
        fields.push("user".to_owned());
    }
    if fields.is_empty() {
        Ok(PostCheck::Verified)
    } else {
        Err(UnitRefusal::WrongStart { fields })
    }
}
