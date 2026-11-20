//! The per-entry rules of a package archive: type, path, depth, size and duplicates.
//!
//! Shared by the audit pass and the extraction pass so both accept exactly the same entries.

use std::collections::BTreeSet;

use domovoi_config::PackageLimits;
use domovoi_core::package::{PackageFailureCode, PackageName};
use tar::EntryType;

use super::error::{PackageError, quote_bytes};

fn fail(code: PackageFailureCode, detail: impl Into<String>) -> PackageError {
    PackageError::new(code, detail)
}

/// What an accepted entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A directory.
    Dir,
    /// A regular file of `size` bytes; `executable` when the archive marks it owner-executable.
    File {
        /// Declared content length.
        size: u64,
        /// Whether the owner-execute bit is set in the archive.
        executable: bool,
    },
}

/// An entry that passed [`check_entry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// The path relative to the package directory, `/`-separated; empty for the package
    /// directory itself. Every part is already validated, so it is safe to join to a root.
    pub rel: String,
    /// Directory or file.
    pub kind: EntryKind,
}

/// The paths seen so far, to reject duplicates and file/directory clashes.
#[derive(Debug, Default)]
pub struct SeenPaths {
    explicit: BTreeSet<String>,
    files: BTreeSet<String>,
    dirs: BTreeSet<String>,
}

impl SeenPaths {
    fn record(&mut self, path: &str, is_dir: bool) -> Result<(), PackageError> {
        let key = path.to_ascii_lowercase();
        let clash = |detail: &str| fail(PackageFailureCode::DuplicatePath, detail);
        if !self.explicit.insert(key.clone()) {
            return Err(clash("a path appears twice (case-insensitively)"));
        }
        let mut prefix = String::new();
        let parts: Vec<&str> = key.split('/').collect();
        for (index, part) in parts.iter().enumerate() {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let last = index + 1 == parts.len();
            if !last && self.files.contains(&prefix) {
                return Err(clash("a path lies under a file"));
            }
            if last && is_dir && self.files.contains(&prefix) {
                return Err(clash("a directory has the name of a file"));
            }
            if last && !is_dir && self.dirs.contains(&prefix) {
                return Err(clash("a file has the name of a directory"));
            }
            if !last || is_dir {
                self.dirs.insert(prefix.clone());
            }
        }
        if !is_dir {
            self.files.insert(key);
        }
        Ok(())
    }
}

/// Checks one entry's type, path, depth and size and records it in `seen`.
///
/// `raw_path` is the entry's path bytes exactly as stored. `mode` is the archive's mode;
/// only its owner-execute bit is used.
///
/// # Errors
/// The `link_entry`, `special_entry`, `path_invalid`, `path_escape`, `limit_path_length`,
/// `limit_depth`, `limit_file_size` and `duplicate_path` codes.
pub fn check_entry(
    name: &PackageName,
    limits: &PackageLimits,
    kind: EntryType,
    raw_path: &[u8],
    size: u64,
    mode: u32,
    seen: &mut SeenPaths,
) -> Result<EntryInfo, PackageError> {
    let is_dir = match kind {
        EntryType::Regular => false,
        EntryType::Directory => true,
        EntryType::Symlink | EntryType::Link => {
            return Err(fail(
                PackageFailureCode::LinkEntry,
                format!("a link entry {} is not allowed", quote_bytes(raw_path)),
            ));
        }
        other => {
            return Err(fail(
                PackageFailureCode::SpecialEntry,
                format!("an entry of type {other:?} is not allowed"),
            ));
        }
    };
    let invalid = |detail: &str| {
        fail(
            PackageFailureCode::PathInvalid,
            format!("{detail}: {}", quote_bytes(raw_path)),
        )
    };
    let escape = || {
        fail(
            PackageFailureCode::PathEscape,
            format!(
                "path leaves the package directory: {}",
                quote_bytes(raw_path)
            ),
        )
    };
    let Ok(text) = std::str::from_utf8(raw_path) else {
        return Err(invalid("the path is not UTF-8"));
    };
    if text.contains('\0') || text.contains('\\') {
        return Err(invalid("the path has a NUL or a backslash"));
    }
    if raw_path.len() > usize::try_from(limits.path_bytes).unwrap_or(usize::MAX) {
        return Err(fail(
            PackageFailureCode::LimitPathLength,
            format!("a path is longer than {} bytes", limits.path_bytes),
        ));
    }
    let trimmed = text.strip_suffix('/').unwrap_or(text);
    if !is_dir && trimmed.len() != text.len() {
        return Err(invalid("a file path ends with a slash"));
    }
    if trimmed.starts_with('/') {
        return Err(escape());
    }
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.contains(&"..") || parts.first().copied() != Some(name.as_str()) {
        return Err(escape());
    }
    for part in &parts {
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-');
        if part.is_empty() || part.starts_with('.') || !part.chars().all(allowed) {
            return Err(invalid(
                "a path part is empty, hidden or has other characters",
            ));
        }
    }
    if parts.len() > usize::try_from(limits.path_depth).unwrap_or(usize::MAX) {
        return Err(fail(
            PackageFailureCode::LimitDepth,
            format!("a path is deeper than {} parts", limits.path_depth),
        ));
    }
    if !is_dir && size > limits.file_bytes {
        return Err(fail(
            PackageFailureCode::LimitFileSize,
            format!("a file is larger than {} bytes", limits.file_bytes),
        ));
    }
    seen.record(trimmed, is_dir)?;
    let rel = parts.get(1..).unwrap_or_default().join("/");
    let kind = if is_dir {
        EntryKind::Dir
    } else {
        EntryKind::File {
            size,
            executable: mode & 0o100 != 0,
        }
    };
    Ok(EntryInfo { rel, kind })
}
