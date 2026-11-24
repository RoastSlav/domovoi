//! The three identities: account and group names, the path table and the token sets.
//!
//! Pure. The installer, the helper, the self-test and the agent read the same definitions here.

mod accounts;
mod paths;
mod table;
#[cfg(test)]
mod tests;
mod tokens;

pub use accounts::{
    AGENT_ACCOUNT, AGENT_GROUP, DOCKER_GROUP, GroupEntry, JOURNAL_GROUP, SKILL_ACCOUNT,
    SkillAccountFault, UserEntry, evaluate_agent_groups, evaluate_docker_membership,
    evaluate_runner_groups, evaluate_skill_account,
};
pub use paths::{
    FactsError, FaultKind, PathFacts, PathFault, PathId, PathKind, PathSpec, Presence, Principal,
    TreeRule, evaluate_path, evaluate_tree,
};
pub use table::{HELPER_STATE_DIR, HostDirs, expected_paths};
pub use tokens::{CheckToken, RefusalToken};
