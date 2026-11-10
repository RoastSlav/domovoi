# 32 · Refactor: per-host settings in their own table

Do this before 15 (skill management) and 36, which add more per-host state.

## Background (verified on `main`)

- `settings` is `key TEXT PRIMARY KEY, value TEXT` (`crates/domovoi-store/migrations/0001_init.sql:279`)
  behind `SettingsRepo` (`crates/domovoi-core/src/ports/repos/accounts.rs`: `get`,
  `set`, `set_many`, `list_prefix`). Besides the hub config it now holds per-host data
  under made-up keys:

  | Key | Written by | Read by |
  | --- | --- | --- |
  | `host.<name>.checks` | `settings/service.rs:381` | `agent_server/overrides.rs:22` |
  | `host.<name>.docker` | `settings/host_docker.rs:72` | `agent_server/overrides.rs:39` |
  | `host.<name>.skills` | `agent_server/skill_policy.rs:105` | `skill_policy.rs:58` |
  | `host.<id>.profile` | `profile/mod.rs:239` | `profile/mod.rs:180` |

  (all paths under `crates/domovoi-hub/src/`). Key helpers: `settings/stored.rs:33-42`,
  `skill_policy.rs:26-29`, `profile/mod.rs:157`.
- Problems: two are keyed by host **name** and one by **id**; nothing ties them to the
  `hosts` row (`hosts.id` is the primary key, `name` is unique); values have no schema;
  `list_prefix` callers can see other hosts' keys.
- SQLite runs with `foreign_keys(true)` (`crates/domovoi-store/src/db.rs:94`).

## Steps

1. **Migration** `crates/domovoi-store/migrations/0005_host_settings.sql`:
   ```sql
   CREATE TABLE host_settings (
       host_id    TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
       kind       TEXT NOT NULL CHECK (kind IN ('checks', 'docker', 'skills', 'profile')),
       value      TEXT NOT NULL,
       updated_at INTEGER NOT NULL,
       PRIMARY KEY (host_id, kind)
   ) STRICT;
   ```
   Then copy the old rows: for `checks`, `docker`, `skills` join `hosts` on
   `settings.key = 'host.' || hosts.name || '.' || kind`; for `profile` on
   `'host.' || hosts.id || '.profile'`. Use `0` for `updated_at` (unknown). Delete the
   copied `settings` rows in the same migration. Rows whose host no longer exists are
   dropped.
2. **Port** in `crates/domovoi-core/src/ports/repos/hosts.rs` (stays under 500 lines;
   else a new `repos/host_settings.rs`):
   ```rust
   pub enum HostSettingKind { Checks, Docker, Skills, Profile }
   pub trait HostSettingsRepo: Send + Sync {
       async fn get(&self, host: HostId, kind: HostSettingKind) -> Result<Option<Value>, StoreError>;
       async fn set(&self, host: HostId, kind: HostSettingKind, value: &Value, at: Timestamp) -> Result<(), StoreError>;
       async fn delete(&self, host: HostId, kind: HostSettingKind) -> Result<(), StoreError>;
   }
   ```
   Doc every item. Implement it in `domovoi-store` (new `repos/host_settings.rs`) and in
   `domovoi-testkit`, and add a contract test in
   `crates/domovoi-testkit/src/contracts/` like the existing ones.
3. **Callers.** Replace the four key families with the new port. Callers that only have
   a host name look up the `Host` first (they already do for most paths). Delete the key
   helper functions. `settings::host_docker` currently writes through `set_many` to stay
   atomic with nothing else; a single `set` is enough.
4. Update the module doc of `settings/stored.rs` (it lists `host.<name>.checks`).
5. `docs/book/src/reference/data-model.md`: add `host_settings`; say the `settings`
   table holds only the hub configuration, UI secrets and `last_apply`, `hub_host_id`,
   `update_check`.

## Tests

- Store: `crates/domovoi-store/tests/sqlite/migrations.rs`:
  `migrates_per_host_settings_out_of_the_settings_table` (seed the old keys for an
  existing host and for a missing one, migrate, assert the new rows and that the old
  keys are gone).
- Store: deleting a host row deletes its settings (cascade).
- Existing hub tests in `tests/hub/skills.rs` and `tests/hub/profile.rs` read the old
  keys directly (`host_skills_key("nas")`, `profile_key(..)`): change them to read
  through `HostSettingsRepo`.

## Done when

- `git grep -n '"host\.' crates/*/src` finds nothing.
- Per-host data survives the migration and disappears with its host.
