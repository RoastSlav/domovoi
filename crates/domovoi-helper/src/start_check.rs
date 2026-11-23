//! The helper's start checks: read-only, never a repair.
//!
//! The checks look at the helper's own directory, lock, key, state, platform record and
//! `helper.toml` through [`HostInspect`], which has no method that changes anything, and they
//! report every offending path with what was found and what was expected. Putting a wrong mode
//! right is a separate, owner-confirmed command.

use domovoi_config::parse_helper;
use domovoi_core::identity::{
    AGENT_ACCOUNT, FaultKind, HostDirs, PathFault, PathId, PathKind, PathSpec, RefusalToken,
    evaluate_path, expected_paths,
};
use domovoi_core::ports::HostInspect;

/// Why the helper refuses to serve: the token, and every offending path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("helper start refused: {}", .token)]
pub struct StartRefusal {
    /// The refusal token.
    pub token: RefusalToken,
    /// Every path that departs from the table, with found and expected facts.
    pub faults: Vec<PathFault>,
}

impl StartRefusal {
    fn bare(token: RefusalToken) -> Self {
        Self {
            token,
            faults: Vec::new(),
        }
    }
}

/// The table rows the helper looks at when it starts.
const CHECKED: &[PathId] = &[
    PathId::HelperDir,
    PathId::HelperLock,
    PathId::HelperKey,
    PathId::HelperState,
    PathId::HelperPlatform,
    PathId::HelperToml,
];

fn facts_of(
    inspect: &dyn HostInspect,
    spec: &PathSpec,
) -> Result<domovoi_core::identity::PathFacts, domovoi_core::identity::FactsError> {
    let found = if spec.kind == PathKind::Dir {
        inspect.lstat(&spec.path)
    } else {
        inspect.open_facts(&spec.path)
    };
    found.map_err(Into::into)
}

/// Runs the start checks over `dirs`.
///
/// Order: the `domovoi-agent` account must resolve (`agent_account_missing`); the state directory
/// must exist (`uninitialized`); every checked path must match the table
/// (`helper_state_permissions`, with all faults listed, and an unopenable lock reported as a
/// permission fault, never as corruption); `helper.toml` must parse (`helper_config_invalid`).
/// The absence of the key and the state file does not refuse start in this milestone.
pub fn run_start_checks(inspect: &dyn HostInspect, dirs: &HostDirs) -> Result<(), StartRefusal> {
    let agent = match inspect.user(AGENT_ACCOUNT) {
        Ok(Some(account)) => (account.uid, account.gid),
        Ok(None) | Err(_) => return Err(StartRefusal::bare(RefusalToken::AgentAccountMissing)),
    };
    let specs: Vec<PathSpec> = expected_paths(dirs)
        .into_iter()
        .filter(|spec| CHECKED.contains(&spec.id))
        .collect();
    let mut faults = Vec::new();
    for spec in &specs {
        if let Some(fault) = evaluate_path(spec, facts_of(inspect, spec), agent) {
            let directory_missing =
                fault.id == PathId::HelperDir && fault.why == FaultKind::Missing;
            faults.push(fault);
            if directory_missing {
                return Err(StartRefusal {
                    token: RefusalToken::Uninitialized,
                    faults,
                });
            }
        }
    }
    if !faults.is_empty() {
        let only_lock_missing = faults
            .iter()
            .all(|f| f.id == PathId::HelperLock && f.why == FaultKind::Missing);
        let token = if only_lock_missing {
            RefusalToken::Uninitialized
        } else {
            RefusalToken::HelperStatePermissions
        };
        return Err(StartRefusal { token, faults });
    }
    check_config(inspect, dirs)
}

fn check_config(inspect: &dyn HostInspect, dirs: &HostDirs) -> Result<(), StartRefusal> {
    let path = dirs.etc_dir.join("helper.toml");
    let invalid = |detail: String| {
        tracing::error!(path = %path.display(), "helper.toml is not valid: {detail}");
        StartRefusal::bare(RefusalToken::HelperConfigInvalid)
    };
    let text = inspect
        .read_text(&path)
        .map_err(|e| invalid(e.to_string()))?;
    match parse_helper(&text) {
        Ok(_) => Ok(()),
        Err(issues) => {
            let detail = issues
                .iter()
                .map(|i| format!("{}: {}", i.path, i.message))
                .collect::<Vec<_>>()
                .join("; ");
            Err(invalid(detail))
        }
    }
}
