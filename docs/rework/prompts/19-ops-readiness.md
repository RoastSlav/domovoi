# 19 — Operations readiness

| | |
|---|---|
| Branch | `ops-readiness` |
| Milestone | F4 |
| Depends on (must be merged) | 08 (store durability settings, `quick_check` at open, migration numbering) |
| Can run in parallel with | 17, 18, 57 |
| Produces (shared contracts) | `crates/domovoi-hub/src/settings/step_up.rs` (the declarative step-up list); `StoreHealth` port; `GET /readyz`; `[metrics] token`; `verify_backup`; `restore_from_backup` and `domovoi restore`; `StoreOptions.keep_pre_migrate`; `AdvisoryKind::BackupFailed` |
| Consumes | Prompt 08's `quick_check` and migration conventions (verify names in code). Prompt 09's startup audit verification result, **only if it is already merged and exposed** (see `/readyz`). |
| Launch gate | none |
| Suggested model | medium |

## Objective

Close the operational gaps the foundation work left open, each small and testable:

1. **Step-up list.** Replace the hard-coded `is_sensitive` conditions with a **declarative list** of settings (and declared future actions) that need password re-entry, covering `llm.<slot>.cli.*` (command, args, preset), and a test that fails when a new CLI setting is added without being covered.
2. **`GET /readyz`.** Readiness separate from liveness: store writable, migrations done, the writers alive.
3. **`/metrics` behind authentication**: a web session or a configured bearer token. Secure by default.
4. **Backup verification.** Every nightly backup is verified (`quick_check`, known migrations) before older backups are pruned; failure raises an advisory instead of a log line only.
5. **`domovoi restore <file>`**: verify, move the current database aside, install the backup atomically, migrate forward, never overwrite silently, refuse while a hub is running.
6. **Pre-migrate pruning.** `pre-migrate-*` backups are kept to the newest 3 (default; the setting arrives with configurable retention).

## Architectural context

Plain-terms background (RW-24, RW-28, `04` §14 and §17, finding D24 and D25):

- **Step-up authentication** means: a settings change that widens trust or can run code needs the admin's password typed again, even with a valid session. Today the check is a handful of string comparisons in `is_sensitive` (secrets, `discord.allowed_user_ids`, `web.trusted_proxies`, `webhooks`). It misses `llm.*.cli.command/args`: a stolen admin session could set an arbitrary program for the hub to run (and drop `--tools ""`) with no password. RW-24 requires one declarative list, checked by a test. The list also *names* the later step-up actions (publishing a policy version, enabling a capability, starting an installation, changing retention, turning autonomy back on) so they are documented in one place; only the entries whose features exist today are enforceable now.
- **Liveness vs readiness.** `/healthz` stays a constant `ok` (the process serves HTTP). `/readyz` says the hub can do its job: it can write to the store, all migrations are applied, the incident manager and the action service are running. Orchestrators use it; it reveals only check names, never details.
- **`/metrics`** exposes counters about the whole system and is currently open to anyone who can reach the web port (finding D24). Prometheus scrapers do not hold a browser session, so a bearer token is supported (`[metrics] token`, a secret like any other, so setting it needs step-up). Without a token only a logged-in session can read it.
- **Backups.** Copies are made with `VACUUM INTO` on the single writer connection. Nothing verifies them; failure is only a `tracing::warn!`; pre-migrate copies are never pruned; there is no restore command (finding D25). Deleted data stays in backups until they rotate: nightly 7, pre-migrate last 3 (D-21). Restoring a backup brings deleted data back; the docs say so.
- **Restore** is an explicit offline operator action: it never runs inside the hub, requires that no hub holds the data directory (`hub.lock`), and keeps the previous database next to the restored one.
- This prompt introduces **no new approval path** and touches no action, policy or LLM code.

## Repository context

Verify each item by opening it. Line numbers are from when this brief was written.

**Step-up**

- `crates/domovoi-hub/src/settings/patch.rs:104-117` `is_sensitive(&Change) -> bool`:

```rust
let path = change.path();
matches!(change, Change::Secret { .. })
    || path == "discord.allowed_user_ids"
    || path == "web.trusted_proxies"
    || path == "webhooks"
    || path.starts_with("webhooks.")
```

  `Change::path()` gives the dotted leaf path (`:46-53`); `diff()` reports **leaf** paths (arrays are leaves, tables are walked), so `llm.primary.cli.args` is one leaf. A patch that nulls the whole `cli` table yields leaf changes for each removed `cli.*` key.
- `crates/domovoi-hub/src/settings/service.rs:232-244`: `if patched.changes.iter().any(is_sensitive)` demands the password (`SettingsError::PasswordRequired`, then `auth.confirm_password`). Imported at `:21`.
- Config shape: `crates/domovoi-config/src/hub/llm.rs:27-89` `LlmProvider { kind, model, fallback_models, base_url, api_key, cli: Option<CliConfig{preset, command, args}>, price, timeout }` under `[llm.primary]` and `[llm.secondary]`.
- Existing tests: unit test in `patch.rs:~229-300`; integration `crates/domovoi-hub/tests/hub/settings.rs:189` `sensitive_change_requires_password_and_shares_login_rate_limit` (helper `patch(&a, &login, base, changes, password)` at `:12`).

**Health, readiness, metrics**

- `crates/domovoi-hub/src/api/system.rs:126-157` `healthz()` returns `"ok"`; `metrics(State)` renders `state.deps.metrics` (`Option<MetricsRender>`) with no authentication. Routes registered at `crates/domovoi-hub/src/api/mod.rs:376-377` (`routes!(system::healthz)`, `routes!(system::metrics)`).
- `crates/domovoi-hub/src/api/mod.rs:139-192` `ApiDeps` (already holds `manager: ManagerHandle`, `actions_service: ActionHandle`, `config: Arc<HubConfig>`); built in `crates/domovoi-hub/src/runtime/mod.rs:~279`, `crates/domovoi-hub/examples/dev_hub.rs:488`, `crates/domovoi-hub/tests/hub/api_harness.rs:377`. `Authed` extractor `:247-266` (session via `csrf::session_id(&headers)` and `state.auth.current(id)`).
- `crates/domovoi-hub/src/ingest/auth.rs:~46-60` `pub(crate) fn verify_bearer(secret: &SecretString, header: Option<&HeaderValue>) -> bool` (constant-time SHA-256 compare). Reuse it.
- `crates/domovoi-engine/src/manager/handle.rs:24-30` `ManagerHandle { tx: mpsc::Sender<Envelope>, .. }`; `crates/domovoi-engine/src/actions/mod.rs` `ActionHandle { shared, writer: WriterHandle { tx } }` (`writer.rs:~60`). A closed channel means the task ended.
- Existing tests that read `/metrics` unauthenticated and **must be updated**: `crates/domovoi-hub/tests/hub/api_routes.rs:243` and `:380-403`, `crates/domovoi/tests/single_box.rs:221`. `/healthz` tests: `api_auth.rs:225`, `api_routes.rs:236`.
- Config sections: `crates/domovoi-config/src/hub/mod.rs:28-71` `HubConfig`; `BackupSection` `:354-373`; `Secrets for HubConfig` (`:392-415`, enumerates secrets so they are resolved and redacted); `HUB_SECTIONS: [&str; 13]` in `crates/domovoi-config/src/env.rs:13-27` (top-level tables that take `DOMOVOI_*` overrides). Validation in `crates/domovoi-config/src/validate.rs`.
- Docs: `docs/book/src/reference/cli.md:~169` ("serves Prometheus text, with no login needed"), `docs/book/src/reference/api.md:156-159`.

**Backups**

- `crates/domovoi-store/src/backup.rs`: `BACKUP_PREFIX = "domovoi-"`, `pre_migrate_name(at)` (`pre-migrate-%Y%m%dT%H%M%SZ.db`), `vacuum_into`, `Store::backup_to(dir, now)` (`:43-60`: creates dir, `VACUUM INTO`, then `prune(dir, keep_backups)`), `prune()` (`:63-85`, only `domovoi-*.db`).
- `crates/domovoi-store/src/db.rs:25-47` `StoreOptions { backup_dir, keep_backups, now }` and `StoreOptions::beside(db, now)`; `:~148-168` `Store::open` takes the pre-migrate copy when `needs_backup(applied, available)`; `MIGRATOR` `:15`; `applied_versions` `:100-125`; `connect_options` (WAL; prompt 08 changes `synchronous`). `crates/domovoi-store/src/error.rs` `StoreOpenError { Io{path, reason}, Database, NoFts5, Migrate }`.
- `StoreOptions { .. }` literal sites: `crates/domovoi-hub/src/runtime/adapters.rs:31-43` and `crates/domovoi-hub/src/settings/apply.rs:~118` (and any other: `grep -rn "StoreOptions {" crates`).
- Scheduler: `crates/domovoi-hub/src/runtime/schedulers.rs:~340-365` runs `store.purge(..)` daily at 03:30 and `store.backup_to(&dir, now)` at 04:00 (`DailyJob::Backup`); failure only `tracing::warn!`. `:~236-250` keeps externally managed advisory kinds from being auto-resolved (`TriageDegraded | DeliveryFailed | StoredSecretUnreadable | SettingsApplyFailed`).
- Advisories: `crates/domovoi-core/src/advisory.rs:10-32` `AdvisoryKind`; ids via `domovoi_engine::advisories::advisory_id(kind, host, subject)` (`engine/src/advisories.rs:69`); the `advisories` table has a `CHECK (kind IN (...))` last rebuilt in `crates/domovoi-store/migrations/0004_settings_advisories.sql` (copy that rebuild pattern). Raise/clear pattern: `crates/domovoi-hub/src/settings/apply.rs:131-200` (`raise(...)`, `advisories.resolve(id, now)` ignoring `StoreError::NotFound`).
- Tests: `crates/domovoi-store/tests/sqlite/{migrations.rs,common.rs}` (helper `Fresh::new()`, `needs_backup`, `pre_migrate_name`), `purge.rs`.
- Book: `docs/book/src/reference/backup.md`, `docs/book/src/install/upgrading.md:40`, `docs/book/src/reference/cli.md`.

**CLI**

- `crates/domovoi/src/cmd/mod.rs`: `Command` enum (`Hub`, `Agent`, `Check`, `Notify`, `Selftest`, `Digest`, `Upgrade`, `Version`, hidden `Openapi`, `Schema`), `run()` match, `block_on`, `HubConfigArg` (`--config`, `effective()`), `fail()`. `crates/domovoi/src/cmd/digest.rs` is a minimal subcommand to copy. `is_quiet()` makes every command quiet except `hub run` and `agent run`.
- Lock: `crates/domovoi-hub/src/runtime/lock.rs` `HubLock::acquire(data_dir)` (`LockError::Held`), re-exported from `domovoi_hub::runtime`. `domovoi_hub::admin::AdminError` maps it to `HubRunning` (`admin/mod.rs:~55-62`).
- CLI reference: `docs/book/src/reference/cli.md`.

## Exact scope

Build:

1. `step_up.rs` and the `is_sensitive` rewrite, tests, docs.
2. `StoreHealth` port; `GET /readyz`; `ApiDeps.store_health`; `is_alive()` on the two handles.
3. `[metrics] token` config section; authenticated `/metrics`.
4. `verify_backup`; verification inside `backup_to`; `AdvisoryKind::BackupFailed` with its migration; scheduler raises and clears it.
5. `StoreOptions.keep_pre_migrate` and `prune_pre_migrate`.
6. `restore_from_backup` (store), `domovoi restore` (CLI), hub-level lock wrapper.
7. Docs and the config reference.

Files expected to change or be added:

- hub: `src/settings/{step_up.rs (new),patch.rs,mod.rs}`, `src/api/{system.rs,mod.rs}`, `src/api/readiness.rs` **(new)**, `src/runtime/{mod.rs,schedulers.rs,adapters.rs}`, `src/settings/apply.rs`, `src/admin/mod.rs` (a `restore` entry point), `examples/dev_hub.rs`, tests `tests/hub/{settings.rs,api_routes.rs,api_harness.rs,api_auth.rs}` and a new `tests/hub/readiness.rs` registered in the hub test `main.rs`.
- core: `src/ports/store_health.rs` **(new)** (or in an existing ports file) + re-export; `src/advisory.rs`.
- store: `src/backup.rs`, `src/restore.rs` **(new)**, `src/db.rs`, `src/error.rs`, `src/lib.rs`, `migrations/NNNN_backup_failed_advisory.sql` **(new)**, `impl StoreHealth for Store`; tests `tests/sqlite/{backup.rs (new),restore.rs (new),migrations.rs}` + `main.rs`.
- testkit: `InMemoryStore: StoreHealth`.
- engine: `ManagerHandle::is_alive`, `ActionHandle::is_alive` (`src/manager/handle.rs`, `src/actions/mod.rs`).
- config: `src/hub/mod.rs` (`MetricsSection`, `Secrets` visit), `src/env.rs` (`HUB_SECTIONS` gains `"metrics"`; bump the array length), tests; regenerate `docs/book/src/reference/configuration.md`.
- domovoi (bin): `src/cmd/{mod.rs,restore.rs (new)}`; test for argument parsing.
- web: `src/api/openapi.json` and `schema.d.ts` regenerated (new `/readyz` operation, `/metrics` 401). No hand-written web code.
- docs: see Documentation requirements.

## Explicit non-goals

- No LLM, approval, policy, action or protocol change. No new `HumanApproval` constructor.
- No change to the web password flow, login throttle, session handling or `confirm_password`.
- No enforcement of step-up for features that do not exist yet (policy publication, capability enablement, installation, autonomy). They are only **declared** in the list.
- No `retention.*` configuration keys or purge changes (that is the configurable-retention work). The step-up list already names the `retention` prefix so that work needs no change here.
- No `llm.*.base_url` step-up (RW-24 does not list it). Mention it in the completion report as an observation for the owner.
- No backup encryption, no off-host copy, no change to the nightly schedule (04:00) or to `backup.keep`.
- No change to `/healthz` behaviour (it stays a constant `ok`).
- No audit verification code. `/readyz` reports audit status only if the audit-chain prompt already exposes a startup result.
- No automatic restore, no restore from inside the running hub or the web UI.
- No `docker-compose`/Dockerfile healthcheck change (documented only).

## Required interfaces and contracts

### Step-up list (**fixed** content, **convention** shape)

`crates/domovoi-hub/src/settings/step_up.rs`:

```rust
/// How a rule picks settings paths. Paths are dotted leaf paths such as `llm.primary.cli.args`.
pub enum PathRule {
    /// Exactly this path.
    Exact(&'static str),
    /// This path or anything under it. `*` stands for exactly one segment.
    /// `llm.*.cli` covers `llm.primary.cli.command`, `llm.secondary.cli.args`, ...
    Under(&'static str),
}

/// One reason a settings change needs the password again.
pub struct StepUpRule { pub path: PathRule, pub why: &'static str }

/// Settings whose change needs the password. Every entry is enforced.
pub const STEP_UP_SETTINGS: &[StepUpRule] = &[
    StepUpRule { path: PathRule::Exact("discord.allowed_user_ids"), why: "widens who can approve actions from chat" },
    StepUpRule { path: PathRule::Exact("web.trusted_proxies"),      why: "changes which client addresses the hub trusts" },
    StepUpRule { path: PathRule::Under("webhooks"),                 why: "webhook secrets and sources decide who can raise incidents" },
    StepUpRule { path: PathRule::Under("llm.*.cli"),                why: "the program (and its arguments) the hub runs as the model" },
    StepUpRule { path: PathRule::Under("metrics"),                  why: "who can read the hub's metrics" },
    StepUpRule { path: PathRule::Under("retention"),                why: "changes how long data is kept; applies once configurable retention exists" },
];
// Any `Change::Secret` is also sensitive (set or clear of any stored secret), as today.

/// Actions that need the password again when their features exist. Declared only:
/// no code enforces them yet, and each says what must exist first.
pub const STEP_UP_ACTIONS: &[StepUpAction] = &[
    StepUpAction { id: "policy.publish",         applies_once: "policy versions exist" },
    StepUpAction { id: "capability.enable",      applies_once: "capability packages exist" },
    StepUpAction { id: "installation.start",     applies_once: "package installation exists" },
    StepUpAction { id: "autonomy.resume",        applies_once: "autonomy (the pause switch) exists" },
];
pub struct StepUpAction { pub id: &'static str, pub applies_once: &'static str }

/// Whether `path` is covered by `rule`.
pub fn covers(rule: &PathRule, path: &str) -> bool;
/// Whether changing `path` needs the password (any STEP_UP_SETTINGS rule covers it).
pub fn path_needs_step_up(path: &str) -> bool;
```

`covers` compares segment by segment: `Exact` needs equal segment lists; `Under(prefix)` needs the path to have at least as many segments as the prefix, each prefix segment equal or `*`. `webhooks` under means `webhooks` and `webhooks.<name>.secret`. `patch::is_sensitive` becomes `matches!(change, Change::Secret{..}) || path_needs_step_up(change.path())`. Keep `is_sensitive` public with the same signature. Add `pub mod step_up;` to `settings/mod.rs`.

### Readiness (**fixed**)

```rust
// core: crates/domovoi-core/src/ports/store_health.rs
#[async_trait]
pub trait StoreHealth: Send + Sync {
    /// Whether a write transaction can begin and roll back (the single writer is free and the disk accepts writes).
    async fn writable(&self) -> Result<(), StoreError>;
    /// Whether every embedded migration has been applied successfully.
    async fn migrations_current(&self) -> Result<bool, StoreError>;
}
// engine: ManagerHandle::is_alive(&self) -> bool  == !self.tx.is_closed()
//         ActionHandle::is_alive(&self) -> bool   == !self.writer.tx.is_closed()
// hub: ApiDeps gains `pub store_health: Arc<dyn StoreHealth>`
```

`GET /readyz` (no login, like `/healthz`), operation id `readyz`, tag `system`:

```json
200 {"ready":true,"failing":[]}
503 {"ready":false,"failing":["store_writable","action_service"]}
```

Check names (a fixed vocabulary, `&'static str`): `store_writable`, `migrations`, `incident_manager`, `action_service`. Evaluate every check even when an earlier one fails. A `StoreHealth` error counts as failing that check. Add a fifth check name `audit` **only** if prompt 09's startup verification result is exposed by merged code (find it with `grep -rn "audit.*verif" crates/domovoi-hub/src crates/domovoi-store/src`); if absent, omit and say so in the report. Do not implement audit verification here. Response type: `ReadyzBody { ready: bool, failing: Vec<String> }` with `ToSchema`. Details (error text) go to `tracing::warn!` only, never to the response.

`impl StoreHealth for Store`: `writable`: acquire the writer connection, `BEGIN IMMEDIATE`, then `ROLLBACK` (never commits anything). `migrations_current`: read `_sqlx_migrations` (`success = 1`) and compare to `MIGRATOR` versions (reuse `applied_versions` logic on the open pool, not a new connection to the file).

### Metrics authentication (**fixed**)

Config (hub.toml): 

```toml
[metrics]
# Bearer token for Prometheus. Without it only a logged-in web session can read /metrics.
token = { env = "DOMOVOI_METRICS_TOKEN" }
```

`MetricsSection { token: Option<Secret> }` (`#[serde(default)]`, `deny_unknown_fields`, doc comments), field `pub metrics: MetricsSection` on `HubConfig`, visited in `Secrets::secrets_mut` as `"metrics.token"`, `"metrics"` added to `HUB_SECTIONS`. `GET /metrics` returns 200 when **either** `Authorization: Bearer <token>` matches `metrics.token` (via `verify_bearer`, only when a token is configured) **or** a valid session exists; otherwise 401 with the existing `unauthorized` (or `setup_required`) error body and **no** metrics text. Implementation: handler takes `HeaderMap` and `Result<Authed, ApiError>` (axum accepts `Result<T, T::Rejection>` as an extractor). The OpenAPI entry gains the 401 response. A wrong bearer token with no session is 401, never 200.

### Backup verification (**fixed**)

```rust
// crates/domovoi-store/src/backup.rs
/// How thoroughly to check a database file.
pub enum Depth { Quick, Full }          // PRAGMA quick_check | PRAGMA integrity_check

/// What a verified backup is.
pub struct BackupReport { pub migrations: usize, pub newest_migration: i64, pub size_bytes: u64 }

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("{}: {reason}", path.display())] Io { path: PathBuf, reason: String },
    #[error("cannot read the backup: {0}")] Unreadable(String),
    #[error("the database failed its integrity check: {0}")] Corrupt(String),
    #[error("the backup has no migration history; it is not a Domovoi database")] NotDomovoi,
    #[error("the backup was made by a newer Domovoi (migration {0}); upgrade before restoring it")] Newer(i64),
    #[error("the backup has a migration that did not finish ({0})")] Unfinished(i64),
}

/// Opens `path` read-only (never creates it) and checks it.
pub async fn verify_backup(path: &Path, depth: Depth) -> Result<BackupReport, BackupError>;
```

Checks, in order: file exists and is non-empty; open with `SqliteConnectOptions::new().filename(path).read_only(true).create_if_missing(false)`; the pragma must return exactly the single row `ok` (any other rows are joined into `Corrupt`); `_sqlx_migrations` must exist (else `NotDomovoi`) with at least one row; every row `success = 1` (else `Unfinished(version)`); every version must be known to `MIGRATOR` (else `Newer(max unknown)`).

`Store::backup_to` order becomes: `VACUUM INTO` the target; `verify_backup(&target, Depth::Quick)`; on failure **delete the new file** and return `StoreOpenError::Verify { path, reason }` (new variant); only after success run the existing nightly `prune` and then `prune_pre_migrate(dir, keep_pre_migrate)`. A failed backup never evicts a good one.

### Pre-migrate pruning (**fixed**)

```rust
// StoreOptions gains: pub keep_pre_migrate: usize,   // default 3 in `beside()`
/// Deletes the oldest `pre-migrate-*.db` files in `dir` beyond `keep` (at least 1 is always kept). Names sort by time.
pub(crate) fn prune_pre_migrate(dir: &Path, keep: usize) -> Result<(), StoreOpenError>;
```

Called in `Store::open` right after the pre-migrate copy is taken (so the new copy is the newest and is always kept) and in `backup_to`. Files that do not match `pre-migrate-*.db` are never touched. Update the `StoreOptions { .. }` literals in the hub (use `keep_pre_migrate: 3`).

### Backup advisory (**fixed**)

`AdvisoryKind::BackupFailed` (doc: "The nightly backup failed or did not verify; the newest good backup is older than expected."). No migration: prompt 08 removed the `advisories.kind` CHECK, so add the `AdvisoryKind::BackupFailed` variant and its string mapping `backup_failed` in Rust only (verify 08's `NNNN_advisories_kind_unchecked.sql` is merged). Scheduler on `backup_to` error: upsert an advisory with id `advisory_id(AdvisoryKind::BackupFailed, None, "nightly")`, title `The nightly backup failed`, detail = the redacted error text plus "The newest earlier backup is still in the backups folder."; on success: resolve it (ignore `StoreError::NotFound`). Add `AdvisoryKind::BackupFailed` to the retain-list at `schedulers.rs:~243` (it is managed by the backup job, not by the advisory rules). Also set a gauge `domovoi_backup_last_success_timestamp_seconds` on success.

### Restore (**fixed**)

```rust
// crates/domovoi-store/src/restore.rs
pub struct RestoreReport {
    pub backup: BackupReport,
    pub previous: Option<PathBuf>,        // where the old database was moved
    pub migrations_applied_after: usize,  // migrations run when the restored file was opened
}
#[derive(Debug, thiserror::Error)]
pub enum RestoreError { /* Backup(BackupError), Io{path,reason}, Open(StoreOpenError), SameFile */ }

/// Installs `backup` as `database`. The caller holds the data-directory lock.
pub async fn restore_from_backup(database: &Path, backup: &Path, now: Timestamp) -> Result<RestoreReport, RestoreError>;
```

Steps (**fixed order**):

1. Refuse when `backup` and `database` are the same file (`SameFile`).
2. `verify_backup(backup, Depth::Full)`; any error aborts with nothing changed.
3. If `database` exists: move it, and `<database>-wal` and `<database>-shm` if present, to `<database>.before-restore-<UTC stamp>` (plus `-wal`/`-shm` suffixes). Use `std::fs::rename` (same directory, so same file system). The previous database is **never deleted** by this command.
4. Copy `backup` to `<database>.restore-tmp` in the same directory, set mode `0600` on Unix, `sync_all()` the file, `rename` it to `database`, then `sync_all()` the directory on Unix.
5. Open the result with `Store::open` using `StoreOptions::beside(database, now)` (this applies pending migrations and takes a `pre-migrate` copy when the backup is older than this build), count applied versions before and after, `close()` it.
6. On failure in steps 4 or 5: delete the partial `restore-tmp` or move the failed `database` to `<database>.failed-restore-<stamp>`, move the previous database back when there was one, and return the error. The operator never ends without a database they had.

CLI (`crates/domovoi/src/cmd/restore.rs`):

```text
domovoi restore <BACKUP_FILE> [--config <hub.toml>] [--yes]
```

- Without `--yes`: verify the file, print what would happen (backup path, migration count, size, where the current database will be moved, the reminders below) and exit with code 2 and the line `Nothing changed. Run again with --yes to restore.`
- With `--yes`: `HubLock::acquire(data_dir)` first (error text: `a hub is running on <dir>; stop it before restoring`), then `restore_from_backup(<data_dir>/hub.db, file, now)`, then print the report and the reminders: "Restore secret.key and tls/ from the same time if they were lost." and "Data deleted since this backup was taken is back; retention will remove it again at the next purge." and `Start the hub again.`
- Exit codes: 0 restored, 1 error, 2 not confirmed.
- Resolve `data_dir` from `HubConfigArg::effective()` exactly like `digest.rs` does; `hub.db` is the database file name (`runtime/adapters.rs`: `data_dir.join("hub.db")`).

`Command::Restore(restore::RestoreArgs)` in `cmd/mod.rs`; `is_quiet()` already treats it as quiet (output is the point).

## Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file in "Repository context". Prompt 08 added `integrity.rs` (`quick_check`) and a `compat` module with a `schema_compat` check that runs on a database path: reuse the `quick_check` parser for `Depth::Quick` and, if `compat::check(path)` can be called on a backup file, use it for the `Newer` refusal instead of re-implementing it (keep the `_sqlx_migrations` version check as the fallback). Prompt 09 added `domovoi audit verify`: the restore report should suggest running it afterwards (print the line only if that command exists). Confirm prompt 08 is merged (check `connect_options` for `synchronous` and the `quick_check` at open; `ls crates/domovoi-store/migrations` for the highest number). Check whether prompt 09 exposes a startup audit-verification result. Confirm `verify_bearer` is reachable from `api/system.rs` (it is `pub(crate)` in `ingest/auth.rs`: if `ingest` is private to the crate that is still fine). Confirm `HubLock` is exported. If another branch already added `StoreOptions.keep_pre_migrate` or a `step_up.rs`, reuse it rather than duplicating, and say so. Write discrepancies down before editing.
2. **Step-up list:** `step_up.rs`, rewrite `is_sensitive`, unit tests.
3. **Store health and readiness:** the port, impls, `is_alive`, `readyz` handler, `ApiDeps.store_health`, the three construction sites, OpenAPI.
4. **Metrics auth:** config section, secrets visit, `HUB_SECTIONS`, handler, update the three existing tests that read `/metrics`.
5. **Backups:** `Depth`, `verify_backup`, `BackupError`, `StoreOpenError::Verify`, `backup_to` ordering, `prune_pre_migrate`, `StoreOptions.keep_pre_migrate`, call in `Store::open`, advisory kind + migration + scheduler wiring.
6. **Restore:** `restore.rs`, CLI command, hub-level tests.
7. **Regenerate** `cargo xtask docs-config`, `cargo xtask openapi && cd web && npm run gen:api`; commit the generated files.
8. **Docs**, then checks, then the completion report.

## Database and persistence

One migration: `crates/domovoi-store/migrations/NNNN_backup_failed_advisory.sql`, where `NNNN` is the next unused number after the highest existing file (prompt 08 and the LLM-reliability change also add migrations; if the number is taken when you rebase, renumber yours, never edit a merged file). Follow the `schema_compat` convention from prompt 08 (verify how a new migration is registered).

(No advisories migration here; see "Backup advisory".)

- **N-1 upgrade test:** in `tests/sqlite/migrations.rs`, build a database migrated to the previous version, insert one open advisory of kind `triage_degraded`, open with `Store::open`, assert the row survives with identical fields and that an advisory of kind `backup_failed` round-trips through `AdvisoryRepo`.
- The restore and backup code touch files, not schema. `StoreHealth::writable` begins and rolls back a transaction and writes nothing.

## Protocol and compatibility

- No agent protocol change.
- **HTTP:** new `GET /readyz` (unauthenticated, constant vocabulary). `GET /metrics` now answers 401 without a session or token: **a deliberate tightening.** Existing scrapers need `[metrics] token` and `authorization` in their scrape config. Call this out in the PR description and the docs.
- **Config:** new optional `[metrics]` table; no existing key changes meaning. The settings UI treats `metrics.*` as step-up through the declarative list.
- **CLI:** new `domovoi restore`. No existing command changes.
- **Rust API:** `StoreOptions` gains a field (literals updated); `ApiDeps` gains a field (three construction sites); `AdvisoryKind` gains a variant (check exhaustive matches with `grep -rn "AdvisoryKind::" crates` and the web: advisories are rendered from `title`/`detail`, so no web change is expected; confirm).

## Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Settings patch touches `llm.primary.cli.args` without a password | `SettingsError::PasswordRequired` (HTTP as for other sensitive changes); nothing stored. |
| Same with a wrong password | `BadPassword` / rate limit as today (shared login throttle). |
| Patch nulls the whole `llm.secondary.cli` table | Leaf changes under `llm.secondary.cli.*` are covered: password required. |
| Non-sensitive change (for example `triage.per_hour_runs`) | No password, as today. |
| `/readyz` with the store writer busy for the `busy_timeout` | `store_writable` fails; 503; warning logged with the error text. |
| `/readyz` when the action service task has ended | `action_service` in `failing`; 503. |
| `/readyz` while everything is up | 200 `{"ready":true,"failing":[]}`. |
| `/metrics` with a session | 200. With the right bearer token | 200. With a wrong or missing token and no session | 401, empty body of metrics text. With no token configured and no session | 401. |
| Nightly backup file fails `quick_check` | File deleted; `StoreOpenError::Verify`; advisory `backup_failed` raised; older backups untouched. |
| Nightly backup verified | Older nightly beyond `keep` and pre-migrate beyond 3 pruned; advisory resolved; gauge set. |
| `VACUUM INTO` fails (disk full) | Error as today, advisory raised, nothing pruned. |
| Fewer than 3 pre-migrate files | Nothing deleted. A directory with other files | untouched. |
| `restore` without `--yes` | Verifies, prints the plan, changes nothing, exit 2. |
| `restore` while a hub holds the lock | Refuses before touching any file, exit 1. |
| Backup corrupt, not a Domovoi database, or from a newer build | Refused with the specific `BackupError` text; nothing moved; exit 1. |
| Failure after the old database was moved | The previous database is moved back; the failed file is kept as `*.failed-restore-*`; exit 1. |
| Backup is older than this build | Restored, then migrated forward with a pre-migrate copy; the report says how many migrations ran. |

## Security requirements

- **Step-up cannot be bypassed by a different spelling of the same setting.** Matching is on parsed dotted leaf paths from the patch diff, not on raw JSON text. A patch that sets the CLI through any shape that lands on a `llm.<slot>.cli.*` leaf is covered. The test walks the config schema (below) so a new `cli` field cannot slip through.
- **Secrets:** `metrics.token` is a `Secret` (resolved from env/file/stored, redacted by `Secrets::secrets_mut`), compared in constant time with the existing helper, never logged or returned. Setting it is a `Change::Secret`, so it already needs step-up.
- **`/metrics` fails closed:** an unreadable session store (an error from `auth.current`) is a 401/500, never 200.
- **`/readyz` discloses only check names.** No paths, versions, error text or counts.
- **Restore never deletes the previous database**, never runs while a hub holds the directory, writes the new file with owner-only permissions (`0600` on Unix, matching how the data directory is private), and installs it by atomic rename after `fsync`.
- **Backups verify before pruning:** a corrupt backup cannot displace good ones.
- **No approval, execution or LLM tool changes;** `HumanApproval` call sites unchanged.
- Audit: settings changes keep emitting `settings.changed` with paths only. `domovoi restore` has no audit trail of its own beyond the files it leaves (the database is offline); document that.

## Testing requirements

Names are fixed unless marked "(your choice)". Use `domovoi-testkit` fakes where a hub or engine is involved (`InMemoryStore` with `fail_next`, `FakeClock`); store tests use real SQLite temp files (`Fresh`, `tempfile`).

**Hub: step-up** (`crates/domovoi-hub`)

- `llm_cli_change_requires_step_up` (integration, `tests/hub/settings.rs`, same style as `sensitive_change_requires_password_and_shares_login_rate_limit`): patches to each of `llm.primary.cli.command`, `llm.primary.cli.args`, `llm.primary.cli.preset`, `llm.secondary.cli.command` without a password are refused as password-required and store nothing; with the right password the first succeeds (use a valid candidate config, such as setting `args` on an existing CLI primary; if the harness config has no CLI provider, drive the `SettingsService`-level check through `patch::apply` + `is_sensitive` and assert the HTTP refusal with a minimal valid `llm.primary` of kind `cli` including `cli.preset`).
- `step_up_rules_cover_their_paths` (unit, `step_up.rs`): table of `(path, expected)` including `llm.primary.cli.args` true, `llm.secondary.cli.command` true, `llm.primary.model` false, `llm.primary.api_key` (is a `Change::Secret`, not a path rule) false at path level, `webhooks.grafana.secret` true, `webhooks` true, `webhooks_other` false, `metrics.token` true, `retention.signals_days` true, `triage.per_hour_runs` false, `llm.cli` false (wildcard needs one segment), `discord.allowed_user_ids` true, `discord.channel_id` false.
- `every_llm_cli_setting_in_the_schema_requires_step_up` (unit): serialise `HubConfig::json_schema()` to a `serde_json::Value`; collect every property path by walking `properties`, resolving `$ref` into `$defs` (schemars 1.x puts shared types under `$defs`; handle `anyOf`/`oneOf`/`allOf` of `{$ref}` and `{type:null}`, which is how `Option<T>` is emitted); for the `LlmSection` slots `primary` and `secondary`, collect every path that has a segment named `cli` and assert each one is covered by `path_needs_step_up`. The test must fail if a new field is added to `CliConfig` and not covered: prove it by temporarily asserting on a fabricated path `llm.primary.cli.new_field` (covered, because `Under` is prefix based).
- `step_up_list_declares_future_actions` (unit): `STEP_UP_ACTIONS` ids are unique, each has non-empty `applies_once`, and none of them appear in `STEP_UP_SETTINGS`.
- Existing `patch.rs` unit test keeps passing unchanged.

**Hub: readiness and metrics** (`tests/hub/readiness.rs`, `api_routes.rs`, `api_auth.rs`, `api_harness.rs`)

- `readyz_is_ready_when_the_store_and_services_are_up` (200, body `{"ready":true,"failing":[]}`, no auth needed).
- `readyz_names_each_failing_check_and_nothing_else`: with `InMemoryStore::fail_next` before the call the `store_writable` check fails: 503, `failing == ["store_writable"]`, the body contains no error text; separately a stub `StoreHealth` returning `migrations_current == false` yields `["migrations"]`.
- `readyz_reports_a_stopped_action_service`: drop or shut down the services in the harness (or construct `ApiDeps` with handles whose receivers were dropped; `ManagerHandle::is_alive`/`ActionHandle::is_alive` unit tests in the engine crate can cover the primitive: `handle_reports_not_alive_after_its_service_stops`).
- `healthz_stays_constant` (existing assertions `api_auth.rs:225`, `api_routes.rs:236-243` adjusted only for metrics).
- `metrics_requires_a_session_or_the_bearer_token` (401 without either, 401 with a wrong token, 200 with a session, 200 with the right token when `[metrics] token` is configured; with no token configured a bearer header is ignored and still 401). Update the existing `advisories_digest_and_metrics` (`api_routes.rs:380-403`) to log in first, and the security-headers test (`:243`) to expect 401 for `/metrics` while still checking the security headers on the 401 response. Update `crates/domovoi/tests/single_box.rs:221` to scrape with a session or a configured token.
- `metrics_token_is_a_secret_and_needs_step_up`: a patch setting `metrics.token` requires the password (it is `Change::Secret`) and the value never appears in `settings.changed` audit details.

**Config** (`crates/domovoi-config`): `metrics_section_parses_and_resolves_its_token` (env and file sources), `metrics_token_is_visited_as_a_secret`, `env_overrides_reach_the_metrics_section` (`DOMOVOI_METRICS_TOKEN`... only if overrides work by section; follow the existing env tests), regenerate docs.

**Store** (`crates/domovoi-store/tests/sqlite`)

- `backup_verification_accepts_a_fresh_backup`: `backup_to` returns a file that `verify_backup(.., Depth::Full)` accepts; report has `migrations == MIGRATOR.iter().count()`.
- `corrupt_backup_is_rejected_and_removed`: overwrite bytes in the middle of a copied backup (or write random bytes to a file) and assert `Corrupt`/`Unreadable`; and a `backup_to` run whose verification fails (inject by pointing at a directory where a post-copy hook is not possible: instead unit-test the removal path through a `verify_or_remove(path)` helper **(your choice)** that `backup_to` calls) leaves no file and returns `StoreOpenError::Verify`.
- `failed_backup_does_not_prune_good_ones`: three existing nightly files plus `keep_backups = 3`; a failing verification leaves all three.
- `backup_from_a_newer_build_is_refused`: insert a `_sqlx_migrations` row with version 999999 into a copy, expect `Newer(999999)`; a row with `success = 0` gives `Unfinished`; a SQLite file without `_sqlx_migrations` gives `NotDomovoi`.
- `pre_migrate_backups_are_pruned_to_the_last_three`: create five `pre-migrate-<stamp>.db` files plus `notes.txt` and one `domovoi-*.db`; open a database that needs a migration (create a legacy file as in `backup_is_taken_before_pending_migrations`); afterwards exactly the three newest pre-migrate files remain (the new one included), the other two files are untouched. Also `keep_pre_migrate = 0` keeps one.
- `restore_replaces_the_database_and_keeps_the_previous_one`: restore into a directory with an existing populated `hub.db` (+ `-wal`); the result opens, contains the backup's data, and `<db>.before-restore-*` holds the old data.
- `restore_refuses_a_corrupt_or_foreign_backup_without_touching_anything`.
- `restore_into_an_empty_directory_creates_the_database` (no previous).
- `restore_migrates_an_older_backup_forward`: build an older database by running only the first migrations (a `sqlx::migrate::Migrator` whose `migrations` field is a prefix of `MIGRATOR.migrations`; verify the struct fields in the pinned sqlx version) then restore it; the report shows pending migrations applied and a `pre-migrate` copy exists.
- `restore_puts_the_old_database_back_when_the_open_fails`: a backup that verifies but whose migration fails (include a deliberately incompatible table, such as a table named like a later migration's with a conflicting definition) restores the previous database and leaves `*.failed-restore-*`. If you cannot build such a fixture reliably, test the rollback helper directly **(your choice)** and say so.
- `restore_sets_owner_only_permissions` (`#[cfg(unix)]`).
- `backup_failed_advisory_migration_keeps_existing_advisories` (the N-1 test above).

**Hub admin and CLI**

- `restore_refuses_while_a_hub_holds_the_data_directory` (hub or CLI test: acquire `HubLock`, call the restore entry point, expect the "stop it before restoring" error and no file change).
- `restore_without_yes_changes_nothing_and_exits_2` (CLI-level: call the command function with `yes = false`; assert files unchanged and the exit code).
- `nightly_backup_failure_raises_and_success_clears_the_advisory` (scheduler/unit with `InMemoryStore` and a failing backup function; if the scheduler is not unit-testable, extract the `record_backup_result(advisories, clock, result)` function **(your choice)** and test it).

## Acceptance criteria

1. Changing `llm.primary.cli.command`, `.args` or `.preset` (and the same under `secondary`) without the password is refused (`llm_cli_change_requires_step_up`); a schema walk proves no `llm.*.cli` leaf is uncovered (`every_llm_cli_setting_in_the_schema_requires_step_up`).
2. `is_sensitive` reads only from `STEP_UP_SETTINGS`; `STEP_UP_ACTIONS` names the future step-up actions and states what each waits for.
3. `GET /readyz` returns 200 or 503 with only check names; `/healthz` is unchanged.
4. `GET /metrics` is 401 without a session or valid bearer token, 200 with either; `metrics.token` is a secret that needs step-up to change.
5. A nightly backup is verified before older backups are pruned; a failed verification deletes the new file, keeps good backups, and raises the `backup_failed` advisory that clears on the next success.
6. At most the newest 3 `pre-migrate-*.db` files remain after opening a database that needed a migration; unrelated files are untouched.
7. `domovoi restore <file> --yes` verifies, refuses while a hub runs, moves the old database to `hub.db.before-restore-<stamp>`, installs the backup atomically, migrates forward, and restores the old database if anything fails; without `--yes` it changes nothing and exits 2.
8. `cargo xtask docs-config` and `cargo xtask openapi` produce no diff after the change; the generated files are committed.
9. No `HumanApproval::new` call site moved; `cargo xtask arch-check` passes; `crates/*/tests/gates.rs` unchanged and green.

## Migration and compatibility

- Existing installs: one additive migration (new advisory kind). Pre-migrate files accumulated so far are pruned to 3 at the next start that runs a migration or the next nightly backup; this deletes old pre-migrate copies, which is the stated retention (D-21): call it out in the PR description.
- Prometheus scrapers stop working until a token is configured (or they use a session): documented loudly. `/healthz` users are unaffected.
- Single-box mode and the Docker image use `/healthz`: unchanged.
- No agent, protocol or data change.

## Documentation requirements

- `docs/book/src/reference/backup.md`: add sections "Verification" (every nightly copy is checked; a `backup_failed` advisory appears when it fails), "How long copies are kept" (7 nightly, 3 pre-migrate; deleted data stays in these copies until they rotate out; restoring brings deleted data back) and "Restoring" (stop the hub, `domovoi restore <file> --yes`, what it moves aside, what it prints, keep `secret.key` and `tls/`).
- `docs/book/src/reference/cli.md`: add `domovoi restore` (arguments, exit codes) and replace the "with no login needed" sentence about `/metrics` with the token/session rule and a Prometheus scrape example using `authorization: { credentials_file: ... }`.
- `docs/book/src/reference/api.md:156-159`: document `/readyz` and the `/metrics` rule.
- A short note in `docs/book/src/guide/hardening.md`: which settings need the password again (list them from `STEP_UP_SETTINGS`, in plain words) and that the CLI program of a model provider is among them.
- `docs/book/src/install/upgrading.md:40`: mention that only the last 3 pre-migrate copies are kept.
- Regenerate: `cargo xtask docs-config`; `cargo xtask openapi && cd web && npm run gen:api`; `cargo xtask docs-links`.
- `ARCHITECTURE.md`: do not edit (updated once per milestone, after F4).

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.
