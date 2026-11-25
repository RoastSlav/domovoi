//! `unit.restart`: re-resolve the identity, compare it with what was approved, spend the
//! budget, restart by canonical id, and check the new process. In this order, and nothing else.
//!
//! The only `systemctl` argv this module builds is `restart -- <Id>`. It never reloads, never
//! resets a failed unit, never stops or `try-restart`s, never sets a property, and takes no
//! corrective action after a wrong start.

use std::collections::BTreeMap;

use domovoi_core::unit::{
    IdentityHash, PostCheck, UnitId, UnitName, UnitRefusal, compare_identity, keys,
    post_restart_check, resolve_identity,
};

use super::UnitEnv;
use super::budget::Budget;
use crate::reads::tools::{Tool, locate, spec};
use crate::state::StateLock;

/// The argument vector of the one restart command.
#[must_use]
pub fn restart_argv(id: &UnitId) -> Vec<String> {
    vec![
        "restart".to_owned(),
        "--".to_owned(),
        id.as_str().to_owned(),
    ]
}

/// Runs the restart steps for `id`, bound to `hash`. The platform tier gate has already passed.
pub async fn restart(
    env: &UnitEnv<'_>,
    id: &UnitId,
    hash: IdentityHash,
) -> Result<BTreeMap<String, String>, UnitRefusal> {
    // 2: the helper-side lock serialises helper requests only.
    let lock = StateLock::open(env.state_dir).map_err(|_| UnitRefusal::ReadFailed)?;
    let guard = lock.acquire().map_err(|_| UnitRefusal::ReadFailed)?;

    // 3: the id must be the manager's id for that name; every refusal applies; then compare.
    let name = UnitName::parse(id.as_str()).map_err(|_| UnitRefusal::ReadFailed)?;
    // First: the id on the request must be the manager's id for that name, before any other
    // rule runs, so an alias that now means another unit is `target_changed`, not a verdict
    // about the unit it moved to.
    let reported = env
        .sys
        .load_unit(&name)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?;
    if reported.id != id.as_str() {
        return Err(UnitRefusal::TargetChanged {
            field: "id".to_owned(),
        });
    }
    let identity = resolve_identity(&name, env.sys, env.fs, env.policy).await?;
    compare_identity(hash, &identity)?;
    // The bound properties the post-restart check compares the new process with.
    let bound = env
        .sys
        .service_props(id)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?;

    // 5: the budget, spent before the restart so a crash still counts.
    let now = env.clock.now();
    let mut budget = Budget::load(env.state_dir)?;
    budget.check(
        id.as_str(),
        now,
        env.mutations_per_hour,
        bound.start_limit_interval_usec,
        bound.start_limit_burst,
    )?;
    budget.record(id.as_str(), now, bound.start_limit_interval_usec);
    budget.store(&guard, env.state_dir)?;

    // 6
    let before = env
        .sys
        .runtime(id)
        .await
        .map_err(|_| UnitRefusal::ReadFailed)?;

    // 7: `systemctl restart -- <Id>`; the blocking return is the wait for the job (8).
    let program = locate(env.inspect, Tool::Systemctl).ok_or(UnitRefusal::ReadFailed)?;
    let mut command = spec(program, restart_argv(id));
    command.timeout = env.timeout;
    let output = env
        .exec
        .run(command)
        .await
        .map_err(|_| UnitRefusal::RestartFailed)?;
    let stale_start = output.stderr.contains("changed on disk");
    if output.timed_out {
        return Err(UnitRefusal::RestartFailed);
    }
    if output.exit_code != Some(0) {
        // The result is the unit's `Result`, never the message text, which differs by version.
        let after = env.sys.runtime(id).await;
        return Err(match after {
            Ok(runtime) if runtime.result == "start-limit-hit" => UnitRefusal::StartLimitHit,
            _ => UnitRefusal::RestartFailed,
        });
    }

    // 9: the process, never the loaded properties again.
    let after = env
        .sys
        .runtime(id)
        .await
        .map_err(|_| UnitRefusal::NoRestartObserved)?;
    let process = if after.main_pid == 0 {
        None
    } else {
        env.sys.process_view(after.main_pid).await.ok()
    };
    let expected_uid = if bound.user.is_empty() {
        None
    } else {
        env.inspect.user(&bound.user).ok().flatten().map(|u| u.uid)
    };
    let verification = post_restart_check(&bound, expected_uid, &before, &after, process.as_ref())?;

    // 10
    let mut out = BTreeMap::new();
    out.insert(keys::OUT_RESTARTED.to_owned(), "true".to_owned());
    out.insert(keys::OUT_INVOCATION_BEFORE.to_owned(), before.invocation_id);
    out.insert(keys::OUT_INVOCATION_AFTER.to_owned(), after.invocation_id);
    out.insert(keys::OUT_MAIN_PID.to_owned(), after.main_pid.to_string());
    out.insert(
        keys::OUT_VERIFICATION.to_owned(),
        match verification {
            PostCheck::Verified => keys::VERIFIED,
            PostCheck::Unverified => keys::UNVERIFIED,
        }
        .to_owned(),
    );
    out.insert(keys::OUT_STALE_START.to_owned(), stale_start.to_string());
    Ok(out)
}
