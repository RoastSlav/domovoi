//! The renames a restore makes, and how a failure of one is reported.

use std::path::Path;

use super::RestoreError;
use super::durable::{Fence, Stage};
use super::files::{Moves, Reservation, companions, suffix};

/// Moves the current database and its sidecars to the preserved name.
pub(super) fn move_aside(
    moves: &mut Moves,
    database: &Path,
    mut before: Option<&mut Reservation>,
    fail_at: Option<&str>,
) -> Result<(), RestoreError> {
    for (source, tail) in companions(database) {
        if !source.exists() {
            continue;
        }
        if forced(fail_at, "aside", tail) {
            return Err(RestoreError::Io {
                path: source,
                reason: "cannot move it aside: simulated failure".to_owned(),
            });
        }
        let Some(before) = before.as_deref_mut() else {
            return Err(RestoreError::Io {
                path: source,
                reason: "no preserved name was reserved".to_owned(),
            });
        };
        let destination = suffix(before.path(), tail);
        moves.perform(&source, &destination)?;
        if tail.is_empty() {
            // The real database has taken the name. From here it is never a placeholder,
            // however the restore turns out, so a rollback that cannot finish leaves it
            // where it is instead of deleting it.
            before.filled();
        }
    }
    Ok(())
}

/// Moves the prepared copy over `database`, with the SQLite side files its open left.
pub(super) fn swap_in(
    moves: &mut Moves,
    temporary: &Path,
    database: &Path,
    fail_at: Option<&str>,
) -> Result<(), RestoreError> {
    for tail in ["-wal", "-shm"] {
        let side = suffix(temporary, tail);
        if !side.exists() {
            continue;
        }
        if forced(fail_at, "swap", tail) {
            return Err(RestoreError::Io {
                path: side,
                reason: "cannot install it: simulated failure".to_owned(),
            });
        }
        moves.perform(&side, &suffix(database, tail))?;
    }
    if forced(fail_at, "swap", "") {
        return Err(RestoreError::Io {
            path: temporary.to_owned(),
            reason: "cannot install it: simulated failure".to_owned(),
        });
    }
    moves.perform(temporary, database)
}

/// Whether the step `phase` with sidecar `tail` is the one a test asked to fail.
pub(super) fn forced(fail_at: Option<&str>, phase: &str, tail: &str) -> bool {
    let wanted = match (phase, tail) {
        ("aside", "-wal") => "aside-wal",
        ("aside", "-shm") => "aside-shm",
        ("swap", "-wal") => "swap-wal",
        ("swap", "-shm") => "swap-shm",
        ("swap", "") => {
            // The rollback-failure steps install nothing either.
            return matches!(
                fail_at,
                Some(
                    "swap-database"
                        | "undo-database"
                        | "sync-rollback"
                        | "undo-database-sync-rollback"
                )
            );
        }
        ("sync", "aside") => "sync-aside",
        ("sync", "install") => "sync-install",
        // Only consulted after a rollback, by `roll_back`.
        ("sync", "rollback") => {
            return matches!(
                fail_at,
                Some("sync-rollback" | "undo-database-sync-rollback")
            );
        }
        _ => return false,
    };
    fail_at == Some(wanted)
}

/// Whether the file that first moves aside should fail to move back.
///
/// `undo-database` makes the install fail and then the rollback fail too, which is how a
/// test reaches [`RestoreError::RecoveryIncomplete`] without depending on permissions.
/// `undo-database-sync-rollback` adds a failing flush to the rollback, for the combined
/// case where every failure has to be reported at once.
pub(super) fn undo_fails(fail_at: Option<&str>) -> bool {
    matches!(
        fail_at,
        Some("undo-database" | "undo-database-sync-rollback" | "failed-database-with-wal-stuck")
    )
}

/// The error a test's forced directory-flush failure produces.
pub(super) fn simulated_not_durable(fence: &Fence) -> RestoreError {
    RestoreError::NotDurable {
        stage: fence.stage_name().to_owned(),
        path: fence.directory().to_path_buf(),
        reason: "simulated failure".to_owned(),
    }
}

/// Reports a failed stage after its rollback ran.
///
/// The original failure is kept, and so is every file the rollback could not put back. A
/// rollback whose own directory flush fails is reported too: the files look restored, but
/// a crash could still undo that, and the operator must not be told otherwise.
pub(super) fn roll_back(
    err: RestoreError,
    stuck: Vec<String>,
    fence: &Fence,
    fail_at: Option<&str>,
) -> RestoreError {
    // The flush that matters after a rollback is the one that records the moves back, so
    // it is reported as that, not as the stage that failed.
    let after = Fence::around(fence.database(), Stage::Rollback);
    let durability = if forced(fail_at, "sync", "rollback") {
        Some(format!(
            "{}: simulated failure",
            after.directory().display()
        ))
    } else {
        after.commit().err().map(|err| err.to_string())
    };
    let recovery = if stuck.is_empty() {
        "every file moved back".to_owned()
    } else {
        stuck.join("; ")
    };
    match durability {
        None if stuck.is_empty() => err,
        None => RestoreError::RecoveryIncomplete {
            reason: err.to_string(),
            recovery,
        },
        Some(durability) => RestoreError::RecoveryIncompleteNotDurable {
            reason: err.to_string(),
            recovery,
            durability,
        },
    }
}
