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

   The current `hub.db` (with its `-wal` and `-shm`) is moved to `hub.db.before-restore-<time>`, the backup is copied into place with owner-only permissions and synced, then opened so any pending migrations run.
4. Start the hub again, then check the audit log with `domovoi audit verify`.

The previous database is never deleted by the command. If anything fails after it was moved aside, it is moved back and the failed file is kept as `hub.db.failed-restore-<time>`.

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
