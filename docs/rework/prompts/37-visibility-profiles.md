# 37 — Visibility categories, presentation profiles and the owner preferences endpoint

| | |
|---|---|
| Branch | `visibility-profiles` |
| Milestone | U3 |
| Depends on (must be merged) | 30 and 29, and through them 26–28 and 59. Prompt 30 (operations UI) and prompt 29 (hub use-case layer) are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"**. If 29 or 30 is not merged, stop and report: this prompt classifies the screens 30 builds and adds a command to the service layer 29 builds. |
| Can run in parallel with | 31–36 (order-flexible, never concurrent). 36 and this prompt both edit `web/src/app/Layout.tsx`, `web/src/components/index.ts` and `web/DESIGN.md`; whichever runs second rebases on the first. 38, 43 and 48–51 **follow** this prompt and consume what it produces |
| Produces (shared contracts) | (1) The **visibility categories contract**: categories `P0` to `P3`, the three profiles and the fixed category-to-presentation table (`web/src/design/visibility.ts`: `Category`, `ProfileId`, `Presentation`, `presentationFor`, `PRESENTATION`); the hook **`useVisibility(category)`**, `useProfile()`, the kit pieces `Visible`, `DetailsScope`, `Disclosure`, `VisibilityPage`, the helper `visibilityAttrs`; the provider `VisibilityProvider`. (2) The **owner preferences endpoint** `GET` and `PUT /api/v1/me/preferences`, stored per user, and its hub service `PreferenceService`. (3) The **fixed category table of the operations screens** (section 6.4) and the rule that decides a category for any later screen (6.5). (4) The test helper `web/src/test/visibility.tsx` and the named test `p0_visible_in_every_profile`. (5) The settings section "Display" and the account-menu "Detail level" switch |
| Consumes | 30: the operations pages, step outline, inspector, `ApprovalCard`, `StepOutline`/`steps.ts`, the right slot, `npm run smoke`. 29: `crate::services::{Services, ServicesDeps, Caller}`, rule `CALLER-1`, rules `MOD-1..4`. 28/27/26 only through 30's hooks. Existing: `SettingsRepo` (generic key and JSON value store), `Clock`, the kit (`Section`, `Menu`, `SegmentedControl`, `Select`, `DataTable`, `Count`, `Tooltip`) |
| Launch gate | none |
| Suggested model | medium: many small files; every classification decision is already made here, so the judgement is in keeping P0 content on screen and the tests honest |

## 1. Objective

Let the person who runs the servers choose how much the interface shows, without ever being able to hide
what keeps them safe. Concretely:

- **One shared mechanism.** Every element of a screen carries exactly one **category**: `P0` safety, approval,
  audit and current state; `P1` primary; `P2` technical detail; `P3` metadata. Three **profiles**
  (**Detailed**, **Standard**, **Essential**) decide, from the category alone, whether the element is shown,
  collapsed or hidden. There are no three layouts: one set of components, one table. The primitive is
  `useVisibility(category)`; the kit pieces `Visible`, `DetailsScope` and `Disclosure` do the common cases.
- **P0 is never hidden or collapsed in any profile**, including a per-page override. This is enforced in the
  table lookup itself, in the kit pieces, and by a test that renders each operations screen in every profile
  with the same fixtures and asserts every P0 element is present and expanded (`p0_visible_in_every_profile`).
- **The owner's choice is stored on the hub, per user**: `GET` and `PUT /api/v1/me/preferences`, a hub service
  `PreferenceService` behind the use-case layer of prompt 29, kept in the existing generic `settings` table (no
  migration). Today there is one owner (D-13); the key carries the user id, so a second identity gets its own
  choice with no change. A browser cache of the last known choice avoids a flash on load; the hub is the
  authority.
- **Where the person chooses**: Settings, section "Display" (the default profile, an optional override per
  page, a table showing exactly what each profile does, Reset), and a one-click "Detail level" group in the
  account menu ("one setting away").
- **The operations screens built by prompt 30 are classified**: step kinds and outcomes in the timeline and
  tree, the operation detail page, the details block, the step inspector fields, the approval card, the
  incident page's Investigations section and the Operations list. Section 6.4 is the table; it is **fixed**.
- **Default profile for a new owner: Detailed** (everything shown), as the design decided. With no preferences,
  an unreachable hub or a damaged stored value the interface shows everything: it fails visible.

Profiles are display preferences only. They never change authorization, tool availability, audit retention,
execution behaviour or any security control; the hub never reads the preference when it decides anything.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**RW-21 / D-10 (profiles in v1).** Detailed, Standard and Essential ship in v1. They are one set of
components with a category-to-visibility table, not three layouts. P0 (safety, approval, audit, current state)
content is present in every profile. The design pack adds **Custom**: per-page overrides of one of the three
(a person may want a compact list and a detailed operation page). The table, **fixed** (this is the product
decision; a page that needs a different result for one element changes the element's category, never the table):

| Category | Detailed | Standard | Essential |
|---|---|---|---|
| P0 safety, approval, audit, current state | shown | shown | shown |
| P1 primary | shown | shown | shown |
| P2 technical detail | shown | collapsed (one click away, with a count) | collapsed |
| P3 metadata | shown | collapsed | hidden, with a visible "+N details" indicator that reveals them |

Rules that follow, all **fixed**:

1. *Collapsed* means a labelled toggle (with a count) stands where the element would be; the element is **not
   rendered until opened** (profiles also save work). *Hidden* means nothing stands there except one
   "+N details" control per **details scope**, which reveals every hidden element of that scope and can hide
   them again. Hidden content must always be reachable: a P3 element outside any details scope is therefore
   shown collapsed, never dropped.
2. **P0 is never hidden or collapsed**, also not when a page override or an unreadable preference is in play,
   and **a P0 element is never placed inside a P2 or P3 container** (a collapsed parent would hide it).
3. P0 and P1 look the same in every profile. The difference is a guarantee: P0 is the set the test asserts and
   the set no future table change may touch. When unsure between P0 and P1 for anything that tells a person
   what is happening, what is waiting for them, what authority applies or what was recorded, choose P0.
4. Profiles **never omit durable history**: collapsed and hidden content stays one action away and the hub
   returns the same data in every profile. The hub does not know the profile.
5. Profiles are **display preferences only**. They never change authorization, tool availability, audit
   retention, execution behaviour or security controls. A setting that affects authority belongs to the policy
   system, not here.
6. The UI reads the profile through **one provider**; everything else asks `useVisibility(category)`.
7. Dialogs, toasts, menus and tooltips are the answer to a person's action and are never subject to
   visibility: an Approve dialog shows its consequence, the typed confirmation and the plan code in every
   profile.

**D-13 / RW-29 (single owner, identities ready).** One account today. The preference is **per user**: stored
under the user's id, read and written only for the signed-in user, never by a user id in a URL. A second account
would get its own choice with no schema or route change.

**RW-19 / prompt 29 (use-case layer).** A hub command goes through a method on `crate::services` that takes
`&Caller`; the HTTP handler only parses, calls the service and maps the answer. Saving a preference is such a
command, so it is `PreferenceService::update`. The read (`get`) goes through the same service. No engine
handle, no new top-level hub module (`services/preferences.rs` lives inside the existing `services` module).

**RW-22 / RW-11 (approval card) and 30's decision 7.** Prompt 30 built the approval card, the banners, the
outcome pills and the plan-hash prefix so that they are never inside a collapsed section, and kept technical
detail (intent, output, ids, times) in the inspector and the details block "so 37 can classify without moving
things". This prompt classifies without moving anything: the visual result in **Detailed** is byte-for-byte
the screens of prompt 30 (their tests pass unchanged).

**Decisions made here (do not revisit).**

1. **Default profile is Detailed** (DESIGN.md "Presentation profiles": "Detailed (default for a new owner)").
   A research note once proposed Standard; the design pack and DESIGN.md decided Detailed. Standard and
   Essential are one click away in the account menu.
2. **No migration.** The preference is a JSON document under the key `preferences.<user id>` in the existing
   `settings` table through the existing `SettingsRepo` port (the same pattern as `host.<name>.profile` and
   `hub_host_id`). An absent key means the defaults. See section 8.
3. **Endpoint is `/api/v1/me/preferences`** (`GET`, `PUT`). `PUT` replaces the `presentation` group it is given
   and leaves every other group of the stored document untouched, so later preference groups (saved views,
   columns) add keys without a new route. Last write wins; no revision counter.
4. **Per-page overrides are in v1**: `pages: { "<page key>": "<profile>" }`. The hub validates the key's shape
   (`^[a-z][a-z0-9_-]{0,31}$`, at most 32 entries), not membership: later web prompts add page keys with no hub
   change. The web has a closed list of the page keys it uses (`VISIBILITY_PAGES`); the Display section offers
   those only and preserves unknown keys it received.
5. **Categories are written in code**, one per element, through `Visible`, `visibilityAttrs` or
   `useVisibility`, with a stable string `id` (`operation.banner`). There is no central element registry.
   The category table of section 6.4 lives in this prompt, in `web/DESIGN.md` and in the tests, and the tests
   fail when code and table disagree.
6. **A step row's category depends on its kind and its outcome** (6.4): a problem outcome (failed, refused,
   unavailable, result unknown, never ran) makes any step P0, so a failure is never folded away.
7. **The unclassifiable is shown**: an unknown step kind is P1, an unknown outcome is P0, an unknown profile
   string in a stored document or a hub answer is read as Detailed.
8. **Collapsed list rows fold into one group**, not one toggle per row: consecutive collapsed steps become one
   "N technical steps" disclosure.
9. **Nothing is persisted by profile** besides the preference itself: whether a disclosure is open is in
   memory and resets on reload. The kit helps with a `reveal` prop (open because the selected step is inside).

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, or **(introduced by NN — verify)**.
Web paths are under `web/` unless they start with `crates/` or `docs/`.

### 3.1 Hub: what exists for a per-user preference

- `crates/domovoi-core/src/ports/repos/accounts.rs:13` **(existing)**: `trait SettingsRepo { get(key) -> Option<Value>;
  set(key, &Value); set_many(&[(String, Option<Value>)]); list_prefix(prefix) }`: a generic key and JSON value store
  ("Runtime settings changed from the web UI, as JSON values by key"). `crates/domovoi-store/src/repos/accounts.rs:14`
  **(existing)** implements it on the `settings` table (`key TEXT PRIMARY KEY, value TEXT NOT NULL`, created in
  `0001_init.sql`); `crates/domovoi-testkit/src/repos/accounts.rs:10` **(existing)** has `InMemoryStore: SettingsRepo`
  (use it in unit tests).
- The `settings` table already holds more than the UI config document. Keys seen in code: `config`, `config.previous`,
  `secret.<path>`, `last_apply` (`crates/domovoi-hub/src/settings/stored.rs:17-27`, written only by the settings service),
  `host.<name>.checks`, `host.<name>.docker`, `host.<host id>.profile` (`profile/mod.rs`), `hub_host_id`
  (`runtime/adapters.rs:147`), the digest's last-sent key (`runtime/schedulers.rs:161`). **`preferences.` is a free
  prefix.** The settings service reads only the keys it owns (`config`, `secret.*`, `last_apply`, `host.<name>.*`) and nothing in the hub lists the table by prefix, so a `preferences.*` row never reaches `GET /api/v1/settings`.
- Migrations on the committed tree end at `0011_plan_binding.sql`; unmerged prompts add more. **This prompt adds none.**
- `crates/domovoi-hub/src/api/mod.rs` **(existing)**: `Authed(pub Current)` extractor (`:248`), `Current { user: User, session_hash,
  csrf_token }` (`crates/domovoi-hub/src/auth/mod.rs:103`), `ApiDoc` with `tags(...)` (`:295`), `pub fn routes()` with
  `.routes(routes!(...))` lines (`:322`). Unsafe methods pass the CSRF guard in `crate::auth::csrf` (router-wide, `router.rs`), so a
  `PUT` needs no extra code. `api/error.rs` **(existing)**: `ApiError::{BadRequest(String), NotFound, Internal, ...}` with
  `From<StoreError>`; `bad_request` is already worded by the web (`design/messages.ts` `HUB_CODES`).
- `api/rules.rs` and `api/notes.rs` **(existing)** are the models for a handler with `#[utoipa::path(...)]`, a request DTO
  deriving `Deserialize, ToSchema` and a response DTO deriving `Serialize, ToSchema` (copy their doc-comment style).
- **Prompt 29 — introduced by prompt 29, verify in code before use**: `crate::services::{Services, ServicesDeps}` (`Services` holds
  `incidents`, `approvals`, `operations`, `hosts`, `briefing`), `Caller { user, surface }` with `Caller::user() -> UserId`,
  `Authed::caller(&self) -> Caller` (the one web call site, `CALLER-1`), `ApiState::services()`. `ServicesDeps` is built in
  three places: `crates/domovoi-hub/src/runtime/mod.rs`, `crates/domovoi-hub/examples/dev_hub.rs`,
  `crates/domovoi-hub/tests/hub/api_harness.rs`. The layer's rule: a command goes through a service; a service never imports
  `crate::api`, never spawns, returns domain types and its own small error enum, never `ApiError`.
- Tests: `crates/domovoi-hub/tests/hub/api_harness.rs` **(existing)**: `Api`, `Login`, `api()`, `a.admin().await -> Login`,
  `a.send(Method, uri, Some(&login), Some(body)) -> Reply { status, body, text }`, `a.get`, `a.post`. `tests/hub/main.rs`
  registers test modules. `tests/hub/api_routes.rs:529` `openapi_snapshot` compares the generated OpenAPI with
  `tests/hub/snapshots/hub__api_routes__openapi.snap` (review with `cargo insta review`). `tests/gates.rs` **(existing)** is the
  hub's gate file (for example `no_tool_or_chat_command_reaches_settings`, `:737`).

### 3.2 Web: shell, kit and data layer

- `src/app/Layout.tsx` **(existing)**: `Layout()` (`:36`) wraps `HostProvider` and `PageTitleProvider` around the shell and the
  page (`:60`). Prompt 30 adds `RightSlotProvider` and the `.body` row **(introduced by 30 — verify)**. The visibility provider
  goes inside the same wrapper chain, once.
- `src/app/SidebarFooter.tsx` **(existing)**: `UserRow()` (`:32`) builds the account `Menu` with `items={[ ...THEMES.map(... group:
  "Theme", checked ...), { label: "Log out", ... } ]}` (`:50-60`) and the comment "Account settings join this menu later."
- `src/components/Menu.tsx:9` **(existing)** `MenuItem { checked?, group?, label, onSelect, icon?, disabledReason?, ... }`: consecutive
  items with one `group` sit under a heading; `checked` makes a radio item.
- `src/features/settings/sections/views.ts:7` **(existing)**: `VIEW_SECTIONS: SectionSpec[]` with one entry (`id: "status"`, `fields:
  none`, `view: StatusSection`). `sections/index.ts:11` says "To add one (Account, for example), add one entry here".
  `SectionSpec` (`sections/types.ts`) has `id`, `title`, `intro`, `fields`, `view`. `SectionBody.tsx` draws a section's `view`.
  **`SettingsPage.tsx`** shows `SettingsEditor` (sections) only when `settings.editable` is set; when a file manages the hub it shows
  `ReadOnlySettings` instead (`ReadOnlySettings.tsx:94`, a list of `Block title=... ` entries ending with "About and updates",
  `:170`). `SECTIONS` is therefore **not** reachable in the read-only mode: the Display panel must be added to both (6.9).
- `src/api/queryClient.ts:5` **(existing)**: `keys` object. `src/api/hooks/session.ts` **(existing)**: `useSession`, a hook that
  calls `call<T>(api.GET("/api/v1/session"))`. `src/api/types.ts` **(existing)**: DTO aliases from `schema.d.ts`, `call`, `ApiFailure`.
- Persisted per-viewer conveniences use `localStorage` inside try and catch: `src/design/theme.ts:9` (`THEME_KEY`), `src/app/
  HostContext.tsx:4`. Copy that pattern for the cache of 6.8; never let a storage failure throw.
- `src/components/index.ts` **(existing)** is the only import point for pages. Kit pieces used here: `Section`, `Stack`, `Cluster`,
  `Text`, `InlineMessage`, `Button`, `Count`, `Tooltip`, `InfoTip`, `SegmentedControl`, `Select`, `Field`, `DataTable`, `Tag`,
  `QueryBoundary`, `LoadError`, `Skeleton`, `useActionFeedback`, `Menu`. `Section` already has a `collapsed` prop with its own toggle
  (`Section.tsx`): it is for page sections, not for the inline disclosure this prompt needs.
- `src/design/tone.ts`, `src/design/*Vocab.ts` **(existing)**: status words. Profiles and categories are **not** statuses: their words
  live in the new `src/design/visibility.ts`.
- `src/design/messages.ts` **(existing)**: `describeError`; `bad_request` and `unavailable` are worded. No new catalogue entry is needed.
- Tests: `src/test/apiMock.ts` **(existing)**: `route("GET /api/v1/...", handler)`, `calls`, `failure(status, code, message)`, `api.PUT`
  exists; **an unregistered route throws `no test route for ...` (`:74`)**, which only fails that one query. `src/test/render.tsx`
  `renderAt(url, path, element)` does **not** mount a visibility provider. `src/test/setup.ts` clears `localStorage` after every test and
  runs axe over what a UI test leaves on screen.
- `DESIGN.md` **(existing)**: "Planned" section, "Presentation profiles" paragraph and its table (`:390-403`), "Shell additions" and "New
  kit components" (`:405-418`: "the profile provider and visibility primitive"), "Checks that will fail your PR" (`:464`).

### 3.3 Web: the operations screens this prompt classifies (introduced by prompt 30 — verify every name in code)

Prompt 30 creates, under `src/features/operations/`: `OperationsPage.tsx`, `OperationsTable.tsx`, `OperationDetailPage.tsx`,
`OperationHeader.tsx`, `OperationBanner.tsx`, `banner.ts`, `WaitingForYou.tsx`, `QuestionCard.tsx`, `StepOutline.tsx`, `StepRow.tsx`,
`steps.ts`, `StepInspector.tsx`, `InspectorSections.tsx`, `OperationDetails.tsx`, `StaleNote.tsx`, `IncidentOperations.tsx`; under
`src/features/approvals/`: `ApprovalCard.tsx`, `StepApproveControl.tsx`; and the right slot under `src/app/rightSlot/`. What this
prompt relies on, from 30's brief:

- Detail page order (30 6.9): header (title, status pill, cycle tag, host tags, Pause and "More"), `StaleNote`, `AllHostsNote`, **one
  banner**, the `Progress` line, **"Waiting for you"** (approval cards and question cards, only when something waits), **Steps** (view
  switch Timeline or Tree and `StepOutline`), **Details** (`OperationDetails`: Kind, Incidents, Started, Ended, Pinned subjects, then
  Budgets), the inspector surface in the right slot.
- `StepOutline({ trace, executions, selected, onSelect, view })`: timeline = one `<ol aria-label="Cycle N">` per cycle (a visible
  muted "Cycle N" label when more than one cycle exists), tree = nested `<ol>` by `parent` with fold buttons; rows are `StepRow`
  (kind icon and label, summary, "Retry" tag, outcome `StatusPill`, time, contract for executions); windowing shows the newest 200
  plus unfinished, `outcome_unknown` and selected steps and a "Show {n} earlier steps" button. `steps.ts` has `byCycle`, `buildTree`,
  `windowSteps`, `connections`, `pendingQuestions`, `awaitingExecutions`.
- Step kinds: `reasoning_turn evidence_request assessment plan policy_decision approval execution verification notification question
  note`. Step outcomes: `pending running succeeded failed refused not_run unavailable outcome_unknown skipped cancelled`. Both come from
  `src/design/operationVocab.ts` (`STEP_KINDS`, `STEP_OUTCOMES`); an unknown value is a neutral pill.
- Inspector (30 6.14), mounted as `RightSlotSurface id="inspector"`, blocks in order: (1) kind icon and label, outcome `StatusPill`, `phase`
  and `cycle` muted, times; (2) `summary` and `reason` (`reason` as an `InlineMessage` for `refused not_run unavailable failed
  outcome_unknown`); (3) for an execution step, `ApprovalCard readOnly` and, while pending, "Decide in Waiting for you on the page.";
  (4) **Connections**; (5) **Details** from `useOperationStep`: the intent as a `KeyValueList mono`, then the output in a `CopyBlock`
  labelled "Step output". `useOperationStep` fetches the step; `reason` exists only on that DTO (not on the trace), so the inspector
  keeps fetching it in every profile.
- `ApprovalCard({ source, host, why, extras, emphasis, readOnly, operation, incident })` renders: skill, host, risk pill, check and target
  tags, arguments, the model's reason ("Model's reason (not checked): ..."), optional extras (effect class, policy line, agent refusal),
  a footer (who suggested it or who approved it and when, `Expiry`, `PlanTag`, optional links), a state note, and the decision area.
  `ActionCard` is a thin wrapper over it. `approvals.test.tsx` (714 lines) and the incident tests must keep passing unchanged: they render
  with no provider, which means Detailed.
- The Operations list (30 6.9): `DataTable` with columns Operation (goal link and a "Legacy" tag), Status, Hosts, Updated;
  `QueryBoundary`, `LoadError`, `EmptyState`, partial-failure `InlineMessage`. `IncidentOperations` renders `Section title="Investigations"`.
- `npm run smoke` (30): Playwright against the built bundle with a mock hub in `scripts/smoke/{server,data,checks}.mjs`. The mock
  answers an exact list of `/api/v1/*` routes and **fails the run on any other**.

## 4. Exact scope

Every web source file stays at or under 250 lines and every non-test Rust file at or under 500. **(new)** means the file does
not exist yet.

### 4.1 Hub (`crates/domovoi-hub`): the preferences service and endpoint

| File | Change |
|---|---|
| `src/services/preferences.rs` **(new)** | `PreferenceService`, `Preferences`, `PresentationPrefs`, `Profile`, `PresentationInput`, `PreferencesUpdate`, `PreferenceError`, `preferences_key`, the limits (6.1). Unit tests inside the file with `InMemoryStore` |
| `src/services/mod.rs` | `mod preferences;` and re-exports; `Services` gains `pub preferences: PreferenceService`; `ServicesDeps` gains `pub settings: Arc<dyn SettingsRepo>` (skip if 29's merged `ServicesDeps` already holds one) and `Services::new` builds the service from `settings` and `clock` |
| `src/api/preferences.rs` **(new)** | `get` and `put` handlers with `#[utoipa::path]`, the DTOs `PreferencesDto`, `PresentationDto`, `PreferencesUpdateDto` and their mappers (6.2) |
| `src/api/mod.rs` | `mod preferences;`, tag `("me", "The signed-in person's own preferences")`, `.routes(routes!(preferences::get, preferences::put))` |
| `src/runtime/mod.rs`, `examples/dev_hub.rs`, `tests/hub/api_harness.rs` | add the `settings` value to the three `ServicesDeps` constructions (each already holds the store) |
| `tests/hub/preferences.rs` **(new)**, `tests/hub/main.rs` | the HTTP tests of 12.1 and the module registration |
| `tests/gates.rs` | the gate of 12.1 (`display_preferences_never_reach_authority`); extend, never weaken |
| `tests/hub/snapshots/hub__api_routes__openapi.snap` | regenerated and **read** with `cargo insta review` |
| `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | **regenerated**, never hand-edited (`cargo xtask openapi && cd web && npm run gen:api`) |

No change to `domovoi-core`, `domovoi-engine`, `domovoi-store`, `domovoi-testkit`, `xtask` or any `Cargo.toml` (arch-check still
runs once, because a hub module was added).

### 4.2 Web: design table, kit, provider, data layer

| File | Change |
|---|---|
| `src/design/visibility.ts` **(new)** | pure: `Category`, `CATEGORIES`, `ProfileId`, `PROFILE_IDS`, `DEFAULT_PROFILE`, `Presentation`, `PRESENTATION`, `presentationFor`, `visibilityOf`, `isProfileId`, `PROFILE_META`, `CATEGORY_META`, `PRESENTATION_WORDS`, `VISIBILITY_PAGES` (6.3). `visibility.test.ts` **(new)** |
| `src/components/visibilityContext.tsx` **(new)** | `VisibilityContext`, `VisibilityPage`, `useProfile`, `useVisibility`, `visibilityAttrs`, `DEFAULT_PRESENTATION_PREFS` (6.3) |
| `src/components/Disclosure.tsx` **(new)** | the collapsed-content toggle (6.3) |
| `src/components/Visible.tsx`, `DetailsScope.tsx` **(new)**, `visibility.module.css` **(new)**, `index.ts` | the block primitive and the details scope; exports of the whole visibility group (6.3) |
| `src/app/VisibilityProvider.tsx`, `visibilityCache.ts` **(new)** | the hub-backed provider and the browser cache (6.8); `Layout.tsx` mounts the provider once |
| `src/api/hooks/preferences.ts` **(new)**, `queryClient.ts`, `types.ts` | `usePreferences`, `useSavePreferences`, `keys.preferences`, the aliases `Preferences` and `PresentationPrefs` (6.7) |
| `src/test/visibility.tsx` **(new)** | `ProfileProvider` for tests, `renderWithProfile`, `p0Elements`, `expectP0Everywhere` (12.2) |

### 4.3 Web: where the person chooses

| File | Change |
|---|---|
| `src/features/settings/views/DisplaySection.tsx`, `ProfileTable.tsx` **(new)** | the Display panel: default profile, per-page overrides, the table of what each profile does, Reset (6.9) |
| `src/features/settings/sections/views.ts` | one entry `{ id: "display", title: "Display", ... view: DisplaySection }` |
| `src/features/settings/ReadOnlySettings.tsx` | one `<Block title="Display"><DisplaySection /></Block>` before "Advisories" (so a file-managed hub still has it) |
| `src/app/SidebarFooter.tsx` | account menu: group "Detail level" (three radio items) and a "Display settings" entry (6.9) |

### 4.4 Web: classifying the operations screens (introduced by prompt 30; edit in place, move nothing)

| File | Change |
|---|---|
| `src/features/operations/stepVisibility.ts` **(new)** | `stepCategory`, `treeCategory`, `reasonCategory`, `groupSteps`, `PROBLEM_OUTCOMES`, `KIND_CATEGORY` (6.4) |
| `OperationsPage.tsx`, `OperationsTable.tsx` | `VisibilityPage page="operations"`; `Visible` markers of the list rows (6.4) |
| `OperationDetailPage.tsx`, `OperationHeader.tsx`, `OperationBanner.tsx`, `WaitingForYou.tsx`, `StaleNote.tsx` | `VisibilityPage page="operation"`; `Visible` markers (6.4) |
| `StepOutline.tsx`, `StepRow.tsx` (and a small `StepGroup.tsx` **(new)**) | group collapsed rows through `groupSteps`; `visibilityAttrs` on rows |
| `OperationDetails.tsx` | split into three blocks: scope (P0), budgets and incidents (P1), metadata (P3 in a `DetailsScope`) |
| `StepInspector.tsx`, `InspectorSections.tsx` | the inspector blocks classified as in 6.4 |
| `src/features/approvals/ApprovalCard.tsx` | root `Visible` P0, model reason `Visible` P1; no other change |
| `src/features/incidents/` `IncidentOperations.tsx` | P1 section, P0 status marker |
| `src/features/operations/*.test.tsx`, `src/features/approvals/*.test.tsx` | the tests of section 12 |
| `src/dev/VisibilityDemos.tsx` **(new)**, `DesignPage.tsx`, `src/dev/visibilityDemos.test.tsx` **(new)** | catalogue demos of the kit pieces in all three profiles |

### 4.5 Smoke test, docs

| File | Change |
|---|---|
| `scripts/smoke/server.mjs`, `data.mjs`, `checks.mjs` (prompt 30) | the mock answers `GET` and `PUT /api/v1/me/preferences`; a new check runs the detail page in every profile, both themes, at 375 and 1280 px (12.4) |
| `DESIGN.md` | updates of section 15 |
| `../AGENTS.md` | one bullet under "Web UI" (section 15) |
| `../docs/book/src/reference/api.md`, `reference/data-model.md`, `guide/display.md` **(new)**, `SUMMARY.md` | section 15 |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Classifying any screen other than the operations list, the operation detail page, the inspector, the approval card and the incident page's Investigations section. The Overview, Incidents, Hosts, Settings, Logs, Notes pages and the shell navigation are **not** touched (they render as today: every element is, in effect, P1) | later prompts classify their own screens with the same API; 6.5 is the rule they follow |
| The Operator panel and conversation screens (prompt 36) | its owner or a follow-up adds `Visible` markers using 6.5 (doc: conversation and composer P0, tool details P2) |
| The graph canvas, its outline and node sheet | 38 (consumes `useVisibility`, `Visible`, `visibilityAttrs`; it must keep the outline P0) |
| Policy pages, the capability install wizard, Control Room, attention centre, knowledge and audit pages | 43, 48–51 |
| A **hub-side** meaning of the profile: filtering API answers, shortening the digest, a Discord card per profile, a different retention | never: profiles are display only (rule 5 of section 2) |
| Other preference groups (saved views, table columns, density, dashboard layout, theme on the hub) | later; the stored document and `PUT` already leave room (decision 3) |
| Server-side validation of page keys against a list | decision 4: shape only |
| Syncing a changed preference to other open tabs live | a tab picks it up on its next focus refetch; no journal frame, no SSE topic |
| Persisting whether each collapsed block is open | decision 9: in memory only |
| Auditing a preference change | it changes nothing a policy, approval or audit depends on; no audit row, no journal row |
| A second user account, roles, per-role defaults | D-13: the key is per user and that is all that is prepared |
| A new hub top-level module, a migration, a wire (protocol) change, a config key, a new dependency | none needed; stop and report if one seems needed |
| Changing prompt 30's pages, banners, approval flow or any approval rule beyond adding markers | out of scope; Detailed output must stay as 30 built it |
| `screens.mjs` per-profile screenshots | the real-browser smoke test (12.4) and the vitest matrix cover the profile product; do not extend `screens.mjs` |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly; later prompts call it) or **convention** (follow repository style). Every public item gets a
doc comment saying what it is for (Rust) or a JSDoc saying what it is for and not for (web kit). Never write a prompt number, "the
build plan" or an RW or D id into code, comments, the book, commit messages or the PR description.

### 6.1 Hub service **fixed** (names, parameters, return types, rules); layout **convention**

```rust
// crates/domovoi-hub/src/services/preferences.rs   (new; no import of crate::api, crate::auth::interactions, crate::runtime, crate::router)

/// Version of the stored document; bumped only for an incompatible change.
pub const PREFERENCES_VERSION: u32 = 1;
/// The most page overrides one person may store.
pub const MAX_PAGE_OVERRIDES: usize = 32;
/// The longest page key, in characters.
pub const MAX_PAGE_KEY_CHARS: usize = 32;

/// The `settings` key that holds one person's preferences: `preferences.<user id>`.
pub fn preferences_key(user: UserId) -> String;

/// How much the interface shows. Display only: nothing in the hub reads it to decide anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile { #[default] Detailed, Standard, Essential }
impl Profile {
    pub fn parse(text: &str) -> Option<Self>;     // exactly "detailed" | "standard" | "essential", lowercase
    pub fn name(self) -> &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresentationPrefs { pub profile: Profile, pub pages: BTreeMap<String, Profile> }

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Preferences { pub presentation: PresentationPrefs, pub updated_at: Option<Timestamp> }

/// A requested presentation group, unvalidated text exactly as received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationInput { pub profile: String, pub pages: BTreeMap<String, String> }
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreferencesUpdate { pub presentation: Option<PresentationInput> }

#[derive(Debug, thiserror::Error)]
pub enum PreferenceError {
    #[error("{0}")] Invalid(String),                 // text shown to the person
    #[error(transparent)] Store(#[from] StoreError),
}

#[derive(Clone)]
pub struct PreferenceService { settings: Arc<dyn SettingsRepo>, clock: Arc<dyn Clock>, write: Arc<tokio::sync::Mutex<()>> }
impl PreferenceService {
    pub fn new(settings: Arc<dyn SettingsRepo>, clock: Arc<dyn Clock>) -> Self;
    pub async fn get(&self, caller: &Caller) -> Result<Preferences, PreferenceError>;       // = self.read_for(caller.user())
    pub async fn update(&self, caller: &Caller, update: PreferencesUpdate) -> Result<Preferences, PreferenceError>;  // = self.write_for(caller.user(), update)
    // private: read_for(&self, user: UserId) and write_for(&self, user: UserId, update). The unit tests call these two, so they never
    // need to build a Caller (a Caller is built only by a transport, and the arch rule counts its construction sites).
}
```

The stored value under `preferences.<user id>` **(fixed)**:

```json
{ "version": 1, "updated_at": "2026-11-18T10:00:00Z",
  "presentation": { "profile": "standard", "pages": { "operation": "essential" } } }
```

Rules **(fixed)**:

1. **Identity.** The key is built from `caller.user()` only. No method takes a user id. `get` and `update` never touch another
   user's key (a unit test with two users proves it).
2. **Reading is total.** `get` fails only when the store fails. A missing key gives `Preferences::default()` (Detailed, no overrides,
   `updated_at: None`). A stored value that is not a JSON object, a `presentation` that is missing or not an object, a `profile`
   that is not one of the three names, a page entry whose key is malformed or whose profile is unknown, and an `updated_at` that does
   not parse are each **ignored** (profile falls back to Detailed; bad entries are dropped; at most 32 entries are kept, in key
   order). Reading never errors on content and never writes. This is the "fails visible" rule: whatever is wrong, the person sees
   everything.
3. **Writing validates everything before it locks or writes.** `update` with `presentation: None` returns `get(caller)` unchanged.
   Otherwise: `profile` must satisfy `Profile::parse`, else `Invalid("unknown profile `<text>`; use detailed, standard or essential")`;
   more than `MAX_PAGE_OVERRIDES` entries gives `Invalid("at most 32 pages can have their own detail level")`; each key must match
   `^[a-z][a-z0-9_-]{0,31}$` (write a small manual check, no new dependency), else
   `Invalid("page key `<text>` must start with a lowercase letter and use only lowercase letters, digits, - and _ (up to 32 characters)")`;
   each override must satisfy `Profile::parse`, else the "unknown profile" text naming it. Echoed text is cut to 40 characters.
   Only then take the service's mutex, read the stored value as a JSON map (a non-object counts as an empty map), set `version`,
   `updated_at` (from `Clock`, RFC 3339 through `Timestamp`'s `Display`) and **replace the `presentation` entry only**, leave every other
   key of the map as it was, `SettingsRepo::set` it once, and return the new `Preferences`. The mutex only keeps two in-process updates
   from losing each other's other keys; the last write of `presentation` wins.
4. **No audit, no journal, no effect, no event.** The service writes one row and returns. It never calls the engine, the manager or
   the notifier.
5. The service is `Clone + Send + Sync + 'static`, never spawns a task, never builds a `Caller`, an `Authorization` or a
   `HumanApproval` (rules of the use-case layer).

### 6.2 HTTP API **fixed** (paths, bodies, statuses), **convention** (DTO layout)

`GET /api/v1/me/preferences` (operation id `get_preferences`, tag `me`) answers 200:

```json
{ "version": 1, "updated_at": null,
  "presentation": { "profile": "detailed", "pages": {} } }
```

`PUT /api/v1/me/preferences` (operation id `update_preferences`, tag `me`) takes:

```json
{ "presentation": { "profile": "standard", "pages": { "operations": "detailed" } } }
```

and answers 200 with the same shape as `GET` (after the write, `updated_at` set). `pages` may be omitted (means `{}`); `presentation`
may be omitted or `null` (a no-op that answers the current value); unknown extra keys in the body are ignored (forward compatibility).
`profile` and the page values are **strings** in the schema (documented as `detailed`, `standard` or `essential`), so an older client
or a newer hub never fails to decode.

| Situation | Status and body |
|---|---|
| No session | 401 `unauthorized` (the `Authed` extractor) |
| Unknown profile, bad page key, too many pages | 400 `{"error":"bad_request","message":"<the Invalid text>"}` |
| Body is not JSON | the framework's rejection, as for every other route |
| Store failure | 500 `internal`, details logged only (`ApiError::internal("saving preferences", err)`) |

```rust
// crates/domovoi-hub/src/api/preferences.rs   (convention: copy the doc style of api/rules.rs)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PreferencesDto { pub version: u32, pub updated_at: Option<String>, pub presentation: PresentationDto }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PresentationDto { pub profile: String, #[serde(default)] pub pages: BTreeMap<String, String> }
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, ToSchema)]
pub struct PreferencesUpdateDto { #[serde(default)] pub presentation: Option<PresentationDto> }

pub(super) async fn get(State(state): State<ApiState>, auth: Authed) -> Result<Json<PreferencesDto>, ApiError>;
pub(super) async fn put(State(state): State<ApiState>, auth: Authed, Json(body): Json<PreferencesUpdateDto>)
    -> Result<Json<PreferencesDto>, ApiError>;
// both: let caller = auth.caller(); state.services().preferences.get(&caller) / .update(&caller, body.into())
// PreferenceError::Invalid(text) -> ApiError::BadRequest(text); PreferenceError::Store(e) -> ApiError::internal(..)
```
No `user` field, no id in the path: the route can only ever address the signed-in person. Register both handlers with
`.routes(routes!(preferences::get, preferences::put))` (same path, two methods in one call, as `settings::show` and
`settings_write::update` are).

### 6.3 Web: the table, `useVisibility` and the kit pieces **fixed** (this is the contract 38, 43 and 48–51 call)

```ts
// web/src/design/visibility.ts   (pure: no React, no DOM, no storage; unit-tested)
export type Category = "P0" | "P1" | "P2" | "P3";
export const CATEGORIES: readonly Category[];                       // ["P0","P1","P2","P3"]
export type ProfileId = "detailed" | "standard" | "essential";
export const PROFILE_IDS: readonly ProfileId[];                     // detailed, standard, essential
export const DEFAULT_PROFILE: ProfileId;                            // "detailed"
export type Presentation = "shown" | "collapsed" | "hidden";
export const PRESENTATION: Readonly<Record<ProfileId, Readonly<Record<Category, Presentation>>>> = {
  detailed:  { P0: "shown", P1: "shown", P2: "shown",     P3: "shown" },
  standard:  { P0: "shown", P1: "shown", P2: "collapsed", P3: "collapsed" },
  essential: { P0: "shown", P1: "shown", P2: "collapsed", P3: "hidden" },
};
/** The one lookup. P0 is "shown" whatever the table or the profile says. */
export function presentationFor(profile: ProfileId, category: Category): Presentation;
export function isProfileId(value: unknown): value is ProfileId;

export interface Visibility {
  category: Category;
  profile: ProfileId;                 // the profile in force at this place (a page override, else the default)
  presentation: Presentation;
  shown: boolean; collapsed: boolean; hidden: boolean;     // exactly one is true
}
export function visibilityOf(profile: ProfileId, category: Category): Visibility;

export const PROFILE_META: Readonly<Record<ProfileId, { label: string; hint: string }>>;
//   detailed  "Detailed"  "Shows everything, technical detail and metadata included."
//   standard  "Standard"  "Shows what matters. Technical detail and metadata are one click away."
//   essential "Essential" "Shows what matters and what needs you. Technical detail is one click away; metadata is behind a \"+N details\" button."
export const CATEGORY_META: Readonly<Record<Category, { label: string; hint: string }>>;
//   P0 "Safety and state"   "Approvals, what is happening now, what authority applies and what was recorded. Always shown."
//   P1 "Primary"            "The page's main content and frequent actions."
//   P2 "Technical detail"   "Diagnostics, raw inputs and outputs, relations and history."
//   P3 "Metadata"           "Identifiers, times and other rarely needed facts."
export const PRESENTATION_WORDS: Readonly<Record<Presentation, string>>;
//   shown "Shown"   collapsed "Collapsed, one click away"   hidden "Behind a \"+N details\" button"
/** The pages that offer their own detail level. A later prompt appends its pages. */
export const VISIBILITY_PAGES: readonly { key: string; label: string }[];
//   [{ key: "operations", label: "Operations list" }, { key: "operation", label: "Operation detail" }]
```

```tsx
// web/src/components/visibilityContext.tsx  (kit; exported from components/index.ts)
export interface PresentationChoice { profile: ProfileId; pages: Readonly<Record<string, ProfileId>> }
export const DEFAULT_PRESENTATION_CHOICE: PresentationChoice;       // { profile: "detailed", pages: {} }
/** No provider above means Detailed: a test or a login page never needs one, and nothing is ever hidden by accident. */
export const VisibilityContext: Context<PresentationChoice>;
/** Names the page for per-page overrides. Renders its children with no element of its own. */
export function VisibilityPage(props: { page: string; children: ReactNode }): JSX.Element;
export function usePresentationChoice(): PresentationChoice;        // the owner's default and overrides (for the settings UI and the menu)
export function useProfile(): { profile: ProfileId; page: string | undefined; custom: boolean };
/** THE PRIMITIVE. Never throws; without a provider it answers Detailed. */
export function useVisibility(category: Category): Visibility;
/** For an element that cannot take a wrapper (a list row, a table cell): spread onto it. */
export function visibilityAttrs(category: Category, id: string): { "data-visibility": Category; "data-visibility-id": string };
```

`useProfile()` returns `pages[page]` when a `VisibilityPage` is above and has an override (then `custom: true`), else the default
profile. `useVisibility(category) = visibilityOf(useProfile().profile, category)`.

```tsx
// web/src/components/Visible.tsx, DetailsScope.tsx, Disclosure.tsx   (kit; each with a JSDoc "for" and "not for")
export type VisibleProps =
  | { category: "P0" | "P1"; id: string; children: ReactNode }
  | { category: "P2" | "P3"; id: string; label: string; count?: number; reveal?: boolean; children: ReactNode };
export function Visible(props: VisibleProps): JSX.Element | null;
export function DetailsScope(props: { children: ReactNode }): JSX.Element;
export function Disclosure(props: { label: string; count?: number; summary?: string; reveal?: boolean;
                                    as?: "div" | "li"; children: ReactNode }): JSX.Element;
```

`Visible` behaviour **(fixed)**. `id` is a stable string naming the element **kind** (`operation.banner`), lowercase, dots and hyphens,
never data; it becomes `data-visibility-id`. `label` (required for P2 and P3, by the type) is the toggle's text, sentence case; `count`
is how many items the block holds (shown after the label; for P3 it is what "+N details" adds up, default 1).

| presentation | where | `Visible` renders |
|---|---|---|
| shown | anywhere | `<div class="contents" data-visibility="<cat>" data-visibility-id="<id>" data-presentation="shown">children</div>`; the wrapper has `display: contents`, so it changes no layout |
| collapsed | anywhere | the same wrapper with `data-presentation="collapsed"` holding a `Disclosure` (label, count, `reveal`) whose body is `children`, **mounted only while open** |
| hidden | inside a `DetailsScope`, not revealed | nothing; the scope adds `count` to its "+N details" total |
| hidden | inside a `DetailsScope`, revealed | as shown, with `data-revealed="true"` |
| hidden | no `DetailsScope` above | as **collapsed** (reachable, never dropped) |

P0 and P1 always render as shown; for them `label`, `count` and `reveal` do not exist in the type, and `presentationFor` cannot return
anything but "shown" for P0. A `Visible` registers a hidden block with its scope in an effect (add on mount or change, remove on cleanup).

`DetailsScope` renders its children, then, when the hidden total `n > 0`, one button (a kit `button` in `visibility.module.css`, the
same look as the `Section` toggle) with `aria-expanded`: text `+{n} details` (`+1 detail` when n is 1) while closed, `Hide {n} details`
while open; clicking toggles the scope's `revealed` state. State is in memory. A scope that has no hidden block renders no button.
Use one scope around the P3 blocks that belong together, directly after them, so the button sits where the facts would be.

`Disclosure` renders `<div|li class="disclosure"><button type="button" aria-expanded aria-controls=…><Icon chevron/> label [Count]
[summary]</button>{open && <div id role="group" aria-label={label}>children</div>}</div>` (`as="li"` for a list row). It starts closed,
starts open when `reveal` is true at mount, and opens (but never closes by itself) when `reveal` turns true later. `count` uses the kit
`Count` (tone neutral, label "items"); `summary` is muted text after it. Its button follows the kit focus ring and the 32 px row
height rules. No animation (nothing to reduce).

### 6.4 The category table of the operations screens **fixed**

`VisibilityPage page="operations"` wraps the list page; `VisibilityPage page="operation"` wraps the detail page (the inspector is
mounted by the detail page, so it inherits `operation`; React context crosses the slot's portal). Element ids are the ids in the
`data-visibility-id` attribute. "Mark" says how to carry it: **V** = `<Visible>`, **A** = `visibilityAttrs` on an element, **H** =
`useVisibility` in the component.

**Operations list** (doc: name, status, target, updated time are P0; initiator, cycle, duration P1; model usage P2: none of the P1/P2
fields exist yet, so the list looks the same in every profile today):

| id | element | cat | mark |
|---|---|---|---|
| (none) | `PageHeader` title, intro and docs link: strings given to the kit, always rendered, P1 by definition; no marker | P1 | none |
| `operations.filters` | status `FilterChips` | P1 | V |
| `operations.table` | the whole `DataTable`: Operation (goal link), Status pill, Hosts, Updated | P0 | V |
| `operations.legacy-tag` | the "Legacy" tag with its hint | P1 | V |
| `operations.state` | loading skeleton, `LoadError`, `EmptyState`, the classic-actions warning, "Load more" | P0 | V |

**Operation detail page**:

| id | element | cat | mark |
|---|---|---|---|
| `operation.title` | the `meta` of `PageHeader`: status `StatusPill`, host tags (the `h1` is the page title, a string prop: it is always rendered and carries no wrapper) | P0 | V |
| `operation.cycle-tag` | the "Cycle N" tag | P1 | V |
| `operation.controls` | the `actions` of `PageHeader`: Pause and "More" (Cancel operation) | P0 | V |
| `operation.stale-note` | "Could not refresh this page" with Retry | P0 | V |
| `operation.scope-note` | `AllHostsNote` | P1 | V |
| `operation.schema-warning` | "built for an older operation format" warning | P0 | V |
| `operation.banner` | the one banner and its buttons | P0 | V |
| `operation.progress` | "Working: ..." and "Queued. ..." line | P0 | V |
| `operation.waiting` | the whole "Waiting for you" section: approval cards, question cards | P0 | V |
| `operation.steps-head` | "Steps" heading and the Timeline or Tree switch | P1 | V |
| `operation.cycle-label` | the muted "Cycle N" label above a list | P1 | V |
| `operation.step` | every step row; its category is `stepCategory(kind, outcome)` (timeline) or `treeCategory` (tree), see below | P0, P1 or P2 | A (`visibilityAttrs(category, "operation.step")` on the row's `li`) |
| `operation.step-group` | a run of consecutive technical (P2) steps in Standard and Essential | P2 | V inside `StepGroup` (`as="li"`, label `"{n} technical step(s)"`, summary the kinds) |
| `operation.earlier-steps` | "Show {n} earlier steps" button | P1 | V |
| `operation.scope` | **Pinned subjects** (host, check, target: the only targets an action may name) | P0 | V |
| `operation.incidents` | Incidents (links) | P1 | V |
| `operation.budgets` | the Budgets rows | P1 | V |
| `operation.metadata` | Kind, Started, Ended | P3, `count={3}` | V inside a `DetailsScope` |

`OperationDetails` keeps one `Section title="Details"` and one `KeyValueList` look; it splits its rows into the three blocks above
(`operation.scope`, `operation.incidents` with `operation.budgets`, `operation.metadata`) without changing their order or text.
Order of rows inside Details is unchanged in Detailed.

**Step inspector** (one `Visible` per block of prompt 30's 6.14; the slot's `h2` is chrome and carries no category):

| id | element | cat | mark |
|---|---|---|---|
| `inspector.kind` | kind icon and label | P1 | V |
| `inspector.outcome` | the outcome `StatusPill` | P0 | V |
| `inspector.times` | `phase`, `cycle`, started, ended, "Took ..." | P3, `count={2}` (phase and cycle count as one, times as one) | V inside a `DetailsScope` |
| `inspector.summary` | the step's summary | P1 | V |
| `inspector.reason` | the step's reason | **P0 when the outcome is a problem outcome, else P1** (`reasonCategory(outcome)`) | V |
| `inspector.approval` | the read-only `ApprovalCard` and the "Decide in Waiting for you" pointer | P0 | V |
| `inspector.connections` | Connections list | P2, `count` = number of connections | V |
| `inspector.intent` | the intent key list | P2, `count` = number of keys | V |
| `inspector.output` | "Step output" block, its redaction line and the "No output is stored" line | P2 | V |

**Approval card** (`ApprovalCard`, all callers: the page, the inspector, incidents, `ActionCard`):

| id | element | cat | mark |
|---|---|---|---|
| `approval.card` | the whole card: skill, host, risk pill and its tip, check, target, arguments, expiry, plan-hash prefix and binding label, who approved and when, extras (effect, policy, refusal), state note, Approve and Deny | P0 | V (root) |
| `approval.reason` | "Model's reason (not checked): ..." | P1 | V |
| `approval.origin` | "Suggested by ... " (legacy) | P1 | V |

The decision area (Approve, Deny, the confirmation dialog, the typed confirmation, the shown plan hash) is **P0 and lives inside
`approval.card`**; `StepApproveControl.tsx`, `ApproveControl.tsx` and `OperationBanner.tsx` must contain **no** visibility primitive
(a gate test, 12.3, scans them).

**Incident page** (`IncidentOperations`): `incident.investigations` P1 (the section), and the status pill of each row `incident.investigation-status` P0.
Nothing else on the incident page is touched.

**Labels of the collapsed toggles (fixed, so tests can name them)**: `operation.metadata` "Kind and times"; `inspector.times` "Times and phase";
`inspector.connections` "Connections"; `inspector.intent` "Intent"; `inspector.output` "Step output"; a step group "{n} technical step" or "{n} technical steps".

**Step categories (`stepCategory(kind, outcome)`) fixed.** Base category by kind:

| kind | base | why |
|---|---|---|
| `approval` | P0 | a person's decision and its record |
| `execution` | P0 | what was done to a host |
| `verification` | P0 | whether the fix worked: current state |
| `question` | P0 | the model waits for, or recorded, a person's answer |
| `assessment` | P1 | findings |
| `plan` | P1 | what is proposed |
| `policy_decision` | P1 | how a rule judged an action (the policy line on the approval card is P0) |
| `notification` | P1 | what was sent to the person |
| `note` | P1 | a comment, often the person's answer |
| `evidence_request` | P2 | a read-only look at a host |
| `reasoning_turn` | P2 | a model call |
| any other (unknown) kind | P1 | unclassifiable is shown |

Then **escalation**: when the outcome is in `PROBLEM_OUTCOMES = { failed, refused, not_run, unavailable, outcome_unknown }` the step is
**P0 whatever its kind**; an outcome that is not one of the ten known values is also **P0**. `running`, `pending`, `succeeded`,
`skipped`, `cancelled` keep the base (the banner, the progress line and "Waiting for you" already carry the current state in P0).
`reasonCategory(outcome)` is P0 for `PROBLEM_OUTCOMES` and any unknown outcome, else P1.

**Tree rule.** In the tree view a step's category is the **most important** (lowest P number) of its own and all its descendants'
(`treeCategory`), so a collapsed technical parent never hides a P0 child. The timeline view uses `stepCategory` directly.

**Grouping (`groupSteps`) fixed.**

```ts
// web/src/features/operations/stepVisibility.ts   (introduced by this prompt; pure, unit-tested)
export const PROBLEM_OUTCOMES: ReadonlySet<string>;
export const KIND_CATEGORY: Readonly<Record<string, Category>>;
export function stepCategory(kind: string, outcome: string): Category;
export function reasonCategory(outcome: string): Category;
export function treeCategory(steps: readonly TraceStep[], id: string): Category;   // min over the step and its descendants
export type OutlineItem<T> = { type: "item"; item: T } | { type: "group"; items: T[] };
/** Consecutive items whose presentation is not "shown" in `profile` become one group; the rest stay single, in order. */
export function groupSteps<T>(items: readonly T[], profile: ProfileId, categoryOf: (item: T) => Category): OutlineItem<T>[];
```

`StepOutline` runs `windowSteps` first (prompt 30), then `groupSteps` per list (per cycle in the timeline, per sibling list in the
tree) with `profile = useProfile().profile`. In **Detailed no group exists** (every item is shown), so the output equals prompt 30's.
A group renders as `<StepGroup as li>` = `Disclosure as="li"` with label `"{n} technical step"` or `"{n} technical steps"` and `summary`
the kinds counted (`"Model turn ×2, Evidence"`, using `stepKindMeta(kind).label`, in first-seen order), `reveal` = the group contains
the selected step, children = a nested `<ol>` of the group's `StepRow`s. The selected step is therefore always visible, even when
reached by a link (`?step=` naming a technical step opens its group). A group never holds a P0 or P1 step, by construction.
In the tree a group is formed among **siblings** only, and its rows keep their nested children.

### 6.5 How any later screen decides a category **fixed** (the rule 38, 43 and 48–51 follow)

Ask in this order and stop at the first yes. Write the answer as the element's category in code and in its page specification.

1. **P0**: does it tell the person what is happening now, what is waiting for them, what authority applies or what was recorded, or
   does it report a failure, an unknown result, a pause, a limit, stale data, a load error or an empty or loading state? Does it hold a
   decision control (approve, deny, resume, cancel, confirm, typed confirmation, the plan code the decision binds)? Then P0.
2. **P1**: is it the page's main content or a frequent action?
3. **P2**: is it a diagnostic, a raw input or output, a relation, a history or a secondary action?
4. **P3**: is it an identifier, a time, a model parameter or other rarely needed metadata?

Hard rules: a P0 element is never inside a `Visible` of category P2 or P3 (a collapsed parent would hide it); anything with a
decision control is P0; an element that tells the person a request failed or data is old is P0; a P3 block sits inside a
`DetailsScope`; list rows and table cells use `visibilityAttrs`, not a wrapper `div`; dialogs, toasts, menus and tooltips carry no
category; visually hidden live-region lines (`role="status"` announcements) carry none. A page needing a different result for one element changes **that
element's** category, never `PRESENTATION`.

### 6.6 Hooks **fixed** (names, request shapes), **convention** (layout)

```ts
// web/src/api/hooks/preferences.ts
export function usePreferences();            // useQuery, key keys.preferences = ["preferences"]; GET /api/v1/me/preferences; staleTime 60_000
export function useSavePreferences();        // useMutation; variables: { presentation: { profile: string; pages: Record<string,string> } }
                                             // PUT /api/v1/me/preferences body { presentation }; onSuccess: client.setQueryData(keys.preferences, saved)
```
The two aliases `Preferences` and `PresentationPrefs` in `types.ts` come from the generated `PreferencesDto` and `PresentationDto`. Every
caller of `useSavePreferences` wraps it in `useActionFeedback` (`error: "save the display setting"`, or the verb phrase of the control:
"save the detail level"). A failed load is never read as "defaults": the Display panel shows `LoadError`; the provider (below) silently
keeps what it has, because showing more is the safe direction.

### 6.7 Provider and cache **fixed** (behaviour)

```tsx
// web/src/app/VisibilityProvider.tsx   (mounted once in Layout, inside HostProvider)
export function VisibilityProvider({ children }: { children: ReactNode }): JSX.Element;
// web/src/app/visibilityCache.ts
export const VISIBILITY_KEY = "domovoi.visibility";
export function parsePresentation(raw: unknown): PresentationChoice;      // total: never throws; unknown profile -> "detailed"; bad page entries dropped
export function readCache(): PresentationChoice | undefined;               // localStorage inside try/catch; undefined when absent or unreadable
export function writeCache(choice: PresentationChoice): void;              // try/catch; failure is ignored
```

```text
VisibilityProvider:
  query  = usePreferences()                       // no spinner, no error UI, never blocks rendering
  cached = readCache() once, at mount             // the last choice this browser saw (a per-viewer convenience)
  server = query.data ? parsePresentation(query.data.presentation) : undefined
  value  = server ?? cached ?? DEFAULT_PRESENTATION_CHOICE        // memoised by profile and pages
  effect: when server changes, writeCache(server)
  render <VisibilityContext.Provider value={value}>{children}</VisibilityContext.Provider>
```
The hub's value always wins over the cache. The cache only fills the gap before the first answer so a Standard owner does not see a
Detailed flash. It carries no authority and is never read by anything but this provider.

### 6.8 Display panel and account menu **fixed** (behaviour), **convention** (markup)

`DisplaySection` (`src/features/settings/views/DisplaySection.tsx`), used as the `view` of the `display` section **and** as the body of
a `Block` in `ReadOnlySettings`, inside `QueryBoundary query={usePreferences()} context="load display settings"` with a `Skeleton`:

1. `Section`-free blocks with `h3`-sized subheads (the settings views' `styles.subhead`): **Detail level**: a `SegmentedControl
   label="Detail level"` (Detailed, Standard, Essential, labels from `PROFILE_META`), a line with the chosen profile's `hint`, and the
   saved profile as the value. Choosing saves at once: `useSavePreferences` with `{ profile: <new>, pages: <current pages> }`,
   success toast `Detail level set to {label}.`
2. **What each profile shows**: `ProfileTable`, a `DataTable` (`caption="What each detail level shows"`) with columns Category
   (`CATEGORY_META` label, hint in the column cell's `InfoTip`), Detailed, Standard, Essential; every cell is
   `PRESENTATION_WORDS[PRESENTATION[profile][category]]`, derived from the table, never typed again. The P0 row reads "Shown" in all
   three columns.
3. **Per page**: for each entry of `VISIBILITY_PAGES` a `Field` labelled with its label and a `Select`: first option value `""` text
   "Same as my detail level ({label})", then the three profiles. Choosing saves `pages` with that key set (or removed for `""`),
   keeping every other key (also keys this build does not list). Toast `{page label}: {label}.` or `{page label} follows your detail level.`
4. **Reset**: `Button variant="secondary"` "Reset to Detailed" sends `{ profile: "detailed", pages: {} }` (toast "Display reset to Detailed.");
   `disabledReason` "Already using the defaults." when the profile is Detailed and no override exists.
5. A final `InlineMessage tone="info"`: "These choices change what is shown, never what is allowed. Approvals, what is happening now,
   failures and anything that needs you are always shown."

Account menu (`SidebarFooter.tsx`): after the "Theme" items and before "Log out", a group **"Detail level"** with three items
(`Detailed`, `Standard`, `Essential`; `group: "Detail level"`, `checked` = the owner's default profile from `usePresentationChoice()`), each
saving `{ profile, pages: <current pages> }` through `useActionFeedback` (success `Detail level set to {label}.`), then an entry
"Display settings" (icon `gear`) that navigates to `/settings/display`. The chosen item changes after the hub answers (no optimistic
write).

## 7. Ordered implementation steps

Work in `web/` for web steps and at the repository root for hub steps. Commit in small green steps (one-line Conventional Commits, exactly one
scope, for example `feat(hub): per-user display preferences`), after steps 3, 6, 7, 8, 10 and 11 (each step names its commit). Run the
scoped checks of each area after every step you commit.

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Confirm 29 and 30 are merged: `crates/domovoi-hub/src/services/` (with `Caller`, `Services`, `ServicesDeps`), `web/src/features/operations/`,
     `web/src/app/rightSlot/`, `web/src/features/approvals/ApprovalCard.tsx`, `web/scripts/smoke.mjs`, `web/src/test/operationFixtures.ts`
     (30's helper over the five trace fixtures). **If any is missing, stop and report.** Write down every real name that differs from
     section 3 (for example `ServicesDeps` fields, the file names inside `features/operations/`, how 30 named the inspector blocks).
   - Read `crates/domovoi-hub/src/api/mod.rs` (`Authed`, `routes()`, `ApiDoc` tags), `api/rules.rs` (handler and DTO style), `services/mod.rs`,
     `services/caller.rs`; confirm `Authed::caller()` exists. Read `web/DESIGN.md` completely, `web/src/components/index.ts`,
     `Section.tsx`, `Menu.tsx`, `SegmentedControl.tsx`, `inputs.tsx` (`Select`), `Count.tsx`, `SidebarFooter.tsx`, `features/settings/
     {SettingsPage,ReadOnlySettings}.tsx`, `sections/views.ts`, `src/test/{apiMock,render,setup}.ts*`.
   - Read prompt 30's code you will edit completely: `StepOutline.tsx`, `StepRow.tsx`, `steps.ts`, `StepInspector.tsx`, `InspectorSections.tsx`,
     `OperationDetails.tsx`, `OperationDetailPage.tsx`, `OperationHeader.tsx`, `ApprovalCard.tsx`, `OperationsTable.tsx`.
   - Run the web suite once for the baseline (`npm test`) and the hub scoped tests (`cargo nextest run -p domovoi-hub`) so you know what was
     green. List the tests that render `Layout` (`layout.test.tsx`, `routes.test.tsx`, `palette.test.tsx`): they will now call
     `GET /api/v1/me/preferences`.
   - `grep -rn "preferences" crates web/src` (nothing should exist) and `ls crates/domovoi-store/migrations/` (you add nothing).
2. **Hub service.** `services/preferences.rs` with the types, the reading and writing rules of 6.1 and the unit tests of 12.1 (`InMemoryStore`,
   `FakeClock`). Wire `Services` and `ServicesDeps`. Run `cargo nextest run -p domovoi-hub preferences`.
3. **Hub endpoint.** `api/preferences.rs`, the tag and routes in `api/mod.rs`, the three `ServicesDeps` constructions, `tests/hub/preferences.rs`
   and its registration, the gate in `tests/gates.rs`. `cargo xtask openapi && cd web && npm run gen:api`, then `cargo insta review` (read the
   whole diff: two paths, three schemas, one tag, nothing else). `cargo fmt --all`, `cargo clippy -p domovoi-hub --all-targets -- -D warnings`,
   `cargo nextest run -p domovoi-hub`, `cargo xtask arch-check`. Commit `feat(hub): per-user display preferences`.
4. **The pure table.** `src/design/visibility.ts` and `visibility.test.ts` (12.2). Nothing else imports it yet.
5. **Kit.** `visibilityContext.tsx`, `Disclosure.tsx`, `Visible.tsx`, `DetailsScope.tsx`, `visibility.module.css`, the `index.ts` block, then `kit` tests of
   12.2. Tokens only (no raw colour, no inline style, no native `title`). Run `npx vitest run src/components src/design`.
6. **Data layer and provider.** `keys.preferences`, `types.ts` aliases, `hooks/preferences.ts`, `visibilityCache.ts`, `VisibilityProvider.tsx`, mount in `Layout.tsx`
   (one wrapper line inside `HostProvider`), tests of 12.2. Run `npx vitest run src/app src/api`; fix only tests that assert an exact list of
   API calls or menu items (add `route("GET /api/v1/me/preferences", ...)` where a layout test needs it; a missing route alone only fails that query).
   Commit `feat(web): visibility profiles and the preferences hooks`.
7. **Where the person chooses.** `DisplaySection.tsx`, `ProfileTable.tsx`, the `views.ts` entry, the `ReadOnlySettings` block, the account menu group;
   tests of 12.2. Commit `feat(web): display settings and detail level switch`.
8. **Pure step logic.** `stepVisibility.ts` and its tests (12.3). Commit `feat(web): step categories for the operations timeline`.
9. **Classify the operations screens.** Run `npx vitest run src/features/operations src/features/approvals src/features/incidents` first and keep it green after
   each file: **no existing test may change** (they render with no provider, which is Detailed). Edit in this order: `ApprovalCard`, `OperationHeader` and
   banner and waiting and stale note, `OperationDetails`, `StepGroup`/`StepOutline`/`StepRow`, the inspector blocks, the list page and table,
   `IncidentOperations`. Add `VisibilityPage` to the two pages. Move nothing; wrap.
10. **Tests of the product.** The matrix and `p0_visible_in_every_profile` (12.3), the gate scans (12.3), the demos (`VisibilityDemos.tsx`, `visibilityDemos.test.tsx`;
    `DesignPage.tsx`). Look at `/dev/design` in both themes. Commit `feat(web): classify the operations screens by visibility category`.
11. **Smoke test.** Update 30's mock hub and checks (12.4); `npm run build` then `npm run smoke`. Commit `test(web): browser smoke test across detail levels`.
12. **Docs.** `DESIGN.md`, `AGENTS.md`, the book pages (section 15); `cargo xtask docs-config` is not needed (no config key); `cargo xtask docs-links`.
13. **Checks and completion report.**
    - Hub: `cargo fmt --all`; `cargo clippy -p domovoi-hub --all-targets -- -D warnings`; `cargo nextest run -p domovoi-hub`; `cargo xtask arch-check`.
    - Web: `npm run format` then `npm run format:check`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run build` (record the new `npm run size`
      numbers: the kit and the table are small, but 30 left little room; if a limit is exceeded stop and report), `npm run smoke`, `npm run gen:api` followed
      by `git diff --exit-code -- src/api` (must be clean after the regeneration you committed).
    - By hand in the browser at 375, 768, 1280 and 1920 px, both themes, keyboard only, in all three profiles on the operation page with a pending
      approval (start the dev hub in WSL as `TESTING.md` says; if you cannot, say so).
    - Write the completion report in the standard structure.

## 8. Database and persistence

**No migration.** The preference is one row of the existing `settings` table (`key TEXT PRIMARY KEY, value TEXT NOT NULL`, from
`0001_init.sql`), written through the existing `SettingsRepo::set` port. This is the hub's established place for small JSON state that
is not a domain aggregate (`host.<id>.profile`, `hub_host_id`); it is not a `StateWrite` aggregate, needs no journal row and takes no
unit of work (nothing else must commit with it).

| Item | Rule |
|---|---|
| Key | `preferences.<user id>` (`UserId`'s `Display`, a UUID). One row per user. The prefix `preferences.` is owned by `PreferenceService`; no other code reads or writes it (gate, 12.1) |
| Value | JSON object: `version` (1), `updated_at` (RFC 3339), `presentation { profile, pages }`. Other top-level keys, if a later prompt adds them, are preserved by `update` |
| Absent row | the defaults: Detailed, no overrides, `updated_at` null. `get` does not create the row |
| Transaction boundary | one `SettingsRepo::set` call (one statement, atomic). The read-modify-write is serialised inside the process by the service's mutex |
| Backup and restore | the `settings` table is part of the database backup; nothing to add. A restored database keeps its preferences |
| N-1 upgrade | there is nothing to migrate: a database from before this prompt has no `preferences.*` row and reads as defaults. Test `old_database_without_preferences_reads_as_defaults` opens a store that never wrote the key. After a downgrade the older hub ignores the row; it never reads `preferences.*` (it lists no prefix) |
| Retention and deletion | the row stays until overwritten; no retention class (it holds no operational data). It is not deleted when a session ends |
| Secrets | none: the value holds only profile names and page keys |

**Browser storage** is used once, as a cache: `localStorage["domovoi.visibility"]` (6.7). It is read only by `VisibilityProvider`, wrapped
in try and catch, validated by `parsePresentation`, and never trusted for anything but the first paint.

## 9. Protocol and compatibility

No agent protocol change (wire stays 1.2, RW-42), no change to any existing response, status code, error text, audit kind, journal kind,
SSE topic or approval route. The only API change is the two new operations `get_preferences` and `update_preferences` and their schemas
(`PreferencesDto`, `PresentationDto`, `PreferencesUpdateDto`) and the tag `me`. Both are additive: a client that never calls them is
unaffected.

Compatibility inside the pair web and hub (they ship together, but a dev server may point at an older hub): an older hub answers 404 on
`/me/preferences`; the provider treats any failure as "no answer" and shows Detailed, and the Display panel shows `LoadError`. A newer hub
that stores a profile name this web build does not know sends it as a string; `parsePresentation` reads it as Detailed (shows everything).
A newer web that sends a page key an older hub does not list is accepted (decision 4).

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `GET /me/preferences` fails in the provider (network, 401, 404, 5xx) | the provider keeps the cache or Detailed; no banner, no toast, no retry loop (the query's own single retry for 5xx). A 401 is handled by the app's existing unauthorized handling |
| `GET /me/preferences` fails on the Display panel | `LoadError` "load display settings" with Retry; never shown as the defaults |
| The stored row is damaged (not JSON object, wrong types) | `get` answers the defaults for the damaged parts (6.1 rule 2), no error, nothing written; the next `PUT` replaces the `presentation` entry |
| `PUT` with an unknown profile, a malformed page key or more than 32 pages | 400 `bad_request` with the text of 6.1; nothing written; the control that sent it shows the error through `useActionFeedback` (toast), and the value shown stays the server's |
| `PUT` store failure | 500 `internal`; the toast says "Could not save the detail level"; nothing changes on screen |
| A save is in flight and the person clicks another choice | each click sends its own `PUT`; the last answered wins; the UI shows whatever the cache holds after each answer |
| Two tabs save different values | last write wins; the other tab shows the new value at its next focus refetch (staleTime 60 s) |
| A hub value names a profile the web does not know | read as Detailed (shows everything) |
| A `Visible` P2 or P3 block is rendered with `hidden` presentation and no `DetailsScope` above | rendered as collapsed (reachable) |
| A P0 element is given an `id` and `category` whose parent is a collapsed `Visible` | not detectable at run time (the child is never mounted): the matrix test 12.3 fails because the id is missing in Standard; fix by moving the P0 element out of the collapsed parent |
| `localStorage` unavailable or full | the cache is skipped; the profile still comes from the hub |
| `?step=` names a step inside a collapsed group (Standard or Essential) | the group opens with the step visible (`reveal`); the inspector opens as in prompt 30 |
| A step has a problem outcome in a profile that collapses its kind | it is P0 and renders in the list, outside any group |
| Preferences load after the first paint and change the profile | the page re-renders with the new presentation; open disclosures keep their own state; no scroll jump is introduced by the provider (blocks only add or remove collapsed rows below the viewport top) |
| Orchestration is off (list empty) | the operations list shows its P0 empty state in every profile |

## 11. Security requirements

- **Display never gates authority (the main requirement).** The profile changes what is drawn, not what may happen. The hub never reads
  `preferences.*` outside `services/preferences.rs` and `api/preferences.rs`; no engine, store, policy, approval or audit code does (gate
  `display_preferences_never_reach_authority`, 12.1). The browser never uses the profile to decide whether to send a request: no hook
  under `src/api/` imports visibility, and `StepApproveControl.tsx`, `ApproveControl.tsx`, `OperationBanner.tsx` and `banner.ts` contain
  no visibility primitive (gate `visibility_never_gates_approval_or_banner_code`, 12.3). Approve and Deny exist, with their typed
  confirmation and plan hash, in every profile (the P0 test clicks Approve in Essential).
- **P0 is structurally protected.** `presentationFor` returns "shown" for P0 before it looks at the table; `Visible` for P0 and P1 has no
  props that could hide or collapse it; an unknown profile, a missing provider and a damaged preference all produce Detailed; the table
  test asserts P0 "shown" and P1 "shown" in all three profiles.
- **Fail visible, never fail hidden.** Every error path in 10 ends in showing more, not less.
- **Per-user isolation.** Identity comes from the session through `Authed` and `Caller` only; no route or service method takes a user id; a
  unit test and an HTTP test prove one user's `PUT` does not change another's `GET`.
- **Validation and abuse.** The profile is a closed set of three lowercase names; page keys match a strict shape; at most 32 entries; echoed
  bad input is cut to 40 characters; the request body is bounded by the router's existing body limit; the stored value is built by the
  service, never copied from the request body. The web renders page labels only from its own `VISIBILITY_PAGES`; an unknown key received
  from the hub is preserved on save but never rendered as markup.
- **CSRF and session.** `PUT` passes the router's CSRF guard and needs a session (`Authed`); a request without either is refused before the
  service runs (HTTP test).
- **Secrets and logs.** No secret is stored or logged; the service logs nothing about values; a store failure is logged by
  `ApiError::internal` without the body.
- **Audit.** None, by decision (non-goals): a preference changes nothing that is recorded or authorised.
- **Hidden is not secret.** A collapsed or hidden block still comes from the same API answer; hiding is presentation. Do not use a profile as
  redaction, and do not skip fetching or redacting data because a block is collapsed (the inspector keeps fetching the step, whose `reason`
  is P0).

## 12. Testing requirements

Name every test after the behaviour. No test sleeps: hub tests use `FakeClock` from `domovoi-testkit`; web tests use `findBy…` and fake timers.
Web page and component tests use `renderAt` (or `renderWithProfile`, 12.2) and `apiMock`; **axe runs automatically** over whatever a UI test
leaves on screen and no test of this prompt calls `skipAxe()`. Fixtures for the operations screens are prompt 30's: the five trace fixtures
in `web/src/test/fixtures/operation-trace/*.json` through `web/src/test/operationFixtures.ts` (introduced by 30, verify); expectations are
derived from the fixture data (counts of steps, kinds, outcomes) with a **small independent helper written in the test**, never by calling
the function under test.

### 12.1 Hub

Unit tests inside `crates/domovoi-hub/src/services/preferences.rs` (`InMemoryStore`, `FakeClock`). They call the private `read_for(UserId)` and
`write_for(UserId, ..)`, **never building a `Caller`** (do not add a test constructor to `caller.rs`; `get` and `update` are one-line wrappers
covered by the HTTP tests below). In the table, `get` and `update` stand for those two functions:

| Test | Setup, action | Expected |
|---|---|---|
| `a_person_without_a_stored_row_gets_the_defaults` | empty store, `get` | `Detailed`, no pages, `updated_at None`; no row written |
| `update_stores_the_profile_and_page_overrides_and_get_returns_them` | `update(standard, {operation: essential})`, then `get` | both read back; `updated_at` = fake now |
| `update_stamps_the_time_from_the_clock` | advance the fake clock, update again | `updated_at` moves with the clock |
| `update_replaces_the_presentation_group_and_keeps_other_stored_keys` | store `{"version":1,"presentation":{...},"saved_views":{"a":1}}` by hand, `update` | `saved_views` still there; presentation replaced |
| `update_with_no_presentation_changes_nothing` | `update(PreferencesUpdate::default())` | returns the stored value; the row is byte-identical |
| `update_rejects_an_unknown_profile_and_writes_nothing` | profile `"compact"` and `"Detailed"` and `""` | `Invalid` naming the three valid names; row unchanged |
| `update_rejects_a_malformed_page_key` | keys `"Operation"`, `"1op"`, `""`, `"a b"`, 33 letters, `"é"` | each `Invalid`; nothing written |
| `update_accepts_a_well_formed_page_key` | `"op-1_x"`, `"a"`, 32 letters | stored |
| `update_rejects_more_than_32_pages` | 33 entries | `Invalid("at most 32 ...")`; 32 entries accepted |
| `update_rejects_an_unknown_profile_for_a_page` | `pages {operation: "tiny"}` | `Invalid` naming the profile |
| `echoed_bad_input_is_cut_to_40_characters` | profile of 500 characters | the message holds at most 40 of them |
| `reading_a_damaged_row_gives_defaults_for_the_damaged_parts` | rows: `"text"`, `[]`, `{}`, `{"presentation":3}`, `{"presentation":{"profile":7}}`, `{"presentation":{"profile":"x"}}` | `get` succeeds with Detailed each time; nothing written |
| `reading_drops_overrides_with_unknown_profiles_or_bad_keys` | pages `{good: standard, Bad: standard, other: "x"}` | only `good` remains |
| `reading_keeps_at_most_32_overrides_in_key_order` | 40 entries | 32, the first in key order |
| `one_person_cannot_see_or_change_another_persons_preferences` | users A and B; A updates to essential | B's `get` is Detailed; B's update does not change A; the two rows have different keys (`preferences.<A>`, `preferences.<B>`) |
| `concurrent_updates_keep_a_whole_valid_document` | two `update` futures joined | the stored value parses and equals one of the two requests |
| `old_database_without_preferences_reads_as_defaults` | a store holding only `config` and `hub_host_id` rows | defaults; those rows unchanged after an `update` |
| `the_store_failing_is_an_error_not_defaults` | a `SettingsRepo` wrapper whose `get` fails | `Err(Store(_))`, not defaults (a failed read must not overwrite on the next write; the write path also errors) |

HTTP tests in `crates/domovoi-hub/tests/hub/preferences.rs` (harness `api()`, `a.admin()`):

| Test | Expected |
|---|---|
| `preferences_require_a_session` | `GET` and `PUT` without login: 401 |
| `put_without_the_csrf_token_is_refused` | cookie but no `x-csrf-token`: the same refusal `api_auth.rs` asserts for other unsafe routes; nothing stored |
| `a_new_owner_gets_detailed_and_no_overrides` | 200 `{"version":1,"updated_at":null,"presentation":{"profile":"detailed","pages":{}}}` |
| `put_saves_and_get_returns_it` | `PUT {presentation:{profile:"standard",pages:{operations:"detailed"}}}` 200 with `updated_at` = the harness clock; `GET` equals the `PUT` answer |
| `put_with_a_bad_profile_is_a_400_with_the_hub_text` | 400, `error` `bad_request`, message names the three valid names |
| `put_with_a_bad_page_key_is_a_400` | 400 |
| `put_without_presentation_is_a_no_op_that_returns_the_current_value` | body `{}` and `{"presentation":null}`: 200, unchanged |
| `put_ignores_unknown_body_keys` | extra keys: 200 |
| `saving_preferences_changes_no_setting_approval_or_audit_row` | `GET /api/v1/settings` identical before and after; the audit log has the same number of rows |
| `a_second_account_has_its_own_preferences` | if the harness can create and log in a second user (use `UserRepo::create` with a hash and `AuthState` login as the other tests do), A's `PUT` does not change B's `GET`; otherwise the unit test above is the proof and the report says so |
| `the_openapi_document_lists_the_preferences_operations` | `get_preferences` and `update_preferences` under `/api/v1/me/preferences`; schema `PresentationDto.profile` has type `string`; the existing `openapi_snapshot` is regenerated and reviewed |

Gate `display_preferences_never_reach_authority` in `crates/domovoi-hub/tests/gates.rs` (a source scan; extend, never weaken): the strings
`PreferenceService`, `preferences_key` and the literal `"preferences.` appear in no `.rs` file under `crates/*/src` except
`crates/domovoi-hub/src/services/preferences.rs`, `services/mod.rs` (construction), `api/preferences.rs` and `api/mod.rs` (route), and
`crates/domovoi-hub/src/runtime/mod.rs` only if `Services::new` is not where the service is built; and `services().preferences` is used only in
`api/preferences.rs`. Include a self-test of the scan on an in-memory sample that contains a forbidden use (as the other scans in `gates.rs` do).

### 12.2 Web: table, kit, provider, hooks, settings

| Test (file) | Expected |
|---|---|
| `p0_and_p1_are_shown_in_every_profile` (`design/visibility.test.ts`) | for all three profiles, `presentationFor(p, "P0")` and `("P1")` are `shown` |
| `table_matches_the_product_decision_exactly` | the nine non-trivial cells of 2: Detailed all shown; Standard P2 and P3 collapsed; Essential P2 collapsed, P3 hidden |
| `p0_is_shown_even_if_the_table_were_edited` | mutate a copy of `PRESENTATION` to hide P0 and call a wrapped lookup: `presentationFor` still answers `shown` (the P0 guard is in the function) |
| `visibility_of_has_exactly_one_flag_true` | each profile and category |
| `profile_meta_category_meta_and_words_cover_every_value` | non-empty label and hint for each id; sentence case; no exclamation mark |
| `is_profile_id_rejects_everything_but_the_three_names` | `""`, `"Detailed"`, `null`, `42` |
| `use_visibility_without_a_provider_answers_detailed` (`components/visibility.test.tsx`) | no provider: every category `shown` |
| `use_visibility_follows_the_provider_and_a_page_override` | default essential: P3 `hidden`; inside `VisibilityPage page="operation"` with override detailed: P3 `shown`; inside another page: `hidden` |
| `visible_shows_its_children_in_a_marked_wrapper_when_shown` | `data-visibility`, `data-visibility-id`, `data-presentation="shown"`; the wrapper has `display: contents` (class check) |
| `visible_collapsed_shows_a_toggle_with_label_and_count_and_does_not_mount_children` | Standard, P2: a button with `aria-expanded="false"`, label, count; a child with a mount spy was never rendered; click: children render, `aria-expanded="true"` |
| `visible_reveal_opens_a_collapsed_block_at_mount_and_when_it_turns_true` | `reveal` true at mount: open; false then true later: opens |
| `visible_hidden_inside_a_details_scope_renders_nothing_and_adds_to_the_plus_n_button` | Essential, two P3 blocks with counts 2 and 1: button "+3 details"; no block text on screen |
| `plus_details_reveals_every_hidden_block_and_hide_puts_them_back` | click: blocks show, button reads "Hide 3 details"; click: gone again |
| `plus_one_detail_is_singular_and_a_scope_without_hidden_blocks_has_no_button` | |
| `visible_hidden_without_a_details_scope_is_collapsed_not_dropped` | Essential, P3, no scope: toggle present |
| `p0_and_p1_visible_ignore_every_profile` | all three profiles: children rendered, `data-presentation="shown"` |
| `disclosure_li_variant_is_a_list_item_and_axe_clean_in_a_list` | `as="li"` inside `ol` |
| `disclosure_button_has_an_accessible_name_and_controls_its_region` | `aria-controls` points at the region; role group labelled |
| `visibility_attrs_returns_the_two_data_attributes` | |
| `parse_presentation_is_total` (`app/visibilityCache.test.ts`) | `null`, `3`, `{profile:"x"}`, `{profile:"essential",pages:{a:"standard",b:"zzz",c:5}}` -> Detailed or the valid parts only; never throws |
| `cache_round_trips_and_ignores_unreadable_storage` | `writeCache`/`readCache`; `localStorage.getItem` throwing -> `undefined`, no throw; garbage JSON -> `undefined` |
| `provider_uses_the_cache_until_the_hub_answers_then_the_hub_wins` (`app/visibilityProvider.test.tsx`) | cache standard, hub pending: probe shows standard; hub answers essential: probe shows essential and the cache now says essential |
| `provider_shows_detailed_when_the_hub_fails_and_nothing_is_cached` | `failure(500...)`: probe shows detailed; no toast |
| `provider_reads_an_unknown_profile_from_the_hub_as_detailed` | |
| `the_layout_mounts_the_provider_once` (`layout.test.tsx`) | `GET /api/v1/me/preferences` is called once for the shell |
| `preferences_hooks_send_the_documented_requests` (`api/hooks/preferences.test.ts`) | `GET` path; `PUT` body `{presentation:{profile,pages}}`; the query cache holds the answer after `PUT`; the second consumer does not refetch |
| `display_section_shows_the_saved_profile_and_the_table` (`features/settings/displaySection.test.tsx`) | segmented control checked = hub value; table cells equal `PRESENTATION_WORDS[...]`; the P0 row reads "Shown" three times |
| `choosing_a_profile_saves_it_keeping_the_page_overrides_and_says_so` | `PUT` body has the new profile and the current pages including a key the build does not list; toast "Detail level set to Standard." |
| `choosing_a_page_override_sets_or_clears_that_key_only` | Select values `""` and `"essential"` |
| `reset_is_disabled_with_a_reason_when_already_default_and_resets_otherwise` | `disabledReason`; click sends `{detailed, {}}` |
| `a_failed_save_says_so_and_keeps_the_shown_value` | `failure(400, "bad_request", ...)`: toast, control unchanged |
| `a_failed_load_is_a_load_error_not_the_defaults` | `LoadError` "load display settings" with Retry |
| `display_is_reachable_in_a_file_managed_hub` (`settings.test.tsx`) | `editable: null` settings: a "Display" block with the control is on screen |
| `display_is_a_section_in_the_editable_settings` | `/settings/display` renders the panel; the section list has "Display" |
| `account_menu_offers_detail_level_with_the_current_one_checked_and_saves_a_choice` (`layout.test.tsx`) | group "Detail level" with three radio items; choosing Essential sends `PUT`; "Display settings" navigates to `/settings/display` |
| `visibility_demos_render_every_primitive_in_every_profile` (`dev/visibilityDemos.test.tsx`) | demos of `Visible`, `DetailsScope`, `Disclosure` under `ProfileProvider` for all three profiles; axe runs (no `skipAxe`) |

Test helper `web/src/test/visibility.tsx` **(new, fixed names)**:

```tsx
export function ProfileProvider(props: { profile: ProfileId; pages?: Record<string, ProfileId>; children: ReactNode }): JSX.Element; // supplies VisibilityContext directly
export function renderWithProfile(profile: ProfileId, url: string, path: string, element: ReactNode, pages?: Record<string, ProfileId>): ReturnType<typeof renderAt>;
export function p0Elements(root?: ParentNode): HTMLElement[];       // querySelectorAll('[data-visibility="P0"]')
export function expectP0Everywhere(options: {
  render: (profile: ProfileId) => Promise<{ unmount: () => void }>;   // render the screen and wait until it is ready
  expectedIds: readonly string[];                                       // P0 ids the fixture must produce (non-empty)
}): Promise<void>;
```
`expectP0Everywhere` renders the screen once per profile (Detailed, Standard, Essential), and each time asserts: every expected id occurs among the
`[data-visibility="P0"]` elements; every P0 element is connected and has **no ancestor** matching `[data-presentation="collapsed"]`, `[hidden]`,
`[aria-hidden="true"]`; and the **count of P0 elements per id equals Detailed's** (so a profile can never remove one). It fails when `expectedIds` is empty
(a test that proves nothing must not pass). It deliberately does not test `[inert]`: when the right slot is a modal sheet the page behind it is inert by
design (prompt 30), which is not a profile effect. Tests that open the inspector render it beside the page, with prompt 30's slot test helper
(`src/test/rightSlot.tsx`, measured width 1300), so the page is not inert.

### 12.3 Web: the operations screens (files in `src/features/operations/`, `src/features/approvals/`, `src/features/incidents/`)

Pure logic (`stepVisibility.test.ts`):

| Test | Expected |
|---|---|
| `kind_decides_the_base_category` | table-driven over the 11 kinds with `succeeded`: approval, execution, verification, question P0; assessment, plan, policy_decision, notification, note P1; evidence_request, reasoning_turn P2 |
| `a_problem_outcome_makes_any_kind_p0` | every kind × `failed`, `refused`, `not_run`, `unavailable`, `outcome_unknown`: P0 |
| `running_pending_skipped_and_cancelled_keep_the_base_category` | every kind × these: the base |
| `unknown_kind_is_p1_and_unknown_outcome_is_p0` | `"from_the_future"`, `"odd"` |
| `reason_category_is_p0_for_problem_and_unknown_outcomes_else_p1` | |
| `tree_category_is_the_most_important_of_the_step_and_its_descendants` | a P2 parent with a P0 grandchild is P0; a P2 parent with only P2 children is P2; a missing parent makes a root |
| `group_steps_groups_consecutive_non_shown_items_and_leaves_the_rest_single` | profile standard over a made-up list `[P0,P2,P2,P1,P2,P0]` -> single, group(2), single, group(1), single |
| `group_steps_never_groups_anything_in_detailed` | every item single |
| `group_steps_never_puts_a_p0_or_p1_item_in_a_group` | property over all kinds × outcomes × profiles |
| `group_steps_preserves_order_and_every_item_exactly_once` | flatten equals input |

Screens (`visibilityScreens.test.tsx`, with `renderWithProfile`):

| Test | Expected |
|---|---|
| `detailed_renders_the_screens_exactly_as_before` | the existing operation, approval and incident suites pass **unchanged** (run them first); plus: in Detailed no `[data-presentation="collapsed"]` exists on any operations screen |
| `standard_folds_consecutive_technical_steps_into_one_group_each_and_keeps_every_other_row` | `waiting-approval` fixture: number of groups and of single rows equal the independent helper's; the group toggle reads "{n} technical steps"; every non-technical row is still a row |
| `opening_a_group_shows_its_rows_in_order_with_their_outcomes` | click the toggle: rows appear, each with its outcome pill |
| `a_technical_step_with_a_problem_outcome_is_not_folded_in_any_profile` | fixture mutated so an evidence step is `refused`: it is a top-level row in Standard and Essential |
| `a_step_link_to_a_technical_step_opens_its_group_and_the_inspector` | URL `?step=<technical id>` in Essential: the group is open, the row is `aria-current`, the inspector shows it |
| `tree_view_does_not_fold_a_technical_parent_that_has_a_p0_child` | mutated trace |
| `essential_hides_operation_metadata_behind_plus_3_details_and_the_button_reveals_it` | "+3 details"; Kind, Started and Ended absent; click: all three; "Hide 3 details" |
| `standard_collapses_operation_metadata_into_a_toggle_with_the_count` | a toggle named "Kind and times" with count 3; its three facts are not on screen until it is opened |
| `pinned_subjects_budgets_and_incidents_stay_shown_in_every_profile` | text present in all three |
| `banner_and_waiting_for_you_are_identical_in_every_profile` | `textContent` of the banner and of the "Waiting for you" section equal across profiles, for every fixture that has them |
| `the_operations_list_is_the_same_in_every_profile` | rows and states identical |
| `inspector_in_standard_collapses_connections_intent_and_output_and_keeps_outcome_and_summary` | three toggles; outcome pill and summary shown; the step request is still made |
| `inspector_reason_of_a_failed_step_is_shown_in_essential` | `question-and-budget-pause` fixture, a `refused` step: the reason `InlineMessage` is on screen |
| `inspector_times_and_phase_are_behind_plus_details_in_essential` | "+2 details" reveals them |
| `approval_card_is_complete_in_every_profile` | skill, host, risk, check, target, arguments, expiry, `Plan abababab`, "v2", Approve and Deny present in all three profiles |
| `approving_in_essential_still_asks_for_the_typed_confirmation_and_sends_the_shown_plan_hash` | destructive variant: click Approve (dialog with consequence), confirm disabled until the exact skill name, then `POST ... approve` with `plan_hash` and `confirm_text` |
| `a_page_override_beats_the_default_for_that_page_only` | default essential, override `operation: detailed`: detail page shows metadata and technical rows; with the override on `operations` only the detail page stays essential |
| `the_profile_never_changes_which_requests_are_made` | `calls` of the detail page with the inspector open are the same set in Detailed and Essential, apart from the preferences request |
| `p0_visible_in_every_profile` | **the named test (06)**: for each of the five fixtures, `expectP0Everywhere` over `/operations/<id>` with the inspector open on its execution step if it has one (else its first step), with `expectedIds` computed by the independent helper: `operation.title` always; `operation.controls` when the status is not terminal; `operation.banner` when the fixture's banner row (30, 6.11) says so; `operation.waiting` when an execution awaits approval or a question is pending; `operation.progress` when running or queued; `operation.scope` always; `operation.step` once per step whose independent category is P0 (and that count is asserted equal to the number of `[data-visibility="P0"][data-visibility-id="operation.step"]` elements); `approval.card` once per awaiting execution plus once more for the inspector's read-only card; `inspector.outcome` always; `inspector.approval` for an execution step; `inspector.reason` when the step has a problem outcome and a reason. Also run it for the list page (`operations.table`, `operations.state`) and for the incident page's Investigations rows (`incident.investigation-status`). At least one fixture must make `operation.banner`, one `operation.waiting` and one a problem step, so the test cannot pass vacuously |
| `p0_visible_in_every_profile_with_a_page_override` | the same assertions with default Detailed and override Essential for `operation`, and the reverse |

Gates (`src/features/operations/visibilityGate.test.ts`, reading sources with `import.meta.glob("../../**/*.{ts,tsx}", { query: "?raw", eager: true })`,
excluding tests):

| Test | Expected |
|---|---|
| `visibility_never_gates_approval_or_banner_code` | `StepApproveControl.tsx`, `ApproveControl.tsx`, `OperationBanner.tsx`, `banner.ts` contain none of `useVisibility`, `useProfile`, `Visible`, `presentationFor`, `DetailsScope`, `visibilityAttrs` |
| `no_api_code_imports_visibility` | nothing under `src/api/` imports `design/visibility` or the visibility kit |
| `visible_usage_follows_the_rules` | every `<Visible` in `src/features` and `src/app` has a string-literal `id` matching `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$`, has `category` as a literal, `label` when the category is P2 or P3, and no `reveal` on P0 or P1 |
| `every_p0_id_of_the_table_appears_in_the_sources` | the P0 ids of 6.4 (`operations.table`, `operations.state`, `operation.title`, `operation.controls`, `operation.stale-note`, `operation.schema-warning`, `operation.banner`, `operation.progress`, `operation.waiting`, `operation.scope`, `inspector.outcome`, `inspector.approval`, `approval.card`, `incident.investigation-status`) each occur as an `id` in a source file (the code and the table agree) |

### 12.4 Smoke test (extends prompt 30's `npm run smoke`)

`scripts/smoke/server.mjs` answers `GET /api/v1/me/preferences` (state: `{version:1, updated_at:null, presentation:{profile:"detailed",pages:{}}}`) and
`PUT` (stores the body, answers it, records it); the "any other `/api` request fails the run" rule stays. New check, after the nine of prompt 30
(each prints one line): **10. Detail levels**: for each profile (detailed, standard, essential) × each colour scheme (`emulateMedia({ colorScheme })`)
× width 1280 and 375: set the mock's preference, open `/operations/<waiting id>`, and assert: the banner "Waiting for your approval" and the approval card (with
`Plan xxxxxxxx`, "v2" and an enabled Approve) are visible; in Detailed the technical rows of the fixture are listed and in Standard and Essential a "technical steps" toggle
stands in their place; in Essential "+N details" is present under Details; `documentElement.scrollWidth <= clientWidth`; axe in the real browser reports no
violations (including colour contrast). **11. Switching**: at 1280 px choose Essential in the account menu: the captured `PUT` body is
`{"presentation":{"profile":"essential","pages":{}}}` and the page changes without a reload. No `console.error`, no unexpected request.

### 12.5 Guardrails that must keep passing unchanged

`guardrails.test.ts`, `cssGuard`, `contrast`, `voice`, `tone`, `messages`, the 250-line cap, the no-inline-style, no-native-title and no-raw-button rules (the kit's
`Disclosure` and `DetailsScope` use a raw `button` as `Section` does, inside `src/components`, which the lint allows), `approvals.test.tsx` and every prompt 30 test.

## 13. Acceptance criteria

1. `PRESENTATION` in `src/design/visibility.ts` equals the table of section 2, and `presentationFor` answers `shown` for every P0 whatever the profile or
   table (tests of 12.2).
2. `useVisibility(category)`, `useProfile()`, `visibilityAttrs`, `VisibilityPage`, `Visible`, `DetailsScope` and `Disclosure` exist with the signatures and
   behaviour of 6.3; without a provider every category is `shown`; a collapsed block is not mounted until opened; a hidden block is always reachable
   (inside a `DetailsScope` through "+N details", else collapsed).
3. `GET` and `PUT /api/v1/me/preferences` behave as 6.2: per signed-in user, strict validation with 400 and the documented text, a no-op for an absent
   `presentation`, unknown body keys ignored, session and CSRF required; the HTTP and unit tests of 12.1 pass.
4. The preference is stored under `preferences.<user id>` in the existing `settings` table through `SettingsRepo`; **no migration**, no `Cargo.toml`
   change, no wire change; a database without the row reads as Detailed; reading never errors on damaged content.
5. The write goes through `crate::services::PreferenceService` taking `&Caller`; the handler only parses, calls and maps; the gate
   `display_preferences_never_reach_authority` passes; arch-check passes.
6. `VisibilityProvider` is mounted once in `Layout`; the hub's value wins over the browser cache; a failed load leaves Detailed (or the cache) and shows
   nothing; an unknown profile reads as Detailed.
7. The person can change the detail level in one click from the account menu and in Settings, Display (default profile, per-page overrides, the derived
   table, Reset), also when a file manages the hub's settings; each change reports through `useActionFeedback`; a failed load of the panel is a `LoadError`.
8. The operations list, the operation detail page, the details block, the step outline, the inspector, the approval card and the incident page's
   Investigations section carry the categories of 6.4; **Detailed renders them as prompt 30 built them and every prompt 30 test passes unchanged.**
9. In Standard and Essential, consecutive technical steps (evidence requests and model turns without a problem outcome) fold into one "N technical steps"
   disclosure per run; a step with a problem outcome never folds; a `?step=` link into a folded run opens it; the tree never folds a parent that has a more
   important descendant; Details metadata (kind, started, ended) is collapsed in Standard and behind "+3 details" in Essential; the inspector's connections,
   intent and output are collapsed in Standard and Essential and its times behind "+N details" in Essential.
10. The named test `p0_visible_in_every_profile` passes for the five trace fixtures, the list page and the incident page, with a non-empty expected set, and
    again with a per-page override in both directions.
11. In Essential an approval can still be approved exactly as in Detailed: the card shows skill, host, risk, arguments, expiry, plan-hash prefix and binding
    label; the dialog asks for the typed confirmation when required; the request carries the shown plan hash.
12. The profile never changes what is requested from the hub (a test compares the requests), no approval, banner or API code reads it (gate scans), and no
    hub code outside the two preference files reads the preference.
13. Every new UI test passes axe without `skipAxe()`, and the real-browser smoke test checks every profile, both colour schemes, at 375 and 1280 px, including
    colour contrast, plus the account-menu switch.
14. `cargo xtask openapi && npm run gen:api` was run, the insta snapshot was read and accepted, and `npm run gen:api` then leaves no diff.
15. Every new source file respects its line cap; no raw colour, inline style, native `title` or raw `<button>` was added outside `src/components`; icons only
    through `Icon`; `npm run lint`, `typecheck`, `test`, `format:check`, `build` (size within budget) and `smoke`, and the hub's fmt, clippy, nextest and
    arch-check, pass.
16. `web/DESIGN.md`, `AGENTS.md`, the book pages and `/dev/design` are updated (section 15).

## 14. Migration and compatibility

- **Existing installs and data.** Nothing to migrate. The first load after the upgrade finds no row and shows Detailed, which is the screens the
  owner already has (a Detailed operation screen is exactly prompt 30's). Standard and Essential are chosen, never imposed.
- **Existing tests.** Tests that render `Layout` now see one extra `GET /api/v1/me/preferences`. A missing route only fails that query and the screen
  stays Detailed. Edit a test only if it asserts an exact list of calls or of account-menu items (then register the route, or the new group).
  No operations, approval or incident test is edited.
- **Prompt 30's smoke mock.** 30's mock hub fails on any unlisted `/api` request; this prompt adds the preferences route to it. Any prompt that adds a
  new shell-level request must do the same.
- **Prompt 30's `ApprovalCard`, `ActionCard` and the Approvals page** keep their output in Detailed; the legacy `approvals.test.tsx` is untouched.
- **Prompt 36 (Operator) and later screens.** Each classifies its own elements with 6.5, wraps its page in `VisibilityPage`, adds its page key to
  `VISIBILITY_PAGES` and its P0 ids to its own `p0_visible_in_every_profile` case through `expectP0Everywhere`. 38 (graph) keeps the non-canvas outline and
  the approval card P0, marks the canvas P1 and its node sheet like the inspector. 43, 48–51 follow the same recipe; none of them edits `PRESENTATION`.
- **Hub prompts that run later** add their own services next to `PreferenceService`; none may read `preferences.*` (the gate).
- **Roles later (D-13).** Preferences are keyed by user; a role system adds roles to identities and may supply a default profile per role by reading
  `get` first and falling back, with no route or schema change here.

## 15. Documentation requirements

Write for the product's readers; never mention prompts, prompt numbers, the build plan or agents in any committed file.

- **`web/DESIGN.md`** (same PR): (a) New section **"Presentation profiles"** after "The shell" (and the "Planned" paragraph of the same name is **removed**,
  as is "the profile provider and visibility primitive" from "New kit components"): the table of section 2, the seven rules, Custom per page, default Detailed,
  where the person chooses (account menu, Settings, Display), the hub-stored per-user preference with the browser cache, the P0 guarantee and the test
  `p0_visible_in_every_profile`. (b) A **"Classify an element"** recipe (6.5, the four questions and the hard rules) and the category tables of 6.4 for the operations
  screens. (c) "Components": add `Visible`, `DetailsScope`, `Disclosure`, `VisibilityPage` (what each is for and not for) and the hooks `useVisibility`, `useProfile`.
  (d) "How people use it": a short "Choose how much to see" paragraph (one click from the account menu; Essential still shows everything that needs you). (e) "Checks
  that will fail your PR": rows for "A P0 element missing or collapsed in a profile" (`npm test`, fix: take it out of the collapsed container; `p0_visible_in_every_profile`)
  and "Visibility primitive in approval, banner or API code" (`npm test`, fix: remove it; the profile never gates authority). (f) "Performance budget": record the new
  `npm run size` numbers. (g) "Recipes": "Add a setting" stays for hub settings; add a line that per-person display choices are preferences, not settings fields.
- **`AGENTS.md`** (committed; keep in step), under "Web UI": one bullet: every element of an operations-style screen carries a visibility category P0 to P3 through
  `Visible`, `visibilityAttrs` or `useVisibility`; P0 (safety, approval, audit, current state) is never hidden or collapsed in any profile; no approval, banner or request
  code may read the profile; see `web/DESIGN.md`.
- **Book.** New page `docs/book/src/guide/display.md`, "Detail level", linked from `SUMMARY.md` under Guides after Retention: what Detailed, Standard and Essential
  show, what is always shown (approvals and what needs you, what is happening now, failures and unknown results), where to change it (account menu, Settings, Display),
  per-page choices and Reset, that the choice is saved with the account on the hub and is the same in every browser, and that it changes only what is shown, never what
  is allowed. `docs/book/src/reference/api.md`: a "Preferences" section (`GET` and `PUT /api/v1/me/preferences`, the body, the 400 text, per-user). `docs/book/src/
  reference/data-model.md`: the `settings` row also holds each account's display preferences. Link only to pages that exist; run `cargo xtask docs-links`.
- **OpenAPI**: `cargo xtask openapi && cd web && npm run gen:api`, then `cargo insta review` for the hub snapshot. **Config reference**: no config key, so
  `cargo xtask docs-config` is not needed. **`ARCHITECTURE.md`**: no change (the route family and the `settings` table are already described; no new crate edge, module
  zone or invariant). No ADR (no arch rule, boundary or invariant changes).

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

Also state in this report: every real name from prompts 29 and 30 that differed from section 3 (`ServicesDeps`, the file and component names of the operations
screens, the inspector blocks); the new `npm run size` numbers; the number of P0 elements
per fixture that `p0_visible_in_every_profile` asserted; which existing tests you had to edit (there should be none for operations, approvals and incidents,
and only call-list or menu-list assertions elsewhere); and whether the hub HTTP test for a second account ran or was replaced by the unit test.
