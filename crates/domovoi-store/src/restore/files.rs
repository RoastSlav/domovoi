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
/// the name afterwards.
pub(super) fn reserve(base: &Path) -> Result<Reservation, RestoreError> {
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
                return Ok(Reservation {
                    path: candidate,
                    own: true,
                });
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

/// A name claimed by [`reserve`], and whether what sits there is still this run's own file.
///
/// The placeholder is empty when it is created, and only this run writes to the name until
/// a move fills it: `reserve` refuses a name that already exists. `own` is cleared the
/// moment a real file is renamed over the placeholder, so cleanup never has to guess from
/// a file's size — a real database may be zero bytes, and deleting it would destroy the
/// only copy of what the operator had.
pub(super) struct Reservation {
    path: PathBuf,
    /// True while `path` holds only what this run put there: an empty placeholder or a
    /// partial copy. Never true once a moved file has been renamed over it.
    own: bool,
}

impl Reservation {
    /// The reserved path.
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// The reserved path, owned.
    pub(super) fn to_path_buf(&self) -> PathBuf {
        self.path.clone()
    }

    /// Records that a file was renamed over the placeholder. From now on the path holds
    /// someone else's data and is never removed as a placeholder.
    pub(super) fn filled(&mut self) {
        self.own = false;
    }

    /// Removes the reserved name, but only while it holds nothing but this run's own
    /// placeholder or partial copy.
    pub(super) fn abandon(&mut self) {
        if self.own {
            let _ = std::fs::remove_file(&self.path);
            self.own = false;
        }
    }

    /// [`Self::abandon`], and the SQLite sidecars this run's own copy may have left.
    pub(super) fn discard(&mut self) {
        if self.own {
            for tail in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(suffix(&self.path, tail));
            }
            self.own = false;
        }
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

    /// Whether a move that this journal recorded has since been undone.
    ///
    /// `undo` pops the entries it reverts, so anything still listed here that is no longer
    /// at its destination was moved back by a rollback that could not finish.
    pub(super) fn still_moved_to(&self, to: &Path) -> bool {
        self.moved_to(to) && to.exists()
    }

    /// Reverses every move, newest first. Returns one line per move it could not undo.
    ///
    /// `fail_first` makes the first rename fail, which is how the tests reach a rollback
    /// that cannot finish without depending on file permissions.
    pub(super) fn undo(&mut self, fail_first: bool) -> Vec<String> {
        self.undo_with(fail_first, false)
    }

    /// [`Self::undo`], with the choice of which move cannot go back:
    /// `fail_first` gives up on the newest, `fail_sidecar` on the newest sidecar.
    pub(super) fn undo_with(&mut self, fail_first: bool, fail_sidecar: bool) -> Vec<String> {
        let mut stuck = Vec::new();
        let mut first = fail_first;
        let mut sidecar = fail_sidecar;
        while let Some((from, to)) = self.done.pop() {
            let is_sidecar = to
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("-wal") || name.ends_with("-shm"));
            let failed = if first || (sidecar && is_sidecar) {
                first = false;
                sidecar = false;
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

#[cfg(test)]
mod tests {
    use super::{Moves, suffix};

    /// The journal after moving a database and one sidecar into `to`, then undoing.
    fn undo_case(
        dir: &std::path::Path,
        fail_first: bool,
        fail_sidecar: bool,
    ) -> (Vec<String>, bool, bool) {
        let from = dir.join("prepared.db");
        let to = dir.join("kept.db");
        std::fs::write(&from, b"database").unwrap();
        let from_wal = suffix(&from, "-wal");
        std::fs::write(&from_wal, b"sidecar").unwrap();

        let mut moves = Moves::default();
        moves.perform_sidecars(&from, &to).unwrap();
        moves.perform(&from, &to).unwrap();
        let stuck = moves.undo_with(fail_first, fail_sidecar);
        (stuck, from.exists(), suffix(&to, "-wal").exists())
    }

    #[test]
    fn a_rollback_that_can_finish_puts_every_file_back() {
        let dir = tempfile::tempdir().unwrap();
        let (stuck, database_back, wal_still_kept) = undo_case(dir.path(), false, false);
        assert!(stuck.is_empty(), "{stuck:?}");
        assert!(database_back, "the database is back at its old name");
        assert!(
            !wal_still_kept,
            "the sidecar went back with the database, not stranded"
        );
    }

    #[test]
    fn a_sidecar_that_cannot_go_back_is_named_and_keeps_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (stuck, database_back, wal_still_kept) = undo_case(dir.path(), false, true);
        assert_eq!(stuck.len(), 1, "exactly one file is named: {stuck:?}");
        assert!(
            stuck[0].contains("kept.db-wal"),
            "the moved sidecar is the one named: {stuck:?}"
        );
        assert!(database_back, "the database itself went back");
        assert!(wal_still_kept, "the sidecar stayed where it was kept");
        assert_eq!(
            std::fs::read(dir.path().join("kept.db-wal")).unwrap(),
            b"sidecar",
            "and it holds its bytes"
        );
    }

    #[test]
    fn the_newest_file_that_cannot_go_back_is_named() {
        let dir = tempfile::tempdir().unwrap();
        let (stuck, database_back, _) = undo_case(dir.path(), true, false);
        assert_eq!(stuck.len(), 1, "{stuck:?}");
        assert!(
            stuck[0].contains("kept.db back to"),
            "the database is named: {stuck:?}"
        );
        assert!(!database_back, "it stayed at the kept name");
    }
}
