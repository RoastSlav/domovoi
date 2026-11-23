//! The path table of the three-identities layout and the evaluator over it.
//!
//! One list drives the installer, ST-10, ST-11, the helper's start check and `helper repair`, so
//! they cannot disagree about a mode.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::tokens::{CheckToken, RefusalToken};
use crate::ports::TreeEntry;

/// The kind of object a path must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathKind {
    /// A directory.
    Dir,
    /// A regular file.
    File,
    /// A Unix socket node.
    Socket,
}

/// Who owns a path, resolved to a numeric id only at run time, never hard-coded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Principal {
    /// uid or gid 0.
    Root,
    /// The `domovoi-agent` account or group.
    Agent,
}

/// Whether absence of a path is a fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    /// The path must exist.
    Required,
    /// The path may be absent (a later milestone creates it); when present it must match.
    IfPresent,
}

/// What the contents of a directory must satisfy beyond the directory itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeRule {
    /// No rule below the path.
    None,
    /// No entry has a group or other write bit.
    NoGroupOtherWrite,
    /// As above, plus every entry is owned by root and files are mode 0644 or 0755.
    RootOwnedReadOnly,
}

/// Names a row of the table, for reports and for choosing the right token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathId {
    /// `/etc/domovoi`.
    EtcDomovoi,
    /// The skills directory.
    SkillsDir,
    /// `helper.toml`.
    HelperToml,
    /// `agent.toml`.
    AgentToml,
    /// The agent's state directory.
    AgentState,
    /// The packages directory.
    PackagesDir,
    /// The helper's state directory.
    HelperDir,
    /// The helper's lock file.
    HelperLock,
    /// The helper's attestation key.
    HelperKey,
    /// The helper's state file.
    HelperState,
    /// The helper's platform record.
    HelperPlatform,
    /// The helper's socket node.
    HelperSocket,
}

impl PathId {
    /// Whether the row lives in the helper's state directory (reported as `helper_state_permissions`).
    #[must_use]
    pub const fn is_helper_state(self) -> bool {
        matches!(
            self,
            Self::HelperDir
                | Self::HelperLock
                | Self::HelperKey
                | Self::HelperState
                | Self::HelperPlatform
        )
    }

    /// The row's stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EtcDomovoi => "etc_domovoi",
            Self::SkillsDir => "skills_dir",
            Self::HelperToml => "helper_toml",
            Self::AgentToml => "agent_toml",
            Self::AgentState => "agent_state",
            Self::PackagesDir => "packages_dir",
            Self::HelperDir => "helper_dir",
            Self::HelperLock => "helper_lock",
            Self::HelperKey => "helper_key",
            Self::HelperState => "helper_state",
            Self::HelperPlatform => "helper_platform",
            Self::HelperSocket => "helper_socket",
        }
    }
}

/// One row of the table: what a path must be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathSpec {
    /// Which row this is.
    pub id: PathId,
    /// Where it lives.
    pub path: PathBuf,
    /// What it must be.
    pub kind: PathKind,
    /// Who owns it.
    pub owner: Principal,
    /// Which group owns it.
    pub group: Principal,
    /// The permission bits it must have.
    pub mode: u32,
    /// Whether absence is a fault.
    pub required: Presence,
    /// What the contents must satisfy.
    pub tree: TreeRule,
}

/// Type, mode and owner of a path, as an inspection found them or as the table expects them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathFacts {
    /// The object type; `None` for a type the table has no name for.
    pub kind: Option<PathKind>,
    /// Whether the path itself is a symlink.
    pub symlink: bool,
    /// Permission bits (`mode & 0o7777`).
    pub mode: u32,
    /// Owner uid.
    pub uid: u32,
    /// Owner gid.
    pub gid: u32,
}

/// Why an inspection produced no facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum FactsError {
    /// The path does not exist.
    #[error("not found")]
    NotFound,
    /// The inspector may not look at the path.
    #[error("permission denied")]
    PermissionDenied,
    /// Any other error.
    #[error("{0}")]
    Io(String),
}

/// How a path differs from the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    /// A required path is absent.
    Missing,
    /// The path is the wrong kind of object.
    WrongType,
    /// The path is a symlink, whatever it points to.
    Symlink,
    /// The permission bits differ.
    Mode,
    /// The owner differs.
    Owner,
    /// The group differs.
    Group,
    /// An entry below a directory has a forbidden write bit or the wrong owner or mode.
    WriteBitInTree,
    /// The path exists but cannot be opened or inspected by this caller.
    Unreadable,
}

/// One departure from the table, with what was found and what was expected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathFault {
    /// The table row.
    pub id: PathId,
    /// The offending path (an entry below the row's path for a tree fault).
    pub path: PathBuf,
    /// What was found; `None` when nothing could be read.
    pub found: Option<PathFacts>,
    /// What the table expects, with ids resolved.
    pub expected: PathFacts,
    /// The first way it differs.
    pub why: FaultKind,
}

impl PathFault {
    /// The self-test token this fault is reported under: helper state is
    /// `helper_state_permissions`, the socket node `socket_mode_wrong`, everything else
    /// `path_mode_wrong`.
    #[must_use]
    pub const fn check_token(&self) -> CheckToken {
        if self.id.is_helper_state() {
            CheckToken::HelperStatePermissions
        } else if matches!(self.id, PathId::HelperSocket) {
            CheckToken::SocketModeWrong
        } else {
            CheckToken::PathModeWrong
        }
    }

    /// The token the helper refuses with when this fault is found at start.
    #[must_use]
    pub const fn refusal_token() -> RefusalToken {
        RefusalToken::HelperStatePermissions
    }
}

fn resolve(principal: Principal, agent_id: u32) -> u32 {
    match principal {
        Principal::Root => 0,
        Principal::Agent => agent_id,
    }
}

impl PathSpec {
    /// The facts this row expects, with owner and group resolved against the agent's `(uid, gid)`.
    #[must_use]
    pub fn expected_facts(&self, agent: (u32, u32)) -> PathFacts {
        PathFacts {
            kind: Some(self.kind),
            symlink: false,
            mode: self.mode,
            uid: resolve(self.owner, agent.0),
            gid: resolve(self.group, agent.1),
        }
    }
}

/// Compares what an inspection found with `spec`; `None` means the path is as the table says.
///
/// `agent` is the resolved `(uid, gid)` of `domovoi-agent`. A symlink is a fault whatever it
/// points to. The first difference found is the fault's `why`; `found` and `expected` carry the
/// rest. An absent path is a fault only when `spec.required` is [`Presence::Required`].
#[must_use]
pub fn evaluate_path(
    spec: &PathSpec,
    found: Result<PathFacts, FactsError>,
    agent: (u32, u32),
) -> Option<PathFault> {
    let expected = spec.expected_facts(agent);
    let fault = |found: Option<PathFacts>, why| {
        Some(PathFault {
            id: spec.id,
            path: spec.path.clone(),
            found,
            expected: expected.clone(),
            why,
        })
    };
    let facts = match found {
        Ok(facts) => facts,
        Err(FactsError::NotFound) => {
            return match spec.required {
                Presence::Required => fault(None, FaultKind::Missing),
                Presence::IfPresent => None,
            };
        }
        Err(FactsError::PermissionDenied | FactsError::Io(_)) => {
            return fault(None, FaultKind::Unreadable);
        }
    };
    let why = if facts.symlink {
        FaultKind::Symlink
    } else if facts.kind != Some(spec.kind) {
        FaultKind::WrongType
    } else if facts.mode & 0o7777 != expected.mode {
        FaultKind::Mode
    } else if facts.uid != expected.uid {
        FaultKind::Owner
    } else if facts.gid != expected.gid {
        FaultKind::Group
    } else {
        return None;
    };
    fault(Some(facts), why)
}

/// Applies the row's tree rule to the entries below its path; every offending entry is a fault.
///
/// `entries` come from a walk that does not follow symlinks. A symlink below the directory is a
/// fault.
#[must_use]
pub fn evaluate_tree(spec: &PathSpec, entries: &[TreeEntry]) -> Vec<PathFault> {
    if spec.tree == TreeRule::None {
        return Vec::new();
    }
    let root_owned = spec.tree == TreeRule::RootOwnedReadOnly;
    let wanted_mode = |f: &PathFacts| {
        if root_owned && f.kind == Some(PathKind::File) {
            if f.mode & 0o111 != 0 { 0o755 } else { 0o644 }
        } else {
            f.mode & 0o7777 & !0o022
        }
    };
    entries
        .iter()
        .filter(|entry| {
            let f = &entry.facts;
            f.symlink
                || f.kind.is_none()
                || f.mode & 0o022 != 0
                || (root_owned && f.uid != 0)
                || (root_owned
                    && f.kind == Some(PathKind::File)
                    && f.mode & 0o7777 != wanted_mode(f))
        })
        .map(|entry| PathFault {
            id: spec.id,
            path: entry.path.clone(),
            found: Some(entry.facts.clone()),
            expected: PathFacts {
                kind: entry.facts.kind,
                symlink: false,
                mode: wanted_mode(&entry.facts),
                uid: if root_owned { 0 } else { entry.facts.uid },
                gid: entry.facts.gid,
            },
            why: FaultKind::WriteBitInTree,
        })
        .collect()
}
