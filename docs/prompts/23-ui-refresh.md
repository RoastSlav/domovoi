# 23 · Feature: a denser, more modern, faster web UI

Three prompts. 23a (app shell and host selector) is best done early, so later pages use
it; 23b (page layouts) and 23c (performance) can come last. Web only; no API changes
unless a step says so.

## Background (verified)

- Shell: `web/src/app/Layout.tsx` + `Layout.module.css`: a top bar (brand, host pills,
  live indicator, user) and a horizontal nav, then `<main>` capped at **960px**
  (`.bar, .nav, .main { max-width: 960px }`), which leaves wide empty margins on
  desktop.
- Tokens: `web/src/app/global.css` defines colours (light and dark), `--radius: 6px`,
  system font. Shared styles in `web/src/components/ui.module.css` (buttons, empty
  state, tabs, dialogs); each feature has its own `*.module.css`.
- Routes: `web/src/app/routes.tsx` imports every page eagerly; no code splitting.
- Pages filter by host in different ways: incidents `?host=`, skills `?host=`, notes a
  `HostScope` component; there is no app-wide host selection.
- Data: TanStack Query (`web/src/api/queryClient.ts`, `staleTime: 10_000`), SSE
  invalidation in `web/src/api/events.ts`; hosts poll every 30 s (`useHosts`).
- Tests: Vitest + Testing Library; tests look things up by role and label, so keep
  accessible names stable or update the tests.
- No new npm dependencies (rule 7): plain CSS modules, no UI kit.

---

## 23a · App shell with a sidebar and a global host selector

### Steps

1. **Tokens** (`global.css`): add a spacing scale (`--space-1: 0.25rem` … `--space-6: 2rem`),
   `--radius: 8px`, `--radius-sm: 6px`, `--shadow-1: 0 1px 2px rgb(0 0 0 / 0.06), 0 1px 3px rgb(0 0 0 / 0.1)`,
   `--sidebar-width: 15rem`, `--content-max: 1400px`, font sizes `--text-sm: 0.875rem`,
   `--text-base: 0.95rem`. Keep every existing colour token and the dark-mode block;
   add `--surface-2` (a step darker than `--surface`) for hover and selected states.
2. **Host context.** New `web/src/app/HostContext.tsx`: `HostProvider` and
   `useSelectedHost(): [string | undefined, (name: string | undefined) => void]`. The
   value comes from the URL `?host=` when present, else `localStorage` key
   `selected-host` (try/catch), else "all hosts" (`undefined`). Setting it updates the
   URL search param of the current page and `localStorage`. Wrap the layout in it.
3. **Layout** (`Layout.tsx` + CSS), CSS grid:
   - ≥ 900px: a fixed left sidebar (`--sidebar-width`) with the brand, the **host
     selector**, the nav links (with small inline SVG icons, `aria-hidden`), and at the
     bottom the live indicator and the user menu (username, Log out). Content area to
     the right, padding `--space-5`, `max-width: var(--content-max)`.
   - < 900px: a top bar with the brand, the host selector and a "Menu" button
     (`aria-expanded`, `aria-controls`) that opens the nav as a sheet; close it on
     navigation and on Escape.
   - Host selector: a `<select aria-label="Host">` with "All hosts" and every host,
     each option prefixed with ● / ○ by connection state. Replace `HostPills` with it.
   - Show a count badge on "Approvals" with the number of `proposed` actions
     (`useActions("proposed")` from `web/src/api/hooks/actions.ts`).
4. **Use the selection.** `IncidentListPage` and `SkillsPage` read the host from
   `useSelectedHost()` instead of their own `?host=` handling (the URL param keeps
   working because the context reads it); `NotesPage` defaults its `HostScope` to it.
   Keep the in-page host `<select>` on Incidents in sync or remove it (then update the
   tests that use the "Host" label: the global selector now has that label).
5. **Layout primitives** in `ui.module.css`: `.card` (surface, border, radius, padding
   `--space-4`, `--shadow-1`), `.toolbar` (flex, gap, wrap), `.grid` (auto-fill columns
   `minmax(18rem, 1fr)`), `.table` (full width, `--text-sm`, row hover `--surface-2`,
   sticky header), `.muted`. Buttons: 2rem min-height, `--text-sm`, a `ghost` variant in
   `Button.tsx`.

### Tests

- `web/src/app/` test: the sidebar renders nav links; on a narrow viewport (mock
  `matchMedia` or test the Menu button directly) Menu toggles the nav; choosing a host
  in the selector updates `?host=`.
- Update existing tests broken by the moved host select.

---

## 23b · Denser page layouts

### Steps

1. **Incidents list**: on ≥ 900px render the rows as a table (badge, title, host, check,
   status, last fired, occurrences) using `.table`; on narrow screens keep the card rows.
   Keep link names (the incident title) unchanged.
2. **Incident detail**: two columns on ≥ 1100px: left (2fr) the card, details, chat;
   right (1fr) controls, actions, skills used, timeline. One column below that.
3. **Hosts**: a `.grid` of host cards (name, status dot and text, version and
   "Outdated" tag, open incidents count linking to `/incidents?host=`), instead of the
   table, with the same accessible names.
4. **Settings**: a left list of section links (anchors) on ≥ 900px and the sections on
   the right; one column on phones.
5. Reduce vertical gaps: headings `margin: 0 0 var(--space-3)`, sections
   `margin-top: var(--space-5)`.
6. Check every page at 375px, 768px, 1280px and 1920px wide (Playwright is
   pre-installed: run `npm run dev` against `cargo run -p domovoi-hub --example dev_hub`
   and take screenshots) and fix overflow; long values use `overflow-wrap: anywhere`.

### Tests

Existing page tests keep passing (update selectors only where the structure changed).

---

## 23c · React performance

### Steps

1. **Code splitting.** In `routes.tsx`, load every page except `IncidentListPage`,
   `LoginPage` and `SetupPage` with `React.lazy(() => import("...").then((m) => ({ default: m.PageName })))`
   and wrap `<Outlet />` in `Layout.tsx` with `<Suspense fallback={<p>Loading…</p>}>`.
   In `vite.config.ts`, put `react-markdown` and its dependencies in their own chunk
   (`build.rollupOptions.output.manualChunks`).
2. **Fewer re-renders.** Wrap list row components (`IncidentRow`, `SkillCard`, host
   cards, `ActionItem`) in `memo`; make sure their props are stable (no inline objects).
   `useHostNames` already memoises; use it instead of mapping `hosts.data` in render.
3. **Fewer requests.** `useHosts`: `staleTime: 15_000`. Incident detail: on an SSE
   event only invalidate that incident's detail and the lists (already so: keep it).
   Set `refetchOnWindowFocus: false` for the settings schema and settings queries.
4. **Measure.** Run `npm run build` before and after and put the size of the largest JS
   chunk before/after in the commit message.

### Tests

- `npm test` passes (lazy pages: tests that render a page directly are unaffected; tests
  that render `AppRoutes` may need `await screen.findBy…`).

### Done when (all of 23)

- Sidebar layout on desktop, menu on phones, a global host selector, content up to
  1400px wide, denser tables and grids, no horizontal scroll at 375px, smaller initial
  bundle.
- `npm run lint && npm run typecheck && npm test && npm run build` pass.
