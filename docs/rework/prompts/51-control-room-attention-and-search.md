# 51 — Control Room, attention centre and global search

| | |
|---|---|
| Branch | `control-room-attention-and-search` |
| Milestone | U4 |
| Depends on (must be merged) | 37 (visibility profiles), 30 (operations UI: `ApprovalCard`, `useOperations`, `useOperation`, the right slot, `npm run smoke`), 28 (journal stream, `as_of_seq`), 27 (operation read routes, `ApiDeps.operation_repo`), 35 (`GET /api/v1/experience/search` and `web/src/api/hooks/experience.ts`), and through them 26, 29 and 59. 34 (knowledge facts, `GET /api/v1/knowledge/facts`) is needed **only** for the facts group of the search (section 6.7 says what to do when it is absent). All of these are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"**. If 37, 30, 28, 27 or 35 is not merged, stop and report |
| Can run in parallel with | 43, 48, 49, 50 (order-flexible, never concurrent). 50 edits the same shell files (`nav.ts`, `pages.ts`, `routes.tsx`), `VISIBILITY_PAGES`, `DESIGN.md`, `scripts/smoke/*`, `scripts/screens.mjs` and `crates/domovoi-hub/src/api/mod.rs`; 48 may add fact hooks; whichever runs second rebases on the first |
| Produces (shared contracts) | (1) Six additive `OverviewDto` fields (`operations_running`, `operations_waiting_approval`, `operations_waiting_user`, `operations_paused`, `actions_outcome_unknown`, `attention_total`). (2) The web attention model `web/src/features/attention/` (`AttentionItem`, `attentionItems`, `useAttentionSources`, `AttentionRows`) that the shell count, the Control Room and the attention centre share. (3) The Control Room (the page at `/`), the attention centre `/attention`, the global search `/search`, their nav items, and the page keys `control-room`, `attention`, `search` in `VISIBILITY_PAGES` |
| Consumes | 30: `ApprovalCard`, `ActionCard`, `useOperations`, `useOperation`, `awaitingExecutions`, `planTextFor`, `operationStatusMeta`, `stepOutcomeMeta`, `executionStateMeta`, `OperationSummary`, `?status=` on `/operations`. 28: the shell's `applyJournal` invalidation and `as_of_seq`. 27: `operation_repo`, the list route. 35: `useExperienceSearch`, `ExperienceItemDto`. 34: the facts list route. 37: `Visible`, `VisibilityPage`, `visibilityAttrs`, `expectP0Everywhere`, the §6.5 rule. Existing: `useHosts`, `useIncidents`, `useActions`, `useNoteSearch`, `hostConnectionOf`, `HostCards`, the kit |
| Launch gate | none |
| Suggested model | medium: three pages over one shared model; the judgement is in never reading a failed source as "nothing needs you" and in keeping the decision controls single |

## 1. Objective

Answer, in one place each, the three questions the person who runs the servers asks:

- **"Is anything wrong, and what is Domovoi doing?"** The **Control Room** is the first page (route `/`, replacing the
  Overview page): four tiles, what needs the owner, what is running, the approvals waiting for a decision (the shared
  `ApprovalCard`, decided right there) and the hosts that need a look, with `Incompatible` hosts distinct from offline ones.
- **"What needs me, most urgent first?"** The **attention centre** (`/attention`) lists everything that waits for the owner in
  one ordered list with three urgency tiers: results that are unknown, approvals, questions, paused operations, incidents
  that need action and hosts that are unreachable. A shell entry ("Attention" in the navigation) carries the live count.
- **"Where did I see that?"** **Global search** (`/search?q=`) over hosts, incidents, active operations, past operations
  (experience), notes and, when the knowledge API exists, facts. It says which sources it covers and never reads a failed
  source as "nothing found".

Everything is live through the shell's journal stream (no page opens a stream of its own) and everything follows the rules of
the earlier UI prompts: one approval card, P0 content never hidden in any profile, a failed load never reads as empty.

The hub gains one small additive piece: six counts on `GET /api/v1/overview` so the shell can show the attention count on
every page with the one request it already makes. Everything else is web.

Known gap this prompt closes: the overview's `approvals_waiting` and the Approvals page list **classic** actions only, so a
step waiting for approval was reachable only through the Operations list, the operation page and the incident page. The
Control Room and the attention centre show both kinds together. The Approvals page and `approvals_waiting` stay as they are
(classic actions); that is reported, not changed.

There is no blocker. One interpretation the owner should know: "operations" in search means **active** operations (matched in
the browser against complete lists, because the hub has no text search over unfinished operations) plus **past** operations
(the hub's experience search); an unfinished operation is also reachable through its incident.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**RW-21 / D-08 (UI track).** Operations are the unit of work; incidents stay the detected problem. The Control Room is the
home of the owner's day: it must show **active operations, not only incidents and approvals**, and a host whose agent is
`Incompatible` must appear in the attention queue and in the host section as "upgrade required", **distinct from offline**
(D-20, persisted state, survives a hub restart). Clicking a metric opens the filtered list it counts. No counter is duplicated
across a tile, a heading and a filter on one page.

**RW-22 (one approval card).** `ApprovalCard` (built by prompt 30) renders every approval: operation detail (its canonical
home), incidents, the Control Room, the Operator and the attention centre. A page never shows two Approve buttons for one
step. The card's decision area is P0 and never reads the profile.

**D-02 (experience is evidence, never authorization).** Past operations in search come from the hub's experience search; every
result and the group heading say "historical evidence, not authorization". A past approval or success never pre-approves
anything and nothing here links a past result to an approve action.

**D-20 / RW-17 (hosts).** A host is `Known` (connected), `Unknown` (never connected), `Absent` (was connected, now gone) or
`Incompatible` (another protocol minor; also "expected incompatible" for an agent that has not reconnected since a hub
upgrade). The API already reports this in `HostDto` (`connected`, `agent_enrolled`, `protocol_state`, `incompatible_since`,
`last_seen`); the web maps it with `hostConnectionOf` to the `HOST_CONNECTIONS` vocabulary (`connected`, `offline`,
`never_connected`, `hub_only`, `incompatible`, `expected_incompatible`). Nothing new is needed on the hub for hosts. The hosts list
refreshes by polling (30 s); the `hosts` journal topic is reserved and emits nothing yet.

**RW-19 / prompt 29 (use-case layer).** Reads may stay in the transport that renders them. The overview counts are a read
(`api/overview.rs` already reads repositories directly); no command, engine handle or service is added.

**Live updates (prompt 28).** The shell's one stream (`useLiveEvents`, topics `incidents,approvals,operations`) calls
`applyJournal`, which invalidates `keys.incidentLists`, `keys.incident(id)`, `keys.actions`, `keys.operations` (a prefix that
covers every operation list and detail) and `keys.overview`, skipping a query whose cached data has `as_of_seq >=` the frame's
`seq`. Every query used here is under one of those keys and every operation or overview response carries `as_of_seq`, so the
pages are live with **no stream of their own**. Lists that are not paged snapshots (`hosts`, notes, experience) refresh by their
own polling or on demand.

**RW-21 / D-10 (profiles, prompt 37).** Every element of the three pages carries one category from 37's four questions. These pages
are mostly P0 by nature (what is happening now, what waits for the owner, failures, loading and error states); the tables of 6.8
give every element. Profiles never change what is requested.

**Decisions made here (do not revisit).**

1. **The Control Room replaces the Overview page at `/`.** The route, the lazy page entry `PAGES.overview` and the folder
   `web/src/features/overview/` keep their names (less churn); the page's title, content and the navigation label become "Control
   room". The component keeps the exported name `OverviewPage`.
2. **One attention model, three consumers.** `attentionItems(inputs, now)` (pure) turns the loaded sources into an ordered list with a
   tier and a rank (6.2). The attention centre shows all items; the Control Room shows the first five that are not approvals (the
   approval cards sit in their own section, so nothing is said twice); the shell count comes from the hub (`attention_total`), built
   from the same definitions so the number and the list agree (6.1).
3. **The attention centre embeds the approval card; the Control Room does too.** Approve and Deny live in the card (secondary emphasis
   in the Control Room where other actions share the screen, primary in the attention centre where the card is the page's purpose).
   Rows that are not approvals are links to where the decision is made (the operation, the incident, the host).
4. **Urgency is a fixed ranking, not a score** (6.2). Three tiers with headings "Act now", "Waiting for you", "Worth a look".
5. **A failed source is never "nothing needs you".** Each source (incidents, classic approvals, classic unknown results, operations
   waiting for approval, waiting for an answer, paused, hosts) can fail alone; the page lists what loaded, says which sources failed
   and offers Retry; the all-clear state appears only when every source loaded and none had an item.
6. **Operation lists are asked per status** (`useOperations({ status })`, the hub filters), never filtered from one big page in the
   browser. At most seven status queries exist and each is idle unless its page needs it (6.3).
7. **Search is composed in the browser from existing routes**; no backend search endpoint is added. Two groups (hosts, active
   operations) are matched in the browser because their source lists are **complete and small**; every other group is searched by
   the hub. If a result group needs a new endpoint, stop and report.
8. **Search covers its sources and says so** (a visible sentence). No permission filtering in v1 (single owner); the single place
   that decides which groups run is `SEARCH_GROUPS` (6.7), the hook point for future roles.
9. **The unit of attention is the thing that waits, not the incident behind it.** An open incident and an approval about it are two
   items, each with its own decision.

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, or **(introduced by NN — verify)**. Hub paths are relative to
`C:\Programming\domovoi`; web paths start with `web/`.

### 3.1 Hub: the overview today (existing)

- `crates/domovoi-hub/src/api/overview.rs`: `OverviewDto` (`:24`) with `incidents_action_needed`, `incidents_open`, `incident_groups`
  (`IncidentGroupsDto` `:41`: `needs_action`, `monitor`, `pending`, `expected`, `handled`), `approvals_waiting`, `hosts_offline`,
  `hosts_total`, all `u32`, "each count stops at 200" (`MAX_PAGE`). `is_offline(state, host)` (`:73`) is true when the host's agent is
  not connected **and** the host has a live (enrolled, not revoked) agent: that covers offline, incompatible and expected-incompatible
  hosts, and not a host that never connected or the hub's own machine. `show` (`:93`) takes `OverviewQuery { host }` (a host name;
  404 when unknown), lists active incidents and `proposed` actions with `Page { limit: MAX_PAGE }` and returns the counts.
  **Prompt 28 adds `as_of_seq`** read by `snapshot_seq(&state)` before the data **(introduced by 28 — verify)**.
- `ApiDeps` **(existing, `api/mod.rs:135`)**; **prompt 27 adds `operation_repo: Arc<dyn OperationRepo>`** (always the store; empty results
  while orchestration is off) **(introduced by 27 — verify)**. `OperationRepo::list(&OperationFilter { status, incident, host, kind },
  &Page)` newest first **(introduced by 26)**; `OperationStatus::NAMES`: `queued running waiting_approval waiting_user waiting_agent
  waiting_timer paused succeeded failed escalated cancelled`; a paused operation's reason is `PauseReason::{Budget, OutcomeUnknown,
  GlobalPause, Error}` **(introduced by 26)**. `ActionFilter { host, status: Option<String>, .. }` and the legacy status name
  `outcome_unknown` (`crates/domovoi-core/src/action/request.rs:112` `ActionStatus::NAMES`, 12 names).
- Tests: `crates/domovoi-hub/tests/hub/api_overview.rs` **(existing)**: `overview_counts_incidents_approvals_and_offline_hosts` (`:26`) and
  `overview_does_not_count_a_host_without_an_agent_as_offline` (`:95`) are the models; `api_harness.rs` `api()`, `a.admin()`,
  `a.get`, `a.propose(risk)`; `tests/hub/main.rs` registers test modules; `tests/hub/api_routes.rs:529` `openapi_snapshot`.

### 3.2 Web: the Overview page you replace (existing)

- `web/src/features/overview/OverviewPage.tsx` (140 lines): `OverviewPage()` (`:101`) = `PageHeader title="Overview" intro="What needs you
  and how your hosts are doing."`, the tiles, two columns (`AttentionList`, `ApprovalList`), `HostsSection`. `hasProblem(host)` (`:30`: not
  connected or incompatible, outdated, or open incidents), `worst(a, b)` (`:40`), `PROBLEM_HOSTS = 6` (`:27`), `HostsSection` (`:50`, a
  `Section title="Hosts"` with `HostCards`, or "All N hosts are fine", plus "View all N hosts").
- `OverviewTiles.tsx` (98 lines): `fleetOf(hosts)` (counts connected, offline, never-connected, skipping `hub_only`), the four `StatTile`s
  "Needs attention", "Open incidents", "Waiting for approval", "Hosts online". `OverviewLists.tsx` (153 lines): `AttentionList` (incidents,
  `group: "needs_action"`, severity then recency), `ApprovalList` (classic `proposed` actions, soonest expiry first), `PREVIEW_ROWS = 5`.
  `overview.module.css` (113 lines): `.tiles`, `.columns`, `.list`, `.row`, `.title`, `.host`, `.meta`, `.viewAll`: placement only; reuse the
  pattern in the new feature's own module.
- `overview.test.tsx` (261 lines) renders `<OverviewPage/>` with `renderAt`, `route("GET /api/v1/hosts" ...)`, fixtures `nas`, `incident`,
  `action`, `overview()` from `web/src/test/fixtures.ts`. Tests that stay valid because the **Hosts section keeps its words and cards**:
  `hosts_are_cards_that_link_to_the_host`, `hosts_shows_only_hosts_with_problems_six_at_most_and_a_link_to_all`,
  `a_connected_host_with_open_incidents_or_an_old_agent_is_a_problem`, `when_every_host_is_fine_one_muted_line_says_so`. Tests that assert the
  old tiles, lists and title change (6.9 lists them).
- The shell: `web/src/app/nav.ts` `NAV_GROUPS` (`:36`; "Respond": Overview `/`, Incidents, Approvals, Chat; prompt 30 inserts Operations between
  Incidents and Approvals; prompt 36 may replace Chat), `NavCount { field, label, tone }` where `field` is any numeric key of `Overview`;
  `SidebarNav.tsx:47` reads `useOverview(host)` (polled every 30 s, refreshed by `applyJournal`). `AppPalette.tsx` lists every nav item under
  "Pages" and has an "Actions" group; the palette filters locally and does not expose the typed text. Tests that name the link "Overview":
  `web/src/app/layout.test.tsx:102,107,116` and `routes.test.tsx:96-99` (the document title `Overview · Domovoi`).
- Data hooks (existing): `useOverview(host)` (`api/hooks/overview.ts`), `useHosts()` and `useHostNames()` (`api/hooks/hosts.ts`, poll 30 s),
  `useIncidents(filters, enabled)` (infinite; `IncidentFilters { status, host, severity, group, q }`; `INCIDENT_PAGE = 50`),
  `useActions({ status, host, decided, since })` (limit 200), `useNoteSearch(q, host)` (idle while `q` is blank; up to 20 hits),
  `keys` in `api/queryClient.ts`. `GET /api/v1/incidents?q=` is a case-insensitive "contains" over title, check and target; the incident list
  page keeps `?q=` in its URL (`features/incidents/IncidentListPage.tsx:109`). `web/src/features/hosts/HostCards.tsx` `HostCards({ hosts })`
  sorts the cards itself (`DEFAULT_HOST_SORT`) and shows each host's connection pill (`hostConnectionMeta`).
- Approval pieces: `web/src/features/approvals/ActionCard.tsx` (`ActionCard({ action, host, why, showIncident, incidentTitle, approve })`,
  `EXPIRY_WARNING_MS = 15 min`, moved by prompt 30 to `approvalBits.tsx` and re-exported) and **(introduced by 30 — verify)** `ApprovalCard
  ({ source: { kind: "step", operationId, execution } | { kind: "legacy", action }, host, why, extras, emphasis, readOnly, operation, incident })`,
  `ApprovalPlan`. Design vocabularies **(introduced by 30)**: `operationStatusMeta`, `stepOutcomeMeta` (`outcome_unknown` is "Result unknown"),
  `executionStateMeta` (`awaiting_approval`), all through `design/tone.ts`; existing `severityMeta`, `actionStatusMeta`, `hostConnectionMeta`,
  `riskMeta`.
- Operation hooks **(introduced by 30 — verify names and shapes)**: `useOperations(filters: { status?, incident?, host?, kind? }, enabled = true)`
  (infinite query keyed `keys.operationList(filters)`, `GET /api/v1/operations` with `status`, `host`, `cursor`, `limit` 50;
  `data.pages[0].items: OperationSummary[]`, `OperationSummary { id, kind, goal, status, pause_reason: { type, exhausted?, step?, message? } |
  null, waiting_for, cycle, incidents, hosts, created_at, started_at, ended_at, updated_at }`), `useOperation(id)` (`OperationDetail { as_of_seq,
  operation, trace, executions }`, key `keys.operation(id)`), `steps.ts` `awaitingExecutions(executions)` and `planTextFor(trace, stepId)`.
- Experience **(introduced by 35 — verify)**: `GET /api/v1/experience/search?q=&scope=global|host&host=&limit=` (q at most 200 characters; limit
  1 to 50, default 10) answering `ExperienceSearchDto { scope, host, query, items: ExperienceItemDto[], as_of_seq, label }`;
  `ExperienceItemDto { operation: PastOperationDto { operation_id, goal, ended_at, outcome, verified, hosts: { id, name | null }[], contracts,
  narrative }, match_pass, snippet | null }`; `label` is always "historical evidence, not authorization". Hook `useExperienceSearch({ q, scope:
  "global" } | undefined)` in `web/src/api/hooks/experience.ts`.
- Knowledge **(introduced by 34 — verify; may be absent)**: `GET /api/v1/knowledge/facts?q=&limit=` answering `FactPageDto { items:
  FactDto[], next_cursor }` where `FactDto { id, subject, predicate, value /* any JSON */, source, status, version, observed_at, fresh_until, ... }`;
  with `q` the list is a search (live facts only, at most 50, no cursor). Prompt 48 builds the pages and hooks later; this prompt only searches.
- Test and smoke infrastructure: `web/src/test/{apiMock,render,fixtures,setup}.ts*` (`route`, `calls`, `failure`, `renderAt`; an unregistered route
  throws "no test route for ..." which fails only that query), `web/src/test/operationFixtures.ts` and `rightSlot.tsx` **(introduced by 30)**,
  `web/src/test/visibility.tsx` **(introduced by 37)**; `web/scripts/smoke/{server,data,checks}.mjs` **(introduced by 30; 37 and 50 extend)** whose mock
  hub fails the run on any unlisted `/api` request; `web/scripts/screens.mjs` `PAGES` list (it has no entry for `/`).

## 4. Exact scope

**(new)** means the file does not exist yet. Every web source file stays at or under 250 lines and every non-test Rust file at or under 500.

### 4.1 Hub (only the overview counts)

| File | Change |
|---|---|
| `crates/domovoi-hub/src/api/overview.rs` | six new `u32` fields on `OverviewDto` and their counting (6.1); if the file would pass 500 lines move the counting into `api/overview_counts.rs` **(new)** |
| `crates/domovoi-hub/tests/hub/api_overview.rs` | the tests of 12.1 |
| `crates/domovoi-hub/tests/hub/snapshots/hub__api_routes__openapi.snap` | regenerated and **read** with `cargo insta review` (six properties on `OverviewDto`, nothing else) |
| `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | **regenerated**, never hand-edited (`cargo xtask openapi && cd web && npm run gen:api`) |

No new route, no `ApiDeps` field (27 added `operation_repo`), no `Cargo.toml`, migration, wire or config change, no other crate. `cargo xtask arch-check` is not
needed unless you added a hub file (then run it).

### 4.2 Web: the shared attention model (`web/src/features/attention/`, all **(new)**)

`attentionModel.ts` (types, `TIERS`, `RANKS`, `attentionItems`, `hostAttention`), `useAttentionSources.ts` (the seven source queries and `summarizeSources`),
`AttentionRows.tsx` (the link rows), `AttentionRow.tsx`, `ApprovalGroup.tsx` (step and classic approval cards with their loading states),
`AttentionPage.tsx`, `attention.module.css` (placement only, tokens only), the tests of section 12.

### 4.3 Web: the Control Room (`web/src/features/overview/`)

| File | Change |
|---|---|
| `OverviewPage.tsx` | becomes the Control Room page: title "Control room", `VisibilityPage page="control-room"`, sections of 6.4; `hasProblem`, `PROBLEM_HOSTS` stay exported; `worst` becomes `hostSeverity` (6.4) |
| `OverviewTiles.tsx` | the four tiles of 6.4 (`fleetOf` stays exported and unchanged) |
| `OverviewLists.tsx` | **removed**: its two lists are replaced by `AttentionRows` and `ApprovalGroup`; `PREVIEW_ROWS` moves to the attention feature |
| `RunningNow.tsx` **(new)** | the "Running now" section (running operations) |
| `overview.module.css` | keep the classes still used, drop the unused ones |
| `overview.test.tsx` | updated as 12.3 says |

### 4.4 Web: global search (`web/src/features/search/`, all **(new)**)

`SearchPage.tsx`, `useSearchGroups.ts` (the hook of 6.7 and `SEARCH_GROUPS`), `matching.ts` (pure word matching), `ResultGroup.tsx`, `groups/`
(`IncidentsGroup.tsx`, `ActiveOperationsGroup.tsx`, `PastOperationsGroup.tsx`, `NotesGroup.tsx`, `FactsGroup.tsx`, `HostsGroup.tsx`), `search.module.css`,
tests. Hooks: `web/src/api/hooks/factSearch.ts` **(new; only if prompt 48 has not already merged an equivalent: reuse it then)**.

### 4.5 Web: shell, data layer, design, wiring

| File | Change |
|---|---|
| `web/src/app/nav.ts` | "Respond" group: item 1 label "Control room" (`to: "/"`, icon `dashboard`, `preload: PAGES.overview.preload`), item 2 `{ to: "/attention", label: "Attention", icon: "alert-circle", count: { field: "attention_total", tone: "danger", label: "need you" }, preload: PAGES.attention.preload }`; "Knowledge" group: item 1 `{ to: "/search", label: "Search", icon: "search", preload: PAGES.search.preload }`; other items unchanged and in their order |
| `web/src/app/pages.ts`, `routes.tsx` | lazy pages `attention` and `search`; routes `/attention` and `/search` inside `<Layout/>` |
| `web/src/app/AppPalette.tsx` | one "Actions" item "Search everything" (icon `search`, keywords `["find", "look up"]`) that navigates to `/search` |
| `web/src/api/queryClient.ts` | `keys.factSearch(q)` under its own root; nothing else (operation and experience keys come from 30 and 35) |
| `web/src/api/hooks/operations.ts` (30) | only if missing: export `operationOptions(id)` (`{ queryKey, queryFn }`) used by `useOperation` and by `ApprovalGroup` (6.5) |
| `web/src/design/visibility.ts` | `VISIBILITY_PAGES` gains `control-room` ("Control room"), `attention` ("Attention centre"), `search` ("Search") |
| `web/src/test/fixtures.ts` | `overview()` gains the six fields (default 0); factories for operation summaries if 30's helper lacks them |
| `web/src/dev/AttentionDemos.tsx` **(new)**, `DesignPage.tsx`, `web/src/dev/attentionDemos.test.tsx` **(new)** | `/dev/design` demos (attention rows of every kind, tiers, the partial-failure message, a search group in each state) |
| `web/scripts/smoke/{server,data,checks}.mjs`, `web/scripts/screens.mjs` | 6.10 |
| `web/DESIGN.md`, `docs/book/src/guide/control-room.md` **(new)**, `SUMMARY.md`, `reference/api.md`, `ARCHITECTURE.md` | section 15 |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Changing the Approvals page, the legacy approve routes or `approvals_waiting` (it stays "classic actions waiting") | later, with the legacy executor's removal |
| A hub endpoint for an attention feed, an active-operations filter, a text search over operations, or a combined search route | not needed; if one seems needed, stop and report (decisions 6 and 7) |
| Snoozing, dismissing or acknowledging an attention item; "mark as seen" | an item leaves the list when the thing it names is decided or resolved; nothing else clears it |
| Customisable Control Room widgets, drag and drop, saved layouts | deferred (06 "Deferred": demand unvalidated) |
| The autonomy-state indicator and the pause switch | later (autonomy) |
| The graph, the Operator panel, the policy, knowledge and capability pages | 38, 36, 43, 48, 49 (the facts group only searches; it links nowhere until a facts page exists) |
| Root-agent, skill-runner and helper warnings on hosts | the host-page prompts; the host section here shows only the connection state |
| Live host availability over the stream (the `hosts` topic) | reserved by the journal stream; hosts keep polling every 30 s |
| Per-profile screenshots in `screens.mjs` | the smoke test and the vitest matrix cover the profile product |
| Persisting search history, recent searches or the last query | none; the query lives in the URL |
| Highlighting or ranking beyond each source's own order | each group keeps its source's order |
| Any change to prompt 30's cards, banners or approval rules; any use of the profile in approval, banner or API code | out of scope; 37's gate scans still pass |
| A migration, config key, wire change or new dependency | none needed |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly; tests and later prompts call it) or **convention** (follow repository style). Every public Rust item gets a doc
comment saying what it is for; every kit or web helper gets a JSDoc. Never write a prompt number, "the build plan" or an RW or D id into code,
comments, the book, commit messages or the PR description.

### 6.1 `OverviewDto` additions **fixed** (names, meaning), counting **convention**

Six new `u32` fields on `OverviewDto` (doc comments say what each counts and, like the others, "stops at 200"). All honour the optional `host` query
exactly as the existing counts do (`only` is that host's id):

| Field | Counts |
|---|---|
| `operations_running` | operations whose status is `running` (`OperationRepo::list(&OperationFilter { status: Some("running"), host: only, .. }, &Page { cursor: None, limit: MAX_PAGE })`, then `items.len()`) |
| `operations_waiting_approval` | status `waiting_approval` (operations, not steps: one operation may hold several steps) |
| `operations_waiting_user` | status `waiting_user` |
| `operations_paused` | status `paused`, whatever the reason (including result unknown) |
| `actions_outcome_unknown` | classic actions with status `outcome_unknown` (`ActionFilter { host: only, status: Some("outcome_unknown"), .. }`) |
| `attention_total` | `incident_groups.needs_action + approvals_waiting + operations_waiting_approval + operations_waiting_user + operations_paused + actions_outcome_unknown + hosts_offline`, with `saturating_add` |

`attention_total` uses `incident_groups.needs_action` (the group the incident list filters on, which is what the attention centre lists), **not**
`incidents_action_needed` (which also counts open incidents triage only wants watched). `hosts_offline` is the existing count and equals the number of
host rows of the attention centre (a host that is not connected and has a live agent: offline, incompatible or expected incompatible). Read the counts
**after** `snapshot_seq` (28), like the existing ones; the value of `as_of_seq` is unchanged. With orchestration off `operation_repo` returns nothing, so
the four operation counts are 0 and the rest is as before. No existing field changes. Example addition to the JSON: `"operations_running": 1,
"operations_waiting_approval": 2, "operations_waiting_user": 0, "operations_paused": 1, "actions_outcome_unknown": 0, "attention_total": 6`.

### 6.2 The attention model **fixed** (`web/src/features/attention/attentionModel.ts`, pure, no React, no I/O)

```ts
export type AttentionKind = "result_unknown" | "approval" | "question" | "paused" | "incident" | "host";
export type AttentionSource = "operation" | "action" | "incident" | "host";
export type Tier = 0 | 1 | 2;
export const TIER_TITLES: Readonly<Record<Tier, string>> = { 0: "Act now", 1: "Waiting for you", 2: "Worth a look" };

export interface AttentionItem {
  key: string;                  // "operation:<id>" | "action:<id>" | "incident:<id>" | "host:<id>"
  kind: AttentionKind;
  source: AttentionSource;
  tier: Tier;
  rank: number;                 // the table below, 0 first
  title: string;                // operation goal; "<skill> on <host name or 'a host'>" for a classic action; incident title; host name
  meta: StatusMeta;             // the pill: from design/tone.ts lookups only
  hostIds: string[];            // empty for a host item (its own id is in `key`)
  href: string;                 // where the decision is made
  since: string | null;         // RFC 3339, for ordering and the time shown
  expiresAt: string | null;     // classic approvals only
  why: string;                  // one plain sentence of what to do, from attentionText.ts ("" when the title says it all)
}

export interface AttentionInputs {
  incidents: readonly IncidentSummary[];      // group needs_action
  proposed: readonly Action[];                // classic actions waiting for approval
  unknownActions: readonly Action[];          // classic actions with status outcome_unknown
  waitingApproval: readonly OperationSummary[];
  waitingUser: readonly OperationSummary[];
  paused: readonly OperationSummary[];
  hosts: readonly Host[];
  hostNames: ReadonlyMap<string, string>;     // id to name
}
export function attentionItems(inputs: AttentionInputs): AttentionItem[];   // ordered: tier, then rank, then the within-rank order below
export function hostAttention(host: Host): "incompatible" | "offline" | "expected_incompatible" | undefined;
```

`hostAttention` calls the existing `hostConnectionOf(host)`: `incompatible`, `offline` and `expected_incompatible` map to themselves; `connected`,
`hub_only` and `never_connected` give `undefined` (a host that never connected, or the hub's own machine, is not an attention item). **The ranking is
this table, fixed** (the pill is the vocabulary entry named; a pause reason of an unknown type is the "paused, other" row):

| rank | tier | kind and source | when | pill (lookup) | `href` |
|---|---|---|---|---|---|
| 0 | 0 | `result_unknown`, operation | `paused` with `pause_reason.type === "outcome_unknown"` | `stepOutcomeMeta("outcome_unknown")` | `/operations/{id}` |
| 1 | 0 | `result_unknown`, action | classic status `outcome_unknown` | `actionStatusMeta("outcome_unknown")` | `/incidents/{incident_id}` |
| 2 | 0 | `host` | `hostAttention` is `incompatible` | `hostConnectionMeta("incompatible")` | `/hosts/{name}` |
| 3 | 0 | `incident` | severity `critical` | `severityMeta` | `/incidents/{id}` |
| 4 | 0 | `host` | `offline` | `hostConnectionMeta("offline")` | `/hosts/{name}` |
| 5 | 1 | `approval`, operation then action | operation `waiting_approval`; classic `proposed` | `operationStatusMeta("waiting_approval")` or `actionStatusMeta("proposed")` | `/operations/{id}` or `/approvals` |
| 6 | 1 | `question` | operation `waiting_user` | `operationStatusMeta("waiting_user")` | `/operations/{id}` |
| 7 | 1 | `paused` | `paused` and not outcome unknown (budget, error, manual, global pause, unknown) | `operationStatusMeta("paused")` | `/operations/{id}` |
| 8 | 1 | `host` | `expected_incompatible` | `hostConnectionMeta("expected_incompatible")` | `/hosts/{name}` |
| 9 | 2 | `incident` | severity `warning` | `severityMeta` | `/incidents/{id}` |
| 10 | 2 | `incident` | any other severity (`info`, unknown) | `severityMeta` | `/incidents/{id}` |

Order **within a rank** (fixed): rank 5 lists operations first (oldest `since` first: the one that has waited longest) and then classic actions by
`expiresAt` ascending; every other rank by `since` descending (most recent first), `null` last; remaining ties by `key`. An incident that needs
action and an approval about it are **two items**. A host item's `since` is `incompatible_since`, else `last_seen`, else `null`. An operation item's
`since` is `updated_at`; a classic action's `created_at`; an incident's `last_firing_at`. `why` comes from `design/attentionText.ts` (new):

| Item | `why` |
|---|---|
| result unknown, operation | "It may have run on the host. Check the host before continuing." |
| result unknown, action | `actionStatusMeta("outcome_unknown").hint` (the existing words) |
| approval | "" (the card says it) |
| question | "Domovoi asked a question and continues once you answer." |
| paused: `budget` | "A limit is spent. Raise it to continue, or cancel it." |
| paused: `error` | "It paused after an error." |
| paused: `manual` | "It was paused by a person." |
| paused: anything else | "It is paused." |
| host | `hostConnectionMeta(connection).hint` (the existing words) |
| incident | "" |

No new status vocabulary is created: every pill is an existing lookup. The model never calls the network and never reads the clock.

### 6.3 Sources hook **fixed** (names, behaviour), layout **convention** (`useAttentionSources.ts`)

```ts
export type SourceName = "incidents" | "approvals" | "unknown results" | "operations waiting for approval"
                       | "operations waiting for an answer" | "paused operations" | "hosts";
export interface SourceFailure { name: SourceName; error: unknown; retry: () => void }
export interface AttentionState {
  items: AttentionItem[];            // from every source that loaded; empty until something loaded
  pending: boolean;                  // at least one source has never answered
  failed: SourceFailure[];           // sources whose latest answer is an error and that have no data
  truncated: SourceName[];           // sources with more pages than the first
  allLoaded: boolean;                // every source answered successfully
  inputs: AttentionInputs;           // what the items were built from (for the approval group)
}
export function useAttentionSources(host: string | undefined): AttentionState;
export function summarizeSources(parts: SourcesParts): AttentionState;      // pure core of the hook, unit-tested
```
The hook calls, explicitly and unconditionally (never in a loop; the lint rule forbids it): `useIncidents({ host, group: "needs_action" })`,
`useActions({ status: "proposed", host })`, `useActions({ status: "outcome_unknown", host })`, `useOperations({ status: "waiting_approval", host })`,
`useOperations({ status: "waiting_user", host })`, `useOperations({ status: "paused", host })` and `useHosts()`; `hostNames` from the host list; for a
chosen `host` only that host's row is considered. It reads **only the first page** of each infinite query (`data.pages[0]`) and puts the source in
`truncated` when `hasNextPage` is true. A source "failed" when `isError` and there is no data (a failed background refresh keeps the old data and is
not a failure here). `retry` refetches that query. Nothing in the hook or the model reads the profile.

### 6.4 The Control Room **fixed** (behaviour), **convention** (markup)

`OverviewPage` (still the default export of the lazy page): `PageHeader title="Control room" intro="What is running, what needs you and how your hosts are
doing." docs="guide/control-room.html"`, then `<VisibilityPage page="control-room">` around:

1. **Tiles** (`OverviewTiles`, `<Visible category="P0" id="control.tiles">`), a `Grid min="xs" gap={3} fill` of four `StatTile`s fed by `useOverview(host)` and
   `useHosts()` through the existing combined `QueryBoundary` (`context="load the control room"`, skeleton of four blocks):

   | Label | Value | Tone | Hint | `to` |
   |---|---|---|---|---|
   | Needs you | `attention_total` | `danger` when > 0, else `success` | ">0": "Open the attention centre"; 0: "All clear" | `/attention` |
   | Running now | `operations_running` | `progress` when > 0, else none | ">0": "Operations in progress"; 0: "Nothing running" | `/operations?status=running` |
   | Waiting for approval | `approvals_waiting + operations_waiting_approval` | `pending` when > 0, else none | ">0": "Nothing runs until you decide"; 0: "Nothing to decide" | `/attention` |
   | Hosts online | `"{online} of {total}"` (existing `fleetOf`) | existing | existing `fleetHint` | `/hosts` |

2. **Two columns** (stacked under 1024 px, as today): left `Section title="Needs you"` with `actions` a `Link` "Attention centre" to `/attention`, content
   `<Visible category="P0" id="control.attention">`: from `useAttentionSources(host)` the first **5** items whose `kind` is not `approval`
   (`AttentionRows`, the same rows the attention centre uses). States: `pending` with no items → `Skeleton shape="rows" count={3}`; `failed` non-empty →
   an `InlineMessage tone="warning" title="Could not load {names joined with ', ' and 'and'}"` with a Retry `Button` calling every `retry`, **above** whatever
   loaded; `allLoaded` and no such item → `EmptyState icon="check-circle"` titled "Nothing needs you" (titled "Nothing else needs you" when approvals
   exist) with the body "No result is unknown, nothing is paused and no host or incident needs action."; never the empty state while anything failed or
   is pending. Right `Section title="Running now"` with `actions` a `Link` "All operations" to `/operations`, content `<Visible category="P0"
   id="control.running">` = `RunningNow`: `useOperations({ status: "running", host })`, the first 5 rows (`StatusPill sm` of `operationStatusMeta`,
   goal as the link to `/operations/{id}`, host names, `RelativeTime` of `updated_at`), `QueryBoundary context="load running operations"`, empty
   `EmptyState icon="play" title="Nothing is running"` ("Operations appear here while Domovoi investigates or fixes something.").
3. **Waiting for approval**: `Section title="Waiting for approval"` with `<Visible category="P0" id="control.approvals">` around `ApprovalGroup` (6.5) with `limit=3`,
   `emphasis="secondary"`, and, when more items exist, the line `Link` "{n} more waiting" to `/attention`. Empty (all sources loaded, no approval):
   `EmptyState icon="approve" title="Nothing to approve"` ("Actions Domovoi proposes show up here.").
4. **Hosts**: the existing `HostsSection` inside `<Visible category="P0" id="control.hosts">`, with its "View all N hosts" `Link` wrapped in
   `<Visible category="P1" id="control.hosts-link">`. `hasProblem` is unchanged. `worst` becomes `hostSeverity(a, b)` (the selection of the six that are
   shown; `HostCards` still sorts the cards it gets): rank `incompatible` 0, `offline` 1, `expected_incompatible` 2, `never_connected` 3, outdated or only
   open incidents 4 (from `hostConnectionOf`), then more open incidents first. An `incompatible` host is therefore always among the six and is never labelled
   offline (its card shows the existing "Incompatible" pill).

The page follows the host selector (hosts filtered as today, every query gets `host`). It opens no stream and polls nothing itself: the shell's
`applyJournal` refreshes incidents, actions, operations and the overview; the hosts query polls every 30 s (`HOSTS_REFRESH_MS`).

### 6.5 Attention centre and the approval group **fixed**

`AttentionPage` (`/attention`, lazy `PAGES.attention`): `PageHeader title="Attention" intro="What waits for you, most urgent first." docs="guide/control-room.html"`;
`<VisibilityPage page="attention">`; it follows the host selector (no `AllHostsNote`). Content, inside `<Visible category="P0" id="attention.list">`:

- `failed` non-empty: `InlineMessage tone="warning"` as in 6.4 (what loaded is still listed). `truncated` non-empty: a muted line "More items exist than are
  listed here. Open Incidents or Operations for the rest." `pending` and nothing loaded: `Skeleton shape="rows" count={5}`. `allLoaded` with no item:
  `EmptyState icon="check-circle" title="Nothing needs you"` (body of 6.4). Never the empty state while a source failed or is pending.
- For tier 0, 1, 2 in order, only tiers that have an item: `Section title={TIER_TITLES[tier]}` (no counts in headings). Tier 1 shows, first, the approvals as cards
  (`ApprovalGroup`, `emphasis="primary"`, `ATTENTION_CARDS = 10` with a `Button` "Show {n} more" that reveals ten more), then its other rows. Every other item is
  a row (`AttentionRows`): one `<ol>` per tier; each `<li>` carries `visibilityAttrs("P0", "attention.item")` and is **one link** (the whole row): `StatusPill sm`
  (`meta`), the title, the host name(s) ("a host" when unknown; "{name} and {n} more" for several), `RelativeTime` of `since`, and under the title the muted `why`
  when it is not empty. Rows have a pointer cursor and a hover state through the same CSS pattern as `overview.module.css`.

`ApprovalGroup({ operations, actions, hostNames, limit?, emphasis })` (`ApprovalGroup.tsx`) renders, in this order: one `StepApprovals` per operation (the
hub's order), then one classic card per action by `expires_at` ascending; `limit` counts items (an operation is one item, an action is one item) and the
caller shows the "more" line. `StepApprovals({ operation, emphasis })`: `useOperation(operation.id)` (30), then `QueryBoundary`-like handling:
loading → `Skeleton shape="block"`; failure → `InlineMessage tone="warning" title="Could not load {goal}"` with Retry (never silently skipped); loaded →
for each `awaitingExecutions(detail.executions)` one `ApprovalCard source={{ kind: "step", operationId, execution }} host={hostNames.get(execution.host_id)}
why={planTextFor(detail.trace, execution.step)} emphasis={emphasis} operation={{ id: operation.id, goal: operation.goal }}`; loaded with none awaiting → nothing
(the next refresh drops the operation from the list). A classic action renders `ActionCard action={action} host={hostNames.get(action.host_id)} showIncident`.
`ApprovalGroup` imports **no** approve or deny hook and contains no mutation: the decision is made inside the cards (30's gate `approve_mutations_are_only_used_by_the_approve_controls`
must keep passing). When `useOperation` is called here and prompt 30's file does not export an options factory, add `operationOptions(id)` to
`web/src/api/hooks/operations.ts` and make `useOperation` use it (a refactor with no behaviour change; 30's tests pass unchanged); `StepApprovals` still calls
the hook, so cache key and data shape are 30's.

### 6.6 Navigation, palette and the shell count **fixed**

`NAV_GROUPS` as in 4.5. The Attention entry's count is `attention_total` with tone `danger` and label "need you" (read by assistive technology as "6 need you");
it shows only while above zero, like the other counts. `AppPalette` gains the Action "Search everything" and lists the new pages automatically. The document
title of `/` becomes "Control room · Domovoi" through the `PageHeader` title (update the tests that expected "Overview").

### 6.7 Global search **fixed** (behaviour), **convention** (markup)

`/search?q=<text>` (`PAGES.search`). `PageHeader title="Search" intro="Find incidents, operations, notes and facts across every host."`; `AllHostsNote`
("Search covers every host."); `<VisibilityPage page="search">`; a `<div role="search">` with `<Visible category="P1" id="search.box">` around `TextInput
type="search" aria-label="Search" maxLength={200} placeholder="Search incidents, operations, notes and facts"` whose value is state initialised from `?q=`;
after `GLOBAL_SEARCH_DEBOUNCE_MS = 300` (kit `useDebounced`) the trimmed text is written to `?q=` (`replace`) and drives the queries; emptying the box clears
`?q=` at once. **Nothing is requested for fewer than 2 characters**; the page then shows the muted line "Type at least 2 characters." (no group, no empty state).

```ts
// web/src/features/search/useSearchGroups.ts
export interface SearchGroupSpec { id: "incidents" | "operations" | "past" | "notes" | "facts" | "hosts"; label: string; matched: "hub" | "browser" }
export const SEARCH_GROUPS: readonly SearchGroupSpec[];   // in this order; the ONE place that decides which groups exist and run (the hook for future roles)
//   incidents "Incidents" hub; operations "Active operations" browser; past "Past operations" hub; notes "Notes" hub; facts "Facts" hub; hosts "Hosts" browser
export type GroupState<T> = { status: "idle" | "pending" | "error" | "ready"; items: T[]; total: number; error?: unknown; retry: () => void; note?: string };
export function useSearchGroups(q: string): { incidents; operations; past; notes; facts?; hosts: GroupState<...> };   // q already trimmed; < 2 characters => every group "idle"
export function scopeSentence(groups: readonly SearchGroupSpec[]): string;
```
The hook calls explicitly: `useIncidents({ q }, enabled)`; seven `useOperations({ status })` for `queued`, `running`, `waiting_approval`, `waiting_user`,
`waiting_agent`, `waiting_timer`, `paused` (each `enabled` only while a query is active); `useExperienceSearch(enabled ? { q, scope: "global" } : undefined)`;
`useNoteSearch(enabled ? q : "", undefined)`; `useFactSearch(enabled ? q : "")` (below); `useHosts()`. Groups:

| Group | Source and matching | Row |
|---|---|---|
| Incidents | hub: `GET /incidents?q=`, first page (50); "Open in Incidents" `Link` to `/incidents?q=<q>` | severity pill, title with `Highlight`, host name, `RelativeTime` of `last_firing_at`; link `/incidents/{id}` |
| Active operations | **browser**: the first page (50) of each of the seven status lists, concatenated; an operation matches when **every word** of `q` occurs, ignoring case, in its `goal` or in one of its host names (`matching.ts`, pure). `note` = "Only the newest 50 operations in each state were searched." when any list `hasNextPage` | `operationStatusMeta` pill, goal with `Highlight`, hosts, `RelativeTime` of `updated_at`; link `/operations/{id}` |
| Past operations | hub: `useExperienceSearch` (`scope: "global"`); the group heading is followed by the response's `label` text, always visible | goal with `Highlight` as the link `/operations/{operation_id}`, `operationStatusMeta(outcome)` pill, a `Tag` "Verified" when `verified`, host names, `RelativeTime` of `ended_at`, the `snippet` (text) when present |
| Notes | hub: `useNoteSearch` (up to 20 hits) | title with `Highlight` as the link `/notes/{id}`, the `snippet` as text |
| Facts | hub: `GET /api/v1/knowledge/facts?q=<q>&limit=10`, only if the route exists in the generated schema; the row has **no link** (no facts page exists yet) | subject, predicate, the value as text (a string as is; anything else compact JSON cut at 120 characters), a `Tag` with `source`, and the muted text "May be out of date" when `status === "stale"`; never a status pill |
| Hosts | **browser**: `useHosts()` is the complete list; a host matches when every word occurs in its name | name with `Highlight` as the link `/hosts/{name}`, `hostConnectionMeta` pill |

If `/api/v1/knowledge/facts` is **not** in `web/src/api/schema.d.ts` (prompt 34 not merged), do not create `FactsGroup`, `useFactSearch` or the registry entry; the
search then covers five groups and says so; record it in the completion report. `useFactSearch(q)` (new, `api/hooks/factSearch.ts`, key `keys.factSearch(q)`):
`GET /api/v1/knowledge/facts` with `{ q, limit: 10 }`, idle while `q` is empty; reuse an equivalent hook if prompt 48 already added one.

`ResultGroup({ group, title, state, children })`: a `Section` (region named by the title). `pending` → `Skeleton shape="rows" count={2}`; `error` → compact `LoadError`
(`context="search {lowercase label}"`, Retry); `ready` with 0 → muted "No {lowercase label} match."; `ready` with items → the first 5 rows as a list and, when `total >
5`, `Button variant="ghost"` "Show all {total}" that reveals the rest of the **already loaded** items (it never asks the hub for more); the group's `note` as muted text.

Overall (`<Visible category="P0" id="search.status">`): after every group settled — all `ready` with 0 results → `EmptyState icon="search" title="Nothing found"` ("Nothing
matches “{q}” in the sources above."); any `error` → `InlineMessage tone="warning" title="Some sources could not be searched"` ("Results from the others are shown.");
never "Nothing found" while any group is `error` or `pending`. Coverage (`<Visible category="P0" id="search.scope">`): the muted sentence built by `scopeSentence(SEARCH_GROUPS)`:
"Searches incidents, active operations, past operations, notes, facts and hosts. Active operations and hosts are matched here against complete lists; the rest are searched by the hub."
(the facts word follows the registry). All result text is rendered as React text; `Highlight` marks words with `<mark>` and never interprets markup; no link is built from
model-written or note text except the ids the hub sent.

### 6.8 Visibility categories of the three pages **fixed** (37's rule, applied; the executor does not change them)

| id | page | element | category | mark |
|---|---|---|---|---|
| (none) | all | `PageHeader` title and intro, section headings: strings given to the kit | P1 by definition | none |
| `control.tiles` | control-room | the four tiles and their load state | P0 | V |
| `control.attention` | control-room | "Needs you" rows, partial-failure message, empty and loading states | P0 | V |
| `control.running` | control-room | "Running now" rows and states | P0 | V |
| `control.approvals` | control-room | the approval cards, "n more waiting", the empty state | P0 | V |
| `control.hosts` | control-room | the hosts section and its states | P0 | V |
| `control.hosts-link` | control-room | "View all N hosts" | P1 | V |
| `attention.list` | attention | tiers, rows, approval cards, partial and truncated messages, empty and loading states | P0 | V |
| `attention.item` | attention | each row's `li` (inside `attention.list`) | P0 | A (`visibilityAttrs`) |
| `search.box` | search | the search field | P1 | V |
| `search.scope` | search | the "searches ..." sentence | P0 | V |
| `search.status` | search | "Nothing found" and "Some sources could not be searched" | P0 | V |
| `search.incidents`, `search.operations`, `search.past`, `search.notes`, `search.facts`, `search.hosts` | search | one block per result group (rows, "Show all", its load error, its note) | P1 | V |
| `search.history-label` | search | the "historical evidence, not authorization" text under "Past operations" | P0 | V |

A P0 element is never inside a P2 or P3 `Visible` (there is none on these pages). The pages are P0 by nature; profiles change nothing visible here today, but every id is
marked so a later table change cannot hide one (the P0 tests of 12.4 prove it). `ApprovalCard` and `ActionCard` carry their own categories from 37.

### 6.9 Existing tests that change **fixed** (and only these)

`web/src/features/overview/overview.test.tsx`: the page title and intro (`Control room`); the tile tests (`tiles_show_what_needs_the_person_and_lead_to_their_lists`,
`tiles_say_all_clear_when_nothing_is_wrong`; `hosts_online_counts_connected_agents_and_says_who_never_connected` keeps its assertions, only the helper's title changes);
`overview_says_nothing_needs_you_when_both_lists_are_empty`, `needs_attention_lists_the_first_five_incidents_as_links`,
`needs_attention_puts_the_most_severe_first_then_the_most_recent`, `waiting_for_approval_lists_actions_with_skill_host_risk_and_a_link`,
`a_long_host_name_in_a_waiting_row_is_cut_and_keeps_its_tooltip` and `a_failed_load_is_an_error_with_retry_and_never_nothing_needs_you` are **rewritten for the new sections**
(same intent, new elements; section 12.3 names the replacements) and `overview_follows_the_host_selector` also checks the operation queries; the four hosts-section tests stay
**unchanged**. `web/src/app/layout.test.tsx` (`:102`, `:107`, `:116`: the link is "Control room"; the Respond list gains "Attention") and `routes.test.tsx` (`:96-99`: heading and document
title "Control room"). `web/src/test/fixtures.ts` `overview()` gains the six fields. Any other test that asserts an exact navigation list gains the new items. No other assertion changes.

### 6.10 Smoke test additions **fixed** (what they check)

Prompt 30's mock hub fails the run on any unlisted `/api` request. Extend it: `GET /api/v1/overview` with the six new fields (non-zero: one operation running, one waiting for
approval, one classic action, one incompatible host); `GET /api/v1/incidents` (honours `group` and `q`); `GET /api/v1/operations` honours `status` (a different small page per
status; empty for the rest); `GET /api/v1/actions` honours `status` (`proposed`, `outcome_unknown`); `GET /api/v1/hosts` includes one `incompatible` host; `GET /api/v1/experience/search`;
`GET /api/v1/notes/search`; `GET /api/v1/knowledge/facts` (only if the route exists). New checks after the earlier ones (continue the numbering): **Control Room** (open `/`: heading "Control room"; tiles
"Needs you", "Running now", "Waiting for approval", "Hosts online"; a running operation row; an approval card with `Plan xxxxxxxx` and an enabled Approve; the incompatible host's card says "Incompatible"
and not "Offline"; clicking the "Running now" tile lands on `/operations?status=running`), **Attention** (`/attention`: the headings "Act now" and "Waiting for you" in that order; the result-unknown row before
the approval card; the sidebar link "Attention" shows the count; the mock pushes a journal frame and the list updates without a reload), **Search** (`/search?q=plex`: the six group headings, the history label under
"Past operations", the scope sentence, "Show all" reveals more; `?q=a` shows "Type at least 2 characters."), then at 375 px no horizontal scroll on the three pages, axe in the real browser (including colour
contrast) on the three pages in both colour schemes and each profile, no `console.error`, no unexpected request.

## 7. Ordered implementation steps

Work on branch `control-room-attention-and-search`, cut from the latest `overhaul-investigation`. Commit in small green steps (one-line Conventional Commits, exactly one
scope each: `feat(hub)`, `feat(web)`, `test(web)`, `docs`), after steps 3, 5, 7, 9, 10 and 11. Run the scoped checks of each area after every commit.

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Confirm 37, 30, 28, 27 and 35 are merged: `web/src/design/visibility.ts`, kit `Visible`, `web/src/features/approvals/ApprovalCard.tsx`, `web/src/api/hooks/operations.ts`
     (`useOperations`, `useOperation`), `web/src/features/operations/steps.ts` (`awaitingExecutions`, `planTextFor`), `web/src/api/journalFrames.ts`, `web/src/api/hooks/experience.ts`,
     `web/scripts/smoke/server.mjs`, `crates/domovoi-hub/src/api/snapshot.rs`, `ApiDeps.operation_repo`. **If any is missing, stop and report.** Check `web/src/api/schema.d.ts` for the path
     `/api/v1/knowledge/facts`: its presence decides whether the facts group exists (6.7). Write down every real name that differs from section 3 (hook signatures, `OperationSummary` field
     names, whether `OperationSummary.hosts` holds ids, the shape of 30's `ApprovalCard` props, the smoke mock's route table, whether `operationOptions` exists).
   - Open every file of section 3 and confirm the line numbers. Read `crates/domovoi-hub/src/api/overview.rs` and `tests/hub/api_overview.rs` completely, `web/src/features/overview/*` completely,
     `web/src/app/{nav,AppPalette,SidebarNav,routes,pages}.ts*`, `features/hosts/HostCards.tsx`, `design/{tone,fleetVocab}.ts`, `src/test/{apiMock,render,fixtures,setup}.ts*`.
   - Read `web/DESIGN.md` completely (rules, "Overview page" layout pattern, the "Planned" section as prompts 30 and 37 left it) and prompt 37's classification table so ids match.
   - `grep -rn "Overview" web/src --include=*.test.tsx` and keep the list of tests that name the page or the nav link ("Overview"); `wc -l` on `OverviewPage.tsx`, `nav.ts`, `tone.ts`.
   - Run once for the baseline: `cargo nextest run -p domovoi-hub` and, in `web/`, `npm test`. Note the counts.
2. **Hub counts.** Add the six fields and their counting (6.1), the tests of 12.1. `cargo xtask openapi && cd web && npm run gen:api`, `cargo insta review` (the diff must be exactly six properties on `OverviewDto`),
   `cargo fmt --all`, `cargo clippy -p domovoi-hub --all-targets -- -D warnings`, `cargo nextest run -p domovoi-hub`. If you added a file, `cargo xtask arch-check`.
3. Commit `feat(hub): attention counts on the overview`.
4. **The attention model.** `attentionModel.ts`, `attentionText.ts` (in `web/src/design/`), their unit tests (12.2), then `useAttentionSources.ts` with `summarizeSources` and its tests, `fixtures.ts`
   (`overview()` and any factory). `npx vitest run src/features/attention src/design`; `npm run lint && npm run typecheck`.
5. **Rows, approval group, attention page, route, nav item.** `AttentionRow(s)`, `ApprovalGroup`, `AttentionPage`, `pages.ts`, `routes.tsx`, `nav.ts` (both new items), `VISIBILITY_PAGES`, the palette action, tests of 12.3.
   Commit `feat(web): attention centre`.
6. **Control Room.** Run `npx vitest run src/features/overview src/app` **before** editing any test (record what fails after your page change: only the tests of 6.9 may fail). Rewrite `OverviewPage`, `OverviewTiles`,
   `RunningNow`, delete `OverviewLists.tsx`, `hostSeverity`; update the tests of 6.9; add the new ones (12.3). Fix `layout.test.tsx` and `routes.test.tsx` only where 6.9 says.
7. Commit `feat(web): control room`.
8. **Search.** `matching.ts`, `useSearchGroups.ts`, `useFactSearch` (if the route exists), the groups, `SearchPage`, route and nav item; tests of 12.3.
9. Commit `feat(web): global search`.
10. **Product tests and catalogue.** The matrix and `p0_visible_in_every_profile` cases (12.4), the source-gate id list of 37 (add this prompt's P0 ids to `every_p0_id_of_the_table_appears_in_the_sources`; extend, never
    weaken), `AttentionDemos` and `/dev/design`. Look at `/dev/design`, `/`, `/attention` and `/search?q=nas` in both themes. Commit `test(web): control room, attention and search across detail levels`.
11. **Smoke test and screens.** `scripts/smoke/*` (6.10), `scripts/screens.mjs` entries `["control-room", "/"]`, `["attention", "/attention"]`, `["search", "/search?q=nas"]`; `npm run build` then `npm run smoke`.
    Commit `test(web): browser smoke test for the control room, attention centre and search`.
12. **Docs.** Section 15; `cargo xtask docs-links`. Commit `docs: control room, attention centre and search`.
13. **Checks and completion report.**
    - Hub: `cargo fmt --all`; `cargo clippy -p domovoi-hub --all-targets -- -D warnings`; `cargo nextest run -p domovoi-hub`.
    - Web: `npm run format` then `npm run format:check`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run build` (record the new `npm run size` numbers; if a limit is exceeded stop and report: the three pages are
      lazy except the Control Room, which was already lazy), `npm run smoke`, `npm run gen:api` then `git diff --exit-code -- src/api`.
    - By hand at 375, 768, 1280 and 1920 px, both themes, keyboard only, in all three profiles on the three pages, with a waiting approval, a result-unknown operation and an incompatible host present (start the dev hub
      in WSL as `TESTING.md` says; if you cannot, say so). `node scripts/screens.mjs` if the dev hub runs.
    - Write the completion report in the standard structure.

## 8. Database and persistence

**No migration, no schema change, no new table or column.** The six counts are reads through ports that exist (`OperationRepo::list`, `ActionRepo::list`, the host and incident reads the handler already makes);
`ls crates/domovoi-store/migrations/` must be identical before and after. **Transactions:** none; each count is one read. **N-1 upgrade test:** not applicable (no schema change); the existing upgrade tests must
still pass. Browser storage is not used for state that matters: the search query lives in the URL (`?q=`), the host choice in the existing host-selector storage, nothing else is persisted.

## 9. Protocol and compatibility

No agent protocol change (wire stays 1.2, RW-42), no new route, no change to any existing response field, status code, audit kind, journal kind or SSE topic. The only API change is six **additive** properties on
`OverviewDto`; a client that ignores them is unaffected, and the web type marks them required, so read them defensively (`overview.attention_total ?? 0`) because a dev server may point at an older hub.

Inside the pair web and hub: an older hub answers the overview without the new fields, so the Attention count is absent (no badge) and the "Needs you" tile shows 0 with the wrong "All clear" hint; to avoid that
lie the tile reads "Needs you" with hint "Open the attention centre" and **no tone** when `attention_total` is `undefined`. A hub without orchestration answers the operation lists with empty pages (the Control Room shows its
empty states). A hub without the experience or knowledge routes answers 404: that group shows its compact `LoadError`, never "no matches". A newer hub that sends an unknown status, pause reason type, severity,
protocol state or outcome renders a neutral pill with the raw value and the "paused, other" row; nothing throws.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| One attention source fails (for example the paused-operations list) | the rows of the other sources are listed; an `InlineMessage` warning names the failed source and offers Retry; the empty state and the all-clear tile hint are not shown |
| Every source fails | the same message with no rows; `LoadError`-style Retry; never "Nothing needs you" |
| A source is still loading | skeleton rows if nothing loaded yet, else the loaded rows (the list grows as sources answer; ordering is recomputed by the pure model); the empty state waits for `allLoaded` |
| A background refresh fails while data is on screen | the data stays; the shell's own "Live updates paused" banner covers a dropped stream; the failed source is not reported (it has data) |
| A source has more than one page | only the first page is read; the page says "More items exist than are listed here. Open Incidents or Operations for the rest." |
| `GET /operations/{id}` fails for an operation waiting for approval | an `InlineMessage` "Could not load {goal}" with Retry in that card's place; the other cards render |
| An operation is listed as waiting for approval but its detail has no awaiting execution (it changed meanwhile) | nothing renders for it; the next refresh removes it from the list |
| An approval is decided in a card | the card's own mutation feedback (30); the lists refetch through the existing invalidation (`keys.operations`, `keys.actions`, `keys.overview`) and the card leaves |
| An approval expires while shown | the card shows its expiry state and a disabled Approve with a reason (30); the hub moves it out on its own |
| A frame newer than a list's `as_of_seq` arrives | the shell's `applyJournal` refetches that list; nothing here listens to the stream |
| The host selector changes | every query takes the new `host`; the overview counts follow (`useOverview(host)`) |
| The overview answer lacks the new fields (older hub) | no Attention badge; the "Needs you" tile has no tone and the hint "Open the attention centre" |
| Orchestration is off | operation lists are empty; "Running now" says "Nothing is running"; classic approvals and incidents still list |
| A host's state is unknown to this build | neutral pill with the raw value; the host is not an attention item (`hostAttention` returns `undefined` for states it does not know) |
| Search text shorter than 2 characters | no request is made; "Type at least 2 characters." |
| One search group fails | only that group shows its compact `LoadError` with Retry; the others show their results; the overall message says some sources could not be searched; "Nothing found" is not shown |
| Every group is ready with no match | one `EmptyState` "Nothing found" |
| The experience or facts route answers 404 (older hub) | that group's `LoadError`; the page still works |
| Search text contains regex characters or markup | matched literally by `Highlight` and `matching.ts`; shown as text |
| `?q=` is longer than 200 characters | cut to 200 in the field and in every request (the hub refuses a longer experience query) |
| A result group has more than five items | five shown, "Show all {n}" reveals the loaded rest; no extra request |
| A result links to something the person may not open | not applicable in v1 (single owner); the single filter point is `SEARCH_GROUPS` |

## 11. Security requirements

- **No self-approval, no second approval path.** Approve and Deny exist only inside `ApprovalCard` and `ActionCard` as clicks (30). `ApprovalGroup`, `AttentionRows`, the search groups, the tiles and the
  model contain no approve or deny hook, no effect that calls a mutation and no keyboard shortcut for either. The source gate of 30 (`approve_mutations_are_only_used_by_the_approve_controls`) must keep
  passing and a new gate (12.4) proves no file of `features/attention` or `features/search` imports `useApprove`, `useApproveStep`, `useDeny` or `useDenyStep`.
- **One decision control per step.** The Control Room and the attention centre never show two Approve buttons for one step; the Control Room shows approvals only in its approval section and its "Needs you" rows
  exclude them. Rows link to the place of decision; they never decide.
- **Past is not permission.** Search results from experience carry the hub's "historical evidence, not authorization" label (P0) and link only to the operation page; no result offers approve, retry or re-run.
- **Fail visible.** Every error path ends in showing more, not less: a failed source is named, never read as empty; an unknown value is a neutral pill; the profile cannot hide any of it (all P0).
- **Display is not authority.** No hook under `src/api/` and no file of the three features imports the visibility primitives except to mark elements (`Visible`, `visibilityAttrs`, `VisibilityPage`);
  nothing decides a request or a decision from the profile. The hub never reads the preference.
- **Untrusted text.** Goals, incident titles, note titles and snippets, fact values, experience snippets and model-written `why` text are rendered as React text only: no `dangerouslySetInnerHTML`, no Markdown, no link
  built from text (only ids the hub sent), no `title` attribute. Search words are matched and highlighted literally.
- **No new network path.** No `fetch` outside `src/api`; no external origin; the search query goes only to the hub through existing typed routes, in the query string of `GET` requests (it is the user's own search
  text, never a secret; it is not logged by the web). The URL holds `?q=` and nothing else sensitive.
- **Authority is the hub's.** The hub routes re-check everything; the pages hide or disable only to explain. The overview counts are reads behind `Authed` and take only the existing `host` filter.
- **Bounded work.** Every list reads one page (50 or 200 items); the approval cards fetch at most one operation detail per listed operation and at most `ATTENTION_CARDS` (10) at a time on the attention centre and 3 on
  the Control Room; search issues no request below 2 characters; the seven status queries of search run only while a query is active.

## 12. Testing requirements

Name every test after the behaviour. No test sleeps: hub tests use `FakeClock`; web tests use `findBy…` and fake timers (the search debounce is advanced with fake timers). Web page and component tests use `renderAt` (or
`renderWithProfile`, introduced by 37) and `apiMock`; **axe runs automatically** over whatever a UI test leaves on screen and only `attentionDemos.test.tsx` calls `skipAxe()` (a catalogue of fragments, with a comment saying
so). Fixtures for operations are prompt 30's (`src/test/operationFixtures.ts`, the five trace fixtures); expectations are derived from the fixture data with a small helper written in the test, never by calling the function
under test.

### 12.1 Hub (`crates/domovoi-hub/tests/hub/api_overview.rs`)

Harness: `api()`, `a.admin()`, `a.get`, `a.propose`; operations are created through the store's unit of work the way prompt 27's hub tests do (use their helper; do not build a second way).

| Test | Setup, action | Expected |
|---|---|---|
| `overview_counts_operations_by_state` | create operations in `running`, `waiting_approval` (two), `waiting_user`, `paused` (one for a budget, one result unknown) and `succeeded`; `GET /api/v1/overview` | `operations_running 1`, `operations_waiting_approval 2`, `operations_waiting_user 1`, `operations_paused 2`; the finished one is counted nowhere |
| `overview_counts_classic_actions_with_an_unknown_result` | one classic action moved to `outcome_unknown`, one `proposed` | `actions_outcome_unknown 1`, `approvals_waiting 1` |
| `attention_total_is_the_sum_of_what_the_attention_centre_lists` | the two tests' data plus an incident in group `needs_action`, one open incident the triage marked `monitor` and an offline host | `attention_total` equals `incident_groups.needs_action + approvals_waiting + operations_waiting_approval + operations_waiting_user + operations_paused + actions_outcome_unknown + hosts_offline`; the `monitor` incident is not in it |
| `attention_counts_follow_the_host_filter` | data on `nas` and `media`; `?host=nas` | every new count (and the total) covers only `nas`; `?host=nope` is still 404 |
| `operation_counts_are_zero_when_orchestration_is_off` | the default harness (no operations) | the four operation counts are 0; the other counts are unchanged from the existing tests |
| `new_overview_fields_are_in_the_openapi_document` | the generated document | `OverviewDto` lists the six properties as required integers; the existing `openapi_snapshot` is regenerated and reviewed |

The existing two tests of this file must pass unchanged.

### 12.2 Web: the model, the sources, matching (`src/features/attention/*.test.ts`, `src/features/search/matching.test.ts`)

| Test | Expected |
|---|---|
| `ranking_follows_the_fixed_table` | one input of every row of 6.2 (shuffled): the output order is tier, then rank 0 to 10; tiers are 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2 |
| `a_result_unknown_operation_and_a_result_unknown_action_come_before_everything_else` | both first, operation before action |
| `an_incompatible_host_is_its_own_row_ahead_of_an_offline_one_and_never_reads_as_offline` | `hostConnectionMeta("incompatible")` label "Incompatible" for it and "Offline" for the other; `expected_incompatible` is rank 8, tier 1 |
| `a_host_that_never_connected_or_is_the_hub_itself_is_not_an_attention_item` | `hostAttention` returns `undefined` for `never_connected` and `hub_only` and `connected` |
| `critical_incidents_are_act_now_and_warning_and_info_are_worth_a_look` | tiers 0, 2, 2; an unknown severity is rank 10 |
| `approvals_list_operations_first_oldest_waiting_first_then_classic_actions_by_expiry` | the within-rank order of rank 5 |
| `other_ranks_order_by_recency_with_unknown_times_last_and_keys_as_the_tie_break` | |
| `an_incident_and_an_approval_about_it_are_two_items` | two keys |
| `paused_operations_say_why_in_plain_words` | `budget`, `error`, `manual`, `global_pause` and an unknown type give the sentences of 6.2 (the unknown type is rank 7, "It is paused.") |
| `every_pill_comes_from_a_design_lookup_and_unknown_values_are_neutral` | for each row the `meta` equals the lookup of the table; a made-up pause type and severity give neutral metas |
| `summarize_reports_failed_sources_by_name_and_keeps_the_rest` | one source in error with no data: `failed` has it with its `retry`; `items` come from the others; `allLoaded` false |
| `summarize_does_not_report_a_failed_refresh_that_still_has_data` | error with data: not in `failed` |
| `summarize_marks_sources_with_more_pages_as_truncated` | `hasNextPage` true |
| `summarize_is_all_loaded_only_when_every_source_answered` | |
| `matching_needs_every_word_ignores_case_and_treats_characters_literally` | `matchesAll("Restart PLEX on nas", ["plex", "nas"])`, `"a.b"` does not match `"axb"` |

### 12.3 Web: pages (`src/features/attention/attention.test.tsx`, `src/features/overview/overview.test.tsx`, `src/features/search/search.test.tsx`)

Attention centre:

| Test | Expected |
|---|---|
| `attention_lists_tiers_in_order_with_their_headings` | h1 "Attention"; headings "Act now", "Waiting for you", "Worth a look" in document order; an empty tier has no heading; no counts in headings |
| `a_result_unknown_operation_is_a_link_row_with_its_why_and_goes_to_the_operation` | pill "Result unknown", the sentence, `href /operations/{id}` |
| `approvals_are_shared_cards_with_primary_approve_and_an_operation_link` | one `ApprovalCard` per awaiting execution of a waiting operation (`why` from `planTextFor`), a classic action as `ActionCard`; each Approve is the card's primary |
| `an_approval_that_cannot_load_says_so_and_keeps_the_other_cards` | `failure(500)` for one `GET /operations/{id}`: "Could not load {goal}" with Retry |
| `approval_cards_beyond_ten_are_revealed_by_show_more` | 12 operations: ten cards, "Show 2 more" |
| `a_failed_source_is_named_and_the_rest_is_listed_never_nothing_needs_you` | paused list `failure(500)`: warning "Could not load paused operations" with Retry; other rows present; no "Nothing needs you" |
| `retry_refetches_every_failed_source` | click Retry: the failed requests repeat |
| `all_clear_shows_nothing_needs_you_only_when_every_source_loaded` | every route empty: `EmptyState` "Nothing needs you" |
| `more_than_one_page_says_so` | `next_cursor` on the incidents page |
| `the_attention_page_follows_the_host_selector` | `?host=nas`: every request has `host=nas` |
| `a_journal_frame_newer_than_the_page_refreshes_it` | real `applyJournal` with `{ aggregate_type: "operation", seq: as_of_seq + 1 }`: the operations lists refetch and the new row shows; `seq <= as_of_seq` changes nothing |
| `attention_page_has_no_approve_or_deny_outside_a_card` | every Approve and Deny button is inside a `group` named "Action …" |

Control Room (`overview.test.tsx`; the four hosts-section tests unchanged):

| Test | Expected |
|---|---|
| `control_room_has_its_title_and_intro` | h1 "Control room"; document title "Control room · Domovoi" |
| `tiles_show_attention_running_approvals_and_hosts_and_lead_to_their_lists` | "Needs you 6" → `/attention`; "Running now 1" → `/operations?status=running`; "Waiting for approval 3" (`approvals_waiting + operations_waiting_approval`) → `/attention`; "Hosts online 1 of 2" → `/hosts` with "1 offline" in words |
| `tiles_say_all_clear_when_nothing_is_wrong` | "Needs you 0" with "All clear"; "Running now 0" with "Nothing running" |
| `a_missing_attention_count_reads_neutral_not_all_clear` | overview answer without the new fields: no tone, hint "Open the attention centre" |
| `needs_you_lists_the_first_five_non_approval_items` | seven items: five rows, none an approval; a link "Attention centre" (no number) |
| `needs_you_says_nothing_else_needs_you_when_only_approvals_wait` | approvals present, no other item: title "Nothing else needs you" |
| `needs_you_never_reads_a_failed_source_as_empty` | one source fails: the warning with Retry, no empty state |
| `running_now_lists_running_operations_with_links` | rows with pill, goal link, hosts; empty state "Nothing is running" |
| `waiting_approvals_are_cards_decided_in_place_with_secondary_approve` | up to 3 cards, `emphasis` secondary, the "{n} more waiting" link when more exist; approving sends the shown plan hash (30's flow, one test through the Control Room) |
| `no_step_has_two_approve_buttons_on_the_control_room` | with a waiting operation also present in "Needs you": exactly one Approve for it |
| `an_incompatible_host_is_among_the_cards_and_never_called_offline` | host with `protocol_state "incompatible"`: card pill "Incompatible"; with six offline hosts too it is still shown (the selection ranks it first) |
| `host_selection_ranks_incompatible_then_offline_then_expected_incompatible` | `hostSeverity` order |
| `control_room_follows_the_host_selector` | every query carries `host` |
| `a_failed_overview_is_a_load_error_with_retry` | `failure(500)`: "Could not load the control room" |
| `a_journal_frame_refreshes_the_control_room` | as in the attention test, for the operation lists and the overview |

Search:

| Test | Expected |
|---|---|
| `nothing_is_requested_for_fewer_than_two_characters` | `?q=a`: no request except `GET /hosts`; the muted line |
| `search_groups_run_for_a_query_and_show_their_headings_and_the_scope_sentence` | `?q=plex`: the six headings (five without the facts route), the sentence built from `SEARCH_GROUPS` |
| `the_query_lives_in_the_url_after_the_debounce_and_clearing_removes_it_at_once` | fake timers |
| `incidents_group_asks_the_hub_with_q_and_links_to_each_incident_and_to_the_filtered_list` | `GET /incidents?q=plex`; row link; "Open in Incidents" `/incidents?q=plex` |
| `active_operations_are_matched_here_against_all_seven_status_lists` | seven `GET /operations?status=…`; a goal containing both words matches, one containing one does not; a host name match counts |
| `active_operation_search_says_when_it_could_not_see_everything` | `next_cursor` on one list: the note |
| `past_operations_come_from_experience_search_with_the_historical_label` | `GET /experience/search?q=plex&scope=global`; the label text under the heading; rows link to `/operations/{id}`; outcome pill and "Verified"; no approve control anywhere |
| `notes_group_uses_note_search_and_links_to_the_note` | |
| `facts_group_searches_the_knowledge_route_and_links_nowhere` | value as text, source tag, "May be out of date" for `stale` (skipped when the route is absent; then the test `without_the_facts_route_the_page_has_five_groups` runs instead) |
| `hosts_are_matched_in_the_browser_from_the_complete_list` | |
| `each_group_shows_five_and_show_all_reveals_the_loaded_rest_without_a_request` | |
| `a_failed_group_shows_its_error_and_the_rest_show_results_never_nothing_found` | one `failure(500)`: its `LoadError` with Retry; the warning "Some sources could not be searched"; no "Nothing found" |
| `no_match_anywhere_says_nothing_found_only_when_every_group_succeeded` | |
| `result_text_is_literal_and_highlighted_never_interpreted` | a title of `<b>x</b> plex` appears as text with `plex` marked |
| `search_ignores_the_host_selector_and_says_so` | `AllHostsNote`; requests carry no `host` |

Shell, routes, catalogue:

| Test | Expected |
|---|---|
| `attention_and_search_are_in_the_navigation_with_the_count` (`layout.test.tsx`) | Respond: "Control room", "Attention", "Incidents", … in order; "Attention" shows the count read as "6 need you" from `attention_total`; Knowledge: "Search" first |
| `the_palette_offers_search_everything_and_lists_the_new_pages` (`palette.test.tsx`) | the Action navigates to `/search` |
| `new_routes_render_their_pages` (`routes.test.tsx`) | `/attention`, `/search` |
| `attention_demos_render_every_kind_and_state` (`dev/attentionDemos.test.tsx`, `skipAxe()` with a comment) | rows of every kind, tiers, the partial-failure message, a search group in each state |

### 12.4 Product tests and gates

| Test | Expected |
|---|---|
| `p0_visible_in_every_profile_on_the_control_room` | `expectP0Everywhere` over `/` with a waiting approval, a running operation, an unknown-result operation and an incompatible host; `expectedIds = ["control.tiles", "control.attention", "control.running", "control.approvals", "control.hosts"]`; the count of each id equals Detailed's in every profile |
| `p0_visible_in_every_profile_on_the_attention_centre` | `expectedIds = ["attention.list"]` and one `attention.item` per non-approval item; with a page override in both directions |
| `p0_visible_in_every_profile_on_search` | `expectedIds = ["search.scope", "search.status"]` (status when a group failed or nothing matched) and `search.history-label` when past results exist |
| `the_profile_never_changes_which_requests_these_pages_make` | the call sets in Detailed and Essential are equal apart from the preferences request |
| `approving_from_the_control_room_in_essential_still_needs_the_typed_confirmation_and_sends_the_shown_plan_hash` | a destructive waiting step approved from the Control Room in Essential |
| `attention_and_search_code_never_imports_an_approve_hook` (`src/features/attention/gate.test.ts`, reads sources with `import.meta.glob("../**/*.{ts,tsx}", { query: "?raw", eager: true })`, excluding tests) | no file of `features/attention`, `features/search` or `features/overview` (except through the card components) mentions `useApprove`, `useApproveStep`, `useDeny` or `useDenyStep` |
| `every_p0_id_of_the_table_appears_in_the_sources` (37's gate, extended) | the P0 ids of 6.8 are added to its list |
| `visible_usage_follows_the_rules` (37's gate) | passes: every `Visible` here has a literal id of the right shape, a category literal, and `label` only for P2 or P3 |

### 12.5 Smoke test (extends `npm run smoke`)

The checks of 6.10 pass against the built bundle in a real browser (the only place colour contrast, real focus and the 375 px layout are exercised), with no `console.error` and no unexpected API request.

### 12.6 Guardrails that must keep passing unchanged

`guardrails.test.ts`, `cssGuard`, `contrast`, `voice`, `tone`, `messages`, the 250-line cap (check `OverviewPage.tsx`, `nav.ts`, `tone.ts`, `AppPalette.tsx`), no inline style, no native `title`, no raw `<button>`, every
prompt 30, 37 and 50 test, `approvals.test.tsx`, and the hub's `gates.rs`.

## 13. Acceptance criteria

1. `OverviewDto` carries the six counts with the definitions of 6.1; `attention_total` equals the sum stated there and uses `incident_groups.needs_action`; they honour `?host=`; no existing field or test changed; the
   OpenAPI snapshot differs by exactly six properties and `npm run gen:api` leaves no diff.
2. `attentionItems` orders items by the fixed table of 6.2 (tier, rank, within-rank order) with pills only from design lookups; a host that is incompatible is its own row and is never labelled offline; a failed source
   is never an empty list (`summarizeSources`).
3. The Control Room at `/` shows four tiles (each a link to its filtered list), "Needs you" (first five non-approval items), "Running now", the approval cards (at most three, secondary Approve, a "more" link) and
   the hosts section, with incompatible hosts always selected for display; every state (loading, partial failure, empty, error) reads correctly and none reads a failure as "nothing needs you".
4. The attention centre at `/attention` lists every item in three tiers with headings, approvals as shared cards (primary Approve) and everything else as one-link rows with a plain "why"; failed and truncated
   sources are said; the all-clear appears only when every source loaded and none had an item.
5. The shell shows "Attention" with the live count from `attention_total`; the Control Room replaces "Overview" in the navigation and the document title; "Search" is in the navigation and the palette has "Search
   everything".
6. Global search at `/search?q=` asks nothing below 2 characters, searches its groups (hosts and active operations in the browser against complete lists, the rest by the hub), says which sources it covers, shows
   five per group with "Show all" over the loaded items, labels past operations "historical evidence, not authorization", and never says "Nothing found" while a group failed.
7. No step has two Approve buttons on any of the pages; Approve and Deny exist only inside the shared cards; no file of the new features imports an approve or deny hook (gate).
8. The pages are live through the shell's stream (a frame newer than a list's `as_of_seq` refreshes it) and open no stream or poll of their own.
9. Every element carries the category of 6.8; P0 is present and expanded in all three profiles on all three pages (`p0_visible_in_every_profile_on_*`), also with page overrides; the profile never changes a request.
10. Unknown values render neutral and never throw; an older hub (no new fields, no experience or knowledge route) degrades as section 9 says.
11. Untrusted text is rendered as text; highlighting is literal; no link is built from text.
12. The Overview tests changed are exactly those of 6.9 and the four hosts-section tests pass unchanged; `layout.test.tsx` and `routes.test.tsx` change only where 6.9 says.
13. Every new Rust file is at most 500 lines and every web file at most 250 (including `OverviewPage.tsx`, `nav.ts`, `tone.ts`); no raw colour, inline style, native `title` or raw `<button>` added; icons only through `Icon`;
    no migration, `Cargo.toml`, route or wire change.
14. The browser smoke test passes the Control Room, Attention and Search checks including colour contrast, in both colour schemes and each profile.
15. `cargo fmt`, `clippy`, `nextest` for `domovoi-hub`, `npm run format:check`, `lint`, `typecheck`, `test`, `build` (size within budget), `smoke` and `gen:api` (no diff) pass.
16. `web/DESIGN.md`, the book (`guide/control-room.md`, `SUMMARY.md`, `reference/api.md`), `ARCHITECTURE.md` and `/dev/design` are updated as section 15 says, with no prompt number or build-plan reference in any
    committed file.

## 14. Migration and compatibility

- **Existing installs and data.** Nothing to migrate. After the upgrade `/` shows the Control Room; every bookmark and link to `/` still works; the Approvals page, the Incidents pages and the Operations pages are unchanged.
  With `[orchestration] enabled = false` (the default until the legacy triage path is removed) the operation counts are 0, "Running now" is empty and the attention centre lists incidents, classic approvals, classic
  unknown results and hosts, which is what the owner already had.
- **Existing tests.** Only those of 6.9 change. Tests that build an `Overview` answer gain the six fields through the shared `overview()` fixture. Tests that mock `GET /api/v1/overview` inline (`grep -rn
  "api/v1/overview" web/src`) are checked: a missing new field must not break them (the page reads the fields defensively).
- **Other U4 prompts.** The policy, knowledge, capability and audit prompts add nav items and `VISIBILITY_PAGES` entries beside these; whichever merges second keeps both sides. The knowledge pages prompt may add a
  facts hook and a facts page: it should reuse `useFactSearch` or replace it, and may then link the facts rows of the search; that is its change, not this one.
- **Prompt 36 (Operator) and 58 to 60.** If the Chat item was replaced by the Operator, keep this prompt's two inserted items where section 4.5 says and leave the rest. When the legacy executor is removed, the classic
  sources (`proposed`, `outcome_unknown` actions) become empty and `approvals_waiting` is retired with the Approvals page; the attention model's classic rows then never appear and can be deleted by that change.
- **Rollback.** No schema and only additive JSON; the previous web build ignores the new fields.

## 15. Documentation requirements

Write for the product's readers; never mention prompts, prompt numbers, the build plan or agents in any committed file.

- **`web/DESIGN.md`** (same PR; re-read it as the earlier prompts left it): (a) "The shell": the Respond group now starts with "Control room" and "Attention" (count from `attention_total`), the Knowledge group starts
  with "Search"; the palette's "Search everything". (b) "Layout patterns": replace the **Overview page** pattern by the **Control room** pattern (four tiles, then "Needs you" and "Running now" in two columns, the
  approval cards, the hosts that need a look) and add the **Attention centre** pattern (tier sections, link rows, shared approval cards) and the **Search page** pattern (a box, one section per source, five rows and
  "Show all", the coverage sentence). (c) "How people use it": rewrite "Morning check" around the Control room ("answer at a glance: what needs you, what is running, what waits for approval, which hosts need a
  look; an incompatible host is its own state and never reads as offline") and add "Decide an approval from the attention centre" (one paragraph: the card is the same, Approve is primary there and secondary in the
  Control room, rows link to the place of decision). (d) The category tables of 6.8 added to 37's "Classify an element" tables. (e) "Planned": remove the attention-centre entry of "Shell additions" (keep the
  autonomy indicator). (f) "Performance budget": record the new `npm run size` numbers. (g) "Checks that will fail your PR": nothing new except the gate row if 37 listed gates there.
- **Book.** New page `docs/book/src/guide/control-room.md`, "Control room, attention and search", linked from `SUMMARY.md` under Guides before Retention: what each tile counts, what "Needs you" and the attention
  centre list and in which order (the fixed table in plain words), what each urgency heading means, that approvals are decided in the same card everywhere, how an incompatible host differs from an offline one, what
  search covers and what it does not (unfinished operations are matched by goal in the browser; past operations come from the hub and are evidence, never authorization), and that a source that fails is named instead
  of hidden. `reference/api.md`: a short "Overview" section listing the counts including the six new ones and the `host` filter. Link only to pages that exist; run `cargo xtask docs-links`.
- **`ARCHITECTURE.md`**: one line in §10 saying the overview counts include operation states and an attention total; no other section. **OpenAPI**: `cargo xtask openapi && cd web && npm run gen:api`, then
  `cargo insta review`. **Config reference**: no config key, so `cargo xtask docs-config` is not needed. No ADR (no arch rule, boundary or invariant changes). **`AGENTS.md`**: no change.

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

Also state in this report: every real name from prompts 27, 28, 30, 35, 37 (and 34 if merged) that differed from section 3 (hook signatures, `OperationSummary` fields, `ApprovalCard` props, the smoke mock's route table,
whether `operationOptions` already existed); whether the facts route existed (and therefore whether the facts group was built); the exact list of existing tests you changed and why (it must match 6.9); the new `npm run
size` numbers; the line counts of `OverviewPage.tsx`, `nav.ts` and `tone.ts` after; the number of P0 elements per page that the `p0_visible_in_every_profile_on_*` tests asserted; and whether `cargo xtask docs-links`,
`node scripts/screens.mjs` and the real-browser smoke test were run. List as open items for the owner: the Approvals page and `approvals_waiting` still count classic actions only; "operations" in search means active
operations matched in the browser plus past operations from the hub; the operation lists are refetched by the shell's stream policy (several status queries per operation frame on the Control Room), so if that proves
too chatty a hub-side `active` filter is the next step.
