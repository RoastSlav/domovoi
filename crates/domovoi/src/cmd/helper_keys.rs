//! The owner commands of the helper's attestation key: `init` completion, `rotate-key`,
//! `recover` and `fingerprint`, and the executor the route reads of `safety.attest` run through.
//!
//! All of them are run by hand as root. None runs implicitly, and `rotate-key` and `recover`
//! never run from the installer.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use domovoi_core::attest::BootId;
use domovoi_core::identity::HostDirs;
use domovoi_core::ports::{Clock as _, HostInspect as _, ProcessExecutor, Redactor};
use domovoi_helper::attest::lifecycle::{
    KeyInfo, LifecycleError, Replaced, fingerprint, init_attestation, recover, rotate_key,
};
use domovoi_helper::attest::{BOOT_ID_PATH, KEY_FILE, STATE_FILE};
use domovoi_helper::inspect::UnixHostInspect;
use domovoi_helper::state::{InitError, InitOutcome, StateLayout, init};

use crate::ExitCode;
use crate::init::SystemClock;

/// Passes text through unchanged: a pattern redactor could rewrite the route JSON that is about
/// to be hashed.
struct PassThrough;

impl Redactor for PassThrough {
    fn redact(&self, text: &str) -> String {
        text.to_owned()
    }
}

/// The executor the route reads of `safety.attest` run through: the runner's, with no
/// environment of its own, so the whole environment of an `ip` call is what the call names.
pub fn route_reader() -> Arc<dyn ProcessExecutor> {
    Arc::new(
        domovoi_runner::SandboxExecutor::new(Arc::new(PassThrough)).with_base_env(BTreeMap::new()),
    )
}

fn boot_id() -> anyhow::Result<BootId> {
    let text = UnixHostInspect::new().read_text(Path::new(BOOT_ID_PATH))?;
    Ok(BootId::parse(text.trim())?)
}

fn describe(key: &KeyInfo) {
    println!("fingerprint: {}", key.grouped());
    println!("key id:      {}", key.key_id);
}

fn require_root() -> anyhow::Result<()> {
    anyhow::ensure!(
        UnixHostInspect::new().euid() == 0,
        "this command changes root-owned files; run it as root"
    );
    Ok(())
}

/// `helper init`: the directory and lock as before, then the key and the first state.
///
/// A key and a state that both exist are never touched, and say so with a success exit so that
/// running the installer again is harmless. Only one of the two is a failed earlier run, which
/// `recover` resolves.
pub fn init_state(dirs: &HostDirs) -> anyhow::Result<ExitCode> {
    require_root()?;
    let dir = &dirs.helper_dir;
    match init(StateLayout::PRODUCTION, dir) {
        Ok(InitOutcome::Created) => println!("created {}", dir.display()),
        Ok(InitOutcome::AlreadyInitialised) => println!("{} is in place", dir.display()),
        Err(InitError::Faults(faults)) => {
            for fault in &faults {
                crate::cmd::warn(domovoi_helper::selftest::render_fault(fault).trim_end());
            }
            crate::cmd::warn("nothing was changed; run `domovoi helper repair`");
            return Ok(ExitCode::FAILURE);
        }
        Err(err) => return Err(err.into()),
    }
    match (dir.join(KEY_FILE).exists(), dir.join(STATE_FILE).exists()) {
        (true, true) => {
            println!("the attestation key and state already exist; nothing was changed");
            println!("use `domovoi helper rotate-key` or `domovoi helper recover` to replace them");
            Ok(ExitCode::SUCCESS)
        }
        (false, false) => {
            let key = init_attestation(StateLayout::PRODUCTION, dir, &boot_id()?)?;
            println!("created the attestation key and state (generation 0)");
            describe(&key);
            println!("compare this fingerprint with the hub before enrolling the key");
            Ok(ExitCode::SUCCESS)
        }
        _ => {
            crate::cmd::warn(
                "only one of the attestation key and state exists; run `domovoi helper recover`",
            );
            Ok(ExitCode::FAILURE)
        }
    }
}

/// `helper rotate-key` (`rotate` true) and `helper recover`: a new key, with the old public key
/// and state retired.
pub fn replace_key(dirs: &HostDirs, rotate: bool) -> anyhow::Result<ExitCode> {
    require_root()?;
    let now = SystemClock.now();
    let boot = boot_id()?;
    let result = if rotate {
        rotate_key(StateLayout::PRODUCTION, &dirs.helper_dir, &boot, now)
    } else {
        recover(StateLayout::PRODUCTION, &dirs.helper_dir, &boot, now)
    };
    match result {
        Ok(Replaced { old, new, retired }) => {
            match old {
                Some(old) => println!("retired key: {}", old.grouped()),
                None => println!("the old key could not be read; only the state was retired"),
            }
            println!("retired to:  {}", retired.display());
            println!("new key");
            describe(&new);
            println!(
                "the hub shows \"helper key changed\" until the owner enrols this fingerprint"
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(err @ (LifecycleError::NotRotatable | LifecycleError::NothingToRecover)) => {
            crate::cmd::warn(&err.to_string());
            Ok(ExitCode::FAILURE)
        }
        Err(err) => Err(err.into()),
    }
}

/// `helper fingerprint`: the fingerprint and key id, computed from the private key file.
pub fn print_fingerprint(dirs: &HostDirs) -> anyhow::Result<ExitCode> {
    require_root()?;
    match fingerprint(&dirs.helper_dir) {
        Ok(key) => {
            describe(&key);
            Ok(ExitCode::SUCCESS)
        }
        Err(LifecycleError::NoKey) => {
            crate::cmd::warn(
                "there is no usable attestation key; run `domovoi helper init` or `recover`",
            );
            Ok(ExitCode::FAILURE)
        }
        Err(err) => Err(err.into()),
    }
}
