# P10 — Store: SQLite persistence

| | |
| --- | --- |
| Phase | 3, Hub core |
| Depends on | P03 |
| Crates | `domovoi-store` |
| Size | L |
| Invariants touched | INV-7 (append-only audit log), R13 (append-only migrations) |

## Goal

Implement every repository port from P03 on SQLite with `sqlx`, with embedded migrations, FTS5 search, an append-only audit log enforced by the database, backups, retention, and proof via the testkit contract suites that the SQLite adapter behaves exactly like the fakes.

## Read first

- `ARCHITECTURE.md` §9, INV-7, R13
- ADR 0005
- P03 ports and `domovoi-testkit::contracts`

## In scope

Add `sqlx` (workspace features), `tokio`, `serde`, `serde_json`, `thiserror`, `jiff`, `uuid`, `async-trait`, `tracing`. Dev: `domovoi-testkit`, `tempfile`, `tokio` (`macros`, `rt-multi-thread`).

### Opening (`db.rs`)

- `Store::open(path: &Path, opts: StoreOptions) -> Result<Store, StoreError>`:
  1. Create the parent directory with mode 0700.
  2. If the database exists and has pending migrations, run `VACUUM INTO '<backup_dir>/pre-migrate-<timestamp>.db'` first.
  3. Connection options: WAL journal, `synchronous = NORMAL`, `foreign_keys = ON`, `busy_timeout = 5s`; pool of 4 connections; a separate single-connection pool for writes is acceptable if it simplifies `SQLITE_BUSY` handling.
  4. Run `sqlx::migrate!("./migrations")`.
- `Store::backup_to(dir) -> Result<PathBuf>` (`VACUUM INTO`, keeps the newest `keep` files), `Store::purge(now, policy)` for retention (signals 30 d, timeline entries and triage transcripts 90 d, resolved advisories 30 d; incidents and audit kept).
- Verify at startup that FTS5 is available (`CREATE VIRTUAL TABLE IF NOT EXISTS temp.fts_probe USING fts5(x)`); fail with a clear message otherwise.

### Schema (`migrations/0001_init.sql`)

Tables from `ARCHITECTURE.md` §9. Conventions: IDs `TEXT PRIMARY KEY`, timestamps `INTEGER` unix milliseconds UTC, structured fields as `TEXT` JSON, enums as `TEXT` with `CHECK` constraints. Indexes for every repository query (fingerprint + status, host, created_at, status). FTS5:

- `notes_fts(title, body, tags)` with `content='notes'` and triggers on insert/update/delete.
- `incidents_fts(title, summary_text)` maintained by triggers.
- `audit_log` with triggers `BEFORE UPDATE` and `BEFORE DELETE` that `RAISE(ABORT, 'audit_log is append-only')`.

### Repositories (`repos/*.rs`, one file per port)

- Map between core types and rows explicitly (`FromRow` structs per table, conversion functions); no core type derives `sqlx` traits (core stays I/O-free).
- Queries are runtime-checked `sqlx::query_as` (ADR 0005). Keep SQL in `const` strings next to the function that uses it.
- Cursor pagination: opaque base64 of `(created_at, id)`; stable ordering newest first.
- `consume_enrollment_token` is a single `UPDATE ... WHERE token_hash = ? AND used_at IS NULL AND expires_at > ? RETURNING host_id` so it is atomic.
- `NoteRepo::search` uses `bm25` ranking and `snippet()`; input is sanitised into an FTS query (quote each term; never pass raw user syntax).

## Out of scope

- Engine logic, HTTP. Multi-node or remote databases.

## Tests required

In `crates/domovoi-store/tests/`, each test using a fresh temp database:

1. Run every `domovoi_testkit::contracts::*` suite against the SQLite store (`contracts.rs`). All must pass unchanged.
2. `audit_log_rejects_update_and_delete` (raw SQL `UPDATE`/`DELETE` fail).
3. `migrations_run_on_empty_database_and_are_idempotent`.
4. `backup_is_taken_before_pending_migrations` (simulate by creating a database at an older migration set using a test-only migrator with only the first file, if feasible; otherwise test `backup_to` and the decision function separately).
5. `fts_search_ranks_title_matches_first_and_sanitises_operators` (queries like `"NEAR(" OR` do not error).
6. `enrollment_token_consumption_is_atomic_under_concurrency` (two tasks race; exactly one wins).
7. `pagination_is_stable_with_equal_timestamps`.
8. `purge_respects_retention_policy`.
9. `timestamps_round_trip_with_millisecond_precision`.
10. `busy_database_retries_instead_of_failing` (parallel writers).

## Architecture gates

- `domovoi-store` depends only on `domovoi-core` internally (R1).
- `cargo xtask arch-check --base origin/main` in CI will reject later edits to `0001_init.sql` (R13). Get the schema right now; later prompts add new migration files.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/reference/data-model.md` describes every table in one or two lines and the retention policy.

## Deliverables

- `crates/domovoi-store/migrations/0001_init.sql`
- `crates/domovoi-store/src/{db,backup,purge,repos/*,rows/*,error}.rs`
- `crates/domovoi-store/tests/*`, `docs/book/src/reference/data-model.md`

## PR

Title: `feat(store): SQLite repositories with FTS5, append-only audit log and backups`
