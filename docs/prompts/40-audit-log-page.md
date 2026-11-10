# 40 · Feature: an audit log page

The audit log is append-only and well filled (approvals, denials, dispatches, settings
saves, chat interactions, dismissals) but nothing shows it. Do it after 35 so chat
proposals show the asker.

## Background (verified on `main`)

- Port: `AuditLog::list(&AuditFilter { actor, kind, subject, since }, &Page)`, newest
  first (`crates/domovoi-core/src/ports/repos/audit.rs`). Implemented in
  `crates/domovoi-store/src/repos/audit.rs`.
- `AuditEvent { at, actor, kind, subject, detail }`; actors look like `user:<id>`,
  `chat:<platform>:<user>`, `system:<component>`; subjects like `action:<id>`,
  `incident:<id>`.
- Kinds in use: `action.*` (`crates/domovoi-engine/src/actions/audit.rs`), `chat.*`
  (`crates/domovoi-hub/src/auth/interactions/mod.rs`), settings and others: grep
  `kind:` and `AuditEvent {` under `crates/*/src` for the full list.
- Pagination helper and cursor style: copy `GET /api/v1/actions`
  (`crates/domovoi-hub/src/api/actions.rs:45-68`).

## Steps

1. **API** new `crates/domovoi-hub/src/api/audit.rs`:
   `GET /api/v1/audit?kind=&actor=&subject=&since=&cursor=&limit=` → `{ items, next_cursor }`.
   Each item: `at`, `actor`, `actor_label` (for `user:<id>` the username, for
   `chat:discord:<id>` "Discord <id>", for `system:x` "Domovoi (x)"), `kind`,
   `subject`, `subject_link` (`/incidents/<id>` for incidents, the incident of an action
   for `action:<id>` when the detail has `incident`, else `null`), `detail` (JSON).
   Run every string in `detail` through the redactor before returning it. Read-only,
   session required. Regenerate OpenAPI.
2. **Kind prefix filter.** `kind` matching is exact in the port. Add
   `kind_prefix: Option<String>` to `AuditFilter` (store: `kind LIKE ? || '%'` with the
   prefix escaped) so the UI can filter `action.` or `chat.`.
3. **Web** `web/src/features/audit/AuditPage.tsx`, route `/audit`, sidebar entry under
   Settings (or near Approvals; match the shell's grouping):
   - A table: time (`RelativeTime` with the full time in `title`), who, what (kind as a
     short phrase: map the known kinds to words, fall back to the kind), subject link,
     and a "Details" toggle showing `detail` as formatted JSON.
   - Filters: kind group (All, Actions, Chat, Settings, Incidents), and "since"
     (24 h, 7 days, 30 days, all). Infinite scroll with "Load more" like the incident
     list.
   - Loading, empty ("Nothing recorded yet") and error states with a retry button.
4. **Book.** `docs/book/src/security.md`: where to see the audit log and that it cannot
   be edited.

## Tests

- Hub API: filters by kind prefix and since; labels; the detail is redacted.
- Store: `kind_prefix` matches `action.` but not `actions.` and escapes `%`.
- Web: renders rows, filters, opens details, shows the error state.

## Done when

- An operator can answer "who approved this, from where, and what ran" from the UI.
