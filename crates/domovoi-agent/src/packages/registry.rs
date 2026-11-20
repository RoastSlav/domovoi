//! `.packages.toml`: which package versions are installed, and who signed them.
//!
//! The registry is the commit point of an install and of an uninstall. It is read fresh for
//! every operation and written through [`write_atomic_durable_with`], so a reader sees the old
//! or the new file and a power failure cannot leave a mix. An unreadable, unparsable or
//! future-format file is never guessed at: the caller fences the service.

use std::path::Path;

use domovoi_core::package::{
    ArchiveDigest, KeyId, MAX_INSTALLED_PACKAGES, PackageFailureCode, PackageName, PackageVersion,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::error::PackageError;
use super::layout::PRIVATE_FILE_MODE;
use crate::fs_atomic::{
    DurableFaults, DurableWriteError, read_optional, write_atomic_durable_with,
};

/// The only registry format this build reads or writes.
pub const REGISTRY_FORMAT: u32 = 1;

/// One installed package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    /// Package name.
    pub name: PackageName,
    /// The one installed version.
    pub version: PackageVersion,
    /// The key that signed the installed archive.
    pub key_id: KeyId,
    /// SHA-256 of the installed archive.
    pub archive_sha256: ArchiveDigest,
    /// When it was installed.
    pub installed_at: Timestamp,
    /// The execution that installed it.
    pub execution_id: Uuid,
    /// Names of its skills, sorted.
    pub skills: Vec<String>,
}

/// The registry file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    format: u32,
    #[serde(default, rename = "package")]
    packages: Vec<RegistryEntry>,
}

/// What a registry write did when it failed.
#[derive(Debug)]
pub enum WriteFailure {
    /// The old registry is authoritative; nothing was committed.
    BeforeCommit(PackageError),
    /// The rename happened but durability is unknown; re-read to decide.
    Indeterminate(PackageError),
}

/// The installed packages, sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    entries: Vec<RegistryEntry>,
}

fn corrupt(detail: impl Into<String>) -> PackageError {
    PackageError::new(PackageFailureCode::StoreCorrupt, detail)
}

impl Registry {
    /// Reads the registry at `path`; a missing file is an empty registry.
    ///
    /// # Errors
    /// `store_corrupt` when the file cannot be read or parsed, has another format, repeats a
    /// package, or lists more packages than allowed.
    pub fn read(path: &Path) -> Result<Self, PackageError> {
        let Some(bytes) =
            read_optional(path).map_err(|_| corrupt("the package registry cannot be read"))?
        else {
            return Ok(Self::default());
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| corrupt("the registry is not UTF-8"))?;
        let file: RegistryFile =
            toml::from_str(text).map_err(|_| corrupt("the package registry cannot be parsed"))?;
        if file.format != REGISTRY_FORMAT {
            return Err(corrupt("the package registry has an unsupported format"));
        }
        if file.packages.len() > MAX_INSTALLED_PACKAGES {
            return Err(corrupt("the package registry lists too many packages"));
        }
        let mut entries = file.packages;
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        if entries.windows(2).any(|pair| pair[0].name == pair[1].name) {
            return Err(corrupt("the package registry repeats a package"));
        }
        Ok(Self { entries })
    }

    /// Writes this registry durably (mode 0600).
    ///
    /// # Errors
    /// [`WriteFailure`] telling the caller whether the old file is still authoritative.
    pub fn write(&self, path: &Path, faults: DurableFaults) -> Result<(), WriteFailure> {
        let file = RegistryFile {
            format: REGISTRY_FORMAT,
            packages: self.entries.clone(),
        };
        let text = toml::to_string(&file).map_err(|_| {
            WriteFailure::BeforeCommit(PackageError::new(
                PackageFailureCode::Storage,
                "the registry cannot be encoded",
            ))
        })?;
        write_atomic_durable_with(path, text.as_bytes(), PRIVATE_FILE_MODE, faults).map_err(
            |error| match error {
                DurableWriteError::BeforeCommit(inner) => WriteFailure::BeforeCommit(
                    PackageError::new(PackageFailureCode::Storage, inner.to_string()),
                ),
                DurableWriteError::Indeterminate(inner) => WriteFailure::Indeterminate(
                    PackageError::new(PackageFailureCode::Storage, inner.to_string()),
                ),
            },
        )
    }

    /// The entry for `name`.
    #[must_use]
    pub fn get(&self, name: &PackageName) -> Option<&RegistryEntry> {
        self.entries.iter().find(|entry| &entry.name == name)
    }

    /// All entries, sorted by name.
    #[must_use]
    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    /// How many packages are installed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is installed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// A copy with `entry` added or replacing the entry of the same name.
    #[must_use]
    pub fn with(&self, entry: RegistryEntry) -> Self {
        let mut entries: Vec<RegistryEntry> = self
            .entries
            .iter()
            .filter(|existing| existing.name != entry.name)
            .cloned()
            .collect();
        entries.push(entry);
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Self { entries }
    }

    /// A copy without the entry for `name`.
    #[must_use]
    pub fn without(&self, name: &PackageName) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|existing| &existing.name != name)
                .cloned()
                .collect(),
        }
    }
}
