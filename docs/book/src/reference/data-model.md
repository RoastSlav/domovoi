# Data model

The hub keeps its state in one SQLite database in WAL mode, under `[hub] data_dir`. Migrations are embedded in the binary and run at start-up. The schema starts at one baseline, `0001_init.sql`. Migrations are append-only: a released migration is never edited, only followed by a new one.

## Conventions

- Ids are UUIDs stored as `TEXT`.
- Timestamps are `INTEGER` unix milliseconds, UTC.
- Hashes are 32-byte `BLOB`s; fingerprints and plan hashes are lowercase hex `TEXT`.
- Structured values (labels, evidence, arguments, reports) are `TEXT` JSON.
- Enums are `TEXT`. Some columns carry a `CHECK` constraint; enum-like columns that may gain variants are validated by the program so a new variant needs no migration.
- Every table is `STRICT`, so SQLite rejects a value of the wrong type.

## Tables

| Table | Contents |
| --- | --- |
| `resolution_summary_fields` | Append-only revisions of each field of a finished operation's summary: field, revision number, value, source (`code`, `model` or `user`), actor and time. Triggers reject every update and delete. |
| `operation_resolutions` | One row per operation with a summary: the current value of each field, rewritten in the same transaction as every revision. It feeds the full-text index. |
| `operation_resolutions_fts` | Full-text index of the current summary text, kept in step by triggers. |
| `operation_resolution_hosts` | The hosts of each summary, so a search can stay on one host. |
| `operation_resolution_subjects` | The host, check and target of each summary, to find similar operations. |
| `hosts` | Every host Domovoi knows: name, kind (local, remote, external), labels. Names are unique. |
| `agents` | Agent identities: public key, version, enrolled and revoked times, plus what the agent last reported after authenticating: its protocol, release, last authentication and, when the hub rejected its protocol, the time it did so. A host has at most one agent that is not revoked, and a revoked agent never returns. An agent id names one enrollment for life: only its reported version changes; its host, public key and enrollment time are fixed. |
| `enrollment_tokens` | Hashes of single-use enrollment tokens with their host and expiry. Consuming one is a single atomic update. |
| `signals` | Raw signals from checks, probes and webhooks, kept as JSON with indexed host, check, status and time. |
| `incident_signals` | Which signals belong to which incident. |
| `incidents` | Incident state as JSON, with indexed fingerprint, host, status, severity and times, and a `version` column that every writer names and the store increments. At most one active incident per fingerprint. |
| `incidents_fts` | Full-text index of incident titles, kept in step by triggers. |
| `incident_events` | Each incident's timeline (status changes, triage reports, operations), in the order it happened. Each new entry is also recorded in `journal` in the same transaction. |
| `operations` | One row per operation: kind, goal, status (a cache of the status derived from its steps), pause reason, cycle counter, budgets and the subjects it is pinned to. See [Operations](../concepts/operations.md). |
| `operation_incidents` | Which incidents an operation is linked to. |
| `steps` | The immutable steps of an operation: kind, phase, cycle, ordinal, intent and outcome. A trigger refuses any update to a step's identity columns. |
| `step_edges` | Links from a later step to an earlier step of the same operation. A trigger refuses an edge that points forward. |
| `executions` | One row per Execution step: host, capability contract, plan hash, risk, approval, the hash of the chat button token and the dispatch facts. |
| `execution_permits` | The policy permit that authorized an execution: decision, policy version, ceiling hash, plan hash and expiry. |
| `execution_permit_rules` | The rules that applied to each permit. |
| `unknown_resolutions` | How a person settled an execution whose outcome was unknown, with who and when. |
| `journal_floor` | One row recording the sequence number through which the journal was pruned. |
| `llm_admission` | The model runs admitted in the last hour, for the hourly cap. |
| `policy_versions` | The draft and every published policy version: status, number, rule counts and the hash of its rules. A published version never changes. |
| `policy_rules` | The rules of each version. Triggers freeze them once the version is published. |
| `policy_acknowledgements` | The conflicts acknowledged when a version was published. |
| `policy_shadow_decisions` | One recorded decision per Execution step in shadow mode, with the enforced and the shadow answer. Written once. |
| `policy_shadow_state` | The progress and the last error of the shadow recording sweep. |
| `autonomy_state` | One row: whether policy permits are paused, and who changed it and when. |
| `rule_circuit_resets` | When and by whom a suspended rule's circuit breaker was reset. |
| `package_artifacts` | The package archives the hub stores, by SHA-256 digest, with their signature text. The archive files themselves are not in the database or in backups. |
| `install_jobs` | One row per package install or uninstall request. |
| `install_job_targets` | One row per host of a job: attempt, plan hash, approval, status and transfer facts. A partial unique index allows one unfinished target per host. |
| `host_packages` | The packages each host last reported. |
| `helper_keys` | The helper key the owner pinned for each host. |
| `helper_attestation` | The latest attestation outcome per host, and the verified report while that outcome is verified. |
| `helper_high_water` | The highest accepted attestation generation and hashes per host and key. |
| `containment_blocks` | Network-source blocks: host, operation and step, canonical source, lifetime, plan hash, safety assurance, phase and deadline. See [Blocking a network source](../guide/containment.md). |
| `audit_log` | Who did what, when: approvals, dispatches, results, logins. Append-only: triggers reject every update, delete and replace. Every row carries `prev_hash` and `hash`. An audit row is written in the same transaction as the change it records. |
| `ack_rules` | Standing acknowledgements: check, optional host and target pattern, evidence predicates, note, expiry, and who created the rule and when. |
| `maintenance_windows` | Planned windows per host when incidents are held back. |
| `baselines` | What each host is expected to run: containers, units and listening ports, with criticality and when they were last seen. |
| `notes` | Runbook notes in Markdown, global or for one host, with tags. |
| `notes_fts` | Full-text index of note titles, bodies and tags, kept in step by triggers. |
| `conversations` | Operator conversations: id, title (the first question), scope (`global` or one host), the attached operation, the last message ordinal and the times. Kept after its messages are purged. |
| `messages` | The messages of an Operator conversation, in order: role, status (`pending`, `complete`, `failed` or `cancelled`), turn, text, failure, the read tools used, and the times. Each conversation has at most one pending answer. Message text lives only here, so retention can delete it. |
| `triage_cache` | Triage reports keyed by incident fingerprint and a digest of the evidence, so the same evidence is not triaged twice. |
| `llm_usage` | Tokens and cost per LLM call, for the monthly budget. |
| `metric_samples` | Numeric samples agents report for the daily digest (disk usage, temperatures, counts). The view `digest_facts` reads and inserts into it. |
| `facts` | Knowledge facts: a subject, a predicate from a closed list, a value, a source, a status and the times that drive freshness. A retracted fact keeps only its tombstone once its value is cleared. See [Knowledge](../concepts/knowledge.md). |
| `facts_fts` | Full-text index of live facts, kept in step by triggers. |
| `relations` | Typed links between two subjects, such as one host depending on another. Retracted relations are cleared like retracted facts. |
| `entities` | Named things that facts and relations can refer to, as `entity:<uuid>`. |
| `management_path_confirmations` | The owner's confirmations of management-path facts. |
| `advisories` | Things worth knowing that are not incidents: a missing heartbeat, a stale image, an outdated agent. |
| `users` | Web users: username and argon2id password hash. Usernames are case-sensitive. |
| `sessions` | Web sessions: hashes of the session and CSRF tokens, owner and expiry. |
| `settings` | Hub settings changed from the UI, as JSON values. It also holds each account's display preferences under `preferences.<user id>`; a missing row means the default detail level. |
| `notify_state` | Per incident: where its card and thread are on each chat platform, when it was last reminded about, folded repeats not yet mentioned, and an alert held for the end of quiet hours. |
| `journal` | The operational record: typed events in commit order, with a global sequence number, the aggregate they belong to and when they happened. An optional `legacy_event_seq` column points at the `incident_events` row an entry mirrors. |
| `effects` | The work outbox: side effects that must happen later, such as a verification re-check, with their due time, attempts, claim and lease. See [The effects table](#the-effects-table). |
| `schema_compat` | One row naming the lowest Domovoi version that can read this schema. |
| `audit_anchor` | One row that starts the audit hash chain: the install id and the anchor hash that the first audit row's `prev_hash` points at. It is append-only too. |

### The effects table

`effects` is the work outbox: work that must happen later, such as the re-check after an execution, is stored here in the same commit as the change that asked for it. A row has `kind`, `aggregate_type`, `aggregate_id`, `payload`, `status`, `not_before`, `attempt`, `max_attempts`, `dedup_key`, `claim_token`, `claim_generation`, `lease_until`, `runner_id`, `last_error`, `created_at` and `completed_at`.

A row is `pending` until it is due, `claimed` while a runner holds it under its lease, and then `done`. It becomes `dead` after its last attempt, which raises an advisory, or `cancelled` when the work no longer applies.

### Audit chain

Every audit row has a `prev_hash` and a `hash`. The first row's `prev_hash` is the anchor's own hash, and each later row's `prev_hash` is the previous row's hash. A row's `hash` covers its sequence number, its fields and its detail.

`domovoi audit verify` walks the chain and reports whether it is `empty`, `intact` or `broken`. An edit, a deletion or an insert without chain fields breaks it. The web UI's [Audit log](../guide/audit.md) page shows the same status.

## Search

Resolution summaries are searched the same way, in two passes: operations that contain every word come first, then operations that contain any word. A search on one host returns only operations whose hosts are exactly that host.

Notes and incident titles are searched with SQLite FTS5. Search text is split into words and every word must appear; FTS5 operators such as `OR`, `NEAR`, `*` and column filters in the search text are treated as plain words. Notes rank by how often the words occur, a title occurrence counting ten times a body or tag occurrence, so title matches come first.

## Retention

Every period is a setting in `[retention]`; the [Retention guide](../guide/retention.md) explains what each one removes and what deletion guarantees about backups.

| Data | Setting | Default |
| --- | --- | --- |
| Signals | `signals_days` | 30 days |
| Incident timeline entries other than follow-up conversations | `incident_timeline_days` | 90 days |
| Follow-up questions and answers, and Operator conversation messages | `conversations_days` | 180 days |
| Notification state of resolved incidents | `notifications_days` | 30 days |
| Cached triage reports | `triage_cache_days` | 90 days |
| Resolved advisories | `resolved_advisories_days` | 30 days after resolving |
| Operation journal (accepted, not applied yet) | `operation_journal_days` | 180 days |
| Raw execution outputs (accepted, not applied yet) | `execution_outputs_days` | 30 days |
| Retracted fact content | `retracted_facts_days` | 30 days |

Maintenance windows and sessions are removed when they end or expire. Incidents, operations and resolution summaries, the audit log and metric samples are kept. Resolution summaries hold no raw output and stay with their operation; deleting data does not remove it from backups until they rotate.

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

A binary that knows fewer migrations than the database ignores the ones it does not know, so a release can be rolled back to the previous one when that release already has this rule. Otherwise the way back is to restore a backup.
