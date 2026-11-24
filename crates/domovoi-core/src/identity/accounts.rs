//! The three identities' names and the pure checks over the account database.
//!
//! This module holds the only non-test definition of the account and group names, so a rename
//! is one edit and a scan can prove nothing else spells them (arch rules IDENT-1 and ID-2).

use serde::{Deserialize, Serialize};

/// The unprivileged account the control agent runs as.
pub const AGENT_ACCOUNT: &str = "domovoi-agent";
/// The group-less account skills run as.
pub const SKILL_ACCOUNT: &str = "domovoi-skill";
/// The group that owns `/etc/domovoi` and the helper socket.
pub const AGENT_GROUP: &str = "domovoi-agent";
/// The only supplementary group the agent may hold, and only when it exists.
pub const JOURNAL_GROUP: &str = "systemd-journal";
/// The group whose membership is root on a Docker host; no Domovoi account may be in it.
pub const DOCKER_GROUP: &str = "docker";

/// One row of the user database, as far as the isolation checks need it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserEntry {
    /// Login name.
    pub name: String,
    /// Numeric user id.
    pub uid: u32,
    /// Primary group id.
    pub gid: u32,
    /// Login shell path.
    pub shell: String,
}

/// One row of the group database, with its listed members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupEntry {
    /// Group name.
    pub name: String,
    /// Numeric group id.
    pub gid: u32,
    /// Login names the group lists as members.
    pub members: Vec<String>,
}

/// Why `domovoi-skill` fails the account check (ST-03).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "fault", rename_all = "snake_case")]
pub enum SkillAccountFault {
    /// The account is uid 0.
    #[error("the skill account is root")]
    IsRoot,
    /// The account shares the agent's uid.
    #[error("the skill account has the agent's uid")]
    SharesAgentUid,
    /// The account has a shell that allows login.
    #[error("the skill account has a login shell: {shell}")]
    LoginShell {
        /// The shell found.
        shell: String,
    },
    /// The account belongs to groups besides its own.
    #[error("the skill account belongs to extra groups: {gids:?}")]
    ExtraGroups {
        /// The group ids found other than the primary.
        gids: Vec<u32>,
    },
    /// Some group lists the account as a member.
    #[error("a group lists the skill account as a member")]
    ListedInGroup,
}

fn extras(groups: &[u32], allowed: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = groups
        .iter()
        .copied()
        .filter(|gid| !allowed.contains(gid))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// ST-02: the agent's `getgroups()` list is its primary group plus `systemd-journal`.
///
/// `groups` is the supplementary list of the running process (it may or may not repeat the
/// primary group). It passes only when every entry is the primary or the journal group, the
/// journal group is present whenever it exists, and neither gid 0 nor the docker gid appears.
/// On failure the gids that should not be there are returned; when the only problem is a missing
/// journal membership the whole list found is returned instead.
pub fn evaluate_agent_groups(
    groups: &[u32],
    primary: u32,
    journal: Option<u32>,
    docker: Option<u32>,
) -> Result<(), Vec<u32>> {
    let mut allowed = vec![primary];
    allowed.extend(journal);
    let mut bad = extras(groups, &allowed);
    if primary == 0 {
        bad.push(0);
    }
    bad.extend(docker.filter(|d| groups.contains(d) && !bad.contains(d)));
    bad.sort_unstable();
    bad.dedup();
    if !bad.is_empty() {
        return Err(bad);
    }
    if journal.is_some_and(|j| !groups.contains(&j)) {
        let mut found = groups.to_vec();
        found.sort_unstable();
        found.dedup();
        return Err(found);
    }
    Ok(())
}

/// ST-03: `domovoi-skill` is neither root nor the agent, cannot log in and has no group but its own.
///
/// `groups_of` is the NSS-aware list of the account (`getgrouplist`); `member_of_any` is true when
/// any group other than its own primary lists the account as a member.
pub fn evaluate_skill_account(
    user: &UserEntry,
    agent_uid: u32,
    groups_of: &[u32],
    member_of_any: bool,
) -> Result<(), SkillAccountFault> {
    if user.uid == 0 {
        return Err(SkillAccountFault::IsRoot);
    }
    if user.uid == agent_uid {
        return Err(SkillAccountFault::SharesAgentUid);
    }
    let shell = user.shell.rsplit('/').next().unwrap_or("");
    if shell != "nologin" && shell != "false" {
        return Err(SkillAccountFault::LoginShell {
            shell: user.shell.clone(),
        });
    }
    let gids = extras(groups_of, &[user.gid]);
    if !gids.is_empty() {
        return Err(SkillAccountFault::ExtraGroups { gids });
    }
    if member_of_any {
        return Err(SkillAccountFault::ListedInGroup);
    }
    Ok(())
}

/// ST-04: neither Domovoi account is listed in the docker group. Returns the offending names.
pub fn evaluate_docker_membership(docker: Option<&GroupEntry>) -> Result<(), Vec<String>> {
    let Some(group) = docker else {
        return Ok(());
    };
    let found: Vec<String> = [AGENT_ACCOUNT, SKILL_ACCOUNT]
        .into_iter()
        .filter(|name| group.members.iter().any(|m| m == name))
        .map(str::to_owned)
        .collect();
    if found.is_empty() { Ok(()) } else { Err(found) }
}

/// ST-05: the runner's groups are exactly its primary group. Returns the extra gids.
///
/// The collection runs inside the runner; this only decides.
pub fn evaluate_runner_groups(groups: &[u32], primary: u32) -> Result<(), Vec<u32>> {
    let bad = extras(groups, &[primary]);
    if bad.is_empty() { Ok(()) } else { Err(bad) }
}
