//! Whether the agent runs as the unprivileged account: the privilege boundary.

use serde::{Deserialize, Serialize};

use crate::identity::{
    AGENT_ACCOUNT, DOCKER_GROUP, JOURNAL_GROUP, UserEntry, evaluate_agent_groups,
};
use crate::ports::{HostInspect, InspectError};

/// Why the boundary is not in effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryReason {
    /// The agent runs as root.
    AgentIsRoot,
    /// The agent runs as some account other than `domovoi-agent`.
    AgentNotDomovoiAgent {
        /// The uid it runs as.
        uid: u32,
    },
    /// The agent is the right account but holds unexpected groups.
    AgentGroupsUnexpected {
        /// The offending gids.
        gids: Vec<u32>,
    },
    /// The `domovoi-agent` account does not exist.
    AccountMissing,
}

/// Whether this host has the privilege boundary of the three-identities layout.
///
/// A legacy root agent is `NotInEffect`, which is not a blocked platform: the agent keeps working
/// but gets no unattended execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "boundary", rename_all = "snake_case")]
pub enum BoundaryState {
    /// The agent is `domovoi-agent` with exactly the expected groups.
    InEffect,
    /// It is not, for the stated reason.
    NotInEffect {
        /// Why.
        reason: BoundaryReason,
    },
}

impl BoundaryState {
    /// The label shown beside a host whose boundary is not in effect.
    pub const NOT_IN_EFFECT_LABEL: &'static str = "privilege boundary not in effect";

    /// Whether the boundary holds.
    #[must_use]
    pub const fn in_effect(&self) -> bool {
        matches!(self, Self::InEffect)
    }
}

/// ST-01 and ST-02 over plain values.
///
/// `account` is the `domovoi-agent` entry (`None` when absent); `groups` is the process's
/// `getgroups()`; `journal` and `docker` are the gids of those groups when they exist.
#[must_use]
pub fn evaluate_boundary(
    euid: u32,
    account: Option<&UserEntry>,
    groups: &[u32],
    journal: Option<u32>,
    docker: Option<u32>,
) -> BoundaryState {
    if euid == 0 {
        return BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentIsRoot,
        };
    }
    let Some(account) = account else {
        return BoundaryState::NotInEffect {
            reason: BoundaryReason::AccountMissing,
        };
    };
    if account.uid != euid {
        return BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentNotDomovoiAgent { uid: euid },
        };
    }
    match evaluate_agent_groups(groups, account.gid, journal, docker) {
        Ok(()) => BoundaryState::InEffect,
        Err(gids) => BoundaryState::NotInEffect {
            reason: BoundaryReason::AgentGroupsUnexpected { gids },
        },
    }
}

/// [`evaluate_boundary`] over the inspection port, as the agent does once at start.
///
/// An inspection error is reported as the boundary not being in effect for the account being
/// missing: nothing here may claim a boundary it could not look at.
#[must_use]
pub fn inspect_boundary(inspect: &dyn HostInspect) -> BoundaryState {
    let look = || -> Result<BoundaryState, InspectError> {
        let account = inspect.user(AGENT_ACCOUNT)?;
        let groups = inspect.supplementary_groups()?;
        let journal = inspect.group(JOURNAL_GROUP)?.map(|g| g.gid);
        let docker = inspect.group(DOCKER_GROUP)?.map(|g| g.gid);
        Ok(evaluate_boundary(
            inspect.euid(),
            account.as_ref(),
            &groups,
            journal,
            docker,
        ))
    };
    look().unwrap_or(BoundaryState::NotInEffect {
        reason: BoundaryReason::AccountMissing,
    })
}
