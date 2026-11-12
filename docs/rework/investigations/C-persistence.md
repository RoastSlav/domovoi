# C. Persistence investigation

Scope: `crates/domovoi-store`, ADRs 0005/0011/0014/0019, ARCHITECTURE.md section 9, design-pack 06/07/02.
Read-only. Labels: FACT (read in code or docs), INFERENCE (derived), RECOMMENDATION.
Paths are relative to `C:\Programming\domovoi`. Store crate is `crates/domovoi-store` ("S/").

## 1. Schema map

### 1.1 Tables (migrations `S/migrations/0001..0005`)

FACT. Conventions: ids TEXT (UUID), timestamps INTEGER unix ms, hashes BLOB(32), structured fields TEXT JSON, enums TEXT + CHECK, all tables `STRICT` (0001_init.sql:1-5).

| Table | Key | Notes | Mutability |
| --- | --- | --- | --- |
| `hosts` | `id`; `name` UNIQUE (:7-13) | `kind` CHECK, `labels` JSON | mutable (upsert) |
| `agents` | `agent_id`; FK `host_id` (:15-24) | partial unique index: one live agent per host | mutable only via `revoked_at`; "revoked never comes back" enforced in a tx (hosts.rs:128-152) |
| `enrollment_tokens` | `token_hash` | `used_at` set by one `UPDATE ... RETURNING` (hosts.rs:~210) | single-use |
| `incidents` | `id` (:34-47) | scalar index columns + full state in `body` JSON; partial unique index `incidents_one_active_per_fingerprint` (:49); 4 more indexes; `incidents_fts(title, summary_text)` content-table + 3 triggers (:56-74) | mutable; whole-row overwrite (incidents.rs:155-179) |
| `signals` | `id` (:77-87) | scalar cols + `body` JSON; index on `observed_at` | append, then purged at 30 d |
| `incident_signals` | (`incident_id`,`signal_id`) | `signal_id` FK ON DELETE CASCADE; `incident_id` has no FK (:89-94) | append; cascades with signal purge |
| `incident_events` | `seq` AUTOINCREMENT (:97-106) | timeline; FK to incidents ON DELETE CASCADE; `detail` JSON | append-only by convention only (no trigger); purged at 90 d |
| `actions` | `id` (:108-127) | status is JSON (`{"type":...}`), `args` JSON, approval split in 3 columns with CHECK that approver/at are both set or both NULL; `chat_token_hash` UNIQUE; no FK on `incident_id`/`host_id` | mutable only through guarded `update_from` |
| `audit_log` | `seq` AUTOINCREMENT (:133-156) | 3 triggers: no UPDATE, no DELETE, no INSERT-OR-REPLACE | truly append-only (INV-7) |
| `ack_rules` | `id` | `predicates` JSON; `created_at`/`created_by` added in 0005 (nullable for old rows) | mutable, hard-deleted |
| `maintenance_windows` | UNIQUE(host,start,end,note) | | insert-or-ignore, purged when ended |
| `baselines` | (`host_id`,`kind`,`key`) | learned container/unit/port set; `managed` flag | upsert/delete |
| `notes` | `id` | `title, body, tags(JSON), updated_at`; `notes_fts` content-table with triggers (:198-216) | mutable, **no version, no history**; hard delete |
| `triage_cache` | (`fingerprint`,`evidence_digest`) | JSON `report` | upsert, purged at 90 d |
| `llm_usage` | AUTOINCREMENT | tokens/cost per call | append, never purged |
| `digest_facts` | AUTOINCREMENT | (host,key,label,value,unit,observed_at); fed hourly by every agent (crates/domovoi-agent/src/scheduler.rs:35, hub agent_server/session.rs:101-107) | append, **never purged** (purge.rs:1-2) |
| `advisories` | `id` | `kind` CHECK enum; rebuilt in 0003 and 0004 via create/copy/drop/rename to widen the CHECK | mutable (`resolved_at`), purged 30 d after resolve |
| `users`, `sessions` | | session tokens/CSRF hashed | sessions purged on expiry |
| `settings` | `key` | JSON value; also stores host profile JSON (hub profile/mod.rs:236-237) | upsert/delete; `set_many` is a transaction (accounts.rs:39-52) |
| `notify_state` | `incident_id` | per-incident notifier memory (0002) | upsert/delete |

Relationships are weak: only `agents`, `enrollment_tokens`, `incident_events`, `incident_signals.signal_id` carry FKs (foreign keys are ON, db.rs:94). `incidents.host_id`, `actions.incident_id/host_id`, `notes.host_id`, `baselines.host_id`, etc. are unchecked TEXT. INFERENCE: referential integrity for the core operational tables is application-enforced, so any new entity layer that points at them cannot lean on FKs without a migration.

### 1.2 Append-only vs mutable

- FACT truly append-only (DB-enforced): `audit_log` only (0001:145-156).
- FACT append-only by code convention: `incident_events`, `signals`, `llm_usage`, `digest_facts`. `incident_events` and `signals` are deleted by retention, so they are not an archive.
- FACT mutable whole-row snapshots with no history: `incidents` (body JSON overwritten on each change), `notes`, `baselines`, `ack_rules`, `advisories`, `settings`, `actions` (status overwritten; history lives only in `audit_log`).
- FACT: actions' history of transitions is reconstructible only from `audit_log` rows keyed by `subject = 'action:<id>'` (engine actions/recover.rs:99-110 parses it that way). The audit `subject` and `detail` are free text/JSON, not FK-linked.

### 1.3 Audit and history tables

- `audit_log`: seq, at, actor, kind, subject, detail JSON; indexes on actor, kind, subject, at. Listed by seq cursor (repos/audit.rs:38-60). Kept forever (ARCHITECTURE.md s9; purge.rs:1-2).
- `incident_events`: the per-incident timeline (see above).
- There is no notes history, no incident-body history, no knowledge change history.

## 2. Concurrency

### 2.1 SQLite mode and pools

- FACT: WAL, `synchronous = NORMAL`, `foreign_keys = ON`, busy_timeout 5 s (db.rs:19-20, 88-96).
- FACT: two pools on the same file: **writer pool max_connections = 1**, reader pool 4 (db.rs:21-22, 145-162). Every `INSERT/UPDATE/DELETE` goes through `self.write`; reads through `self.read`. The one write-pool read is `consume_enrollment_token`, which is an `UPDATE ... RETURNING` (hosts.rs:208-226).
- FACT: migrations run on the writer pool before the read pool exists (db.rs:154-162).
- FACT: no `wal_autocheckpoint`, `journal_size_limit`, `mmap_size`, `cache_size`, `temp_store` tuning; no `PRAGMA integrity_check` or `quick_check` at start (grep of db.rs).
- INFERENCE: `synchronous = NORMAL` in WAL means a power loss can drop the last committed transactions (not corrupt the file). Acceptable for a monitoring tool, but not for "the approval I just made is durable"; the pre-restart approval path (ADR 0019) relies on the store having it.
- INFERENCE: with one writer connection, any multi-statement flow holds the only write connection for its duration; 5 s busy timeout protects against a second process, not against pool exhaustion (pool acquire timeout is the sqlx default, 30 s).

### 2.2 Write serialisation (application level)

- FACT: incident state has one writer task, the incident manager; commands in, `decide()` pure, writes applied, events published (ADR 0011; engine manager/actor.rs:119-150).
- FACT: action decisions have one writer task plus a store guard `UPDATE ... WHERE id = ? AND json_extract(status,'$.type') = ?` (ADR 0019; repos/actions.rs:528-557). Loser gets `Conflict`.
- FACT: ADR 0019 removed plain `ActionRepo::update`; the only update is `update_from`.
- FACT: single-statement atomicity is used where it matters: enrollment token consumption (`UPDATE ... RETURNING`), audit append (`INSERT ... RETURNING seq`), upserts via `ON CONFLICT`.
- FACT: tests cover two `Store` handles on one file racing (tests/sqlite/concurrency.rs:11-80).

### 2.3 Optimistic concurrency

- FACT: the **only** optimistic guard is the action status string (`update_from`). There is no `version`/`revision` column anywhere (0001..0005).
- FACT: `IncidentRepo::update` is an unconditional `UPDATE ... WHERE id = ?` that rewrites every column and `body` (incidents.rs:155-179). Safety rests entirely on the single-writer manager (ADR 0011).
- FACT: notes `replace` is read-then-upsert with no version check (hub api/notes.rs:171-181): `get` for existence, then `upsert` overwriting. Two UI edits last-write-wins. `notes_import` and host profile `refresh` also upsert unconditionally (hub profile/mod.rs:236-237). The profile refresh and a user edit of the same note (profile note id is stable, :226-228) can overwrite each other.
- FACT: `discard_pending` is a real transaction (read status, delete links, delete incident) (incidents.rs:271-302). Because the writer pool has one connection and `begin()` is deferred, the read-then-write inside it is safe only because no other writer connection exists in-process.
- INFERENCE: the in-process pool of 1 is the real mutual exclusion for every read-modify-write in the store. A second hub process on the same file, or any future increase in the writer pool, would turn those into lost-update/`SQLITE_BUSY` risks (a deferred transaction that reads then upgrades to write can fail with BUSY_SNAPSHOT in WAL, which busy_timeout does not retry).

### 2.4 Cross-repository multi-step writes are not transactional

FACT. The store exposes one repo per aggregate; engine code composes them, with no shared transaction:

- Incident manager applies `decision.writes` one by one (`InsertSignal`, `InsertIncident`/`UpdateIncident`, `Timeline`) (engine manager/actor.rs:174-181, 254-262). On error it reloads the cache and returns; earlier writes of the same command stay committed. A crash between writes leaves, for example, a signal row without the incident update, or a timeline entry missing. (Only signal + link is atomic: SignalRepo::insert, incidents.rs:67-98.)
- Action decision: `update_from` commits, then `audit.append` runs as a separate statement; failure to append is only logged (engine actions/mod.rs:127-143, `announce`). FACT: the audit log can therefore lack the record of a decision the store holds. The audit-based loop guard reload (recover.rs:140-194) then undercounts after restart.
- `Store::purge` runs six independent DELETEs, each its own implicit transaction (purge.rs:330-372).

INFERENCE: today these are tolerable because each record is a self-contained snapshot (`incidents.body`). They become real consistency bugs the moment two stores must agree (see 5.4).

## 3. Retention, pruning, backup, restore, recovery

### 3.1 Retention

FACT (purge.rs:287-296 defaults; schedulers.rs:353 calls `RetentionPolicy::default()` daily): signals 30 d, incident timeline 90 d, triage cache 90 d, resolved advisories 30 d, ended maintenance windows, expired sessions. Never purged: `incidents`, `audit_log`, `digest_facts`, `llm_usage`, `notes`, `baselines`, `actions`, `notify_state` (rows for resolved incidents).
- FACT: the policy is not configurable (hard-coded `default()` at the call site). ARCHITECTURE.md s9 says "signals 30 days, transcripts 90 days" and "incidents and audit kept"; the code purges **incident timeline entries** at 90 d (an "incidents kept" claim that silently thins the timeline of old incidents). INFERENCE: doc and code disagree on what is retained; the UI will show an incident with an emptied timeline after 90 days.
- INFERENCE: `incidents.body` JSON, `audit_log.detail`, `actions.args` and `digest_facts` grow without bound. At homelab volume this is small (numbers in 5.6).
- FACT: no `PRAGMA incremental_vacuum` / `auto_vacuum`; space freed by purge is reused but the file does not shrink except through `VACUUM INTO` backups.

### 3.2 Backup

- FACT: `VACUUM INTO` before any pending migration, named `pre-migrate-<time>.db` (db.rs:127-144, backup.rs:7-17). Pre-migrate backups are **never pruned** (prune only matches `domovoi-*.db`, backup.rs:34-43, 54).
- FACT: nightly `VACUUM INTO` via the writer connection, keeps newest 7 (backup.rs:27-43; scheduler DailyJob::Backup, schedulers.rs:362-367). It holds the single writer connection for the whole copy, so hub writes queue (busy-wait on the pool) during the backup. INFERENCE: seconds for tens of MB; grows linearly.
- FACT: backups are not verified (no open/`quick_check`), not encrypted, stored next to `hub.db` by default (`<data_dir>/backups`), so a disk loss loses both. `secret.key` and `tls/` are separate and documented (docs/book/src/reference/backup.md:12-31).
- FACT: failure is only `tracing::warn!` (schedulers.rs:364-366); no advisory is raised for a failed backup. The advisory kinds enum has no `backup_failed` (0004).

### 3.3 Restore

- FACT: no restore command or API. Docs say copy a backup into the data dir (docs/book/src/install/upgrading.md:42, reference/backup.md:17). No post-restore reconciliation: agents keep their spool and reconnect; restored DB may be older than agents' acknowledged `signals_ack`, so a restore silently loses acknowledged signals (INFERENCE from agent spool semantics, reference/agent.md:46).
- RECOMMENDATION: add `domovoi hub restore <file>` that runs `quick_check`, refuses newer-than-binary schema, and raises a "restored from backup at T" advisory.

### 3.4 Recovery on startup

- FACT incidents: manager `reload()` pages all `active = true` incidents into memory (actor.rs:119-137). Ticker drives TTL/re-notify (actor.rs:285-310). No separate "in-flight" state beyond the incident status + timeline.
- FACT actions (ADR 0014/0019, engine actions/recover.rs): at service start (a) the executor scans `status = approved` and starts them; (b) `fail_abandoned` fails everything still `dispatched` with reason "the hub restarted while this ran; check the host before running it again" through the guarded write; (c) the loop guard is rebuilt from `audit_log` dispatch rows of the last hour. Scans retry 3x and report errors rather than a shortened list (recover.rs:20-53).
- INFERENCE: `approved` actions run after restart; `dispatched` ones are conservatively failed. This is the correct bias for non-idempotent actions, and is the exact precedent the design pack's "resume without repeating a non-idempotent action blindly" requirement (02:63) needs. Gap: it relies on audit rows that may be missing (see 2.4).
- FACT: no startup `integrity_check`, no detection of a half-applied multi-write sequence (2.4).
- FACT: migrations run at open, after the pre-migrate backup; failure returns `StoreOpenError::Migrate` and leaves the backup (db.rs:154-157). sqlx runs each migration file in its own transaction (INFERENCE from sqlx behaviour, not asserted in repo); 0003/0004 are table rebuilds and are safe because `advisories` has no inbound FK.

## 4. Existing search, notes, history, suggestions

- FACT FTS5 with `unicode61 remove_diacritics 0`, external-content tables synced by triggers: `notes_fts(title, body, tags)` and `incidents_fts(title, summary_text)` (0001:56-74, 198-216). `Store::open` probes for FTS5 and fails with `NoFts5` otherwise (db.rs:150-153).
- FACT user text never reaches FTS5 as syntax: words split on non-alphanumerics, each quoted, ANDed (fts.rs:1-30). Consequences: no prefix search, no phrase search, no stemming, no OR, no typo tolerance.
- FACT notes search: `bm25(10,1,1)` weighted title boost, `snippet()` on body, filter `host_id IS NULL OR host_id = ?`, then re-ranked in Rust (notes.rs:129-160). Candidate cap `MAX_CANDIDATES`.
- FACT incident search used by triage ("similar incidents") queries `incidents_fts` **title column only** (`{title} : (...)`, incidents.rs:340-354). `summary_text` is just `check_key + target` (incidents.rs:52-57), so only title is searched in practice. Triage context also lists the last 50 incidents and the host's notes (engine triage/context.rs:139, 172, 201, 233).
- FACT incident list "text" filter uses `LIKE %x% ESCAPE '!'` on title/check_key/target (incidents.rs:241-242), not FTS: a full scan with a leading wildcard.
- FACT notes: flat table, tags JSON, optional `host_id`; no entity link, no history. Host profiles are notes tagged generated, id kept stable across refreshes (hub profile/mod.rs:226-237), plus JSON in `settings`.
- FACT history in practice: `incident_events` (timeline), `audit_log`, `triage_cache`, `incidents.body` (verdict, follow-ups, triage). No "experience" abstraction.
- FACT suggestions: ADR 0016 is **Proposed** (docs/adr/0016-improvement-suggestions.md:3); it plans `AdvisoryKind::Suggestion` with a typed payload on the `advisories` table. grep of `S/src` and ports finds no suggestion repo; the advisory CHECK in 0004 has no `suggestion` kind. INFERENCE: not implemented; adding it needs another CHECK-widening rebuild of `advisories` (the 0003/0004 pattern).
- FACT: knowledge-like concepts live as separate unrelated tables: `baselines`, `ack_rules`, `maintenance_windows`, `digest_facts`, `notes`, profile JSON. S/src/repos/knowledge.rs holds baselines, ack rules, windows, advisories, digest facts; notes are in notes.rs. The design pack itself warns not to overload `knowledge.rs` (07:Migration approach 2).

## 5. Evaluation of the proposals

### 5.1 Relational core (keep SQLite, relational for query-critical state)

- JUSTIFIED. FACT: it already exists and works; ADRs 0005/0011/0019 fit the homelab. 07 recommends the same.
- CAVEAT (FACT): the "relational core" is currently half-relational: incidents and signals are JSON blobs with index columns, actions have a JSON status filtered through `json_extract` (actions.rs:533, 579-580, 630), incident group filter reaches into `$.verdict` (incidents.rs:235). The pack's "keep stable/query-critical structure relational" conflicts with the store's own practice.
- RECOMMENDATION: before adding new aggregates, decide the pattern once: (a) relational columns for anything filtered/joined, JSON `body` for the rest, plus a `status_type` generated column instead of `json_extract(status,'$.type')` scans; (b) add a `version INTEGER` column to every mutable aggregate that a human or two tasks can edit (notes, incidents if the manager ever shares ownership, new knowledge rows) and make updates `WHERE id=? AND version=?`.

### 5.2 Typed entity/relationship graph

- PARTLY JUSTIFIED. The pack's own text says "start with a small set" (09 Q1.4) and "no graph DB" (09 Q2.2). INFERENCE: entities/edges are justified only for the use cases that need traversal: "what depends on this container / what breaks if this host goes down", used by triage context and blast-radius in policy. If every use is 1-2 hops filtered by host, a plain `facts` table with `subject_kind/subject_ref` and a `depends_on` predicate is sufficient.
- Simpler alternative: no `entities` table at first. Reuse existing ids (`host_id`, container name from `baselines(kind,key)`, skill name) as a typed reference `(kind, ref)`; add `relationships(src_kind, src_ref, type, dst_kind, dst_ref, ...)`. Introduce `entities` only when something needs attributes or lifecycle of its own (a service that is not a container/unit/port).
- RISK (INFERENCE): baselines already model "container/unit/port exists on host" with `learned_at/last_seen_at`; a new `entities` table duplicates that state. Two sources of truth for "the host runs service X" must be reconciled (who writes, who wins).
- Recursive CTE traversal on SQLite is adequate at this scale (5.6). The bounded-depth/nodes contract in 07 is the right guard and is independent of storage.
- RECOMMENDATION: make `entities` a derived/linked projection of existing sources first (hosts, baselines, containers from agent profile) with `source` + `last_seen_at`, and allow user-created entities later. Do not migrate baselines into it in v1.

### 5.3 Knowledge facts with provenance

- JUSTIFIED in direction (explainability, correction, "never turn inference into confirmed fact"), but the proposed `knowledge_facts` columns (07 schema) are heavy: source_type, source_ref, confidence, status (5 values), valid_from/until, observed_at, last_verified_at, supersedes_id, created/updated. INFERENCE: for a homelab operator the useful subset is `subject, predicate, value, source{user|agent|integration|inferred}, status{current|stale|retracted}, last_verified_at, supersedes_id`. Confidence should be an enum, not a number (06 itself says model confidence is not calibrated).
- Consistency risk (INFERENCE): facts duplicate data that already lives in `notes` (free text), host-profile notes + `settings` JSON, `baselines`, `digest_facts`. Without a stated owner per fact kind the UI shows contradictory values. 06 says "preserve provenance, surface conflict"; that needs a deterministic conflict rule (same subject+predicate, different value, both current) enforced by a partial unique index or a service check, not by convention.
- FACT constraint from AGENTS.md: facts must never grant permission; the pack separates Policy from Knowledge, which maps cleanly to keeping `knowledge_facts` free of any column the approval path reads. RECOMMENDATION: enforce with a gate test (nothing in `domovoi-engine::actions` or `auth` imports the fact repo).
- Deletion contract (06 "Deletion must remove from search indexes, graph edges, caches"): with FTS content-tables kept by triggers (as notes do today) deletion is atomic in one transaction. That argues for triggers + same-DB FTS, and against a second index store.
- Retraction vs delete: 07 wants "supersede/retract, not overwrite" and 06 wants true deletion that leaves no sensitive content. These conflict; pick: `retracted` keeps tombstone (id, subject, predicate, retracted_at, by) with `value_json = NULL`.

### 5.4 Append-only experience events

- JUSTIFIED as concept; DUPLICATES existing structures. FACT: `incident_events` (timeline), `audit_log`, `triage_cache`, and the incident `body` already hold "what happened". The proposed `experience_events(id, incident_id, event_type, phase, actor, occurred_at, summary, payload_json, correlation_id)` is a third log.
- Dual-write risk (FACT/INFERENCE): the codebase already shows the failure mode. Action state is written, then audit is appended non-atomically and failure is only logged (engine actions/mod.rs:127-143); the incident manager writes incident row, timeline row and signal separately (actor.rs:174-181). Adding experience events written next to each of those multiplies the windows where relational state and the log disagree. Because the pack wants "notifications derived from committed state transitions" (02:63), the log must be the thing that commits atomically with the state.
- RECOMMENDATION (preferred): do not create a third log. Extend `incident_events` (it already has `seq`, `incident_id`, `kind`, `summary`, `detail`) with `phase`, `actor`, `correlation_id`, and an `event_entity_links` table; keep `audit_log` as the immutable security record. Stop deleting timeline rows at 90 d, or move expiry to `detail` redaction (null the payload, keep the row), since an "experience" archive that deletes its own events cannot serve "what happened last time".
- RECOMMENDATION: write state change and its event in **one transaction**. This needs a store-level unit-of-work (e.g. `Store::in_tx(|tx| ...)` or domain methods like `ActionRepo::transition_with_audit(action, expected, audit_entry)`), because the engine today cannot span repos. The writer tasks (ADR 0011/0019) stay as the sole callers, so the single-writer property is preserved while the dual write becomes one commit. Doing this also fixes the existing audit gap in 2.4.
- Do not make the event log the source of truth (event sourcing) here: the incident manager is a pure `(state, command) -> (state, events)` over snapshots (ADR 0011), and replay would require every historical event to carry full payloads. Keep snapshots authoritative; events are derived and append-only.
- Retention: 06/09 Q1.6 say "metadata, summaries, references and hashes; raw only within existing bounded retention". That fits `detail` redaction without row deletion.

### 5.5 Vector search

- NOT JUSTIFIED now; the pack agrees (09 Q2.1: "No initially"; 07: "only when evaluation shows lexical/graph retrieval is insufficient"). FACT: current FTS is the weakest form (AND of exact tokens, no prefix/stem, title-only for incidents), so "lexical is insufficient" is not yet proven; it is under-built.
- Cheaper steps first (RECOMMENDATION): index `summary_text` and a bounded body excerpt for incidents; add prefix queries (`word*` on the last token) and OR-fallback when AND returns nothing; add tokenizer `porter unicode61` or trigram for identifiers like `pool_degraded` (current words() splits on `_`); add a search eval set (20 real queries and expected hits) so a later vector decision is measured.
- If it ever proceeds: SQLite options are `sqlite-vec` (loadable extension; conflicts with bundled-SQLite/sqlx static build and with the "rustls only, nothing external" posture, needs a C extension and a model); or brute-force cosine over BLOB vectors in Rust (adequate to ~100k vectors). Embedding requires either an LLM-provider embedding call (sends note/incident text off-host, a privacy decision) or a local model (new heavy dependency). Embeddings must be derived data keyed by `(content_hash, model_id)` so deletion and re-embed are cheap; deletion workflow must purge them (07 requires this).
- Keep behind a `SearchPort`; no schema now.

### 5.6 Is SQLite sufficient? (workload numbers)

INFERENCE from retention + ADR statements; no measurement exists in the repo.
- ADR 0011 states "hundreds of signals per minute" as ample. Signals purged at 30 d; at a pessimistic 100/min that is 4.3 M rows (~1-2 KB with JSON body => ~4-8 GB), unrealistic for 1-20 hosts; a realistic rate (one signal per state change, flapping aside) is a few thousand per day => <0.2 M rows / 30 d.
- digest_facts: hourly per agent (agent/scheduler.rs:35) x say 10 keys x 10 hosts => 2,400/day => ~0.9 M rows/yr (~100 MB with indexes). Unbounded; worth a retention row.
- Incidents: dozens per month; `incidents.body` KBs each. Audit: hundreds of rows per day at most. Notes: tens to hundreds.
- Proposed knowledge: entities ~ (hosts x (containers+units+ports+filesystems)) = 10 x 100 = ~1,000-5,000; relationships ~ 2-3x; facts ~ 10-50 per entity => tens of thousands of rows. Experience events: a few hundred per incident x dozens of incidents => tens of thousands per year. All trivially within SQLite and recursive CTE reach (depth <= 4, node cap ~200).
- Write concurrency: single writer connection with WAL; contention is dominated by the nightly `VACUUM INTO` backup (blocks the writer connection) and the incident burst when many agents reconnect. Fine at this scale. Becomes a problem only if (a) per-host high-frequency metrics are stored, (b) a second hub process shares the file, or (c) the writer pool grows without version guards (2.3).
- VERDICT: SQLite is sufficient for the target scale with a large margin, including FTS5 and a graph of ~10^4 nodes. Do not add Postgres or a graph DB.

## 6. Migration risks

### 6.1 Data that must not be lost
- `audit_log` (immutable record; triggers block UPDATE/DELETE, so a table rebuild migration cannot copy+drop it without temporarily dropping the triggers; any change must be additive: new columns via ALTER ADD COLUMN, new tables).
- `actions` rows with approvals (`HumanApproval::restore` path, ADR 0014; column CHECK `(approved_by IS NULL) = (approved_at IS NULL)`). Any restructuring must keep `plan_hash`, `args`, approval triple and `chat_token_hash` byte-identical, since dispatch re-validates against the stored plan hash (ADR 0014 consequences).
- `incidents.body` JSON: serde-shaped state, decoded with `from_json`. FACT: there is no body schema version field. Changing the `Incident` struct requires serde defaults or a migration that rewrites bodies in SQL (hard with `json_set` across variants like `Muted{..}`). RECOMMENDATION: add `schema_version` to bodies before the orchestration work lands.
- `notes` (user-authored; no history, so a bad migration loses text irrecoverably), `ack_rules` (rows before 0005 have NULL creator), `baselines` (learned state; re-learnable but noisy), `users`/`sessions`/`settings` (encrypted secrets; unreadable without `secret.key`).
- `incident_events` older than 90 d are already gone; do not promise historical experience for old incidents.

### 6.2 Hard migrations
- FACT: widening an enum CHECK means rebuilding the table (0003, 0004 pattern). Adding `suggestion`, `backup_failed`, `knowledge_stale` advisories repeats this; each rebuild must also recreate indexes. RECOMMENDATION: move advisory `kind` validation out of SQL (validate in Rust) in the next rebuild, so future kinds need no migration.
- FACT: `incidents_fts` and `notes_fts` are external-content tables tied to `rowid` with triggers. Any rebuild of `incidents` or `notes` (new PK, new columns by copy) changes rowids unless explicitly preserved and requires `INSERT INTO fts(fts) VALUES('rebuild')`. Prefer `ALTER TABLE ADD COLUMN` and new tables.
- FACT: R13 forbids editing merged migrations; the repo has no down migrations. Combined with `pre-migrate` backups never pruned (3.2), rollback is "restore the backup" only. An older binary opening a DB migrated by a newer one fails (sqlx version mismatch; INFERENCE).
- Renaming `knowledge.rs` concepts: the Rust module name collides with the new "knowledge" service (07 migration step 2); a code-only concern, but port names (`BaselineRepo`, `AckRepo`, `FactRepo`: the last means *digest* facts) will clash with new "facts". RECOMMENDATION: rename `FactRepo`/`digest_facts` to `MetricSampleRepo`/`host_metrics` in a code-only change before adding `knowledge_facts`.

### 6.3 Mapping existing rows to the new model

| Existing | Target | How |
| --- | --- | --- |
| `hosts` | `entities(kind=host)` | Keep `hosts` as the canonical table; entity row keyed by the same UUID (no new id), created lazily. No data movement. |
| `baselines(container/unit/port)` | `entities` + `runs_on` relationship, `source=agent-discovered`, `last_verified_at=last_seen_at` | Projection job, not a migration; baseline stays authoritative for alerting. |
| `digest_facts` | not knowledge | Time-series samples; leave as is. Do not map into `knowledge_facts`. |
| host profile (note + `settings` JSON) | facts with `source=agent-discovered` + the note remains | Parse profile sections into facts only when the structured shape is stable; keep note. |
| `notes` | stay; add `note_links(note_id, entity_kind, entity_ref)` | Additive; FTS untouched. |
| `ack_rules`, `maintenance_windows` | stay policy/operational state | The pack puts "known conditions" under Policy/Knowledge ambiguously; decide explicitly. They gate notification, so treat as Policy. |
| `incident_events` | experience events (extend in place) | ALTER ADD COLUMN `phase/actor/correlation_id`; null for old rows. |
| `audit_log` | stays | Never merged into experience. |
| `triage_cache`, `llm_usage` | stay | Derived/cost data. |

INFERENCE: nothing needs an in-place transformation of existing rows if the new model is additive and linked by id. That is the lowest-risk path and matches 07 migration steps 1-5.

## 7. Open questions

1. Who is the source of truth when a fact (e.g. "service X depends on Y") disagrees between a user-entered fact, an agent-discovered baseline and a host-profile note? Needs a precedence order (user > integration > agent > inferred) and a conflict state, not newest-wins.
2. Should experience events replace the incident timeline (single table) or sit beside it? This decides whether retention can keep deleting `incident_events` at 90 d. The existing code and ARCHITECTURE.md s9 disagree on what "incidents kept" covers.
3. Is a cross-repo unit-of-work acceptable under R1/R2 (store and engine ports)? `domovoi-engine` has no I/O and talks to repos through `domovoi-core` ports; a transactional port needs an ADR (new port shape), e.g. domain-level combined methods rather than exposing a transaction handle to the engine.
4. Is `synchronous = NORMAL` acceptable once orchestration persists phases, deadlines and operation states that gate non-idempotent actions after a crash? Using FULL costs little at this write rate.
5. Where do durable operation/phase records (02 "Operations") live: new tables with `version` guard and a writer task like ADR 0011/0019, or inside `incidents.body`? The pack says operations are independent of incidents, so new tables plus a third single-writer.
6. Backup policy: encrypted/offsite target, verification, failure advisory, retention for `pre-migrate-*` files, and whether the new stores (embeddings) are rebuilt from source rather than backed up.
7. Is a second hub process on the same SQLite file a supported topology (the concurrency test assumes it works)? If not, document single-process and take an advisory file lock at open.
8. Embedding privacy: may note/incident text leave the host for embeddings? Required before any vector work.
9. Does the audit log need hash chaining (tamper evidence) if it becomes the compliance record for autonomous actions? Today only triggers protect it, and anyone with file access can drop them.
10. Deletion semantics: true erase (including backups, which carry the old content for up to 7 nights plus all pre-migrate files) versus tombstone. 06 requires deletion from "caches and retrieval paths" but backups are not mentioned.

## Key code references (for follow-up)

- Pools and PRAGMAs: `crates/domovoi-store/src/db.rs:88-96, 145-162`
- Backup/prune: `crates/domovoi-store/src/backup.rs:27-43`; scheduler `crates/domovoi-hub/src/runtime/schedulers.rs:353-367`
- Retention: `crates/domovoi-store/src/purge.rs:287-372`
- Guarded action write: `crates/domovoi-store/src/repos/actions.rs:528-557`
- Non-atomic state + audit: `crates/domovoi-engine/src/actions/mod.rs:127-143`
- Non-atomic incident writes: `crates/domovoi-engine/src/manager/actor.rs:174-181, 254-262`
- Startup recovery: `crates/domovoi-engine/src/actions/recover.rs`; `crates/domovoi-engine/src/manager/actor.rs:119-137`
- Notes read-modify-write: `crates/domovoi-hub/src/api/notes.rs:171-181`; profile: `crates/domovoi-hub/src/profile/mod.rs:226-237`
- FTS: `crates/domovoi-store/src/fts.rs`, `repos/notes.rs:129-160`, `repos/incidents.rs:340-354`
