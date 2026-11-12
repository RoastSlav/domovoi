# B - Frontend investigation (web/ vs design-pack docs 12-17)

Read-only. Branch `overhaul-investigation`, HEAD `64117f0`. Paths relative to `C:\Programming\domovoi`.
Labels: FACT (read in code/docs), INFERENCE (derived), RECOMMENDATION.
`web/src/...` abbreviated `src/...`. Doc refs: `12`..`17` = `docs/design-pack/NN-ui-*.md`.

---

## 1. Current structure

### 1.1 Stack and shell
- FACT: React 19, react-router 8 (data router), TanStack Query 5, openapi-fetch + generated `schema.d.ts`, lucide icons behind `Icon`; no state library, no graph/chart/virtualisation/animation library (`web/package.json`; grep for "virtual" in `src/components` returns nothing).
- FACT: `src/app/App.tsx` = `QueryClientProvider > ToastProvider > RouterProvider`; `createBrowserRouter([{path:"*"}])` is used only so `useBlocker`-style leave guards work (`App.tsx:117-119`). Real route table is `src/app/routes.tsx:48-90`.
- FACT: Shell = `Layout.tsx` (`Layout.tsx:36-110`): `HostProvider > PageTitleProvider > Sidebar + AppPalette + TopBar + <main><banners/><Suspense><Outlet/>`. `useLiveEvents()` is called once here (`Layout.tsx:37`).
- FACT: No right-hand panel/inspector/split-pane slot exists in the shell; `<main>` is a single column capped by `--content-max` 1200 px (`DESIGN.md:72`, `Layout.tsx:87-104`).

### 1.2 Routes / pages (`routes.tsx:53-84`, `pages.ts:117-131`, `nav.ts:167-220`)
| Route | Page | Loading |
|---|---|---|
| `/setup`, `/login` | SetupPage, LoginPage | eager |
| `/` | OverviewPage | lazy (`PAGES.overview`) |
| `/incidents`, `/incidents/:id` | IncidentListPage, IncidentDetailPage | eager (first download, `routes.tsx:9-10`) |
| `/chat` | ChatPage | lazy |
| `/approvals` | ApprovalsPage | lazy |
| `/rules` ("Known conditions") | RulesPage | lazy |
| `/hosts`, `/hosts/:name` | HostsPage, HostDetailPage (tabs Overview/Profile/Checks/Logs/SSH logins) | lazy |
| `/skills` | SkillsPage (+ detail dialog) | lazy |
| `/notes`, `/notes/:id` | NotesPage, NoteEditorPage (`useLeaveGuard`) | lazy |
| `/digest`, `/logs` | DigestPage, LogsPage | lazy |
| `/settings`, `/settings/:section` | SettingsPage (+ sections/fields/form) | lazy |
| `/welcome` | FirstRun wizard | lazy |
| `/dev/design` | DesignPage (dev server only, `routes.tsx:18`) | lazy, absent from prod |
- FACT: nav groups Respond / Fleet / Knowledge / System (`nav.ts:167-220`); counts come only from `GET /overview` (`nav.ts:137-145`, `api/hooks/overview.ts`).
- FACT: no Operations, Capabilities, Policies, Integrations, Audit, Notifications, Topology, Knowledge-graph or Operator routes exist. There is a comment "The audit log goes here when it exists" (`nav.ts:216`).

### 1.3 Kit (`src/components/`, ~70 components; barrel `index.ts`)
- Status/feedback: StatusPill, BadgePill, Tag, Count, ToneStripe, Toast (+`useToast`), InlineMessage, Banner, InlineError, ErrorDetail, `useActionFeedback`.
- Hints: Tooltip, InfoTip, DocsLink, Kbd, Highlight.
- Buttons: Button (variant/size/icon/loading/`disabledReason`/`hint`), IconButton, LinkButton, CopyButton.
- States: QueryBoundary, Loading, Skeleton, Spinner, Progress, LoadError, EmptyState.
- Layout: PageHeader, Section, Card, Text, Stack, Cluster, Grid, AuthCard, BrandMark.
- Data: DataTable (+DataRow; sortable, stacked cards on phone, `rowHref`/`onRowClick`, truncate, fixed layout), Stepper, Timeline (oldest-first, "Show all N" after 8, `Timeline.tsx:15,27`), KeyValueList, FilterChips, Tabs (controlled via URL), RelativeTime, Countdown, CopyBlock, Markdown (own chunk), ChatThread/ChatComposer/ChatToolTags, StatTile, FormErrorSummary, SubmitBar.
- Forms: Field, TextInput, NumberInput, Textarea, Select, PasswordInput, Checkbox, Switch, SegmentedControl, DropZone.
- Overlays: Dialog, ConfirmDialog, PasswordPrompt, Menu, OptionPicker, Popover, CommandPalette (lazy chunk).
- FACT: There is no tree, split pane/resizable divider, side drawer/inspector, canvas/graph, virtual list, wizard shell beyond `Stepper`, chart/sparkline, matrix/grid editor, rich/code editor, toggle-group persisted preference store, or notification-centre component.

### 1.4 Data layer
- FACT: only door to the hub is `api` (openapi-fetch, typed from `src/api/openapi.json`), with CSRF middleware (`api/client.ts:25-36`) and 401 -> `/login` or `/setup` middleware (`client.ts:51-62`). Lint forbids `fetch` outside `src/api` (`DESIGN.md:377`). Exception in practice: `api/health.ts:19` raw `fetch("/healthz")`.
- FACT: hooks per domain in `src/api/hooks/` (actions, chat, hosts, incidents, notes, overview, profile, rules, session, settings, system, updates). Query keys centralised in `api/queryClient.ts:5-33`. Defaults: `staleTime 10s`, retry once for 5xx only, mutations never retry (`queryClient.ts:40-55`).
- FACT: errors become `ApiFailure(status, code, message, retryAfter, issues)` via `call()` (`api/types.ts:~90-145`); words come only from `design/messages.ts`.
- FACT: REST surface is 50 operations. Resource families: ack-rules, actions (list/approve/deny), advisories, auth, chat (single POST), digest, events (SSE), hosts (+checks/docker/enrollment/logs/profile/revoke/skills), incidents (+ack/ask/dismiss/followups/mute/triage/unmute), notes (+search/import), overview, session, settings (+schema/models/test), system/logs, system/update, healthz, metrics (`openapi.json` paths).
- FACT: list pagination = cursor on incidents only (`useInfiniteQuery`, `api/hooks/incidents.ts:41-51`, 50/page); actions are a fixed `limit: 200` fetch (`api/hooks/actions.ts:20`); hosts, skills, notes, rules have no pagination.

### 1.5 Live updates
- FACT: one browser `EventSource` on `GET /api/v1/events` (`api/events.ts:86`), two event names `incident` and `action`, payload `UiEvent {kind, event, incident_id, action_id?, status?}` (schema). No `id:` field, no resume token; server skips `TriageRequested`, `TriageDegraded`, `ActionApproved` (`crates/domovoi-hub/src/api/events.rs:48-52`).
- FACT: client treats an event purely as a cache-invalidation hint: `applyEvent` invalidates incident lists, that incident, overview, and actions when `kind==="action"` (`events.ts:34-44`). Payload content is never rendered.
- FACT: server side is a tokio `broadcast`; a slow client silently loses events (`events.rs:76-80`, "the UI refetches on the next one").
- FACT: reconnect = exponential backoff 1 s -> 30 s (`events.ts:13-26`); status "connecting/live/offline"; "paused" banner only after 5 s down (`Layout.tsx:90-94`); after any reconnect the client blanket-invalidates incidents, actions, overview (`events.ts:88-93`).
- FACT: everything else is polling: overview 30 s (`overview.ts:7`), hosts + host detail 30 s (`hosts.ts:15,47`), logs on a refresh interval with tab-hidden pause (`hosts.ts:83-85`, `system.ts:66-68`), update check, profile poll while empty (`profile.ts:18`), enrollment 2 s (`hosts.ts:18`).
- FACT: host connect/disconnect, check results, skill runs, settings, notes, log lines have no push path.
- FACT: transient "news" is toast-only: `useActionOutcomes` (`features/approvals/useActionOutcomes.ts`) tells the person when an action leaves running; `useStatusNews` for incident status changes. Both derive from diffing refetched data, not from events.

### 1.6 Mutations and feedback
- FACT: `useActionFeedback(mutation, {success, error, errorDisplay, undo})` wraps every `useMutation` (`components/useActionFeedback.ts:243-291`): success toast (optionally with Undo), error toast or inline, never rejects. ESLint rule fails `.mutate(` in a file without it unless `// feedback-ok:` (`DESIGN.md:375`); 4 files use the escape hatch (`IncidentControls.tsx:1`, `IncidentRowMenu.tsx:1`, `settings/useApply.ts:1`, `setup/useWizard.ts:1`).
- FACT: 46 files reference `useActionFeedback`; 32 `.mutate(` call sites in features.
- FACT: mutations invalidate by key prefix, not optimistic (`actions.ts:48-60`, `incidents.ts` `refreshIncident`). `ownChanges` map (`incidents.ts:78-87`, `OWN_CHANGE_MS` 2 s) dedups "this changed under you" toasts for self-initiated changes; it is a time-window heuristic.

### 1.7 Loading / error / empty / offline / partial
- FACT: `QueryBoundary` (`components/QueryBoundary.tsx:151-189`): no data + error -> `LoadError` with Retry; no data + pending -> skeleton (`role=status`) or `Loading`; empty -> `EmptyState`; data present -> children. If data exists and a refetch fails, the error is silently dropped (docstring says so, `:143-149`).
- FACT: no component or hook surfaces `isFetching`, `dataUpdatedAt`, `isStale`, `isRefetchError` (grep: only `isFetchingNextPage` in two pages). So there is no "stale" or "refreshing" or "last updated" UI anywhere.
- FACT: "offline" = (a) global live-status dot + 5 s banner; (b) per-host `hostConnectionOf` pill; (c) chat inline "host is offline" warning (`ChatPage.tsx:376-380`). No per-section "source unavailable" notion; no 403/permission-denied state (single-user hub, so none needed today).
- FACT: partial failure is handled ad hoc: Overview combines `overview` + `hosts` into one boundary (`OverviewPage.tsx:106-114`), so either failing blanks both tiles; other columns have separate boundaries. No shared pattern.
- FACT: unknown enum values are rendered as neutral pills with the raw value (`DESIGN.md:129`; `tone.ts`; `timeline.ts` KINDS fallback).

### 1.8 State management
- FACT: server state = TanStack Query. URL state = `useSearchParams` for incident filters, host tab, logs, settings section, `?host=` (`IncidentListPage.tsx:103`, `HostDetailPage.tsx:43`, `LogsPage.tsx:36`). Client prefs = `localStorage` (selected host `HostContext.tsx:144-167`, sidebar rail `useSidebarRail.ts`, theme `design/theme.ts`, hosts cards/table `useHostsView.ts`) and `sessionStorage` (chat transcripts `features/chat/conversation.ts:23-43`). No per-user server-side preferences (no endpoint).
- FACT: global host scope = `HostContext` (`app/HostContext.tsx:187-218`), precedence route host > `?host=` > localStorage; selecting on a host page navigates.

### 1.9 Accessibility and perf guardrails (what actually enforces)
- FACT (`DESIGN.md:359-381`, `eslint/domovoi-rules.js`, `src/guardrails.test.ts`): raw colour only in `tokens.css`; dark blocks identical; WCAG AA token-pair test; every status has label/tone/hint/icon; every error has words; voice test; axe runs automatically over whatever each page test leaves on screen (`test/axe.ts`, `setup.ts`); lint bans inline style, `title=`, raw `<button>`, verdict literals, bare `disabled`, deep kit imports, `fetch` outside api, files > 250 lines.
- FACT: axe in jsdom cannot check colour-contrast or layout (`axe.ts` comment); contrast is proven by token-pair arithmetic only.
- FACT: perf budget: initial JS gzip <= 170 kB (now 154.8), any chunk <= 120 kB (largest 110.7), CSS <= 30 kB (14.6) enforced by `scripts/size.mjs` inside `npm run build` (`DESIGN.md:383-401`). Font (~48 kB) outside the budget. CSP `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; ... ` (`crates/domovoi-hub/src/csp.rs:10`).
- FACT: no browser-level (Playwright) tests in CI. Playwright is a devDependency used only by manual `scripts/screens.mjs` (screenshots at 4 widths x 2 themes); `.github/workflows` has no playwright/e2e reference. 69 vitest files.
- FACT: motion tokens are two values, 120 ms and 180 ms, zero under reduced-motion (`styles/tokens.css:175-176,398-401`); 7 `@keyframes` across CSS modules (spinner, shimmer, progress slide, toast/overlay/palette/submitbar enter). No JS animation.

---

## 2. Inconsistencies in the current state

1. FACT: **Live vs polling split.** Incidents/actions/overview are SSE-invalidated; hosts, host detail, logs, profile, updates are polled (30 s). A host going offline can take up to 30 s to show while the "Live" dot is green (`hosts.ts:15`, `events.ts`). DESIGN.md says the Live indicator "is truthful" (`DESIGN.md:356`) but "Live" only describes incident/action push.
2. FACT: **Overview invalidation overlap.** `useOverview` polls every 30 s AND is invalidated by every incident/action event and every mutation; the key `["overview"]` prefix-matches both host-scoped variants (`queryClient.ts:214-215`). Every event for one incident refetches overview for all open host scopes.
3. FACT: **Event payload unused.** The SSE payload includes `action_id`, `status`, `event` name but is discarded; yet separate toast logic re-derives status transitions by diffing (`useActionOutcomes`, `useStatusNews`). Two mechanisms for the same fact, one time-window heuristic (`OWN_CHANGE_MS`).
4. FACT: **Refresh failure invisible.** `QueryBoundary` keeps stale data and shows nothing on a failed refetch; no freshness indicator anywhere. Contradicts DESIGN principle 6 "Never lying" in spirit and spec 15 s10 (refresh error vs first-load error).
5. FACT: **Guard bypasses.** `feedback-ok` header exempts a file from the mutation lint without proving the wrapping (4 files); `api/health.ts` uses raw `fetch`.
6. FACT: **Inconsistent not-found / error handling.** Incident detail hand-rolls a 404 -> EmptyState and skeleton (`IncidentDetailPage.tsx:62-90`) while Notes editor and others use `LoadError`/`Skeleton` ad hoc; DESIGN.md says use `QueryBoundary` "around everything a page loads" but 24 of 94 data-using feature files use it.
7. FACT: **Host scope semantics.** Pages either follow the selector (Overview, Approvals, Incidents, Chat, Logs, Notes, Rules, Skills) or declare `AllHostsNote` (Digest, Hosts, Settings); host-detail forces the selection but restores the user's choice on leaving (`HostContext.tsx:177-186`). Spec 13 s2 asks for "a documented, predictable rule"; current rule is documented in code comments only and differs from DESIGN.md line 341 ("leaving keeps it").
8. FACT: **Chat is not durable and not streaming.** One POST returns the whole answer (`api/hooks/chat.ts`), history lives in `sessionStorage` and is resent each call (`ChatPage.tsx:343-358`); incident chat is a separate server-side follow-up store (`/incidents/{id}/ask`, `followups`). Two chat models with different persistence.
9. FACT: **Pagination inconsistency.** Incidents cursor-paged; actions hard-capped at 200 with no "more"; nothing virtualised. Spec requires pagination/virtualisation for "large lists".
10. FACT: **Approve flow lives in multiple places.** Approvals page, incident detail `ActionSections`, overview `ApprovalList` all render action cards; "primary vs secondary Approve" rules are encoded in prose and in props (`ackIsPrimary`, `IncidentDetailPage.tsx:97-100`), not in one component contract.
11. FACT: **Motion contract is narrower than the spec** (see s3.3): DESIGN.md "Motion is 120 to 180 ms" (`DESIGN.md:29`) is lint/test-enforced via tokens; doc 15 s2 proposes 180-500 ms.
12. FACT: **Timeline is the only history primitive**, keyed `${at}-${index}` (`Timeline.tsx:~38`): not stable under insertion, not virtualised, summary-only (`kind`+`summary` strings, schema `TimelineEntryDto {at,kind,summary}`), phrase table is client-side (`design/timeline.ts`) so each new backend event kind needs a client release.
13. FACT: **DESIGN.md vs reality is mostly consistent** (first-download set matches `routes.tsx`; size numbers are produced by the script), but it claims "Every page follows the host selector or renders AllHostsNote" and `Digest/Hosts/Settings` do; `Welcome`, `NotFound`, `Setup` do not and need no note - fine, but not stated.
14. INFERENCE: **Event semantic debt.** `UiEvent.kind` is a free string ("incident"/"action"), `event` is another free string; the client matches only `kind`. Adding operation/host/policy events with this shape produces more string-matching.

---

## 3. Spec critique (docs 12-17)

### 3.1 Internal contradictions and tensions
1. **"Detailed first" vs bundle/perf/QA budgets.** 12 s3 + 16 s4 make the full trace the reference design, while 16 s9 demands virtualisation, off-main-thread layout and a "bundle budget", and 16 s10 requires every page be tested in 3 profiles x 2 themes x 4 viewports x 8 data states. Nothing says which wins on a phone, and no per-page cost ceiling is given. INFERENCE: matrix = 3x2x4x8 = 192 cases per page before graph states.
2. **Presentation profiles vs priority classes.** 12 s5 defines P0-P3 as properties of content; s3 profiles (Detailed/Standard/Essential) reshuffle visibility. 14 never says how P-levels map to profiles (is P2 hidden in Essential? P3 in Standard?). 16 s4 says "P0 and P1 expanded by default where space permits" for Detailed only. Mapping must be a table or profiles are untestable.
3. **"Not an expert mode" vs "Essential" profile.** 12 s3: detailed is not a hidden expert mode; 12 s3 Essential: "technical detail remains accessible"; 16 s4 Essential: "never deleted or inaccessible". Acceptable but no rule for the default profile for a new user (Detailed is "reference", yet 12 s7 "Do not require customization before it becomes useful"). Default profile undefined (open question).
4. **Shell: Operator panel vs resizable inspector vs Ctrl+K.** 13 s2 makes Operator launcher P0 with a persistent non-modal resizable side panel; 14 s5 also needs a contextual node inspector (resizable, pinnable, 15 s4); 14 s8 Operator workspace also has an inspector. On 1280 px with a 240 px sidebar, panel + page + inspector leaves ~600 px. Doc says "reserve enough space" but gives no layout budget or precedence rule (what closes when both want the right side?).
5. **Escape semantics.** 13 s2: Escape closes the topmost overlay/panel. 15 s4/6: inspector, side panel, popover, dialog and graph selection all want Escape. Not ordered; also 15 s6 "never cancel a running operation" is a non-sequitur for Escape.
6. **Search vs palette vs Operator.** 13 s7 "do not imply a full search index exists"; 15 s11 "Global search and Operator are separate launchers"; 13 s2 `Ctrl/Cmd+J` for Operator. DESIGN.md already uses `Ctrl K` for the sidebar button. But 13 s2 also says "Commands clearly separated from search results" while 15 s11 "search selection must not run a high-impact operation" - OK; and today the palette contains "Actions" (logout, theme). Not contradictory, but the palette's current group model (`CommandPalette groups: {title, items}`) needs an object-search source type with async results; kit has no async-result contract.
7. **Tabs rules.** 12 s6 "Tabs must not hide P0"; 14 s9 Host detail has 7 tabs where Overview is P0 but "Capabilities", "Monitoring" (checks, health) and "Activity" (incidents/ops) are also P0 content for different questions. 13 s6 "avoid very deep navigation" with 13 s5 map 3 levels (Area > Object > Tab). No depth rule.
8. **Operation detail P0 overload.** 14 s5 labels graph (Dominant), current state, exact approval request as P0 and findings/plan as P1 "large or medium", plus an inspector. Dominant + two large + contextual cannot all be visible at 1280x720. 14 s2 "Largest element: orchestration graph (Dominant)" but 14 s5 also demands "Operation conversation" as P1. No wireframe-level allocation, yet 14 s1 says space "must be validated at realistic viewport sizes".
9. **Incident vs Operation ownership of actions.** 14 s6 says keep incident controls (ack/mute/dismiss) separate from operation controls (stop/cancel/retry); 14 s5 says "If approval required, present the exact request at the point of need" on Operation detail; today approvals are incident-bound (`actions.incident_id`). Where does a user-requested operation's approval appear (Operations only? Approvals page? Attention centre)? 13 s2 adds an attention centre; 14 s3 has "Waiting for user" on Overview. Three surfaces for approvals with no canonical owner (violates 13 s6 "one canonical detail page").
10. **Overview identity.** 14 s3 Overview P0 list is 4 large/medium widgets + P1 customisable widgets; 12 s7 "dashboard widgets add/remove/rearrange/resize"; 17 s1 acceptance "no counter duplicated across tile, heading, filter" and 14 s3 "must not become a wall of charts". Customisable layouts and "no duplication" are individually fine but user layouts make duplication unenforceable.
11. **Live-follow and focus.** 15 s4 "Live-follow may scroll/pan only when explicitly enabled or when the user is already following" vs "default framing should keep the current active path visible". "Already following" is not defined (what counts as following: last interaction? viewport contains active node?).
12. **"Permission-filtered" search / "permission denied" states** (12 s9, 14 s17.7, 15 s10-11, 16 s10) vs current product: single-user, no roles (`SessionDto` has a user, no role); 05/09 docs leave multi-user open. Dead requirement for v1 or a hidden RBAC scope.
13. **Policy simulator "same deterministic evaluator as real dispatch"** (14 s12, 15 s7) vs 14 s12 "Policy version at the time". Needs historical policy snapshots and full context snapshots stored at decision time; none of that is specified as a data requirement in docs 12-17 (cross-doc dependency not listed in 17).
14. **12 s9 "unavailable, stale, offline, partial" vs 16 s10 "stale, offline, partial, permission-denied"** vs 15 s10: three similar but non-identical state lists. No canonical state taxonomy (name -> trigger -> visual -> role-liveness) so each page would invent its own.
15. **Motion: DESIGN.md vs 15.** DESIGN.md `:29` hard-limits motion to 120-180 ms and the guardrail tests assume two tokens; 15 s2 introduces 100-140, 140-180, 160-220, 180-240, 250-500 ms. 15 says "preserve that calm baseline" while changing it.
16. **16 s2 "Current design system is a base, not a prison"** vs `DESIGN.md` guardrails that are explicitly non-negotiable (no inline style, 250-line file cap, kit barrel only, tokens only). A canvas graph needs inline-computed transforms/SVG attributes (lint bans inline `style`; "computed values need a disable comment", `DESIGN.md:370`). The spec doesn't acknowledge that these rules will need amending for the graph (and 250-line cap for a graph engine).
17. **13 s1 area list vs 13 s5 map:** Policies & Autonomy, Integrations, Knowledge each contain 3-5 sub-pages; "Experience" (06 doc) is listed as an open question (13 s9) but 17 s6 journey 6 requires playback and "cycle comparison" that depend on it.

### 3.2 Vague or unverifiable requirements
- "Fast, responsive, modern and pleasant" (12 s1), "polished" (12/16/17), "alive" (15 s1), "feels dynamic" (12 s9): no metric. FACT: DESIGN.md has measurable LCP/CLS/TBT numbers (`DESIGN.md:397-399`); the new docs give none.
- "Enough screen area for every dominant item" (12 s10 q4): no minimum width/height for the graph; 16 s5 "minimum practical viewport" not defined. RECOMMENDATION: set a number (e.g. graph canvas >= 640x400 CSS px at >= 1024 px viewport; below that switch to list+sheet).
- "Meaningful in-progress summaries" (14 s3), "true progress" (14 s3): the engine has no progress model (see 3.4).
- "Realistic fleet sizes" (16 s9, 16 s10): not specified (hosts, operations, nodes per operation, log lines). RECOMMENDATION: fix test fixtures (e.g. 50 hosts, 5 000 operations, 2 000 nodes/op, 100 k log lines).
- "Large and busy datasets are tested; no clipping or excessive re-rendering" (17 s9): no render-count or frame-time threshold.
- "Keyboard-accessible node navigation model" (16 s8): no spec (arrow keys across branches? roving tabindex over a DAG?). Hard to specify for a DAG, easy to get wrong; needs a defined traversal order and a tested contract.
- "Manual screen-reader review for complex interactions" (16 s8): not reproducible; no named SR/browser pair.
- "Browser-level tests cover at least one complete workflow through the page" (17 s9): repo has zero such tests and no browser test infra in CI (s1.9). Per-page that is a new test stack (Playwright + a deterministic hub/fixture) not mentioned as a prerequisite.
- "Historical operation state is stable and reproducible" (17 s6): requires immutable events and deterministic layout; 16 s9 asks for deterministic layout; no statement on layout-version stability across releases.
- "Cached layout" for historical traces (16 s9): cache location (server? IndexedDB?) not stated.
- "Search results scoped and permission-filtered" (15 s11, 17 s8): see 3.1.12.
- Motion quality gate (15 s12) lists 7 attributes per animation: fine as process but unverifiable in CI unless animation records are code-adjacent (see 3.3).

### 3.3 Impractical / risky: motion and animation
1. FACT: ranges "250-500 ms linear active-edge reveal", "180-260 status settle", "160-220 node activation" (15 s2) only make sense with a motion engine; repo has CSS keyframes only. INFERENCE: node/edge animation on an SVG/canvas graph with hundreds of nodes needs a layout+render library (see 3.5) and a JS animation path that respects reduced-motion, which today is implemented by zeroing CSS tokens (`tokens.css:398-401`) and cannot reach JS timers.
2. Live-event batching vs per-event animation (16 s9 "batch live event processing" vs 15 s4 "new node appears with a 160-220 ms transition"). Under a burst (parallel capability calls) batching and per-node motion conflict. RECOMMENDATION: cap concurrently animating nodes (e.g. <= 5) and coalesce others to a static insert; spec it.
3. "Failure gets a brief emphasis once" (15 s4) and "avoid flashing" (15 s3) with a once-only pulse is fine, but "once" needs memory across re-mounts/reconnects/historical view: otherwise every reload replays emphasis. Needs a seen-set keyed by node id + state version.
4. "Graph playback" (15 s2 last row, 14 s5 "historical playback") is a feature, not a token: needs an event-time axis, scrub, speed, and deterministic replay; effectively a second visualisation mode. Cost not acknowledged; classify as later milestone.
5. "Page content transition 140-200 ms" (15 s2) on route change conflicts with lazy-loaded pages already showing a `PageSkeleton` inside `Suspense` (`Layout.tsx:99`) and with the stated "avoid transitions that delay navigation". RECOMMENDATION: drop page transitions; keep skeleton-in-place.
6. "Side panel 180-240 ms" slide: sliding a panel over/beside a page forces reflow of main content if it is "beside" (13 s3 "non-modal resizable"). Layout shift contradicts 16 s9 "avoid layout shift". Choose overlay-with-offset-padding or animate `transform` only.
7. "Do not animate in ways that move keyboard focus" (15 s3) - no mechanism to test. RECOMMENDATION: a test helper that asserts `document.activeElement` is stable across a simulated event burst.
8. "Reduced motion replaces with static state, arrows" (15 s3): requires static alternative designs for each animated element; counts toward the 192-case matrix.
9. INFERENCE: motion tokens as spec'd would break the existing `cssGuard`/token tests only if they enforce two values; check `styles/cssGuard.ts` before adding. Needs new tokens (`--motion-panel`, `--motion-graph-node`, `--motion-edge`) in `tokens.css` and a JS reader (`getComputedStyle`) that returns 0 under reduced motion.

### 3.4 IA vs a plausible backend: data/endpoints/events that do not exist today
FACT sources: `openapi.json` paths list (s1.4), `UiEvent`, `08-current-state-gap-analysis.md`, `B`-series inventory. "Missing" below = no endpoint or schema in the OpenAPI document.

**Cross-cutting**
- Operations as a first-class record: list, filters (status/origin/host/type/date/initiator/capability/outcome), detail, saved views; none exists. INFERENCE: `actions`/`incidents` are the nearest. Required endpoints: `GET /operations`, `GET /operations/{id}`, `POST /operations` (user-initiated), `POST /operations/{id}/cancel|pause|retry`.
- Orchestration event stream: durable ordered events per operation with node identity, parent/branch/cycle ids, owner component, phase, correlation ids, started/ended, input/output refs, capability id/version, policy result, LLM provider/model/tokens/latency/cost. Missing entirely. Needed: `GET /operations/{id}/graph` (snapshot with `version`/`seq`), `GET /operations/{id}/events?after=seq` (replay), SSE `operation` events with monotonically increasing `seq` per operation and `Last-Event-ID` support, event id uniqueness for duplicate suppression (15/16 require "duplicate event delivery" handling).
- SSE v2: today no `id:`, no replay, lossy broadcast (`events.rs:76-80`), host events absent, payload lacks status/node detail. Required: event ids, resume, per-topic subscriptions (operation:{id}, host:{id}, attention), host connect/disconnect, check-state, capability deploy progress, policy changes, conversation messages.
- Attention / notifications centre (13 s2, 14 s16): durable notifications table, unread state, snooze/quiet hours, preferences; only Discord notifier settings exist. Needs `GET /attention` (approvals + questions + manual interventions + failures), `POST /attention/{id}/read`, preference endpoints.
- Global search (13 s7, 15 s11): none besides `GET /notes/search` and incident `q`. Needs `GET /search?q=&types=` returning typed, scoped, status-bearing hits across hosts, incidents, operations, capabilities, knowledge, policies, integrations, settings.
- Per-user preferences (presentation profile, dashboard layout, saved views, columns, pinned items): no user-prefs endpoint (only localStorage). Needs `GET/PUT /me/preferences` with schema versioning, plus saved views/filters.
- Audit log (14 s15): `audit_log` table exists (C-persistence) but no API; no actor/outcome/policy-transition schema. Needs `GET /audit` with filters + integrity verify.
- Pagination/sort/filter contract for all lists: only incidents has cursors.

**Per page**
- Control Room (14 s3): active operations summary (needs Operations), environment health (storage pressure, services with issues - no service/workload model; `OverviewDto` has 6 counters + incident_groups), LLM/provider status and usage (tokens/latency/cost: not recorded or exposed), suggestions/maintenance reminders (ADR 0016 suggestions unimplemented), fleet resource trends (no metrics time series API; `/metrics` is Prometheus text for operators), pinned items, widget layout persistence.
- Operations list/detail (14 s4-5): everything in the "Orchestration event stream" bullet; findings/hypotheses with uncertainty and discarded hypotheses (today `ReportDto {cause, evidence, proposals, verdict, ...}` is a single report); plan with preconditions/dependencies/rollback/expiry (today `ActionDto` has `risk`, `args`, `reason`, `expires_at`); cycles/replans (no cycle model; 04 doc plans it); model usage per call; compare-cycles endpoint; cancel/pause/retry with "what stopping means".
- Incidents (14 s6): `related operations`, recurrence count (exists: `occurrences`), `assigned` (no assignment concept), "historical similar cases" (engine has similar-incident search internally; not exposed), linked knowledge. `IncidentSummary` has no `needs_action`/`related_operation` field.
- Operator panel/workspace (13 s3, 14 s7-8, 15 s6): durable conversations (list/create/messages/pagination), operation linking, per-conversation scope, message streaming or event push, stop-generation vs cancel-operation, action cards inside messages (structured), attachments later. Today `POST /chat` returns one string and the client owns history.
- Hosts (14 s9): host role/criticality, tags/groups, capability coverage, resource summary - `HostDto` has `kind`, `version`, `outdated`, `open_incidents` only; "Workloads" (services/containers/processes/dependencies): containers only via `/hosts/{name}/docker`; "Network & security": ports/SSH logins partly via profile/ssh tab; topology and dependencies: no entities/relationships API; "Activity" per host: no feed.
- Capability Center (14 s10): catalog (sources, signatures, trust, versions, compatibility), installed per agent, deployments (per-target progress, partial success, rollback), capability detail (typed input/output schemas, effects, timeouts, verification, permissions), install flow with preflight and rollout strategy, enable vs authorize. Today only `/hosts/{name}/skills` (list, enable/disable) with manifest metadata. Entire package-distribution/signing channel absent.
- Knowledge (14 s11): entities, facts with provenance/freshness/confidence/state, relationships, edit/correct/delete, suggestions accept/edit/reject/defer, graph traversal API with bounded depth/filters; notes exist but no entity links/revisions/tags UI beyond basics.
- Policies & Autonomy (14 s12): policy CRUD, effective-policy resolver by host x capability, inheritance and precedence explanation, versions/history, simulator endpoint reusing the evaluator, historical context snapshots per decision, decision history. None exist; autonomy itself is "missing" per 08.
- Integrations (14 s13): provider registry, connections, health history, scopes, credential set/replace/test, capability mapping. Only `POST /settings/test/{discord,heartbeat,llm}` and `/settings/models` exist.
- System (14 s14): backups/restore, diagnostics, retention - `/settings` exposes config; no backup UI endpoint; `/system/update` and `/system/logs` exist.
- Logs (14 s15): host logs + hub logs exist (`/hosts/{name}/logs`, `/system/logs`); no operation/incident correlation, no structured-field filters, no live push (polling).

### 3.5 Library/tooling implications (INFERENCE, to be validated in a spike)
- Graph rendering: React Flow (xyflow) ~ 45-60 kB gzip + a layout step (dagre ~ 25-30 kB gz or elkjs ~ 100-150 kB gz in a worker). Both can sit in a lazy chunk < 120 kB individually under `size.mjs` rules, but layout in a Web Worker needs `worker-src` (falls back to `default-src 'self'`, so a same-origin worker file is OK; `blob:` workers are NOT allowed by current CSP `csp.rs:10`). Worker file must be emitted as a hashed asset by Vite.
- Hand-rolled SVG + dagre-style layout is feasible for <= ~300 nodes and avoids a dependency, at the cost of pan/zoom/minimap/a11y work. Decision belongs to a spike; budget is the deciding criterion.
- Virtualisation: no library today (TanStack Virtual ~ 5 kB gz would fit). Needed for logs, event streams, operations list.
- Charts/sparklines: none; a tiny SVG sparkline component is cheaper than a chart lib.
- Split-pane / resizable panel: implement in-house (pointer events + keyboard) to keep dependency count down; kit must add a11y semantics (`role=separator`).

---

## 4. Reuse map

### 4.1 Reusable as is (FACT-backed)
- Tokens + semantic tones + `tone.ts` vocabulary approach (16 s1 keeps it). Add new vocabularies in `src/design/` per recipe `DESIGN.md:421-427` (operation status, node status incl. "uncertain", "planned", "retrying", "monitoring"; policy outcome allow/ask/deny/unavailable/cannot-determine; capability install/enable/authorize; knowledge state confirmed/discovered/inferred/preference/constraint/observation).
- Feedback: `useActionFeedback`, toasts, `InlineMessage`, `Banner`, `LoadError`, `ConfirmDialog` (names object + consequence) - all directly usable for operation controls and policy apply.
- Layout/primitives: Button/IconButton/LinkButton, Tag/StatusPill/Count, Tooltip/InfoTip, Card/Section/KeyValueList, FilterChips, Tabs (URL-controlled), Menu/OptionPicker/Popover, Dialog, Field/forms, Switch/SegmentedControl, Stepper (as base for install wizard), Skeleton/Progress/Spinner/EmptyState, Markdown, CopyBlock, RelativeTime/Countdown, Kbd.
- DataTable for Operations list, Audit, Deployments, Capability inventory, Knowledge table (sortable/server-sorted, stacked on phones, `rowHref`). Needs virtualisation and column picker (15 s9) added.
- CommandPalette: reuse as the command layer; add an async "search results" group source.
- `HostProvider/useSelectedHost` for the environment context (13 s2); extend to carry a typed scope (all / host / incident / operation) used by the Operator.
- `api` client, `call()`, `ApiFailure`, `keys` centralisation, `QueryBoundary` pattern, `useLeaveGuard`.
- Guardrail machinery (eslint rules, axe-in-tests, contrast test, size script, `/dev/design`, `screens.mjs`) as the harness for new surfaces.

### 4.2 Needs redesign / extension
- **Live layer** (`api/events.ts`): replace "invalidate by prefix" with a typed event bus keyed by topic, sequence/Last-Event-ID resume, batching, and per-query cache patching (setQueryData) for operation nodes. Keep invalidation as the fallback.
- **QueryBoundary**: add stale/refreshing/refresh-error and "partial" (multiple sources) states and a freshness label; currently refresh errors are dropped (s2.4).
- **Shell** (`Layout.tsx`): add right-side slot(s) (Operator panel, inspector), a global activity indicator, attention-centre entry, Operator launcher; resolve precedence of right-side surfaces.
- **Timeline**: index-keyed, summary-string-only, client phrase map; becomes a technical trace view over structured events (virtualised, filterable, stable ids). Keep as the compact "Activity" view.
- **ChatThread/ChatComposer**: tied to one-shot POST with `{role,text}` turns; needs structured parts (text, action card, evidence, tool call), streaming/push, scroll-lock "new activity" affordance, durable history, context chip.
- **Overview/OverviewTiles/StatTile**: tile+two-list design is the exact shape 14 s3 says to move beyond; widget host + per-user layout store needed. StatTile and attention/approval rows reusable inside widgets.
- **Action cards (`features/approvals/*`)**: today incident-bound; need a generic "proposed step" card that works inside conversations, operations and attention centre (one component, one primary-action rule).
- **HostDetailPage tabs**: current tabs (Overview, Profile, Checks, Logs, SSH) map to future Overview, Monitoring, Network & security, Activity; Docker panel maps to Workloads; Skills maps to Capabilities. Reuse tab bodies, redo grouping.
- **Settings framework** (`FieldSpec` renderers, save bar, source badge): reusable for System and for policy/integration config forms; "UI preferences" section is new.
- **Search/URL state**: filters in URL exist for incidents/logs; generalise into a `useUrlFilters` hook + saved views to meet 15 s9.
- **Motion tokens**: extend `tokens.css` (+ JS accessor), per 3.3.9.

### 4.3 Missing components for the target IA (new kit work)
1. `GraphCanvas` (pan/zoom/fit/minimap, node/edge renderer, selection, live-follow, collapse groups, keyboard nav) + `GraphNode`/`GraphEdge` + non-canvas `GraphOutline` (tree/list alternative, 16 s8).
2. `Inspector` (right drawer, pin/resize/close, focus management) and `SplitPane` (keyboard-accessible splitter).
3. `SidePanel` host (Operator) with resize, pin, shortcut, mobile sheet.
4. `VirtualList`/virtualised `DataTable` mode; `LogStream` (live, pause, new-activity pill).
5. Operator parts: `ConversationList`, `Message` with structured parts, `ActionCard` (generic), `ContextChip/ScopeSelector`.
6. `PolicyMatrix` (host x capability grid with inheritance), `RuleEditor`, `EvaluationTrace` (matched rules), `DiffView` (policy/cycle compare), `SimulatorScenarioPicker`.
7. `EntityGraph` (reuses GraphCanvas with different node grammar), `FactRow` (provenance/freshness/confidence).
8. `InstallWizard` shell (multi-target progress table, per-target retry).
9. `Sparkline`/mini-trend, `FreshnessLabel`, `LiveBadge` (per-section freshness), `AttentionCenter` list.
10. `WidgetGrid` (add/remove/resize/persist) for Control Room.
11. `ProfileProvider` + `PresentationBoundary` (reads presentation profile, exposes `isVisible(category)`), `ColumnPicker`.
12. `NotificationBell`/activity indicator, `Toast` is not a durable record (13 s2).
13. Test infra: fixtures for large graphs/lists, Playwright harness, graph-state matrix tests.

---

## 5. Is "detailed view first, simpler profiles derived later" sound?

- RECOMMENDATION: **Sound in principle, with three changes.**
  1. **Define the profile mechanism before building the first page** (not after): a tiny `PresentationProfile` context and a rule table mapping information category (not P-level) -> visibility per profile, plus a `<Disclosure category>` / `useVisibility(category)` primitive. Rationale: retrofitting visibility into hundreds of bespoke JSX branches later is the usual failure; the cost now is ~1 hook and ~1 component.
  2. **Derive profiles by hiding/collapsing, never by different layouts.** 16 s4 already says this ("view preferences over the same information"). Enforce with a test: render each page in all 3 profiles with the same fixture and assert that every P0 element + every authority/audit element is present (maybe collapsed) in each - this is what 17 s9 asks for, and is automatable.
  3. **Do not make "Detailed" the first-run default for everyone.** FACT: DESIGN.md principles are "crisp, quiet, dense" and 10-second comprehension (`DESIGN.md:304-305`). INFERENCE: a new user opening Detailed Control Room sees raw phases and token counts; recommend default = Standard, Detailed one click away and remembered. This resolves tension 3.1.3 and is consistent with 12 s7 "defaults must be coherent".
- Risk (INFERENCE): "detailed first" encourages building the graph/inspector before the data exists; the profile model is cheap but the detailed data model (events, nodes, cycles) is the expensive and risky piece. The UI must be built against a **frozen event/trace schema** (a contract with fixtures), not against the live engine.
- Existing assets that already follow this: `Timeline` "Show all", `CopyBlock`, `InfoTip`, host detail tabs, settings "advanced" - good seeds.

---

## 6. Suggested order of frontend work and backend prerequisites

Principle: ship vertical slices that each leave main working (design pack README rule). Frontend can mock via MSW-like fixtures keyed to OpenAPI, so UI work does not strictly wait, but each slice needs a contract.

| # | Frontend slice | Backend prerequisite | Notes |
|---|---|---|---|
| 0 | Decisions + spikes: graph library/layout, worker/CSP, profile mapping, motion tokens, route/IA naming; amend `DESIGN.md` (motion, graph exceptions to lint) | none | Spike must measure gz size vs 120 kB chunk rule |
| 1 | Foundation: typed live layer v2 (topics, seq, batching, freshness), `QueryBoundary` stale/partial states, `FreshnessLabel`, profile context, motion tokens, VirtualList, SplitPane/Inspector/SidePanel shells, Playwright harness | SSE v2 contract (event ids, resume) can land after; keep invalidation fallback | Pure frontend; unblocks all pages; no IA change visible yet |
| 2 | Shell + IA reshuffle behind existing pages: new nav groups, operator launcher placeholder, attention entry, scope chip | none (placeholders) | Ship under existing routes |
| 3 | Operations list + detail **timeline/list view first** (graph later) on a frozen trace schema; incidents <-> operations links; generic ActionCard | Operations record + `GET /operations`, `/operations/{id}/events` with `seq`; incident->operation link field; operation SSE topic | Smallest slice that proves the event contract |
| 4 | Orchestration graph (read-only, live-follow, inspector, outline, collapse, cycles) | Node/branch/cycle ids and phase semantics in events; stable layout inputs | Biggest UX/perf risk; gate on spike |
| 5 | Operator side panel + workspace (durable conversations, context, action cards, stop vs cancel) | Conversations API + message events + operation creation; chat routed through orchestrator | Do after 3 so panel can show operations |
| 6 | Control Room v2 (active ops, waiting-for-user, health, recent activity) with fixed default layout; widget customisation later | Operations summary, attention feed, provider usage, fleet health summary | Reuse tiles/rows |
| 7 | Hosts/Infrastructure re-grouping (tabs per 14 s9) incl. Activity feed | Host activity feed, workloads/services endpoint, tags/role | Reuse existing tab bodies |
| 8 | Attention centre + notifications preferences | Durable notifications + prefs | Needed before autonomy ships approvals outside incidents |
| 9 | Policies & Autonomy: effective-policy matrix + decision history first; editor; simulator last | Policy model, evaluator, decision records with context + policy version snapshots | Must follow backend autonomy design; do not mock the evaluator in UI |
| 10 | Capability Center: Installed (read) -> Detail -> Discover -> Install wizard/Deployments | Capability registry, packaging/signing, per-agent deployment state | Largest backend dependency; last |
| 11 | Knowledge browser + entity detail + suggestions; graph explorer later (reuse GraphCanvas) | Entities/facts/relationships/provenance API; suggestions (ADR 0016) | Notes stay as-is |
| 12 | Integrations; System extras (audit, backups, diagnostics); global search; dashboard widgets & saved views | Provider registry; audit API; search index; prefs API | Search must not ship before permission/freshness story |
| 13 | Hardening: profile matrix tests, large-data perf, a11y manual review, replay/playback | Historical snapshots, layout versioning | Playback last |

RECOMMENDATION: slices 1-3 can run in parallel with backend "operation record + event schema" work; 4+ must not start until the trace schema is frozen with golden fixtures that both sides test against.

---

## 7. Open questions

1. Event/trace schema ownership and shape: node ids, parent/branch/cycle/phase fields, planned-vs-happened marker, uncertain-outcome state, versioning policy. Who freezes it, and are golden fixtures shared with the Rust side?
2. Transport: stay on SSE (EventSource cannot set headers; no binary) vs WebSocket for conversations (agent link already WS). Per-topic subscription or one multiplexed stream? Resume via `Last-Event-ID` or snapshot-then-delta?
3. Default presentation profile for new users and per-category mapping table (see 5.1). Are profiles stored server-side per user (needs prefs API) or browser-local?
4. Graph tech: library vs in-house, layout engine, worker usage under current CSP (`worker-src` unset -> `default-src 'self'`; `blob:` not allowed). Acceptable gz budget for the graph chunk (current limit 120 kB; initial JS headroom is only 15 kB).
5. Is the 250-line file cap and "no inline style" rule relaxed for the graph (transform/viewBox math)? Which amendments to `DESIGN.md` and the ESLint rules are authorised?
6. Where does an approval live for operations without an incident (Operations only, Approvals page, Attention centre)? One canonical home needed (13 s6).
7. Approvals page vs Attention centre vs Control Room "Waiting for user": merge or keep three views of the same query?
8. Multi-user/roles: do "permission denied" states and permission-filtered search exist in v1? Today `SessionDto` has no role.
9. Operator panel layout precedence at 1280 px (panel vs inspector vs sidebar) and its default width; does the panel push content or overlay?
10. Motion: adopt the 15 s2 values (needs DESIGN.md/tokens/test changes) or keep 120-180 ms and apply only to the graph? Is playback in scope for the first release?
11. Mobile scope: is operation detail/graph expected to be fully usable on a phone, or list+sheet only (16 s5 suggests the latter)? Which actions (approve/deny/stop) must work on a phone?
12. Search: is the first release a client-side union (pages + hosts + incidents + operations already in cache) or a server index? Freshness/permission semantics must be decided before UI promises it (13 s7).
13. Test strategy: which browser-level workflows are required, who owns the deterministic hub fixture (extend `cargo run -p domovoi-hub --example dev_hub`?), and does CI run them?
14. Fleet-size targets for virtualisation and performance tests (hosts, operations, nodes/operation, events/second).
15. Naming migration: "Skills" -> "Capabilities", "Known conditions" -> ? (policy/knowledge?), "Chat" -> "Operator", "Digest" fate; URL redirects for existing links and Discord deep links.
16. Does the lint rule "`<verdict literal>` only via `verdictMeta`" extend to new vocabularies, and who owns keeping `design/*Vocab.ts` in step with Rust enums (no codegen today; unknown values render neutral)?
