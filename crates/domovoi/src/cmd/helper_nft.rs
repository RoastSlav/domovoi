//! `domovoi helper nft-selftest`, `nft-selftest-sender` and `containment-drill`.
//!
//! The commands are thin: they build the executor, the host view and the clock, and call the
//! helper crate. Only the one-shot self-test unit and the installer run them; the long-lived
//! helper never does.

use std::sync::Arc;

use domovoi_core::identity::HostDirs;
use domovoi_helper::inspect::UnixHostInspect;
use domovoi_helper::nft::command::{self, Env};

use crate::ExitCode;
use crate::cmd::helper_keys as keys;
use crate::init::SystemClock;

fn env(dirs: HostDirs) -> anyhow::Result<Env> {
    let exe = std::env::current_exe()?;
    Ok(Env {
        exec: keys::route_reader(),
        inspect: Arc::new(UnixHostInspect::new()),
        clock: Arc::new(SystemClock),
        dirs,
        exe: exe.to_string_lossy().into_owned(),
    })
}

fn print(result: Result<Vec<String>, String>) -> anyhow::Result<ExitCode> {
    match result {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(why) => {
            crate::cmd::warn(&format!("could not run: {why}"));
            Ok(ExitCode::FAILURE)
        }
    }
}

/// `nft-selftest`: exit 0 for any completed run, non-zero only when it could not run.
pub async fn selftest(
    dirs: HostDirs,
    if_changed: bool,
    interval_only: bool,
) -> anyhow::Result<ExitCode> {
    print(command::selftest(&env(dirs)?, if_changed, interval_only).await)
}

/// `nft-selftest-sender`: prints the number of datagrams sent.
pub fn sender(args: &[String]) -> anyhow::Result<ExitCode> {
    print(command::sender_command(args).map(|line| vec![line]))
}

/// `containment-drill`.
pub async fn drill(dirs: HostDirs) -> anyhow::Result<ExitCode> {
    print(command::containment_drill(&env(dirs)?).await)
}

/// `nft-recreate`: deletes the helper's own empty table so the next request builds it again.
pub async fn recreate(dirs: HostDirs) -> anyhow::Result<ExitCode> {
    let path = std::path::Path::new("/etc/domovoi/helper.toml");
    let table = if path.exists() {
        domovoi_config::load_helper(path)
            .map(|(config, _)| config.nft.table)
            .map_err(|_| anyhow::anyhow!("helper.toml does not load"))?
    } else {
        domovoi_config::HelperConfig::default().nft.table
    };
    print(command::recreate(&env(dirs)?, &table).await)
}
