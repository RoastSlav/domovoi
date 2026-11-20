//! The extraction pass: writes an audited archive into a private tree.
//!
//! Entries are never unpacked by the `tar` crate. Each one is checked again by the shared
//! walker, then written here with an explicit mode, so the archive cannot choose an owner, a
//! mode, a link or a path.

use std::io::Write;
use std::path::{Path, PathBuf};

use domovoi_config::PackageLimits;
use domovoi_core::package::{ArchiveDigest, PackageName};
use domovoi_core::ports::Clock;

use super::archive::walk;
use super::entry::EntryKind;
use super::error::PackageError;
use super::layout::{DIR_MODE, EXEC_MODE, FILE_MODE, make_dir, set_mode};
use crate::fs_atomic::sync_dir;

/// Extracts the archive at `archive` into the new directory `tree`.
///
/// # Errors
/// Any archive, limit or digest code, or `storage`.
pub fn run(
    archive: &Path,
    name: &PackageName,
    limits: &PackageLimits,
    clock: &dyn Clock,
    expected: &ArchiveDigest,
    tree: &Path,
) -> Result<(), PackageError> {
    let file = std::fs::File::open(archive)
        .map_err(|error| PackageError::io("opening the archive", &error))?;
    make_dir(tree, DIR_MODE)?;
    let mut made: Vec<PathBuf> = vec![tree.to_path_buf()];
    walk(file, name, limits, clock, expected, &mut |info, content| {
        if info.rel.is_empty() {
            return Ok(());
        }
        let target = tree.join(&info.rel);
        match info.kind {
            EntryKind::Dir => ensure_chain(tree, &info.rel, &mut made),
            EntryKind::File { executable, .. } => {
                if let Some(parent) = Path::new(&info.rel).parent() {
                    let parent = parent.to_string_lossy();
                    if !parent.is_empty() {
                        ensure_chain(tree, &parent, &mut made)?;
                    }
                }
                let mut out = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|error| PackageError::io("creating a file", &error))?;
                std::io::copy(content, &mut out)
                    .map_err(|error| PackageError::io("writing a file", &error))?;
                out.flush()
                    .and_then(|()| out.sync_all())
                    .map_err(|error| PackageError::io("syncing a file", &error))?;
                drop(out);
                set_mode(&target, if executable { EXEC_MODE } else { FILE_MODE })
            }
        }
    })?;
    for dir in made.iter().rev() {
        sync_dir(dir).map_err(|error| PackageError::io("syncing a directory", &error))?;
    }
    Ok(())
}

/// Creates every missing directory of `rel` under `tree` with mode 0755.
fn ensure_chain(tree: &Path, rel: &str, made: &mut Vec<PathBuf>) -> Result<(), PackageError> {
    let mut current = tree.to_path_buf();
    for part in rel.split('/') {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(PackageError::new(
                    domovoi_core::package::PackageFailureCode::DuplicatePath,
                    "a file and a directory share a path",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                make_dir(&current, DIR_MODE)?;
                made.push(current.clone());
            }
            Err(error) => return Err(PackageError::io("inspecting a directory", &error)),
        }
    }
    Ok(())
}
