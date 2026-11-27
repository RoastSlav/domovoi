//! `host_binary`: is a program file or a package present on this host.
//!
//! Read-only and without a process: the standard program directories are looked at by name and
//! the package databases are read as files (dpkg's `info/<package>.list`, pacman's
//! `local/<package>-<version>`). The only negative this can give is "not found in these
//! places"; it never says a program is not installed, because a program can live anywhere.

use std::fmt::Write;

use domovoi_core::ports::HostFs;

/// Where a program file is looked for, in the order a login shell searches them.
pub const PROGRAM_DIRS: [&str; 6] = [
    "/usr/local/sbin",
    "/usr/local/bin",
    "/usr/sbin",
    "/usr/bin",
    "/sbin",
    "/bin",
];
const DPKG_INFO: &str = "/var/lib/dpkg/info";
const PACMAN_LOCAL: &str = "/var/lib/pacman/local";

/// What `host_binary` found, as text for the model.
pub async fn host_binary(fs: &dyn HostFs, name: &str, package: Option<&str>) -> String {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for dir in PROGRAM_DIRS {
        let path = format!("{dir}/{name}");
        if fs.exists(&path).await {
            found.push(path);
        } else {
            missing.push(dir);
        }
    }
    let mut out = String::new();
    if found.is_empty() {
        _ = writeln!(
            out,
            "file `{name}`: not found in {}. This does not prove it is not installed: it may live elsewhere, and this tool does not search the whole disk.",
            PROGRAM_DIRS.join(", ")
        );
    } else {
        _ = writeln!(out, "file `{name}`: found at {}.", found.join(", "));
        if !missing.is_empty() {
            _ = writeln!(out, "not found in: {}.", missing.join(", "));
        }
        out.push_str("This only says a file of that name exists; it does not say it can be run (mode and owner are not read).\n");
    }
    out.push_str(&owner_line(fs, &found).await);
    if let Some(package) = package {
        out.push_str(&package_line(fs, package).await);
    }
    out
}

/// Most package file lists read for one owner lookup.
const MAX_LISTS: usize = 8000;

/// The paths a package database may know a found file under: the file itself and its
/// usr-merge twin (`/bin/x` and `/usr/bin/x` are the same file on a merged system).
fn spellings(found: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for path in found {
        out.push(path.clone());
        match path.strip_prefix("/usr") {
            Some(rest) => out.push(rest.to_owned()),
            None => out.push(format!("/usr{path}")),
        }
    }
    out
}

/// Which package owns the found file, from the package file lists, or an explicit statement
/// that this was not found or not checked. Never a guess from the program's name.
async fn owner_line(fs: &dyn HostFs, found: &[String]) -> String {
    if found.is_empty() {
        return "owner package: not looked up, no file was found.\n".to_owned();
    }
    let wanted = spellings(found);
    let mut checked = Vec::new();
    if let Ok(entries) = fs.read_dir(DPKG_INFO).await {
        checked.push("dpkg");
        for entry in entries
            .iter()
            .filter(|e| e.ends_with(".list"))
            .take(MAX_LISTS)
        {
            let Ok(text) = fs.read_to_string(&format!("{DPKG_INFO}/{entry}")).await else {
                continue;
            };
            if let Some(hit) = text.lines().find(|line| wanted.iter().any(|w| w == line)) {
                let package = entry.trim_end_matches(".list");
                let package = package.split(':').next().unwrap_or(package);
                return format!("owner package: `{package}` (dpkg lists {hit}).\n");
            }
        }
    }
    if let Ok(entries) = fs.read_dir(PACMAN_LOCAL).await {
        checked.push("pacman");
        let relative: Vec<&str> = wanted.iter().map(|w| w.trim_start_matches('/')).collect();
        for entry in entries.iter().take(MAX_LISTS) {
            let Ok(text) = fs
                .read_to_string(&format!("{PACMAN_LOCAL}/{entry}/files"))
                .await
            else {
                continue;
            };
            if let Some(hit) = text.lines().find(|line| relative.contains(line)) {
                return format!("owner package: `{entry}` (pacman lists /{hit}).\n");
            }
        }
    }
    if checked.is_empty() {
        return "owner package: unknown (not checked): neither a dpkg nor a pacman database is readable here (rpm and other managers are not read). Do not say whether a package is installed.\n".to_owned();
    }
    format!(
        "owner package: no {} package file list names this file. That does not prove it is unowned (it may be built locally or the database may be incomplete); do not claim a package is missing.\n",
        checked.join(" or ")
    )
}

async fn package_line(fs: &dyn HostFs, package: &str) -> String {
    let mut checked = Vec::new();
    if let Ok(entries) = fs.read_dir(DPKG_INFO).await {
        checked.push("dpkg");
        let plain = format!("{package}.list");
        let arch = format!("{package}:");
        if entries
            .iter()
            .any(|e| *e == plain || (e.starts_with(&arch) && e.ends_with(".list")))
        {
            return format!(
                "package `{package}`: recorded as installed by dpkg (its file list is present).\n"
            );
        }
    }
    if let Ok(entries) = fs.read_dir(PACMAN_LOCAL).await {
        checked.push("pacman");
        let prefix = format!("{package}-");
        if let Some(version) = entries.iter().find_map(|e| {
            let rest = e.strip_prefix(&prefix)?;
            rest.starts_with(|c: char| c.is_ascii_digit())
                .then(|| rest.to_owned())
        }) {
            return format!(
                "package `{package}`: recorded as installed by pacman, version {version}.\n"
            );
        }
    }
    if checked.is_empty() {
        return format!(
            "package `{package}`: not checked; neither a dpkg nor a pacman database is readable here (rpm and other managers are not read).\n"
        );
    }
    format!(
        "package `{package}`: not recorded by {}. A package may have another name than the program.\n",
        checked.join(" or ")
    )
}

#[cfg(test)]
mod tests {
    use domovoi_testkit::FixtureHostFs;

    use super::host_binary;

    fn world() -> (tempfile::TempDir, FixtureHostFs) {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "usr/sbin/ip",
            "usr/bin/ls",
            "var/lib/dpkg/info/iproute2.list",
        ] {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, "").unwrap();
        }
        std::fs::write(
            dir.path().join("var/lib/dpkg/info/iproute2.list"),
            "/usr/sbin/ip\n",
        )
        .unwrap();
        let fs = FixtureHostFs::new(dir.path());
        (dir, fs)
    }

    #[tokio::test]
    async fn a_program_in_a_standard_directory_is_found_and_its_package_is_read() {
        let (_dir, fs) = world();
        let text = host_binary(&fs, "ip", Some("iproute2")).await;
        assert!(text.contains("found at /usr/sbin/ip"), "{text}");
        assert!(text.contains("recorded as installed by dpkg"), "{text}");
        assert!(text.contains("owner package: `iproute2`"), "{text}");
        assert!(text.contains("mode and owner are not read"), "{text}");
    }

    #[tokio::test]
    async fn a_missing_program_is_not_found_in_these_places_never_not_installed() {
        let (_dir, fs) = world();
        let text = host_binary(&fs, "nginx", None).await;
        assert!(text.contains("not found in /usr/local/sbin"), "{text}");
        assert!(
            text.contains("does not prove it is not installed"),
            "{text}"
        );
        assert!(!text.contains("not recorded by"), "{text}");
        assert!(text.contains("not looked up"), "{text}");
    }

    #[tokio::test]
    async fn the_owner_is_found_without_the_caller_naming_the_package() {
        // `ip` is owned by `iproute2`; a lookup by the program name must not say `ip` is
        // "not recorded" (the real-host report).
        let (_dir, fs) = world();
        let full = _dir.path().join("var/lib/dpkg/info/iproute2.list");
        std::fs::write(full, "/.\n/bin/ip\n/usr/sbin/ip\n").unwrap();
        let text = host_binary(&fs, "ip", None).await;
        assert!(text.contains("owner package: `iproute2`"), "{text}");
        assert!(!text.contains("not recorded"), "{text}");
    }

    #[tokio::test]
    async fn a_file_no_list_names_says_so_without_claiming_a_missing_package() {
        let (_dir, fs) = world();
        let text = host_binary(&fs, "ls", None).await;
        assert!(text.contains("no dpkg package file list names"), "{text}");
        assert!(text.contains("do not claim a package is missing"), "{text}");
    }

    #[tokio::test]
    async fn without_a_package_database_the_package_is_not_checked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("usr/bin")).unwrap();
        std::fs::write(dir.path().join("usr/bin/ip"), "").unwrap();
        let fs = FixtureHostFs::new(dir.path());
        let text = host_binary(&fs, "ip", None).await;
        assert!(text.contains("unknown (not checked)"), "{text}");
    }

    #[tokio::test]
    async fn pacman_packages_match_by_name_and_version_only() {
        let dir = tempfile::tempdir().unwrap();
        for entry in ["iproute2-6.1-1", "iproute2-extra-1.0-1"] {
            std::fs::create_dir_all(dir.path().join("var/lib/pacman/local").join(entry)).unwrap();
        }
        let fs = FixtureHostFs::new(dir.path());
        let text = host_binary(&fs, "ip", Some("iproute2")).await;
        assert!(text.contains("pacman, version 6.1-1"), "{text}");
        let other = host_binary(&fs, "x", Some("iproute")).await;
        assert!(other.contains("not recorded by pacman"), "{other}");
    }

    #[tokio::test]
    async fn a_path_in_the_name_is_never_followed() {
        // The tool call parser refuses such a name; the lookup does not rely on it alone.
        let (_dir, fs) = world();
        let text = host_binary(&fs, "../etc/passwd", None).await;
        assert!(text.contains("not found"), "{text}");
    }
}
