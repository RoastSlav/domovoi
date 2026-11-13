# Data model

The hub keeps its state in one SQLite database in WAL mode, under `[hub] data_dir`. Migrations are embedded in the binary and run at start-up. They are append-only: a released migration is never edited, only followed by a new one.

## Conventions

- Ids are UUIDs stored as `TEXT`.
- Timestamps are `INTEGER` unix milliseconds, UTC.
- Hashes are 32-byte `BLOB`s; fingerprints and plan hashes are lowercase hex `TEXT`.
- Structured values (labels, evidence, arguments, reports) are `TEXT` JSON.
- Enums are `TEXT`. Older columns carry a `CHECK` constraint; new enum-like columns are validated by the program so a new variant needs no migration.
- Every table is `STRICT`, so SQLite rejects a value of the wrong type.

## Tables

| Table | Contents |
| --- | --- |
| `hosts` | Every host Domovoi knows: name, kind (local, remote, external), labels. Names are unique. |
| `agents` | Agent identities: public key, version, enrolled and revoked times. A host has at most one agent that is not revoked, and a revoked agent never returns. |
| `enrollment_tokens` | Hashes of single-use enrollment tokens with their host and expiry. Consuming one is a single atomic update. |
| `signals` | Raw signals from checks, probes and webhooks, kept as JSON with indexed host, check, status and time. |
| `incident_signals` | Which signals belong to which incident. |
| `incidents` | Incident state as JSON, with indexed fingerprint, host, status, severity and times. At most one active incident per fingerprint. |
| `incidents_fts` | Full-text index of incident titles, kept in step by triggers. |
| `incident_events` | Each incident's timeline (status changes, triage reports, actions), in the order it happened. |
| `actions` | Proposed and approved actions: skill, arguments, plan hash, status, proposer, approval and the chat button token hash. |
| `audit_log` | Who did what, when: approvals, dispatches, results, logins. Append-only: triggers reject every update, delete and replace. |
| `ack_rules` | Standing acknowledgements: check, optional host and target pattern, evidence predicates, note, expiry, and who created the rule and when. |
| `maintenance_windows` | Planned windows per host when incidents are held back. |
| `baselines` | What each host is expected to run: containers, units and listening ports, with criticality and when they were last seen. |
| `notes` | Runbook notes in Markdown, global or for one host, with tags. |
| `notes_fts` | Full-text index of note titles, bodies and tags, kept in step by triggers. |
| `triage_cache` | Triage reports keyed by incident fingerprint and a digest of the evidence, so the same evidence is not triaged twice. |
| `llm_usage` | Tokens and cost per LLM call, for the monthly budget. |
| `digest_facts` | Facts agents report for the daily digest (disk usage, temperatures, counts). |
| `advisories` | Things worth knowing that are not incidents: a missing heartbeat, a stale image, an outdated agent. |
| `users` | Web users: username and argon2id password hash. Usernames are case-sensitive. |
| `sessions` | Web sessions: hashes of the session and CSRF tokens, owner and expiry. |
| `settings` | Hub settings changed from the UI, as JSON values. |
| `notify_state` | Per incident: where its card and thread are on each chat platform, when it was last reminded about, folded repeats not yet mentioned, and an alert held for the end of quiet hours. |
| `journal` | The operational record: typed events in commit order, with a global sequence number, the aggregate they belong to and when they happened. |
| `effects` | The work outbox: pending side effects with their kind, payload, attempts, claim token and lease. |
| `schema_compat` | One row naming the lowest Domovoi version that can read this schema. |

## Search

Notes and incident titles are searched with SQLite FTS5. Search text is split into words and every word must appear; FTS5 operators such as `OR`, `NEAR`, `*` and column filters in the search text are treated as plain words. Notes rank by how often the words occur, a title occurrence counting ten times a body or tag occurrence, so title matches come first.

## Retention

| Kept | For |
| --- | --- |
| Signals | 30 days |
| Incident timeline entries | 90 days |
| Cached triage reports | 90 days |
| Resolved advisories | 30 days after resolving |
| Maintenance windows, sessions | until they end or expire |
| Incidents, audit log, digest facts | forever |

## Backups

Backups use `VACUUM INTO`, which writes a consistent copy while the hub keeps running.

Backups go to `[backup] dir` (default `<data_dir>/backups`).

- Before migrating an existing database, the hub writes `pre-migrate-<time>.db` there. If a migration fails, that file is the database as it was.
- The nightly backup writes `domovoi-<time>.db` and keeps the newest `[backup] keep` (default 7).

The database directory is created readable by its owner only.

## Durability

Every commit is flushed to disk before it is acknowledged (`synchronous=FULL`), so an approval or an audit row survives a power loss. State, journal, audit and effects of one business event are written in one SQLite transaction: either all of them are stored or none is.

At start-up the hub runs `PRAGMA quick_check`. If the database reports a problem, the hub refuses to start and says so. The fix is to restore a backup; the hub never continues with a damaged database. On a very large database the first start after an upgrade is slower because this check reads the whole file.

## Compatibility

`schema_compat` records the lowest Domovoi version that can read the schema. A binary older than that refuses to open the database, before it takes a backup or touches the file, and says which version is needed. The only way back is to restore an older backup.

A binary that knows fewer migrations than the database ignores the ones it does not know, so upgrading can be rolled back to the previous release when that release already has this rule. A rollback to an older release that does not is a forward-fix or restore situation.
