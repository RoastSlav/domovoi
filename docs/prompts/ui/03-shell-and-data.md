# UI 03 · The shell, navigation, the host selector everywhere, and data loading

Branch: `ui-shell`. PR: `feat(web): …`. Needs 02. API changes allowed (summary counts,
server-side host filters).

## Background (verified on `main`, paths under `web/src/`)

- Shell: `app/Layout.tsx` (skip link, `Sidebar`, `<main>`, `UpdateBanner`,
  `SetupBanner`, `<Outlet/>`), `app/Sidebar.tsx` (brand, `HostSelect`, Menu button
  for phones with a sheet that closes on navigation and Esc, `NAV` list, Approvals count
  from `useActions("proposed")` that counts **all hosts**, `LiveIndicator`, `UserMenu`).
- Host selector (`app/HostContext.tsx`, `useSelectedHost()`): honoured by
  `features/incidents/IncidentListPage.tsx`, `features/notes/NotesPage.tsx`,
  `features/skills/SkillsPage.tsx`. Ignored by `ApprovalsPage`, `RulesPage`,
  `DigestPage`, `HostsPage`, `SettingsPage` and the Approvals count.
  `features/chat/ChatPage.tsx` keeps its own host in `useState`;
  `features/logs/LogsPage.tsx` uses its own `?source=`. `SkillsPage` shows the first
  host's skills under "All hosts". `HostsPage.tsx` builds `/hosts/${host.name}` without
  `encodeURIComponent`. `app/HostSelect.tsx` shows an empty list on error.
- `app/routes.tsx` imports every page eagerly and renders `null` while the session
  loads (blank screen); `vite.config.ts` has no chunking.
- Data: TanStack Query, keys in `api/queryClient.ts`, SSE invalidation in
  `api/events.ts` (`incident` and `action` events only).
- Over-broad refetching:
  - `api/hooks/incidents.ts`: `useAck`, `useMute`, `useUnmute`, `useDismiss`,
    `useRetryTriage` invalidate all of `keys.incidents`, refetching every infinite list.
    `useAsk` refreshes follow-ups only but the answer adds a timeline line.
  - `api/hooks/profile.ts`: polls forever, even after a profile exists.
  - `features/settings/useApply.ts`: after a restart invalidates every query.
  - `useDeleteNote` leaves the deleted note in the cache.
  - `useHost` has no refetch interval while `useHosts` polls every 30 s, so the host
    detail page's connection state goes stale.

## Steps

1. **Shell layout.**
   - Desktop (≥ 1024 px): a brown sidebar (`--color-sidebar-bg`) with the brand mark,
     grouped navigation, and at the bottom the live status and the user menu. A slim
     top bar in the main column: page title / breadcrumb (`Hosts / nas-01`), the host
     selector, and page-level actions slot. Content max width from tokens, generous
     side padding, `<main>` scrolls, sidebar stays.
   - Tablet (768–1023 px): sidebar collapses to icons with tooltips; expand on demand,
     choice remembered in `localStorage` (try/catch).
   - Phone (< 768 px): top bar with brand, host selector and Menu; navigation opens as a
     full-height sheet (keep today's close-on-navigate and Esc behaviour, add a focus
     trap and return focus). Consider a bottom bar with the 4 most used destinations
     (Incidents, Approvals, Hosts, Chat) if it tests well at 375 px; your call.
   - Navigation groups: **Respond** (Incidents, Approvals, Chat), **Fleet** (Hosts,
     Skills, Logs), **Knowledge** (Known conditions, Notes, Digest), **System**
     (Settings; leave a natural slot for Audit log). Keep the routes and link names
     (tests use them).
   - Counts beside nav items using `Count`: Incidents → open incidents needing action
     (`danger`), Approvals → actions waiting (`pending`), Hosts → offline hosts
     (`danger`, only when > 0). Counts follow the host selector. If computing a count
     needs a full list fetch, add a small endpoint (for example
     `GET /api/v1/overview?host=` → `{ incidents_action_needed, incidents_open,
     approvals_waiting, hosts_offline, hosts_total }`), invalidated by the existing SSE
     events, polled at most every 30 s. Rule 9 applies.
   - Live status: `liveMeta` pill; "Offline" shows a `Banner` ("Live updates paused.
     Reconnecting…") after 5 s offline, cleared on reconnect with a short success toast
     ("Back online") only if the banner had been shown.
   - User menu (`Menu`): username, theme (System/Light/Dark from 01), a slot for
     Account (coming later), Log out.
   - `UpdateBanner` and `SetupBanner` use `Banner`; at most one banner of each kind; a
     dismissed update banner stays dismissed for that version.

2. **Host selector everywhere.**
   - Restyle `HostSelect` as a combobox-like select showing each host's connection
     pill (and, later, a maintenance tag); with many hosts (> 8) allow typing to filter.
     It shows a `LoadError`-style inline message with Retry on error, not an empty list.
   - Approvals and Known conditions filter by the selected host. Actions are paged, so
     filter on the server: add `?host=` to `GET /api/v1/actions` (`ActionFilter.host`
     already exists in the store) and regenerate the OpenAPI; rules may filter on the
     client. The Approvals count follows the selection.
   - Chat uses the selected host as its default and writes back when the user changes
     it on the page.
   - Logs: when a host is selected, default `?source=` to that host.
   - Skills under "All hosts": a short prompt "Choose a host to see its skills" with the
     host list as links, never the first host.
   - Digest, Hosts and Settings stay unfiltered but show a one-line `InlineMessage`
     (`info`) "Showing all hosts" when a host is selected, so it is clear why.
   - `encodeURIComponent` every host and skill name put into a path (search for
     template paths with `${`).

3. **Routing and code splitting.**
   - `routes.tsx`: Incidents (list and detail), Login and Setup stay eager; every other
     page is `React.lazy`. Wrap `<Outlet/>` in `Layout.tsx` in `<Suspense>` with a
     page-shaped skeleton (`PageHeader` skeleton + content skeleton), not a spinner.
   - Prefetch a route's chunk on nav link hover/focus (call the lazy import).
   - `RequireSession` renders a full-page skeleton of the shell while the session loads,
     never `null`; its "Cannot reach the hub" state uses `LoadError`.
   - `vite.config.ts`: `react-markdown` and its dependencies in their own chunk
     (`build.rollupOptions.output.manualChunks`, or the Vite 8 equivalent), and any
     large new dependency from 02 likewise.
   - Each route sets `document.title` ("nas-01 · Hosts · Domovoi") and moves focus to
     the page heading on navigation (not on first load), announced politely.
   - A real 404 page inside the shell with a link back to Incidents; remove
     `app/Placeholder.tsx` if nothing uses it.

4. **Narrow invalidation and polling.**
   - `useAck`, `useMute`, `useUnmute`, `useDismiss`, `useRetryTriage`: invalidate
     `keys.incident(id)` and only the list queries (`["incidents", "list"]`), not
     `followups`. Where it is safe, apply the change optimistically to the cached
     detail and lists and roll back on error (the toast from 02 reports the failure).
   - `useAsk`: also invalidate `keys.incident(id)`.
   - `useProfile`: `refetchInterval: (q) => (q.state.data ? false : PROFILE_POLL_MS)`.
   - `useApply`: after a restart invalidate `settings`, `hosts`, `session` and
     `updates` only.
   - `useDeleteNote`: `removeQueries(keys.note(id))`.
   - `useHost`: same interval as `useHosts`, or read the host from `useHosts` data.
   - SSE: if the overview endpoint exists, invalidate it on `incident` and `action`
     events. Pause polling while the tab is hidden (TanStack's
     `refetchIntervalInBackground: false`, check it is the default you rely on).

5. **Catalogue and docs.** Add the shell states (desktop, collapsed, phone sheet,
   offline banner) to `/dev/design` if practical; add "Add a page" and "Add a nav item"
   recipes to `DESIGN.md` (lazy route, `PageHeader`, `QueryBoundary`, document title,
   nav group, count).

## Tests

- Layout: nav groups render with their links; the phone sheet opens, traps focus,
  closes on Esc and returns focus; collapsed sidebar items have tooltips with names.
- Counts: Approvals count follows the selected host; Hosts count hides at zero.
- Host selector: Approvals is filtered (server param sent); Skills under All hosts
  shows the prompt; Chat defaults to the selected host; HostSelect error shows Retry.
- Hook tests (`api/hooks/incidents.test.ts`): acking invalidates the detail and the
  lists but not follow-ups; a failed optimistic ack rolls back.
- Profile polling stops once a profile is loaded (fake timers).
- `RequireSession` shows a skeleton, not nothing, while loading.
- Hub tests for any new endpoint or query parameter.
- Tests rendering `AppRoutes` may need `await screen.findBy…` for lazy pages.

## Done when

- The shell looks and behaves like a finished product at 375, 768, 1280 and 1920 px in
  both themes.
- Every page follows the host selector or says it shows all hosts.
- An ack refetches one incident and the visible lists, nothing else; the initial bundle
  is smaller than the baseline.

## Out of scope

- Page content redesigns (04–06).
