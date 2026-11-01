# Backups

The hub keeps everything in its data directory (`/var/lib/domovoi` by default):

| Path | What it holds |
| --- | --- |
| `hub.db` | Incidents, signals, actions, notes, the audit log, accounts and the settings saved in the web UI. |
| `backups/` | Nightly copies of `hub.db`, and one taken before every migration. |
| `secret.key` | The key that encrypts secrets entered in the web UI: bot tokens, API keys, webhook secrets and heartbeat URLs. |
| `tls/` | The agent server's certificate, which every enrolled agent pins. |

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
