# Backups

The hub keeps everything in its data directory (`/var/lib/domovoi` by default):

| Path | What it holds |
| --- | --- |
| `hub.db` | Incidents, signals, actions, notes, the audit log, accounts and the settings saved in the web UI. |
| `backups/` | Nightly copies of `hub.db`, and one taken before every migration. |
| `secret.key` | The key that encrypts secrets entered in the web UI: bot tokens, API keys, webhook secrets and heartbeat URLs. |
| `tls/` | The agent server's certificate, which every enrolled agent pins. |

A restored database verifies as it was at backup time: `domovoi audit verify` checks the hash chain in the copy exactly as it stood when the backup was taken.

## Verification

Every nightly copy is checked before it counts as a backup. The hub runs `PRAGMA quick_check` on the new file, reads its migration history and refuses a copy that is damaged, is not a Domovoi database, or was made by a newer release.

- A copy that fails is deleted, older backups stay where they are, and a `backup_failed` advisory appears in the digest and on the Overview page.
- The next successful backup clears the advisory and sets `domovoi_backup_last_success_timestamp_seconds`.
- A failed `VACUUM INTO` (a full disk, for example) raises the same advisory and prunes nothing.

`domovoi restore` checks the file again, in full, before it changes anything.

## How long copies are kept

| Copies | Kept | Where the count is set |
| --- | --- | --- |
| Nightly, `domovoi-<time>.db` | 7 | `backup.keep` |
| Before a migration, `pre-migrate-<time>.db` | 3 | `backup.keep_pre_migrate` |

Anything else in the backups folder is never touched.

Deleted data stays in the copies that still hold it: resolving an incident or letting retention purge signals removes rows from `hub.db`, not from yesterday's backup. Restoring a backup therefore brings that data back, and retention removes it again at the next purge. Shred anything that must not come back.

## Restoring

Restoring is an offline operator action. It never runs inside a running hub, and the web UI cannot do it.

1. Stop the hub.
2. Check what would happen:

   ```sh
   domovoi restore /var/lib/domovoi/backups/domovoi-20261114T040000Z.db
   ```

   It verifies the file and prints the migration count, the size, and where the current database would be moved. Nothing changes, and it exits with code 2.
3. Run it again with `--yes`:

   ```sh
   domovoi restore /var/lib/domovoi/backups/domovoi-20261114T040000Z.db --yes
   ```

   The backup is first prepared beside the live database, and only then installed:

   1. the backup is verified in full;
   2. it is copied to `hub.db.restore-tmp`;
   3. the copy is made owner-only (on Unix) and synced;
   4. the copy's directory entry is synced (on Unix);
   5. the copy is opened and migrated forward, so pending migrations run on the copy;
   6. only now is the current `hub.db`, with its `-wal` and `-shm`, moved to `hub.db.before-restore-<time>`;
   7. the prepared copy is renamed into place.

   Because the copy is migrated before the live database is touched, a backup that cannot be opened or migrated leaves the live database and every sidecar exactly where they were. That matters when the file is damaged in a way the full check still tolerates.
4. Start the hub again, then check the audit log with `domovoi audit verify`.

The previous database is never deleted by the command, on any path including a failed install. If installing the prepared copy fails, everything already moved is moved back, and the report says so. If that rollback itself cannot finish, the command fails with an error naming every file that is still out of place rather than reporting a clean failure.

A database that will not open is kept for inspection as `hub.db.failed-restore-<time>`. On Windows the failed open may still hold the file, so it stays at `hub.db.restore-tmp` instead; the log names whichever path applies.

Names never collide: a preserved database takes a numbered suffix (`hub.db.before-restore-<time>-2`) rather than replacing an earlier one.

### What durability the install has

- **Unix:** the prepared copy's data and its directory entry are synced before the live database is moved, so a crash after the swap leaves the restored database in place. Errors from either sync stop the restore before anything moves.
- **Windows:** there is no portable directory fsync, so the final rename is not made durable against a power loss between it and the next start. The file's own data is synced and the renames are ordered, and the preserved `hub.db.before-restore-<time>` is what to fall back to.

Restoring a directory that holds only `hub.db-wal` or `hub.db-shm`, without `hub.db`, moves those sidecars aside too rather than discarding them: they keep the `.before-restore-<time>` prefix and the log names it. A restore with nothing to preserve leaves no empty `.before-restore-*` file.

Restore `secret.key` and `tls/` from the same time if they were lost. Without `secret.key` the secrets entered in the web UI cannot be read (see below); without `tls/` the hub creates a new certificate no enrolled agent accepts.

## Keep `secret.key` with your backups, but apart from them

Secrets saved in the web UI are stored in `hub.db` only in encrypted form, so a database backup copied off the machine carries no usable tokens. That only holds if the key does not travel with it.

- Back up `secret.key` once, to a different place from the database backups (a password manager, or an encrypted store).
- Restore it into the data directory, with mode `0600`, before starting a restored hub.

## Without the key

If `secret.key` is missing or is a different key, the hub still starts with every other setting and all data. Only the secrets entered in the UI cannot be read:

- a `StoredSecretUnreadable` advisory names them;
- the settings that need them stay off (a webhook without its secret is not accepted, for example);
- enter them again on the Settings page, and they are encrypted with the current key.

Secrets taken from files or environment variables (`{ env = .. }`, `{ file = .. }`) are not affected.

## Without `tls/`

Agents pin the hub's certificate. A hub that lost `tls/` creates a new certificate, which no enrolled agent accepts. Restore `tls/` from a backup, or enroll each agent again.
