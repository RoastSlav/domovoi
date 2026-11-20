//! Where packages live on disk, whether that directory is safe to write, and the few
//! filesystem helpers every package module shares.
//!
//! Nothing in an archive decides a mode, an owner or a path: directories are made 0755, files
//! 0644 (0755 when the archive marks them owner-executable), and every path is built from a
//! validated name, version or execution id.

use std::path::{Component, Path, PathBuf};

use domovoi_core::package::PackageFailureCode;

use super::error::PackageError;
use super::{PackageDirs, SpaceProbe};

/// Mode of every directory the agent creates for a package.
pub const DIR_MODE: u32 = 0o755;
/// Mode of a regular file.
pub const FILE_MODE: u32 = 0o644;
/// Mode of a regular file the archive marks owner-executable.
pub const EXEC_MODE: u32 = 0o755;
/// Mode of the private staging area and the registry.
pub const PRIVATE_DIR_MODE: u32 = 0o700;
/// Mode of the registry file and staged transfer files.
pub const PRIVATE_FILE_MODE: u32 = 0o600;
/// Name of the registry file inside `packages_dir`.
pub const REGISTRY_FILE: &str = ".packages.toml";
/// Name of the disposable staging area inside `packages_dir`.
pub const STAGING_DIR: &str = ".staging";

/// Roots a package directory may never equal, contain or lie inside.
const PROTECTED_ROOTS: &[&str] = &[
    "/etc/domovoi",
    "/var/lib/domovoi-helper",
    "/run",
    "/proc",
    "/sys",
    "/dev",
    "/boot",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
];

/// The default protected roots as paths.
#[must_use]
pub fn default_protected_roots() -> Vec<PathBuf> {
    PROTECTED_ROOTS.iter().map(PathBuf::from).collect()
}

/// A `packages_dir_unsafe` failure.
fn unsafe_dir(detail: impl Into<String>) -> PackageError {
    PackageError::new(PackageFailureCode::PackagesDirUnsafe, detail)
}

/// Creates directory `path` (one level) and sets `mode` explicitly, never relying on umask.
///
/// # Errors
/// A `storage` failure.
pub fn make_dir(path: &Path, mode: u32) -> Result<(), PackageError> {
    std::fs::create_dir(path).map_err(|error| PackageError::io("creating a directory", &error))?;
    set_mode(path, mode)
}

/// Sets the permission bits of `path`.
///
/// # Errors
/// A `storage` failure.
pub fn set_mode(path: &Path, mode: u32) -> Result<(), PackageError> {
    set_mode_sys(path, mode).map_err(|error| PackageError::io("setting permissions", &error))
}

#[cfg(unix)]
fn set_mode_sys(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode_sys(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// Creates the `\<name\>` directory under `packages_dir` when missing (0755).
///
/// # Errors
/// A `storage` failure.
pub fn ensure_dir(path: &Path, mode: u32) -> Result<(), PackageError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(PackageError::new(
            PackageFailureCode::Storage,
            "a non-directory is in the way",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => make_dir(path, mode),
        Err(error) => Err(PackageError::io("inspecting a directory", &error)),
    }
}

/// The effective user id of this process (0 where the platform has none).
#[must_use]
pub fn effective_uid() -> u32 {
    euid_sys()
}

#[cfg(target_os = "linux")]
fn euid_sys() -> u32 {
    nix::unistd::geteuid().as_raw()
}

#[cfg(not(target_os = "linux"))]
fn euid_sys() -> u32 {
    0
}

/// `a` equals `b`, contains it, or lies inside it, comparing whole path components.
fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Best-effort canonical form: the real path when it exists, else the path as given.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Checks that `dirs.packages_dir` is safe to write: absolute, not `/`, a real directory (not
/// a symlink), owned by `uid`, not writable by group or others, and not overlapping any
/// protected root.
///
/// # Errors
/// `packages_dir_unsafe` with the reason.
pub fn check_packages_dir(dirs: &PackageDirs, uid: u32) -> Result<(), PackageError> {
    let dir = &dirs.packages_dir;
    if !dir.is_absolute() {
        return Err(unsafe_dir("the packages directory is not an absolute path"));
    }
    if dir
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count()
        == 0
    {
        return Err(unsafe_dir("the packages directory is the filesystem root"));
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|error| {
        unsafe_dir(format!(
            "cannot inspect the packages directory: {}",
            error.kind()
        ))
    })?;
    if meta.file_type().is_symlink() {
        return Err(unsafe_dir("the packages directory is a symbolic link"));
    }
    if !meta.is_dir() {
        return Err(unsafe_dir("the packages path is not a directory"));
    }
    check_owner_and_mode(&meta, uid)?;
    let real = canonical(dir);
    for root in dirs.protected_roots() {
        if overlaps(dir, &root) || overlaps(&real, &canonical(&root)) {
            return Err(unsafe_dir(
                "the packages directory overlaps a protected directory",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn check_owner_and_mode(meta: &std::fs::Metadata, uid: u32) -> Result<(), PackageError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if meta.uid() != uid {
        return Err(unsafe_dir(
            "the packages directory is not owned by the agent's user",
        ));
    }
    if meta.permissions().mode() & 0o022 != 0 {
        return Err(unsafe_dir(
            "the packages directory is writable by group or others",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_owner_and_mode(_meta: &std::fs::Metadata, _uid: u32) -> Result<(), PackageError> {
    Ok(())
}

/// Creates `packages_dir` (0755) when it is missing and its parent exists.
///
/// # Errors
/// `packages_dir_unsafe` when it cannot be created.
pub fn create_packages_dir(dir: &Path) -> Result<(), PackageError> {
    match std::fs::symlink_metadata(dir) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => make_dir(dir, DIR_MODE)
            .map_err(|_| unsafe_dir("the packages directory does not exist and cannot be created")),
        Err(error) => Err(unsafe_dir(format!(
            "cannot inspect the packages directory: {}",
            error.kind()
        ))),
    }
}

/// Real free-space probe (`statvfs`) on Linux.
#[derive(Debug, Default, Clone, Copy)]
pub struct StatvfsSpace;

impl SpaceProbe for StatvfsSpace {
    fn free_bytes(&self, path: &Path) -> std::io::Result<u64> {
        free_bytes_sys(path)
    }
}

#[cfg(target_os = "linux")]
fn free_bytes_sys(path: &Path) -> std::io::Result<u64> {
    let stat = nix::sys::statvfs::statvfs(path).map_err(std::io::Error::from)?;
    let blocks = stat.blocks_available();
    let size = stat.fragment_size();
    Ok(blocks.saturating_mul(size))
}

#[cfg(not(target_os = "linux"))]
fn free_bytes_sys(_path: &Path) -> std::io::Result<u64> {
    Err(std::io::Error::other("free space is not available here"))
}
