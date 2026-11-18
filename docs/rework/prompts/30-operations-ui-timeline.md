# 30 — Operations UI: shell right slot, Operations list and detail, step timeline, inspector, approval card

| | |
|---|---|
| Branch | `operations-ui-timeline` |
| Milestone | U1 |
| Depends on (must be merged) | 27, 28 and 29, and through them 26 and 59 (and everything 26 needs: 22–25). Prompts 24–29 and 59 are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"**. If 27, 28 or 29 is not merged, stop and report: this prompt cannot be built against guessed routes. |
| Can run in parallel with | 31, 34, 40, 41, 58 (order-flexible, never concurrent). None of them edits `web/src/app`, `web/src/components` or `web/DESIGN.md`. 36, 37, 38, 43, 49 and 51 **follow** this prompt and consume what it produces |
| Produces (shared contracts) | (1) The **right-slot manager** `web/src/app/rightSlot/`: `RightSlotProvider`, `RightSlotHost`, `RightSlotSurface`, `useRightSlot`, `slotLayout`, the kit chrome `SidePanel`, tokens `--right-slot-width`, `--right-slot-content-min`, `--motion-panel`. (2) The **structured approval card** `ApprovalCard` (`web/src/features/approvals/ApprovalCard.tsx`) and its model `ApprovalPlan`. (3) Routes `/operations` and `/operations/:id` (`?step=<id>` selects a step). (4) Web hooks in `web/src/api/hooks/operations.ts` and `operationControls.ts`. (5) Design vocabularies for operation status, step outcome, step kind, edge kind, execution state, pause and wait words. (6) The browser smoke test `npm run smoke` (`web/scripts/smoke.mjs`) and its CI step |
| Consumes | 26: the frozen trace schema, the five golden fixtures in `web/src/test/fixtures/operation-trace/*.json`, `web/src/api/operationTrace.ts` (27 re-points it at the generated types). 27: `GET /api/v1/operations`, `GET /api/v1/operations/{id}`, `GET .../steps/{step}`, `POST .../steps/{step}/approve` and `.../deny`, the DTOs `OperationPage`, `OperationSummaryDto`, `OperationDetailDto`, `ExecutionDto`, `StepDetailDto`, `ApproveStepRequest`, `ActionDto.legacy`, `IncidentDetail.operations`. 28: `useJournalStream` / `useLiveEvents`, `applyJournal`, query keys `keys.operations` and `keys.operation(id)`. 29: `POST /api/v1/operations/{id}/cancel`, `/pause`, `/resume`, `POST .../steps/{step}/answer`, `OperationControlDto`, `BudgetExtensionDto`. 18 (merged): `ActionCard`, `ApproveControl`, `PlanTag` |
| Launch gate | none |
| Suggested model | medium: many small, well-specified files; the judgement is in keeping every web rule and the approval path safe |

## 1. Objective

Give the person who runs the servers one place to watch and steer what Domovoi is doing, built only on the
read and command routes the earlier prompts publish:

- **The shell gets one right-hand slot** shared by every side surface. This prompt builds the manager and the
  first surface that uses it (the step **inspector**); the Operator panel of a later prompt registers with it.
  Wide screens show the slot beside the page when the page keeps at least 640 px; otherwise the most recently
  opened surface wins and the other collapses to a tab; on phones (and when even one surface does not fit) the
  slot is a modal sheet. It never covers content that can still be used.
- **Operations list** (`/operations`): operations and, labelled **Legacy**, old actions, newest first,
  filtered by status and by the host selector, with loading, empty, error and "load more" states.
- **Operation detail** (`/operations/:id`): header with goal, status and the controls (pause, cancel),
  **one banner** for the state that needs attention (outcome unknown, budget spent, paused, waiting for
  approval, waiting for an answer, waiting for the host), a "Waiting for you" section holding the approval
  cards and question forms, the **step timeline** (default) and **step tree** (same data, grouped by parent),
  details and budgets. The graph is a later prompt: nothing here draws one.
- **Step inspector** in the right slot: status, times, connections to other steps, the step's stored intent and
  its bounded, redacted output.
- **One structured approval card**, `ApprovalCard`, used by operation steps and by the existing legacy actions.
  For a step it shows contract, host, target, arguments, risk, expiry, the plan-hash prefix labelled
  "v2 binding", and the model's reason as secondary, labelled text. Approving sends the **plan hash the card
  showed**, and a destructive step (or one of unknown risk) needs the skill name typed.
- **Live**: the page follows the shell's journal stream; a frame newer than the page's `as_of_seq` refreshes it.
- **A browser smoke test** (Playwright, already a dev dependency) runs the built bundle against a mock hub in CI.

With orchestration off (still the default until prompt 58) nothing breaks: the list is empty with an
explanation and the Approvals page is unchanged. This prompt changes no Rust and no wire contract.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**RW-21 / D-08 (timeline first).** The timeline and the later graph render from one frozen trace
(`schema_version: 1`, five golden fixtures shared by Rust tests and `web/src/test/fixtures`). The trace carries
`operation`, `steps[{id, parent, kind, phase, cycle, outcome, started_at, ended_at, summary, refs}]` and
`edges[{from, to, kind}]`. `parent` is grouping (tree), `edges` are causal links (graph, and the inspector's
"connections"). The trace never holds output, intent or plan hashes; those come from the step and execution
DTOs. Unknown kinds, outcomes and edge kinds must render generically (neutral pill with the raw value): a newer
hub never breaks the page. This prompt builds the timeline and tree; the graph is prompt 38.

**RW-22 (one approval card).** Every approval surface renders, from deterministic code, contract, target,
arguments, effect class, reversibility, expiry, plan-hash prefix and the policy decision. Model text is
secondary and labelled. One shared component serves operation detail (its canonical home), incidents, the
Control Room and the Operator. Today the hub supplies only part of that list for a step (`ExecutionDto`:
contract, host, check, target, args, state, plan hash and binding, risk, `confirm_required`, expiry, approval,
reason). The card renders those now and takes the rest (effect class and reversibility, policy decision, an
agent refusal) as **optional extras** that later prompts fill; with no extras nothing is invented or shown.

**RW-11 stage M3 (approval binding).** A step is approved with
`POST /api/v1/operations/{id}/steps/{step}/approve {plan_hash, confirm_text?}`. `plan_hash` is the 64-hex v2
hash the person saw; the hub refuses another (409). `confirm_text` is the skill's name, required when the
execution says `confirm_required` (destructive, or a skill that is no longer on the host: unknown risk fails
closed). The legacy action endpoint stays for legacy actions, whose hash is the older v1 binding labelled
"v1 binding" (RW-11; `PlanTag` already does this). The UI **never approves for a person**: no automatic
confirm, no default-focused Approve in a dialog, no shortcut.

**RW-19 / prompt 29 (use-case layer).** The browser only calls the hub's HTTP routes. Cancel, pause, resume and
answer are human-only routes with no step-up; each answers `OperationControlDto { as_of_seq, operation }`.
Resume can raise a spent budget limit but never remove it. A repeat of cancel or pause is a silent success.

**RW-32 (legacy actions).** Old `actions` rows are never converted. With orchestration on the hub marks every
action `legacy: true`; the Operations list shows them as rows labelled **Legacy** that open the incident page
(where their approval card and Approve live). Nothing is synthesised.

**Live updates (prompt 28).** The shell's stream (`useLiveEvents`, topics `incidents,approvals,operations`)
already invalidates `keys.operations` and `keys.operation(id)` through `applyJournal`, skipping a query whose
cached data has `as_of_seq >=` the frame's `seq`. The list and detail responses carry a top-level `as_of_seq`,
so this works with no new stream. A page opens no EventSource of its own.

**D-12 (motion).** Longer motion is allowed for panels and visualisation (planned token `--motion-panel`,
240 ms, `transform` only); reduced motion is mandatory (the token becomes 0 ms; state stays visible without
motion; no motion carries information).

**Decisions made here (do not revisit).**

1. The right slot is **one slot, many surfaces**: surfaces register by id; at most two are visible side by
   side. The Operator panel (later) is `RightSlotSurface id="operator"`; it builds no dock of its own.
2. Open state is owned by the slot manager (in memory, per tab, not persisted). The inspector's *selection* is
   owned by the URL (`?step=<id>`); a surface's `onClose` callback keeps the two in step.
3. One banner per operation, chosen by priority (outcome unknown, then budget, then error, then manual or
   other pause, then waiting for approval, waiting for an answer, waiting for the host, failed, escalated).
4. The approval card's primary decision lives in "Waiting for you" on the detail page. The inspector shows the
   same card **read-only** (no Approve or Deny), so one page never has two Approve buttons.
5. Filtering is the hub's job. The list asks `GET /operations?status=` and never filters loaded rows in the
   browser. The only client-side step is merging the two sources (operations and legacy actions) in time order.
6. The Playwright smoke test uses the `playwright` library already in `web/package.json` (no new dependency),
   the **built** bundle and a **mock hub** in Node. It is not an end-to-end test of the hub.
7. Visibility profiles (P0 to P3) are prompt 37: no `useVisibility` here. Keep P0 content (approval card,
   banners, outcome pills, plan hash prefix) always on screen and never inside a collapsed section, and keep
   technical detail (intent, output, ids, times) in the inspector and the details section, so 37 can classify
   without moving things.

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, or **(introduced by NN — verify)**.

### 3.1 Shell

- `web/src/app/Layout.tsx` **(existing)**: `Layout()` calls `useLiveEvents()` (`:37`), wraps `HostProvider` and
  `PageTitleProvider`, renders `<Sidebar>`, `<AppPalette>`, then `<div className={styles.column}>` holding
  `<TopBar>` and `<main id="main" className={styles.main} tabIndex={-1}>` (`:87`) whose `.content` holds the
  hub banners and `<Suspense><Outlet/></Suspense>` inside `TopBarSlot.Provider`. 110 lines.
- `web/src/app/Layout.module.css` **(existing)**: `.shell` (grid from 768 px: `var(--sidebar-width)` column and
  the rest, `height: 100dvh`), `.column` (`:21`, flex column), `.main` (`:28`; from 768 px `overflow-y: auto;
  scrollbar-gutter: stable`), `.content` (max `--content-max`, padding `--content-pad`).
- `web/src/app/PageSkeleton.tsx` **(existing)**: `ShellSkeleton` (`:34`) renders `.shell > .sidebar +
  .column > .topbar + main`. It must keep the shape of the real frame.
- `web/src/app/TopBar.tsx` **(existing)**: `TopBarSlot` context and `TopBarActions` (a portal into the bar) are
  the pattern to copy for a portal; `Breadcrumb` uses `useTrail()`.
- `web/src/app/PageTitle.tsx` **(existing)**: `defaultDetail(section, rest)` (`:45`) has a `switch` per section
  (`hosts`, `incidents`, `notes`); `usePageName(name)` names a detail page.
- `web/src/app/nav.ts` **(existing)**: `NAV_GROUPS` (`:36`); group "Respond" holds Overview, Incidents,
  Approvals, Chat. `web/src/app/pages.ts` **(existing)**: `PAGES` map and `page()` helper (lazy plus preload).
  `web/src/app/routes.tsx` **(existing)**: `AppRoutes` registers routes under `<Layout/>`; pages other than
  Incidents, Login and Setup come from `PAGES` and are never imported directly.
- `web/src/app/useMediaQuery.ts` **(existing)**: `WIDE_QUERY = "(min-width: 768px)"`, `useMediaQuery(query)`.
- `web/src/app/HostContext.tsx` **(existing)**: `useSelectedHost() -> [name | undefined, set]`;
  `web/src/app/AllHostsNote.tsx` **(existing)**: `<AllHostsNote>` says a page ignores the host selector.
- Tokens: `web/src/styles/tokens.css` **(existing)**: `--motion-fast: 120ms`, `--motion-base: 180ms` (`:175-176`),
  zeroed in the `@media (prefers-reduced-motion: reduce)` block (`:398-404`); `--z-overlay: 30`; `--sidebar-width`.
  `styles/cssGuard.test.ts` forbids raw colours outside this file.

### 3.2 Approvals (merged prompt 18)

- `web/src/features/approvals/ActionCard.tsx` **(existing)**: exports `EXPIRY_WARNING_MS` (`:23`), `suggestedBy`
  (`:28`), `Expiry` (`:40`), `ActionCard` (`:103`). It renders skill, host, risk pill and `InfoTip`, `ActionArgs`,
  the model reason ("Model's reason (not checked): "), who suggested it, `Expiry`, `PlanTag`, an optional incident
  link, a state note (`Progress` while in flight, `InlineMessage` for expired, failed, outcome unknown, not run)
  and, while `status === "proposed"`, `ApproveControl`. Used by `WaitingCard.tsx`, `features/incidents/
  ActionSections.tsx`, `dev/RespondDemos.tsx`; `Expiry` is also used by `WaitingRows.tsx`.
- `ApproveControl.tsx` **(existing, `:31`)**: Approve opens `ConfirmDialog` naming the skill, arguments and host
  and, for `risk === "destructive"`, `requireText = action.skill` sent as `confirm_text`; Approve is disabled with a
  reason once `useRemaining(action.expires_at) === 0`; Deny is one click. Mutations go through `useActionFeedback`.
- `PlanTag.tsx` **(existing, `:27`)**: `<PlanTag planHash binding />` shows `Plan <8 hex>`, a `v2` / `v1 binding` /
  `unknown binding` tag and an `InfoTip` with `PLAN_HASH_HINT_V2` / `V1` / `UNKNOWN`. Reuse it unchanged.
- `ActionArgs.tsx` **(existing)**: `key = value` lines in mono. `api/hooks/actions.ts` **(existing)**: `useApprove`,
  `useDeny` (`useActionChange` invalidates `keys.actions`, `keys.incidentDetails`, `keys.overview`).
- `approvals.test.tsx` **(existing, 714 lines)** is the regression suite for the legacy card. It must pass
  **unchanged**.

### 3.3 Web data layer

- `web/src/api/client.ts`, `types.ts` **(existing)**: `api.GET/POST` typed from `schema.d.ts`; `call<T>(request)`
  throws `ApiFailure`; `defined(obj)` drops empty query fields; `types.ts` re-exports DTO aliases (164 lines).
- `web/src/api/queryClient.ts` **(existing, `:5`)**: `keys`; **28 adds** `operations: ["operations"]` and
  `operation(id): ["operations","detail",id]` **(introduced by 28 — verify)**. `api/hooks/incidents.ts`
  **(existing)** is the model for an infinite list (`useIncidents`) and `useIncident`.
- `web/src/api/events.ts` **(existing)**: `useLiveEvents()` (`Layout` imports it from here; 28 turns it into a
  `useJournalStream` over `incidents,approvals,operations`). `web/src/api/journalFrames.ts`
  **(introduced by 28 — verify)**: `applyJournal(client, frame)` and the `as_of_seq` predicate.
- `web/src/api/operationTrace.ts` and `web/src/test/fixtures/operation-trace/{waiting-approval,verified-success,
  failed-verification-new-cycle,outcome-unknown-paused,question-and-budget-pause}.json`
  **(introduced by 26, re-pointed by 27 — verify)**. Together the files hold every `StepKind`, `StepOutcome` and
  `EdgeKind`.
- `web/src/api/schema.d.ts` and `openapi.json` **(existing, generated)**: after 27 and 29 they hold the operation
  paths and DTOs. Never hand-edit them; this prompt changes no hub code, so it must not need to regenerate them
  (if `npm run gen:api` produces a diff, a prerequisite is wrong: stop and report).
- Contract shapes (introduced by 27 and 29; **verify every field name against the generated `schema.d.ts`**):

```ts
OperationSummaryDto { id; kind; goal; status; pause_reason: {type; exhausted?; step?; message?} | null;
                      waiting_for: string[]; cycle; incidents: string[]; hosts: string[];
                      created_at; started_at | null; ended_at | null; updated_at }
OperationPage       { items: OperationSummaryDto[]; next_cursor: string | null; as_of_seq: number }
OperationDetailDto  { as_of_seq; operation: OperationSummaryDto; trace: OperationTraceDto; executions: ExecutionDto[] }
ExecutionDto        { step; approval_step | null; contract; contract_version; host_id; check; target | null;
                      args: {[k]: string}; state; plan_hash; plan_binding /* "v2" */; risk | null;
                      confirm_required: boolean; expires_at; approved_by | null; approved_at | null; reason | null }
StepDetailDto       { id; operation_id; ordinal; kind; phase; parent | null; cycle; outcome; started_at | null;
                      ended_at | null; summary | null; reason | null; output | null /* redacted, <= 16 KiB */;
                      intent: unknown /* the stored intent JSON */; created_at }
ApproveStepRequest  { plan_hash; confirm_text? }
OperationControlDto { as_of_seq; operation: OperationSummaryDto }
BudgetExtensionDto  { kind /* model_turns evidence_requests executions cycles wall_time tokens cost */; limit }
```

  `ExecutionDto.state` is one of `proposed awaiting_approval authorized dispatched refused not_run unavailable
  succeeded failed outcome_unknown skipped cancelled` (26). Pause reason `type` is one of `budget`
  (`exhausted` = a budget kind), `outcome_unknown` (`step`), `global_pause`, `error` (`message`), `manual` (29).
  `OperationStatus` names: `queued running waiting_approval waiting_user waiting_agent waiting_timer paused
  succeeded failed escalated cancelled`.
- Error behaviour of the routes (27, 29): 404 unknown id; 409 with plain hub text for plan changed, expired, already
  approved, not paused, budget still spent, question not waiting, another change landed first; 403 for a missing
  typed confirmation; 503 "orchestration is not enabled"; lists answer 200 with empty pages when it is off.
  `describeError` (`web/src/design/messages.ts`) already words `conflict`, `forbidden`, `not_found`,
  `unavailable`: **no new error catalogue entry is needed** (if a route answers a code not in `HUB_CODES`, stop).

### 3.4 Kit and design

- `web/src/components/index.ts` **(existing)** is the only import point. Kit pieces used here: `PageHeader`
  (`meta`, `actions`, `intro`, `docs`), `Section` (`title`, `description`, `actions`, `collapsed`), `Card`,
  `DataTable` (`columns`, `rowKey`, `rowHref`, `caption`, `empty`), `FilterChips`, `SegmentedControl`, `Tabs`,
  `KeyValueList`, `StatusPill`, `Tag`, `Count`, `Banner`, `InlineMessage`, `Progress`, `QueryBoundary`, `LoadError`,
  `EmptyState`, `Skeleton`, `Button`, `IconButton`, `LinkButton`, `Menu`, `ConfirmDialog`, `Dialog`, `Field`,
  `NumberInput`, `Textarea`, `CopyBlock`, `RelativeTime`, `Countdown`/`useRemaining`, `Tooltip`, `InfoTip`,
  `useActionFeedback`, `Cluster`, `Stack`, `Text`.
- `web/src/design/tone.ts` **(existing, 208 lines)**: statuses reach the UI only through lookups here; tables sit in
  `*Vocab.ts` and are listed in `VOCABULARIES` (`:` near the end). `design/status.ts` **(existing)**: `Table<K>`,
  `StatusMeta {label, tone, hint, icon}`, `lookup()` (unknown value gives a neutral meta with the raw value).
  `design/icons.tsx` **(existing)**: closed `ICONS`; names used below exist: `play pause ban x-circle check-circle
  clock loader alert skip chat search info rules note tool user help edit digest approve chevron-right
  chevron-down close`.
- `web/src/dev/` **(existing)**: `/dev/design` demo files; every kit component and state gets a demo.
- Tests: `web/src/test/{apiMock,render,fixtures,axe,setup}.ts*` **(existing)**. `renderAt(url, path, element)` wraps
  `HostProvider`, a fresh `QueryClient`, `ToastProvider` and a data router. `apiMock` answers `api.GET/POST` from
  routes a test registers (`route("GET /api/v1/operations/{id}", handler)`), records `calls`, and returns
  `failure(status, code, message)`. `setup.ts` runs axe over whatever a test under `src/(features|app|dev|components)`
  leaves on screen (`skipAxe()` only for a deliberate fragment). jsdom has no `ResizeObserver` and no layout.

### 3.5 Playwright today

- `playwright ^1.55.1` is already a **devDependency** (`web/package.json`), used by `web/scripts/screens.mjs`
  **(existing)**: a library script (`import { chromium } from "playwright"`), run by hand, **not in CI**, driving an
  installed Chrome (`SCREENS_CHANNEL`, default `chrome`). There is **no** `@playwright/test` runner, no
  `playwright.config`, no e2e directory. CI job `web` **(existing, `.github/workflows/ci.yml:283-311`)** runs
  `npm ci`, `format:check`, `lint`, `test`, the `gen:api` diff check, `build`, then uploads `dist`.
  `TESTING.md:135` documents `screens.mjs` as not part of CI.

## 4. Exact scope

All paths are under `web/` unless stated. Every source file stays at or under 250 lines (blank lines and
comments are not counted by the rule, but plan for it). **(new)** means the file does not exist yet.

### 4.1 Right-slot manager and shell (`src/app/`, `src/components/`, `src/styles/`)

| File | Change |
|---|---|
| `src/app/rightSlot/types.ts` **(new)** | `SurfaceId`, `SurfaceSpec`, `OpenOptions`, `RightSlotApi` (6.1) |
| `src/app/rightSlot/layout.ts` **(new)** | pure `slotLayout`, `SLOT_WIDTH_PX = 400`, `SLOT_CONTENT_MIN_PX = 640` (6.2) |
| `src/app/rightSlot/state.ts` **(new)** | pure reducer: register, unregister, open, close, measure (6.3) |
| `src/app/rightSlot/RightSlotContext.tsx` **(new)** | `RightSlotProvider`, `useRightSlot`, `useSlotModal` |
| `src/app/rightSlot/RightSlotSurface.tsx` **(new)** | registers a surface and portals its children into the host while open |
| `src/app/rightSlot/RightSlotHost.tsx` **(new)** | the column or sheet: chrome, tab switch, Escape, focus, inert page, width measurement |
| `src/app/rightSlot/rightSlot.module.css` **(new)** | placement only, tokens only |
| `src/app/rightSlot/index.ts` **(new)** | barrel for the five exports above |
| `src/components/SidePanel.tsx`, `sidePanel.module.css` **(new)**, `index.ts` | kit chrome: header (title, actions, close) and a scrolling body (6.4) |
| `src/app/Layout.tsx`, `Layout.module.css` | wrap in `RightSlotProvider`; new `.body` row holding `<main>` and `<RightSlotHost/>`; mark the sidebar, top bar and `<main>` with `data-slot-inert` |
| `src/app/PageSkeleton.tsx` | `ShellSkeleton` gets the same `.body` wrapper so the frame does not jump |
| `src/app/Sidebar.tsx`, `TopBar.tsx` | only the `data-slot-inert` attribute on their root element |
| `src/styles/tokens.css` | `--right-slot-width: 25rem`, `--right-slot-content-min: 40rem`, `--motion-panel: 240ms` (zeroed under reduced motion) |
| `src/styles/motion.test.ts` **(new)** | motion token guard (12.1) |

### 4.2 Data layer (`src/api/`, `src/design/`)

| File | Change |
|---|---|
| `src/api/types.ts` | aliases: `OperationSummary`, `OperationPage`, `OperationDetail`, `Execution`, `StepDetail`, `OperationControl`, `BudgetExtension` (from the generated schemas; the trace types stay in `operationTrace.ts`) |
| `src/api/queryClient.ts` | add `operationList(filters)` = `["operations","list",filters]` and `operationStep(op, step)` = `["operations","detail",op,"step",step]` beside 28's `operations` and `operation(id)`; do **not** redefine those two |
| `src/api/hooks/operations.ts` **(new)** | `useOperations`, `useOperation`, `useOperationStep` (6.8) |
| `src/api/hooks/operationControls.ts` **(new)** | `useApproveStep`, `useDenyStep`, `useCancelOperation`, `usePauseOperation`, `useResumeOperation`, `useAnswerStep` (6.8) |
| `src/design/operationVocab.ts` **(new)** | `OPERATION_STATUSES`, `STEP_OUTCOMES`, `EXECUTION_STATES`, `STEP_KINDS`, `EDGE_KINDS` (6.5) |
| `src/design/operationText.ts` **(new)** | `waitText`, `budgetLabel`, `REVERSE_EDGE_LABELS`, banner and pause words (6.6) |
| `src/design/tone.ts`, `status.ts`, `actionVocab.ts` | lookups `operationStatusMeta`, `stepOutcomeMeta`, `executionStateMeta`, `stepKindMeta`, `edgeKindMeta`, their entries in `VOCABULARIES`, their types in `status.ts`; `Risk` gains `unknown` and `RISKS.unknown` (6.5) |

### 4.3 Approval card (`src/features/approvals/`)

| File | Change |
|---|---|
| `approvalPlan.ts` **(new)** | `ApprovalPlan`, `planFromAction`, `planFromExecution`, `skillName` (6.7) |
| `approvalBits.tsx` **(new)** | `Expiry`, `suggestedBy`, `EXPIRY_WARNING_MS` moved here **unchanged** |
| `ApprovalCard.tsx` **(new)** | the one card (6.7) |
| `StepApproveControl.tsx` **(new)** | Approve (ConfirmDialog, typed confirmation, shown hash) and Deny for a step |
| `ActionCard.tsx` | becomes a thin wrapper over `ApprovalCard` for a legacy `Action`; re-exports `Expiry`, `suggestedBy`, `EXPIRY_WARNING_MS`; same props and output as today |

### 4.4 Operations feature (`src/features/operations/`, all **(new)**)

`OperationsPage.tsx` (list page), `OperationsTable.tsx`, `operationRows.ts` (merge), `OperationDetailPage.tsx`,
`OperationHeader.tsx`, `OperationBanner.tsx`, `banner.ts`, `WaitingForYou.tsx`, `QuestionCard.tsx`,
`StepOutline.tsx`, `StepRow.tsx`, `steps.ts`, `StepInspector.tsx`, `InspectorSections.tsx`,
`OperationDetails.tsx`, `budget.ts`, `CancelDialog.tsx`, `RaiseLimitDialog.tsx`, `StaleNote.tsx`,
`IncidentOperations.tsx`, `operations.module.css`, and the tests of section 12.

### 4.5 Routes, navigation, incident link (`src/app/`, `src/features/incidents/`)

| File | Change |
|---|---|
| `src/app/pages.ts` | `operations` and `operation` lazy pages |
| `src/app/routes.tsx` | `<Route path="/operations" …/>` and `<Route path="/operations/:id" …/>` from `PAGES`, inside `<Layout/>` |
| `src/app/nav.ts` | item `{ to: "/operations", label: "Operations", icon: "play", preload: PAGES.operations.preload }` between Incidents and Approvals; **no count** (the overview has no operation counts) |
| `src/app/PageTitle.tsx` | `defaultDetail`: section `operations` with a rest gives "Operation" |
| `src/features/incidents/IncidentDetailPage.tsx`, `incidents.module.css` | one new slot `data-slot="operations"` after `going`, rendering `<IncidentOperations operations={data.operations}/>` |

### 4.6 Smoke test, scripts, CI, docs

| File | Change |
|---|---|
| `scripts/smoke.mjs`, `scripts/smoke/{server,data,checks}.mjs` **(new)** | the Playwright smoke test and its mock hub (6.10) |
| `package.json` | script `"smoke": "node scripts/smoke.mjs"`; **no new dependency** |
| `scripts/screens.mjs` | add `["operations", "/operations"]` and, when `SCREENS_OPERATION` names an id, `["operation", "/operations/<id>"]` |
| `../.github/workflows/ci.yml` | in job `web`, after `npm run build`: install Chromium, run `npm run smoke` (6.10) |
| `src/dev/OperationDemos.tsx` **(new)**, `src/dev/DesignPage.tsx`, `src/dev/operationDemos.test.tsx` **(new)** | catalogue demos (6.9) |
| `DESIGN.md` | updates of section 15 |
| `../TESTING.md`, `../AGENTS.md`, `README.md` | the new command (section 15) |
| `../docs/book/src/concepts/operations.md` | a section "The Operations page" (section 15) |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| The interactive graph canvas, its keyboard model, playback | 33 (spike), 38, 52 |
| Visibility profiles, `useVisibility`, the P0 to P3 preference | 37 |
| The Operator panel, conversations, streaming text, `live.*` events | 36 (registers `RightSlotSurface id="operator"`; builds no dock) |
| Attention centre, autonomy indicator, Control Room, global search | 51 |
| Policy pages, policy outcome statuses, the effect class and policy decision **data** | 40, 43 (the card only has optional slots for them) |
| The unverified-human variant for containment, helper and key screens | 46, 47, 49 |
| Any Rust, wire, OpenAPI or migration change; new hub routes | none (stop and report if one seems needed) |
| Changing the Approvals page, the legacy approve routes or counts. **Known gap**, report it: the overview's `approvals_waiting` and the Approvals page list legacy actions only; a step waiting for approval is found through the Operations list, the operation page and the incident page | 51 |
| A new Playwright test runner (`@playwright/test`) or any new dependency | not needed |
| Real-hub end-to-end tests, CSP checks of the embedded UI | the hub's own tests; the smoke test serves `dist` from Node |
| Virtualised lists (a step list over 200 rows shows a "Show earlier steps" button) | a later kit component |
| Persisting the slot's open state or the timeline/tree choice across reloads | per-viewer conveniences are not needed now |
| Seeding operations into `examples/dev_hub.rs` | Rust; `SCREENS_OPERATION` covers manual checks |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly), **convention** (follow repository style).

### 6.1 Right-slot manager API **fixed** (this is what prompt 36 and later surfaces call)

```ts
// web/src/app/rightSlot/types.ts
/** A surface's stable id. Known today: "inspector". Prompt 36 adds "operator". */
export type SurfaceId = string;

/** What a surface says about itself; given to RightSlotSurface while it is mounted. */
export interface SurfaceSpec {
  id: SurfaceId;
  /** The surface's accessible name, its heading and its tab: "Step inspector", "Operator". */
  label: string;
  /** A name from design/icons.tsx, used on the tab. */
  icon: IconName;
}

export interface OpenOptions {
  /** Where focus returns when the surface closes. Default: `document.activeElement` at the call. */
  opener?: HTMLElement | null;
  /** Move focus into the surface on opening. Default true; false for a surface opened by an address. */
  focus?: boolean;
}

export interface RightSlotApi {
  /** Surfaces currently registered (mounted), by id. */
  specs: Readonly<Record<SurfaceId, SurfaceSpec>>;
  /** Open surfaces, oldest first: the last is the most recently opened or chosen. */
  openIds: readonly SurfaceId[];
  isOpen(id: SurfaceId): boolean;
  /** Opens `id`, or brings it to the front when it is already open. A no-op for an unregistered id. */
  open(id: SurfaceId, options?: OpenOptions): void;
  close(id: SurfaceId): void;
  toggle(id: SurfaceId, options?: OpenOptions): void;
}
```

```tsx
// web/src/app/rightSlot/index.ts exports:
export function RightSlotProvider(props: { children: ReactNode }): JSX.Element;   // once, in Layout
export function RightSlotHost(): JSX.Element | null;                              // once, in Layout, beside <main>
export function useRightSlot(): RightSlotApi;      // throws "useRightSlot needs a RightSlotProvider above it" outside the provider
export function useSlotModal(): boolean;           // true while the slot is a modal sheet (the page is inert)
export function RightSlotSurface(props: {
  spec: SurfaceSpec;
  children: ReactNode;                // rendered ONLY while the surface is open, inside the slot; never mounted while closed
  actions?: ReactNode;                // header actions, left of the close button
  onClose?: () => void;               // called when the SLOT closes the surface (Escape, close button, tab close); not on unmount
  initialFocus?: () => HTMLElement | null;   // default: the surface's heading (tabIndex -1)
}): JSX.Element | null;
export { slotLayout, SLOT_WIDTH_PX, SLOT_CONTENT_MIN_PX } from "./layout";
```

Rules **fixed**:

1. A surface owner mounts `<RightSlotSurface spec=…>` (it registers the spec on mount, unregisters on unmount). A
   closed surface renders nothing and mounts none of its children (so a closed Operator panel holds no stream).
   A surface that unmounts while open is removed from `openIds` without calling `onClose`.
2. `open` records `opener` (see `OpenOptions`) and, unless `focus === false`, moves focus into the surface after it
   renders (`initialFocus()`, else the surface heading). Opening a surface that is already open only brings it to the
   front: it never steals focus.
3. `close(id)` removes it from `openIds`, calls its `onClose`, and returns focus to the recorded opener when that
   element is still in the document, else to `#main`.
4. **Escape** (one thing per press): a `keydown` listener on the host acts only when `event.key === "Escape"`, the
   event is not `defaultPrevented`, and `document.activeElement` is inside a surface. It calls `close` for that
   surface and `preventDefault()`. Tooltips, popovers, menus, dialogs and the command palette already consume
   Escape first (`Dialog` also stops propagation), so a popover inside the inspector closes before the inspector
   does. Escape with focus on the page does nothing here.
5. The host is a `<aside>` labelled by the surface (`aria-label = spec.label`) beside the page; in sheet mode it
   is `role="dialog" aria-modal="true"` with the same label.
6. The slot is not persisted, and its state is per tab.

### 6.2 Layout rule **fixed**

```ts
// web/src/app/rightSlot/layout.ts
export const SLOT_WIDTH_PX = 400;          // equals --right-slot-width (25rem)
export const SLOT_CONTENT_MIN_PX = 640;    // equals --right-slot-content-min (40rem): the page keeps at least this
export type SlotMode = "closed" | "side" | "sheet";
export interface SlotLayout {
  mode: SlotMode;
  visible: readonly SurfaceId[];   // rendered now, in open order; at most two
  tabs: readonly SurfaceId[];      // when more are open than visible: every open id, to switch between; else []
}
/** `available` is the width of the page body: the main column plus the slot (px). `wide` is WIDE_QUERY (>= 768 px). */
export function slotLayout(input: { available: number; wide: boolean; open: readonly SurfaceId[] }): SlotLayout;
```

```text
slotLayout:
  open empty                                   -> { closed, [], [] }
  not wide                                     -> { sheet, [newest], tabs: open.length > 1 ? open : [] }
  fits(n) = available - n * SLOT_WIDTH_PX >= SLOT_CONTENT_MIN_PX
  n = min(open.length, 2)
  fits(n)                                      -> { side, last n of open, [] }
  open.length > 1 and fits(1)                  -> { side, [newest], open }          // the other collapses to a tab
  fits(1)                                      -> { side, [newest], [] }
  otherwise                                    -> { sheet, [newest], open.length > 1 ? open : [] }
```

Examples (these are test cases): 1040 px with one open -> `side`; 1039 -> `sheet`; 1440 with two open -> both
`side`; 1439 with two open -> `side`, newest visible, `tabs` = both ids; 900 with two open -> `sheet` with tabs;
any width with `wide: false` -> `sheet`. `available` is measured by a `ResizeObserver` on the `.body` element and
stored in the provider (reducer action `measure`); where `ResizeObserver` does not exist (jsdom) the host reads
`element.clientWidth` once and on `window` resize, and a test may set it with `measure`. The 640 px counts the
`<main>` element's outer width (its padding included).

A **tab switch** (a kit `SegmentedControl` with `label="Side panels"` and one option per id in `tabs`, icon and
label from the specs) shows above the visible surface(s) whenever `tabs` is not empty. Choosing a tab calls
`open(id)`, which brings it to the front.

### 6.3 State **convention** (pure reducer in `state.ts`, fully unit-tested)

```ts
interface SlotState { specs: Record<SurfaceId, SurfaceSpec>; open: SurfaceId[]; openers: Record<SurfaceId, HTMLElement | null>; available: number }
type SlotAction =
  | { type: "register"; spec: SurfaceSpec } | { type: "unregister"; id: SurfaceId }
  | { type: "open"; id: SurfaceId; opener: HTMLElement | null } | { type: "close"; id: SurfaceId }
  | { type: "measure"; available: number };
```
`open` of an unregistered id changes nothing; `open` of an open id moves it to the end; `unregister` also drops it
from `open`. `onClose` callbacks are looked up by the provider (a ref map), not stored in the reducer.

### 6.4 Kit chrome and tokens **fixed**

```tsx
// web/src/components/SidePanel.tsx (new kit component; JSDoc says what it is for and not for)
SidePanel(props: { title: string; onClose: () => void; actions?: ReactNode; headingRef?: Ref<HTMLHeadingElement>;
                   mode?: "side" | "sheet"; children: ReactNode }): JSX.Element
```
A header with an `h2` (16 px/600), the actions, an `IconButton icon="close" label="Close {title}"`, and a body that
scrolls on its own (`min-height: 0; overflow-y: auto`). It has no knowledge of the slot. The host renders one
`SidePanel` per visible surface. Prompt 36 listed a `SidePanel({ title, open, onClose, … })` of its own: this one
replaces it (no `open` prop: the slot decides).

Tokens in `tokens.css` (not colours, so the two dark blocks are untouched), under "Layout" and "Motion":
`--right-slot-width: 25rem;`, `--right-slot-content-min: 40rem;`, `--motion-panel: 240ms;`; the reduced-motion block sets
`--motion-panel: 0ms;`. Opening a surface runs `animation: slot-in var(--motion-panel) var(--ease-out)` with
`@keyframes slot-in { from { transform: translateX(var(--space-6)); } }` on the panel only (`transform` only, no
opacity, no width or height change): the slot column takes its final width at once, so the page never reflows
mid-transition. Closing is instant. Sheet mode: `position: fixed; inset: 0; z-index: var(--z-overlay)` on a surface
background, so toasts (`--z-toast`) stay above it. While `mode === "sheet"` the host sets the `inert` attribute on every
`[data-slot-inert]` element (sidebar, top bar, main) and removes it when the mode changes or the host unmounts.

### 6.5 Vocabularies **fixed** (words) and wiring **convention**

New file `design/operationVocab.ts` exports five `Table<…>` objects. Every entry has `label`, `tone`, `hint`, `icon`;
the lookups go in `tone.ts` and the tables join `VOCABULARIES` (the existing `tone.test.ts` then checks them).
Sentence case; no exclamation marks.

`OPERATION_STATUSES` (key: label, tone, icon, hint):

| key | label | tone | icon | hint |
|---|---|---|---|---|
| `queued` | Queued | neutral | clock | Waiting for a free slot; it starts on its own. |
| `running` | Running | progress | loader | Domovoi is working on it now. |
| `waiting_approval` | Waiting for approval | pending | clock | Nothing runs until a person approves the proposed action. |
| `waiting_user` | Waiting for your answer | pending | help | Domovoi asked a question and continues once you answer. |
| `waiting_agent` | Waiting for the host | info | offline | A host's agent is not connected; it continues when the agent returns. |
| `waiting_timer` | Waiting for a check | info | clock | A follow-up check is scheduled; it continues when it is due. |
| `paused` | Paused | warning | pause | It stopped and does nothing until a person resumes it. |
| `succeeded` | Succeeded | success | check-circle | It finished and reached its goal. |
| `failed` | Failed | danger | x-circle | It finished without reaching its goal. |
| `escalated` | Escalated | danger | alert | Domovoi stopped and handed it to a person. |
| `cancelled` | Cancelled | neutral | ban | A person cancelled it. |

`STEP_OUTCOMES`: `pending` Not started (neutral, clock, "Waiting its turn."); `running` Running (progress, loader, "In
progress now."); `succeeded` Succeeded (success, check, "It finished as intended."); `failed` Failed (danger, x-circle,
"It ran and reported a failure."); `refused` Refused (danger, ban, "Something refused to run it, such as a check, a
person or the agent."); `not_run` Never ran (neutral, ban, "Nothing was started: the approval expired, or the host has
no record of it."); `unavailable` Unavailable (warning, offline, "What it needs was not available, such as the host or a
capability."); `outcome_unknown` Result unknown (warning, alert, "It may have run on the host; check the host before
running it again."); `skipped` Skipped (neutral, skip, "It was not needed and was left out."); `cancelled` Cancelled
(neutral, x, "It was cancelled before it finished.").

`EXECUTION_STATES`: `proposed` Proposed (neutral, clock, "Proposed; its approval is not open yet."); `awaiting_approval`
Waiting for approval (pending, clock, "Nothing runs until a person approves it."); `authorized` Approved (progress, loader,
"Approved; about to be sent to the host's agent."); `dispatched` Running (progress, loader, "Sent to the host's agent;
waiting for the result."); `refused` Refused (danger, ban, "A person denied it, or the plan no longer fit. Nothing ran.");
`not_run` Never ran (neutral, ban, "Nobody approved it in time, or the agent has no record of it. Nothing ran.");
`unavailable` Unavailable (warning, offline, "The host or the capability was not available. Nothing ran."); `succeeded`,
`failed`, `outcome_unknown`, `skipped`, `cancelled` as in `STEP_OUTCOMES`.

`STEP_KINDS` (all `neutral`; shown as icon plus label, not as a pill): `reasoning_turn` Model turn (chat, "Domovoi asked
the model to think about the evidence."); `evidence_request` Evidence (search, "A read-only look at a host, such as logs or
container state."); `assessment` Assessment (info, "What Domovoi concluded from the evidence."); `plan` Plan (note, "What
Domovoi proposes to do."); `policy_decision` Policy decision (rules, "How a policy rule judged an action."); `approval`
Approval (user, "A person decides whether an action may run."); `execution` Action (tool, "An action that changes
something on a host, once approved."); `verification` Check (check-circle, "A follow-up check that the action fixed the
problem."); `notification` Notification (digest, "A message sent to you."); `question` Question (help, "Domovoi asks you
something it cannot find out itself."); `note` Note (edit, "A comment added by a person or by Domovoi.").

`EDGE_KINDS` (neutral, icon `chevron-right`): `depends_on` Depends on; `caused_by` Caused by; `verifies` Verifies;
`retry_of` Retry of; `answers` Answers (hints one short sentence each).

Lookups in `tone.ts` (one line each): `operationStatusMeta(status)`, `stepOutcomeMeta(outcome)`,
`executionStateMeta(state)`, `stepKindMeta(kind)`, `edgeKindMeta(kind)`, all through `lookup()` so an unknown value is a
neutral pill with the raw value. Types go in `status.ts`. **Risk**: add `"unknown"` to `Risk` and an entry `RISKS.unknown`
(label "Risk unknown", tone warning, icon alert, hint "The skill is not on the host now, so its risk is unknown. It is
treated as destructive: its name must be typed to approve."); `grep -rn "Risk\b" src` and fix any exhaustive use.

### 6.6 Words for waits, pauses and budgets **fixed** (`design/operationText.ts`)

```ts
export type BudgetKind = "model_turns" | "evidence_requests" | "executions" | "cycles" | "wall_time" | "tokens" | "cost";
export const BUDGET_KINDS: readonly BudgetKind[];
export function budgetLabel(kind: string): string;   // Model turns, Evidence requests, Actions, Cycles, Time, Tokens, Cost; unknown: the raw string
export function waitText(kind: string): string;      // approval "waiting for your approval", user "waiting for your answer",
                                                     // agent "waiting for the host's agent", timer "waiting for a scheduled check"; unknown: "waiting"
export const REVERSE_EDGE_LABELS: Record<string, string>;  // depends_on "Needed by", caused_by "Led to", verifies "Verified by",
                                                           // retry_of "Retried by", answers "Answered by"
export function reverseEdgeLabel(kind: string): string;    // unknown: the raw kind
```

### 6.7 `ApprovalCard` **fixed** (name, props and model: prompts 43, 49 and 51 call it)

```ts
// web/src/features/approvals/approvalPlan.ts
export interface ApprovalPlan {
  id: string;                      // action id or step id (keys, aria)
  family: "step" | "action";
  skill: string;                   // display name: skillName(contract)
  contract: string;                // step: "skill.restart_container"; action: the skill name
  contractVersion: number | null;
  host: string | undefined;        // a display name when the caller knows it
  check: string | null; target: string | null;
  args: Record<string, string>;
  risk: string;                    // "read" | "write" | "destructive" | "unknown" (null risk becomes "unknown")
  state: string;                   // execution state (step) or action status (action)
  stateMeta: StatusMeta;           // executionStateMeta(...) or actionStatusMeta(...)
  waiting: boolean;                // step: state === "awaiting_approval"; action: status === "proposed"
  inFlight: boolean;               // step: authorized | dispatched; action: approved | dispatched
  planHash: string; planBinding: string;      // as received: "v2" for steps; "v1" or "v2" for actions
  expiresAt: string;
  createdAt: string | null; suggestedBy: string | null;   // action only
  approvedBy: string | null; approvedAt: string | null;   // step only
  reason: string | null;
  confirmRequired: boolean;        // step: ExecutionDto.confirm_required; action: risk === "destructive"
  confirmText: string;             // what must be typed: skillName(contract)
}
export function skillName(contract: string): string;   // "skill.restart_container" -> "restart_container"; anything else unchanged
export function planFromExecution(execution: Execution, host?: string): ApprovalPlan;
export function planFromAction(action: Action, host?: string): ApprovalPlan;
```
`skillName` and `confirmText` are **convention until verified**: prompt 27's approve route compares `confirm_text` with
"the skill's name"; read `crates/domovoi-hub/src/api/operations.rs` and use exactly what it compares (if it compares the
whole contract id, `confirmText` is `contract`). Record which in the completion report.

```tsx
// web/src/features/approvals/ApprovalCard.tsx
export type ApprovalSource =
  | { kind: "step"; operationId: string; execution: Execution }
  | { kind: "legacy"; action: Action };

export interface ApprovalExtras {            // data later prompts supply; the card renders only what is present
  effect?: { class: string; reversible: boolean | null };   // Tags "Effect: <class>" and "Reversible" / "Not reversible" / nothing when null
  policy?: { outcome: string; rule?: string };               // one labelled text line: "Policy: <outcome> (rule <rule>)"; no pill yet
  refusal?: { reason: string };                              // InlineMessage tone warning, title "The agent refused it", body = reason
}

export interface ApprovalCardProps {
  source: ApprovalSource;
  host?: string | undefined;                 // host display name: "on nas"
  why?: string | undefined;                  // model text; shown secondary as "Model's reason (not checked): …"
  extras?: ApprovalExtras | undefined;
  emphasis?: "primary" | "secondary";        // how Approve is drawn; default "primary"; destructive/unknown risk is always danger
  readOnly?: boolean;                        // no Approve/Deny: the inspector's use
  operation?: { id: string; goal: string };  // adds a link "<goal>" to /operations/<id>, for lists outside the detail page
  incident?: { id: string; title?: string }; // legacy only: adds a link to the incident (ActionCard's `showIncident`)
}
export function ApprovalCard(props: ApprovalCardProps): JSX.Element;
```

Behaviour **fixed**:

1. Layout is today's `ActionCard`'s: `Card` with `tone` from `plan.stateMeta.tone` and `label={"Action " + plan.skill}`
   (role group); header: skill (mono, strong), "on {host}" muted, risk `StatusPill` plus `InfoTip` (learnMore
   `concepts/actions.html`), the state pill when not waiting; then check and target as `Tag`s ("Check disk.usage",
   "Target plex"; only when present), `ActionArgs`, the model text, the `extras`, a footer `Cluster` (who suggested it or
   who approved it and when, `Expiry` while waiting, `PlanTag planHash planBinding`, the optional links), the state note,
   then the decision area.
2. **State note** by `plan.state` (never colour alone): in flight -> `Progress` ("Approved. Sending it to the agent…" /
   "Running on {host}. Waiting for the result…"); `not_run`, `refused`, `unavailable`, `failed`, `outcome_unknown`,
   `expired`, `rejected`, `verify_failed`, `denied` -> an `InlineMessage` with the vocabulary's `hint` as title and
   `plan.reason` as body (tone from the state meta, `outcome_unknown` warning). Action statuses keep exactly the notes
   `ActionCard` draws today (`approvals.test.tsx` proves it).
3. **Decision area**, only when `plan.waiting && !readOnly`: for `kind: "legacy"` the existing `ApproveControl`; for
   `kind: "step"` `StepApproveControl` (below). A destructive or unknown-risk step's Approve is a `danger` button; any
   other is `emphasis`. Approve and Deny always carry `hint`s as `ApproveControl` does.
4. `StepApproveControl`: Approve opens a `ConfirmDialog`: title `Run {skill}?` (`Run destructive action {skill}?` when
   `confirmRequired`), consequence "{skill} with {args} runs on {host} as soon as you approve." (destructive: "… It can
   lose data, and it cannot be undone."; unknown risk: "Its risk is unknown, so it is treated as destructive."),
   `requireText = plan.confirmText` when `confirmRequired`, `confirmLabel="Approve"`, tone `danger` when `confirmRequired`.
   **The plan hash sent is the one in `plan.planHash` at the moment the dialog opened**: keep it in state on open; if
   a refetch changes `plan.planHash` while the dialog is open, set `confirmDisabledReason` to "The plan changed while you
   were looking. Close this and review it again." and show an `InlineMessage`. Approve is disabled with
   `disabledReason` "This request expired; triage can propose it again." once `useRemaining(expiresAt) === 0`. Deny is one
   click (no dialog). Mutations: `useActionFeedback(useApproveStep(operationId), { success: "Approved {skill}. It runs now.",
   error: "approve the action", errorDisplay: "inline" })` and `useActionFeedback(useDenyStep(operationId), { success:
   "Denied {skill}. Nothing ran.", error: "deny the action" })`. A 409 (plan changed, expired, already approved) shows
   inside the dialog with the hub's text through `describeError`.
5. `ActionCard` keeps its exact props (`action`, `host`, `why`, `showIncident`, `incidentTitle`, `approve`) and output; it
   only maps to `ApprovalCard source={{kind:"legacy", action}}`. A **legacy** card shows `PlanTag` with the action's own
   binding ("v1 binding" for v1); a **step** card shows "v2" (`ExecutionDto.plan_binding`); an unknown binding shows
   "unknown binding" (existing `PlanTag`).
6. Nothing in the card approves anything by itself: no effect calls a mutation; Approve and the dialog's confirm are the
   only two paths, both user clicks.

### 6.8 Hooks **fixed** (names and request shapes), **convention** (layout)

```ts
export interface OperationFilters { status?: string | undefined; incident?: string | undefined; host?: string | undefined; kind?: string | undefined }
export const OPERATION_PAGE = 50;
useOperations(filters, enabled = true)      // useInfiniteQuery, key keys.operationList(filters); GET /api/v1/operations with query
                                            // defined({...filters, cursor, limit: OPERATION_PAGE}); getNextPageParam = next_cursor
useOperation(id)                            // useQuery, key keys.operation(id); GET /api/v1/operations/{id}; data: OperationDetail
useOperationStep(operationId, stepId | undefined)   // key keys.operationStep(..); GET .../steps/{step}; enabled only with a stepId

useApproveStep(operationId)   // variables { step, planHash, confirmText? } -> POST .../steps/{step}/approve body { plan_hash, confirm_text? } (confirm_text omitted when undefined)
useDenyStep(operationId)      // { step } -> POST .../steps/{step}/deny
useCancelOperation(id)        // { reason? } -> POST /operations/{id}/cancel body { reason } ({} when none)
usePauseOperation(id)         // void -> POST /operations/{id}/pause (no body)
useResumeOperation(id)        // { extend: BudgetExtension[] } -> POST /operations/{id}/resume body { extend }
useAnswerStep(id)             // { step, text } -> POST /operations/{id}/steps/{step}/answer body { text }
```
Every mutation's `onSuccess` invalidates `keys.operations` (a prefix: the list, the detail and the step caches), `keys.overview`
and `keys.incidentDetails`; none sets query data by hand and none retries (the client default). Query errors are never
swallowed: pages use `QueryBoundary`/`LoadError`.

### 6.9 Pages **fixed** (behaviour), **convention** (markup)

**List** `/operations` (`OperationsPage`): `PageHeader title="Operations" intro="What Domovoi is investigating and
fixing, and what waits for you." docs="concepts/operations.html"`. `FilterChips label="Status"` with values `all`,
`waiting_approval`, `waiting_user`, `paused`, `running` (labels from `operationStatusMeta`; no counts: there are none to
show). Status and the page's host selector (`useSelectedHost`, sent as `host`) are asked of the hub. The status is in the
URL (`?status=`). Legacy actions: `useActions({ host, status })` where `status` is `proposed` for the chip
`waiting_approval`, absent for `all`, and the legacy source is **not queried** (and shows nothing) for the other chips; only
rows with `legacy === true` are listed. `mergeRows` (`operationRows.ts`, pure):

```ts
export type OperationRow =
  | { kind: "operation"; id: string; goal: string; status: string; hosts: string[]; updatedAt: string; href: string }
  | { kind: "legacy"; id: string; goal: string; status: string; hosts: string[]; updatedAt: string; href: string };
export function mergeRows(input: { operations: OperationSummary[]; legacy: Action[]; hostNames: Map<string, string>;
  moreOperations: boolean; moreLegacy: boolean }): OperationRow[];
```
Sorted by `updatedAt` descending (ties: operations first, then id). Legacy `goal` is `"{skill} on {host}"` (host name or
"a host"), `href` `/incidents/{incident_id}`, status pill through `actionStatusMeta`, label `Tag` "Legacy" with hint "From
before operations. It finishes the classic way and is not converted." Operation `href` `/operations/{id}`. **Gap rule**: when
a source still has more pages, rows older than that source's oldest loaded `updatedAt` are not shown (so no row hides behind
an unloaded one); "Load more" fetches the next page of every source that has one. Columns (`DataTable`, stable `useMemo`):
Operation (goal link, `clamp2`, the Legacy tag), Status (`StatusPill sm`), Hosts (names, "+N" when more than two, "–" when
none), Updated (`RelativeTime`). The whole row is the click target (`rowHref`). States: `QueryBoundary` with a `Skeleton
rows`; failure is `LoadError` "load operations" with Retry (a failed legacy source shows an `InlineMessage` above the table
and still lists operations); empty: `EmptyState title="No operations yet"` ("With orchestration on, each new incident is
investigated as an operation. Classic approvals stay on the Approvals page.") and a `Link` to `/approvals`. Under a chip
the empty title is "No operations in this state". The page follows the host selector.

**Detail** `/operations/:id` (`OperationDetailPage`): `useOperation(id)`; `usePageName` (goal cut at 60 characters).
404 -> `EmptyState` "No such operation" with a link back; other failure -> `LoadError` "load the operation"; loading ->
`Skeleton`s. Renders, top to bottom (one `<h1>`: `PageHeader title={goal}`):

1. `PageHeader meta`: `StatusPill` (`operationStatusMeta`), `Tag` "Cycle N" when `cycle > 0`, a `Tag` per host name;
   `actions`: **Pause** (`Button variant="secondary"`, icon `pause`) and a `Menu` "More" with "Cancel operation…". Pause
   is hidden when terminal or paused; for `queued` it has `disabledReason` "It has not started yet. Cancel it instead.".
   Cancel is hidden when terminal. Pause: `useActionFeedback(usePauseOperation(id), { success: "Operation paused. Nothing
   new starts until you resume it.", error: "pause the operation" })`.
2. `StaleNote` when the query has data **and** `isError` (a refresh failed): `InlineMessage tone="warning" title="Could not
   refresh this page"` with "Showing what loaded {RelativeTime of dataUpdatedAt}." and a Retry `Button`.
3. `AllHostsNote` ("An operation is shown whatever host is selected.").
4. **One banner** (`OperationBanner`) from `operationBanner(...)` in `banner.ts` (pure, 6.11).
5. While `running`, `Progress label="Working: {summary of the running step}"` (or "Working…" with no summary); while
   `queued`, "Queued. It starts when a slot is free."
6. `Section title="Waiting for you"` (anchor id `waiting-for-you`, heading focusable by the banner's Review button),
   rendered **only when** something waits: one `ApprovalCard source={{kind:"step", operationId, execution}}` per execution
   whose state is `awaiting_approval` (host name from `useHostNames()`, `why` = `planTextFor`, `emphasis` "primary" when it is
   the only one, else "secondary"), and one `QuestionCard` per pending question step.
7. `Section title="Steps"` with `actions` = `SegmentedControl label="View"` ("Timeline" | "Tree", default Timeline) and
   `StepOutline` (6.12).
8. `Section title="Details"`: `OperationDetails` (6.13).
9. The inspector surface (6.14).

**Incident page**: `IncidentOperations({ operations })` renders `Section title="Investigations"` with one row per
`OperationSummaryDto` (status `StatusPill sm`, goal as a link to `/operations/{id}`, `RelativeTime updated_at`), nothing at
all when the array is empty (verify the field name `IncidentDetail.operations` in the generated schema).

### 6.10 Playwright smoke test **fixed** (what it checks), **convention** (layout)

`npm run smoke` = `node scripts/smoke.mjs`. Needs `dist/` (run `npm run build` first; it exits with a clear message if `dist/`
is missing). It uses the `playwright` library already installed: `chromium.launch({ channel: process.env.SMOKE_CHANNEL || undefined })`
(empty = Playwright's own Chromium after `npx playwright install chromium`; `SMOKE_CHANNEL=chrome` uses an installed Chrome, as
`screens.mjs` does). Header comment lists the environment like `screens.mjs`. Files: `smoke/server.mjs` (a Node `http` server on
`127.0.0.1:0`: serves `dist/` with fallback to `index.html` for unknown non-`/api` paths, and a **mock hub** for `/api/v1/*`,
including `GET /api/v1/events` as a held-open `text/event-stream` that the checks can push frames into), `smoke/data.mjs`
(builds the list, detail, step and action payloads **from the five fixture JSON files** read with `readFileSync`, so the smoke
test and the vitest tests share the contract), `smoke/checks.mjs`, `smoke.mjs` (orchestration, output, exit code).

The mock answers exactly: `GET /api/v1/session`, `/hosts`, `/overview`, `/system/update`, `/settings` (with `editable: null`),
`/actions` (one `legacy: true` action), `/operations`, `/operations/{id}`, `/operations/{id}/steps/{step}`; `POST` approve, deny,
cancel, pause, resume, answer; and the stream. **Any other `/api` request fails the run** (the script lists it), so a new shell
call is noticed and the mock updated deliberately. A POST changes the mock's state and pushes a journal frame
(`aggregate_type: "operation"`, topics `operations` and `operation:<id>`, `seq` above the page's `as_of_seq`).

Checks, in order (each prints one line; the first failure exits 1 with the page's text and a screenshot path in `smoke-out/`):

1. Open `/operations`: heading "Operations"; the waiting operation and one row tagged "Legacy" are listed; "Operations" is in the
   navigation.
2. Click the operation row: heading is the goal; the banner "Waiting for your approval"; the approval card names the skill, the host,
   the arguments and a `Plan xxxxxxxx` tag with "v2".
3. Click Approve: a dialog opens and nothing was sent; confirm; the captured `POST .../approve` body has `plan_hash` equal to the
   full hash in the page data; with a destructive variant of the data the confirm button stays disabled until the skill name is typed.
4. The mock pushes a frame: the page updates **without a reload** (the banner changes and the card no longer offers Approve).
5. Click a timeline step: the right slot opens beside the page at 1440 px wide; the URL has `?step=`; focus is inside it; Escape closes
   it, the URL loses `?step=` and focus returns to the row.
6. At 375 px: no horizontal scroll (`documentElement.scrollWidth <= clientWidth`) on the list and the detail page; selecting a step opens
   the slot as a full-screen sheet and the page behind has `inert`.
7. With `emulateMedia({ reducedMotion: "reduce" })`: the opened panel's computed `animation-duration` is `0s`.
8. Axe in the real browser: `page.addScriptTag({ path: require.resolve("axe-core") })` then `axe.run` on the list and the detail page
   (and with the inspector open): no violations **including colour contrast**, which jsdom cannot check.
9. No `console.error` and no uncaught page error during the run; no unexpected API request.

CI (`ci.yml`, job `web`, after `- run: npm run build`):
```yaml
      - name: Browser smoke test
        run: |
          npx playwright install --with-deps chromium
          npm run smoke
```
Upload `web/smoke-out` as an artifact `if: failure()` (retention 7 days) and add `smoke-out` to `.gitignore` next to `screens-out`
(verify how `screens-out` is ignored: `web/.gitignore` or the root file).

### 6.11 Banner priority **fixed** (`banner.ts`: `operationBanner(operation, steps, executions, hostNames): BannerSpec | null`)

```ts
export interface BannerSpec { key: string; tone: Tone; title: string; body: string; actions: BannerAction[] }
export type BannerAction = "show_step" | "resume" | "raise_limit" | "review" | "cancel";   // OperationBanner maps each to a Button
```

| # | When (first match wins) | tone | title | body | actions |
|---|---|---|---|---|---|
| 1 | `paused`, reason `outcome_unknown` | warning | "Result unknown: {summary of reason.step, else "an action"}" | "It may have run on {host}. Check the host before continuing, then resume or cancel." | `show_step`, `resume`, `cancel` |
| 2 | `paused`, reason `budget` | warning | "Paused: the {budgetLabel(exhausted).toLowerCase()} limit is spent" | "Raise the limit to let it continue, or cancel it." | `raise_limit`, `cancel` |
| 3 | `paused`, reason `error` | danger | "Paused after an error" | the reason's `message` as given | `resume`, `cancel` |
| 4 | `paused`, reason `manual` | info | "Paused by a person" | "Nothing runs until you resume it." | `resume`, `cancel` |
| 5 | `paused`, any other reason (`global_pause`, unknown) | warning | "Paused" | "This pause is lifted somewhere else. Resume is not available here." (an unknown type adds its raw name) | `cancel` |
| 6 | `waiting_approval` | pending | "Waiting for your approval" | "{n} action(s) below wait for you. Nothing runs until you approve." | `review` |
| 7 | `waiting_user` | pending | "Waiting for your answer" | "Domovoi asked a question below." | `review` |
| 8 | `waiting_agent` | info | "Waiting for {host names}" | "Its agent is not connected. The operation continues when it returns; if it stays away too long the step is marked unavailable." | none |
| 9 | `failed` | danger | "The operation failed" | "It stopped without reaching its goal. The steps below show where." | none |
| 10 | `escalated` | warning | "Domovoi handed this to you" | "It stopped trying. The steps below show what it found and did." | none |
| – | anything else (`queued`, `running`, `waiting_timer`, `succeeded`, `cancelled`) | no banner | | | |

Controls: `show_step` selects the step (sets `?step=`); `resume` calls `useResumeOperation` with `{ extend: [] }` after a
`ConfirmDialog` **only for row 1** (title "Continue without knowing the result?", consequence "Domovoi does not know whether
{step} ran on {host}. Continuing may run it again.", confirm "Resume operation"); rows 3 and 4 resume directly with a toast
"Operation resumed"; `raise_limit` opens `RaiseLimitDialog` (6.13); `review` moves focus to the "Waiting for you" heading and
scrolls it into view; `cancel` opens `CancelDialog` (6.13). The banner is a kit `Banner` (title, body, `action` = a `Cluster` of
`Button size="sm"`), `role` follows the kit (danger is an alert).

### 6.12 Step outline **fixed** (behaviour)

`StepOutline({ trace, executions, selected, onSelect, view })` renders the trace's steps:

- **Timeline**: one `<ol aria-label="Cycle N">` per cycle (ascending), steps in ordinal order, with a visible muted "Cycle N" label
  (only when more than one cycle exists, else one list labelled "Steps"). **Tree**: nested `<ol>` by `parent` (a step whose
  parent is missing is a root); each group has an `IconButton` (`chevron-down`/`chevron-right`, `aria-expanded`) that folds it;
  all groups start open.
- A **row** (`StepRow`): kind icon and label (`stepKindMeta`), the step's `summary` (or nothing), a `Tag` "Retry" when the step
  has a `retry_of` edge, the outcome `StatusPill size="sm"` (`stepOutcomeMeta`), and the time (`RelativeTime` of `ended_at`
  else `started_at`, "–" when neither). The row's title is a `Link` to `?step=<id>` (`replace`), `aria-current="true"` when
  selected; clicking anywhere in the row follows it (the kit's list-row rule). Execution rows add the contract from the
  matching `ExecutionDto` (mono) after the summary.
- Windowing: more than 200 steps show the newest 200 plus every step that is not final (pending or running), every
  `outcome_unknown` step and the selected one; a `Button` "Show {n} earlier steps" reveals the rest.
- No colour-only signal: the outcome has its label; a running step also shows the loader icon.

`steps.ts` (pure, all unit-tested): `byCycle(steps)`, `buildTree(steps)`, `windowSteps(steps, {limit, showAll, keep})`,
`retryTarget(trace, stepId)`, `connections(trace, stepId)` (outgoing: this step depends on or was caused by X, with
`edgeKindMeta` labels; incoming with `reverseEdgeLabel`), `planTextFor(trace, executionStepId)` (the `summary` of the `plan` step
the execution has a `caused_by` or `depends_on` edge to; else the latest `plan` step with a lower ordinal in the same cycle; else
`undefined`), `pendingQuestions(trace)`, `awaitingExecutions(executions)`.

### 6.13 Details, budgets and the two dialogs **fixed**

`OperationDetails` (inside `Section title="Details"`): a bordered `KeyValueList`: Kind, Incidents (links to `/incidents/{id}`),
Started and Ended (`RelativeTime`, "–" when absent), Pinned subjects (one line each: host name, check, target; the only targets
an action may name), then **Budgets**: one row per kind from `BUDGET_KINDS`: "{used} of {limit}" or "{used} (no limit)", with
units from `budget.ts` (`formatBudget(kind, used, limit)`: counts as numbers; time as minutes; cost as money-less units with two
decimals; tokens with thousands separators). `budget.ts` also holds `limitFor(kind, trace)` -> `{ limit: number | null; used: number }`
(mapping `wall_time` to `wall_seconds`, `cost` to `cost_micros`, `cycles` used from `operation.cycle`), `toApiLimit(kind, input)` and
`fromApiLimit(kind, api)` (minutes <-> seconds; cost units <-> micros, `Math.round(x * 1_000_000)`).

`RaiseLimitDialog` (`Dialog`, title "Raise the {label} limit"): shows "Limit now: {formatted}. Used: {formatted}." and a `Field` with
a `NumberInput` (hint "Enter a limit above {limit now}."; default = `Math.max(limit + 1, Math.ceil(limit * 1.5))` in display units);
validation before sending: a number above the current limit, else the field error "Enter a limit above {limit now}."; Submit "Raise
limit and resume" sends `useResumeOperation` `{ extend: [{ kind: exhausted, limit: toApiLimit(...) }] }` with `errorDisplay: "inline"`
so the hub's 400/409 text shows inside the dialog; success toast "Limit raised. The operation resumed."; Cancel closes.

`CancelDialog` (`ConfirmDialog`, `tone="danger"`): title "Cancel this operation?", consequence "“{goal}” stops. Steps that have not started
are cancelled. An action already running on a host is not recalled; its result is still recorded.", a `Field` "Reason (optional)" with a
`Textarea` (hint "Kept in the history. Do not paste secrets."; at most 500 characters: counted in the hint, over the limit disables
confirm with `confirmDisabledReason`), `confirmLabel="Cancel operation"`; sends `{ reason }` (trimmed; omitted when empty); success toast
"Operation cancelled."; inline error on failure.

`QuestionCard` (`Card tone="pending"`, label "Question"): the question text (`intent.text` from `useOperationStep`, falling back to
the trace summary while it loads), an expiry `Expiry` when `intent.expires_at` is a string, `Field label="Your answer"` with a
`Textarea` (hint "Up to 2000 characters. It goes to the model as your answer."), and `Button` "Send answer" (disabled with
`disabledReason` when empty, over 2000 characters, or the operation is `paused`: "Resume the operation before answering."). On success
the field clears and the toast says "Answer sent." The draft is kept in memory only while the card is mounted.

### 6.14 Inspector surface **fixed**

`StepInspector` is mounted once by `OperationDetailPage` as
`<RightSlotSurface spec={{ id: "inspector", label: "Step inspector", icon: "search" }} onClose={clearSelection}>` and
**opened by the selection**: an effect keyed on the valid `?step=` value calls `open("inspector", { focus: <true when the change came
from a row click, false on the first render from a deep link>, opener: the clicked row link })`; when the param is absent or names no
step of the trace it calls `close("inspector")` and drops an invalid param (`replace`). `onClose` removes `?step=`. Content, in
order, as `Section`-free blocks (the slot heading is the panel's `h2`; use `h3`-sized `Text strong` labels inside, no kit `Section`):

1. The step: kind icon and label, outcome `StatusPill`, `phase` and `cycle` as muted text, times (`RelativeTime`, "Took {duration}" when
   both exist).
2. `summary` and `reason` (`reason` as an `InlineMessage` with the outcome's tone when the outcome is `refused`, `not_run`,
   `unavailable`, `failed` or `outcome_unknown`).
3. For an Execution step with an `ExecutionDto`: `ApprovalCard readOnly` (host name, no decision buttons) and, while it awaits approval,
   one line "Decide in Waiting for you on the page." with a `Button` that moves focus there.
4. **Connections**: from `connections(trace, id)`, each a `Link` to `?step=<other>` labelled with the edge label and the other step's
   kind and summary; "No connections" when none.
5. **Details** (`useOperationStep`): loading `Skeleton`; error `LoadError` "load the step" with Retry; the `intent` as a
   `KeyValueList mono` of its top-level keys except `kind` (strings and numbers as text; other values as compact JSON cut at 400
   characters), then the **output** in a `CopyBlock label="Step output"` with a muted line "Redacted and shortened to 16 KiB."; with no
   output: "No output is stored for this step. Raw outputs are removed after the retention period."
6. The panel is a `SidePanel`; keyboard: Tab order follows the blocks above; Escape per 6.1.

Selecting a different step while the inspector is open changes its content, does not move focus, and announces the change through a
visually hidden `role="status"` line ("Showing {kind label}: {summary}").

## 7. Ordered implementation steps

Work in `web/` unless a path says otherwise. Commit in small green steps (one-line Conventional Commits, exactly one scope,
for example `feat(web): right-hand slot for side panels`), after steps 4, 6, 8, 11, 13 and 15. Run `npm run lint`, `npm run
typecheck` and the vitest files of the area after every step you commit.

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Confirm 26, 27, 28, 29 and 59 are merged: `ls src/test/fixtures/operation-trace/` (five files and `trace.test.ts`), `src/api/operationTrace.ts`,
     `src/api/journalStream.ts`, `src/api/journalFrames.ts`, `keys.operations` / `keys.operation`, and in `src/api/schema.d.ts` the paths
     `/api/v1/operations`, `/api/v1/operations/{id}`, `.../steps/{step}`, `.../steps/{step}/approve`, `.../deny`, `/cancel`, `/pause`, `/resume`,
     `.../steps/{step}/answer`, and the schemas of section 3.3 (`ActionDto.legacy`, `IncidentDetail.operations`). **If any is missing, stop and
     report.** Write down every real field name that differs from section 3.3.
   - Read `crates/domovoi-hub/src/api/operations.rs` and `dto_operations.rs` (read only): what `confirm_text` is compared with (decides
     `skillName`, 6.7), the real `ExecutionDto.risk` source, and the 409/403 texts. Read each of the five fixtures completely.
   - Open every web file named in section 3 and confirm the line numbers; run `npm ci` if `node_modules` is stale and run the whole web suite once
     (`npm test`) to know the baseline. Run `grep -rn "Risk\b" src` and `grep -rn "ActionCard\|Expiry\|suggestedBy" src` and keep the lists.
   - Check `.gitignore` handling of `screens-out`; check `docs/book/src/concepts/operations.md` exists (26 creates it); check that
     `ls ../crates/domovoi-hub/src/api/` shows no route you would need that is missing.
2. **Tokens and motion guard.** Add the three tokens (6.4) and the reduced-motion line; add `src/styles/motion.test.ts` (12.1).
3. **Vocabularies.** `design/operationVocab.ts`, `operationText.ts`, the lookups and `VOCABULARIES` entries, `Risk` `unknown`; tests of 12.1.
4. **Data layer.** Type aliases, query keys, `hooks/operations.ts`, `hooks/operationControls.ts`, `hooks/operations.test.ts`. Commit
   `feat(web): operation hooks, vocabularies and motion tokens`.
5. **Right-slot manager.** `types.ts`, `layout.ts` + test, `state.ts` + test, then context, surface, host, CSS, `SidePanel`
   (kit export, `/dev/design` demo in step 14), the Layout and skeleton changes, the `data-slot-inert` attributes. Component tests of 12.2.
6. Commit `feat(web): right-hand slot shared by side panels`.
7. **Approval card.** `approvalPlan.ts`, `approvalBits.tsx`, `ApprovalCard.tsx`, `StepApproveControl.tsx`, make `ActionCard` a wrapper. Run
   `npx vitest run src/features/approvals` **before** touching any test: `approvals.test.tsx` and the incident tests must pass unchanged. Then
   add `approvalCard.test.tsx` (12.3). Commit `feat(web): one structured approval card for steps and actions`.
8. **Pure logic.** `steps.ts`, `banner.ts`, `budget.ts`, `operationRows.ts` with their unit tests (12.4).
9. **List page.** `OperationsPage`, `OperationsTable`, route, `pages.ts`, nav item, `PageTitle`, tests of 12.5. Check `routes.test.tsx`,
   `layout.test.tsx` and `palette.test.tsx` still pass (update only what lists navigation items).
10. **Detail page.** header, controls and dialogs, banner, waiting section, outline, details, stale note; tests of 12.6 and 12.7 with the five fixtures.
11. Commit `feat(web): operations list and detail pages`.
12. **Inspector.** `StepInspector`, `InspectorSections`, URL selection effect, focus rules; tests of 12.8. Commit `feat(web): step inspector`.
13. **Incident link.** `IncidentOperations` and the incident page slot; test of 12.9.
14. **Catalogue.** `src/dev/OperationDemos.tsx` in `DesignPage` with: `SidePanel` (side and sheet), `ApprovalCard` (waiting, destructive, unknown risk,
    read-only, decided, refused, in flight, with extras, legacy v1), the operation status, outcome, execution and risk vocabularies come for free from
    `VOCABULARIES`; `operationDemos.test.tsx` (`skipAxe()` with a comment: a catalogue of fragments, like `newKitDemos.test.tsx`). Look at `/dev/design` in
    both themes.
15. **Smoke test, scripts, CI.** `scripts/smoke*`, `package.json`, `ci.yml`, `.gitignore`, `screens.mjs`. Build (`npm run build`), then
    `npm run smoke` locally. If Chromium is missing, `npx playwright install chromium`. Commit `test(web): browser smoke test for the operations pages`.
16. **Docs.** `DESIGN.md`, `TESTING.md`, `AGENTS.md`, `README.md`, the book section; `cargo xtask docs-links` (needs Rust; if you cannot run it, say so).
17. **Checks and completion report.**
    - `npm run format` then `npm run format:check`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run build` (this runs `size`; record the new
      numbers), `npm run smoke`, and `npm run gen:api` followed by `git diff --exit-code -- src/api` (must be clean).
    - `node scripts/screens.mjs` against the dev hub if you can start it (WSL; see `TESTING.md`) with `SCREENS_OPERATION` unset (list only) or set to a
      real id; otherwise say it was not run. Check by hand in the browser at 375, 768, 1280 and 1920 px, both themes, keyboard only.
    - Write the completion report in the standard structure.

## 8. Database and persistence

Not applicable: this prompt touches no database, no migration and no hub code. Browser storage is not used for state that
matters; the only storage is the existing host selection. Nothing the slot or the page remembers survives a reload except the
URL (`?step=`, `?status=`).

## 9. Protocol and compatibility

No wire change, no new hub route and no OpenAPI change (RW-42: wire 1.2 is frozen). The page tolerates a newer hub: an unknown
status, outcome, step kind, edge kind, execution state, pause type, wait kind or budget kind renders as a neutral pill or plain
text with the raw value and never throws; an unknown trace `schema_version` greater than 1 shows an `InlineMessage` warning
("This page was built for an older operation format. Some details may be missing.") above the steps and still renders what it can.
The hub may answer `409`, `403`, `404`, `503` for any command; every one is worded by `describeError`.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `GET /operations` fails | `LoadError` "load operations" with Retry; never the empty state |
| `GET /actions` fails, operations load | operations listed; `InlineMessage` tone warning "Could not load classic actions" above the table, with Retry |
| `GET /operations/{id}` is 404 | `EmptyState` "No such operation" with a link to `/operations` |
| `GET /operations/{id}` is 503 or fails | `LoadError` "load the operation" with Retry |
| A refetch fails while data is on screen | the page stays; `StaleNote` says so with Retry |
| The step detail request fails | `LoadError` "load the step" inside the inspector; the rest of the inspector stays |
| Approve answers 409 (plan changed, expired, already approved, another change landed first) | message inside the confirmation dialog in the hub's words; nothing else changes; the page refetches |
| Approve answers 403 (typed confirmation missing) | inside the dialog; cannot happen with the field filled in |
| Approve or Deny answers 503 (orchestration off or restarting) | inside the dialog (Approve) or as an error toast (Deny), in `describeError`'s words |
| The plan hash changes under an open dialog | confirm disabled with the reason of 6.7; the dialog stays open so the person can read it |
| Expiry passes while the card is open | Approve gets its disabled reason; Deny stays |
| Pause, cancel, resume, answer fail | pause and answer: error toast; cancel and raise-limit: inside the dialog; the banner stays as it was |
| Cancel or pause of an already cancelled or paused operation | the hub answers 200; the toast is the normal success one |
| The event stream drops | the shell's existing "Live updates paused" banner; nothing here duplicates it; data is not marked fresh |
| A frame arrives for this operation | `applyJournal` refetches `keys.operation(id)` unless the cached `as_of_seq` already covers the frame |
| `?step=` names a step not in the trace | the param is removed (`replace`); no inspector |
| The trace has a step whose parent is missing | it is a root of the tree |
| An execution step has no matching `ExecutionDto` | the row shows without the contract; the inspector shows no card and says "No action details are stored for this step." |
| A unknown value anywhere | neutral pill or raw text, never an exception |

## 11. Security requirements

- **No self-approval.** Approve and Deny exist only as clicks. No `useEffect`, polling, keyboard shortcut, URL parameter, banner action, SSE
  frame or test helper calls an approve or deny mutation. A source-scan test (12.10) proves `useApproveStep` / `useApprove` is imported only
  by `StepApproveControl.tsx` / `ApproveControl.tsx`.
- **Plan binding.** The step approval always sends `plan_hash` taken from the `ExecutionDto` the person was looking at when the dialog opened
  (6.7 rule 4). It never computes a hash and never sends one it did not display. The card shows the 8-hex prefix, the binding label ("v2") and the
  explanation `PLAN_HASH_HINT_V2`.
- **Fail closed on risk.** `confirm_required` from the hub decides the typed confirmation; a null risk shows as "Risk unknown" and is treated as
  destructive. The UI never lowers `confirm_required`; if the field is missing in a response (older hub), it is treated as `true`.
- **Typed confirmation** is exact (case and whitespace), as `ConfirmDialog.requireText` already enforces.
- **Model text** (`why`, step summaries, questions) is untrusted: rendered as React text (never `dangerouslySetInnerHTML`, no Markdown, no links
  built from it), labelled "Model's reason (not checked)", and visually secondary. Step output and intent are shown as text in a `CopyBlock` /
  `KeyValueList`.
- **No secrets in the page.** Output arrives redacted from the hub; the UI neither redacts nor stores it, never logs it, and never puts it in a
  URL. Cancel reason and answer text go only in request bodies. Drafts of answers live in component state only.
- **Authority is the hub's.** Every command is a hub route that re-checks state; the UI hides or disables controls only to explain, never as
  the check. Do not read tokens, cookies or CSRF values outside `src/api`.
- **No new network path.** No `fetch` outside `src/api`, no external origin, no `blob:` or inline script; the smoke test's server is local.
- **Right slot.** In sheet mode the page behind is `inert`; Escape never closes a dialog and a panel at once; focus returns to the opener.
- **Audit** is the hub's (`execution.approved`, `operation.cancelled`, …); the UI adds none.

## 12. Testing requirements

Name every test after the behaviour. Page and component tests use `renderAt` and `apiMock`; axe runs automatically over what a test leaves on
screen (do not call `skipAxe()` except where stated). No test sleeps: use `findBy…` and fake timers. For the right slot, tests render
`RightSlotProvider` + `RightSlotHost` around the component through a small helper `src/test/rightSlot.tsx` **(new)** that also stubs
`ResizeObserver` and lets a test set the measured width. For the five trace fixtures use `src/test/operationFixtures.ts` **(new)**: it imports
the JSON files by explicit path and builds `OperationDetail` payloads (`operation` summary from the trace, `executions` from `refs`: `state` from the
outcome, `awaiting_approval` for a pending execution of a `waiting_approval` operation, `plan_hash = "ab".repeat(32)`, `risk "write"`,
`confirm_required false`, `expires_at` in the future; overridable) and an `Action`-with-`legacy` helper. Expectations are derived from the fixture
data (`trace.operation.goal`, step counts), never copied strings.

### 12.1 Tokens, vocabularies (`src/styles/motion.test.ts`, `src/design/operationVocab.test.ts`)

| Test | Expected |
|---|---|
| `motion_tokens_never_exceed_600ms_and_reduced_motion_zeroes_every_one` | reads `tokens.css?raw`: every `--motion-*` duration in the root block is at most 600 ms; every one of them (including `--motion-panel`) is `0ms` in the reduced-motion block |
| `slot_constants_equal_the_tokens` | `SLOT_WIDTH_PX` equals `--right-slot-width` (25rem = 400) and `SLOT_CONTENT_MIN_PX` equals `--right-slot-content-min` (40rem = 640), parsed from `tokens.css?raw` |
| `operation_vocabularies_cover_every_value_the_contract_names` | all 11 statuses, 10 outcomes, 12 execution states, 11 kinds, 5 edge kinds have label, tone, hint and icon (the existing `tone.test.ts` also covers them through `VOCABULARIES`) |
| `every_status_kind_outcome_and_edge_in_the_fixtures_has_words` | reads the five fixtures: each `operation.status`, `pause_reason.type`, step `kind`, `outcome` and edge `kind` is a key of its table |
| `unknown_operation_values_are_neutral_with_the_raw_value` | each lookup returns tone `neutral` and the raw string as label for `"from_the_future"` |
| `unknown_risk_has_its_own_status` | `riskMeta("unknown")` is a warning with the hint of 6.5 |
| `operation_text_words_are_plain` | `waitText`, `budgetLabel`, `reverseEdgeLabel` return the strings of 6.6 and fall back to the raw value |

### 12.2 Right slot (`src/app/rightSlot/*.test.ts(x)`)

| Test | Setup, action | Expected |
|---|---|---|
| `slot_layout_is_closed_with_nothing_open` | `slotLayout({available: 2000, wide: true, open: []})` | `mode closed` |
| `slot_layout_puts_one_surface_beside_the_page_when_640px_remain` | 1040 and 1039, one open | `side` at 1040, `sheet` at 1039 |
| `slot_layout_shows_two_surfaces_only_when_640px_still_remain` | 1440 and 1439, two open | both visible at 1440; at 1439 newest only, `tabs` both |
| `slot_layout_is_a_sheet_when_even_one_does_not_fit_and_always_on_a_phone` | 900 two open; `wide: false` any width | `sheet`, tabs for two |
| `slot_state_open_moves_an_open_surface_to_the_front_and_ignores_unregistered_ids` | reducer | order and no-op as 6.3 |
| `slot_state_unregister_removes_the_surface_from_open` | reducer | gone |
| `opening_a_surface_renders_it_beside_the_page_and_moves_focus_in` | width 1300; button calls `open("a")` | `aside` named by the label, heading focused, children rendered; before opening, children are not mounted |
| `opening_with_focus_false_leaves_focus_alone` | `open("a", {focus:false})` | `document.activeElement` unchanged |
| `closing_returns_focus_to_the_opener_and_calls_onclose` | close button | opener focused, `onClose` called once |
| `unmounting_an_open_surface_removes_it_without_calling_onclose` | unmount owner | slot empty, `onClose` not called |
| `escape_closes_the_surface_with_focus_after_a_popover_has_taken_its_own_escape` | an `InfoTip` open inside the surface | first Escape closes the tip only; second closes the surface |
| `escape_with_focus_on_the_page_does_nothing` | focus a page button, Escape | still open |
| `a_second_surface_that_does_not_fit_collapses_the_first_to_a_tab` | width 1100, open a then b | only b rendered; a `radiogroup` "Side panels" with both; choosing a swaps them |
| `a_narrow_shell_shows_a_modal_sheet_and_makes_the_page_inert` | width 700 | `role dialog`, `aria-modal`, `[data-slot-inert]` elements have `inert`; closing removes it |
| `the_page_keeps_its_content_when_the_slot_is_open_side_by_side` | width 1300 | `main` and `aside` are siblings in `.body`; `main` has no `inert` |

### 12.3 Approval card (`src/features/approvals/approvalCard.test.tsx`)

| Test | Expected |
|---|---|
| `step_card_shows_skill_host_check_target_args_risk_expiry_and_the_v2_plan_prefix` | group "Action restart_container"; "on nas"; Tags "Check …" and "Target …"; `key = value` lines; risk pill; "Expires in"; `Plan abababab` and "v2" |
| `step_approve_asks_first_and_sends_the_plan_hash_it_showed` | click Approve: dialog with consequence, no request yet; confirm: `POST /api/v1/operations/{id}/steps/{step}/approve` with `{ plan_hash }` (no `confirm_text` key); success toast |
| `step_needing_confirmation_requires_the_skill_name_typed_exactly` | `confirm_required: true`: confirm disabled for "Restart_container", "restart_container " and "restart"; enabled for the exact name; body has `confirm_text` |
| `an_unknown_risk_step_is_treated_as_destructive` | `risk: null, confirm_required: true`: pill "Risk unknown", danger Approve, typed confirmation |
| `a_missing_confirm_required_field_is_treated_as_required` | `confirm_required` undefined in the payload: typed confirmation is asked for |
| `the_hash_sent_is_the_one_shown_when_the_dialog_opened` | open dialog; rerender with another `plan_hash`: confirm disabled with the plan-changed reason; no request goes out with either hash until the dialog is reopened |
| `approve_conflict_is_shown_inside_the_dialog_in_the_hubs_words` | route answers `failure(409, "conflict", "the plan changed since you opened it; reload the page and review it again")`: text inside the dialog, no error toast |
| `expired_step_disables_approve_with_a_reason_and_keeps_deny` | `expires_at` in the past |
| `deny_is_one_click_and_says_nothing_ran` | `POST …/deny`; toast "Denied restart_container. Nothing ran." |
| `a_decided_step_shows_who_approved_it_and_has_no_buttons` | state `succeeded`, `approved_by "user:1"` |
| `read_only_card_has_no_decision_buttons` | `readOnly` |
| `refused_not_run_and_unavailable_steps_explain_themselves` | each state shows its hint as the title and `reason` as the body |
| `in_flight_step_shows_progress` | `authorized` and `dispatched` |
| `model_text_is_secondary_and_labelled_and_rendered_as_text` | `why="<b>x</b>"` appears literally, preceded by "Model's reason (not checked): " |
| `extras_render_only_when_given` | with `effect`, `policy`, `refusal`: the tags, the policy line and the warning; without: none of them |
| `legacy_card_keeps_its_output_and_v1_label` | the existing `approvals.test.tsx` passes unchanged; plus one test: a v1 action shows "v1 binding" |
| `a_card_with_an_operation_link_links_to_the_operation` | `operation` prop |

### 12.4 Pure logic (`src/features/operations/*.test.ts`)

| Test | Expected |
|---|---|
| `merge_lists_operations_and_legacy_actions_newest_first` | interleaved by `updatedAt`; ties: operation first |
| `merge_hides_rows_older_than_the_oldest_loaded_row_of_a_source_with_more_pages` | gap rule of 6.9 |
| `merge_shows_everything_when_both_sources_are_complete` | no cut |
| `merge_marks_legacy_rows_and_points_them_at_their_incident` | `kind legacy`, `href /incidents/<id>` |
| `banner_priority_is_outcome_unknown_then_budget_then_error_then_manual_then_other_pause_then_waits_then_failed_escalated` | table-driven over every row of 6.11, including two states at once |
| `no_banner_for_queued_running_waiting_timer_succeeded_cancelled` | `null` |
| `banner_for_waiting_approval_counts_the_pending_actions` | n from `executions` |
| `steps_group_by_cycle_in_ordinal_order` | from the failed-verification fixture: cycle 0 then 1 |
| `tree_nests_children_under_parents_and_treats_missing_parents_as_roots` | |
| `window_keeps_open_unknown_and_selected_steps` | 300 generated steps, limit 200 |
| `connections_list_both_directions_with_edge_labels` | outgoing "Depends on", incoming "Needed by" |
| `plan_text_is_the_plan_step_linked_by_an_edge_or_the_latest_before_it_in_the_cycle` | three cases and `undefined` |
| `budget_units_convert_both_ways` | minutes <-> seconds; cost <-> micros; counts unchanged; `limitFor("cycles")` uses `operation.cycle` |
| `format_budget_says_no_limit_for_null` | |

### 12.5 Hooks and list (`src/api/hooks/operations.test.ts`, `src/features/operations/operationsList.test.tsx`)

| Test | Expected |
|---|---|
| `list_hook_sends_filters_and_the_cursor` | query `status`, `host`, `limit 50`, then `cursor` for page 2; empty values dropped |
| `each_mutation_sends_its_path_and_body` | approve (`plan_hash`, optional `confirm_text`), deny, cancel (`reason` or `{}`), pause (no body), resume (`extend`), answer (`text`) |
| `each_mutation_invalidates_operation_queries_overview_and_incident_details` | spy on `invalidateQueries` |
| `operations_list_shows_operations_and_legacy_actions_with_a_label` | rows, "Legacy" tag with its hint, newest first |
| `legacy_row_links_to_its_incident` | `href` |
| `status_chip_asks_the_hub_to_filter_and_is_kept_in_the_url` | chip `paused`: request has `status=paused`, no `GET /actions`; chip `waiting_approval`: `GET /actions` with `status=proposed`; `?status=` set |
| `list_follows_the_host_selector` | selected host `nas` goes as `host=nas` |
| `empty_list_says_what_operations_are_and_links_to_approvals` | |
| `a_failed_operations_load_is_a_load_error_with_retry_not_an_empty_state` | Retry refetches |
| `a_failed_classic_actions_load_still_lists_operations_and_says_so` | |
| `load_more_asks_for_the_next_page_of_each_source_that_has_one` | |
| `operations_is_in_the_navigation_between_incidents_and_approvals` | in `routes.test.tsx`/`layout.test.tsx` |

### 12.6 Detail page with the five fixtures (`src/features/operations/operationFixtures.test.tsx`, `describe.each` over the files)

For each fixture: `renders_<name>_without_violations` renders `/operations/<id>` and asserts (axe runs automatically): the `h1` is the goal; the
status pill label is `operationStatusMeta(status).label`; the number of step rows equals `trace.steps.length` (or the windowed count, none exceeds
200); every distinct `kind` label and every outcome label of the fixture appears; the matching banner of 6.11 (or none); Approve is offered iff an
execution is `awaiting_approval`. Fixture-specific:

| Fixture | Extra expectations |
|---|---|
| `waiting-approval` | banner "Waiting for your approval"; "Waiting for you" holds one `ApprovalCard` with an Approve button; the policy-decision and model-turn kinds render rows |
| `verified-success` | no banner; no Approve or Deny anywhere; the execution row shows its contract |
| `failed-verification-new-cycle` | two cycle lists ("Cycle 0", "Cycle 1"); a "Retry" tag on the retry execution; no banner; "Cycle 1" tag in the header |
| `outcome-unknown-paused` | banner "Result unknown: …" with Show the step, Resume and Cancel operation; the step's pill reads "Result unknown" |
| `question-and-budget-pause` | banner naming the exhausted limit with "Raise limit and resume"; the answered note and question rows; refused, unavailable, skipped and cancelled pills present |

Also `unknown_kind_outcome_and_edge_render_neutral_and_never_throw` (mutate a fixture copy: kind `"from_the_future"`, outcome `"odd"`, edge
`"links"`, status `"new_status"`, pause type `"mystery"`), `a_newer_trace_schema_shows_a_warning_and_still_renders`, and
`a_trace_with_300_steps_shows_the_newest_200_and_every_open_one`.

### 12.7 Detail behaviours and controls (`operationDetail.test.tsx`, `operationControls.test.tsx`)

| Test | Expected |
|---|---|
| `detail_names_the_page_and_follows_the_host_selector_note` | breadcrumb detail from `usePageName`; `AllHostsNote` when a host is selected |
| `no_such_operation_says_so_with_a_link_back` | 404 |
| `failed_load_is_a_load_error_with_retry` | 503 |
| `a_failed_refresh_keeps_the_page_and_says_so` | second fetch fails: content stays, `StaleNote` with Retry |
| `a_journal_frame_newer_than_the_page_refreshes_it` | real `applyJournal` with `{aggregate_type:"operation", aggregate_id, seq: as_of_seq + 1}`: second `GET` happens; the new status shows |
| `a_journal_frame_the_page_already_contains_changes_nothing` | `seq <= as_of_seq`: no second `GET` |
| `waiting_for_you_lists_each_pending_approval_once_and_the_inspector_card_is_read_only` | one Approve button on the page with the inspector open |
| `pause_button_posts_and_toasts_and_is_hidden_when_finished_or_paused` | |
| `pause_of_a_queued_operation_is_disabled_with_a_reason` | |
| `cancel_asks_first_names_the_operation_and_sends_the_reason` | dialog text, body `{reason}`; empty reason sends `{}` |
| `cancel_reason_over_500_characters_disables_confirm_with_a_reason` | |
| `outcome_unknown_resume_asks_first_and_posts_an_empty_extend` | body `{extend: []}` |
| `manual_pause_resumes_without_a_dialog` | |
| `raise_limit_posts_extend_for_the_exhausted_kind_in_api_units` | wall time in minutes -> seconds |
| `raise_limit_below_the_current_limit_is_a_field_error_and_sends_nothing` | |
| `a_budget_resume_conflict_shows_the_hubs_text_inside_the_dialog` | 409 |
| `answer_posts_the_text_clears_the_field_and_toasts` | body `{text}`; Send disabled when empty, over 2000, or paused (with reasons) |
| `review_button_moves_focus_to_waiting_for_you` | |
| `waiting_agent_banner_names_the_hosts` | |

### 12.8 Inspector (`stepInspector.test.tsx`)

| Test | Expected |
|---|---|
| `clicking_a_step_opens_the_inspector_and_puts_the_step_in_the_url` | `?step=<id>`, `aside` "Step inspector", focus inside, row has `aria-current` |
| `the_inspector_loads_the_step_and_shows_intent_and_bounded_output` | `GET …/steps/{step}` once; intent keys (no `kind`); output in a block labelled "Step output"; no output -> the retention sentence |
| `a_deep_link_opens_the_inspector_without_taking_focus` | initial URL with `?step=` |
| `an_unknown_step_in_the_url_is_dropped` | param removed, no inspector |
| `escape_closes_the_inspector_removes_the_param_and_returns_focus_to_the_row` | |
| `selecting_another_step_changes_content_without_moving_focus_and_announces_it` | status line |
| `connections_link_to_the_other_steps` | clicking selects it |
| `step_detail_failure_is_a_load_error_inside_the_inspector_only` | |
| `an_execution_step_shows_the_card_read_only_and_points_at_waiting_for_you_while_pending` | |

### 12.9 Incident link, catalogue, routes

`incident_page_lists_its_operations_and_nothing_when_there_are_none` (in `incidents` tests); `operation_demos_render_in_every_state` (`skipAxe()`
with a reason); `operations_routes_render_their_pages` and `nav_and_palette_list_operations` (in `routes.test.tsx` and `palette.test.tsx`);
`layout_renders_the_slot_host_beside_main_and_the_skeleton_has_the_same_shape` (in `layout.test.tsx`).

### 12.10 Gates and guardrails

`approve_mutations_are_only_used_by_the_approve_controls` in a new `src/features/approvals/gates.test.ts`: reads sources with
`import.meta.glob("../../**/*.{ts,tsx}", { query: "?raw", eager: true })` (excluding tests) and asserts `useApproveStep(` and `useApprove(` appear only in
their hook file and in `StepApproveControl.tsx` / `ApproveControl.tsx`. Existing guardrails (`guardrails.test.ts`, `cssGuard`, `contrast`, `voice`,
`tone`, `messages`) must pass unchanged; add no `eslint-disable` except where 6.4 requires none (no inline style is used: the slot's geometry is CSS only).

### 12.11 Smoke test

`npm run smoke` passes against the built bundle with all nine checks of 6.10; it is the only test that exercises a real layout, real focus, real
`ResizeObserver`, reduced motion and colour contrast.

## 13. Acceptance criteria

1. The shell has one right-hand slot: `slotLayout` follows 6.2 (tests), the host sits beside `<main>`, a sheet makes the page `inert`, and nothing
   covers usable content.
2. `useRightSlot`, `RightSlotSurface`, `RightSlotProvider`, `RightSlotHost`, `slotLayout` and `SidePanel` exist with the signatures of 6.1 to 6.4; a surface
   owner needs no code outside `RightSlotSurface` and a toggle to add a panel.
3. Opening a surface moves focus in (unless `focus: false`), Escape closes one thing per press, closing returns focus, unmounting never calls `onClose`.
4. Panel motion uses `--motion-panel` (240 ms, `transform` only) and is instant under `prefers-reduced-motion`; `motion.test.ts` guards 600 ms.
5. `/operations` lists operations and Legacy actions merged newest first, filters through the hub, and has loading, empty, partial-failure and
   error states; a failed load is never an empty state.
6. `/operations/:id` renders all five golden fixtures without axe violations; unknown values render neutral; the banner follows the priority table.
7. Waiting executions show `ApprovalCard`s under "Waiting for you"; the same card is read-only in the inspector; the page never has two Approve buttons
   for one step.
8. Approving a step sends the plan hash shown when the dialog opened; a destructive or unknown-risk step needs the skill name; a changed hash under an open
   dialog blocks confirm; every 409 reads in the hub's words inside the dialog.
9. `ActionCard` behaves exactly as before: `approvals.test.tsx` and the incident tests pass unchanged; the card shows "v1 binding" for legacy v1 and "v2" for steps.
10. Pause, cancel (with reason), resume (with and without a raised limit) and answer call the routes of prompt 29 with the bodies of 6.8 and report results
    through `useActionFeedback` / inline errors.
11. The page refreshes from the shell's journal stream, and only when a frame is newer than the page's `as_of_seq`.
12. The inspector opens from `?step=`, shows status, connections, intent and bounded output, and closes with Escape and the close button, clearing the URL.
13. The incident page lists its operations with links.
14. Statuses, outcomes, execution states, kinds and edge kinds come only from `tone.ts` lookups; no raw colour, no inline style, no native `title`, no raw
    `<button>` was added in `src/features` or `src/app`; every file is at most 250 lines; icons only through `Icon`.
15. `npm run format:check`, `lint`, `typecheck`, `test`, `build` (size within budget), `gen:api` (no diff) and `smoke` pass; the smoke test is a CI step.
16. `DESIGN.md`, `TESTING.md`, `AGENTS.md`, `README.md` and the book are updated (section 15); `/dev/design` shows the new components in both themes.
17. No Rust file, migration, wire contract or OpenAPI file changed.

## 14. Migration and compatibility

- **Existing installs.** With `[orchestration] enabled = false` (the default until prompt 58) the hub returns empty operation lists: the Operations page shows its
  empty state and the Approvals page, incident page and Chat behave as before. The new nav item appears for everyone; no setting hides it.
- **Legacy actions** keep their own card, hub routes and v1 binding; they are listed with a Legacy label and open the incident page.
- **Older hubs** (without the operation routes) answer 404 on `/operations`; the page shows a `LoadError`. The embedded UI and hub ship together, so this only affects
  a dev server pointed at an old hub.
- **Prompt 36 (next)** must: register the Operator with `RightSlotSurface id="operator"`, toggle it with `useRightSlot().toggle("operator")` from its top-bar
  button, use `--right-slot-width` instead of its own panel-width token, and not create `SidePanel` (it exists) or a dock. **Prompts 43, 49 and 51** render
  `ApprovalCard` with `source`, `host`, `why`, `extras`, `operation`.
- **Graph (38).** `StepOutline`, `steps.ts` and the inspector take a trace and a selected step id; the graph can select through the same `?step=` parameter and
  surface.

## 15. Documentation requirements

- **`web/DESIGN.md`** (same PR): (a) "The shell": add the right-hand slot (rules of 6.1 to 6.4, 640 px and 400 px, sheet, inert, Escape order so far) and the
  Operations navigation item. (b) "Components": add `SidePanel` and the feature-level `ApprovalCard` (props, extras, read-only, the destructive/unknown rule); add
  "operation status, step outcome, execution state, step kind, edge kind" to the status list with the "Add a status" note, and `Risk unknown`. (c) "How people use
  it", "Approve a fix": one paragraph on the step card, the plan-hash label and the typed confirmation; "Operations": the banner rule (one at a time) and the
  "Waiting for you" section. (d) "Layout patterns": the operation detail pattern (header, one banner, waiting, steps, details, inspector). (e) "Tokens": add
  `--right-slot-width`, `--right-slot-content-min`, `--motion-panel` to the table. (f) "Performance budget": record the new measured numbers from `npm run size`.
  (g) "Planned": remove what now exists (the right-hand slot sentence, the motion row for `--motion-panel`, the statuses `Refused`, `NotRun`, `Unavailable`,
  `OutcomeUnknown` and operation statuses, the delivered part of the approval card) and keep what does not (attention-centre entry, autonomy indicator, the other
  motion tokens, graph and virtual list, policy decision and unverified-human parts of the card, freshness states of `QueryBoundary`). (h) "Checks that will fail
  your PR": add the smoke test row (`npm run smoke`, fix: the page or the mock).
- **`TESTING.md`** (web paragraph near `:135`): `npm run smoke` (needs `npm run build`; uses Playwright's Chromium, `npx playwright install chromium`, or
  `SMOKE_CHANNEL=chrome`), that CI runs it in job `web`, and `SCREENS_OPERATION` for `screens.mjs`.
- **`AGENTS.md`**: add `npm run smoke` to the Web row of the commands table and to the line listing the web checks (committed file; keep in step).
- **`web/README.md`**: one line under "Check" for `npm run smoke`.
- **Book** `docs/book/src/concepts/operations.md` (26 creates it): a section "The Operations page" written for users (no prompt numbers): the list and the Legacy
  label, the detail page, the banners and what each control does (pause, cancel, resume, raise a limit, answer), approving a step and the typed confirmation, the
  plan code, the inspector. Link `web` DocsLinks only to pages that exist; run `cargo xtask docs-links`.
- No `ARCHITECTURE.md`, config reference, OpenAPI or ADR change (no ADR: no arch rule changes).

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
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
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

Also state in this report: the `confirm_text` rule you found in the approve route and the `skillName` you implemented; the exact
`ExecutionDto` / `OperationSummaryDto` field names that differed from section 3.3; the new `npm run size` numbers; and whether
`screens.mjs` and `cargo xtask docs-links` were run.
