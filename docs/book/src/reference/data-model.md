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
| `agents` | Agent identities: public key, version, enrolled and revoked times, plus what the agent last reported after authenticating: its protocol, release, last authentication and, when the hub rejected its protocol, the time it did so. A host has at most one agent that is not revoked, and a revoked agent never returns. |
| `enrollment_tokens` | Hashes of single-use enrollment tokens with their host and expiry. Consuming one is a single atomic update. |
| `signals` | Raw signals from checks, probes and webhooks, kept as JSON with indexed host, check, status and time. |
| `incident_signals` | Which signals belong to which incident. |
| `incidents` | Incident state as JSON, with indexed fingerprint, host, status, severity and times, and a `version` column that every writer names and the store increments. At most one active incident per fingerprint. |
| `incidents_fts` | Full-text index of incident titles, kept in step by triggers. |
| `incident_events` | Each incident's timeline (status changes, triage reports, actions), in the order it happened. Each new entry is also recorded in `journal` in the same transaction, until the old table is retired. |
| `actions` | Proposed and approved actions: skill, arguments, plan hash, status, proposer, approval and the chat button token hash. |
| `audit_log` | Who did what, when: approvals, dispatches, results, logins. Append-only: triggers reject every update, delete and replace. Rows written since the hash chain was introduced also carry `prev_hash` and `hash`. An action transition is recorded in the same transaction as the change. |
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
| `effects` | The work outbox: side effects that must happen later, such as a verification re-check, with their due time, attempts, claim and lease. See [The effects table](#the-effects-table). |
| `schema_compat` | One row naming the lowest Domovoi version that can read this schema. |
| `audit_anchor` | One row committing to the audit history written before the hash chain existed: an install id, the highest legacy sequence number, the row count and a SHA-256 digest. It is append-only too. |

### The effects table

`effects` is the work outbox: work that must happen later, such as the re-check after an action, is stored here in the same commit as the change that asked for it. A row has `kind`, `aggregate_type`, `aggregate_id`, `payload`, `status`, `not_before`, `attempt`, `max_attempts`, `dedup_key`, `claim_token`, `claim_generation`, `lease_until`, `runner_id`, `last_error`, `created_at` and `completed_at`.

A row is `pending` until it is due, `claimed` while a runner holds it under its lease, and then `done`. It becomes `dead` after its last attempt, which raises an advisory, or `cancelled` when the work no longer applies.

### Legacy and chained audit history

Rows written before the hash chain have no `prev_hash` or `hash`; they are the *legacy* history. The
anchor commits to them once, when the database is first opened by a version that has the chain. Every
row written afterwards is *chained*: its `prev_hash` is the previous chained row's hash (or the
anchor's own hash for the first one) and its `hash` covers the row's sequence number, its fields and
its detail. Legacy rows are never rewritten.

`domovoi audit verify` reports the two parts separately, because they can fail differently: a legacy
row edit or deletion changes the digest but not any chain, while an edit, deletion or unchained insert
after the anchor breaks the chain.

## Search

Notes and incident titles are searched with SQLite FTS5. Search text is split into words and every word must appear; FTS5 operators such as `OR`, `NEAR`, `*` and column filters in the search text are treated as plain words. Notes rank by how often the words occur, a title occurrence counting ten times a body or tag occurrence, so title matches come first.

## Retention

Every period is a setting in `[retention]`; the [Retention guide](../guide/retention.md) explains what each one removes and what deletion guarantees about backups.

| Data | Setting | Default |
| --- | --- | --- |
| Signals | `signals_days` | 30 days |
| Incident timeline entries other than follow-up conversations | `incident_timeline_days` | 90 days |
| Follow-up questions and answers | `conversations_days` | 180 days |
| Notification state of resolved incidents | `notifications_days` | 30 days |
| Cached triage reports | `triage_cache_days` | 90 days |
| Resolved advisories | `resolved_advisories_days` | 30 days after resolving |
| Operation journal (not built yet) | `operation_journal_days` | 180 days |
| Raw execution outputs (not built yet) | `execution_outputs_days` | 30 days |
| Retracted fact content (not built yet) | `retracted_facts_days` | 30 days |

Maintenance windows and sessions are removed when they end or expire. Incidents, operations and resolution summaries, the audit log and digest facts are kept.

## Backups

Backups use `VACUUM INTO`, which writes a consistent copy while the hub keeps running.

Backups go to `[backup] dir` (default `<data_dir>/backups`).

- Before migrating an existing database, the hub writes `pre-migrate-<time>.db` there and keeps the newest `[backup] keep_pre_migrate` (default 3). If a migration fails, that file is the database as it was.
- The nightly backup writes `domovoi-<time>.db` and keeps the newest `[backup] keep` (default 7).

The database directory is created readable by its owner only.

## Durability

Every commit is flushed to disk before it is acknowledged (`synchronous=FULL`), so an approval or an audit row survives a power loss. State, journal, audit and effects of one business event are written in one SQLite transaction: either all of them are stored or none is.

At start-up the hub runs `PRAGMA quick_check`. If the database reports a problem, the hub refuses to start and says so. The fix is to restore a backup; the hub never continues with a damaged database. On a very large database the first start after an upgrade is slower because this check reads the whole file.

## Compatibility

`schema_compat` records the lowest Domovoi version that can read the schema. A binary older than that refuses to open the database, before it takes a backup or touches the file, and says which version is needed. The only way back is to restore an older backup.

A binary that knows fewer migrations than the database ignores the ones it does not know, so upgrading can be rolled back to the previous release when that release already has this rule. A rollback to an older release that does not is a forward-fix or restore situation.
