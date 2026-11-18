# 48 — Knowledge pages: facts, proposals, management paths and experience

| | |
|---|---|
| Branch | `knowledge-pages` |
| Milestone | U4 |
| Depends on (must be merged) | 37 (visibility categories and profiles), 34 (knowledge store and `/api/v1/knowledge/*`), 35 (summary, similar operations and experience search endpoints and the web hooks in `hooks/experience.ts`), 30 (operations UI: shell, right slot, operation and incident pages), and through them 26–29, 31, 32 and 59. All of 26–37 are written and may not be merged when this brief is read: **every name taken from them is "introduced by prompt NN — verify in code before use"**. If 34, 35, 37 or 30 is not merged, stop and report |
| Can run in parallel with | 43, 49, 50, 51 (order-flexible, never concurrent). They share `web/src/app/{pages,routes,nav}.ts(x)`, `design/tone.ts`, `design/visibility.ts` (`VISIBILITY_PAGES`), `api/{queryClient,types}.ts`, `web/DESIGN.md`, `scripts/smoke/*`, `scripts/screens.mjs` and `src/dev/DesignPage.tsx`; whichever runs second rebases onto the first and keeps both sets of entries |
| Produces (shared contracts) | The Knowledge pages (`/knowledge`, `/knowledge/facts/:id`, `/knowledge/entities/:id`, `/knowledge/paths`, `/knowledge/experience`), the hooks `web/src/api/hooks/knowledge.ts` and `knowledgeWrites.ts`, the vocabularies `KNOWLEDGE_STATUSES`, `PATH_STATES`, `COVERAGE_STATES` with `knowledgeStatusMeta`, `pathStateMeta`, `coverageMeta`, the step-up helper `useStepUp` (`features/knowledge/useStepUp.tsx`), the embeddable `ResolutionSummary` and `SimilarOperations` panels, and the four page keys `knowledge`, `knowledge-detail`, `paths`, `experience` appended to `VISIBILITY_PAGES` |
| Consumes | 34: the generated knowledge DTOs and routes (section 3.2). 35: the generated experience DTOs and `useResolutionSummary`, `useSimilarOperations`, `useExperienceSearch`, `useEditSummaryField`. 37: `useVisibility`, `Visible`, `DetailsScope`, `Disclosure`, `VisibilityPage`, `visibilityAttrs`, `VISIBILITY_PAGES`, the helpers `renderWithProfile` and `expectP0Everywhere`, the rule of its section 6.5. 30: `OperationDetailPage`, `IncidentDetailPage` slots, `operationStatusMeta`, the smoke test, `src/test/operationFixtures.ts`. Existing: the kit, `PasswordPrompt`, `useActionFeedback`, `describeError` |
| Launch gate | none. Research item R-10 (helper attestation) is **not** resolved here: coverage is shown as unavailable until a later prompt supplies a real `ManagementCoverage` |
| Suggested model | medium: many small, well-specified files; the judgement is in keeping coverage honest, the password rule exact and every category correct |

## 1. Objective

Give the person who runs the servers one honest place to see and correct **what Domovoi knows**, built only
on the routes that the knowledge store and the experience search already publish. No Rust, no wire change, no
migration: this prompt is web code plus tests.

- **Knowledge browser** (`/knowledge`): facts with source, status and freshness, filterable by host (the shell's
  host selector), predicate, source and status; full-text search; a **Proposals** tab (facts Domovoi inferred
  that wait for the owner's word); **Relations** and **Entities** tabs. A retracted fact is hidden unless the
  owner asks for retracted ones.
- **Fact detail** (`/knowledge/facts/:id`) with provenance (source, who recorded it, evidence), status
  (current, stale, disputed, replaced, retracted), freshness times, competing values and history, and the owner
  actions: **accept** (make it mine), **verify**, **retract**, and **assert** a new fact. **Entity detail**
  (`/knowledge/entities/:id`) with its facts and relations, rename and delete.
- **Management paths** (`/knowledge/paths`, D-28): per host, the paths unattended containment must not cut off,
  with state (candidate, unconfirmed, confirmed, stale), provenance, freshness, **coverage** and **what is
  blocked**. Coverage cannot be verified in this release (the hub's coverage hook always answers
  "unavailable"), so the page says so, disables Confirm with the reason, and **never shows any path as covered**.
- **Experience** (`/knowledge/experience`): owner search over finished operations (one host by default; all
  hosts only when asked), plus two embeddable panels: the **resolution summary** of a finished operation (fields
  with the source of each, confirm or edit of the narrative fields) and **similar past operations**, mounted on
  the operation page and the incident page. Everything that shows a past operation carries the label
  **"historical evidence, not authorization"**.
- **Safety-critical writes ask for the password.** Asserting, accepting or verifying a management path or a
  trusted CIDR, and every path confirmation, go through the existing `PasswordPrompt`. Retracting never does.
- **Every element is classified** with the P0 to P3 rule of the visibility prompt (decision controls and
  states are P0) and the four pages join `VISIBILITY_PAGES`, so Detailed, Standard and Essential all work and
  `p0_visible_in_every_profile` covers them.

With no knowledge yet, an old hub, or orchestration off, nothing breaks: lists show their empty states, and a
failed load is a `LoadError`, never an empty list.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**RW-14 / 04 §10 (what a fact is).** A fact is `(subject, predicate, value)` with a **source** (`user`,
`discovered`, `integration`, `inferred`, `imported`), a **status**, `observed_at`, `valid_until`,
`last_verified_at`, a `supersedes` link and an evidence reference. There is no numeric confidence. Subjects are
strings: `host:<id>`, `container:<host id>/<name>`, `unit:<host id>/<name>`, `entity:<id>`; an **entity** is a
thing the owner invents ("Home VPN"). Predicates are a closed set: `role`, `criticality`, `owner_note`,
`depends_on`, `management_path`, `maintenance_window`, `backup_location`, `never_restart`, `trusted_cidr`,
`host_platform`. Statuses: `current`, `stale` (past its freshness period, computed on read), `disputed` (two
sources disagree on a single-valued predicate until the owner chooses), `superseded` (replaced, kept as
history), `retracted` (withdrawn; hidden from lists, search and model context; its content is purged after the
retention period and only a tombstone stays). A **relation** links two subjects (`depends_on`, `runs_on`,
`routes_to`, `stores_on`, `backs_up_to`, `managed_from`); it is never disputed. The hub owns all of these rules;
the UI only displays them and sends the owner's actions.

**Trust boundary for writes.** Only the signed-in human can write a `user` fact. The HTTP request types carry
**no `source` and no `actor`** (unknown fields are a 400), so the UI never sends them. `inferred` never becomes
`user` except through the owner's **accept**, which saves a new `user` fact and keeps the old one as history.
A fact is advisory input: nothing in this prompt lets a fact authorize an action.

**Safety-critical predicates and step-up (34).** `management_path` and `trusted_cidr` are safety-critical: a
fact of these kinds is used for a safety decision only when it is `user`-sourced **and** current. The hub
therefore demands the owner's password for `POST /facts`, `accept` and `verify` on them and for **every**
`confirm`. Wrong or missing password is 403 (nothing written); a lockout is 429. `retract` never needs it
(it only removes trust). The UI asks first, with the existing `PasswordPrompt`, and never stores the password.

**D-28 / RW-37 (management paths).** Unattended containment on a host needs owner-confirmed, **current**
management paths; unknown, stale, conflicting or unverifiable means refused. A path can be confirmed only when
a verified, fresh helper attestation shows it is covered; the hub never trusts an agent's description of the
helper. The attestation work is a later prompt. Until then the hub ships a coverage hook that always answers
"cannot be verified": `GET /hosts/{name}/management-paths` returns `coverage_available: false`, and every
`confirm` answers 409 "the helper's coverage cannot be verified yet" with nothing written. The UI shows: the
paths, their coverage, their provenance, their freshness and **what is blocked**. Monitoring and work the owner
approves personally are not affected, and the page says so. **The UI never decides or enforces the block**: it
displays a conclusion derived from the hub's data; the hub (and later the policy evaluator) enforces.

**Coverage is shown honestly.** The API exposes one boolean, `coverage_available`, and a per-path `state`; it
has no per-path coverage verdict. So this release renders coverage as exactly two states: *unavailable*
("cannot be verified yet": the only one reachable today) and *checked when you confirm* (the hook is
available). The words "covered" and "protected" appear nowhere as a status of a path. A `confirmed` path says
"Confirmed by you on <date>" and nothing about current coverage.

**RW-25 / D-02 (experience is evidence, never authorization).** Every finished operation gets a resolution
summary: five fields filled by code (`outcome`, `verified`, `subjects`, `contracts_executed`, `hosts`) and three
narrative fields (`problem`, `cause`, `fix`) drafted from the model's own report (`source = model`), which the
owner may confirm or edit (`source = user`). Revisions are append-only and each records who wrote it. Similar
past operations and the owner search return operations with the fixed label **"historical evidence, not
authorization"** in a `label` field of every response. A past approval or success never lets anything run
without approval. Host scope is the default; a global search is something the owner explicitly asks for
(`scope=global`, no host).

**RW-21 / D-10 / 37 (profiles).** Detailed, Standard and Essential are one set of components with a
category-to-visibility table. P0 (safety, approval, audit, current state, failures, decision controls) is never
hidden or collapsed; P1 primary; P2 technical detail collapsed in Standard and Essential; P3 metadata collapsed
in Standard and behind "+N details" in Essential. Profiles are display only: they never change which requests
are made for P0 content and never gate an action. A P0 element is never inside a P2 or P3 container.

**RW-19 (use-case layer).** The browser only calls hub routes. This prompt adds no hub code.

**Live updates (28).** Knowledge journal rows map to **no** SSE topic, so these pages are not live. They
refetch after the owner's own change (every write invalidates the whole `knowledge` query prefix) and on focus.
They never claim to be live. The operation page's summary and similar-operations queries hang under the
operation's query key, so the operation journal frames refresh them without any code here (35).

**Decisions made here (do not revisit).**

1. **Proposals are inferred facts.** The knowledge store defines no "proposal" object. A proposal is a live fact
   whose source is `inferred` (`GET /facts?source=inferred`). Accepting it is the only way it becomes the
   owner's. A **discovered** management path is shown on the management paths page as a *candidate* with its own
   Accept; it is not repeated in Proposals. No second query and no client-side merge of sources.
2. **Retracted and replaced facts are reachable but never default.** Status chips: Live (default; the hub's
   default is current, stale and disputed), Current, Stale, Disputed, Replaced (`superseded`), Retracted. Only
   the last two show rows with no actions.
3. **Relation and entity detail.** The API has no `GET /relations/{id}` and no `GET /entities/{id}`. A relation
   opens in a dialog from its row (the row has the whole DTO); an entity has a page that finds the entity in
   `GET /entities` (an unpaged list) and loads its facts and relations by `subject=entity:<id>`. An unknown
   entity id is "No such entity". No deep link for a relation.
4. **Which predicates the owner may assert.** All except `host_platform`, which is observed, never asserted (the
   hub refuses it when the source is forced to `user`). The subject kinds each predicate allows follow the
   knowledge store's table (section 6.4); the hub stays the authority and its 400 text is shown.
5. **One password flow.** The existing `PasswordPrompt` (kit) is the only step-up UI. A form dialog that needs the
   password is replaced by the prompt (state kept), never stacked with it.
6. **Confirm is disabled, never hidden, while coverage is unavailable**, with a visible reason, and no request
   is sent. When `coverage_available` is true Confirm sends the request and shows the hub's answer.
7. **Similar operations on the incident page** appear only when the incident has at least one investigation
   (`IncidentDetail.operations` non-empty); with orchestration off the page is unchanged.
8. **Source words** (provenance labels) are plain text lookups in `design/knowledgeText.ts`, not statuses. Only
   statuses (knowledge status, path state, coverage) are vocabularies with tones.

## 3. Repository context

Marks: **(existing, path:line)**, **(new)**, or **(introduced by NN, verify)**. Paths are under `web/` unless they start with
`crates/` or `docs/`. Verify every line in step 1.

### 3.1 Existing web code this prompt builds on

- **Password prompt (the step-up pattern).** `src/components/PasswordPrompt.tsx:13-54` **(existing)**: props `reason`, `error?`,
  `busy?`, `confirmLabel?` ("Save"), `onConfirm(password)`, `onCancel`; a `ConfirmDialog` titled "Confirm with your password"
  with one `type="password"` field, Confirm disabled until something is typed, Enter submits; the password is passed to
  `onConfirm` and never kept. Used by `src/features/settings/EditorForm.tsx:107-128` **(existing)**: it calls the save with
  `password`, and on `ApiFailure` status 403 while asking sets the field error "That password is not right.", otherwise closes the
  prompt and shows the failure. `src/features/settings/form/sensitive.ts:17` `isSensitive(changes)` decides when to ask. Copy
  this behaviour, and the sentence "That password is not right.", exactly.
- **Mutations.** `src/components/useActionFeedback.ts` **(existing)**: `useActionFeedback(mutation, { success, error,
  errorDisplay?: "toast" | "inline", undo? })` returns `{ mutate(variables, { onSuccess?, onError?(rawFailure) }), isPending, error:
  DescribedError | undefined, reset }`; `success` may be a function returning a string, a `{title, tone: "info"}` toast or `null`.
  `mutate` never rejects. A failure inside a dialog uses `errorDisplay: "inline"`.
- **Errors.** `src/design/messages.ts:49-76` **(existing)**: the catalogue is by hub error **code** (`forbidden`, `not_found`,
  `bad_request`, `conflict`, `too_many_requests`, `unavailable`, ...); `conflict`, `forbidden`, `bad_request` and `unavailable` add
  the hub's own sentence as the detail. `ApiFailure` (`src/api/types.ts:97`) has `status`, `code`, `message`, `retryAfter`. A 429
  reads "Too many attempts. Try again in N s." from `retryAfter`. No new catalogue entry is needed for the routes here: if a route
  answers a code that is not in `HUB_CODES`, stop and report.
- **Host selector.** `src/app/HostContext.tsx:81` `useSelectedHost() -> [name | undefined, set]`; a `?host=` in the address selects
  a host. `src/app/AllHostsNote.tsx` says a page ignores the selector. `src/api/hooks/hosts.ts:32` `useHostNames()` (id to name map),
  `useHosts()`. `src/features/rules/HostField.tsx` is the model of a host `Select` that shows a `LoadError` when hosts cannot load.
- **Lists.** `src/api/hooks/incidents.ts:40-53` `useIncidents` is the model of an infinite cursor list (`useInfiniteQuery`,
  `initialPageParam: undefined`, `getNextPageParam: page.next_cursor ?? undefined`, `defined({... cursor, limit})`).
  `src/features/notes/NotesPage.tsx` is the model of a list page with search (`useDebounced`, `QueryBoundary`, `EmptyState`).
- **Shell.** `src/app/nav.ts:36-89` **(existing)**: `NAV_GROUPS`; the group titled "Knowledge" (`:76-83`) holds Known conditions, Notes and Digest.
  `src/app/pages.ts:27` `PAGES` map with `page(() => import(...))` entries. `src/app/routes.tsx:48-84` (`AppRoutes`) registers routes inside `<Layout/>`
  from `PAGES`, never importing a page directly. `src/app/PageTitle.tsx:45` `defaultDetail(section, rest)` has a `switch` per section;
  `usePageName(name)` names a page about one thing. The section name comes from the nav item whose `to` equals `/<first segment>`.
- **Query keys and types.** `src/api/queryClient.ts:5` `keys`. `src/api/types.ts:38` `export type Fact = Schemas["FactDto"]` is the **digest** fact
  (a host metric, used by `features/hosts`), and `:81` `Predicate = Schemas["PredicateDto"]` is the ack-rule predicate. **Do not reuse or rename
  these names**; the knowledge aliases of 6.2 are prefixed `Knowledge`.
- **Statuses.** `src/design/status.ts` (`Table<K>`, `StatusMeta {label, tone, hint, icon}`, `lookup()` gives an unknown value a neutral meta with the raw value);
  `src/design/tone.ts:188` `VOCABULARIES`; vocabulary tables sit in `src/design/*Vocab.ts`. Icons available (`src/design/icons.tsx`): `dashboard alert chat rules approve
  server tool note digest log gear menu user logout check x check-circle x-circle alert-circle clock pause play loader ban mute offline copy external refresh more close
  chevron-up chevron-down chevron-left chevron-right search filter plus trash edit eye eye-off skip sun moon monitor info help`. Use only these.
- **Kit** (import from `src/components/index.ts` only): `PageHeader`, `Section`, `Card`, `Tabs` (`id`, `count`, `selected`, `onSelect`; only the selected panel renders),
  `FilterChips`, `SegmentedControl`, `DataTable` (`columns` stable, `rowHref`, `rowKey`, `caption`, `empty`), `KeyValueList`, `StatusPill`, `Tag`, `Count`,
  `InlineMessage`, `Banner`, `Progress`, `QueryBoundary` (`query`, `context`, `loading`, `isEmpty`, `empty`), `LoadError`, `EmptyState`, `Skeleton`, `Button`
  (`disabledReason`, `hint`), `IconButton`, `LinkButton`, `Menu`, `Dialog`, `ConfirmDialog` (`requireText`, `confirmDisabledReason`, `busy`), `PasswordPrompt`,
  `Field`, `TextInput`, `Textarea`, `NumberInput`, `Select`, `Checkbox`, `Tooltip`, `InfoTip`, `RelativeTime`, `CopyBlock`, `Stack`, `Cluster`, `Text`, `useDebounced`.
  From the visibility prompt (introduced by 37): `Visible`, `DetailsScope`, `Disclosure`, `VisibilityPage`, `useVisibility`, `visibilityAttrs`.
- **Tests.** `src/test/apiMock.ts` **(existing)**: `route("GET /api/v1/...", handler)`, `calls`, `failure(status, code, message, headers?)`; `api.GET/POST/PUT/PATCH/DELETE`;
  an unregistered route throws `no test route for ...`. `src/test/render.tsx:15` `renderAt(url, path, element)`. `src/test/setup.ts` runs axe over whatever a UI test leaves on
  screen (`skipAxe()` only for a deliberate fragment) and clears `localStorage`. `src/test/fixtures.ts:7` `nas` host (`id: "host-nas"`, so subject refs in fixtures read
  `host:host-nas`: the UI must never assume a subject id is a UUID).
- **Scripts.** `scripts/screens.mjs:~30` `PAGES` array of `[name, path]`; takes four widths and both themes.
- **Docs.** `DESIGN.md` sections "The shell", "Components", "Planned" (its list of new statuses names "knowledge state"), "Checks that will fail your PR", "Performance budget", "Recipes".

### 3.2 What the earlier prompts publish (introduced by 34, 35, 37, 30: verify every name against the generated `src/api/schema.d.ts` and the merged code)

**Knowledge routes (34, section 6.9), all under `/api/v1/knowledge`, all need a session, unsafe methods carry the CSRF header the API client already sends:**

| Method and path | operation id | Request | Answer | Errors |
|---|---|---|---|---|
| `GET /facts` | `list_facts` | query `host` (a host **name**), `subject`, `predicate`, `source`, `status` (comma list), `q`, `limit` (default 50, max 200), `cursor` | `KnowledgeFactPageDto { items, next_cursor }` | 400 bad filter; 404 unknown host |
| `GET /facts/{id}` | `get_fact` | | `KnowledgeFactDto` | 404 (a malformed id is 404) |
| `POST /facts` | `assert_fact` | `KnowledgeFactInput { subject, predicate, value, evidence?, valid_until?, password? }` | 201 `KnowledgeFactDto` | 400; 403 password; 429 |
| `POST /facts/{id}/accept` | `accept_fact` | `KnowledgeFactAction { expected_version, password? }` | `KnowledgeFactDto` (the **new** user row) | 404; 409 not live or stale version; 403; 429 |
| `POST /facts/{id}/retract` | `retract_fact` | `KnowledgeFactAction` | `KnowledgeFactDto` | 404; 409 |
| `POST /facts/{id}/verify` | `verify_fact` | `KnowledgeFactAction` | `KnowledgeFactDto` | 404; 409 (also for a management path); 403; 429 |
| `POST /facts/{id}/confirm` | `confirm_management_path` | `KnowledgeFactAction` with `password` required | `ManagementPathDto` | 404; 409 not live, not owner fact, uncovered, unverifiable, stale version; 403; 429 |
| `GET /hosts/{name}/management-paths` | `list_management_paths` | | `ManagementPathsDto { host, coverage_available, paths }` | 404 unknown host |
| `GET /relations` | `list_relations` | query `subject`, `kind`, `include_retracted`, `limit`, `cursor` | `RelationPageDto { items, next_cursor }` | 400 |
| `POST /relations` | `assert_relation` | `RelationInput { from, kind, to, evidence? }` | 201 `RelationDto` | 400 |
| `POST /relations/{id}/retract` | `retract_relation` | `{ expected_version }` | `RelationDto` | 404; 409 |
| `GET /entities` | `list_entities` | | `EntityDto[]` | |
| `POST /entities` | `create_entity` | `{ kind, name }` | 201 `EntityDto` | 400; 409 duplicate name |
| `PUT /entities/{id}` | `rename_entity` | `{ name, expected_version }` | `EntityDto` | 404; 409 duplicate or stale |
| `DELETE /entities/{id}` | `delete_entity` | query `expected_version` | 204 | 404; 409 in use or stale |

Wire shapes (JSON, snake_case):

```json
// KnowledgeFactDto (status = the EFFECTIVE status now; stored_status = the row's; value is null once content_purged is true)
{ "id": "…", "subject": "host:…", "predicate": "role", "value": "media server", "source": "user", "status": "current",
  "stored_status": "current", "version": 1, "observed_at": "…", "valid_until": null, "last_verified_at": null,
  "fresh_until": "…", "supersedes": null, "evidence": "note:…", "actor": "user:…", "created_at": "…",
  "retracted_at": null, "content_purged": false }
// ManagementPathsDto / ManagementPathDto: the confirmation never carries evidence
{ "host": "nas", "coverage_available": false,
  "paths": [{ "fact": <KnowledgeFactDto>, "state": "unconfirmed", "confirmation": { "confirmed_by": "user:…", "confirmed_at": "…" } | null }] }
// RelationDto (from and to are null after the content purge)
{ "id": "…", "from": "container:…/plex", "kind": "runs_on", "to": "host:…", "source": "user", "status": "current",
  "stored_status": "current", "version": 1, "observed_at": "…", "evidence": null, "actor": "user:…", "created_at": "…",
  "retracted_at": null, "content_purged": false }
// EntityDto
{ "id": "…", "kind": "vpn", "name": "Home VPN", "created_by": "user:…", "created_at": "…", "version": 1 }
```

With `q` present the list runs a search: at most 50 results, **no cursor, no `next_cursor`**, other filters still narrow. A retracted fact listed with
`status=retracted` shows its value until purged. Error bodies are the existing `ErrorBody`. `value` is any JSON (`unknown` in the generated types).
`KnowledgeFactAction`, `KnowledgeFactInput` and `RelationInput` refuse unknown fields: **the web never sends `source` or `actor`** (a test proves it).

**Fact DTO names.** The knowledge schemas are `KnowledgeFactDto`, `KnowledgeFactPageDto`, `KnowledgeFactInput` and
`KnowledgeFactAction` (the knowledge prompt named them so); `FactDto` stays the existing digest-fact schema
(`crates/domovoi-hub/src/api/dto_admin.rs:83`, `src/api/types.ts:38`) and is not touched. Confirm the names in the generated
`schema.d.ts` in step 1; if the knowledge fact schema is still called `FactDto`, stop and report.

**Experience routes (35, section 6.9):** `GET /api/v1/operations/{id}/summary` (`ResolutionSummaryDto { operation_id, as_of_seq, state: "not_ready" | "drafting" | "ready",
ended_at, fields: SummaryFieldDto[], history: SummaryRevisionDto[], history_truncated, label }`; `SummaryFieldDto { field, value, source: "code" | "model" | "user", actor, revision,
created_at, editable }`), `POST /api/v1/operations/{id}/summary/{field}` (`EditSummaryFieldRequest { value, expected_revision }` -> `EditSummaryResponse { field, unchanged }`; 400
empty, too long or bad text; 403 code-owned; 409 not finished or stale revision; 404; 503 orchestration off), `GET /api/v1/operations/{id}/similar` and
`GET /api/v1/incidents/{id}/similar-operations` (`SimilarOperationsDto { items: SimilarOperationDto[], as_of_seq, label }`; `SimilarOperationDto { operation: PastOperationDto, score,
reasons: { code: "same_check" | "same_target" | "same_contract" | "similar_words", detail }[] }`), and `GET /api/v1/experience/search` (query `q`, `scope` = `host` (default) or `global`, `host` (a
name), `limit`; `ExperienceSearchDto { scope, host, query, items: { operation: PastOperationDto, match_pass, snippet }[], as_of_seq, label }`; `scope=host` without `host` is 400, `scope=global`
with a `host` is 400). `PastOperationDto { operation_id, goal, ended_at, outcome, verified, hosts: { id, name | null }[], contracts: { contract, host_id, target, outcome }[], narrative: SummaryFieldDto[] }`.
Every one of these carries `label`, always `"historical evidence, not authorization"`. Web hooks (35, `src/api/hooks/experience.ts`): `useResolutionSummary(operationId | undefined)`,
`useSimilarOperations({ operation } | { incident } | undefined, limit?)`, `useExperienceSearch({ q, scope: "host", host } | { q, scope: "global" } | undefined)`, `useEditSummaryField()`; fixtures
`src/test/fixtures/experience/{summary-ready,summary-drafting,similar,search-host,search-global}.json`. Verify the exact variables of `useEditSummaryField`.

**Visibility (37):** `src/design/visibility.ts` (`Category`, `ProfileId`, `PRESENTATION`, `presentationFor`, `VISIBILITY_PAGES: readonly { key; label }[]`), `src/components/visibilityContext.tsx`
(`useVisibility`, `VisibilityPage`), `Visible({ category, id, label?, count?, reveal?, children })`, `DetailsScope`, `Disclosure`, `src/test/visibility.tsx`
(`ProfileProvider`, `renderWithProfile(profile, url, path, element, pages?)`, `p0Elements`, `expectP0Everywhere({ render, expectedIds })`). Its rule (6.5): ask in order: P0 if the element says what
is happening now, what waits for the person, what authority applies, what was recorded, reports a failure, an unknown result, stale data, a load error, an empty or loading state, or **holds a
decision control**; else P1 main content or frequent action; else P2 diagnostic, raw data, relation, history, secondary action; else P3 identifier, time or other metadata. Hard rules: never a P0
inside a P2 or P3 `Visible`; list rows and table cells use `visibilityAttrs`, not a wrapper; a P3 block sits in a `DetailsScope`; dialogs, toasts, menus, tooltips carry no category; `id` is a
literal matching `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$`. Its test `visible_usage_follows_the_rules` scans `src/features` and `src/app`, so every `<Visible` written here is checked.

**Operations UI (30):** `src/features/operations/OperationDetailPage.tsx` (sections in order: header, stale note, all-hosts note, banner, progress, "Waiting for you", "Steps", "Details", the
inspector surface), `src/features/incidents/IncidentDetailPage.tsx` with the slot `data-slot="operations"` rendering `IncidentOperations` (`IncidentDetail.operations`), `operationStatusMeta`
(statuses `succeeded failed escalated cancelled` are also the four resolution outcomes), the smoke test `scripts/smoke.mjs` with `scripts/smoke/{server,data,checks}.mjs` (the mock hub answers an
exact list of `/api/v1/*` routes and **fails the run on any other**; 37 added `GET` and `PUT /api/v1/me/preferences`).

## 4. Exact scope

All paths under `web/`. Every source file at or under 250 lines (split by responsibility; the line cap has no exemption). **(new)** = does not exist yet.
No Rust file, migration, config key, OpenAPI file or wire contract changes.

### 4.1 Design vocabularies and data layer

| File | Change |
|---|---|
| `src/design/knowledgeVocab.ts` **(new)** | tables `KNOWLEDGE_STATUSES`, `PATH_STATES`, `COVERAGE_STATES` (6.1) |
| `src/design/knowledgeText.ts` **(new)** | plain words and rules: `PREDICATES`, `SOURCES`, `RELATION_KINDS`, `SUMMARY_SOURCES`, `SUMMARY_FIELDS`, `SAFETY_CRITICAL`, `actorLabel`, `needsPassword` (6.3) |
| `src/design/tone.ts`, `status.ts` | lookups `knowledgeStatusMeta`, `pathStateMeta`, `coverageMeta`; their tables join `VOCABULARIES`; the value types in `status.ts` |
| `src/api/types.ts` | the `Knowledge*` aliases of 6.2 (and the experience aliases if 35 did not add them) |
| `src/api/queryClient.ts` | keys of 6.2 |
| `src/api/hooks/knowledge.ts` **(new)** | read hooks (6.2) |
| `src/api/hooks/knowledgeWrites.ts` **(new)** | write hooks (6.2) |
| `src/test/knowledgeFixtures.ts` **(new)**, `src/test/fixtures/knowledge/*.json` **(new)** | shared fixtures (12.1) |

### 4.2 Knowledge pages (`src/features/knowledge/`, all **(new)**)

| File | What it holds |
|---|---|
| `KnowledgePage.tsx` | the page shell: `VisibilityPage page="knowledge"`, `PageHeader`, host note, `Tabs` (`?tab=` = `facts` (no parameter) / `proposals` / `relations` / `entities`) |
| `factFilters.ts` | pure: URL parameters <-> `FactFilters`, the status chips (6.5) |
| `FactsTab.tsx`, `FactFilterBar.tsx`, `FactsTable.tsx` | the facts list, its filters and search, its table |
| `ProposalsTab.tsx` | inferred facts with Accept and Retract |
| `RelationsTab.tsx`, `RelationsTable.tsx`, `RelationDialog.tsx`, `RelationForm.tsx` | relations list, detail dialog, add form |
| `EntitiesTab.tsx`, `EntityForm.tsx`, `EntityDetailPage.tsx`, `DeleteEntityDialog.tsx` | entities list, create and rename dialog, entity page |
| `FactDetailPage.tsx`, `FactHeader.tsx`, `FactProvenance.tsx`, `FactTimes.tsx`, `FactSiblings.tsx`, `FactHistory.tsx` | the fact page and its blocks |
| `FactActions.tsx`, `factActions.ts` | the owner's buttons (Accept, Verify, Retract) and the pure rule of which apply |
| `AssertFactDialog.tsx`, `ValueFields.tsx`, `SubjectField.tsx`, `valueForm.ts` | the add-a-fact form: per-predicate fields, subject picker, the pure value builder |
| `subjects.ts`, `useSubjectLabels.ts`, `SubjectLink.tsx`, `valueText.ts` | pure subject and value display helpers, and the hook that supplies host and entity names |
| `useStepUp.tsx` | the password flow over `PasswordPrompt` (6.6) |
| `PathsPage.tsx`, `PathsBanner.tsx`, `PathRows.tsx`, `PathActions.tsx`, `HostChooser.tsx`, `blocked.ts` | the management paths page, its banner, rows, actions, host chooser and the pure "what is blocked" rule |
| `knowledge.module.css` | placement only, tokens only |
| tests | section 12 |

### 4.3 Experience (`src/features/experience/`, all **(new)**)

| File | What it holds |
|---|---|
| `ExperiencePage.tsx`, `ExperienceFilters.tsx`, `ExperienceResults.tsx`, `PastOperationCard.tsx`, `experienceParams.ts` | the owner search page (`VisibilityPage page="experience"`) and the pure URL <-> parameters mapping |
| `Narrative.tsx`, `summaryValue.ts` | narrative lines with the source tag of each, and the pure text of a summary value |
| `ResolutionSummary.tsx`, `SummaryFieldRow.tsx`, `EditSummaryDialog.tsx`, `SummaryHistory.tsx` | the resolution summary panel: fields, confirm, edit, history |
| `SimilarOperations.tsx` | the similar-operations panel (operation and incident) |
| `OperationExperience.tsx` | the one component mounted on the operation page: summary (when finished) and similar operations |
| `experience.module.css` | placement only |
| tests | section 12 |

### 4.4 Shell, existing pages, catalogue, scripts, docs

| File | Change |
|---|---|
| `src/app/pages.ts` | lazy pages `knowledge`, `fact`, `entity`, `paths`, `experience` |
| `src/app/routes.tsx` | routes `/knowledge`, `/knowledge/facts/:id`, `/knowledge/entities/:id`, `/knowledge/paths`, `/knowledge/experience` from `PAGES`, inside `<Layout/>` |
| `src/app/nav.ts` | three items in the existing "Knowledge" group (6.7) |
| `src/app/PageTitle.tsx` | `defaultDetail`: section `knowledge` (6.7) |
| `src/design/visibility.ts` (37) | append four entries to `VISIBILITY_PAGES` (6.8) |
| `src/features/operations/OperationDetailPage.tsx` (30) | one import and one element `<OperationExperience .../>` after the Details section; nothing else |
| `src/features/incidents/IncidentDetailPage.tsx`, `incidents.module.css` | one slot `data-slot="experience"` after the operations slot (6.9, item 8) |
| `src/dev/KnowledgeDemos.tsx` **(new)**, `src/dev/DesignPage.tsx`, `src/dev/knowledgeDemos.test.tsx` **(new)** | catalogue demos (12.8) |
| `scripts/smoke/{server,data,checks}.mjs` (30, 37) | the mock answers the knowledge and experience routes; checks 12 to 14 (12.9) |
| `scripts/screens.mjs` | add `knowledge`, `knowledge-paths`, `experience` and, when `SCREENS_FACT` names an id, `fact` |
| `DESIGN.md` | section 15 |
| `../docs/book/src/concepts/knowledge.md`, `concepts/experience.md` (and `guide/display.md` if it lists pages) | section 15 |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Any Rust, hub route, DTO, migration, config key, wire change, OpenAPI edit (`web/src/api/openapi.json`, `schema.d.ts` stay as generated; `npm run gen:api` must leave no diff) | none; stop and report if one seems needed |
| The interactive graph of relations and entities, its keyboard model, playback | 38, 52. Relations are a **table** here |
| Helper views, the effective safety set, helper key enrollment and fingerprint screens, `helper.toml` instructions, per-path "covered or uncovered" judged against verified data | 47 (backend) and 49 (host page views). This page shows coverage only as the two honest states of section 2 |
| Implementing coverage, attestation, pinning, nonces, or any parsing of `CoverageEvidence` (the API does not return it) | 47 |
| Enforcing or computing the containment block anywhere but as display text; any policy or approval change | 39, 40, 45, 46 |
| A hub-side "proposal" object, a bulk accept, a "dismiss" other than retract | not defined by the knowledge store |
| A deep link or `GET` for one relation | the API has none (decision 3) |
| Live updates of knowledge (no SSE topic exists for it) | out of scope; pages refetch after the owner's own change and on focus |
| Global search across notes, facts, incidents and operations | 51 |
| Experience search for the **model**, `search_experience`, context sources, `ExperienceAccess::global_for_owner_conversation` | 31, 35, 36 |
| Editing code-owned summary fields, deleting or clearing a narrative field | forbidden by the summary rules (35) |
| Changing 37's table `PRESENTATION`, the `Visible`/`DetailsScope`/`Disclosure` kit, or 30's approval card, banners and step logic | out of scope |
| New kit components, new icons, new tokens, new dependencies | none needed. If one seems needed, stop and report |
| Persisting filter or tab choices outside the URL | per-viewer conveniences are not needed |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly) or **convention** (follow repository style). Every exported item gets a JSDoc saying what it is for. Never write a
prompt number, "the build plan" or an RW or D id into code, comments, the book, commit messages or the PR description: describe behaviour in plain words.

### 6.1 Vocabularies **fixed** (words, tones, icons, hints; sentence case, no exclamation marks)

`src/design/knowledgeVocab.ts` exports three `Table<…>` objects (the shape of `src/design/actionVocab.ts`); the lookups go in `tone.ts` through `lookup()`
so an unknown value is a neutral pill with the raw value; the three tables join `VOCABULARIES` (the existing `tone.test.ts` then checks them).

`KNOWLEDGE_STATUSES` (the **effective** status of a fact or relation):

| key | label | tone | icon | hint |
|---|---|---|---|---|
| `current` | Current | success | check-circle | Seen or confirmed within its freshness period. It is a hint for you and for Domovoi, never permission. |
| `stale` | Stale | warning | clock | Past its freshness period, so it may no longer be true. Verify it, or let a new observation refresh it. |
| `disputed` | Disputed | pending | alert-circle | Two sources give different values. Choose the one that is right. |
| `superseded` | Replaced | neutral | skip | A newer value replaced this one. It stays as history. |
| `retracted` | Retracted | neutral | ban | Withdrawn. It is not used anywhere, and its content is removed after the retention period. |

`PATH_STATES` (the `state` of a management path):

| key | label | tone | icon | hint |
|---|---|---|---|---|
| `candidate` | Not yours yet | info | help | Domovoi found or inferred this path. It is not used until you accept it. |
| `unconfirmed` | Waiting for you | pending | clock | You entered it but have not confirmed it. Confirming needs the helper's verified coverage. |
| `confirmed` | Confirmed | success | check-circle | You confirmed it. This says nothing about what the helper covers now. |
| `stale` | Stale | warning | clock | It is past its freshness period, or the network changed. Confirm it again. |

`COVERAGE_STATES` (keys `unavailable`, `checkable`; there is **no** "covered" key):

| key | label | tone | icon | hint |
|---|---|---|---|---|
| `unavailable` | Cannot be verified yet | warning | alert-circle | The check that shows the helper protects a path is not available in this version. Paths can be listed and accepted, but not confirmed. |
| `checkable` | Checked when you confirm | info | info | When you confirm a path, the hub asks the helper whether it covers it. This page does not show the answer in advance. |

```ts
// tone.ts (one line each)
export function knowledgeStatusMeta(status: string): StatusMeta;
export function pathStateMeta(state: string): StatusMeta;
export function coverageMeta(available: boolean): StatusMeta;   // lookup(COVERAGE_STATES, available ? "checkable" : "unavailable")
// status.ts: types KnowledgeStatus, PathState, CoverageState
```

### 6.2 Types, query keys and hooks **fixed** (names), **convention** (layout)

```ts
// src/api/types.ts: the right-hand schema names are the real ones found in step 1 (see "Name collision" in 3.2)
export type KnowledgeFact = Schemas["<fact dto>"];            export type KnowledgeFactPage = Schemas["KnowledgeFactPageDto"];
export type KnowledgeFactInput = Schemas["KnowledgeFactInput"];        export type KnowledgeFactAction = Schemas["KnowledgeFactAction"];
export type ManagementPath = Schemas["ManagementPathDto"];    export type ManagementPaths = Schemas["ManagementPathsDto"];
export type KnowledgeRelation = Schemas["RelationDto"];       export type KnowledgeRelationPage = Schemas["RelationPageDto"];
export type KnowledgeRelationInput = Schemas["RelationInput"]; export type KnowledgeEntity = Schemas["EntityDto"];
// src/api/queryClient.ts, added to `keys`
knowledge: ["knowledge"] as const,
facts: (filters: object) => ["knowledge", "facts", filters] as const,
factSearch: (filters: object) => ["knowledge", "fact-search", filters] as const,
fact: (id: string) => ["knowledge", "fact", id] as const,
managementPaths: (host: string) => ["knowledge", "paths", host] as const,
relations: (filters: object) => ["knowledge", "relations", filters] as const,
entities: ["knowledge", "entities"] as const,
```

Read hooks, `src/api/hooks/knowledge.ts`:

```ts
export const FACT_PAGE = 50;
export interface FactFilters { host?: string; subject?: string; predicate?: string; source?: string; status?: string /* comma list */ }
useFacts(filters: FactFilters, enabled = true)                  // useInfiniteQuery, keys.facts(filters); GET /api/v1/knowledge/facts, query defined({...filters, cursor, limit: FACT_PAGE})
useFactSearch(filters: FactFilters & { q: string }, enabled = true) // useQuery, keys.factSearch(filters); same route with q and limit 50, no cursor; idle while q.trim() is empty
useFact(id: string)                                             // useQuery, keys.fact(id); GET .../facts/{id}
useManagementPaths(host: string | undefined)                    // useQuery, keys.managementPaths(host); GET .../hosts/{name}/management-paths; idle without a host
export interface RelationFilters { subject?: string; kind?: string; include_retracted?: boolean }
useRelations(filters: RelationFilters, enabled = true)          // useInfiniteQuery, keys.relations(filters); GET .../relations, query defined({...filters, cursor, limit: FACT_PAGE}); include_retracted is sent only when true
useEntities()                                                   // useQuery, keys.entities; GET .../entities
```

Write hooks, `src/api/hooks/knowledgeWrites.ts`. Each is a `useMutation` over `call<T>(api.POST|PUT|DELETE(...))`; every `onSuccess` invalidates
`keys.knowledge` (a prefix: lists, details, paths, relations, entities) and nothing else; none sets query data by hand; none retries. **No body ever contains the keys `source` or
`actor`.** Optional fields are omitted (not sent as `undefined`).

| Hook | Variables | Request |
|---|---|---|
| `useAssertFact()` | `{ subject, predicate, value: unknown, evidence?, validUntil?, password? }` | `POST /facts` body `{ subject, predicate, value, evidence?, valid_until?, password? }` |
| `useAcceptFact()` | `{ id, expectedVersion, password? }` | `POST /facts/{id}/accept` body `{ expected_version, password? }` |
| `useRetractFact()` | `{ id, expectedVersion }` | `POST /facts/{id}/retract` body `{ expected_version }` (never a password) |
| `useVerifyFact()` | `{ id, expectedVersion, password? }` | `POST /facts/{id}/verify` body `{ expected_version, password? }` |
| `useConfirmPath()` | `{ id, expectedVersion, password: string }` (the password is **required** by the type) | `POST /facts/{id}/confirm` body `{ expected_version, password }` |
| `useAssertRelation()` | `{ from, kind, to, evidence? }` | `POST /relations` |
| `useRetractRelation()` | `{ id, expectedVersion }` | `POST /relations/{id}/retract` body `{ expected_version }` |
| `useCreateEntity()` | `{ kind, name }` | `POST /entities` |
| `useRenameEntity()` | `{ id, name, expectedVersion }` | `PUT /entities/{id}` body `{ name, expected_version }` |
| `useDeleteEntity()` | `{ id, expectedVersion }` | `DELETE /entities/{id}` query `{ expected_version }` |

### 6.3 Words and rules **fixed**, `src/design/knowledgeText.ts` (pure; no React)

```ts
export const HISTORICAL_LABEL = "historical evidence, not authorization";   // equals the `label` the hub sends on every past-operation response
export type SubjectKind = "host" | "container" | "unit" | "entity";
export interface PredicateInfo { label: string; hint: string; subjects: readonly SubjectKind[]; many: boolean; safety: boolean; assertable: boolean }
export const PREDICATES: Readonly<Record<string, PredicateInfo>>;       // the ten below; predicateInfo(key) returns { label: key, hint: "", subjects: all four, many: true, safety: false, assertable: false } for an unknown key
export const SAFETY_CRITICAL: ReadonlySet<string>;                      // exactly management_path, trusted_cidr
export type KnowledgeAction = "assert" | "accept" | "verify" | "retract" | "confirm";
export function needsPassword(action: KnowledgeAction, predicate: string): boolean;   // confirm: true; retract: false; others: SAFETY_CRITICAL.has(predicate)
export const SOURCES: Readonly<Record<string, { label: string; hint: string }>>;       // sourceInfo(key): unknown source -> { label: key, hint: "" }
export const RELATION_KINDS: Readonly<Record<string, string>>;           // sentence forms; relationPhrase(kind): unknown -> the raw kind
export const SUMMARY_FIELDS: Readonly<Record<string, string>>;           // field labels; NARRATIVE_FIELDS = ["problem", "cause", "fix"]
export const SUMMARY_SOURCES: Readonly<Record<string, { label: string; hint: string }>>;
export function actorLabel(actor: string, me: string | undefined): string;
```

`PREDICATES` (label; subject kinds; many; safety; assertable). The subject kinds copy the knowledge store's table; the hub stays the authority:

| key | label | hint | subjects | many | safety | assertable |
|---|---|---|---|---|---|---|
| `role` | Role | What this thing is for. | host, container, unit, entity | no | no | yes |
| `criticality` | Criticality | How much it matters: critical or warning. | host, container, unit, entity | no | no | yes |
| `owner_note` | Note | Something you want Domovoi to remember. | all four | yes | no | yes |
| `depends_on` | Depends on | Something it needs in order to work. | all four | yes | no | yes |
| `management_path` | Management path | A network you reach this host through. Protective actions must never cut it off. | host | yes | **yes** | yes |
| `maintenance_window` | Maintenance window | A time when work on it is expected. | all four | yes | no | yes |
| `backup_location` | Backed up to | Where its data is backed up. | host, container, unit, entity | yes | no | yes |
| `never_restart` | Never restart | Domovoi must not restart it. | container, unit | no | no | yes |
| `trusted_cidr` | Trusted network | A network that is known and expected. | host | yes | **yes** | yes |
| `host_platform` | Platform | The machine's system, kernel and size, observed by Domovoi. | host | no | no | **no** |

`SOURCES`: `user` "You" ("Entered or accepted by you."), `discovered` "Found by Domovoi" ("Observed on the host."), `integration` "From an integration" ("Reported by a connected service."),
`inferred` "Inferred by Domovoi" ("Worked out from evidence. It is not yours until you accept it."), `imported` "Imported" ("Brought in from a file or another system."). `RELATION_KINDS`:
`depends_on` "depends on", `runs_on` "runs on", `routes_to` "routes to", `stores_on` "stores on", `backs_up_to` "backs up to", `managed_from` "is managed from". `SUMMARY_FIELDS`: `outcome`
"Outcome", `verified` "Fix verified", `subjects` "What it was about", `contracts_executed` "What ran", `hosts` "Hosts", `problem` "Problem", `cause` "Cause", `fix` "Fix". `SUMMARY_SOURCES`: `code`
"Filled in by Domovoi", `model` "Drafted by the model, not checked", `user` "Written or confirmed by you". `actorLabel`: `user:<id>` equal to `me` is "You"; any other `user:` is "Another account";
`system:knowledge` "Domovoi (knowledge)"; `system:profile` "Domovoi (host profile)"; `system:operations` "Domovoi (operations)"; `agent:<id>` "A host's agent"; anything else the raw string.

### 6.4 Pure helpers **fixed** (names, inputs, outputs), all unit-tested

```ts
// features/knowledge/subjects.ts
export interface ParsedSubject { kind: SubjectKind; host?: string; name?: string; id?: string }   // host: host id; container/unit: host + name; entity: id
export function parseSubject(ref: string): ParsedSubject | undefined;   // "<kind>:<rest>"; container and unit split <rest> at the FIRST "/"; ids are opaque (never assume a UUID); anything else undefined
export function formatSubject(subject: ParsedSubject): string;           // inverse of parseSubject
export interface SubjectNames { hosts: ReadonlyMap<string, string>; entities: ReadonlyMap<string, string> }
export function subjectLabel(ref: string, names: SubjectNames): { text: string; to?: string };
//   host -> the host name ("an unknown host" when absent), to /hosts/<name>; container and unit -> "<name> on <host label>", to the host; entity -> the entity name ("a removed entity"), to /knowledge/entities/<id>;
//   a malformed ref -> { text: ref } and no link. A ref is shown as data, never as markup.
// features/knowledge/valueText.ts
export function valueText(predicate: string, value: unknown, names: SubjectNames): string;
//   null or undefined -> "". management_path and trusted_cidr {cidr} -> the cidr. maintenance_window {starts_at, ends_at, note?} -> "2026-11-20 02:00 to 2026-11-20 04:00 UTC (note)".
//   host_platform {hostname, os, kernel, arch, cpu_count, mem_total_bytes} -> "nas · Debian 12 · 6.1.0 · x86_64 · 4 CPUs · 15.6 GiB" (GiB to one decimal).
//   never_restart true -> "Never restart". depends_on string -> subjectLabel(...).text. any other string -> itself. Anything else, or a known shape with missing keys -> compact JSON cut at 400 characters with a trailing "…". Never throws.
// features/knowledge/factActions.ts
export interface FactActionSet { accept: boolean; verify: boolean; retract: boolean; confirmElsewhere: boolean }
export function factActions(fact: { status: string; source: string; predicate: string; content_purged: boolean }): FactActionSet;
//   live = status is current, stale or disputed (the effective status). accept = live && source !== "user" && !content_purged. verify = live && predicate !== "management_path".
//   retract = live. confirmElsewhere = live && predicate === "management_path" && source === "user" (the page points at the paths page).
// features/knowledge/valueForm.ts
export interface ValueDraft { predicate: string; text: string; cidr: string; criticality: string; startsAt: string; endsAt: string; note: string; dependsOn: string }
export type BuiltValue = { ok: true; value: unknown } | { ok: false; problem: string };
export function buildValue(draft: ValueDraft): BuiltValue;
//   role: trimmed text, 1 to 80 characters. owner_note 1 to 1000. backup_location 1 to 200. criticality: "critical" or "warning". never_restart: true. depends_on: a non-empty subject ref (formatSubject of the picker).
//   management_path and trusted_cidr: the trimmed non-empty cidr text AS TYPED (the hub canonicalises; the page then shows what the hub stored). maintenance_window: startsAt and endsAt are
//   "YYYY-MM-DDTHH:mm" read as UTC, sent as "<value>:00Z"; the end must be after the start; note optional, 1 to 200 characters. host_platform and unknown predicates: { ok: false }.
//   `problem` is one short plain sentence shown under the field ("Write 1 to 80 characters.", "The end must be after the start.").
// features/knowledge/factFilters.ts
export type StatusChip = "live" | "current" | "stale" | "disputed" | "superseded" | "retracted";
export const STATUS_CHIPS: readonly Chip<StatusChip>[];                 // labels "Live", "Current", "Stale", "Disputed", "Replaced", "Retracted"
export function statusParam(chip: StatusChip): string | undefined;      // "live" -> undefined (the hub's default is current, stale and disputed); otherwise the status word
export interface FactView { status: StatusChip; source: string; predicate: string; q: string }
export function readFactView(params: URLSearchParams): FactView;        // ?status= ?source= ?predicate= ?q=; an unknown or empty value is the default (live, "", "", "")
export function writeFactView(previous: URLSearchParams, change: Partial<FactView>): URLSearchParams;   // defaults leave the address; every other parameter (tab, host) is kept
// features/knowledge/blocked.ts
export interface BlockedSummary { blocked: boolean; reasons: string[] }
export function containmentBlock(input: { coverageAvailable: boolean; states: readonly string[] }): BlockedSummary;
```

`containmentBlock` (**fixed**): `reasons` in this order, each present only when true: coverage unavailable ("The helper's coverage cannot be verified yet."); no path at all ("No management paths are recorded for this host.");
`n` candidates ("{n} path(s) found by Domovoi are not accepted by you yet."); `n` unconfirmed ("{n} path(s) are not confirmed."); `n` stale ("{n} path(s) are stale."); `n` in an unknown state
("{n} path(s) are in a state this page does not know."). `blocked = reasons.length > 0`. It is **not blocked only when** coverage is available, there is at least one path, and every path is `confirmed`.

### 6.5 The Knowledge page and its tabs **fixed** (behaviour), **convention** (markup)

`KnowledgePage`: `VisibilityPage page="knowledge"`; `PageHeader title="Knowledge" intro="What Domovoi knows about your servers, where each fact came from, and what waits for your word."
docs="concepts/knowledge.html"`. `Tabs label="Knowledge"` controlled by `?tab=` (`facts` = no parameter, `proposals`, `relations`, `entities`; unknown value reads as `facts`; `replace`). The Proposals tab shows a
`count` (`label: "waiting"`, tone `pending`) only while above zero, from the **same query** the tab uses (`useFacts({ source: "inferred", host })`), so no extra request. Relations and entities do not follow
the host selector: on those two tabs `AllHostsNote` says "Relations and entities are shown for every host."

**Facts tab** (`FactsTab`): toolbar: `FilterChips label="Status"` over `STATUS_CHIPS`; `Field label="Source"` with a `Select` ("Any source" and the five `SOURCES` labels); `Field label="Kind"` with a `Select` ("Any
kind" and every `PREDICATES` label); `Field label="Search"` with a `TextInput` debounced 300 ms (`useDebounced`) into `?q=`; a primary `Button icon="plus"` "Add a fact" (opens `AssertFactDialog`). All state is in the URL
via `readFactView`/`writeFactView`. Data: `host` = `useSelectedHost()`; with `q` blank `useFacts({ host, predicate, source, status: statusParam(chip) })`, otherwise `useFactSearch` (the other query is disabled).
`FactsTable` (`DataTable`, stable `columns`, `rowHref = /knowledge/facts/<id>`): **Fact** (predicate label strong, subject label muted as `SubjectLink`; a `Tag` "Safety-related" inside a `Tooltip` "Used for safety decisions only
when you entered it and it is current." for safety predicates), **Value** (`valueText`, two lines clamped; a purged fact says "Removed after the retention period."), **Source** (`Tag`, label from `SOURCES`, hint in a
`Tooltip`), **Status** (`StatusPill size="sm"`, `knowledgeStatusMeta(status)`), **Updated** (`RelativeTime` of `last_verified_at ?? observed_at`). Default and chip `live` never request retracted or replaced rows.
`QueryBoundary`: `Skeleton shape="rows"`; failure is `LoadError context="load facts"` (never the empty state); empty: with no filter `EmptyState title="No facts yet"` ("Domovoi records what it learns here, and you can add
what you know. Facts it is not sure of wait for your word.") with the Add button; with a filter `title="No facts match"` ("Try a different status, source or search.") and a "Clear filters" `Button`; chip `retracted`:
"No retracted facts". "Load more" `Button` while `hasNextPage` (`loading` while fetching). A search shows a muted line "Showing the best 50 matches. Narrow the search to see others." when 50 came back.

**Proposals tab**: toolbar-free. One `Card` per proposal (`useFacts({ source: "inferred", host })`, newest first, "Load more"): predicate label and `SubjectLink`, the value (`valueText`), "Inferred by Domovoi <RelativeTime>", the
evidence reference as a link when its kind is `note` (`/notes/<id>`), `incident` (`/incidents/<id>`) or `operation` (`/operations/<id>`) and plain text otherwise, the status `StatusPill`, then **Accept** (`Button`, `hint`
"Saves it as your own fact. The original stays as history.") and **Retract** (`Button variant="secondary"`, `ConfirmDialog` as in 6.9). Accept goes through `useStepUp` and `needsPassword("accept", predicate)`. Empty:
`EmptyState title="Nothing waits for your word"` ("When Domovoi infers something, it appears here until you accept or retract it."). The `knowledge.waiting-note` (P0) on the Facts tab says "N facts inferred by Domovoi wait for your
word." with a link to `?tab=proposals`, only while N is above zero (the same shared query).

**Relations tab**: toolbar `Field label="Kind"` `Select` ("Any kind" and the six kinds), `Checkbox` "Show retracted" (`include_retracted`), `Button icon="plus"` "Add a relation" (`RelationForm`: `SubjectField` From, `Select` kind
from `RELATION_KINDS`, `SubjectField` To; 400 shows inline through `useActionFeedback` with `errorDisplay: "inline"`). `RelationsTable`: **Relation** (a ghost `Button` whose text reads "plex runs on nas" from `subjectLabel` and `relationPhrase`, opening
`RelationDialog`; after the content purge "Removed after the retention period."), **Source**, **Status**, **Updated**, actions column: **Retract** (live rows only; `ConfirmDialog`, no password). `RelationDialog` is a read-only
`KeyValueList` (From, Relation, To, Source, Status, Observed, Evidence, Recorded by, Recorded, Retracted) with Retract inside when live. Loading, error, empty and "Load more" as for facts (`context="load relations"`).

**Entities tab**: `Button icon="plus"` "Add an entity" (`EntityForm`: `Field label="Kind"` `TextInput` hint "Lowercase letters, digits, - and _, up to 32.", `Field label="Name"` `TextInput` hint "Up to 80 characters. Names are unique, ignoring case."). `DataTable`
(`GET /entities` is unpaged): **Name** (link to `/knowledge/entities/<id>`), **Kind** (`Tag`), **Created** (`RelativeTime`), actions: **Rename** (`EntityForm` with the name, sends `expectedVersion`), **Delete** (`DeleteEntityDialog`: `ConfirmDialog tone="danger"`
naming the entity and saying "It must not be used by any live fact or relation."; a 409 shows the hub's text inside the dialog). Empty: `EmptyState title="No entities yet"` ("An entity is a thing you name yourself, such as a VPN or a storage pool, so facts can be about it.").

### 6.6 The password flow **fixed**, `features/knowledge/useStepUp.tsx`

```tsx
export interface StepUp<V> {
  /** Runs the action now, or asks for the password first when `needsPassword`. */
  start(variables: V, needsPassword: boolean): void;
  /** The `PasswordPrompt` while one is open, else null. Render it once. */
  dialog: ReactNode;
  /** A failure to show next to the control when no prompt is open (a request that needed no password). */
  inline: DescribedError | undefined;
}
export function useStepUp<V>(options: {
  confirmLabel: string;
  reason: (variables: V) => string;
  /** Calls the feedback mutation: `feedback.mutate(withPassword(variables, password), callbacks)`. */
  run: (variables: V, password: string | undefined, callbacks: FeedbackCallbacks<unknown>) => void;
  busy: boolean;
  /** The feedback mutation's `error` (it must use `errorDisplay: "inline"`). */
  error: DescribedError | undefined;
  reset: () => void;
}): StepUp<V>;
```

Behaviour (**fixed**): `start(v, false)` calls `run(v, undefined, {})`. `start(v, true)` stores `v` and renders `PasswordPrompt` with `reason(v)`, `confirmLabel`, `busy`. Confirm calls `run(v, password, { onSuccess: close, onError })`; `onError` marks
`wrong = failure instanceof ApiFailure && failure.status === 403`. The prompt's `error` string is "That password is not right." when `wrong` (the exact sentence of the settings prompt), else the failure in words (`[title, detail].join(" ")`
of `error`, which for a 429 reads "Too many attempts. Try again in N s." and for a 409 carries the hub's sentence). Cancel closes the prompt, clears `wrong` and calls `reset`. The password lives only in the prompt's own state and in the
single request body; it is never put in a URL, a log, `localStorage`, a query key or a mutation variable that survives the call. Reasons (**fixed** text): accept, verify or assert on a safety predicate "This kind of fact can affect protective
actions. Enter your password to save it."; confirm "Confirming a management path tells Domovoi it is safe to rely on. Enter your password to confirm it."

### 6.7 Shell **fixed**

- `nav.ts`: three items appended to the "Knowledge" group, in this order: `{ to: "/knowledge", label: "Knowledge", icon: "info", preload: PAGES.knowledge.preload }`, `{ to: "/knowledge/paths", label: "Management paths", icon: "server",
  preload: PAGES.paths.preload }`, `{ to: "/knowledge/experience", label: "Experience", icon: "search", preload: PAGES.experience.preload }`. No counts (the overview has none for these). The command palette lists them without more code.
- `pages.ts`: `knowledge` (`KnowledgePage`), `fact` (`FactDetailPage`), `entity` (`EntityDetailPage`), `paths` (`PathsPage`), `experience` (`ExperiencePage`).
- `routes.tsx`: `/knowledge`, `/knowledge/facts/:id`, `/knowledge/entities/:id`, `/knowledge/paths`, `/knowledge/experience`, each `<PAGES.x.Page />` inside `<Layout/>`.
- `PageTitle.tsx` `defaultDetail`: `case "knowledge"` returns by `rest`: `paths` "Management paths", `experience` "Experience", `facts` "Fact", `entities` "Entity", otherwise undefined. The detail pages call `usePageName` with
  `"<predicate label>: <subject text>"` and the entity name. (The breadcrumb section of every `/knowledge/...` page reads "Knowledge", the label of the item whose `to` is `/knowledge`.)

### 6.8 Visibility: page keys and the category table **fixed**

Append to `VISIBILITY_PAGES` (37): `{ key: "knowledge", label: "Knowledge" }`, `{ key: "knowledge-detail", label: "Fact and entity detail" }`, `{ key: "paths", label: "Management paths" }`, `{ key: "experience", label: "Experience search" }`. Each page
component wraps its content in `VisibilityPage` with its key; `FactDetailPage` and `EntityDetailPage` use `knowledge-detail`. The panels mounted on the operation page inherit that page's key (`operation`); the incident page has no key of its own, so its panel follows the owner's default profile. Classified with the rule of 3.2 (decision controls and states are P0; a P0 is never inside
a P2 or P3). "Mark": **V** = `<Visible>`. Where a row says "wraps the boundary", the whole `QueryBoundary` (loading, error, empty and content) sits inside that one `Visible`.

| id | element | cat | mark |
|---|---|---|---|
| `knowledge.waiting-note` | "N facts inferred by Domovoi wait for your word" and its link | P0 | V |
| `knowledge.filters` | status chips, source, kind and search fields; the relations kind select and "Show retracted" | P1 | V |
| `knowledge.add` | the Add a fact, relation or entity button | P1 | V |
| `knowledge.facts` | wraps the facts boundary: states, table, "Load more", the search-limit line | P0 | V |
| `knowledge.proposals` | wraps the proposals boundary: states, cards, Accept, Retract | P0 | V |
| `knowledge.relations` | wraps the relations boundary: states, table, Details, Retract | P0 | V |
| `knowledge.entities` | wraps the entities boundary: states, table, Rename, Delete | P0 | V |
| `fact.state` | wraps the fact page boundary (loading, load error, "No such fact", the page) | P0 | V |
| `fact.status` | status pill, source tag, safety tag in the header meta | P0 | V |
| `fact.actions` | Accept, Verify, Retract in the header actions | P0 | V |
| `fact.notes` | retracted, purged, replaced and "confirm on the paths page" notes | P0 | V |
| `fact.value` | the value card | P1 | V |
| `fact.siblings` | other live values of the same subject and predicate, with "Use this value" | P1 | V |
| `fact.origin` | "Where it came from": source, recorded by, evidence, earlier value | P2, label "Where it came from" | V |
| `fact.times` | observed, last verified, fresh until, valid until, recorded | P3, `count={5}` | V inside a `DetailsScope` |
| `fact.history` | earlier (replaced) values | P2, label "History" | V |
| `entity.state` | wraps the entity page boundary | P0 | V |
| `entity.actions` | Rename and Delete in the header actions | P0 | V |
| `entity.facts` | "Facts about it" section (contains the P0 `knowledge.facts`) | P1 | V |
| `entity.relations` | "Relations" section (contains the P0 `knowledge.relations`) | P1 | V |
| `paths.state` | wraps the paths boundary (loading, load error, "No such host", the page) | P0 | V |
| `paths.blocked` | what is blocked, with the reasons, and what is not affected | P0 | V |
| `paths.coverage` | the coverage pill and its sentence | P0 | V |
| `paths.list` | the path rows: cidr, state, source, freshness, confirmation, Accept, Confirm, Retract | P0 | V |
| `paths.add` | "Add a management path" | P1 | V |
| `paths.checked` | "Checked <time>" and Refresh | P1 | V |
| `paths.host-chooser` | the host list shown when no host is selected | P1 | V |
| `experience.label` | the "historical evidence, not authorization" notice | P0 | V |
| `experience.filters` | scope control and search field | P1 | V |
| `experience.results` | wraps the results boundary: states, "Choose a host first", cards with narrative and source tags | P0 | V |
| `experience.summary` | wraps the resolution summary panel (states, fields with sources, Confirm, Edit, Add) | P0 | V |
| `experience.summary-history` | revisions of the summary fields | P2, label "Edit history", `count` = revisions | V |
| `experience.similar` | the similar past operations panel (its body repeats the label sentence as plain text) | P2, label "Similar past operations" | V |

The Add buttons (`knowledge.add`, `paths.add`) only open a dialog; the decision is made in the dialog (and its password prompt), which carries no category, so they are P1. `StatusPill`, `Tag`, buttons and table cells inside these blocks carry no marker of their own. The password prompt, confirmation dialogs, the edit dialog, the assert dialog and tooltips are dialogs and carry no category.
The page title, intro and docs link are strings given to `PageHeader`: no marker.

### 6.9 Detail pages, paths page, experience page and panels **fixed** (behaviour), **convention** (markup)

**1. Fact detail** `/knowledge/facts/:id` (`FactDetailPage`, `VisibilityPage page="knowledge-detail"`): `useFact(id)`; a 404 is `EmptyState title="No such fact"` with a link to `/knowledge`; other failures `LoadError context="load the fact"`;
`usePageName("<predicate label>: <subject text>")`. `PageHeader title` is that same string. `meta` = `fact.status`: `StatusPill` of `knowledgeStatusMeta(status)`, the source `Tag` (label from `SOURCES`, hint as `InfoTip`), and for a safety predicate a
`Tag` "Safety-related" with `InfoTip` "This kind of fact is used for a safety decision only when you entered it and it is current." `actions` = `fact.actions` from `factActions(fact)`: **Accept** (label "Make it mine" for an inferred or discovered fact; "Use this value"
when the fact is `disputed`; `hint` "Saves it as your own fact. The original stays as history."), **Verify** (`hint` "Marks it as checked today."), **Retract** (`ConfirmDialog` titled `Retract {predicate label} for {subject text}?`, consequence "It stops being used,
disappears from lists and search, and is no longer shown to the model. Its content is removed after the retention period. You can enter it again later.", `tone="danger"`, confirm label "Retract"). Accept and Verify use `useStepUp` with
`needsPassword(action, predicate)`; Retract never. Every call sends `expectedVersion = fact.version` of the page's last load; a 409 shows inside the dialog or next to the buttons (`describeError`: "That no longer fits the current state." and the hub's sentence) and
the page refetches. A successful **accept** navigates (`replace`) to `/knowledge/facts/<id of the answer>` (the new user row) with the toast "Saved as your own fact. The original stays as history."; verify toast "Marked as checked."; retract toast "Retracted. It is no longer used."
`fact.notes`: for `retracted` "Retracted <RelativeTime of retracted_at> by <actorLabel>"; for `content_purged` "The content was removed after the retention period. Only this record remains."; for `superseded` "A newer value replaced this one. See History.";
for `confirmElsewhere` an `InlineMessage tone="info"` "Confirm this path on the Management paths page." with a `LinkButton` to `/knowledge/paths?host=<host name>` (the host name from `useHostNames` through the subject; omit the button when the host is unknown). `fact.value`: a `Card`
with `valueText`, mono for a cidr, or "Removed after the retention period." when purged. `fact.siblings`: `useFacts({ subject, predicate })` (the hub's live default) minus this fact; nothing when none, `Skeleton` while loading, `LoadError` "load the other values" on failure; each row
a link to that fact with value, source tag and status pill, and **Use this value** (Accept through `useStepUp`) only when that sibling is `disputed`. `fact.origin`: `KeyValueList` Source (label and hint), Recorded by (`actorLabel`), Evidence (a link for `note`, `incident`, `operation` references as in 6.5, plain text otherwise, "None" when null),
Earlier value (a link to `supersedes`, "None" when null). `fact.times`: `KeyValueList` Observed, Last verified ("Never"), Fresh until, Valid until ("No limit"), Recorded, each a `RelativeTime` with the exact time in its tooltip, in one `DetailsScope`. `fact.history`: `useFacts({ subject, predicate, status: "superseded" })`
(mounted only while open), newest first, each row value, source tag and when; "No earlier values." when empty; `LoadError` on failure.

**2. Entity detail** `/knowledge/entities/:id` (`EntityDetailPage`, `knowledge-detail`): `useEntities()` and find the id; absent after a successful load is `EmptyState title="No such entity"`. `PageHeader title={name}` with a kind `Tag`; `entity.actions`: Rename and Delete as in 6.5 (after a delete, navigate to
`/knowledge?tab=entities`). Sections: `entity.facts` "Facts about it" (a `FactsTable` of `useFacts({ subject: "entity:<id>" })` with the status chips of 6.5 and an "Add a fact" button whose dialog is preset to this subject) and `entity.relations` "Relations" (a `RelationsTable` of
`useRelations({ subject: "entity:<id>" })`).

**3. Add a fact** (`AssertFactDialog`, used by the Facts tab, the entity page and the paths page): a `Dialog` form. `Field label="Kind"` `Select` of the **assertable** predicates (the `PREDICATES` labels; `host_platform` is absent); `SubjectField` (kind `Select` limited to the predicate's allowed `subjects`, then the host `Select` from
`useHosts()` (value = host id, shown as the name), a name `TextInput` for container or unit, or an entity `Select` from `useEntities()`); `ValueFields` for the predicate (text `Textarea` or `TextInput`; a `SegmentedControl` for criticality; a cidr `TextInput` with the hint "For example 192.168.1.0/24. Domovoi stores it in its standard form."; two `TextInput type="datetime-local"` fields labelled "Starts (UTC)" and
"Ends (UTC)" and an optional note for a maintenance window; a second `SubjectField` for `depends_on`; nothing for `never_restart`, only the sentence "Domovoi will not restart it."). Save: `buildValue`; a `problem` is shown under its field and nothing is sent. When `needsPassword("assert", predicate)` the dialog is replaced by `PasswordPrompt` via `useStepUp` (the draft is kept; Cancel returns
to the form). Sends `useAssertFact` with `subject`, `predicate`, `value` and the password only when asked. 400s and 409s show inside the dialog (`errorDisplay: "inline"`). Success toast "Fact saved." and the list refreshes. A preset (paths page, entity page) fixes the predicate or the subject and hides that control.

**4. Management paths** `/knowledge/paths` (`PathsPage`, `VisibilityPage page="paths"`, follows the host selector): with no host selected the page shows `paths.host-chooser`: `Section title="Choose a host"` ("Management paths are confirmed host by host.") and a `DataTable` of `useHosts()` (Host, Connection `StatusPill` from `hostConnectionMeta`,
a "Show paths" `Button` that calls the selector's setter); loading and failure as for hosts everywhere (`QueryBoundary`, `LoadError context="load hosts"`). With a host: `useManagementPaths(name)`, a 404 is `EmptyState title="No such host"`; failure `LoadError context="load management paths"`. `PageHeader title="Management paths" intro="The networks you reach each host
through. Protective actions must never cut them off."`; `actions` = `paths.checked`: muted "Checked <RelativeTime of dataUpdatedAt>" and a secondary `Button icon="refresh"` "Refresh" (`refetch`, `loading` while fetching). Then, inside `paths.state`:

1. `paths.blocked` from `containmentBlock({ coverageAvailable, states })`. Blocked: `InlineMessage tone="warning" title="Unattended containment is blocked on {host}"` listing every reason, then the sentence "Monitoring and actions you approve yourself are not affected." Not blocked: `InlineMessage tone="success" title="Management paths are confirmed and current on {host}"` and "Unattended containment also needs the capability, the policy and the helper to allow it. Monitoring and actions you approve yourself are not affected." It is a **display of a conclusion**: nothing on the page enforces anything.
2. `paths.coverage`: a row with `StatusPill` of `coverageMeta(coverage_available)` and a sentence: unavailable "The check that shows the helper protects a path is not available in this version, so no path can be confirmed yet. You can add paths, accept the ones Domovoi found and retract wrong ones."; checkable "When you confirm a path, the hub asks the helper whether it covers it."
3. `paths.list`: a `DataTable` (`caption="Management paths of {host}"`) of `paths`, ordered as the hub sent them: **Path** (the cidr from `valueText`, mono, a link to the fact page), **State** (`StatusPill` of `pathStateMeta(state)`), **Source** (`Tag`), **Freshness** ("Fresh until <RelativeTime of fresh_until>"; for a `stale` fact "Stale since <RelativeTime of fresh_until>"),
   **Confirmation** ("Confirmed by <actorLabel> <RelativeTime>" or "Not confirmed"), actions (hugging column): by state: `candidate` Accept ("Accept as mine", password) and Retract; `unconfirmed` Confirm and Retract; `stale` Confirm ("Confirm again") and Retract; `confirmed` Retract; an unknown state Retract only. **Confirm** is
   a `Button`: while `coverage_available` is false it is **disabled** with `disabledReason` "Coverage cannot be verified yet, so no path can be confirmed. The helper check is not available in this version." and sends nothing; when true it opens the password prompt (`needsPassword("confirm", ...)` is always true) and sends `useConfirmPath` with `expectedVersion = fact.version` and the password. A 409
   ("not live", "accept the path first", "the helper does not cover this path", "cannot be verified yet", "someone changed it meanwhile") shows inside the prompt in the hub's words. Success toast "Path confirmed." Accept success toast "Saved as your own path. Confirm it next." Retract (`ConfirmDialog`, no password) toast "Path retracted."
   Empty list: `EmptyState title="No management paths yet"` ("Add the networks you manage this host from. Until then unattended containment stays blocked.") with the Add button.
4. `paths.add`: primary `Button icon="plus"` "Add a management path" opens `AssertFactDialog` preset to predicate `management_path` and this host (password required). Nothing on this page reads "covered" or "protected" as a status of a path, anywhere.

**5. Experience search** `/knowledge/experience` (`ExperiencePage`, `VisibilityPage page="experience"`): `PageHeader title="Experience" intro="Past operations you can search. They are evidence, never permission." docs="concepts/experience.html"`. `experience.label`: `InlineMessage tone="info" title="Historical evidence, not authorization"` with
"A past approval or success never lets anything run now. Every action still needs its own approval." `experience.filters`: `SegmentedControl label="Search scope"` with "Selected host" (default) and "All hosts" kept in `?scope=global` (absent = host), and a `Field label="Search"` `TextInput` debounced 300 ms into `?q=`. Data: scope host uses the selected host's name (`useSelectedHost`) and
`useExperienceSearch({ q, scope: "host", host })`; scope global uses `{ q, scope: "global" }` and sends **no host**; with scope host and no selected host the request is not made and `experience.results` shows `EmptyState title="Choose a host first"` ("Experience is searched on one host at a time. Pick a host in the selector, or search all hosts on purpose.") with a `Button` "Search all hosts" that sets
`scope=global`. Under global an `InlineMessage tone="warning" title="Searching every host"` ("Results can come from any host.") sits above the results. An empty `q` is valid (the newest finished operations of the scope). Results: one `PastOperationCard` per item: the goal as a link to `/operations/<id>`, a `StatusPill` of `operationStatusMeta(outcome)`, a `Tag` "Fix verified" or "Not verified" (`Tooltip` "A follow-up
check confirmed the fix worked." / "No follow-up check confirmed the fix."), host `Tag`s (names; "a removed host" when `name` is null), `RelativeTime` of `ended_at`, a `Tag` "Matches some of your words" when `match_pass` is `any_word`, the `snippet` as plain muted text when present, "Ran" lines from `contracts` (`<contract> on <target or host name>: <outcome>`), and `Narrative`: problem, cause and fix as labelled lines, each with its source `Tag` from
`SUMMARY_SOURCES` (every narrative field is shown with its source; nothing from the model is shown without it). Text is rendered as React text only (never HTML or Markdown). `QueryBoundary`: `Skeleton`; failure `LoadError context="search past operations"` (never an empty list); empty `EmptyState title="No past operations match"` ("Try fewer or different words, or search all hosts.") and for a blank `q`
"No finished operations yet". The ten best matches come back; when ten do, a muted line says "Showing the 10 best matches. Narrow the search to see others."

**6. Resolution summary panel** `ResolutionSummary({ operationId })` (`experience.summary`): `useResolutionSummary(operationId)`. `state` `not_ready` renders nothing; `drafting` renders `Section title="Resolution summary"` with `Progress label="Domovoi is writing the summary…"`; load failure `LoadError context="load the resolution summary"`. `ready`: `Section title="Resolution summary"
description="What this operation found and did. Domovoi fills some fields; the model drafted the rest until you confirm or edit it."`. One `SummaryFieldRow` per entry of `fields` in the hub's order: label (`SUMMARY_FIELDS`), value (`summaryValue(field, value, hostNames)`: outcome through `operationStatusMeta`, verified "Yes"/"No", subjects "<check> on <target or host name>", contracts "<contract> on <target>: <outcome>", hosts names, narrative text; an unexpected shape is compact JSON, never an exception),
the source `Tag` (`SUMMARY_SOURCES`, hint in a `Tooltip`), "<actorLabel>, <RelativeTime>" and "Revision N". For a field with `editable: true`: **Confirm** (only when the source is `model`; sends the **same text** with `expectedRevision = field.revision`; success toast "Confirmed. Domovoi now treats it as your words."; when the answer says `unchanged` an info toast "Nothing changed: this already is your text.") and **Edit** (`EditSummaryDialog`:
`Textarea` prefilled with the current text, hint "Up to 1000 characters. It is saved as written by you.", Save disabled with a reason when empty or over 1000, sends `expectedRevision = the field's current revision`, success toast "Saved your edit."; 409 and 400 inside the dialog in the hub's words, and a stale revision (409) also refetches the summary and keeps the typed text). For a narrative field that has no revision yet and a `ready` summary, a ghost `Button` "Add the {label}" opens the same dialog with `expectedRevision = 0`. Code-owned fields have no
buttons at all. `experience.summary-history` (P2, label "Edit history", `count` = `history.length`): revisions newest first with field label, source `Tag`, revision, actor and time, and the value as plain text; a muted line "Older revisions are not listed." when `history_truncated`. Mutations use `useEditSummaryField` through `useActionFeedback` (`errorDisplay: "inline"` in the dialog).

**7. Similar past operations panel** `SimilarOperations({ source })` (`experience.similar`, P2, label "Similar past operations"): the body starts with the plain text "Historical evidence, not authorization. A past fix is a hint about where to look; it never approves anything now." then `useSimilarOperations(source)` (mounted only while open, so nothing is requested in Standard or Essential until the person opens it). Items as `PastOperationCard` (compact: no narrative longer than three lines) with
reason `Tag`s: `same_check` "Same check", `same_target` "Same target", `same_contract` "Same action: <detail>", `similar_words` "Similar words" (unknown code: the raw code). Empty "No similar past operations yet." (a known empty); failure `LoadError context="load similar operations"`.

**8. Mounts.** `OperationExperience({ operationId, status })` renders `ResolutionSummary` only when `status` is `succeeded`, `failed`, `escalated` or `cancelled` (anything else, including an unknown status, is not finished) and always `SimilarOperations source={{ operation: operationId }}`. `OperationDetailPage` renders it once, after its Details section. `IncidentDetailPage` gets one slot `data-slot="experience"` after the operations slot, rendering
`SimilarOperations source={{ incident: <incident id> }}` **only when `IncidentDetail.operations` is non-empty** (verify the field name); otherwise nothing, so with orchestration off the incident page is unchanged. Neither mount edits anything else in those files.

## 7. Ordered implementation steps

Work in `web/`. Commit in small green steps (one-line Conventional Commits, exactly one scope `web`, for example `feat(web): knowledge vocabularies and hooks`) after steps 4, 6, 7, 8, 9, 10, 11 and 12. Run `npm run lint`, `npm run typecheck` and the vitest files of the area before every commit.

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Confirm 30, 34, 35 and 37 are merged: `src/features/operations/OperationDetailPage.tsx`, `src/features/incidents/IncidentDetailPage.tsx` with 30's operations slot, `src/api/hooks/experience.ts` (its four hooks and the variables of `useEditSummaryField`), `src/design/visibility.ts` (`VISIBILITY_PAGES`), `src/components/{Visible,DetailsScope,Disclosure,visibilityContext}.tsx`, `src/test/visibility.tsx`
     (`renderWithProfile`, `expectP0Everywhere`), `src/test/operationFixtures.ts`, `scripts/smoke.mjs` and `scripts/smoke/*`, and in `src/api/schema.d.ts` every path and schema of 3.2 (knowledge and experience). **If any is missing, stop and report.** Write down every real name that differs from section 3.2, in particular the **real schema names** of the
     knowledge fact DTO and of the other knowledge DTOs (the `KnowledgeFactDto` collision), the exact query parameters and body fields in the generated types, and whether `IncidentDetail.operations` and `ExecutionDto`-style fields are named as 30 says. If `npm run gen:api` followed by `git diff --exit-code -- src/api` shows a diff, a prerequisite is wrong: stop and report.
   - Read `web/DESIGN.md` completely. Open every web file named in 3.1 and confirm the line numbers. Read `PasswordPrompt.tsx`, `EditorForm.tsx:100-140`, `useActionFeedback.ts`, `QueryBoundary.tsx`, `Tabs.tsx`, `DataTable.tsx`, `FilterChips.tsx`, `SegmentedControl.tsx`, `inputs.tsx` (`Select`, `TextInput`, `Textarea`, `Checkbox`), `HostContext.tsx`, `features/rules/HostField.tsx`.
   - Read 30's `OperationDetailPage.tsx` and `IncidentDetailPage.tsx` completely before adding the one element each (6.9 item 8).
   - Run `npm test` once for the baseline and list the tests that assert the navigation items or the command palette (`routes.test.tsx`, `layout.test.tsx`, `palette.test.tsx`): they will see three more items.
   - Run `grep -rn "knowledge" src` (the generated files only should match) and `ls scripts`.
2. Create branch `knowledge-pages` from the latest `overhaul-investigation`.
3. **Pure code first.** `design/knowledgeVocab.ts`, `knowledgeText.ts`, the lookups in `tone.ts` and `status.ts`, then `subjects.ts`, `valueText.ts`, `factActions.ts`, `valueForm.ts`, `factFilters.ts`, `blocked.ts`, `experience/summaryValue.ts`, `experience/experienceParams.ts` with their unit tests (12.2).
4. **Data layer and fixtures.** Aliases, keys, `hooks/knowledge.ts`, `hooks/knowledgeWrites.ts`, `src/test/knowledgeFixtures.ts` and the JSON fixtures, hook tests (12.3). Commit `feat(web): knowledge vocabularies, hooks and fixtures`.
5. **Shared pieces.** `useSubjectLabels`, `SubjectLink`, `useStepUp` and its tests (12.4).
6. **Knowledge page.** Shell entries (6.7), `KnowledgePage` and the four tabs, `AssertFactDialog`, `SubjectField`, `ValueFields`, relation and entity dialogs; tests of 12.5. Commit `feat(web): knowledge browser`.
7. **Fact and entity detail.** `FactDetailPage` and its blocks, `FactActions`, `EntityDetailPage`; tests of 12.6. Commit `feat(web): fact and entity detail pages`.
8. **Management paths.** `PathsPage` and its parts; tests of 12.7. Commit `feat(web): management paths page`.
9. **Experience.** `ExperiencePage`, `PastOperationCard`, `Narrative`, `ResolutionSummary`, `SimilarOperations`, `OperationExperience`, the two mounts; tests of 12.8. Commit `feat(web): experience search, resolution summary and similar operations`.
10. **Visibility.** Append the four page keys, check every marker against 6.8, add the profile matrix and the gate tests (12.9). Commit `feat(web): classify the knowledge pages by visibility category`.
11. **Catalogue.** `src/dev/KnowledgeDemos.tsx` in `DesignPage` and its test (12.10); look at `/dev/design` and the four pages in both themes. Commit `feat(web): knowledge components in the design catalogue`.
12. **Smoke test and screens.** Mock routes, data, checks 12 to 14, `screens.mjs`; `npm run build`, then `npm run smoke` (12.11). Commit `test(web): browser smoke test for the knowledge pages`.
13. **Docs** (section 15); `cargo xtask docs-links` (needs Rust; if you cannot run it, say so).
14. **Checks and completion report.**
    - `npm run format` then `npm run format:check`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run build` (runs `size`; record the new numbers), `npm run smoke`, and `npm run gen:api` followed by `git diff --exit-code -- src/api` (must be clean).
    - `node scripts/screens.mjs` against the dev hub if you can start it (WSL; see `TESTING.md`), otherwise say it was not run. Check by hand in the browser at 375, 768, 1280 and 1920 px, both themes, keyboard only, in all three profiles.
    - Write the completion report in the standard structure, and add: the real schema names you found, the three places this release shows coverage as unavailable, and any difference between section 3.2 and the generated types.

## 8. Database and persistence

Not applicable: this prompt touches no database, no migration and no hub code. Browser storage holds nothing new; the filters, tab, scope and search live in the address (`?tab=`, `?status=`, `?source=`, `?predicate=`, `?q=`, `?scope=`, `?host=`). A password is never stored anywhere.

## 9. Protocol and compatibility

No wire change, no new hub route, no OpenAPI change, no generated-file edit. The pages tolerate a newer hub: an unknown status, path state, source, predicate, relation kind, summary field, summary source, reason code, match pass or outcome renders as a neutral pill or plain text with the raw value and never throws; an unknown
subject kind shows the raw reference as text. A hub that answers 404 on `/api/v1/knowledge/*` (an old hub behind a dev server) gives `LoadError` on every list. With orchestration off the experience routes answer empty data (200): the search page shows its empty state and the operation and incident pages are unchanged.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| A list request fails (facts, proposals, relations, entities, search, paths, summary, similar) | `LoadError` with Retry in place of that list; never an empty state, never a stale empty table |
| `GET /facts/{id}` is 404 | `EmptyState` "No such fact" with a link to `/knowledge`; same for an unknown entity id ("No such entity") |
| `GET .../management-paths` is 404 | `EmptyState` "No such host" |
| A refetch fails while data is on screen | the data stays; the failure is reported by `QueryBoundary` as everywhere else in the app |
| An action answers 409 (stale version, not live, not owner fact, uncovered, unverifiable, duplicate, in use) | inside the dialog or prompt, or inline next to the buttons, in the catalogue's words plus the hub's own sentence; the page refetches; nothing else changes |
| An action answers 403 inside the password prompt | "That password is not right." under the field; the prompt stays open; the typed password is kept only in the field |
| An action answers 429 | the prompt shows "Too many attempts. Try again in N s." (from `retryAfter`); nothing was written |
| An action answers 400 (bad subject, value, cidr, entity name) | inside the form dialog in the hub's words; the draft is kept |
| A request that needed no password answers 403 | the failure is shown as for any other action (the hub's sentence "this change needs your password" reaches the person); the page does not retry by itself |
| Accept succeeds | the list refreshes; on the detail page the page moves to the new row |
| `coverage_available` is false | Confirm is disabled with its reason; no confirm request exists; Accept, Add and Retract still work |
| The paths page has no host selected | the host chooser; no request to `management-paths` |
| Experience scope is host and no host is selected | "Choose a host first"; no request; "Search all hosts" is a deliberate click |
| The edit of a summary field answers 409 stale | the hub's text in the dialog, the summary refetches, the typed text stays |
| The summary state is `drafting` | `Progress` line; the operation's journal frames refresh it; no polling |
| `similar` or `search` fails | `LoadError`; the page never says "no similar operations" for a failure |
| The hub sends a value of an unexpected shape | `valueText` and `summaryValue` fall back to compact JSON cut at 400 characters |
| An unknown value anywhere | neutral pill or raw text, never an exception |

## 11. Security requirements

- **The owner decides, the hub enforces.** Every action is a hub route that re-checks state, identity and the password. The UI hides or disables controls only to explain, never as the check. No cookie, token or CSRF value is read outside `src/api`.
- **The web never forges provenance.** No request body contains `source` or `actor` (a test inspects every write body). The only way a fact becomes the owner's is a click on Accept, Use this value or an owner assertion; no effect, timer, stream frame, URL parameter or test helper calls a write hook. A source-scan test proves each write hook is
  imported only by its explicit control files (12.9).
- **Step-up.** Assert, accept and verify on `management_path` or `trusted_cidr`, and every confirm, ask for the password with `PasswordPrompt` before the request; retract never does. The password is held in the prompt's state only, sent once in the request body, never put in a URL, a log, storage, a query key or a toast, and never echoed. A wrong password reads
  "That password is not right."; a lockout shows the wait. The step-up list is `SAFETY_CRITICAL` plus "every confirm"; if the hub demands a password the UI did not ask for, the hub's 403 text is shown (no silent retry).
- **Coverage is never shown as covered.** There is no "covered" status in any table, label or hint. The two coverage states are `unavailable` and `checkable`. Confirm is disabled, with a reason, while coverage is unavailable, and no confirm request can be built then. A `confirmed` path says only that the owner confirmed it. The blocked line is a display derived from
  hub data and says other conditions still apply when it is not blocked; it never says an action is allowed.
- **Experience is evidence.** Every past-operation view carries "historical evidence, not authorization" (the page notice, and the panel's body text); the `label` of each response equals the constant in a test. No control on those views approves, runs or schedules anything. Narrative text and snippets are shown as React text only, always with the source of each narrative field; model-drafted text is labelled
  "Drafted by the model, not checked". A global search is a deliberate click and sends no host.
- **Untrusted text.** Fact values, notes, entity names, evidence references, summary text, goals, snippets and every hub sentence are rendered as text: no `dangerouslySetInnerHTML`, no Markdown, no link built from them except the fixed `/notes/`, `/incidents/`, `/operations/` routes from an evidence reference of a known kind with an id that contains only letters, digits and hyphens.
- **No secrets.** Facts are not a secret store, and the UI adds no redaction; it never logs a value, a password or a request body, and never puts one in a URL. Retracted content is shown only when the owner picks the Retracted chip, and a purged one is shown as removed.
- **Profiles never gate authority.** No hook under `src/api/` imports visibility; `useStepUp`, the write hooks, `PathActions` and `FactActions` contain no visibility primitive (a gate test); Accept, Confirm, Retract and the password prompt are available in every profile.
- **No new network path.** No `fetch` outside `src/api`, no external origin.
- **Audit** is the hub's (`knowledge.fact.*`, `knowledge.management_path.confirmed`, `operation.summary_edited`); the UI adds none.

## 12. Testing requirements

Name every test after the behaviour. Page and component tests use `renderAt`, `apiMock` and, for profiles, `renderWithProfile`; axe runs automatically over whatever a test leaves on screen (no test here calls `skipAxe()` except the catalogue test, with a comment). No test sleeps: use `findBy…` and fake timers.
Expectations come from the fixture data (counts, labels derived by a small independent helper written in the test), never from calling the function under test.

### 12.1 Fixtures

`src/test/fixtures/knowledge/*.json` (shared with the smoke test, typed by `knowledgeFixtures.ts` against the generated types; a test `each_knowledge_fixture_has_the_generated_type` is a typecheck plus a label check): `facts-page.json` (a user `role` current; an inferred `role`; a stale `owner_note`; two disputed `criticality` values; a discovered
`management_path`), `fact-retracted.json`, `fact-purged.json` (value `null`, `content_purged: true`), `paths-unavailable.json` (`coverage_available: false`; paths in states `candidate`, `unconfirmed`, `stale`), `paths-available.json` (`true`; one `unconfirmed`, one `confirmed`), `relations-page.json`, `entities.json`. Subject refs use the `nas` host of `src/test/fixtures.ts` (`host:host-nas`).
`knowledgeFixtures.ts` exports builders `knowledgeFact(overrides)`, `managementPath(overrides)`, `knowledgeRelation(overrides)`, `knowledgeEntity(overrides)`. The experience fixtures are 35's.

### 12.2 Pure code (`src/design/*.test.ts`, `src/features/knowledge/*.test.ts`, `src/features/experience/*.test.ts`)

| Test | Expected |
|---|---|
| `knowledge_vocabularies_have_label_tone_hint_and_icon` | every key of `KNOWLEDGE_STATUSES`, `PATH_STATES`, `COVERAGE_STATES` (the existing `tone.test.ts` also covers them through `VOCABULARIES`); unknown values give a neutral meta with the raw value |
| `coverage_is_never_called_covered` | no label or hint of the three tables contains the words "covered" or "protected"; `COVERAGE_STATES` has exactly the keys `unavailable` and `checkable`; `coverageMeta(false)` is the warning one |
| `predicate_table_matches_the_knowledge_store_rules` | ten predicates; `SAFETY_CRITICAL` is exactly `management_path` and `trusted_cidr`; `host_platform` is not assertable; subject kinds per 6.3 (`management_path`, `trusted_cidr`: host only; `never_restart`: container and unit) |
| `needs_password_covers_safety_predicates_and_every_confirm` | table-driven over five actions and ten predicates: confirm always true; retract always false; assert, accept, verify true only for the two safety predicates |
| `actor_label_names_you_and_unknown_actors_stay_raw` | `user:` equal to `me` is "You", another `user:` "Another account", `system:knowledge`, `agent:x`, an unknown string raw |
| `subjects_round_trip_and_never_assume_a_uuid` | `host:host-nas`, `container:host-nas/plex`, `unit:host-nas/nginx.service`, `entity:abc` parse and format back; a container name is everything after the first `/`; `x:y`, `host`, `` give `undefined` |
| `subject_labels_use_names_and_fall_back_without_throwing` | host name, "plex on nas", entity name; unknown host "an unknown host", unknown entity "a removed entity"; a malformed ref is returned as text with no link |
| `value_text_renders_each_predicate_and_never_throws` | cidr, maintenance window, host platform (GiB to one decimal), `never_restart`, `depends_on` through the subject label, plain strings; an object of an unknown shape is compact JSON cut at 400 characters; `null` is "" |
| `fact_actions_follow_the_status_source_and_predicate` | table over statuses, sources, predicates and `content_purged`: live and not user gives accept; verify never for `management_path`; retract only when live; superseded and retracted give nothing; user `management_path` gives `confirmElsewhere` |
| `build_value_validates_each_predicate_and_sends_the_cidr_as_typed` | every rule of 6.4: lengths, criticality words, `never_restart` true, maintenance window UTC conversion and end after start, `host_platform` refused; `192.168.1.7/24` is returned unchanged |
| `fact_filters_round_trip_through_the_address` | defaults leave the URL; `retracted` gives `status=retracted`; unknown values read as defaults; `tab` and `host` are kept |
| `containment_block_follows_the_truth_table` | unavailable gives blocked with its reason first; no paths; candidates, unconfirmed, stale and unknown counted in order; all confirmed with coverage available is not blocked; confirmed paths with coverage unavailable are still blocked |
| `summary_value_handles_every_field_and_odd_shapes` | each of the eight fields renders text; an unexpected shape is compact JSON; never throws |
| `experience_params_default_to_host_scope_and_never_send_a_host_for_global` | `?scope=global` absent means host; global drops the host |

### 12.3 Hooks (`src/api/hooks/knowledge.test.ts`)

| Test | Expected |
|---|---|
| `fact_hooks_send_the_documented_requests` | `useFacts` sends `host`, `predicate`, `source`, `status`, `limit=50` and then `cursor`; empty values are dropped; `useFactSearch` sends `q` and no cursor and is idle for a blank `q` |
| `each_write_hook_sends_its_path_and_body` | the table of 6.2: bodies hold exactly the listed keys (an optional key is absent, not `undefined`); `confirm` always has a `password`; **no body has the key `source` or `actor`**; `retract` never has a `password`; `delete_entity` puts `expected_version` in the query |
| `each_write_invalidates_the_knowledge_prefix_and_nothing_else` | spy on `invalidateQueries`; no retry; no `setQueryData` |
| `relations_hook_sends_include_retracted_only_when_true` | |

### 12.4 Shared pieces (`useStepUp.test.tsx`, `subjectLabels.test.tsx`)

| Test | Expected |
|---|---|
| `step_up_runs_at_once_when_no_password_is_needed` | `start(v, false)`: `run` called with `undefined`, no prompt |
| `step_up_asks_for_the_password_first_and_sends_it_once` | `start(v, true)`: prompt with the reason; nothing sent until Confirm; the request carries the typed password once |
| `a_wrong_password_reads_that_password_is_not_right_and_keeps_the_prompt` | `failure(403, "forbidden", ...)` |
| `a_locked_out_prompt_shows_the_wait` | `failure(429, "too_many_requests", ..., { "retry-after": "28" })` shows "Too many attempts. Try again in 28 s." |
| `cancel_closes_the_prompt_and_sends_nothing` | |
| `the_password_is_not_kept_after_the_call` | after success or cancel, a second `start` shows an empty field; the password string appears in no query key, URL or storage entry |
| `subject_labels_load_host_and_entity_names_once` | one `GET /hosts` and one `GET /entities` for a page that labels many subjects |

### 12.5 Knowledge page (`knowledgePage.test.tsx`, `proposals.test.tsx`, `relations.test.tsx`, `entities.test.tsx`, `assertFact.test.tsx`)

| Test | Expected |
|---|---|
| `facts_tab_lists_facts_with_source_and_status_and_labels_stale_and_disputed` | rows from `facts-page.json`; source tags; "Stale" and "Disputed" pills with their hints |
| `retracted_and_replaced_facts_are_hidden_until_asked_for` | the default request has no `status`; chip Retracted sends `status=retracted` and shows value and no actions; a purged one says "Removed after the retention period." |
| `facts_follow_the_host_selector_and_the_address` | selected host `nas` goes as `host=nas`; chips, source, kind and search are kept in `?status=`, `?source=`, `?predicate=`, `?q=` |
| `search_uses_the_search_route_without_a_cursor_and_says_when_it_is_cut` | `q` set: `GET` with `q`; 50 results show the "Showing the best 50 matches" line |
| `a_failed_facts_load_is_a_load_error_not_an_empty_state` | Retry refetches |
| `facts_empty_states_say_what_to_do` | no facts; filters with no match with Clear filters; no retracted facts |
| `load_more_passes_the_cursor` | |
| `proposals_list_inferred_facts_and_the_tab_counts_them_without_a_second_request` | request has `source=inferred`; the tab count equals the rows; only one such request |
| `accepting_a_proposal_sends_the_expected_version_and_no_password_for_an_ordinary_fact` | body `{ expected_version }`; toast "Saved as your own fact. The original stays as history." |
| `accepting_a_safety_critical_proposal_asks_for_the_password` | a `trusted_cidr` inferred fact: prompt first; body has `expected_version` and `password` |
| `retracting_asks_first_names_the_fact_and_never_asks_for_a_password` | `ConfirmDialog` text; body `{ expected_version }` only |
| `a_stale_version_is_shown_in_the_hubs_words_and_the_list_refetches` | `failure(409, "conflict", "someone changed it meanwhile; reload and try again")` |
| `waiting_note_appears_only_while_proposals_exist` | |
| `relations_tab_lists_retracts_and_opens_the_detail_dialog` | text "plex runs on nas"; Retract body; dialog fields; purged relation says removed |
| `adding_a_relation_sends_from_kind_to_and_shows_the_hubs_400_inside_the_dialog` | |
| `entities_tab_creates_renames_and_deletes_with_the_hubs_conflicts_inside_the_dialog` | duplicate name 409, in-use 409, stale 409 |
| `adding_a_fact_offers_only_assertable_predicates_and_matching_subject_kinds` | no `host_platform`; `management_path` offers hosts only; `never_restart` offers container and unit |
| `adding_a_fact_sends_subject_predicate_and_value_and_never_source_or_actor` | ordinary predicate: no `password` key; a bad value shows its problem under the field and sends nothing |
| `adding_a_safety_critical_fact_swaps_the_form_for_the_password_prompt_and_cancel_returns_to_it` | `management_path`: prompt replaces the form, the draft survives, body has `password` |
| `the_hubs_400_for_a_bad_cidr_shows_inside_the_dialog` | |

### 12.6 Detail pages (`factDetail.test.tsx`, `entityDetail.test.tsx`)

| Test | Expected |
|---|---|
| `fact_detail_shows_status_source_value_and_where_it_came_from` | pill, source tag, value, origin block (recorded by "You", evidence link to `/notes/<id>`), times |
| `fact_detail_for_each_status_shows_the_right_notes_and_actions` | current user fact: Verify, Retract; inferred: Accept ("Make it mine"), Verify, Retract; disputed: "Use this value"; superseded and retracted: no buttons and their notes; purged: the removal note |
| `a_management_path_fact_points_at_the_paths_page_and_offers_no_verify` | |
| `accept_moves_the_page_to_the_new_fact` | location becomes `/knowledge/facts/<new id>` |
| `verify_and_retract_send_the_page_version_and_report_conflicts_in_place` | |
| `siblings_offer_use_this_value_only_when_disputed` | and a failed siblings load is a `LoadError`, not nothing |
| `history_loads_only_when_opened` | no `status=superseded` request until the toggle is opened (Standard profile) |
| `no_such_fact_says_so_with_a_link_back` | 404 |
| `entity_page_shows_its_facts_and_relations_by_subject_and_deletes_back_to_the_list` | requests carry `subject=entity:<id>`; delete navigates to `/knowledge?tab=entities`; an unknown id says "No such entity" |

### 12.7 Management paths (`pathsPage.test.tsx`)

| Test | Expected |
|---|---|
| `coverage_unavailable_is_shown_honestly_and_nothing_reads_covered` | `paths-unavailable.json`: pill "Cannot be verified yet"; the page text has no "covered" or "protected"; the blocked message names the helper reason |
| `confirm_is_disabled_with_its_reason_and_sends_nothing_while_coverage_is_unavailable` | click Confirm: no `POST .../confirm` is ever recorded |
| `the_blocked_message_lists_every_reason_and_says_monitoring_is_unaffected` | candidates, unconfirmed, stale counts as in `containment_block` |
| `paths_show_state_source_freshness_and_confirmation` | each row: state pill, source tag, "Fresh until" or "Stale since", "Not confirmed" or "Confirmed by You" |
| `accepting_a_discovered_path_asks_for_the_password_and_moves_it_to_unconfirmed_after_refetch` | body `{ expected_version, password }` |
| `confirming_with_coverage_available_asks_for_the_password_and_sends_version_and_password` | `paths-available.json`; success toast "Path confirmed." |
| `a_confirm_conflict_shows_the_hubs_sentence_inside_the_prompt` | "the helper does not cover this path" and "someone changed it meanwhile" cases |
| `a_wrong_password_on_confirm_keeps_the_prompt_open_and_changes_nothing` | |
| `adding_a_path_uses_the_password_prompt_and_presets_predicate_and_host` | |
| `retracting_a_path_needs_no_password` | |
| `no_host_selected_shows_the_host_chooser_and_makes_no_paths_request` | choosing a host selects it |
| `unknown_host_and_failed_load_are_distinct` | 404 "No such host"; 500 `LoadError`; empty list "No management paths yet" |
| `all_confirmed_with_coverage_available_is_not_blocked_but_says_other_conditions_apply` | |

### 12.8 Experience (`experiencePage.test.tsx`, `resolutionSummary.test.tsx`, `similarOperations.test.tsx`)

| Test | Expected |
|---|---|
| `every_past_operation_view_carries_the_historical_label` | search page notice, similar panel body, and the constant equals the fixtures' `label` |
| `search_is_host_scoped_by_default_and_asks_to_choose_a_host` | no selected host: "Choose a host first" and no request; with `nas`: `scope=host&host=nas` |
| `global_search_is_a_deliberate_click_and_sends_no_host` | "Search all hosts" sets `?scope=global`; request has `scope=global` and no `host`; the warning "Searching every host" shows |
| `a_blank_search_lists_the_newest_operations` | request with an empty `q` |
| `results_show_outcome_verified_hosts_snippet_contracts_and_the_source_of_every_narrative_field` | from `search-host.json`; model text labelled "Drafted by the model, not checked"; HTML in a value appears literally |
| `a_failed_search_is_a_load_error_and_empty_results_say_so` | |
| `summary_states_not_ready_drafting_ready_and_failed` | nothing; `Progress`; fields; `LoadError` |
| `code_owned_fields_have_no_buttons_and_narrative_fields_show_confirm_and_edit` | |
| `confirm_sends_the_same_text_with_the_current_revision_and_says_nothing_changed_when_unchanged` | |
| `edit_sends_the_new_text_and_revision_and_a_stale_edit_shows_inside_the_dialog_and_refetches` | the draft text stays |
| `an_empty_or_too_long_edit_is_blocked_with_a_reason` | over 1000 characters |
| `a_missing_narrative_field_can_be_added_with_revision_zero` | |
| `summary_history_is_collapsed_in_standard_and_lists_revisions_with_sources` | |
| `similar_operations_show_reason_tags_and_do_not_load_until_opened_in_standard` | no request in Standard before the toggle; Detailed requests at once |
| `the_incident_page_shows_similar_operations_only_when_it_has_investigations` | |
| `the_operation_page_mounts_the_summary_only_for_a_finished_operation` | statuses `succeeded`, `running`, an unknown status |

### 12.9 Visibility and gates (`knowledgeVisibility.test.tsx`, `knowledgeGate.test.ts`, reading sources with `import.meta.glob("../../**/*.{ts,tsx}", { query: "?raw", eager: true })`, excluding tests)

| Test | Expected |
|---|---|
| `p0_visible_in_every_profile_for_the_knowledge_pages` | the named test of the visibility prompt, extended: `expectP0Everywhere` over the facts tab (`knowledge.facts`; `knowledge.waiting-note` when a proposal exists), the proposals tab (`knowledge.proposals`), the fact detail for a retracted fact (`fact.state`, `fact.status`, `fact.notes`) and for an inferred fact (`fact.actions`), the paths page with both fixtures (`paths.state`, `paths.blocked`, `paths.coverage`, `paths.list`), the experience page
  (`experience.label`, `experience.results`) and a finished operation page with a ready summary (`experience.summary`); `expectedIds` computed by an independent helper; at least one case holds each id so the test cannot pass vacuously |
| `p0_visible_in_every_profile_with_a_page_override` | default Detailed with override Essential for `paths`, and the reverse |
| `standard_collapses_origin_history_and_similar_operations_and_essential_hides_the_times` | `fact.origin`, `fact.history`, `experience.similar` are toggles in Standard; `fact.times` is "+5 details" in Essential |
| `approve_like_controls_are_available_in_every_profile` | in Essential: Accept, Retract, the password prompt and Confirm (disabled with its reason) are present |
| `the_profile_never_changes_which_requests_are_made_for_p0_content` | requests of the paths page are the same set in Detailed and Essential (apart from the preferences request) |
| `knowledge_write_hooks_are_only_used_by_explicit_controls` | `useAcceptFact(`, `useVerifyFact(`, `useConfirmPath(`, `useRetractFact(`, `useAssertFact(`, `useEditSummaryField(` appear only in their hook file and in the control files that call them on a click (`FactActions`, `ProposalsTab`, `FactSiblings`, `PathActions`, `AssertFactDialog`, `EditSummaryDialog`, `SummaryFieldRow`); none in a `useEffect` |
| `step_up_and_actions_contain_no_visibility_primitive` | `useStepUp.tsx`, `PathActions.tsx`, `FactActions.tsx` and `src/api/hooks/knowledge*.ts` contain none of `useVisibility`, `useProfile`, `Visible`, `presentationFor`, `DetailsScope`, `visibilityAttrs` |
| `every_p0_id_of_the_knowledge_table_appears_in_the_sources` | the 18 P0 ids of 6.8 each occur as an `id` in a source file; the P2 and P3 ids carry a `label` |
| `no_source_labels_a_path_as_covered` | the knowledge and experience sources contain no string literal matching `/covered|protected/i` except inside comments and tests |

### 12.10 Catalogue (`src/dev/KnowledgeDemos.tsx`, `knowledgeDemos.test.tsx`)

Demos in `/dev/design`: the knowledge statuses, path states and coverage states as pills; a `FactsTable` with every status; the paths banner blocked and not blocked; the coverage row; a password prompt with each of its errors; a `PastOperationCard`; the summary field rows with each source. `knowledge_demos_render_in_every_state` uses `skipAxe()` with a comment: a catalogue of fragments, like the other demo tests. Look at `/dev/design` in both themes.

### 12.11 Smoke test and screens

`scripts/smoke/server.mjs` answers (data from the JSON fixtures, read with `readFileSync` in `data.mjs`): `GET /api/v1/knowledge/facts`, `/facts/{id}`, `/hosts/{name}/management-paths`, `/relations`, `/entities`, `GET /api/v1/experience/search`, and `GET /api/v1/operations/{id}/summary` and `/similar`; `POST /facts/{id}/accept` and `/retract` (state changes). The rule that any other `/api` request fails the run stays. New checks after
those of the earlier prompts, each printing one line: **12. Knowledge**: `/knowledge` lists the facts; Proposals shows the inferred fact; opening a fact shows status, source and Accept; accepting a safety-critical proposal opens the password prompt and nothing is sent until it is confirmed. **13. Paths**: `/knowledge/paths?host=nas` shows "Cannot be verified yet", the blocked message, Confirm disabled with its reason, and **no** `POST .../confirm` is
captured when it is clicked; the page text never contains "covered". **14. Experience**: `/knowledge/experience` shows "Historical evidence, not authorization" and the cards with their sources; at 375 px no horizontal scroll on the four pages; axe in the real browser (including colour contrast) on the four pages in Detailed and Essential, both colour schemes; no `console.error`, no unexpected request. `screens.mjs` gains the pages of 4.4.

## 13. Acceptance criteria

1. `/knowledge` lists facts with source, status and freshness; it follows the host selector; status, source, kind and search are in the address; retracted and replaced facts are requested only when their chip is chosen; a search uses the search route with no cursor; a failed load is a `LoadError`, never an empty list.
2. Proposals are the inferred facts (`source=inferred`), counted on the tab from the same query; accepting one sends `expected_version` (and the password for a safety-critical predicate), and the original stays as history on the hub.
3. Fact detail shows status, source, value, who recorded it, evidence, freshness times, competing values and history, and offers exactly the actions `factActions` allows; accept moves the page to the new fact; retract never asks for a password; every 409 reads in the hub's words in place.
4. Relations and entities have lists, an add form, retract, rename and delete, a relation detail dialog and an entity page; the hub's 400 and 409 texts show inside the dialogs.
5. Asserting, accepting or verifying a `management_path` or `trusted_cidr`, and every confirm, ask for the password with the existing `PasswordPrompt` before any request; the password is sent once in the request body and kept nowhere; a wrong password reads "That password is not right."; a lockout shows its wait.
6. No request body from the web contains `source` or `actor`; the six write hooks of 12.9 are used only by explicit controls.
7. The management paths page shows paths with state, source, provenance, freshness and confirmation, the coverage state, and what is blocked with its reasons; while coverage is unavailable (the only state reachable in this release) Confirm is disabled with its reason, no confirm request can be sent, and **nothing on the page, in any table or in any test reads "covered" or "protected" as a status of a path**.
8. The blocked message follows `containment_block` and is display only: when not blocked it says other conditions still apply; it never says an action is allowed.
9. `/knowledge/experience` searches finished operations on one host by default, asks to choose a host when none is selected, searches every host only after a deliberate click and then sends no host; every past-operation view carries "historical evidence, not authorization"; every narrative field shows its source; text is rendered as text.
10. The resolution summary panel appears on a finished operation's page with fields, sources, Confirm, Edit and history; code-owned fields have no buttons; a stale edit is shown in the dialog and the summary refetches; similar past operations appear on the operation page and, when it has investigations, the incident page, and are not requested in Standard or Essential until opened.
11. Every element is classified as in 6.8; `p0_visible_in_every_profile` passes for the knowledge pages and the panels, with non-empty expected sets and with a page override in both directions; no P0 element is inside a P2 or P3 `Visible`; the four page keys are in `VISIBILITY_PAGES`.
12. Statuses, path states and coverage states come only from `tone.ts` lookups; no raw colour, inline style, native `title` or raw `<button>` was added in `src/features` or `src/app`; icons only through `Icon` and only from the existing set; every file is at most 250 lines; error text comes only from `describeError` plus the single-sentence field problems of `valueForm.ts`.
13. Navigation lists Knowledge, Management paths and Experience in the existing group, the command palette lists them, and the breadcrumbs read "Knowledge" and the page's own name.
14. `npm run format:check`, `lint`, `typecheck`, `test`, `build` (size within budget; numbers recorded in `DESIGN.md`), `smoke` (checks 12 to 14 included) and `gen:api` (no diff) pass; `/dev/design` shows the new states in both themes.
15. No Rust file, migration, config key, wire contract, OpenAPI file or generated file changed.
16. `web/DESIGN.md`, the book pages and the catalogue are updated (section 15); no committed file mentions prompts, the build plan, coding agents or AI assistance.

## 14. Migration and compatibility

- **Existing installs.** Nothing to migrate. The three navigation items appear for everyone; with no knowledge and orchestration off the pages show their empty states and the operation and incident pages are unchanged.
- **Hub pairing.** The embedded UI and the hub ship together. A dev server pointed at an older hub gets 404 on `/api/v1/knowledge/*` and shows `LoadError`s; nothing else breaks.
- **Coverage later.** When the helper attestation work lands and `coverage_available` becomes true, Confirm becomes active with no change to this page; showing a per-path coverage verdict needs a new field in the hub's DTO and a follow-up here. Do not guess one.
- **Other U4 pages.** 43, 49, 50 and 51 add their own page keys, routes and nav items beside these; none may edit `PRESENTATION` or the P0 ids of 6.8. 49's host page may link to `/knowledge/paths?host=<name>`.
- **Existing tests.** Tests that assert the navigation items or the command palette gain three entries; no other existing test changes. If 30's, 36's or 37's tests assert an exact list of `VISIBILITY_PAGES`, extend the list in them.

## 15. Documentation requirements

Write for the product's readers; never mention prompts, prompt numbers, the build plan or agents in any committed file.

- **`web/DESIGN.md`** (same PR): (a) "The shell": the three Knowledge navigation items. (b) "Components" or "Recipes": a recipe **"Ask for the password"** describing `useStepUp` over `PasswordPrompt` (when to ask, the wrong-password sentence, never stored, a form dialog is replaced by the prompt, never stacked), and a recipe **"Show knowledge"** (subject labels, `valueText`, the status and source words). (c) "How people use it": two short
  paragraphs, **Check what Domovoi knows** (proposals wait for your word, retracted facts are hidden, stale and disputed are labelled) and **Check the management paths** (coverage is unavailable until the helper check exists, Confirm is disabled with its reason, the blocked line is a display). (d) "Presentation profiles": the four page keys. (e) Statuses: add knowledge status, path state and coverage to the status list with the "Add a status" note, and
  **remove "knowledge state" from the Planned list of new statuses**; keep the rest of that sentence. (f) "Performance budget": record the numbers from `npm run size` (the pages are lazy chunks; the CSS total grows a little). (g) Keep "Checks that will fail your PR" unchanged (no new check).
- **Book** `docs/book/src/concepts/knowledge.md` (written with the knowledge store): add a section "The Knowledge pages": facts, proposals, retraction, disputes, the password for safety-critical changes, and the management paths page including that confirming a path is not yet possible and why (plain words). `docs/book/src/concepts/experience.md` (written with experience search): add "The Experience page and summaries": the search and its scope, confirming or editing a summary, similar
  operations, and the sentence that a past approval or success never lets anything run. If `guide/display.md` lists the pages that have their own detail level, add the four. Link `DocsLink` only to pages that exist; run `cargo xtask docs-links`.
- **Not changed:** `ARCHITECTURE.md`, the config reference, OpenAPI, `reference/api.md`, ADRs (no arch rule changes), `AGENTS.md` (no command changes).

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
