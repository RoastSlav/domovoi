//! Naming the files a restore sets aside, and tracking every move so it can be undone.

use std::path::{Path, PathBuf};

use super::RestoreError;

/// How many collision suffixes an artifact name tries before the restore gives up.
const NAMES_TRIED: u32 = 16;

/// `path` with `tail` appended, as SQLite names its side files.
pub(super) fn suffix(path: &Path, tail: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(tail);
    PathBuf::from(name)
}

/// Every path SQLite keeps for `database`: the file, its write-ahead log and its shared
/// memory index.
pub(super) fn companions(database: &Path) -> [(PathBuf, &'static str); 3] {
    [
        (database.to_path_buf(), ""),
        (suffix(database, "-wal"), "-wal"),
        (suffix(database, "-shm"), "-shm"),
    ]
}

/// Whether any SQLite sidecar of `database` exists.
pub(super) fn sidecars_present(database: &Path) -> bool {
    ["-wal", "-shm"]
        .iter()
        .any(|tail| suffix(database, tail).exists())
}

/// The same file, judged by canonical path where both exist.
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `base`, then `base-2`, `base-3`, ... for each attempt.
fn numbered(base: &Path, attempt: u32) -> PathBuf {
    if attempt == 0 {
        base.to_path_buf()
    } else {
        suffix(base, &format!("-{}", attempt + 1))
    }
}

/// Claims a fresh name, never one that already exists.
///
/// The placeholder is created atomically with `create_new`, so two runs given the same
/// timestamp cannot take the same name and an earlier artifact is never replaced. A name
/// whose `-wal` or `-shm` is already there is skipped too: an interrupted restore may have
/// left those behind. The caller holds the data-directory lock, so nothing competes for
/// the name afterwards, and the rename that fills it replaces only this run's empty
/// placeholder.
pub(super) fn reserve(base: &Path) -> Result<PathBuf, RestoreError> {
    for attempt in 0..NAMES_TRIED {
        let candidate = numbered(base, attempt);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => {
                if sidecars_present(&candidate) {
                    let _ = std::fs::remove_file(&candidate);
                    continue;
                }
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(RestoreError::Io {
                    path: candidate,
                    reason: err.to_string(),
                });
            }
        }
    }
    Err(RestoreError::Io {
        path: base.to_owned(),
        reason: format!("no free name after {NAMES_TRIED} attempts; move the old ones away"),
    })
}

/// Removes `path` when it is the empty placeholder a reservation made. A file holding
/// anything is never removed.
pub(super) fn remove_if_empty(path: &Path) {
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() == 0) {
        let _ = std::fs::remove_file(path);
    }
}

/// Files moved during a restore, so a failure can put each one back exactly.
#[derive(Default)]
pub(super) struct Moves {
    /// `(from, to)` for every move that succeeded, oldest first.
    done: Vec<(PathBuf, PathBuf)>,
}

impl Moves {
    /// Moves `from` to `to` and records it.
    pub(super) fn perform(&mut self, from: &Path, to: &Path) -> Result<(), RestoreError> {
        std::fs::rename(from, to).map_err(|err| RestoreError::Io {
            path: from.to_owned(),
            reason: err.to_string(),
        })?;
        self.done.push((from.to_owned(), to.to_owned()));
        Ok(())
    }

    /// Moves the SQLite sidecars of `from` to those of `to`, where they exist.
    pub(super) fn perform_sidecars(&mut self, from: &Path, to: &Path) -> Result<(), RestoreError> {
        for tail in ["-wal", "-shm"] {
            let source = suffix(from, tail);
            if source.exists() {
                self.perform(&source, &suffix(to, tail))?;
            }
        }
        Ok(())
    }

    /// Whether `to` is where a move put something.
    pub(super) fn moved_to(&self, to: &Path) -> bool {
        self.done.iter().any(|(_, destination)| destination == to)
    }

    /// Reverses every move, newest first. Returns one line per move it could not undo.
    ///
    /// `fail_first` makes the first rename fail, which is how the tests reach a rollback
    /// that cannot finish without depending on file permissions.
    pub(super) fn undo(&mut self, fail_first: bool) -> Vec<String> {
        let mut stuck = Vec::new();
        let mut first = fail_first;
        while let Some((from, to)) = self.done.pop() {
            let failed = if first {
                first = false;
                Err(std::io::Error::other("rolled back: simulated failure"))
            } else {
                std::fs::rename(&to, &from)
            };
            if let Err(err) = failed {
                stuck.push(format!(
                    "{} back to {}: {err}",
                    to.display(),
                    from.display()
                ));
            }
        }
        stuck
    }
}
