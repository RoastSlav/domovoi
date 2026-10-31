//! A host filesystem backed by a fixture directory.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use domovoi_core::ports::{FsError, HostFs};

/// A [`HostFs`] that maps host-absolute paths onto a fixture root:
/// `/proc/meminfo` reads `<root>/proc/meminfo`.
///
/// Paths that are not absolute or contain `..` return [`FsError::InvalidPath`], like a real
/// adapter. Reads are blocking, which is fine for tests.
#[derive(Debug, Clone)]
pub struct FixtureHostFs {
    root: PathBuf,
}

impl FixtureHostFs {
    /// A filesystem rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, FsError> {
        let rest = path.strip_prefix('/').ok_or(FsError::InvalidPath)?;
        let mut resolved = self.root.clone();
        for component in Path::new(rest).components() {
            match component {
                Component::Normal(part) => resolved.push(part),
                Component::CurDir => {}
                _ => return Err(FsError::InvalidPath),
            }
        }
        Ok(resolved)
    }
}

fn fs_error(error: &std::io::Error) -> FsError {
    match error.kind() {
        ErrorKind::NotFound => FsError::NotFound,
        ErrorKind::PermissionDenied => FsError::PermissionDenied,
        _ => FsError::Io(error.to_string()),
    }
}

#[async_trait]
impl HostFs for FixtureHostFs {
    async fn read_to_string(&self, path: &str) -> Result<String, FsError> {
        let resolved = self.resolve(path)?;
        std::fs::read_to_string(resolved).map_err(|e| fs_error(&e))
    }

    async fn read_dir(&self, path: &str) -> Result<Vec<String>, FsError> {
        let resolved = self.resolve(path)?;
        let entries = std::fs::read_dir(resolved).map_err(|e| fs_error(&e))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| fs_error(&e))?;
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    async fn exists(&self, path: &str) -> bool {
        self.resolve(path).is_ok_and(|p| p.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> FixtureHostFs {
        FixtureHostFs::new(concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/hostfs"))
    }

    #[tokio::test]
    async fn fixture_fs_maps_host_paths_under_the_root() {
        let fs = fixture();
        let meminfo = fs.read_to_string("/proc/meminfo").await.unwrap();
        assert!(meminfo.starts_with("MemTotal:"));
        assert_eq!(fs.read_dir("/proc").await.unwrap(), ["meminfo"]);
        assert!(fs.exists("/proc/meminfo").await);
        assert_eq!(
            fs.read_to_string("/proc/missing").await,
            Err(FsError::NotFound)
        );
    }

    #[tokio::test]
    async fn fixture_fs_rejects_relative_and_parent_paths() {
        let fs = fixture();
        for path in ["proc/meminfo", "/proc/../../Cargo.toml", ""] {
            assert_eq!(
                fs.read_to_string(path).await,
                Err(FsError::InvalidPath),
                "{path}"
            );
            assert!(!fs.exists(path).await);
        }
    }
}
