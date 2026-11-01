//! `domovoi version`.

use crate::ExitCode;

/// Prints the version, git commit, build date and target.
pub fn run() -> anyhow::Result<ExitCode> {
    println!("domovoi {}", env!("CARGO_PKG_VERSION"));
    println!("commit:  {}", env!("DOMOVOI_GIT_COMMIT"));
    println!("built:   {}", env!("DOMOVOI_BUILD_DATE"));
    println!("target:  {}", env!("DOMOVOI_TARGET"));
    Ok(ExitCode::SUCCESS)
}
