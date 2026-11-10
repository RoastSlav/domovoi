# 33 · Feature: maintenance windows from the web UI

"I'm rebooting the NAS for an hour": no cards, no pings, signals still recorded.

## Background (verified on `main`)

- The engine already honours windows: `crates/domovoi-engine/src/suppress.rs`
  (`in_maintenance`, checked before ack rules) and the incident manager loads them for
  every command (`crates/domovoi-engine/src/manager/actor.rs:238-241`,
  `acks.windows_for(host)`), so a new window takes effect on the next signal without a
  reload.
- Type: `MaintenanceWindow { host, starts_at, ends_at, note }`
  (`crates/domovoi-core/src/ack.rs:203`), `covers(now)` is `[starts_at, ends_at)`. It has
  **no id**.
- Table: `maintenance_windows (host_id, starts_at, ends_at, note, UNIQUE(...))`
  (`migrations/0001_init.sql:168`); store code in
  `crates/domovoi-store/src/repos/knowledge.rs:149-175`; ended windows are purged daily
  (`purge.rs`).
- Port `AckRepo` (`crates/domovoi-core/src/ports/repos/knowledge.rs:66-85`):
  `put_window`, `windows_for`, `purge_windows_ended_before`. **Nothing in production
  calls `put_window`**: there is no API, CLI or UI.
- Pattern to copy: ack rules. API in `crates/domovoi-hub/src/api/rules.rs` (list,
  create, delete, audited), UI in `web/src/features/rules/` (`RulesPage.tsx`,
  `AddRuleForm.tsx`), hooks in `web/src/api/hooks/rules.ts`.

## Steps

1. **Id.** Add `MaintenanceWindowId` (UUIDv7 newtype, next to `AckRuleId` in
   `crates/domovoi-core/src/id.rs`) and `id` to `MaintenanceWindow`. Migration
   `0006_maintenance_window_ids.sql` (or the next free number): create
   `maintenance_windows_v2` with `id TEXT PRIMARY KEY`, `host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE`,
   `starts_at`, `ends_at`, `note`, `created_by TEXT NOT NULL`, `created_at INTEGER NOT NULL`;
   copy rows with a fresh id (`lower(hex(randomblob(16)))` is acceptable for old rows),
   drop the old table, rename, recreate the `ends_at` index.
2. **Port.** `put_window` keeps its name; add `delete_window(id)` and
   `list_windows(active_or_future_at: Timestamp)` (all hosts, ending after the instant,
   sorted by start). Implement in store and testkit; extend the contract tests.
3. **API** new `crates/domovoi-hub/src/api/maintenance.rs` (keep `rules.rs` as is):
   - `GET /api/v1/maintenance` → current and future windows with host names.
   - `POST /api/v1/maintenance` `{ host, starts_at?, duration_minutes | ends_at, note }`:
     `starts_at` defaults to now; refuse `ends_at <= starts_at`, more than 7 days long,
     a note over 200 chars, an unknown host. Audit `maintenance.created`.
   - `POST /api/v1/maintenance/{id}/end`: sets `ends_at = now` (ending early keeps the
     record until the purge). Audit `maintenance.ended`.
   - Regenerate OpenAPI (rule 9); add the POST routes to the `writes` list in
     `crates/domovoi-hub/tests/gates.rs`.
4. **When a window ends.** Check `crates/domovoi-engine/src/notify/` (quiet hours
   release lives in `notify/tick.rs` and `notify/quiet.rs`): incidents that opened during
   a window were stored with `notify: false`. If nothing posts them when the window ends,
   post a card for every incident on that host that is still open and was never posted,
   the same way quiet hours release does, on the minute tick. Write the test first to
   find out.
5. **Web.**
   - Host page: a "Maintenance" button opening a dialog (duration presets 30 min, 1 h,
     2 h, 4 h, custom end; note). While a window is active the host header shows
     "In maintenance until 14:30" and an "End now" button.
   - The global host selector and the Hosts list show a small "maintenance" tag.
   - A "Maintenance" section on the Rules page (rename the page title to
     "Known conditions and maintenance" only if it fits; otherwise a tab) listing
     current and planned windows with "End now".
   - Hooks in `web/src/api/hooks/maintenance.ts`, keys in `queryClient.ts`; invalidate
     only `maintenance` and `hosts`.
6. **Book.** `docs/book/src/concepts/incidents.md`: what a window suppresses (cards,
   reminders, digest lines stay), what it keeps (signals, incidents, triage), and what
   happens when it ends.

## Tests

- Engine: `a_signal_during_maintenance_is_stored_but_not_notified` (likely exists for
  the pure function; add one through the manager using a window stored via the port).
- Engine/hub: `an_incident_opened_during_maintenance_is_posted_when_it_ends` if step 4
  needed a change.
- Hub API: create, list, end; validation errors; audit entries written.
- Web: the dialog creates a window; the host header shows it; End now ends it.

## Done when

- An operator can put a host into maintenance for a set time from the UI, see it, and
  end it early, and nothing about the host is posted to Discord meanwhile.
