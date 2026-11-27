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
    for line in restart_hint_lines(unit_dir) {
        println!("{line}");
    }
}

/// What `domovoi upgrade` tells the operator after the swap: what to restart, and that unit
/// files were not touched (a release may change them; only the installer replaces them).
fn restart_hint_lines(unit_dir: &Path) -> Vec<String> {
    let units = install::installed_units(unit_dir);
    let mut lines = Vec::new();
    if units.is_empty() {
        lines.push(format!(
            "No Domovoi service unit was found in {}.",
            unit_dir.display()
        ));
        lines.push("Restart domovoi yourself if it is running.".to_owned());
        return lines;
    }
    lines.push("Restart what runs on this machine, the hub before any agent:".to_owned());
    for unit in units {
        lines.push(format!("  sudo systemctl restart {unit}"));
    }
    let all = install::all_unit_files(unit_dir);
    lines.push(String::new());
    lines.push(format!(
        "Unit files were not changed (found: {}).",
        all.join(", ")
    ));
    lines.push(
        "If this release changes them, run the installer again (sudo sh install.sh --role <role>); it replaces a differing unit, saves the old text and prints what changed. Until then `domovoi agent selftest` reports `unit_modified` when a unit is out of date."
            .to_owned(),
    );
    lines
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

    #[test]
    fn the_hint_says_unit_files_were_not_changed_and_lists_all_units() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "domovoi-agent.service",
            "domovoi-helper@.service",
            "domovoi-skill-runner@.service",
            "other.service",
        ] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let text = super::restart_hint_lines(dir.path()).join("\n");
        assert!(text.contains("sudo systemctl restart domovoi-agent"), "{text}");
        assert!(text.contains("Unit files were not changed"), "{text}");
        assert!(text.contains("domovoi-helper@.service"), "{text}");
        assert!(text.contains("domovoi-skill-runner@.service"), "{text}");
        assert!(!text.contains("other.service"), "{text}");
        assert!(text.contains("install.sh --role"), "{text}");
    }
}
