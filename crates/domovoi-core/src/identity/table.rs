//! The path table of the three-identities layout, built from the host's directories.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::paths::{PathId, PathKind, PathSpec, Presence, Principal, TreeRule};

/// The directories the table is built from.
///
/// This mirrors the agent's configured directories without depending on the config crate (core is
/// pure); callers fill it from `AgentDirs`. `Default` gives the installed locations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostDirs {
    /// `/etc/domovoi`.
    pub etc_dir: PathBuf,
    /// The skills directory (`agent.skills_dir`).
    pub skills_dir: PathBuf,
    /// The agent state directory (`agent.state_dir`).
    pub state_dir: PathBuf,
    /// The packages directory (`agent.packages_dir`).
    pub packages_dir: PathBuf,
    /// The helper state directory.
    pub helper_dir: PathBuf,
    /// The helper socket node.
    pub helper_socket: PathBuf,
}

/// The helper state directory, where the path table puts it.
pub const HELPER_STATE_DIR: &str = "/var/lib/domovoi-helper";

impl Default for HostDirs {
    fn default() -> Self {
        Self {
            etc_dir: PathBuf::from("/etc/domovoi"),
            skills_dir: PathBuf::from("/etc/domovoi/skills"),
            state_dir: PathBuf::from("/var/lib/domovoi-agent"),
            packages_dir: PathBuf::from("/var/lib/domovoi-packages"),
            helper_dir: PathBuf::from(HELPER_STATE_DIR),
            helper_socket: PathBuf::from(crate::helper::HELPER_SOCKET),
        }
    }
}

/// The table for `dirs`, in the order the installer creates the paths.
#[must_use]
pub fn expected_paths(dirs: &HostDirs) -> Vec<PathSpec> {
    use PathId as Id;
    use PathKind::{Dir, File, Socket};
    use Presence::{IfPresent, Required};
    use Principal::{Agent, Root};
    let spec = |id, path: PathBuf, kind, owner, group, mode, required, tree| PathSpec {
        id,
        path,
        kind,
        owner,
        group,
        mode,
        required,
        tree,
    };
    let none = TreeRule::None;
    vec![
        spec(
            Id::EtcDomovoi,
            dirs.etc_dir.clone(),
            Dir,
            Root,
            Agent,
            0o750,
            Required,
            none,
        ),
        spec(
            Id::SkillsDir,
            dirs.skills_dir.clone(),
            Dir,
            Root,
            Root,
            0o755,
            Required,
            TreeRule::RootOwnedReadOnly,
        ),
        spec(
            Id::HelperToml,
            dirs.etc_dir.join("helper.toml"),
            File,
            Root,
            Root,
            0o600,
            Required,
            none,
        ),
        spec(
            Id::AgentToml,
            dirs.etc_dir.join("agent.toml"),
            File,
            Root,
            Root,
            0o644,
            IfPresent,
            none,
        ),
        spec(
            Id::AgentState,
            dirs.state_dir.clone(),
            Dir,
            Agent,
            Agent,
            0o700,
            Required,
            none,
        ),
        spec(
            Id::PackagesDir,
            dirs.packages_dir.clone(),
            Dir,
            Agent,
            Agent,
            0o755,
            Required,
            TreeRule::NoGroupOtherWrite,
        ),
        spec(
            Id::HelperDir,
            dirs.helper_dir.clone(),
            Dir,
            Root,
            Root,
            0o700,
            Required,
            none,
        ),
        spec(
            Id::HelperLock,
            dirs.helper_dir.join("state.lock"),
            File,
            Root,
            Root,
            0o600,
            Required,
            none,
        ),
        spec(
            Id::HelperKey,
            dirs.helper_dir.join("attest.key"),
            File,
            Root,
            Root,
            0o600,
            IfPresent,
            none,
        ),
        spec(
            Id::HelperState,
            dirs.helper_dir.join("state"),
            File,
            Root,
            Root,
            0o600,
            IfPresent,
            none,
        ),
        spec(
            Id::HelperPlatform,
            dirs.helper_dir.join("platform.json"),
            File,
            Root,
            Root,
            0o600,
            IfPresent,
            none,
        ),
        spec(
            Id::HelperSocket,
            dirs.helper_socket.clone(),
            Socket,
            Root,
            Agent,
            0o660,
            Required,
            none,
        ),
    ]
}
