//! The runner's start checks ST-05 and ST-06, read through [`HostInspect`]. Neither is repaired.

use std::path::Path;

use domovoi_core::identity::{SKILL_ACCOUNT, evaluate_runner_groups};
use domovoi_core::isolation::{RunnerFault, evaluate_runner_startup, parse_proc_status};
use domovoi_core::ports::HostInspect;
use domovoi_core::skill_runner::{Refusal, RefusalCode};

const PROC_STATUS: &str = "/proc/self/status";

fn refuse(code: RefusalCode, detail: impl Into<String>) -> Refusal {
    Refusal {
        code,
        detail: detail.into(),
    }
}

/// ST-05 then ST-06. The first failure is returned and answers every request.
pub(super) fn run(inspect: &dyn HostInspect) -> Result<(), Refusal> {
    check_groups(inspect)?;
    check_privileges(inspect)
}

/// ST-05: the process holds no supplementary group besides the skill account's own.
fn check_groups(inspect: &dyn HostInspect) -> Result<(), Refusal> {
    let primary = match inspect.user(SKILL_ACCOUNT) {
        Ok(Some(account)) => account.gid,
        Ok(None) | Err(_) => {
            return Err(refuse(
                RefusalCode::RunnerGroupsExtra,
                "the skill account cannot be resolved, so its groups cannot be checked",
            ));
        }
    };
    let groups = inspect.supplementary_groups().map_err(|_| {
        refuse(
            RefusalCode::RunnerGroupsExtra,
            "the supplementary groups of the runner cannot be read",
        )
    })?;
    evaluate_runner_groups(&groups, primary).map_err(|extra| {
        refuse(
            RefusalCode::RunnerGroupsExtra,
            format!("the runner holds groups besides its own: {extra:?}"),
        )
    })
}

/// ST-06: no capabilities in the effective or bounding set, and `NoNewPrivs` set.
fn check_privileges(inspect: &dyn HostInspect) -> Result<(), Refusal> {
    let unreadable = |why: String| refuse(RefusalCode::RunnerPrivileges, why);
    let text = inspect
        .read_text(Path::new(PROC_STATUS))
        .map_err(|_| unreadable(format!("{PROC_STATUS} cannot be read")))?;
    let status = parse_proc_status(&text).map_err(|fault| unreadable(fault.to_string()))?;
    // The groups are ST-05's, read through `getgroups`.
    let faults: Vec<String> = evaluate_runner_startup(&status)
        .into_iter()
        .filter(|fault| !matches!(fault, RunnerFault::ExtraGroups { .. }))
        .map(|fault| fault.to_string())
        .collect();
    if faults.is_empty() {
        Ok(())
    } else {
        Err(unreadable(faults.join("; ")))
    }
}
