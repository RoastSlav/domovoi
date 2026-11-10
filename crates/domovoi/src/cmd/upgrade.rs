//! `domovoi upgrade`: replaces this binary with a release's, then says what to restart.
//!
//! It downloads the release archive for this machine, checks its SHA-256, and swaps the
//! executable in place. It never restarts a service: Domovoi starts no process outside the
//! runner, so the commands that restart the hub or agent are printed for the operator.

mod fetch;
mod install;
mod package;
mod target;

use std::path::Path;

use anyhow::Context;

use crate::ExitCode;

/// Where systemd unit files installed by `install.sh` live.
const UNIT_DIR: &str = "/etc/systemd/system";

/// Arguments of `domovoi upgrade`.
#[derive(Debug, clap::Args)]
pub struct UpgradeArgs {
    /// The release to install, such as `v0.9.9`. Default: the latest release.
    #[arg(long)]
    version: Option<String>,
    /// Print what would be downloaded and replaced, and change nothing.
    #[arg(long)]
    dry_run: bool,
}

/// Runs the upgrade.
pub async fn run(args: UpgradeArgs) -> anyhow::Result<ExitCode> {
    let target = target::for_this_machine()?;
    let exe = std::env::current_exe().context("cannot tell where this binary is installed")?;
    let base = fetch::release_base();
    let client = fetch::client()?;
    let tag = match &args.version {
        Some(version) => normalize_tag(version)?,
        None => fetch::latest_tag(&client, &base).await?,
    };
    let name = format!("domovoi-{tag}-{target}");
    let archive_url = format!("{base}/download/{tag}/{name}.tar.gz");
    let checksum_url = format!("{archive_url}.sha256");

    println!(
        "Installed:  {} ({})",
        env!("CARGO_PKG_VERSION"),
        exe.display()
    );
    println!("Release:    {tag} for {target}");
    if args.dry_run {
        println!("Dry run. Would download:");
        println!("  {archive_url}");
        println!("  {checksum_url}");
        println!("and replace {}. Nothing was changed.", exe.display());
        return Ok(ExitCode::SUCCESS);
    }

    println!("Downloading {name}.tar.gz");
    let archive = fetch::download(&client, &archive_url, fetch::MAX_ARCHIVE).await?;
    let checksum = fetch::download(&client, &checksum_url, fetch::MAX_CHECKSUM).await?;
    let checksum = String::from_utf8(checksum).context("the checksum file is not text")?;
    package::verify_checksum(&archive, &checksum, &format!("{name}.tar.gz"))?;
    println!("Checksum verified.");
    let binary = package::extract_binary(&archive, &name)?;
    install::replace(&exe, &binary).with_context(|| {
        format!(
            "could not replace {}; run this command with sudo",
            exe.display()
        )
    })?;
    println!("Replaced {} with release {tag}.", exe.display());
    print_restart_hint(Path::new(UNIT_DIR));
    Ok(ExitCode::SUCCESS)
}

/// `v0.9.9` for `0.9.9` or `v0.9.9`; anything that is not a plain release tag is refused,
/// because the tag goes into a download URL.
fn normalize_tag(version: &str) -> anyhow::Result<String> {
    let tag = if version.starts_with('v') {
        version.to_owned()
    } else {
        format!("v{version}")
    };
    anyhow::ensure!(
        is_release_tag(&tag),
        "{version:?} is not a release version such as v0.9.9"
    );
    Ok(tag)
}

/// Whether `tag` is a plain release tag such as `v0.9.9` or `v0.9.9-rc.1`: three numbers
/// after a `v`, then optionally `-` or `+` and letters, digits, `.`, `-` and `+`. Nothing
/// else is allowed, because the tag is written into a download URL.
fn is_release_tag(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix('v') else {
        return false;
    };
    let (numbers, suffix) = match rest.find(['-', '+']) {
        Some(at) => (&rest[..at], &rest[at + 1..]),
        None => (rest, "x"),
    };
    let parts: Vec<&str> = numbers.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        && !suffix.is_empty()
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

fn print_restart_hint(unit_dir: &Path) {
    let units = install::installed_units(unit_dir);
    if units.is_empty() {
        println!(
            "No Domovoi service unit was found in {}.",
            unit_dir.display()
        );
        println!("Restart domovoi yourself if it is running.");
        return;
    }
    println!("Restart what runs on this machine, the hub before any agent:");
    for unit in units {
        println!("  sudo systemctl restart {unit}");
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_tag;

    #[test]
    fn a_version_without_v_gets_one() {
        assert_eq!(normalize_tag("0.9.9").unwrap(), "v0.9.9");
        assert_eq!(normalize_tag("v0.9.9").unwrap(), "v0.9.9");
    }

    #[test]
    fn a_version_that_could_change_the_url_is_refused() {
        for bad in ["v0.9.9/../x", "v0.9.9?x=1", "latest", "v0.9.9;id", ""] {
            assert!(normalize_tag(bad).is_err(), "{bad:?} must be refused");
        }
    }
}
