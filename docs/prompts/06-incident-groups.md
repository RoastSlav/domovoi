# 06 · Feature: group the incident page by state

## Goal

Without a status filter, `/incidents` shows sections instead of one flat list:

1. **Needs attention**: `open` (not acknowledged yet).
2. **Being handled**: `acked` and `muted` (someone knows; waiting to resolve).
3. **Waiting for confirmation**: `pending` (seen once; Domovoi waits for repeat signals
   before alerting).
4. **Resolved**: `resolved`, collapsed by default, newest 20, with a link to
   `/incidents?status=resolved` for the rest.

With a status filter in the URL, keep today's flat list.

## What is already known (verified)

- `web/src/features/incidents/IncidentListPage.tsx` renders one `useIncidents(filters)`
  infinite list with filters from the URL (`status`, `host`, `severity`).
- `GET /api/v1/incidents` accepts one `status` value per request (`pending`, `open`,
  `acked`, `muted`, `resolved`). No backend change is needed: one query per status.
- `useIncidents` lives in `web/src/api/hooks/incidents.ts`; SSE invalidates every
  `["incidents", "list", ...]` key (`web/src/api/events.ts`), so all groups stay live.
- Existing test `incident_list_renders_filters_from_url_and_paginates` in
  `web/src/features/incidents/incidents.test.tsx` expects a single query when the status
  filter is cleared; that assertion must change.

## Read these files

- `web/src/features/incidents/IncidentListPage.tsx`
- `web/src/features/incidents/incidents.module.css`
- `web/src/api/hooks/incidents.ts`
- `web/src/features/incidents/incidents.test.tsx`

## Steps

1. Move `IncidentRow` out of `IncidentListPage.tsx` into
   `web/src/features/incidents/IncidentRow.tsx` (export it) so the page stays short.
2. Create `web/src/features/incidents/IncidentGroups.tsx`:
   - `const GROUPS = [{ title: "Needs attention", statuses: ["open"], hint: "New problems nobody has acknowledged." }, { title: "Being handled", statuses: ["acked", "muted"], hint: "Acknowledged or muted; they close when the problem clears." }, { title: "Waiting for confirmation", statuses: ["pending"], hint: "Seen once. Domovoi alerts if the check fails again." }, { title: "Resolved", statuses: ["resolved"], hint: "Cleared problems, newest first.", collapsed: true }]`.
   - A `Group` component calls `useIncidents({ ...filters, status })` once per status
     (fixed number of hooks per group, so hook order is stable), merges the first pages,
     sorts by `last_firing_at` descending and renders a `<section aria-labelledby>`
     with an `<h2>` "Title (count)", where count shows `50+` when any query has a
     `next_cursor`. Hide a group with zero items, except "Needs attention", which shows
     "Nothing needs attention." when empty.
   - "Resolved" renders inside `<details>` (closed by default) with at most 20 rows and a
     `<Link to="/incidents?status=resolved">All resolved incidents</Link>`.
   - Each group shows its `hint` as muted text under the heading.
3. In `IncidentListPage`, render `<IncidentGroups filters={filters} hostNames={hostNames} />`
   when `filters.status` is empty, else the existing flat list and pagination.
4. Add small CSS for `.group` and `.groupHint` in `incidents.module.css`.

## Tests

In `incidents.test.tsx`:

- Update the last assertion of `incident_list_renders_filters_from_url_and_paginates`
  to expect one query per status (five queries, each with `limit: 50` and the host and
  severity filters).
- Add `incident_list_groups_by_state`: route `GET /api/v1/incidents` by
  `params.query.status`, returning one incident per status, and assert each lands under
  the right heading, and the Resolved group is collapsed.

## Done when

- `/incidents` shows the four groups; `/incidents?status=open` shows the flat list.
- Live updates still refresh every group.
- `npm run lint && npm run typecheck && npm test` pass.
