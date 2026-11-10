//! Swapping the running executable and finding the services to restart.

use std::path::Path;

use anyhow::Context;

/// The service units `install.sh` creates, hub first, which are present in `unit_dir`.
pub fn installed_units(unit_dir: &Path) -> Vec<&'static str> {
    ["domovoi-hub", "domovoi-agent"]
        .into_iter()
        .filter(|unit| unit_dir.join(format!("{unit}.service")).exists())
        .collect()
}

/// Writes `binary` next to `exe` as `domovoi.new` with mode 755, then renames it over
/// `exe`. The rename is atomic, so `exe` is either the old file or the new one, and a
/// process already running the old one is not disturbed. On any failure the old
/// executable is untouched and the temporary file is removed.
pub fn replace(exe: &Path, binary: &[u8]) -> anyhow::Result<()> {
    let staged = exe.with_file_name("domovoi.new");
    let result = stage(&staged, binary).and_then(|()| {
        std::fs::rename(&staged, exe)
            .with_context(|| format!("cannot rename onto {}", exe.display()))
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result
}

fn stage(staged: &Path, binary: &[u8]) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(staged);
    std::fs::write(staged, binary).with_context(|| format!("cannot write {}", staged.display()))?;
    make_executable(staged)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .with_context(|| format!("cannot make {} executable", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{installed_units, replace};

    #[test]
    fn replaces_the_binary_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("domovoi");
        std::fs::write(&exe, b"old").unwrap();
        replace(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert!(!dir.path().join("domovoi.new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn a_failed_swap_keeps_the_old_binary() {
        let dir = tempfile::tempdir().unwrap();
        // A directory cannot be renamed over by a file, so the swap fails after staging.
        let exe = dir.path().join("domovoi");
        std::fs::create_dir(&exe).unwrap();
        std::fs::write(exe.join("keep"), b"old").unwrap();
        assert!(replace(&exe, b"new").is_err());
        assert_eq!(std::fs::read(exe.join("keep")).unwrap(), b"old");
        assert!(!dir.path().join("domovoi.new").exists());
    }

    #[test]
    fn lists_only_the_units_that_exist_hub_first() {
        let dir = tempfile::tempdir().unwrap();
        assert!(installed_units(dir.path()).is_empty());
        std::fs::write(dir.path().join("domovoi-agent.service"), b"").unwrap();
        assert_eq!(installed_units(dir.path()), ["domovoi-agent"]);
        std::fs::write(dir.path().join("domovoi-hub.service"), b"").unwrap();
        assert_eq!(
            installed_units(dir.path()),
            ["domovoi-hub", "domovoi-agent"]
        );
    }
}
