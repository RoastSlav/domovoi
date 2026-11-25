//! The read-only ports the unit identity algorithm needs. The helper implements them.

use std::collections::BTreeSet;

use async_trait::async_trait;

use super::name::{UnitId, UnitName};
use super::props::{LoadedProps, ProcessView, UnitProps, UnitRuntime};

/// Why the manager could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the manager could not be read: {0}")]
pub struct ViewError(pub String);

/// Why the disk could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the disk could not be read: {0}")]
pub struct FsError(pub String);

/// What kind of file a path is, by `lstat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A regular file.
    Regular,
    /// A symbolic link.
    Symlink,
    /// Anything else.
    Other,
}

/// Reads the service manager over its bus. Never `GetUnit` (it fails for an unloaded unit)
/// and never `systemctl show` text.
#[async_trait]
pub trait SystemdView: Send + Sync {
    /// `LoadUnit`, then `GetAll` on the Unit interface; arrays arrive as arrays.
    async fn load_unit(&self, name: &UnitName) -> Result<UnitProps, ViewError>;
    /// Service-interface properties of the loaded unit, runtime fields already removed.
    async fn service_props(&self, id: &UnitId) -> Result<LoadedProps, ViewError>;
    /// The runtime facts: invocation, main pid, state, result, restart count.
    async fn runtime(&self, id: &UnitId) -> Result<UnitRuntime, ViewError>;
    /// What a started process runs.
    async fn process_view(&self, pid: u32) -> Result<ProcessView, ViewError>;
}

/// Reads unit files from disk. Synchronous: reads are small and local.
pub trait UnitFs: Send + Sync {
    /// The kind of file at `path`, without following a link.
    fn lstat(&self, path: &str) -> Result<FileKind, FsError>;
    /// The bytes of `path`: one open, one read.
    fn read_once(&self, path: &str) -> Result<Vec<u8>, FsError>;
    /// The drop-in paths systemd would load for this unit over every search directory,
    /// existing now.
    fn enumerate_dropins(
        &self,
        id: &UnitId,
        names: &BTreeSet<String>,
    ) -> Result<Vec<String>, FsError>;
}
