# 57 — Configurable retention

| | |
|---|---|
| Branch | `configurable-retention` |
| Milestone | F4 |
| Depends on (must be merged) | 08 (store durability settings and migration conventions; no schema change here) |
| Can run in parallel with | 17, 18, 19 |
| Produces (shared contracts) | The `[retention]` config section and `RETENTION_KEYS` registry; `RetentionPolicy` with conversations and notifications; single-transaction `Store::purge` with an FTS-cleanup guard; `backup.keep_pre_migrate` |
| Consumes | From prompt 19, **if merged**: `crates/domovoi-hub/src/settings/step_up.rs` (the `retention` prefix is already in its list) and `StoreOptions.keep_pre_migrate` with `prune_pre_migrate`. Both have documented fallbacks below. |
| Launch gate | none |
| Suggested model | small to medium |

## Objective

Make every retention value of the Domovoi data model **configurable with its D-21 default**, and wire only the keys whose data exists today:

| Data | Key | Default | Applies now? |
|---|---|---|---|
| signals | `retention.signals_days` | 30 | yes |
| incident timeline raw events | `retention.incident_timeline_days` | 90 | yes (all kinds except follow-up conversation entries) |
| conversations (follow-up questions and answers on incidents) | `retention.conversations_days` | 180 | yes |
| notifications and notification state | `retention.notifications_days` | 30 | yes (state of resolved incidents) |
| cached triage reports | `retention.triage_cache_days` | 90 | yes (existing behaviour, now configurable) |
| resolved advisories | `retention.resolved_advisories_days` | 30 | yes (existing behaviour, now configurable) |
| operation journal raw events | `retention.operation_journal_days` | 180 | **no**: declared, "applies once operations exist" |
| raw execution outputs | `retention.execution_outputs_days` | 30 | **no**: declared, "applies once execution outputs are stored apart from the audit log" |
| retracted fact content | `retention.retracted_facts_days` | 30 | **no**: declared, "applies once knowledge facts exist" |
| nightly backups | `backup.keep` (existing) | 7 | yes |
| pre-migrate backups | `backup.keep_pre_migrate` (new) | 3 | yes |
| operations and resolution summaries | none | retained | not applicable |
| audit log | none | retained (never pruned; checkpoint pruning only when it is configured later) | not applicable |

Also: pruning deletes **in one transaction** (so a failure deletes nothing), a guard test makes sure no purged table can gain a full-text index without its cleanup in the same transaction, the settings view reports each key's source label, and changing any `retention.*` key needs the admin's password again.

## Architectural context

Plain-terms background (D-09, D-21, RW-28, RW-24, `04` §14, ADR 0017):

- **All retention is configurable** (owner decision D-09). The defaults are the owner's D-21 values. Operations, resolution summaries and the audit log are retained: they have no day count. The audit log is append-only (database triggers reject UPDATE and DELETE), so nothing here may touch it.
- **What deletion guarantees.** Deletion removes data from the live database (and its full-text index, and model context) in one transaction. It **remains in backups until they rotate out**: up to the configured nightly backups plus the last pre-migrate copies. Restoring a backup brings deleted data back until the next purge. The docs and (later) the retention page say so.
- **Today's data.** Signals, the incident timeline (`incident_events`), the triage cache, resolved advisories, maintenance windows and sessions are purged by `Store::purge` with a hard-coded policy. Follow-up questions and answers on an incident are timeline entries of kind `triage.followup` (and the marker `followup.cleared`): they are the only conversation data that exists today, so the "conversations" key applies to exactly those entries, and the timeline key applies to all other entries. `notify_state` (what the notification policy remembers per incident) is never purged today.
- **Not yet existing.** The operation journal, structured execution outputs and knowledge facts do not exist. Their keys are accepted and documented but **applied to nothing**; each says what must exist first. The 4 KiB output excerpt stored in `action.succeeded`/`action.failed` audit events is part of the append-only audit log and is retained.
- **Settings layers (ADR 0017).** Each key can come from defaults, values saved in the web UI, `hub.toml` or `DOMOVOI_*` variables. The settings view reports the source of each field that a layer set (`default` fields are absent from the map, which means the default). A field set by file or environment is locked in the UI. The retention page in the web UI comes later; this prompt is backend only.
- **Step-up (RW-24).** Changing retention can destroy data sooner, so it needs the admin's password again (same mechanism as secrets and trusted proxies).
- **No migration.** Nothing in the schema changes.

## Repository context

Verify each item by opening it. Line numbers are from when this brief was written.

**Store**

- `crates/domovoi-store/src/purge.rs` (110 lines): `RetentionPolicy { signals, timeline, triage, resolved_advisories }` (all `SignedDuration`; `Default` = 30 d, 90 d, 90 d, 30 d), `PurgeReport { signals, timeline, triage, advisories, windows, sessions }`, `cutoff(now, keep)`, private `Store::delete(sql, before)` executing on `self.write` (one statement, auto-commit), `Store::purge(now, &RetentionPolicy)` running six independent DELETEs and then `tracing::info!(?report, ...)`:

```rust
.delete("DELETE FROM incident_events WHERE at < ?", cutoff(now, policy.timeline))
.delete("DELETE FROM triage_cache WHERE stored_at < ?", cutoff(now, policy.triage))
.delete("DELETE FROM advisories WHERE resolved_at IS NOT NULL AND resolved_at < ?", ...)
.delete("DELETE FROM maintenance_windows WHERE ends_at <= ?", ms(now))
.delete("DELETE FROM sessions WHERE expires_at <= ?", ms(now))
```

- Schema (`crates/domovoi-store/migrations/0001_init.sql`): `incident_events(seq, incident_id, at, kind, summary, detail)` (`:97-106`), `signals`, `triage_cache(stored_at)`, `advisories(resolved_at)`, `maintenance_windows`, `sessions`, `audit_log` with `BEFORE UPDATE/DELETE` triggers (`:133-156`). `migrations/0002_notify_state.sql`: `notify_state(incident_id PK, held 0|1, state, updated_at)`. Full-text tables: `incidents_fts` (content table `incidents`, triggers keep it in step) and `notes_fts` (content table `notes`); **neither base table is purged**.
- Writer pool is one connection (`Store.write`, `crates/domovoi-store/src/db.rs`); transactions on it are exclusive. Find an existing multi-statement transaction in `crates/domovoi-store/src/repos/` (for example `discard_pending` in `repos/incidents.rs`) and copy how it begins and commits.
- Tests: `crates/domovoi-store/tests/sqlite/purge.rs` (`purge_respects_retention_policy`, uses `RetentionPolicy::default()` and asserts `report.signals`, `timeline`, `triage`, `advisories`), `common.rs` (`Fresh`), `Store::pool_for_tests()`.
- Follow-up constants: `crates/domovoi-engine/src/triage/history.rs:9,14` `FOLLOW_UP_KIND = "triage.followup"`, `CLEAR_KIND = "followup.cleared"` (the store cannot depend on the engine, so these strings must move to `domovoi-core`; see Interfaces).

**Config**

- `crates/domovoi-config/src/hub/mod.rs:28-71` `HubConfig` (sections, each `#[serde(default)]`), `BackupSection { dir, keep }` `:354-373` (`default_keep() = 7`), `Secrets for HubConfig` `:392`. Validation `crates/domovoi-config/src/validate.rs` (`backup.keep == 0` check at `:~113`). `HUB_SECTIONS: [&str; N]` in `crates/domovoi-config/src/env.rs:13` (top-level tables taking `DOMOVOI_*` overrides; the array length is part of the type). Duration/day values elsewhere are `u32` or `Duration` types: a plain `u32` of days is the simplest and what this brief uses.
- Provenance: `crates/domovoi-config/src/provenance.rs` (`Source::{Default,Ui,File,Env(var)}`, `label()` -> `default`, `ui`, `file`, `env:<VAR>`; only fields some layer set are recorded). Settings view: `crates/domovoi-hub/src/settings/service.rs:~190-200` (`fields: provenance.iter().map(|(path, source)| (path.to_owned(), source.label()))`), DTO `crates/domovoi-hub/src/api/settings.rs:124-160`. Locking rule `service.rs:~246-255` (`!source.editable()` -> `SettingsError::Locked`). Test pattern: `crates/domovoi-hub/tests/hub/settings.rs` (`saving_merges_with_the_other_layers_and_reports_sources` `:38`, `file_or_env_locked_field_is_409_with_source` `:164`, `sensitive_change_requires_password_and_shares_login_rate_limit` `:189`).

**Hub wiring**

- `crates/domovoi-hub/src/runtime/schedulers.rs:~350-355`: the daily 03:30 job calls `runtime.store.purge(now, &RetentionPolicy::default())`. `runtime.config` is the loaded `HubConfig` (a settings save restarts the runtime, so config is current at start). Conversion helpers live in `crates/domovoi-hub/src/runtime/settings.rs`.
- Backup options: `crates/domovoi-hub/src/runtime/adapters.rs:31-43` builds `StoreOptions { backup_dir, keep_backups, now }` (and `crates/domovoi-hub/src/settings/apply.rs:~118`).
- Step-up: `crates/domovoi-hub/src/settings/patch.rs:104-117` `is_sensitive`. **Prompt 19 replaces this with a declarative list in `crates/domovoi-hub/src/settings/step_up.rs` that already contains `Under("retention")`.**

**Docs**

- `docs/book/src/reference/data-model.md:46-57` has a "Retention" table (30/90/90/30 days; "Incidents, audit log, digest facts: forever") and a "Backups" section; `docs/book/src/concepts/how-it-works.md:41` mentions the 03:30 purge; `docs/book/src/SUMMARY.md` lists pages; `docs/book/src/reference/configuration.md` is generated by `cargo xtask docs-config`.

## Exact scope

Build:

1. Config: `RetentionSection`, `RETENTION_KEYS`, validation, `HUB_SECTIONS`, `backup.keep_pre_migrate`.
2. Core: the two follow-up kind constants.
3. Store: `RetentionPolicy` (+ conversations, notifications), single-transaction `purge`, new deletes, FTS guard constants, `PurgeReport` additions.
4. Hub: map config to policy; scheduler uses it; `StoreOptions.keep_pre_migrate` wiring; step-up check; source labels verified.
5. Docs and config reference.
6. Tests.

Files expected to change or be added:

- config: `src/hub/mod.rs`, `src/hub/retention.rs` **(new)**, `src/validate.rs`, `src/env.rs`, tests, regenerated `docs/book/src/reference/configuration.md`.
- core: `src/triage/mod.rs` (constants) and `src/lib.rs` if a re-export is needed.
- engine: `src/triage/history.rs` (constants re-exported from core under the same public names; behaviour unchanged).
- store: `src/purge.rs`, possibly `src/purge_rules.rs` **(new)** if `purge.rs` would pass 500 lines, `src/db.rs` only if `keep_pre_migrate` is missing (fallback below), `src/lib.rs` (exports); tests `tests/sqlite/purge.rs`.
- hub: `src/runtime/{settings.rs,schedulers.rs,adapters.rs}`, `src/settings/apply.rs` (the `StoreOptions` literal), possibly `src/settings/patch.rs` (fallback below); tests `tests/hub/settings.rs`.
- docs: `docs/book/src/reference/data-model.md`, `docs/book/src/concepts/how-it-works.md`, a new `docs/book/src/guide/retention.md` and `SUMMARY.md`.
- web: none (the retention page is a later change).

## Explicit non-goals

- No migration, no new table, no new column.
- No web UI for retention (a later change builds the page). No change to `web/`.
- No purge of the audit log, incidents, digest facts, `llm_usage`, notes, baselines, actions or users. (`llm_usage` and `digest_facts` growth is a separate finding; D-21 lists no value for them: do not invent one.)
- No application of the three "declared" keys; no operation journal, execution-output or fact code. Do not add tables or stubs for them.
- No change to `backup.keep` semantics (nightly backups). The pre-migrate pruning function itself belongs to the operations-readiness change; this prompt only wires its key (see fallback).
- No `min_retained_seq`, SSE reset or journal logic.
- No change to approvals, protocol, LLM or policy code.
- Do not lower any default or delete more than today by default, except where D-21 differs from today: conversations are kept 180 days instead of 90 (longer), notification state of resolved incidents is now removed after 30 days (new).

## Required interfaces and contracts

### Config (**fixed** names and defaults)

```toml
[retention]
signals_days = 30
incident_timeline_days = 90
conversations_days = 180
notifications_days = 30
triage_cache_days = 90
resolved_advisories_days = 30
# Declared; applied once the data they cover exists:
operation_journal_days = 180
execution_outputs_days = 30
retracted_facts_days = 30

[backup]
keep = 7                 # existing
keep_pre_migrate = 3     # new
```

`crates/domovoi-config/src/hub/retention.rs` **(new)**:

```rust
/// `[retention]`: how many days each kind of raw data is kept. Operations, resolution
/// summaries and the audit log are kept and have no setting here.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetentionSection {
    #[serde(default = "d30")] pub signals_days: u32,
    #[serde(default = "d90")] pub incident_timeline_days: u32,
    #[serde(default = "d180")] pub conversations_days: u32,
    #[serde(default = "d30")] pub notifications_days: u32,
    #[serde(default = "d90")] pub triage_cache_days: u32,
    #[serde(default = "d30")] pub resolved_advisories_days: u32,
    /// Not applied yet: takes effect once operations exist.
    #[serde(default = "d180")] pub operation_journal_days: u32,
    /// Not applied yet: takes effect once execution outputs are stored apart from the audit log.
    #[serde(default = "d30")] pub execution_outputs_days: u32,
    /// Not applied yet: takes effect once knowledge facts exist.
    #[serde(default = "d30")] pub retracted_facts_days: u32,
}
impl Default for RetentionSection { /* the defaults above */ }

/// Whether a retention key does anything today.
pub enum Applies { Now, Once(&'static str) }
pub struct RetentionKey { pub key: &'static str, pub default_days: u32, pub applies: Applies }
/// The registry of every `[retention]` key, in the order of the table above. Docs and tests read it.
pub const RETENTION_KEYS: &[RetentionKey] = &[ /* nine entries; Once("operations exist"), Once("execution outputs are stored apart from the audit log"), Once("knowledge facts exist") */ ];
```

Every field has a doc comment (they become the generated config reference). `HubConfig` gains `#[serde(default)] pub retention: RetentionSection`. `BackupSection` gains `#[serde(default = "default_keep_pre_migrate")] pub keep_pre_migrate: u32` (3) with doc "Pre-migration backups kept. Default 3." and `Default` updated. Add `"retention"` to `HUB_SECTIONS` (bump the array length; if `"metrics"` was already added by another change keep both).

Validation (`validate.rs`, `ConfigIssue::error(path, message)`, same style as `backup.keep`): each of the nine `retention.*_days` must be between 1 and 3650 ("must be 1 to 3650 days"); `backup.keep_pre_migrate` at least 1 ("must be at least 1"). Zero is refused so a mistyped value cannot delete everything at the next purge.

### Core (**fixed**)

In `crates/domovoi-core/src/triage/mod.rs`: `pub const FOLLOW_UP_KIND: &str = "triage.followup";` and `pub const CLEAR_KIND: &str = "followup.cleared";` (doc comments saying they are the timeline kinds of follow-up conversation entries). `crates/domovoi-engine/src/triage/history.rs` keeps exporting `FOLLOW_UP_KIND` and `CLEAR_KIND` under the same names as `pub const FOLLOW_UP_KIND: &str = domovoi_core::triage::FOLLOW_UP_KIND;` (so the engine's public API and every caller stay unchanged). Re-export from `domovoi-core`'s root only if other core constants of that module are (follow the pattern).

### Store (**fixed** behaviour)

```rust
pub struct RetentionPolicy {
    pub signals: SignedDuration,
    pub timeline: SignedDuration,
    pub conversations: SignedDuration,      // new, default 180 d
    pub notifications: SignedDuration,      // new, default 30 d
    pub triage: SignedDuration,
    pub resolved_advisories: SignedDuration,
}
pub struct PurgeReport { /* existing fields */ pub conversations: u64, pub notifications: u64 }
```

`Store::purge(now, &policy)` runs **every** delete inside one transaction on the writer pool and commits once; any error rolls everything back and returns the error (nothing is deleted). The log line and the returned report are produced after the commit. The statements, in this order:

```sql
DELETE FROM signals WHERE observed_at < ?;                                                    -- signals
DELETE FROM incident_events WHERE at < ? AND kind NOT IN (?, ?);                              -- timeline: binds cutoff, FOLLOW_UP_KIND, CLEAR_KIND
DELETE FROM incident_events WHERE at < ? AND kind IN (?, ?);                                  -- conversations
DELETE FROM notify_state WHERE updated_at < ? AND held = 0
    AND incident_id IN (SELECT id FROM incidents WHERE status = 'resolved');                  -- notifications
DELETE FROM triage_cache WHERE stored_at < ?;
DELETE FROM advisories WHERE resolved_at IS NOT NULL AND resolved_at < ?;
DELETE FROM maintenance_windows WHERE ends_at <= ?;
DELETE FROM sessions WHERE expires_at <= ?;
```

Statements are constants with bound parameters (no string formatting of values). Never delete `held = 1` notification state (an alert held for the end of quiet hours) and never state of an unresolved incident (its card and thread references keep later updates from posting duplicate cards).

FTS guard (`purge.rs` or `purge_rules.rs`):

```rust
/// Tables `purge` deletes from.
pub const PURGED_TABLES: &[&str] = &["signals", "incident_events", "notify_state", "triage_cache",
                                     "advisories", "maintenance_windows", "sessions"];
/// Purged tables that have a full-text index, with the statement that removes the index rows.
/// Run inside the same transaction, before the table's own delete. Empty today: no purged
/// table has a full-text index. A table added to PURGED_TABLES that gains an index must be listed here.
pub const FTS_CLEANUP: &[(&str, &str)] = &[];
```

`purge` executes each `FTS_CLEANUP` statement (with the same cutoff bound) inside the transaction before the matching delete; the loop reads the constant so it is not dead code.

### Hub (**fixed**)

```rust
// crates/domovoi-hub/src/runtime/settings.rs
pub(super) fn retention(config: &HubConfig) -> RetentionPolicy {
    // days -> SignedDuration::from_hours(24 * days); the three declared keys are NOT read
}
```

`schedulers.rs`: `runtime.store.purge(now, &settings::retention(&runtime.config))`. `adapters.rs` and `settings/apply.rs`: `StoreOptions { keep_backups: config.backup.keep as usize, keep_pre_migrate: config.backup.keep_pre_migrate as usize, .. }` (for `apply.rs`'s rollback helper that has no full config, keep the default of 3 and say so).

**Step-up.** If `crates/domovoi-hub/src/settings/step_up.rs` exists, do not edit it: assert in a test that `retention.*` is covered. **Fallback if it does not exist** (the operations-readiness change has not merged): in `patch.rs::is_sensitive` add `|| path == "retention" || path.starts_with("retention.")`, and report that the declarative list will absorb it.

**Pre-migrate fallback.** If `StoreOptions.keep_pre_migrate` does not exist yet, add only that field (`usize`, `beside()` sets 3) and wire the config key into it; do **not** write pruning code (that belongs to the operations-readiness change) and say so in the report. If it exists, wire the key and do nothing else in the store.

## Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file listed. Confirm: `purge.rs` still runs six independent deletes; `FOLLOW_UP_KIND`/`CLEAR_KIND` values; `notify_state` columns; neither `incident_events` nor `notify_state` has an FTS index (look at `sqlite_master` through the migrations); whether `step_up.rs` and `StoreOptions.keep_pre_migrate` exist (choose the matching branch above); the current `HUB_SECTIONS` length. If `crates/domovoi-store/migrations` shows a retention-related migration from another change, stop and report. Write discrepancies down before editing.
2. **Core constants and engine re-export.** Run `cargo nextest run -p domovoi-engine` for the triage tests to confirm nothing changed (no behaviour change).
3. **Config:** `retention.rs`, `HubConfig.retention`, `BackupSection.keep_pre_migrate`, `HUB_SECTIONS`, validation, config tests; `cargo xtask docs-config`.
4. **Store:** policy fields, report fields, transactional `purge`, new statements, FTS constants. Keep non-test files under 500 lines (split into `purge_rules.rs` if needed).
5. **Hub:** `retention()` conversion, scheduler call, `StoreOptions` wiring, step-up branch.
6. **Tests** (below). Update `purge_respects_retention_policy` for the new fields without weakening any assertion.
7. **Docs.** Then checks and the completion report.

## Database and persistence

No migration and no schema change. All deletes run in one transaction on the single writer connection; the transaction is exclusive, so no other writer interleaves. `PRAGMA foreign_keys` is on: `incident_events` rows reference `incidents` (never purged), so no cascade is triggered. The purge is idempotent: a second run at the same `now` deletes nothing. N-1 upgrade test: **not applicable** because no schema changes; the existing `migrations` tests remain the guard.

## Protocol and compatibility

- No protocol or wire change. HTTP API unchanged except that `GET /api/v1/settings` now includes retention fields in `fields` when a layer sets them, and `GET /api/v1/settings/schema` includes the `[retention]` section. OpenAPI is unchanged (the schema endpoint returns an untyped object); run `cargo xtask openapi` and commit only if it diffs.
- Config: a new optional table and one new optional `backup` key; old files stay valid. `deny_unknown_fields` means a typo in a retention key is an error with its path.
- Behaviour changes at the next purge after upgrade, by design (D-21): follow-up conversation entries are kept up to 180 days instead of 90; notification state of incidents resolved more than 30 days ago is removed; everything else is as before.

## Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Any single delete fails | Transaction rolls back; `purge` returns the store error; nothing is deleted; the scheduler logs a warning as today and tries again the next day. |
| `retention.signals_days = 0` or above 3650 | Config validation error naming `retention.signals_days`; the hub (or the settings save) is refused. |
| A retention key set in `hub.toml` or the environment | The UI cannot change it: the settings save returns `Locked` with the source label (`file` or `env:DOMOVOI_RETENTION_...`). |
| A retention key saved from the UI without the password | `PasswordRequired`; nothing stored. |
| A declared key (`operation_journal_days`, ...) is set | Accepted and validated; **no effect**; the generated docs and field description say so. |
| Notification state `held = 1` or its incident unresolved | Not deleted, however old. |
| Incident reopened after its notification state was purged | The notification policy treats it as having no card yet and posts a new one (the same path as for a brand-new incident). Reopening is bounded by `incidents.reopen_window`, normally far below 30 days. |
| Follow-up clear marker older than the conversation cutoff | Deleted together with the conversation entries (same key). |
| `purge` runs while a settings restart is pending | Uses the config the runtime started with; the next runtime uses the new values. |

## Security requirements

- **Step-up:** any change to `retention.*` requires password re-entry (declarative list from the operations-readiness change, or the fallback condition). Tested.
- **Never touch the audit log.** No statement targets `audit_log`; its triggers would reject it. A test asserts the audit table is unchanged by a purge with an aggressive policy.
- **Fail safe on bad values:** zero and absurd values are refused at validation so a typo cannot delete all data.
- **No data in logs:** the purge log line carries counts only.
- **Secrets:** none involved.
- **Honest guarantees:** documentation states that deleted data survives in backups until they rotate (nightly `backup.keep`, pre-migrate `keep_pre_migrate`) and that restoring a backup brings it back.
- No approval, execution, policy or LLM change.

## Testing requirements

Names are fixed unless marked "(your choice)". Store tests use SQLite temp databases (`Fresh`); config and hub tests follow the neighbouring files. No test sleeps; `now` is a value passed to `purge`.

**Config** (`crates/domovoi-config`)

- `retention_defaults_match_the_owner_decision`: `RetentionSection::default()` equals 30/90/180/30/90/30/180/30/30 for the nine keys in registry order, and `RETENTION_KEYS` has the same defaults (a table-driven test with the D-21 numbers written out literally).
- `retention_registry_marks_the_unbuilt_keys`: exactly `operation_journal_days`, `execution_outputs_days` and `retracted_facts_days` are `Applies::Once(..)`, each with non-empty text; the other six are `Applies::Now`.
- `retention_values_outside_one_to_3650_are_rejected_with_their_path` (0 and 3651 for one key; the message names `retention.<key>`), `backup_keep_pre_migrate_must_be_at_least_one`.
- `retention_keys_load_from_file_and_environment` (e.g. `DOMOVOI_RETENTION_SIGNALS_DAYS=14` overrides the file; follow the env test style in `env.rs`/`layered.rs`).
- `unknown_retention_key_is_an_error` (`deny_unknown_fields`).
- Regenerated reference: `cargo xtask docs-config` produces no further diff.

**Store** (`crates/domovoi-store/tests/sqlite/purge.rs`)

- `purge_respects_retention_policy` (existing; adapt to the new struct fields; same assertions plus `report.conversations == 0` and `report.notifications == 0` for its data).
- `purge_uses_the_configured_durations`: signals aged 20 days survive a 30-day policy and are deleted by a 14-day policy; the same for timeline, triage cache and resolved advisories.
- `conversations_are_kept_longer_than_the_timeline`: with timeline 90 d and conversations 180 d, a `triage.followup` entry and a `followup.cleared` marker aged 120 days survive while a `status.changed` entry aged 120 days is deleted; at 200 days the conversation entries are deleted too; `report.conversations` and `report.timeline` count them separately.
- `notification_state_is_purged_only_for_resolved_incidents_not_held`: four `notify_state` rows older than 30 days: resolved + `held = 0` (deleted), resolved + `held = 1` (kept), open incident (kept), and a fresh resolved one (kept). Build the incidents with `domovoi_testkit::build` helpers and the `NotifyStateRepo` port (find its method names in `crates/domovoi-core/src/ports/repos/`; insert the incident status through the incident repo).
- `purge_is_all_or_nothing`: install a trigger through `pool_for_tests()` such as `CREATE TRIGGER boom BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'boom'); END;`, add an expired session and an old signal, call `purge`: it returns an error and the old signal is **still there**. Drop the trigger and purge again: both are deleted.
- `purge_never_touches_the_audit_log_or_incidents` (aggressive policy of 1 second for every duration: audit rows and incidents unchanged).
- `purged_tables_with_a_full_text_index_clean_it_in_the_same_transaction` (the guard): query `sqlite_master` for virtual tables whose name ends in `_fts`; for each, derive the content table from its `content = '<table>'` option in the stored SQL; assert that table is not in `PURGED_TABLES`, or is listed in `FTS_CLEANUP`. Today this passes because `incidents` and `notes` are not purged.
- `purge_twice_deletes_nothing_more` (existing idempotence assertion).

**Hub** (`crates/domovoi-hub/tests/hub/settings.rs` and a runtime unit test)

- `retention_policy_comes_from_the_config` (unit, `runtime/settings.rs`): a `HubConfig` with `signals_days = 14`, `conversations_days = 365` converts to the matching durations; changing the three declared keys changes nothing in the result (compare two configs that differ only there).
- `retention_source_labels_are_reported`: saved from the UI -> `fields["retention.signals_days"] == "ui"`; set in the file -> `"file"` and a UI patch of it is refused as locked with the source named; set by `DOMOVOI_RETENTION_SIGNALS_DAYS` -> `"env:DOMOVOI_RETENTION_SIGNALS_DAYS"`; never set -> absent from `fields` (the default applies). Use the existing settings test harness (`api_harness`, `ConfigSources`).
- `retention_change_requires_step_up`: patching `retention.signals_days` without the password is refused as password-required and stores nothing; with the password it is saved (this test fails if neither `step_up.rs` nor the fallback is in place).
- `declared_retention_keys_are_accepted_but_applied_to_nothing`: patching `retention.operation_journal_days` succeeds (after step-up) and `retention(config)` is unaffected (unit-level, as above).
- `scheduler_purges_with_the_configured_policy` (your choice; only if the scheduler is testable with `FakeClock` as the neighbouring scheduler tests are): otherwise cover the wiring by the `retention()` unit test and say so in the report.

**Gate/arch:** `cargo xtask arch-check` unchanged; `crates/domovoi-store` still depends only on `domovoi-core`.

## Acceptance criteria

1. Every key in the objective table exists with its D-21 default and a doc comment; the three unbuilt keys say "applies once <feature> exists" in their field docs and in the registry (`retention_defaults_match_the_owner_decision`, `retention_registry_marks_the_unbuilt_keys`).
2. `Store::purge` honours the configured durations for signals, timeline, conversations, notification state, triage cache and resolved advisories (`purge_uses_the_configured_durations`, `conversations_are_kept_longer_than_the_timeline`, `notification_state_is_purged_only_for_resolved_incidents_not_held`).
3. All deletes run in one transaction: a failure deletes nothing (`purge_is_all_or_nothing`).
4. No purged table can gain a full-text index without its cleanup being run in the same transaction (`purged_tables_with_a_full_text_index_clean_it_in_the_same_transaction`).
5. The audit log, incidents and every table not listed are never purged (`purge_never_touches_the_audit_log_or_incidents`).
6. Values of 0 or above 3650 days are rejected with the key's path; `backup.keep_pre_migrate` defaults to 3 and is at least 1.
7. The settings view reports `ui`, `file` or `env:<VAR>` for each retention field a layer set, and a file- or environment-set field is locked (`retention_source_labels_are_reported`).
8. Changing any `retention.*` setting needs the admin's password again (`retention_change_requires_step_up`).
9. The daily purge uses the configured policy (`retention_policy_comes_from_the_config`, scheduler wiring read in the diff).
10. `cargo xtask docs-config` produces no diff after the change; the book states what deletion guarantees about backups; no migration file was added.

## Migration and compatibility

- Existing installs: no schema change. At the first purge after upgrade, follow-up conversation entries between 90 and 180 days old that the old policy would have deleted are now kept, and old notification state of resolved incidents is removed. State both in the PR description.
- A `hub.toml` without `[retention]` behaves with the defaults above. Single-box mode and the Docker image need no change.
- Agents and the protocol are unaffected.
- The retention page and the audit page (later changes) build on `RETENTION_KEYS` and the settings endpoints; do not change their shapes after this merges.

## Documentation requirements

- New book page `docs/book/src/guide/retention.md` (add to `SUMMARY.md` under "Guides"): the table from the objective (key, default, what is removed, what stays), the statement that operations, resolution summaries and the audit log are kept, a section **What deletion guarantees** (removed from the live database, its search index and what the model sees at the moment of purge; still present in backups until they rotate out: nightly `backup.keep` and the last `backup.keep_pre_migrate` pre-migration copies; restoring a backup brings the data back until the next purge), the three keys marked "applies once the feature exists", and that changing retention needs the password.
- `docs/book/src/reference/data-model.md` "Retention" table: replace with the configurable table (link to the guide page); correct the "Incident timeline entries 90 days" row to say follow-up conversations 180 days.
- `docs/book/src/concepts/how-it-works.md:41`: keep the sentence, add "(see Retention)".
- `cargo xtask docs-config` for the generated reference; `cargo xtask docs-links` for links and anchors.
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
