# 23 · Web: denser page layouts, then a targeted performance pass

Done already: 23a, the app shell (sidebar, menu on phones, global host selector in
`web/src/app/HostContext.tsx`, layout primitives `.card`, `.toolbar`, `.grid`, `.table`,
`.tableWrap` in `web/src/components/ui.module.css:137-185`).

Do 42 first (host selector everywhere, error states), so the layouts start from
consistent pages. Web only; no API changes. No new npm dependencies.

## Background (verified on `main`, paths under `web/src/`)

- Incidents list: `features/incidents/IncidentRow.tsx` renders each incident as a card
  row (`<li className={styles.row}>`: badge and title link, then one meta line with
  host, status, last fired, occurrences, triage note). Groups by state come from
  `IncidentGroups.tsx`.
- Incident detail: `features/incidents/IncidentDetailPage.tsx` stacks card, details,
  chat, controls, actions, skills used and timeline in one column.
- Hosts: already a table (`features/hosts/HostsPage.tsx:75-76`). Settings already has a
  section list next to the sections (`features/settings/settings.module.css:22-31`).
- `app/routes.tsx` imports every page eagerly; `vite.config.ts` has no chunking.
- Data: TanStack Query, keys in `api/queryClient.ts`, SSE invalidation in
  `api/events.ts` (only `incident` and `action` events).
- Tests look things up by role and label; keep accessible names stable or update the
  tests.

---

## 23b · Denser page layouts

### Steps

1. **Incidents list**: on ≥ 900px render each group as a `.table` (badge, title link,
   host, check, status, last fired, occurrences, triage note); keep the card rows below
   900px. The title link keeps its accessible name.
2. **Incident detail**: two columns on ≥ 1100px: left (2fr) card, details, chat; right
   (1fr) controls, proposed actions, skills used, timeline. One column below.
3. **Host detail**: header with status, version, open incidents count and (after 33)
   maintenance state on one line; tabs below. Remove duplicate headings inside tabs.
4. **Spacing**: headings `margin: 0 0 var(--space-3)`, sections
   `margin-top: var(--space-5)`; long values `overflow-wrap: anywhere`.
5. Check every page at 375, 768, 1280 and 1920 px wide (run `npm run dev` against
   `cargo run -p domovoi-hub --example dev_hub`, Playwright is installed) and fix
   horizontal scrolling.

### Tests

- Existing page tests pass; update selectors only where the structure changed.
- A test that the incident list renders a table on a wide viewport (mock `matchMedia`)
  and rows on a narrow one.

---

## 23c · Targeted performance pass

The UI is small (about 8.7k lines); the waste is in what gets refetched, not in
rendering. Do these, measure, and stop.

### Steps

1. **Narrow invalidation** (`api/hooks/incidents.ts`):
   - `useAck`, `useMute`, `useUnmute`, `useDismiss` and `useRetryTriage` invalidate all
     of `keys.incidents` (`:63`, `:105`, `:123-124`), which refetches every infinite
     list on the page. Invalidate `keys.incident(id)` and only the list queries
     (`["incidents", "list"]`); leave `followups` alone.
   - `useAsk` (`:139`) refreshes follow-ups only; also invalidate `keys.incident(id)`
     (the answer adds a timeline line).
2. **Profile polling** (`api/hooks/profile.ts`): `refetchInterval` only while the host
   has no profile yet: `refetchInterval: (query) => (query.state.data ? false : PROFILE_POLL_MS)`.
3. **Settings apply** (`features/settings/useApply.ts:25`): after a restart, invalidate
   `settings`, `hosts`, `session` and `updates`, not every query.
4. **Deleted note**: `useDeleteNote` removes `keys.note(id)` from the cache
   (`removeQueries`).
5. **Host detail freshness**: `useHost` has no refetch interval while `useHosts` polls
   every 30 s, so the detail page's connection state goes stale. Give `useHost` the
   same interval, or read the host from the `useHosts` data.
6. **Code splitting**: in `routes.tsx`, lazy-load Settings, Notes (with the Markdown
   editor), Logs, Skills, Digest and Chat with `React.lazy`, and wrap `<Outlet />` in
   `Layout.tsx` with `<Suspense fallback={<Loading />}>` (`Loading` from 42). In
   `vite.config.ts`, put `react-markdown` and its dependencies in their own chunk
   (`build.rollupOptions.output.manualChunks`).
7. **Measure**: `npm run build` before and after; put the largest chunk's size before and
   after in the PR description.

### Tests

- Hook tests (new `api/hooks/incidents.test.ts`): acking invalidates the detail and the
  lists but not follow-ups.
- Profile: polling stops once a profile is loaded (fake timers).
- `npm test` passes; tests rendering `AppRoutes` may need `await screen.findBy…`.

### Done when (23b and 23c)

- Desktop pages use the width; no horizontal scroll at 375 px.
- An ack refetches one incident and the visible lists, nothing else; the initial bundle
  is smaller.
