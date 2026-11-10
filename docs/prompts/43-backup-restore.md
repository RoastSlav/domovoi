# 43 · Feature: restore the hub from a backup, and see when the last backup ran

## Background (verified on `main`)

- Nightly backups: `VACUUM INTO` at 04:00 hub-local time into `[backup] dir` (default
  `<data_dir>/backups`), keeping `keep_backups` (default 7) files named
  `domovoi-<YYYYMMDDTHHMMSSZ>.db` (`crates/domovoi-store/src/backup.rs`,
  `crates/domovoi-hub/src/runtime/schedulers.rs:357-366`). A failure only logs a warning.
- Before migrations the store writes `pre-migrate-<stamp>.db`; pruning keeps only
  `domovoi-*` files in check, so pre-migrate copies pile up.
- `docs/book/src/reference/backup.md` explains restoring `secret.key` and `tls/` but has
  **no procedure for `hub.db`**, while `install/upgrading.md` tells people to "restore
  the backup taken before the upgrade".
- One hub per data directory is enforced with `<data_dir>/hub.lock`
  (`crates/domovoi-hub/src/runtime/lock.rs`).
- Advisories exist for hygiene findings (`crates/domovoi-engine/src/advisories.rs`) and
  show in the digest and the UI.

## Steps

1. **CLI** `domovoi hub restore <backup.db> [--data-dir ..] [--yes]` in
   `crates/domovoi/src/cmd/` (new `restore.rs`):
   - Refuse while `hub.lock` is held ("stop the hub first").
   - Open the backup read-only and run `PRAGMA integrity_check`; refuse unless `ok`.
     Refuse a file whose migration version is newer than this binary knows.
   - Move the current `hub.db`, `hub.db-wal`, `hub.db-shm` to
     `hub.db.before-restore-<stamp>` (and the sidecars), then copy the backup to
     `hub.db` with mode 0600. Without `--yes`, print what will happen and ask.
   - Print: "Restored. secret.key and tls/ were not touched; if they came from another
     machine, copy them too (see the Backups page)."
   The store crate does the SQLite work (a `pub async fn verify_backup(path)`); the
   binary does the file moves. No new dependency.
2. **Pruning.** `keep_backups` also prunes `pre-migrate-*` files, keeping the newest 3.
3. **Health.** Record the last backup outcome in the `settings` table key
   `last_backup` (`{ at, ok, error }`). Raise a `backup_failed` advisory when the last
   nightly backup failed or none succeeded for 48 h; resolve it on the next success.
   Show "Last backup: 03:00 today" on the Settings → Backups section.
4. **Book.** `docs/book/src/reference/backup.md`: a "Restore" section (stop the hub, run
   the command or copy by hand, sidecar files, `secret.key` and `tls/`, start the hub,
   what to check). `reference/cli.md`: the new command. `install/upgrading.md`: link to
   it.

## Tests

- Store: `verify_backup` accepts a fresh backup and refuses a truncated file.
- CLI (`crates/domovoi/tests/cli.rs`): restore refuses while the lock is held; restores a
  backup and keeps the old file aside.
- Engine/hub: the advisory is raised after a failed backup and resolved after a good one
  (fake clock, fake store).

## Done when

- An operator can restore yesterday's database with one command and the book explains
  it, and a failing backup shows up in the UI instead of only in the log.
