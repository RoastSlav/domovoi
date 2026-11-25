//! `helper_attestation_key_not_readable_by_agent_or_skill`: the privileged custody probe.
//!
//! Never runs by default: it is `#[ignore]`d and also needs `DOMOVOI_PRIVILEGED_TESTS=1` and root.
//! Run it only on a disposable host:
//!
//! ```text
//! sudo DOMOVOI_PRIVILEGED_TESTS=1 cargo test -p domovoi-helper --test privileged_custody -- --ignored
//! ```
//!
//! It makes a real state directory with a real key and a real state through the lifecycle code as
//! root, then drops to two unprivileged identities in turn (two arbitrary non-root uids; no
//! account has to exist) while keeping the saved root uid, and repeats the probe set as each:
//! `stat`, open, copy, create, rename, link, `chmod`, unlink and a lock open on the key, the state
//! and the lock, a listing of the directory, and `environ`, `mem`, `maps` and `fd` of a running
//! root process (`init`, which every Linux host has). Every probe must be refused. The helper
//! process itself is not started here and `ptrace` and `process_vm_readv` are not exercised: the
//! workspace's `nix` has no `ptrace` feature and the kernel's ptrace rules are what protects a
//! root process from those.
#![cfg(unix)]

#[cfg(test)]
mod custody {
    use std::fs::{File, OpenOptions};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use domovoi_core::attest::BootId;
    use domovoi_helper::attest::lifecycle::init_attestation;
    use domovoi_helper::state::StateLayout;
    use nix::unistd::{Gid, Uid, setresgid, setresuid};

    /// The two unprivileged identities the probes run as: the agent's and a skill's stand-in.
    const IDENTITIES: [(&str, u32); 2] = [("agent", 60_001), ("skill", 60_002)];

    fn enabled() -> bool {
        std::env::var("DOMOVOI_PRIVILEGED_TESTS").is_ok_and(|v| v == "1")
            && Uid::effective().is_root()
    }

    /// Every attempt on `state` that must be refused, as the identity the process now has.
    fn attempts(state: &Path, scratch: &Path) -> Vec<(String, bool)> {
        let mut allowed = Vec::new();
        let mut probe = |what: String, succeeded: bool| allowed.push((what, succeeded));
        probe(
            "list the directory".to_owned(),
            std::fs::read_dir(state).is_ok(),
        );
        for name in ["attest.key", "state", "state.lock"] {
            let path = state.join(name);
            probe(format!("stat {name}"), std::fs::metadata(&path).is_ok());
            probe(format!("open {name}"), File::open(&path).is_ok());
            probe(
                format!("open {name} for locking"),
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&path)
                    .is_ok(),
            );
            probe(
                format!("copy {name}"),
                std::fs::copy(&path, scratch.join("copy")).is_ok(),
            );
            probe(
                format!("rename {name}"),
                std::fs::rename(&path, state.join(format!("{name}.moved"))).is_ok(),
            );
            probe(
                format!("link {name}"),
                std::fs::hard_link(&path, state.join(format!("{name}.link"))).is_ok(),
            );
            probe(
                format!("chmod {name}"),
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).is_ok(),
            );
            probe(
                format!("unlink {name}"),
                std::fs::remove_file(&path).is_ok(),
            );
        }
        probe("create".to_owned(), File::create(state.join("new")).is_ok());
        for file in ["environ", "mem", "maps"] {
            probe(
                format!("read /proc/1/{file}"),
                File::open(format!("/proc/1/{file}")).is_ok(),
            );
        }
        probe(
            "list /proc/1/fd".to_owned(),
            std::fs::read_dir("/proc/1/fd").is_ok(),
        );
        allowed
    }

    #[test]
    #[ignore = "privileged: needs root and DOMOVOI_PRIVILEGED_TESTS=1; run on a disposable host"]
    fn helper_attestation_key_not_readable_by_agent_or_skill() {
        if !enabled() {
            eprintln!("skipped: set DOMOVOI_PRIVILEGED_TESTS=1 and run as root");
            return;
        }
        let parent = tempfile::Builder::new()
            .prefix("domovoi-custody-")
            .tempdir_in("/var/tmp")
            .expect("tempdir");
        std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o755))
            .expect("a traversable parent");
        let state = parent.path().join("helper");
        let boot = BootId::parse("5f0c1a2b-3c4d-4e5f-8a9b-0c1d2e3f4a5b").expect("boot");
        init_attestation(StateLayout::PRODUCTION, &state, &boot).expect("init as root");
        let key_before = std::fs::read(state.join("attest.key")).expect("key");

        for (name, id) in IDENTITIES {
            setresgid(Gid::from_raw(id), Gid::from_raw(id), Gid::from_raw(0)).expect("drop gid");
            setresuid(Uid::from_raw(id), Uid::from_raw(id), Uid::from_raw(0)).expect("drop uid");
            let results = attempts(&state, parent.path());
            setresuid(Uid::from_raw(0), Uid::from_raw(0), Uid::from_raw(0)).expect("regain uid");
            setresgid(Gid::from_raw(0), Gid::from_raw(0), Gid::from_raw(0)).expect("regain gid");
            for (what, succeeded) in results {
                assert!(!succeeded, "{name} could {what}");
            }
        }
        assert_eq!(
            std::fs::read(state.join("attest.key")).expect("key"),
            key_before,
            "the key is untouched"
        );
    }
}
