//! Shared fixtures: a healthy scripted host and small builders.

use domovoi_core::identity::{AGENT_ACCOUNT, HostDirs, PathFacts, PathKind, UserEntry};
use domovoi_testkit::FakeHostInspect;

/// The uid and gid the scripted `domovoi-agent` account has.
pub const AGENT_ID: u32 = 900;

/// A `helper.toml` that parses.
pub const GOOD_TOML: &str = "groups = [\"docker.read\", \"smart.read\"]\n";

/// The scripted `domovoi-agent` account.
pub fn agent_user() -> UserEntry {
    UserEntry {
        name: AGENT_ACCOUNT.to_owned(),
        uid: AGENT_ID,
        gid: AGENT_ID,
        shell: "/usr/sbin/nologin".to_owned(),
    }
}

/// Facts of a path of `kind` owned by `uid:gid`.
pub fn facts(kind: PathKind, mode: u32, uid: u32, gid: u32) -> PathFacts {
    PathFacts {
        kind: Some(kind),
        symlink: false,
        mode,
        uid,
        gid,
    }
}

/// A root-owned directory.
pub fn root_dir(mode: u32) -> PathFacts {
    facts(PathKind::Dir, mode, 0, 0)
}

/// A root-owned regular file.
pub fn root_file(mode: u32) -> PathFacts {
    facts(PathKind::File, mode, 0, 0)
}

/// A host on which every start check passes, with `toml` as `helper.toml`.
pub fn healthy_with(dirs: &HostDirs, toml: &str) -> FakeHostInspect {
    FakeHostInspect::new()
        .ids(0, 0)
        .user(agent_user(), &[AGENT_ID])
        .path(&dirs.helper_dir, root_dir(0o700))
        .path(dirs.helper_dir.join("state.lock"), root_file(0o600))
        .path(dirs.etc_dir.join("helper.toml"), root_file(0o600))
        .text(dirs.etc_dir.join("helper.toml"), toml)
}

/// A host on which every start check passes.
pub fn healthy(dirs: &HostDirs) -> FakeHostInspect {
    healthy_with(dirs, GOOD_TOML)
}
