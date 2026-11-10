# 42 · Web: the host selector works everywhere, and every page shows its errors

Web only. Do it before 23b, so the new layouts start from consistent pages.

## Background (verified on `main`, paths under `web/src/`)

**Host selector** (`app/HostContext.tsx`, `useSelectedHost()`):

- Honoured by `features/incidents/IncidentListPage.tsx:71`, `features/notes/NotesPage.tsx:84`,
  `features/skills/SkillsPage.tsx:13`.
- Ignored by `ApprovalsPage`, `RulesPage`, `DigestPage`, `HostsPage`, `SettingsPage`
  and the sidebar Approvals badge (`app/Sidebar.tsx:27-29`, counts all hosts).
- `features/chat/ChatPage.tsx:70` keeps its own host in `useState`;
  `features/logs/LogsPage.tsx` uses its own `?source=`.
- `SkillsPage` shows the **first** host's skills when "All hosts" is selected
  (`SkillsPage.tsx:16,27-31`).
- `HostsPage.tsx` builds `/hosts/${host.name}` without `encodeURIComponent`.

**Error and loading states:**

- `features/hosts/HostDetailPage.tsx:41-49` (`RecentIncidents`): an error shows
  "No incidents.".
- `SkillsPage.tsx:14,26`: a failed hosts query leaves the page blank.
- `features/setup/FirstRun.tsx:399-400`: any error, including a network error, reads
  "Settings are managed in hub.toml".
- `ChatPage.tsx:68-75` and `features/skills/SkillDetail.tsx:46-52`: no loading or error
  handling for their queries. `features/notes/HostScope.tsx:13`,
  `features/rules/AddRuleForm.tsx:91` and `app/HostSelect.tsx:17` show an empty list on
  error.
- Most "Loading…" texts are bare `<p>` (no `role="status"`); `app/routes.tsx:49`
  renders `null` while the session loads (blank screen). Error states are `EmptyState`
  titles with no retry, except `routes.tsx:43-45`.

**Small accessibility gaps:**

- `components/Tabs.tsx:14-20`: arrow keys change the selected tab but do not move focus;
  no Home/End. No test.
- `features/logs/LogViewer.tsx:82` always says "Auto-refresh every 5 s", even when
  `LogsPage` turned refresh off.

## Steps

1. **Shared states.** In `components/`, add `Loading` (`<p role="status">Loading…</p>`)
   and `LoadError({ error, onRetry })` (message from `ApiFailure`, a Retry button calling
   `refetch`). Use them on every page and panel listed above. `RequireSession` shows
   `Loading` instead of `null`.
2. **Host selector.**
   - Approvals and Rules filter by the selected host (client-side filter on `host` /
     `host_id` is fine; add an API `host` query param only if the list is paged, as
     actions are: then add `?host=` to `GET /api/v1/actions` using `ActionFilter.host`,
     which already exists, and regenerate OpenAPI). The Approvals badge counts the
     selected host's proposals.
   - Chat uses the selected host as its default and writes back when changed in the
     page.
   - Logs: when a host is selected, default `?source=` to that host.
   - Skills with "All hosts": show a short prompt "Choose a host to see its skills"
     with the host list, not the first host.
   - Digest, Hosts and Settings: leave unfiltered, but show a one-line note "Showing all
     hosts" when a host is selected, so it is clear why.
3. `encodeURIComponent` every host name and skill name put into a path.
4. **Tabs**: on ArrowLeft/Right/Home/End, select and `focus()` the new tab (keep
   `tabIndex` roving). Add a test.
5. **LogViewer**: take `refreshMs: number | null` and print the line only when set.

## Tests

- One test per fixed error state (mock a failing query, expect the error text and a
  working Retry).
- Host selector: Approvals filtered; Skills with All hosts shows the prompt; Chat
  defaults to the selected host.
- Tabs keyboard test.

## Done when

- No page goes blank or claims "nothing here" when a request failed, and every page
  either follows the host selector or says it shows all hosts.
