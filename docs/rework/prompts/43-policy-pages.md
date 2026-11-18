# 43 — Policy pages: editor, decision explanation, simulator and publication

| | |
|---|---|
| Branch | `policy-pages` |
| Milestone | U4 |
| Depends on (must be merged) | 37 (visibility categories), 40 (policy versions, shadow decisions, simulator and publication API) and 30 (operations UI: kit pieces, lazy-page and route pattern, smoke harness), and through them 39 (evaluator), 29, 28, 27, 26 and 59. **30, 37, 39 and 40 are written but may not be merged when this brief is read: every name taken from them below is "introduced by prompt NN — verify in code before use".** If any of the four is not merged, stop and report: this prompt is built on the 40 routes and on the 37 and 30 web code, and must not guess either. |
| Can run in parallel with | 48, 49, 50, 51 (order-flexible, never concurrent). They share `web/src/app/{nav.ts,pages.ts,routes.tsx,PageTitle.tsx}`, `web/src/api/{queryClient.ts,types.ts}`, `web/src/design/{tone.ts,icons.tsx,visibility.ts}`, `web/src/dev/DesignPage.tsx`, `web/scripts/smoke/*` and `web/DESIGN.md`: whichever runs second rebases on the first. 38 and 52 do not touch these pages |
| Produces (shared contracts) | (1) Pages `/policy`, `/policy/versions/:number`, `/policy/draft`, `/policy/draft/publish`, `/policy/decisions/:execution`, `/policy/simulator`. (2) The reusable `DecisionExplanation` (`web/src/features/policy/decision/`) that prompt 45 and the Control Room (51) embed. (3) Policy words: lookups `policyOutcomeMeta`, `ruleMarkMeta`, `decisionReasonMeta`, `hardLimitMeta`, `findingKindMeta`, `lockChangeMeta`, `shadowStatusMeta`, `comparisonMeta`, `gapInputMeta`, `gapReasonMeta`, `shadowReasonMeta`, `decisionErrorMeta`, `factStatusMeta`, `effectClassMeta` in `web/src/design/` (the **Locked Deny** treatment of D-26). (4) Hooks `web/src/api/hooks/{policy,policyCommands}.ts`. (5) Kit change: `PasswordPrompt` gains optional `children` and `title`; `design/messages.ts` gains `stepUpError`; `useLeaveGuard(dirty, what?)`. (6) Visibility page keys `policy`, `policy-draft`, `policy-decision`, `policy-simulator` and the category tables of section 6.6. (7) Policy fixtures `web/src/test/fixtures/policy/*.json` with the loader `web/src/test/policyFixtures.ts` |
| Consumes | 40: the eleven `/api/v1/policy/*` routes and their DTOs (`PolicyOverviewDto`, `VersionSummaryDto`, `VersionDetailDto`, `DraftDetailDto`, `DraftSavedDto`, `RuleDto`, `PreviewDto`, `FindingDto`, `PublishedDto`, `SimulationDto`, `DecisionDto`, `ShadowPageDto`, `ShadowSummaryRowDto`, `ShadowDetailDto`; Rust names are convention, verify the generated schema names). 39 only through those DTOs (`Outcome`, `OutcomeReason`, `HardLimit`, `Warning`, `FindingKind`). 37: `useVisibility`, `Visible`, `DetailsScope`, `VisibilityPage`, `VISIBILITY_PAGES`, `expectP0Everywhere`, rule 6.5. 30: lazy-page, route and navigation pattern, `design/operationVocab.ts` as the model for a vocabulary file, `npm run smoke` and its mock hub. Existing: the kit (section 3.2), `useHosts`, `useHostNames`, `describeError`, `useActionFeedback`, `useLeaveGuard`, `PasswordPrompt` |
| Launch gate | none. Time windows are UTC (owner decision D-30): the pages read `time_window_zone` from `GET /api/v1/policy` (it is `"UTC"`) and label every `TimeWindow` input and sentence with it. If the overview has no `time_window_zone` field, stop and report |
| Suggested model | medium: many small, well-specified files; the judgement is in keeping the step-up, the acknowledgement and the Locked Deny treatment honest, and every element in its visibility category |

## 1. Objective

Give the owner the screens for the policy that prompt 40 stores and records, built **only** on the eleven `/api/v1/policy/*` routes. This prompt changes no Rust, no wire contract and no OpenAPI document. Concretely:

- **Policy overview** (`/policy`): what the active policy is, whether decisions are being recorded, the hub's standing notes ("nothing here is enforced"), the active version's rules, the published versions and the recorded decisions.
- **Editor** (`/policy/draft`): one draft rule set. Create it from the active version or empty, add, edit, duplicate and remove rules, mark a rule **shadow** or a Deny **locked**, check the rules, save. A **locked Deny** reads differently everywhere (lock icon and the words "Locked Deny", never colour alone; D-26). `TimeWindow` inputs and sentences are labelled with the zone the hub reports (D-30). Saving a change to a locked rule shows the simulation preview of affected decisions and asks for the password (D-26).
- **Publication** (`/policy/draft/publish`): the publication report (every finding with its stable key and resolution), the **acknowledgement** of every blocking finding (D-19), the locked-rule changes with the simulation preview, then publish with a **step-up password**.
- **Decision explanation** (`/policy/decisions/:execution`): for one Execution step, what the active policy decided and what the shadow rules decided, with the stages made visible: matching rules, winners and why, overridden broader rules, the dangerous exception, locked Deny and what the rules would have said without it, the hard limits (shown apart from rules), the inputs that were unavailable, the constraints an Allow carries.
- **Simulator** (`/policy/simulator`): evaluate the draft or a published version over recorded decisions, with **recorded** inputs (a reproduction) or **current** inputs (a what-if, labelled as not a reproduction), with a baseline-versus-simulated diff per item and a summary.
- Every element carries a **visibility category** (37) and every status word comes from the design vocabulary.

Nothing here approves, permits or authorizes anything, and nothing offers an approval control next to a decision: a policy `Deny` says the owner must change the policy (D-23). Policy is **shadow-only** while this prompt ships: decisions are recorded for comparison and are not enforced, and the pages say so with the hub's own notes.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**The hub owns every policy decision; the web only shows them.** The evaluator is a pure function in `domovoi-engine` (39). Prompt 40 stores rule versions, records one decision per Execution step, and answers `preview` (publication report, locked-rule changes, a bounded simulation and a binding `preview_hash`), `simulate` and the publication commands. **The web never re-implements** evaluation, specificity, conflict detection, locked-change detection or the hash. A page that needs one of them asks `POST /api/v1/policy/preview` or `/simulate`. Two copies of "more specific" would drift and change a safety result.

**RW-10, in plain terms (what the pages must explain).** A rule has a scope (host, capability, target), optional conditions and an outcome: *Allow automatically*, *Ask for approval* or *Deny*. The **most specific** matching rule wins, even over a broader Deny (D-05). At equal scope Deny wins. When two matching rules are incomparable (neither is inside the other), the safer outcome wins: Deny, then Ask, then Allow (D-19). A **locked** Deny is an absolute prohibition that no narrower rule can override (D-26); creating, editing, unlocking or removing one needs the password, a simulation preview and is audited. **Hard limits** (the host's own ceiling, destructive never automatic, targets outside the pinned subjects, unknown safety facts, containment, root agent, pause) apply **after** the rules and are not rules: the pages show them in a separate block and never offer to edit them. A policy `Deny` is **never offered for approval** (D-23); an evaluation that cannot establish a safety input is `Cannot decide` (nothing runs, nothing is offered).

**Publication (D-19) and step-up (RW-24).** Publishing needs the password again even with a valid session. The hub requires the acknowledged keys to equal the blocking finding keys **exactly**, and a `preview_hash` that binds the exact rules, the active version number, the blocking keys and the locked changes. Saving a draft that creates, edits, unlocks or removes a locked rule needs the password **and** the matching `preview_hash`. A wrong password is 403 `forbidden`; a lockout is 429 and shares the login limiter. The UI sends the password only in the request body, keeps it only inside the kit's `PasswordPrompt` (cleared when it closes) and never stores or logs it.

**Shadow mode is not enforcement.** Prompt 40 records decisions for comparison; no approval, dispatch or limit reads them, and no `PolicyPermit` exists until prompt 45. The overview carries three standing notes (the hub's own text); the pages show them verbatim and label every decision "Not enforced". Several inputs do not exist before the helper and the ceiling ship; the hub records the fail-closed value and lists the gap, so a decision can be `degraded`. The pages show the gaps beside the result and **never** present a degraded or current-input result as an exact historical reproduction.

**D-30 (TimeWindow zone is UTC).** The hub reports the zone its evaluator uses as `time_window_zone` (for example `"UTC"`). The editor labels its time inputs, and every sentence that mentions a window, with that string. The web hard-codes no zone.

**Visibility profiles (37).** Detailed, Standard and Essential change what is drawn, never what is allowed. P0 (safety, approval, audit, current state) is never hidden or collapsed; anything with a decision control is P0. This prompt classifies its own screens with 37's rule (section 6.6) and wraps each page in `VisibilityPage`.

**Hub use-case layer (29, 40).** Every policy command is a hub service method taking `&Caller`; the browser only calls the routes. There is nothing to add on the hub.

**Decisions made here (do not revisit).**

| # | Decision | Why |
|---|---|---|
| D1 | No client-side evaluator, specificity, conflict or locked-change logic. The editor calls `preview` for them | one implementation, owned by the hub |
| D2 | The rule editor is a **section of the draft page** (`?rule=<id>` or `?rule=new`), not a modal and not a right-slot surface | the design pack forbids a modal as the only way to the full policy editor, and the right slot becomes a modal sheet on narrow screens |
| D3 | **Preview, then save.** Saving the working copy first calls `POST /policy/preview`; with no locked change it then saves with no password; with locked changes it opens the locked-change dialog (changes, simulation, password) and saves with `password` and the preview's `preview_hash` | the client never has to guess whether a change touches a locked rule |
| D4 | The publication page works on the **stored** draft. Unsaved edits disable "Review and publish" with a reason | the hub's `preview_hash` and publish bind the stored revision |
| D5 | The acknowledgement sent is exactly `preview.blocking` (the hub's list of keys), and the Publish button is enabled only when every blocking finding's box is ticked | exact equality is the hub's rule; the UI mirrors it |
| D6 | Outcome tones: Allow automatically `success`, Ask for approval `pending`, Deny `neutral` ("denied" is neutral in `web/DESIGN.md`), Cannot decide and Could not evaluate `warning`. **Locked Deny** is its own entry: `danger`, icon `lock`, label "Locked Deny" | three outcomes must differ at a glance; a locked rule must read differently from an ordinary Deny |
| D7 | Contract and category in a rule are **typed text** validated by shape; unknown names are reported by `preview` (`validation`) before publication | the 40 routes list no contracts; a picker needs a capability endpoint that belongs to prompt 49 |
| D8 | No link from the operation inspector or the approval card to the decision page, and the approval card's `extras.policy` slot is **not** filled from shadow decisions | 37 classifies those screens and its tests count their blocks; a shadow decision shown on an approval card would read as an authoritative policy decision |
| D9 | `DecisionExplanation` is one reusable component for a recorded decision and for a simulated one | the simulator and the decision page must read the same |
| D10 | Policy words live in new vocabulary files under `web/src/design/` and are re-exported from `tone.ts` (which is near the 250-line cap) | statuses reach the UI only through `tone.ts` |
| D11 | Not built here: the host-by-capability matrix, the autonomy pause switch and circuit-breaker display (prompt 45), a "dangerous exception" publication finding (the hub's report has four finding kinds), import and export | no backend for them; stated in section 5 |
| D12 | Kit changes: `PasswordPrompt` gains `children` and `title`; `useLeaveGuard` gains an optional `what`; `design/messages.ts` gains `stepUpError` | the locked-change and publish dialogs need content above the password field, and the draft guard must not say "note" |

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, or **(introduced by NN — verify)**. Paths are under `web/` unless they start with `docs/` or `crates/`. Line numbers were read on the committed tree before prompts 26–42 merged and may have moved.

### 3.1 Existing web code this prompt builds on

- **Kit barrel** `src/components/index.ts` **(existing)** is the only import point for features. Pieces used here: `PageHeader` (`title`, `intro`, `docs`, `meta`, `actions`), `Section`, `Card`, `Stack`, `Cluster`, `Grid`, `Text`, `StatTile`, `StatusPill`, `Tag` (`hint`), `Count`, `Banner`, `InlineMessage`, `InlineError`, `Progress`, `QueryBoundary`, `LoadError`, `EmptyState`, `Skeleton`, `Button` (`variant`, `icon`, `loading`, `disabledReason`, `hint`), `IconButton`, `LinkButton`, `Menu`, `Dialog`, `ConfirmDialog`, `PasswordPrompt`, `Field`, `TextInput` (accepts `type`, so `type="time"` and `type="date"` work), `NumberInput`, `Textarea`, `Select`, `Switch`, `Checkbox`, `SegmentedControl` (two to four choices), `FilterChips`, `Tabs` (`selected` + `onSelect` make it URL-controlled), `DataTable` (`columns`, `rows`, `rowKey`, `rowTone`, `rowHref`, `onRowClick`, `caption`, `empty`; keep `columns` stable with `useMemo`), `KeyValueList` (`mono`), `RelativeTime`, `InfoTip`, `Tooltip`, `SubmitBar`, `useActionFeedback`.
- `src/components/PasswordPrompt.tsx:13` **(existing)**: props `busy, confirmLabel, error?: string, onCancel, onConfirm(password), reason: string`. It renders `ConfirmDialog` titled "Confirm with your password", keeps the password in its own state and passes it only to `onConfirm`. It has **no** `children` and a fixed title; this prompt adds both (section 6.8).
- `src/features/settings/EditorForm.tsx:107-128` **(existing)**: the step-up pattern for settings: `submit(password?)`; a 403 while the prompt is open becomes the field error "That password is not right."; any other failure closes the prompt and is shown with `describeError`. Follow it, but put the sentence in `design/messages.ts` (section 6.8).
- `src/design/messages.ts:29` **(existing)** `HUB_CODES` (includes `forbidden`, `not_found`, `bad_request`, `invalid`, `conflict`, `too_many_requests`, `unavailable`); `:140` `describeError(error, context)` returns `{ title, detail, retryAfter?, issues? }`; `forbidden`, `bad_request`, `conflict` and `unavailable` are *specific* (the hub's own sentence is shown as the detail); `too_many_requests` with `retryAfter` reads "Too many attempts. Try again in N s.". Prompt 40 reuses these codes and adds none, so no catalogue entry is needed for a route's failure (if a policy route answers a code that is not in `HUB_CODES`, stop and report).
- `src/design/tone.ts:188` **(existing)** `VOCABULARIES` (file is 208 lines; 30 adds lookups to it); `src/design/status.ts:85` `lookup(table, value)` returns a neutral meta with the raw value for an unknown key; `StatusMeta { label, tone, hint, icon }`, `Table<K>`. `src/design/tone.test.ts` **(existing)** checks every vocabulary entry: non-empty label, tone in `TONES`, hint longer than 10 characters, icon in `ICON_NAMES`; and that an unknown value falls back to neutral with the raw label.
- `src/design/icons.tsx:52` **(existing)** closed `ICONS` map. It has `ban eye alert info clock check check-circle x-circle pause play edit trash plus search filter external refresh more close chevron-right chevron-down`; it has **no** `lock` and **no** `shield`. This prompt adds both (lucide `Lock`, `Shield`).
- `src/app/pages.ts:27` `PAGES` (lazy pages with `preload`); `src/app/nav.ts:36` `NAV_GROUPS`, group "System" (`:86`) holds only Settings; `src/app/routes.tsx:80` registers `/settings` from `PAGES`; `src/app/PageTitle.tsx` `defaultDetail(section, rest)` has a `switch` (`hosts`, `incidents`, `notes`) and finds the section label by `NAV_ITEMS.find(item.to === "/" + section)`. `routes.test.tsx`, `layout.test.tsx` and `palette.test.tsx` list the navigation items.
- `src/api/queryClient.ts:5` **(existing)** `keys`; `src/api/types.ts` **(existing)** aliases such as `type Action = Schemas["ActionDto"]`, `call<T>(request)` (throws `ApiFailure { status, code, message, retryAfter, issues[{path,message,severity}] }`), `defined(obj)` (drops empty fields for query strings). `src/api/hooks/hosts.ts:23,32` `useHosts()` and `useHostNames()` (host id to name). `src/api/hooks/settings.ts` **(existing)** is the model for a mutation that carries a `password`.
- `src/features/rules/RulesPage.tsx`, `AddRuleForm.tsx`, `rules.test.tsx` **(existing)**: the precedent for a list page with a form and for a test that mocks the client (`vi.mock("../../api/client", () => import("../../test/apiMock"))`, `route("GET /api/v1/...", handler)`, `calls`, `failure(status, code, message, headers?, issues?)`).
- `src/features/notes/useLeaveGuard.tsx:11` **(existing)** `useLeaveGuard(dirty)`: blocks in-app navigation and tab close while dirty; its dialog text says "your changes to this note" (this prompt makes the noun a parameter).
- `eslint.config.js` **(existing)**: in `src/features` and `src/app`: no inline `style`, no native `title=`, no raw `<button>`, `disabled` needs `disabledReason`, a file that calls `.mutate(` or `.mutateAsync(` must import `useActionFeedback`, `InlineMessage` or `InlineError` (or carry `// feedback-ok: <where>`), no deep import of a kit file, `lucide-react` only inside `Icon`, no `fetch` outside `src/api`; `max-lines` 250 (blank lines and comments not counted) for every non-test file under `src`.
- `src/test/{apiMock,render,setup,axe}.ts*` **(existing)**: `renderAt(url, path, element)` wraps `HostProvider`, a fresh `QueryClient`, `ToastProvider` and a data router and does **not** mount a visibility provider (no provider means Detailed); `setup.ts` runs axe over whatever a UI test leaves on screen (`skipAxe()` only for a deliberate fragment), clears `localStorage` after each test, and gives jsdom a `<dialog>`. An unregistered route throws `no test route for ...` and only fails that query.
- `tsconfig.json` **(existing)**: `strict`, `exactOptionalPropertyTypes`, `noUncheckedIndexedAccess`, `verbatimModuleSyntax`, `resolveJsonModule: true` (JSON fixtures can be imported).
- `scripts/screens.mjs:35-48` **(existing)**: the `PAGES` list of `[name, path]` pairs it screenshots at four widths in both themes.
- `web/DESIGN.md` **(existing)**: "Tones", "Components", "Planned" (its last paragraphs describe the **locked rule** treatment, D-26, and list "policy outcome" among the planned statuses), "Checks that will fail your PR", "Performance budget" (`<!-- budget:initial-js-gzip-kb 170 -->`).

### 3.2 What other prompts provide (introduced by — verify every name in code and in the generated `src/api/schema.d.ts`)

- **40**: the routes of section 6.1 and their JSON (40 section 6.10 is the contract). Facts the pages rely on: `PolicyOverviewDto { shadow_enabled, time_window_zone, active: VersionSummary | null, draft: DraftSummary | null, shadow: { last_sweep_at, last_error, total_evaluated, total_unevaluable, total_skipped, last_24h { total, by_status, by_enforced_outcome, degraded, differing } }, notes: string[] }`. `RuleDto` is the editor's own rule shape (`host`, `capability`, `targets`, `conditions`, `outcome`, `note`, `id`, `enabled`, `shadow`, `locked`). A `Deny` rule can be `locked`; `outcome: "unavailable"` is invalid input. Draft saves are whole-draft replacements guarded by `base_revision`. Status mapping: 403 `forbidden` for a missing or wrong password, 429 for a lockout, 400 (`invalid` with `issues[].path = "rules.<id>"`, or `bad_request`) for invalid input, 404 for no draft, no version or no recorded decision, 409 `conflict` for a stale draft, stale active version, stale preview or missing acknowledgement.
- **39** (through 40's DTOs): `Outcome` serialises as `{ "kind": "allow" }`, `{ "kind": "require_approval" }`, `{ "kind": "deny" }` or `{ "kind": "unavailable", "reason": "safety_unknown" }`; `OutcomeReason` is one of `default single_winner equal_scope_deny incomparable_fallback locked_deny`; `HardLimit` is one of ten snake_case names (section 6.3); `Warning` is `dangerous_exception { allow, deny }`, `safety_set_unverified` or `management_paths_unprotected { paths }`; `FindingKind` is `equal_scope_conflict incomparable_conflict ineffective_under_lock target_outside_every_ceiling`.
- **37**: `web/src/design/visibility.ts`, `useVisibility`, `Visible`, `DetailsScope`, `visibilityAttrs`, `VisibilityPage`, `VISIBILITY_PAGES`, `src/test/visibility.tsx` (`renderWithProfile`, `expectP0Everywhere`), gate tests under `src/features/operations/visibilityGate.test.ts` that scan `src/features` and `src/app` for `<Visible` usage (id grammar `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$`, `label` for P2 and P3, no `reveal` on P0 and P1).
- **30**: lazy pages and routes pattern, the nav item pattern, `design/operationVocab.ts` (a vocabulary file and its `tone.ts` lookups), `scripts/smoke.mjs` and `scripts/smoke/{server,data,checks}.mjs` (the mock hub that **fails the run on any `/api` request it does not list**), `npm run smoke` in CI job `web`.

### 3.3 Tests today

Vitest + Testing Library + jsdom, one `*.test.ts(x)` beside the code, `vi.mock` of `api/client` with `src/test/apiMock.ts`, axe in every UI test. Guardrails that must keep passing unchanged: `guardrails.test.ts`, `styles/cssGuard.test.ts`, `styles/contrast.test.ts`, `voice.test.ts` (forbids "Oops", "Something went wrong", "Error:" and "!" in text), `design/tone.test.ts`, `design/messages.test.ts`, and 37's `visibilityGate.test.ts`.

## 4. Exact scope

All paths are under `web/` unless stated. Every source file stays at or under 250 lines (blank lines and comments are not counted by the rule; plan for it). **(new)** means the file does not exist yet. No Rust file, migration, `Cargo.toml`, OpenAPI file or hub code changes.

### 4.1 Data layer and words

| File | Change |
|---|---|
| `src/api/types.ts` | aliases for the generated policy schemas (6.1): `PolicyOverview`, `PolicyVersionSummary`, `PolicyVersionDetail`, `PolicyVersionPage`, `PolicyDraft`, `PolicyDraftSaved`, `PolicyRule`, `PolicyPreview`, `PolicyFinding`, `PolicyPublished`, `PolicySimulation`, `PolicySimulationItem`, `PolicyDecision`, `ShadowPage`, `ShadowRow`, `ShadowDetail`, `LockChange` |
| `src/api/queryClient.ts` | `keys.policy` (prefix) and the keys of 6.4 |
| `src/api/hooks/policy.ts` **(new)** | the read hooks (6.4) |
| `src/api/hooks/policyCommands.ts` **(new)** | the command hooks (6.4) |
| `src/design/policyVocab.ts`, `policyLimits.ts`, `policyRecord.ts` **(new)** | the vocabulary tables of 6.3, split to stay under the cap |
| `src/design/policyMeta.ts` **(new)** | the lookups and `POLICY_VOCABULARIES` (6.3) |
| `src/design/tone.ts` | three lines: `export * from "./policyMeta"` style re-export of the lookups, and `...POLICY_VOCABULARIES` in `VOCABULARIES` (if `tone.ts` would pass 250 counted lines, this is the only change; never move existing code) |
| `src/design/icons.tsx` | `lock` and `shield` |
| `src/design/messages.ts`, `messages.test.ts` | `stepUpError` (6.8) |
| `src/design/visibility.ts` | append the four page keys to `VISIBILITY_PAGES` (6.6) |
| `src/components/PasswordPrompt.tsx` | optional `children` and `title` (6.8); existing callers unchanged |
| `src/features/notes/useLeaveGuard.tsx` | `useLeaveGuard(dirty, what = "note")`; the dialog says `your changes to this ${what}` |

### 4.2 Shell, routes, navigation

| File | Change |
|---|---|
| `src/app/pages.ts` | lazy pages `policy`, `policyVersion`, `policyDraft`, `policyPublish`, `policyDecision`, `policySimulator` |
| `src/app/routes.tsx` | the six routes of 6.5 inside `<Layout/>` from `PAGES` |
| `src/app/nav.ts` | item `{ to: "/policy", label: "Policy", icon: "shield", preload: PAGES.policy.preload }` in group "System", before Settings; **no count** (the overview has none) |
| `src/app/PageTitle.tsx` | `defaultDetail`: `policy` with a rest gives "Draft", "Publish", "Decision", "Simulator" or "Version N" (6.5) |
| `src/app/routes.test.tsx`, `layout.test.tsx`, `palette.test.tsx` | add "Policy" to the lists of navigation items they assert (edit only what lists items) |

### 4.3 Policy feature (`src/features/policy/`, all **(new)**)

| Area | Files |
|---|---|
| Shared | `policy.module.css` (placement only, tokens only), `policyText.ts` (fixed copy of 6.9), `policyZone.ts` (`useTimeWindowZone`), `RuleMarks.tsx` (`OutcomePill`, `RuleMarks`), `RulesTable.tsx`, `ruleText.ts` (pure sentences: `scopeText`, `conditionText`, `actorText`), `ruleModel.ts` (form model, `fromDto`, `toDto`, `validateRule`, `newRule`), `ruleIssues.ts` (hub `issues` to a map by rule id), `PolicyNotes.tsx` (standing notes and the shadow-mode pill) |
| Overview | `PolicyPage.tsx` (header, `Tabs` Rules, Versions, Decisions with `?tab=`), `ShadowCard.tsx`, `ActiveSummary.tsx`, `ActiveRules.tsx`, `DraftCard.tsx`, `VersionsTab.tsx`, `DecisionsTab.tsx`, `VersionPage.tsx` |
| Editor | `DraftPage.tsx`, `StartDraft.tsx`, `useDraftState.ts` (working copy, dirty, add, replace, remove, duplicate), `useSaveDraftFlow.tsx` (preview then save, 6.7), `LockedChangeDialog.tsx`, `DraftChecks.tsx` (the "Check rules" result), `RuleEditor.tsx`, `RuleScopeFields.tsx`, `ConditionFields.tsx`, `DiscardDraft.tsx` |
| Publication | `PublishPage.tsx`, `Findings.tsx`, `LockedChanges.tsx`, `SimulationSummary.tsx` (shared with the editor and simulator), `PublishControls.tsx` |
| Decision | `DecisionPage.tsx`, `decisionModel.ts` (`decisionOf`, the one adapter over the generated `DecisionDto`), `decision/DecisionExplanation.tsx`, `decision/OutcomeBlock.tsx`, `decision/LockedBlock.tsx`, `decision/WhyBlock.tsx`, `decision/LimitsBlock.tsx`, `decision/GapsBlock.tsx`, `decision/ConstraintsBlock.tsx`, `decision/DetailBlocks.tsx` (matched rules, facts, inputs, metadata), `DecisionList.tsx` (the table of recorded decisions, shared by the Decisions tab) |
| Simulator | `SimulatorPage.tsx`, `SimulatorForm.tsx`, `SimulatorResults.tsx`, `simulatorState.ts` (the URL parameters), `DecisionCompare.tsx` |

### 4.4 Tests, fixtures, catalogue, smoke, docs

| File | Change |
|---|---|
| `src/test/fixtures/policy/*.json` **(new)**, `src/test/policyFixtures.ts` **(new)** | the fixtures of 12.1 and a typed loader |
| `src/features/policy/*.test.ts(x)`, `src/design/policyVocab.test.ts`, `src/api/hooks/policy.test.ts`, `src/components/passwordPrompt.test.tsx` **(new)** | the tests of section 12 |
| `src/dev/PolicyDemos.tsx`, `src/dev/policyDemos.test.tsx` **(new)**, `src/dev/DesignPage.tsx` | catalogue: every policy status, `RuleMarks`, `OutcomePill`, `DecisionExplanation` (allow, locked Deny, limits and gaps, unavailable, error), `PasswordPrompt` with content |
| `scripts/smoke/{server,data,checks}.mjs` (30, 37) | the mock answers the policy routes the checks use; checks 12 to 15 (12.5) |
| `scripts/screens.mjs` | add `["policy","/policy"]`, `["policy-draft","/policy/draft"]`, `["policy-simulator","/policy/simulator"]` and, when `SCREENS_DECISION` names an execution id, `["policy-decision","/policy/decisions/<id>"]` |
| `DESIGN.md`, `../docs/book/src/concepts/policy.md` | section 15 |

### 4.5 Size and chunk budget

The pages are lazy (`PAGES`), so they cost nothing in the first download except what `tone.ts` imports eagerly (the vocabulary tables, about 3 kB gzip) and the navigation item. Record `npm run size` before and after. If the initial JavaScript passes the budget in `DESIGN.md`, first shorten hints; only then raise the budget with the reason in the PR and record the new number, as `DESIGN.md` says.

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Any Rust, migration, wire, config or OpenAPI change; any new hub route. **If a page seems to need one, stop and report** | none |
| Evaluating anything in the browser: specificity, conflicts, locked-change detection, the hash, the decision itself | the hub (39, 40) |
| The host-by-capability matrix of the design pack (needs "effective policy per host and capability", which no route returns) | a later prompt with a backend route |
| The autonomy pause switch, rules suspended by a circuit breaker, permit issuance, autonomy state | 45 |
| A publication finding for "a specific Allow overrides a broader Deny" (the hub's report has four finding kinds; deriving it in the browser would be a second implementation of specificity). The exception is shown in decision explanations and simulator results through the hub's `dangerous_exception` warning | report as a gap; a later hub change adds the finding |
| A picker for contracts, categories and fact ids (no route lists them). The editor takes typed text; `preview` names unknown ones | 49 (Capability Center) |
| Filling `ApprovalCard`'s `extras.policy` from shadow decisions, a policy block in the operation inspector, links from operation screens to the decision page | D8: a follow-up with 37's block counts |
| Selecting rules of the **shadow** set separately in the simulator (the simulator offers Enforced, Shadow or Both through the hub) | covered by the hub's `selection` |
| Inline-rules simulation from the UI (the route accepts it; the editor uses `preview` for unsaved rules) | not needed |
| Import and export of rules, policy-as-code | after v1 (design pack P2) |
| Deleting or editing a published version (impossible by design), re-basing a stale draft automatically | the hub refuses; the page offers "Reload the draft" |
| Live updates: `policy.published` maps to no stream topic. Pages refetch on mount and focus only | 28 does not carry policy |
| Retention of shadow rows, pagination beyond "Load more" | 40 / later |
| Saving the simulator's draft changes ("Save/apply from the simulator") | the design pack says apply only after review; the simulator links to the editor |
| Changing prompt 30's operation pages, the approval card or any approval rule; changing 37's `PRESENTATION` table | out of scope |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly; tests and later prompts name them) or **convention** (follow repository style). Every kit component, hook and exported function gets a JSDoc saying what it is for.

### 6.1 Routes, hooks and generated types **fixed** (paths, hook names), **convention** (alias names until verified)

| Hub route (40) | Hook | Used by |
|---|---|---|
| `GET /api/v1/policy` | `usePolicyOverview()` | every page (zone, notes, mode, draft presence) |
| `GET /api/v1/policy/versions?cursor=&limit=` | `usePolicyVersions(limit)` | Versions tab, the simulator's source list |
| `GET /api/v1/policy/versions/{number}` | `usePolicyVersion(number)` | active rules, `VersionPage`, `StartDraft` |
| `GET /api/v1/policy/draft` | `usePolicyDraft(enabled)` | editor, publication |
| `PUT /api/v1/policy/draft` | `useSaveDraft()`, `useStartDraft()` | editor, `StartDraft` |
| `DELETE /api/v1/policy/draft?revision=` | `useDiscardDraft()` | `DiscardDraft` |
| `POST /api/v1/policy/preview` | `usePreviewRules()` (mutation), `useDraftPreview(draft)` (query) | save flow, "Check rules", publication |
| `POST /api/v1/policy/draft/publish` | `usePublishDraft()` | `PublishControls` |
| `POST /api/v1/policy/simulate` | `useSimulate()` | simulator |
| `GET /api/v1/policy/shadow?host=&contract=&status=&outcome=&differing=&since=&until=&cursor=&limit=` | `useShadowDecisions(filters)` | Decisions tab |
| `GET /api/v1/policy/shadow/{execution_id}` | `useShadowDecision(execution)` | `DecisionPage` |

Aliases in `src/api/types.ts` (the generated names are 40's DTO names; verify each in `schema.d.ts` and write down any difference): `PolicyOverview = Schemas["PolicyOverviewDto"]`, `PolicyVersionSummary = ["VersionSummaryDto"]`, `PolicyVersionDetail = ["VersionDetailDto"]`, `PolicyVersionPage = ["VersionPageDto"]`, `PolicyDraft = ["DraftDetailDto"]`, `PolicyDraftSaved = ["DraftSavedDto"]`, `PolicyRule = ["RuleDto"]`, `PolicyPreview = ["PreviewDto"]`, `PolicyFinding = ["FindingDto"]`, `PolicyPublished = ["PublishedDto"]`, `PolicySimulation = ["SimulationDto"]`, `PolicyDecision = ["DecisionDto"]`, `ShadowPage = ["ShadowPageDto"]`, `ShadowRow = ["ShadowSummaryRowDto"]`, `ShadowDetail = ["ShadowDetailDto"]`. `npm run gen:api` must leave **no** diff in `src/api` (this prompt adds no hub code); if it does, a prerequisite is wrong: stop and report.

### 6.2 What the pages assume about the DTOs, and the one adapter **fixed**

- **Timestamps** are RFC 3339 strings, **ids** are strings, **actors** are `user:<uuid>` or `system:<name>`; show them with `actorText` (`user:` gives "a person", `system:<x>` gives "Domovoi", anything else the raw text, like `DecidedList.tsx` does).
- **Outcomes in rows** (`enforced_outcome`, `shadow_outcome`, `finding.resolution`, `by_enforced_outcome` keys) are the snake_case strings `allow require_approval unavailable deny error`; an unknown string renders through `policyOutcomeMeta` as a neutral pill with the raw value.
- **`DecisionDto`** carries `outcome`, `reason`, `matched`, `winners`, `locked`, `overridden [{broader, by}]`, `would_have_been` (a nested decision or null), `constraints { max_per_hour, max_ttl_secs, targets, conditions[] }`, `hard_limits_applied[]`, `facts_used [{fact, status}]`, `typed_confirmation`, `warnings[]`, `policy_version`, `ceiling_hash`; for an evaluation error it carries an error code instead (40 6.10: "`{ "error": "<code>" }` replaces `decision`"). **Check in step 1 how the generated type expresses that union, and how `constraints.conditions` are written**: they are 39's `Condition` serde (`TimeOfDay` may be `{hour, minute}` and `MaxTtl` a duration), unlike `RuleDto`'s (`"HH:MM"`, `max_ttl_secs`). Do not guess: read the generated schema and the Rust DTO, write down the shapes, and implement the one adapter below. If a shape cannot be rendered, stop and report; never add a hub route.

```ts
// src/features/policy/decisionModel.ts (new, pure). The ONLY place that reads the raw DecisionDto.
export type ConditionView =
  | { kind: "arg_constraint"; arg: string; op: "int_at_least" | "int_at_most"; value: number }
  | { kind: "time_window"; start: string; end: string }          // "HH:MM"
  | { kind: "requires_fact"; fact: string; expected: boolean }
  | { kind: "max_rate"; maxPerHour: number }
  | { kind: "max_ttl"; seconds: number | null; raw: string | null } // raw when the duration text was not understood
  | { kind: "unknown"; raw: string };                             // a condition this page does not know: shown as such, never dropped
export interface DecisionDoc {
  outcome: string;                  // "allow" | "require_approval" | "deny" | "unavailable" (unknown strings pass through)
  unavailableReason: string | null; // "safety_unknown" for unavailable
  reason: string;                   // default single_winner equal_scope_deny incomparable_fallback locked_deny
  matched: string[]; winners: string[]; locked: string[];
  overridden: { broader: string; by: string }[];
  wouldHaveBeen: DecisionDoc | null;
  constraints: { maxPerHour: number | null; maxTtlSeconds: number | null; targets: string[] | null; conditions: ConditionView[] };
  limits: string[]; facts: { fact: string; status: string }[];
  typedConfirmation: boolean;       // true when the owner must type a confirmation (informational here)
  warnings: WarningView[];          // { kind: "dangerous_exception"; allow; deny } | { kind: "safety_set_unverified" } | { kind: "management_paths_unprotected"; paths: string[] } | { kind: "unknown"; raw: string }
  ceilingHash: string | null;
}
export type DecisionView = { kind: "decision"; doc: DecisionDoc } | { kind: "error"; code: string };
export function decisionOf(dto: PolicyDecision | null | undefined): DecisionView | null;   // null for null/undefined
export function parseDuration(text: string): number | null;  // ISO 8601 ("PT10M", "PT1H30M") and friendly ("10m", "1h 30m"); anything else null
```

Everything else renders a `DecisionView`; no component reads a `DecisionDto` directly.

### 6.3 Policy vocabularies **fixed** (keys, labels, tones, icons), **convention** (hint wording)

Tables live in `design/policyVocab.ts`, `policyLimits.ts` and `policyRecord.ts` as `Table<K>`; lookups and `POLICY_VOCABULARIES` (an array of `{ name, meta, values }` shaped like the entries of `VOCABULARIES`) live in `design/policyMeta.ts`; `tone.ts` re-exports the lookups and spreads `POLICY_VOCABULARIES` into `VOCABULARIES`, so the existing `tone.test.ts` covers every entry. Every lookup goes through `lookup()` (unknown value: neutral pill with the raw text). Each hint is one plain sentence of more than ten characters saying what the status means; the wording is yours, **tests assert labels, tones and icons only**, except the hints written out below. Sentence case, no exclamation marks. A label that is a whole string literal never appears in feature code (gate 12.7).

`policyOutcomeMeta` (key: label, tone, icon):

| key | label | tone | icon | hint (fixed) |
|---|---|---|---|---|
| `allow` | Allow automatically | success | `check-circle` | It runs without asking you, within the limits shown. |
| `require_approval` | Ask for approval | pending | `clock` | You are offered the action and decide. |
| `deny` | Deny | neutral | `ban` | It is never offered. To allow it, change the policy. |
| `unavailable` | Cannot decide | warning | `alert` | A safety input could not be confirmed. Nothing runs and nothing is offered. |
| `error` | Could not evaluate | warning | `alert` | The rules could not be evaluated, so no decision was reached. |

`ruleMarkMeta`: `locked_deny` **Locked Deny** (danger, `lock`; hint "An absolute prohibition. No narrower rule can override it, and changing it needs your password."); `shadow` Shadow (info, `eye`; "Recorded for comparison and never acted on."); `disabled` Disabled (neutral, `pause`; "Ignored: it neither matches nor explains."); `dangerous_exception` **Allow overrides a broader Deny** (warning, `alert`; "A specific Allow beats a broader Deny. This is deliberate when you wrote it, and dangerous if you did not."); `not_enforced` Not enforced (info, `info`; "Recorded for comparison. No approval, dispatch or limit reads it.").

`decisionReasonMeta` (neutral, `info`): `default` Default for this kind of action; `single_winner` Most specific rule; `equal_scope_deny` Equal scope, Deny wins; `incomparable_fallback` Conflicting rules, safer outcome; `locked_deny` A locked rule applies. Hints: what stage decided it (for `incomparable_fallback`: "Neither rule is more specific, so the safer outcome applies: Deny, then Ask for approval, then Allow.").

`hardLimitMeta` (the ten of 39, in this order; tone `warning` unless noted; icon in parentheses):

| key | label |
|---|---|
| `destructive_never_allowed` | Destructive actions are never automatic (`alert`) |
| `target_not_pinned` | Target is outside the pinned subjects (`ban`) |
| `safety_fact_unknown` | A safety check could not be confirmed (`help`) |
| `ceiling_does_not_cover` | The host's own ceiling does not cover it (`shield`) |
| `containment_unverified` | Protected paths could not be verified (`shield`) |
| `management_path_uncovered` | Some management paths are not protected (`shield`) |
| `containment_target_excluded` | The target is excluded from containment (`ban`, tone `danger`) |
| `helper_safety_incomplete` | The helper cannot compute its safety set (`alert`) |
| `autonomy_inactive` | Autonomy is paused or a rule is suspended (`pause`) |
| `agent_runs_as_root` | The agent runs as root (`alert`) |

`findingKindMeta`: `equal_scope_conflict` Equal-scope conflict (warning, `alert`); `incomparable_conflict` Conflicting overlap (warning, `alert`); `ineffective_under_lock` Overridden by a locked rule (danger, `lock`); `target_outside_every_ceiling` Outside every host ceiling (info, `info`). `lockChangeMeta`: `created` Locked rule added, `edited` Locked rule changed, `unlocked` Rule unlocked, `removed` Locked rule removed (all danger, `lock`). `shadowStatusMeta`: `evaluated` Recorded (success, `check-circle`), `unevaluable` Could not evaluate (warning, `alert`), `skipped` Skipped (neutral, `skip`). `comparisonMeta`: `same` Same (success, `check`), `changed` Changed (warning, `alert`), `not_replayable` Cannot replay (neutral, `help`). `factStatusMeta`: `known` Known (success), `unknown` Unknown (warning), `stale` Stale (warning), `disputed` Disputed (warning). `effectClassMeta`: `read` Read (info, `eye`), `mutate` Mutate (neutral, `edit`), `contain` Contain (warning, `shield`), `destructive` Destructive (danger, `trash`).

`gapInputMeta` (neutral, `info`): `contract` Capability, `target` Target, `ceiling` Host ceiling, `facts` Safety facts, `runs_as_root` Agent privilege, `containment` Containment cover, `autonomy` Autonomy state, `pinned_targets` Pinned targets. `gapReasonMeta` (warning, `alert`): `not_produced_yet` Not available yet; `host_not_connected` Host not connected; `unresolvable` Could not be established. `shadowReasonMeta` (neutral, `info`; the `reason` of a row): `contract_unknown` Capability not known, `target_unresolved` Target not resolved, `target_kind_unsupported` Target kind not supported, `package_contract` Package action, not evaluated, `too_large` Record too large, `late` Evaluated late. `decisionErrorMeta` (warning, `alert`): the twelve `PolicyError` codes of 39 in snake_case (`locked_not_deny unavailable_rule_outcome duplicate_rule_id empty_target_set degenerate_time_window bad_arg_key unknown_contract category_not_in_index contract_not_indexed containment_on_non_contain ceiling_hash_missing too_many_facts`), each with a short label and a hint; an unknown code is the neutral raw pill.

Colour never carries meaning alone: every pill has its label; the **Locked Deny** treatment is the `lock` icon plus the label in the pill (`StatusPill meta={ruleMarkMeta("locked_deny")}`), used identically in the rules table, the editor, the locked-change dialog, the publication report, the decision explanation and the simulator (D-26).

### 6.4 Query keys and hooks **fixed** (names, request shapes), **convention** (layout)

```ts
// src/api/queryClient.ts  (add; every key starts with "policy" so one invalidation covers all)
policy: ["policy"] as const,
policyOverview: ["policy", "overview"] as const,
policyVersions: (limit: number) => ["policy", "versions", limit] as const,
policyVersion: (number: number) => ["policy", "version", number] as const,
policyDraft: ["policy", "draft"] as const,
policyPreview: (revision: number, rulesHash: string) => ["policy", "preview", revision, rulesHash] as const,
policyShadow: (filters: object) => ["policy", "shadow", filters] as const,
policyDecision: (execution: string) => ["policy", "decision", execution] as const,
```

```ts
// src/api/hooks/policy.ts  (reads; every request error propagates to QueryBoundary / LoadError)
export interface ShadowFilters { host?: string | undefined; contract?: string | undefined; status?: string | undefined;
  outcome?: string | undefined; differing?: boolean | undefined; since?: string | undefined; until?: string | undefined }
export const SHADOW_PAGE = 25;
usePolicyOverview()                       // GET /api/v1/policy
usePolicyVersions(limit = 25)             // useInfiniteQuery; GET .../versions with defined({ cursor, limit }); getNextPageParam = next_cursor
usePolicyVersion(number: number | undefined)   // GET .../versions/{number}; enabled only with a number
usePolicyDraft(enabled: boolean)          // GET .../draft; the pages enable it only when overview.draft is not null (no 404 is mapped to "empty")
useDraftPreview(draft: PolicyDraft | undefined) // useQuery; POST .../preview { base_revision: draft.revision, rules: draft.rules };
                                          // key policyPreview(draft.revision, draft.rules_hash); staleTime 0; refetchOnWindowFocus false
useShadowDecisions(filters, enabled = true)    // useInfiniteQuery; GET .../shadow with defined({ ...filters, differing: true|undefined, cursor, limit: SHADOW_PAGE })
useShadowDecision(execution: string | undefined) // GET .../shadow/{execution_id}; a 404 is an error the page words as "No decision recorded yet"
```

```ts
// src/api/hooks/policyCommands.ts  (mutations; call sites wrap each in useActionFeedback)
useStartDraft()    // { from: number | null } -> reads version `from` (rules) or uses [], then PUT /policy/draft { base_revision: null, rules }
useSaveDraft()     // { baseRevision: number | null; rules: PolicyRule[]; password?: string; previewHash?: string }
                   //   -> PUT /policy/draft { base_revision, rules, password?, preview_hash? } (password and preview_hash omitted when undefined)
useDiscardDraft()  // { revision: number } -> DELETE /policy/draft?revision=<n>
usePreviewRules()  // { baseRevision: number | null; rules: PolicyRule[] } -> POST /policy/preview { base_revision, rules }
usePublishDraft()  // { revision: number; previewHash: string; acknowledged: string[]; password: string }
                   //   -> POST /policy/draft/publish { revision, preview_hash, acknowledged, password }
useSimulate()      // SimulateInput -> POST /policy/simulate { rules, selection, inputs, filter, execution_ids, limit }
export interface SimulateInput {
  rules: { source: "draft" } | { source: "version"; number: number };
  selection: "enforced" | "shadow" | "both";
  inputs: "recorded" | "current";
  filter: { host?: string; contract?: string; status?: string; since?: string; until?: string };  // empty values omitted
  executionIds: string[];
  limit: number;                                                                                   // 1..500
}
```

`useStartDraft`, `useSaveDraft`, `useDiscardDraft` and `usePublishDraft` invalidate `keys.policy` on success (a prefix: overview, versions, draft, previews, decisions). `usePreviewRules` and `useSimulate` change nothing and invalidate nothing. None retries (the client default), none sets query data by hand, and none logs a request body. A failed query is never read as empty.

### 6.5 Routes and pages **fixed** (paths, behaviour), **convention** (markup)

```text
/policy                        PAGES.policy          ?tab=rules|versions|decisions   (rules = no parameter)
/policy/versions/:number       PAGES.policyVersion
/policy/draft                  PAGES.policyDraft     ?rule=<rule id>|new
/policy/draft/publish          PAGES.policyPublish
/policy/decisions/:execution   PAGES.policyDecision
/policy/simulator              PAGES.policySimulator ?rules=draft|version-<n>&execution=<id>&selection=enforced|shadow|both&inputs=recorded|current&item=<execution id>
```

`PageTitle.defaultDetail("policy", rest)`: `versions/<n>` gives "Version <n>", `draft` "Draft", `draft/publish` "Publish", `decisions/<id>` "Decision", `simulator` "Simulator". Every page is wrapped in `VisibilityPage` (6.6) and renders `<AllHostsNote>` ("Policy applies to every host. A rule names a host itself.") except the Decisions tab and the simulator, which follow the host selector (they send the selected host's **id** as `host`; with a host selected and the host list still loading the query waits, so nothing unfiltered flashes).

**`/policy` (`PolicyPage`).** `PageHeader title="Policy" intro="What Domovoi may do without asking you, what it asks you about and what it never offers." docs="concepts/policy.html"`; `actions`: a primary `Button` "Edit draft" (to `/policy/draft`) when `overview.draft` exists, else "Start a draft" (`StartDraft` flow: `useStartDraft({ from: active?.number ?? null })`, then navigate to `/policy/draft`; the button's `hint` says "Copies the active version's rules into an editable draft."), and a secondary `LinkButton` "Simulator". Under it, in order: `PolicyNotes` (the hub's `notes` verbatim, each an `InlineMessage tone="info"`), `ShadowCard`, `ActiveSummary`, `DraftCard` (only with a draft), then `Tabs label="Policy views"` (`selected` from `?tab=`): **Rules** (`ActiveRules`: `usePolicyVersion(active.number)` in its own `QueryBoundary context="load the rules"`; read-only `RulesTable`), **Versions** (`VersionsTab`: `DataTable` Version link and "Active" tag, Published, By, Rules, Locked, Shadow; "Load more"; empty "No versions yet"), **Decisions** (`DecisionsTab`: `FilterChips label="Show"` All decisions, Where the two differ (`differing=true`), Could not evaluate (`status=unevaluable`); `DecisionList` whole-row links to `/policy/decisions/<execution_id>`; columns Step (contract mono and host name), Evaluated, Active policy (`OutcomePill`, or the shadow-status pill when not evaluated), Shadow rules, Policy ("version N" or "no policy"), flags (`Tag` "Degraded" with the hint that an input was unavailable; the `late` reason as a `Tag`)). With no active version `ActiveSummary` says: "No policy is published yet. Without rules, reads can run automatically and everything else asks you. Start a draft to write the first rules." `ShadowCard`: with `shadow_enabled` false an `InlineMessage tone="info"` "Decisions are not being recorded. Set shadow to true under [policy] in hub.toml and restart the hub to start."; with a `last_error` an `InlineMessage tone="danger"` titled "The last recording pass failed" with the hub's text; `StatTile`s for the last 24 hours (Recorded, Degraded, Differing, Could not evaluate) and a line "Last pass {RelativeTime}" or "No pass yet". The Decisions tab with recording off and no rows is an `EmptyState` saying so; with recording on and no rows, "No decisions yet. One is recorded for each action an operation proposes."

**`/policy/versions/:number` (`VersionPage`).** A non-numeric or unknown number (404) is `EmptyState "No such version"` with a link back; other failures `LoadError "load the version"`. Header title `Version N`; `meta`: `Tag` "Active" (tone success) or "Replaced by version M"; `actions`: secondary `Button` "Start a draft from this version" (`disabledReason` "A draft already exists. Edit or discard it first." when `overview.draft`), `LinkButton` "Simulate". Blocks: summary `KeyValueList` (published, by, rules, locked, shadow, time-window zone), the read-only `RulesTable`, **Acknowledgements** (a list: finding-kind pill, the two rules, resolution `OutcomePill`, who and when; or "No conflicts needed acknowledging for this version."), and the rules fingerprint as metadata.

**`/policy/draft` (`DraftPage`)** — the editor, 6.7. **`/policy/draft/publish` (`PublishPage`)** — 6.7. **`/policy/decisions/:execution` (`DecisionPage`)** and **`/policy/simulator` (`SimulatorPage`)** — 6.7.

### 6.6 Visibility categories **fixed** (37's rule 6.5 applied; the tests name these ids)

Page keys appended to `VISIBILITY_PAGES`: `{ key: "policy", label: "Policy overview and versions" }`, `{ key: "policy-draft", label: "Policy editor and publication" }`, `{ key: "policy-decision", label: "Policy decision" }`, `{ key: "policy-simulator", label: "Policy simulator" }`. Mark: **V** = `<Visible>`, **A** = `visibilityAttrs`, **S** = a `DetailsScope`. Ids name the element kind (grammar `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$`). A P0 element is never inside a P2 or P3 block. The `PageHeader` title, intro and docs link, dialogs, toasts, menus and tooltips carry no category. **A Visible never wraps a decision control's code**: `PublishControls.tsx`, `LockedChangeDialog.tsx`, `useSaveDraftFlow.tsx` and `StartDraft.tsx` contain none; the **page** wraps them.

| id | element | cat | mark |
|---|---|---|---|
| `policy.state` | loading skeletons, `LoadError`, `EmptyState`, "no such version", the 404 of a decision (every policy page) | P0 | V |
| `policy.mode` | the hub's standing notes (`PolicyNotes`); on the publication page too | P0 | V |
| `policy.shadow` | `ShadowCard`: recording off, last error, last pass, the last-24-hours tiles | P0 | V |
| `policy.active` | `ActiveSummary`: version, published by and when, counts, the time-window zone, or "no policy yet" | P0 | V |
| `policy.rules` | the rules table of the active version | P0 | V |
| `policy.draft` | `DraftCard` | P1 | V |
| `policy.draft-stale` | "the active version changed since this draft began" warning | P0 | V |
| `policy.actions` | the header `actions` (Edit or Start draft, Simulator) | P1 | V |
| `policy.versions` | the Versions tab table | P1 | V |
| `policy.decision-filters` | the Decisions tab chips | P1 | V |
| `policy.decision-rows` | `DecisionList`: what was recorded for each step | P0 | V |
| `policy.fingerprint` | rules fingerprint of a version, "Fingerprint" | P3, `count={1}` | V in an S |
| `version.status` | "Active" or "Replaced by version M" | P0 | V |
| `version.summary` | summary `KeyValueList` | P1 | V |
| `version.rules` | the rules table of a past or active version on `VersionPage` | P1 | V |
| `version.acknowledgements` | what was acknowledged, by whom, when | P0 | V |
| `draft.banner` | unsaved changes, "changed elsewhere", the 400 issues of a failed save | P0 | V |
| `draft.rules` | the working rules table (what authority will apply once published, locked and shadow marks) | P0 | V |
| `draft.actions` | header and `SubmitBar` buttons (Add rule, Check rules, Save draft, Review and publish, Discard) | P1 | V |
| `draft.validation` | the validation problems from "Check rules" | P0 | V |
| `draft.checks` | the rest of the check result: conflicts, locked changes, simulation | P1 | V |
| `draft.editor` | the rule editor form | P1 | V |
| `draft.metadata` | revision, updated by and when, fingerprint | P3, `count={3}` | V in an S |
| `publish.validation` | "Fix these rules before publishing" | P0 | V |
| `publish.findings` | blocking findings with their acknowledgement boxes | P0 | V |
| `publish.notes` | informational findings | P1 | V |
| `publish.locks` | locked-rule changes | P0 | V |
| `publish.widening` | "N recorded decisions would become Allow automatically" | P0 | V |
| `publish.simulation` | the simulation tiles | P1 | V |
| `publish.controls` | the publish summary and button | P0 | V |
| `publish.metadata` | fingerprints and the base version | P3, `count={2}` | V in an S |
| `decision.state` | not recorded, skipped or could not evaluate, evaluation error | P0 | V |
| `decision.step` | which step, host, capability, target | P1 | V |
| `decision.outcome` | the active and the shadow outcome with "Not enforced" and the degraded flag | P0 | V |
| `decision.locked` | the Locked Deny block and what the rules alone would have said | P0 | V |
| `decision.exception` | the "Allow overrides a broader Deny" warning | P0 | V |
| `decision.why` | why this result: reason, winners, overridden rules | P0 | V |
| `decision.limits` | the hard-limits block and the other limit warnings | P0 | V |
| `decision.gaps` | inputs that were not available | P0 | V |
| `decision.matched` | every matching rule | P1 | V |
| `decision.constraints` | what an Allow carries | P1 | V |
| `decision.facts` | facts consulted, label "Facts consulted" | P2, `count` = number of facts | V |
| `decision.inputs` | the recorded inputs, label "Recorded inputs" | P2 | V |
| `decision.metadata` | evaluated at, policy version, fingerprint, execution and operation ids | P3, `count={4}` | V in an S |
| `simulator.state` | loading, errors, "nothing to simulate yet" | P0 | V |
| `simulator.form` | the form | P1 | V |
| `simulator.basis` | what the run used (recorded or current inputs), the label, the hub's notes, "cannot replay" and truncated messages | P0 | V |
| `simulator.widening` | "N decisions would become Allow automatically" | P0 | V |
| `simulator.summary` | the tiles | P1 | V |
| `simulator.results` | the results table | P1 | V |
| `simulator.detail` | the selected item (`DecisionCompare`) | P1 | V |

`DecisionExplanation` is reused by the simulator, so the `decision.*` ids occur twice on that page: an id names a kind, not an instance (37).

### 6.7 Editor, publication, decision and simulator behaviour **fixed**

**Rule model (`ruleModel.ts`, pure, unit-tested).** The working copy of a draft is an array of `PolicyRule` (the generated `RuleDto`). The editor form uses a string-valued `RuleForm`:

```ts
export type Problems = { id?: string; capValue?: string; targetNames?: string; note?: string; conditions: Record<string, string> };
export interface RuleForm {
  id: string; enabled: boolean; shadow: boolean; locked: boolean; outcome: "deny" | "require_approval" | "allow";
  hostId: string;                                  // "" = every host
  capKind: "any" | "effect" | "category" | "contract"; capValue: string;   // effect name, category text or contract text
  targetsKind: "any" | "exact"; targetNames: string;                         // one name per line
  conditions: ConditionForm[]; note: string;
}
export type ConditionForm = { key: string } & (
  | { kind: "arg_constraint"; arg: string; op: "int_at_least" | "int_at_most"; value: string }
  | { kind: "time_window"; start: string; end: string }                       // "HH:MM"
  | { kind: "requires_fact"; fact: string; expected: "true" | "false" }
  | { kind: "max_rate"; max: string } | { kind: "max_ttl"; seconds: string });
export function fromDto(rule: PolicyRule): RuleForm;
export function toDto(form: RuleForm): { ok: true; rule: PolicyRule } | { ok: false; problems: Problems };
export function validateRule(form: RuleForm, otherIds: readonly string[]): Problems;     // empty `conditions` map and no other key = valid
export function newRule(existingIds: readonly string[]): RuleForm;     // enabled, SHADOW true, outcome require_approval, any host, any capability, any target, no conditions
export function duplicateOf(rule: PolicyRule, existingIds: readonly string[]): PolicyRule;  // id "<id>-copy" (made unique with -2, -3), locked false, shadow true
export function diffCount(saved: readonly PolicyRule[], working: readonly PolicyRule[]): number; // added + removed + structurally changed, by id; a display counter only
```

`toDto` produces exactly the JSON of 40 section 6.10: host `{kind:"all"}` or `{kind:"host",host_id}`; capability `{kind:"any"}`, `{kind:"effect",effect}`, `{kind:"category",category}` or `{kind:"contract",contract}`; targets `{kind:"any"}` or `{kind:"exact",names:[…]}` (trimmed, de-duplicated, blank lines dropped); conditions `{kind:"arg_constraint",arg,op,value:<integer>}`, `{kind:"time_window",start,end}`, `{kind:"requires_fact",fact,expected:<boolean>}`, `{kind:"max_rate",max_per_hour}`, `{kind:"max_ttl",max_ttl_secs}`; `outcome` one of `allow require_approval deny`; `locked` is `true` only when `outcome` is `deny` (it is forced false otherwise). **New rules start as shadow rules** (design pack: new rules start in shadow). Validation is a courtesy: the hub validates again and is the authority.

| Field | Client check | Hint on failure (short sentence in `ruleModel.ts`) |
|---|---|---|
| Rule name (`id`) | `^[A-Za-z0-9][A-Za-z0-9_.:-]{0,63}$`, unique among the other working rules | the allowed characters; "Another rule already has this name." |
| Contract | `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`, at most 80 | an example: `container.restart`, `skill.backup_db` |
| Category | same grammar, at most 64 | an example: `container.lifecycle` |
| Target names | each 1 to 128 characters, no control characters; at least one for "Named targets" | "Name at least one target, or choose Any target." |
| Argument | `^[a-z][a-z0-9_]{0,63}$` | the allowed characters |
| Value, seconds, rate | a whole number (seconds and rate not negative) | "Enter a whole number." |
| Time | `HH:MM`; start and end differ | "Start and end must differ." |
| Fact | `^[a-z][a-z0-9_.]{0,63}$` | the allowed characters |
| Note | at most 500 characters | "At most 500 characters." |

**Editor (`DraftPage`).** `overview.draft === null` shows `StartDraft` (`EmptyState` "No draft yet": a primary "Start from version N" when a version is active, a secondary "Start with no rules"; each runs `useStartDraft` through `useActionFeedback`, success "Draft started."). Otherwise `usePolicyDraft(true)` in a `QueryBoundary context="load the draft"`, then `useDraftState(draft)`. Layout: the working `RulesTable mode="edit"` (columns Rule: name, note, marks; Applies to: `scopeText`; Conditions: count and the first condition as text, the rest in a tooltip; Outcome: `OutcomePill`; actions: Edit, Duplicate, Remove in a row `Menu`) and, beside it from 1024 px and under it below, the `RuleEditor` section while `?rule=` names a rule (a `?rule=` that names no working rule is dropped). A locked Deny row has a danger tone stripe and the **Locked Deny** pill; a shadow rule shows the Shadow pill; a disabled rule the Disabled pill. The table says once: "Order does not matter. The most specific rule wins." Header: `PageHeader title="Policy draft"`, `meta` the `Tag`s "Revision N" and "Based on version M" (or "Based on no version"), and the "Unsaved changes" `Tag` while dirty; `actions`: "Add rule" (secondary, `?rule=new`), "Check rules" (secondary) and **Review and publish** (navigates to `/policy/draft/publish`): the one primary button while the draft is clean, secondary and disabled with the reason "Save the draft first." while it is dirty; a `Menu` "More" holds "Discard draft…". There is exactly one primary action at a time: while the working copy is dirty it is the floating `SubmitBar`'s **Save draft** (`SubmitBar changes={diffCount(saved, working)} saveLabel="Save draft"`, with Discard, which reverts the working copy and asks first through a `ConfirmDialog` when more than three changes would be lost). `useLeaveGuard(dirty, "draft")` guards navigation. If `overview.draft.based_on` differs from `overview.active?.number`, `InlineMessage tone="warning" title="The active version changed since this draft began"` ("Review the draft before you publish. Saving it again re-bases it.") with a link to the simulator. If the loaded draft's revision changes while the working copy is dirty (another tab saved), `InlineMessage tone="warning" title="This draft changed elsewhere"` with "Reload the draft", which asks (`ConfirmDialog`, danger) before discarding the local edits. Rule names in `?rule=` are the rule `id`s.

**Rule editor (`RuleEditor`, section "Rule").** A form in a `Card`, built from `Field`s: Rule name (`TextInput`, hint "Shown in decisions and reports."); Outcome (`SegmentedControl` Deny, Ask for approval, Allow automatically: the labels from `policyOutcomeMeta`); **Locked** (`Switch`, only while the outcome is Deny, label from `ruleMarkMeta("locked_deny").label`, hint from `LOCK_HELP`, and a line "Saving a change to a locked rule asks for your password and shows what it would change."); Enabled (`Switch`); Shadow (`Switch`, label from `ruleMarkMeta("shadow").label`, hint `SHADOW_HELP`); Applies to: Host (`Select`: Every host, then each host from `useHosts()` by name with its id as value; a host id that is not in the list stays as "Unknown host (<first 8>)"; a failed host load is a `LoadError` in place of the select), Capability (`Select`: Any capability, An effect class, A category, One capability; then a `Select` of the four effect classes with `effectClassMeta` labels, or a `TextInput` mono for a category or a contract with the hint "Typed as written; Check rules reports names the hub does not know."), Targets (`SegmentedControl` Any target, Named targets; for named targets a `Textarea` "One target name per line", hint "Names match exactly, including case."); Conditions: a list of condition rows (`Select` kind and the inputs below; a "Remove" `IconButton label="Remove condition"`; an "Add condition" `Button`); Note (`Textarea`, 500 characters counted in the hint). Condition inputs: *Argument*: name (`TextInput` mono), `Select` "is at least" and "is at most", `NumberInput`; *Time window*: two `TextInput type="time"` labelled **`Start ({zone})` and `End ({zone})`** where `{zone}` is `useTimeWindowZone()` (the overview's `time_window_zone`), with the hint `Times are read in {zone}. A window that ends before it starts runs overnight.`; *Fact*: id (`TextInput` mono) and `Select` "is true" and "is false"; *Rate*: `NumberInput` "Most per hour"; *Lifetime*: `NumberInput` "Longest lifetime (seconds)". Buttons: primary **Apply to draft** (validates with `validateRule`, writes the rule into the working copy and closes the editor; it does **not** save), secondary **Cancel**. Below the form a muted "In plain words" line with `scopeText` and the conditions as text. A rule with an unknown condition kind (a newer hub) is shown read-only: the row says "A condition this page does not know" and Apply keeps it untouched.

**Check rules (`DraftChecks`).** The "Check rules" button runs `usePreviewRules` on the working copy through `useActionFeedback(… , { success: null, error: "check the rules", errorDisplay: "inline" })` and shows the answer under the table: `draft.validation` (each `validation` entry as a row with the rule id as a link to `?rule=` and the hub's sentence), and `draft.checks` (a count per finding kind with the hub's notes, the locked-rule changes, `SimulationSummary`). It says: "Checking changes nothing. Saving and publishing are separate steps." A 400 whose `issues` have paths `rules.<id>` marks those rows (a danger `InlineMessage` per rule through `issuesByRule`).

**Save flow (`useSaveDraftFlow`).** Decision D3, exactly:

```text
onSave():
  rules = working copy
  preview = usePreviewRules(base_revision = saved.revision, rules)       // failure: the issues go to draft.banner, the toast says "Could not check the rules", stop
  if preview.locked_changes is empty:
      save({ baseRevision: saved.revision, rules })                       // no password, no preview_hash
  else:
      open LockedChangeDialog { changes: preview.locked_changes, simulation: preview.simulation, previewHash: preview.preview_hash }
LockedChangeDialog.onConfirm(password):
      save({ baseRevision, rules, password, previewHash })                 // the hash of THIS preview
      403 -> the password field says stepUpError(error); 429 -> the field says the retry text
      409 -> an inline message in the dialog in the hub's words and a "Check again" button that closes the dialog and runs onSave again
save success: toast "Draft saved." (or "Draft saved. {n} locked rule change(s) were recorded." when there were locked changes); the working copy becomes the response's draft; the dialog closes
```

`LockedChangeDialog` is the kit `PasswordPrompt` (6.8) with `title="Change a locked rule"`, `reason="Enter your password to save these changes."`, `confirmLabel="Save draft"` and `children`: the `LOCK_HELP` text, one row per change (`StatusPill meta={lockChangeMeta(change)}` and the rule name) and `SimulationSummary` (what the change would do to recorded decisions, with the `publish.widening`-style warning when `newly_allowed > 0`). The password lives only in `PasswordPrompt` and is cleared when the dialog closes; the flow code never stores it.

**Publication (`PublishPage`).** No draft: `EmptyState` "No draft to publish" with a link to `/policy/draft`. Otherwise `usePolicyDraft(true)` then `useDraftPreview(draft)`; a failed preview is `LoadError "check the draft"`. Header `PageHeader title="Review and publish"`, `meta` `Tag`s "Draft revision N" and "Based on version M", intro "Publishing makes this draft the next version. Read what changes and acknowledge each conflict first." Blocks, in order: `PolicyNotes`; **validation** (`preview.validation` non-empty: `InlineMessage tone="danger" title="Fix these rules before publishing"`, each entry linking to the editor row); **What this publishes** (a line "Version {base_number + 1}: {n} rules, {k} locked, {s} shadow", counted from `draft.rules`); **Findings**: every finding with `requires_acknowledgement` is a `Card` (`StatusPill findingKindMeta(kind)`; "{first} and {second}" or the single rule when both are equal; "Resolves to" and an `OutcomePill`; `Tag` Shadow when `shadow`; the hub's `note`; a `Checkbox` with the visible label "Acknowledge: {first} and {second} resolve to {resolution label}" and the hint "You must acknowledge every conflict to publish."), findings without it are a compact list of informational notes; with no finding at all `InlineMessage tone="success"` "No conflicts found."; **Locked rules** (when `locked_changes` is not empty: a danger `Card` with the `LOCK_HELP` text and a row per change); **Simulation** (`SimulationSummary` with "Based on the {evaluated} most recent recorded decisions" and a link "See each decision" to `/policy/simulator?rules=draft`; when `newly_allowed > 0` the warning `InlineMessage` "{n} recorded decisions would become Allow automatically"); **Publish** (`PublishControls`): a primary `Button` "Publish version {base_number + 1}" whose `disabledReason` is, in this order, "Waiting for the review." (preview pending), "Fix the problems above first." (validation), "Acknowledge every conflict above first." (a blocking key unticked); click opens `PasswordPrompt` (`title="Publish version N"`, `reason="Enter your password to publish this version."`, `confirmLabel="Publish version N"`, `children`: "{n} rules, {k} locked, {c} conflicts acknowledged, {l} locked-rule changes."). The request is `{ revision: draft.revision, previewHash: preview.preview_hash, acknowledged: preview.blocking (sorted), password }`. Ticks are a `Set` of finding keys, pruned to the current blocking keys whenever the preview changes. Success: toast "Published policy version N.", navigate to `/policy`. 403 is the password field's error (`stepUpError`), 429 its retry text, 409 (stale preview, stale active version, missing acknowledgement) an inline message in the dialog in the hub's words plus an immediate refetch of the draft and the review (`invalidateQueries(keys.policy)`) and "Review it again" closes the dialog; 400 `invalid` lists the issues in the dialog. Nothing is sent before the password is confirmed.

**Decision page (`DecisionPage`).** `useShadowDecision(execution)` and `usePolicyOverview()` (for the zone). A 404 is `EmptyState "No decision recorded for this step"`; its body says "It is recorded within a few seconds of the step being created. Try again shortly." while recording is on, and "Decisions are not being recorded. Set shadow to true under [policy] in hub.toml and restart the hub." while off. Header `PageHeader title="Decision for {contract}"`; `meta`: the `shadowStatusMeta` pill, the `ruleMarkMeta("not_enforced")` pill, a `Tag` "Degraded" when `degraded`; `actions`: `LinkButton` "Open the operation" (`/operations/{operation_id}?step={execution_id}`) and "Simulate this step" (`/policy/simulator?execution={execution_id}`). A status other than `evaluated` shows `decision.state`: the status pill, the reason through `shadowReasonMeta`, "This step was not evaluated, so there is no decision to explain." and the step identity. An evaluated row shows `decision.step` (host name, contract, target, check, the arguments as key = value), `decision.outcome` (two outcome pills "Active policy" and "Shadow rules", each with `Not enforced`; when they differ `InlineMessage tone="warning"` "The shadow rules would decide differently."), `decision.gaps` (`GapsBlock`: with a non-empty `gaps`, `InlineMessage tone="warning" title="Decided with missing inputs"` and "The result uses the safe reading of each missing input. It is not the whole picture." then one row per gap: `gapInputMeta` label, `gapReasonMeta` pill), then `Section title="Active policy"` and `Section title="Shadow rules"`, each a `DecisionExplanation` (for the shadow section, a decision with no matching rule is one line, "No shadow rule matched."), then `decision.inputs`, `decision.metadata`.

**`DecisionExplanation({ title, decision, zone })`** renders one `DecisionView` (6.2) with these blocks in this order, each with the id of 6.6: *outcome* (the `OutcomePill`, the reason pill from `decisionReasonMeta`, the text "Not offered for approval. To allow it, change the policy." for a Deny, "Cannot decide. Nothing runs and nothing is offered." for Cannot decide); *locked* (when `locked` is not empty: a danger `Card` with the **Locked Deny** pill, "Locked by {rule names}", and "Without the lock, the rules would have decided {OutcomePill of wouldHaveBeen}"); *exception* (one warning `InlineMessage` titled with `ruleMarkMeta("dangerous_exception").label` per `dangerous_exception` warning: "Rule {allow} allows this and overrides the broader Deny {deny}."); *why* (the reason's hint, the winners as `Tag`s, and one line per overridden pair "{by} overrides the broader rule {broader}"); *limits* (a `Card` "Hard limits" with an `InfoTip` "Hard limits apply after the rules. They are not rules and cannot be edited here.": a row per applied limit with its `hardLimitMeta` pill and hint; the other warnings (`safety_set_unverified`, `management_paths_unprotected` with the paths as "Not protected: …"); when `wouldHaveBeen` is set "Rules alone: {pill}. After the limits: {pill}."; "Approving this would need a typed confirmation." when `typedConfirmation`); *matched* (a list of every matching rule with `Tag`s Winner, Overridden, Locked); *constraints* (for an Allow: "At most N per hour", "Lasts at most …", "Only for targets: …", each condition through `conditionText(c, zone)`; "An automatic Allow carries these limits."); *facts* (a row per fact with `factStatusMeta`; the line "Only whether each fact was known is recorded, never its value."). An `error` view is one `InlineMessage tone="warning"` titled with the `decisionErrorMeta(code)` label and its hint as the body.

**Simulator (`SimulatorPage`).** Defaults from the URL: `rules` (default `draft` when `overview.draft`, else `version-<active>`, else none: `EmptyState "Nothing to simulate yet"` with a link to the editor), `selection` (default `both`), `inputs` (default `recorded`), `execution` (one id, shown as a removable `Tag` "Only step <first 8>"). Form (`SimulatorForm`, all `Field`s): Rules (`Select`: "Draft, revision N" when a draft exists, "Version N" for each published version with "(active)" on the active one; a note "Older versions are not listed." when the versions page has `next_cursor`); Which rules (`SegmentedControl` "Active policy", "Shadow rules", "Both"); **Inputs** (`SegmentedControl` "Recorded", "Current") with the hint `SIMULATION_RECORDED` or `SIMULATION_CURRENT` for the chosen one; Host (`Select`, default the selected host), Capability contains (`TextInput`), Status (`Select`, "Any status" then the labels of `shadowStatusMeta`, default Recorded), From and To (`TextInput type="date"`, sent as `YYYY-MM-DDT00:00:00Z`, labelled "(UTC)"), Most results (`NumberInput`, 1 to 500, default 200). The primary `Button` "Run simulation" (`loading` while pending). The result (`SimulatorResults`): `simulator.basis` (a `Card`: the hub's `label`, the chosen inputs mode as words, the hub's `notes` verbatim, "{n} could not be replayed" with the hint "A record that was skipped or could not be evaluated cannot be replayed.", and when `truncated` "Showing the newest {n} decisions. Narrow the filter to see others."); `simulator.widening` (when `summary.newly_allowed > 0`, a warning `InlineMessage` "{n} decisions would become Allow automatically"); `simulator.summary` (`SimulationSummary`); `simulator.results` (`DataTable`: Step link to the decision page and the contract, Host, then for each selected side a cell with the baseline `OutcomePill`, an arrow, the simulated `OutcomePill` and the `comparisonMeta` pill, and a `Tag` "Degraded"; rows select with `?item=`); `simulator.detail` (for the selected item, `DecisionCompare`: per selected side a "Recorded" and a "Simulated" `DecisionExplanation` side by side from 900 px, stacked below). An empty result is `EmptyState "No recorded decisions match"`; with recording off it says why. A `current` run says in `simulator.basis` that the result is a what-if and not a reproduction, **always**; a `recorded` run says "Recorded inputs. Where an input was not available when the decision was made, the result shows the gap." The page never calls a current-input result a reproduction.

### 6.8 Kit and message changes **fixed**

```tsx
// src/components/PasswordPrompt.tsx  (existing callers unchanged)
PasswordPrompt(props: { busy?: boolean; confirmLabel?: string; error?: string | undefined; onCancel: () => void;
                        onConfirm: (password: string) => void; reason: string;
                        title?: string;                // default "Confirm with your password"
                        children?: ReactNode })        // rendered between the consequence text and the password field
```

```ts
// src/design/messages.ts
/** The words for a refused step-up: "That password is not right." for a 403 (the page only asks for a password when it already sends one),
 *  the retry sentence for a 429, undefined for anything else (the caller shows describeError). */
export const WRONG_PASSWORD_TEXT = "That password is not right.";
export function stepUpError(error: unknown): string | undefined;
```

`useLeaveGuard(dirty: boolean, what = "note")`: the dialog consequence reads `Your changes to this ${what} are lost if you leave this page.`; the notes page keeps its text. Tests: `password_prompt_shows_extra_content_and_a_custom_title_and_keeps_its_old_behaviour` (new `src/components/passwordPrompt.test.tsx`), `step_up_error_words_a_wrong_password_and_a_lockout_and_nothing_else` (`messages.test.ts`), and the existing note-guard test passes unchanged.

### 6.9 Fixed copy (`policyText.ts`) **fixed** (tests assert these)

```ts
export const LOCK_HELP = "A locked rule is an absolute prohibition. No narrower rule can override it. Changing, unlocking or removing it needs your password.";
export const SHADOW_HELP = "A shadow rule is recorded for comparison and never acted on.";
export const ORDER_NOTE = "Order does not matter. The most specific rule wins.";
export const SIMULATION_RECORDED = "Each decision is re-evaluated with the host state, facts and ceiling recorded when it was made. Where an input was not available then, the result shows the gap.";
export const SIMULATION_CURRENT = "Each step is re-evaluated with today's clock and host state. This is a what-if, not a reproduction of what happened.";
export const PRECEDENCE_HELP = "The most specific matching rule wins, even over a broader Deny. At equal scope, Deny wins. When two rules overlap and neither is more specific, the safer outcome wins: Deny, then Ask for approval, then Allow. A locked rule overrides all of them. Hard limits then apply on top; they are not rules.";
export const zoneHint = (zone: string) => `Times are read in ${zone}. A window that ends before it starts runs overnight.`;
```

`PRECEDENCE_HELP` is the `InfoTip` of the page intro on `/policy` and `/policy/draft` (`InfoTip title="How rules combine" learnMore="concepts/policy.html"`).

## 7. Ordered implementation steps

Work in `web/`. Commit in small green steps (one-line Conventional Commits, exactly one scope `web`, for example `feat(web): policy status words and the Locked Deny treatment`), after steps 2, 3, 4, 6, 7, 8, 9, 10, 11 and 12. Run `npm run lint`, `npm run typecheck` and the vitest files of the area after every commit. Never write a prompt number, the build plan or an RW, D or O id in code, comments, the book, commit messages or the PR description.

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Confirm 30, 37, 39 and 40 are merged: `ls src/features/operations src/app/rightSlot src/design/visibility.ts src/components/Visible.tsx src/test/visibility.tsx scripts/smoke`; in `src/api/schema.d.ts` the eleven policy paths and the schemas of 6.1; `ls ../crates/domovoi-hub/src/api/ | grep policy`; `ls ../docs/book/src/concepts/policy.md`. **If any is missing, stop and report.** Write down every generated schema name that differs from 6.1.
   - Read 40's Rust DTOs read-only (`crates/domovoi-hub/src/api/dto_policy*.rs`) and the generated types: how `DecisionDto` expresses an evaluation error, how `constraints.conditions` are written (6.2), the real field names of `PolicyOverviewDto`, `ShadowDetailDto` and `SimulationDto`. Decide the one `decisionOf` adapter from what you read.
   - `GET /api/v1/policy` against the dev hub (or 40's tests) to see the real `time_window_zone` value; confirm it is `"UTC"` (D-30).
   - Read completely: `DESIGN.md`, `components/index.ts`, `PasswordPrompt.tsx`, `ConfirmDialog.tsx`, `DataTable.tsx`, `Tabs.tsx`, `SegmentedControl.tsx`, `inputs.tsx`, `SubmitBar.tsx`, `StatTile.tsx`, `features/settings/EditorForm.tsx` (lines 107 to 128), `features/notes/useLeaveGuard.tsx`, `features/rules/RulesPage.tsx`, 37's `visibilityContext.tsx`, `Visible.tsx`, `DetailsScope.tsx`, `src/test/visibility.tsx`, `visibilityGate.test.ts`, 30's `design/operationVocab.ts` and `scripts/smoke/*.mjs`.
   - Baseline: `npm test`, `npm run lint`, `npm run size -- --report` (record the numbers), `npm run gen:api` then `git diff --exit-code -- src/api` (must be clean). `grep -rn "effectClass\|EffectClass" src/design` (if a vocabulary for effect classes already exists, use it and do not add `effectClassMeta`; say so). `grep -n "lock\|shield" src/design/icons.tsx` (expect none). Count the lines `tone.ts` has under the lint rule (comments and blank lines excluded).
2. **Words.** `icons.tsx` (`lock`, `shield`), the three vocabulary files, `policyMeta.ts`, the `tone.ts` re-export and `VOCABULARIES` spread, `visibility.ts` page keys, `messages.ts` `stepUpError`; tests of 12.2. Commit.
3. **Kit and guard.** `PasswordPrompt` `children` and `title`, `useLeaveGuard(dirty, what)`; tests. Commit.
4. **Data layer.** `types.ts` aliases, `keys`, `hooks/policy.ts`, `hooks/policyCommands.ts`, the fixtures and `policyFixtures.ts`; tests of 12.1 and 12.3. Commit.
5. **Pure logic.** `ruleModel.ts`, `ruleText.ts`, `ruleIssues.ts`, `decisionModel.ts`, `simulatorState.ts`, `policyZone.ts`, `policyText.ts`; tests of 12.2.
6. **Overview, versions, decisions list.** `RuleMarks`, `RulesTable`, `PolicyNotes`, `ShadowCard`, `ActiveSummary`, `ActiveRules`, `DraftCard`, `VersionsTab`, `DecisionList`, `DecisionsTab`, `PolicyPage`, `VersionPage`; `pages.ts`, `routes.tsx`, `nav.ts`, `PageTitle.tsx`; update the navigation lists in `routes.test.tsx`, `layout.test.tsx`, `palette.test.tsx`; tests of 12.4. Commit.
7. **Decision explanation.** `decision/*`, `DecisionPage`; tests. Commit.
8. **Editor.** `useDraftState`, `StartDraft`, `RuleEditor`, `RuleScopeFields`, `ConditionFields`, `DraftChecks`, `useSaveDraftFlow`, `LockedChangeDialog`, `DiscardDraft`, `DraftPage`; tests. Commit.
9. **Publication.** `SimulationSummary`, `Findings`, `LockedChanges`, `PublishControls`, `PublishPage`; tests. Commit.
10. **Simulator.** `SimulatorForm`, `SimulatorResults`, `DecisionCompare`, `SimulatorPage`; tests. Commit.
11. **Visibility.** Every `Visible` is written with its component; add the matrix, the P0 test and the gates of 12.6 and 12.7. Commit.
12. **Catalogue, smoke, screens.** `PolicyDemos.tsx` in `DesignPage` (look at `/dev/design` in both themes), checks 12 to 15 of 12.5 in 30's smoke harness, `screens.mjs`. `npm run build`, `npm run smoke`. Commit.
13. **Docs** (section 15), then `cargo xtask docs-links`.
14. **Checks and completion report.**
    - `npm run format` then `npm run format:check`, `npm run lint`, `npm run typecheck`, `npm test`, `npm run build` (runs `size`; record the numbers), `npm run smoke`, `npm run gen:api` then `git diff --exit-code -- src/api` (must be clean).
    - By hand in the browser at 375, 768, 1280 and 1920 px, both themes, keyboard only, in Detailed and Essential: the overview, the editor with a locked rule, the publication page with a blocking conflict, a locked decision, the simulator. Use `node scripts/screens.mjs` against the dev hub if you can start it (WSL, see `TESTING.md`); otherwise say it was not run.
    - Write the completion report in the standard structure, with the extra items listed at its end.

## 8. Database and persistence

Not applicable: this prompt touches no database, no migration and no hub code. The browser stores nothing about policy. The password lives only in the `PasswordPrompt`'s component state and is cleared when the dialog closes; the working copy of a draft lives in component state only (the leave guard protects it; the hub's saved draft is the durable copy). The only storage the pages touch is what 37 and the host selector already use.

## 9. Protocol and compatibility

No wire change, no hub route, no OpenAPI change (RW-42: wire 1.2 is frozen). The pages consume the routes of 40 exactly; `npm run gen:api` leaves no diff. Compatibility inside the pair web and hub:

- A **newer hub** may send an outcome, hard limit, finding kind, shadow reason, gap input or reason, decision error code, warning kind or condition kind this build does not know: each renders as a neutral pill or plain text with the raw value (`lookup()` and `ConditionView` `unknown`) and never throws; an unknown condition in a rule is kept untouched on save.
- An **older hub** answers 404 on `/api/v1/policy`: the pages show `LoadError`. Web and hub ship together; this only affects a dev server pointed at an old hub.
- `policy.published` maps to no stream topic (40): the pages do not update live; they refetch on mount and on window focus (the query defaults), and after their own mutations.
- CSRF and the session are the client's (`src/api/client.ts`); the pages read no token.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `GET /policy` fails (network, 5xx, 404 on an old hub) | `LoadError` "load the policy" with Retry in place of the page body; never an empty state |
| The active version's rules fail to load while the overview loaded | `LoadError` "load the rules" inside the Rules tab only; the summary stays |
| `time_window_zone` is missing or empty | the time inputs are disabled with `disabledReason` "The hub did not say which time zone it uses."; sentences with a window say "in the hub's time zone"; nothing is assumed to be UTC |
| No active version | the "No policy is published yet" copy in `ActiveSummary`; the Rules tab says so; Start a draft is offered |
| Decisions list fails | `LoadError` "load the decisions"; empty with recording off says why, empty with recording on says "No decisions yet." |
| `GET /shadow/{id}` is 404 | `EmptyState` "No decision recorded for this step" with the reason by recording state (6.7); any other failure `LoadError` "load the decision" |
| A row is `skipped` or `unevaluable` | `decision.state` with the reason words; no explanation blocks |
| An evaluation error is stored | one warning `InlineMessage` with the code's words; the other side still renders |
| Save: the preview answers 400 with `issues` at `rules.<id>` | nothing is saved; `draft.banner` lists them, the rows are marked; the toast says "Could not check the rules" |
| Save: no locked change in the preview | `PUT` with no password and no hash; success toast "Draft saved." |
| Save: `PUT` answers 409 (draft changed elsewhere) | the "This draft changed elsewhere" warning with "Reload the draft"; the local edits stay until the person confirms the reload |
| Save with a locked change: wrong password (403) | the password field says "That password is not right."; nothing was written; the dialog stays |
| Save with a locked change: lockout (429) | the field says the retry sentence from `retryAfter`; the dialog stays |
| Save with a locked change: 409 stale preview | inline message in the dialog in the hub's words; "Check again" closes it and re-runs the preview |
| Publish: a blocking finding is not ticked | the button stays disabled with its reason; no request |
| Publish: 403 / 429 | the password field's error as above; nothing published |
| Publish: 409 (stale draft, stale active version, stale preview, missing acknowledgement) | inline message in the dialog in the hub's words; the draft and the review are refetched; ticks are pruned to the current blocking keys; "Review it again" closes the dialog |
| Publish: 400 | the issues listed inside the dialog; nothing published |
| Publish: success | toast "Published policy version N."; navigate to `/policy`, whose queries are invalidated |
| The review (`preview`) fails on the publication page | `LoadError` "check the draft" with Retry; Publish is unavailable with "Waiting for the review." |
| Simulate: 404 (no draft) or 400 | inline error in the form with `describeError`; the Draft option is disabled with a reason while no draft exists |
| Simulate: `truncated`, or items `not_replayable` | the notes of `simulator.basis`; nothing is dropped or hidden |
| The person leaves the editor with unsaved edits | the leave guard asks first; closing the tab shows the browser's own prompt |
| The host list fails inside the editor's host select | `LoadError` "load hosts" in place of the select; the rule is not changed |
| A step, rule or version id in the URL does not exist | the page's `EmptyState` (no such version, no decision); a `?rule=` that names no rule is dropped |
| The stream drops | the shell's own banner; the pages do not use the stream |
| Any unknown value | neutral pill or raw text, never an exception |

## 11. Security requirements

- **The browser decides nothing about authority.** No evaluation, specificity, conflict detection, locked-change detection or hash is computed in the web (decision D1). `diffCount` is a display counter. The hub re-checks every command; a disabled control only explains.
- **Step-up.** The password is sent only in the body of `PUT /policy/draft` (when a locked rule changed) and `POST /policy/draft/publish`, over the existing client. It is held only by `PasswordPrompt` state, never copied into feature state, a query key, a URL, a toast, a log or `localStorage` (gate 12.7). A wrong password writes nothing and is worded by `stepUpError`; a lockout shows the hub's retry sentence. Publishing always asks; a locked-rule save always asks.
- **Acknowledgement is exact.** The request carries `preview.blocking` exactly and the button needs every blocking finding ticked; the `preview_hash` sent is the one of the preview on screen. A changed preview resets ticks that no longer apply. The page never offers "publish anyway".
- **Deny is never offered for approval (D-23).** No file under `features/policy` imports an approval hook or card (gate 12.7); a Deny decision says the owner must change the policy.
- **Locked Deny is never colour alone.** The `lock` icon and the text come from one vocabulary entry used everywhere (D-26).
- **Honest labelling.** Every recorded decision is labelled "Not enforced". A degraded decision shows its gaps; a current-input simulation says it is not a reproduction; no page states that a result is exact when an input was missing.
- **Fail closed in the UI.** Unknown values render raw and neutral; an unknown condition is kept, not dropped; a missing zone disables the time inputs; a 404 on a decision is "not recorded", not "allowed".
- **Untrusted text.** Rule notes, rule names, target names and the hub's finding notes and validation sentences are shown as React text (no `dangerouslySetInnerHTML`, no Markdown, no links built from them). Owner text never becomes a route, a class or an attribute value other than a link built from a validated id.
- **No secrets in the page.** The DTOs hold ids, names, argument values the skill already validated and ceiling summaries; the pages render them as text and log none. Fixtures contain no real credential.
- **Visibility never gates authority (37).** `PublishControls.tsx`, `LockedChangeDialog.tsx`, `useSaveDraftFlow.tsx` and `StartDraft.tsx` contain no visibility primitive (gate 12.7); Publish, Acknowledge, Save with a locked change and Discard exist, with their prompts, in every profile; the profile never changes which requests are made.
- **No new network path.** No `fetch` outside `src/api`, no external origin, no inline script. **Audit** is the hub's (`policy.published`, `policy.lock_changed`, `policy.draft_saved`, `policy.draft_discarded`); the UI adds none.

## 12. Testing requirements

Name every test after the behaviour. Page and component tests use `renderAt` (or 37's `renderWithProfile`) and `apiMock`; **axe runs automatically** over whatever a UI test leaves on screen and no policy test calls `skipAxe()` except the catalogue test (a fragment catalogue, with a comment saying so). No test sleeps: use `findBy…` and fake timers. Expectations come from the fixture data through **small independent helpers written in the test**, never by calling the function under test, and never copied strings (except the fixed copy and labels of 6.3 and 6.9). Every page test file starts with `vi.mock("../../api/client", () => import("../../test/apiMock"))`.

### 12.1 Fixtures (fixed names) in `src/test/fixtures/policy/`, built from 40's DTO shapes

The JSON is shared with the browser smoke test (`scripts/smoke/data.mjs` reads the same files with `readFileSync`). `src/test/policyFixtures.ts` imports each file by explicit path and exports it typed with the aliases of 6.1 (`as PolicyOverview` and so on; `satisfies` where the JSON types allow). `policyFixtures.test.ts` `fixtures_carry_every_key_of_the_contract_examples` asserts each fixture has every top-level key of 40's JSON examples (overview, preview, simulation, shadow row and detail, version detail, draft) so a fixture cannot silently lose a field. All ids are fixed UUID-shaped strings; timestamps end with `Z`; `rules_hash` is 64 lowercase hex.

| File | Holds |
|---|---|
| `overview-active.json` | `shadow_enabled: true`, zone `"UTC"`, active version 3 (12 rules, 1 locked, 2 shadow), draft revision 4 based on 3, a recent `last_sweep_at`, `last_error: null`, `last_24h` { total 40, by_status evaluated 38 and unevaluable 2, by_enforced_outcome require_approval 30 and allow 8, degraded 40, differing 5 }, the three standing notes of 40 |
| `overview-fresh.json` | `shadow_enabled: false`, no active version, no draft, zero counters, `last_sweep_at: null`, two notes |
| `overview-berlin.json` | as active but zone `"Europe/Berlin"`, draft based on 2 while the active version is 3 (a stale draft), `last_error: "stored rules unreadable"` |
| `version-3.json` | `VersionDetailDto`: a **locked Deny** on the effect `destructive` (`no-destroy`); an Allow on contract `skill.restart_plex` for target `plex` on one host with `max_rate`, `time_window` 22:00 to 06:00 and `requires_fact`; an Ask on category `container.lifecycle`; a **shadow** Allow with `arg_constraint` and `max_ttl`; a **disabled** rule; a Deny on contract `skill.prune_images`; one acknowledgement (an incomparable conflict, resolution deny) |
| `draft-4.json` | `DraftDetailDto` revision 4: version 3's rules plus a new locked Deny `no-prune` on `skill.prune_images` and an edited `plex-restart` |
| `preview-clean.json` | `validation: []`, `report.findings: []`, `blocking: []`, `locked_changes: []`, `simulation` all zero, three notes, a `preview_hash` |
| `preview-blocking.json` | no validation entries; findings: an incomparable conflict, an equal-scope conflict and an `ineffective_under_lock` (all `requires_acknowledgement: true`), a `target_outside_every_ceiling` note and a shadow incomparable pair (both `false`); `blocking` = the three keys; `locked_changes: [{ rule: "no-prune", change: "created" }]`; simulation { evaluated 200, same 190, changed 6, not_replayable 4, newly_allowed 2, newly_denied 3, newly_requires_approval 1 } |
| `preview-invalid.json` | `validation` with one entry naming `plex-restart` and one with `rule: null`, `report: null`, `blocking: []` |
| `simulate-both.json` | `SimulationDto` "draft revision 4", four items: A enforced `changed` (Ask to Allow, the simulated decision carries a `dangerous_exception` warning), shadow `same`; B enforced `same` (Deny); C `not_replayable`; D a simulated decision that is an evaluation error; `summary` with `newly_allowed: 1`, two notes, `truncated: false` |
| `shadow-page.json` | `ShadowPageDto` with six rows: evaluated allow, ask and deny; `unevaluable` (`contract_unknown`); `skipped` (`package_contract`); a degraded and differing row with reason `late`; `next_cursor` set |
| `decision-locked.json` | `ShadowDetailDto`, evaluated: the enforced decision is a **locked Deny** (`reason: locked_deny`, `locked: ["no-destroy"]`, no winners, an overridden pair, `would_have_been` an Allow), the shadow decision an Ask by default; three gaps; inputs; candidate |
| `decision-exception.json` | enforced Allow (`single_winner`) with a `dangerous_exception` warning, an overridden pair, constraints (`max_per_hour` 3, `max_ttl_secs` 600, targets `["plex"]`, a time window, a fact and an argument condition), two facts; the shadow decision matched nothing |
| `decision-limits.json` | enforced Ask with hard limits `target_not_pinned`, `ceiling_does_not_cover`, `agent_runs_as_root`, `would_have_been` an Allow, a typed confirmation; `degraded` with the gaps ceiling (`not_produced_yet`), facts, autonomy |
| `decision-unavailable.json` | enforced `unavailable` (`safety_unknown`), limit `helper_safety_incomplete`, warnings `safety_set_unverified` and `management_paths_unprotected` with paths |
| `decision-unevaluable.json` | `status: unevaluable`, `reason: contract_unknown`, no inputs and no decisions |
| `decision-error.json` | the enforced decision is an evaluation error `ceiling_hash_missing`, the shadow decision is normal |

### 12.2 Words and pure logic (`src/design/policyVocab.test.ts`, `src/features/policy/*.test.ts`, `src/design/messages.test.ts`)

| Test | Expected |
|---|---|
| `every_policy_value_the_contract_names_has_words` | every key of 6.3 (5 outcomes, 5 marks, 5 reasons, 10 limits, 4 finding kinds, 4 lock changes, 3 shadow statuses, 3 comparisons, 4 fact statuses, 4 effect classes, 8 gap inputs, 3 gap reasons, 6 shadow reasons, 12 error codes) is in its table; `tone.test.ts` then checks label, tone, hint and icon |
| `labels_tones_and_icons_match_the_fixed_tables` | the outcome, mark, limit, finding, lock, shadow-status and comparison rows of 6.3 |
| `every_policy_value_in_the_fixtures_has_words` | reads every fixture: each outcome, reason, limit, finding kind, status, gap value, warning kind, comparison and lock change is a key of its table |
| `unknown_policy_values_are_neutral_with_the_raw_value` | each lookup with `"from_the_future"` |
| `locked_deny_reads_differently_from_deny` | `ruleMarkMeta("locked_deny")` has icon `lock`, tone `danger` and the label "Locked Deny"; `policyOutcomeMeta("deny")` has neither |
| `scope_text_names_host_capability_and_targets` | table over every host, capability and target form of `RuleDto`, host names from a map, an unknown host id as "Unknown host (<first 8>)" |
| `condition_text_names_the_zone_it_was_given` | a time window with zone `"UTC"` and with `"Europe/Berlin"` reads "between 22:00 and 06:00 UTC" and "… Europe/Berlin"; no other text mentions a zone |
| `condition_and_duration_texts_are_plain` | argument, fact, rate and lifetime sentences; 45, 600 and 5400 seconds read "45 s", "10 min", "1 h 30 min" |
| `actor_text_says_a_person_or_domovoi` | `user:…`, `system:policy`, anything else raw |
| `rule_to_dto_produces_the_documented_json` | `toDto(fromDto(rule))` equals the rule for every rule of `version-3.json`; and the exact JSON of 40's example |
| `rule_validation_follows_the_table` | one case per row of the table in 6.7, valid and invalid; the Locked flag is forced false unless the outcome is Deny |
| `new_rules_start_as_shadow_rules_and_duplicates_never_lock` | `newRule` has `shadow: true`; `duplicateOf` a locked rule has `locked: false`, `shadow: true` and a unique id |
| `target_names_are_trimmed_deduplicated_and_never_empty` | blank lines dropped; none left is a problem |
| `diff_count_counts_added_removed_and_changed_rules` | `version-3` to `draft-4` is 2 |
| `an_unknown_condition_is_kept_untouched_through_a_round_trip` | `fromDto` then `toDto` keeps it |
| `decision_of_reads_every_decision_fixture` | allow, locked, limits, unavailable, error, null; the outcome key, reason, limits, warnings and constraints are what the fixture says |
| `decision_of_reads_both_condition_shapes_and_keeps_unknown_ones` | `{hour, minute}` and `"HH:MM"` time windows, ISO and friendly durations, an unknown condition becomes `unknown` with its raw text |
| `parse_duration_understands_iso_and_friendly_text_and_nothing_else` | `PT10M`, `PT1H30M`, `10m`, `1h 30m`; `"soon"` is null |
| `simulator_state_round_trips_the_url_and_falls_back_on_bad_values` | parse and serialise `rules`, `selection`, `inputs`, `execution`, `item`; a bad `selection` is `both` |
| `issues_by_rule_maps_rules_paths_to_rows_and_keeps_the_rest` | `rules.plex-restart` to that row; other paths to the banner |
| `step_up_error_words_a_wrong_password_and_a_lockout_and_nothing_else` | 403 gives `WRONG_PASSWORD_TEXT`; 429 with `retryAfter` gives the retry sentence; 409, 400 and 500 give `undefined` (`messages.test.ts`) |

### 12.3 Hooks (`src/api/hooks/policy.test.ts`)

| Test | Expected |
|---|---|
| `overview_version_and_draft_hooks_call_their_routes` | the three `GET` paths and the `{number}` parameter; the draft hook is not called while disabled |
| `versions_and_decisions_page_with_the_cursor_and_drop_empty_filters` | `limit`, then `cursor` for page 2; `differing=true` sent, empty strings dropped |
| `start_draft_copies_a_versions_rules_or_starts_empty` | `from: 3` reads the version then `PUT` `{ base_revision: null, rules }`; `from: null` sends `rules: []` |
| `save_draft_omits_password_and_hash_unless_given` | body keys for both cases |
| `discard_sends_the_revision_in_the_query` | `DELETE` with `revision` |
| `preview_publish_and_simulate_send_the_documented_bodies` | `{ base_revision, rules }`; `{ revision, preview_hash, acknowledged, password }`; `{ rules: { source: "version", number }, selection, inputs, filter, execution_ids, limit }` |
| `commands_that_change_policy_invalidate_every_policy_query_and_reads_do_not` | spy on `invalidateQueries` for start, save, discard and publish; none for preview and simulate |
| `the_draft_preview_query_is_keyed_by_revision_and_fingerprint` | a second render with the same draft does not post again; a new `rules_hash` does |

### 12.4 Pages and components

| Test (file) | Expected |
|---|---|
| `overview_shows_notes_shadow_status_active_version_and_rules` (`policyOverview.test.tsx`, `overview-active` + `version-3`) | the three notes, the tiles with the fixture's numbers, version 3 summary, the rules table with the **Locked Deny** pill on `no-destroy`, a Shadow pill, a Disabled pill; one `<h1>` "Policy" |
| `overview_without_a_policy_says_defaults_and_offers_a_draft` (`overview-fresh`) | the "No policy is published yet" copy, "Decisions are not being recorded", "Start a draft" |
| `overview_with_recording_off_and_no_rows_explains_why_on_the_decisions_tab` | the empty state names the setting |
| `a_last_recording_error_is_shown_as_a_danger_message_with_the_hubs_text` (`overview-berlin`) | |
| `a_stale_draft_warns_that_the_active_version_changed` (`overview-berlin`) | the warning with a link to the simulator |
| `rules_load_failure_is_a_load_error_inside_the_tab_only` | summary still shown, Retry refetches |
| `an_overview_failure_is_a_load_error_not_an_empty_state` | |
| `versions_tab_lists_versions_with_the_active_tag_and_loads_more` | |
| `decisions_tab_lists_recorded_decisions_with_pills_and_flags_and_links_each_row` (`shadow-page`) | outcome pills, status pill for unevaluable and skipped, "Degraded" tag, link `/policy/decisions/<id>` |
| `decisions_filters_are_sent_to_the_hub_and_follow_the_host_selector` | `differing=true`, `status=unevaluable`, `host=<id>` for the selected host name; no request while the host list loads |
| `version_page_shows_rules_acknowledgements_and_the_active_state` (`version-3`) | "Active", acknowledgements with who and when, "Start a draft from this version" disabled with its reason while a draft exists |
| `no_such_version_says_so_with_a_link_back` | 404 and a non-numeric number |
| `the_time_window_zone_is_whatever_the_hub_reports` (`overview-berlin`) | time inputs and sentences read "Europe/Berlin" and the text "UTC" appears nowhere; with `overview-active` they read "UTC" |
| `a_missing_zone_disables_the_time_inputs_with_a_reason` | |
| `start_draft_copies_the_active_version_and_opens_the_editor` | `PUT` body, success toast, navigation to `/policy/draft` |
| `editor_lists_the_draft_rules_with_locked_shadow_and_disabled_marks` (`draft-4`) | |
| `editing_a_rule_changes_the_working_copy_and_shows_unsaved_changes_without_saving` | Apply writes to the table, "1 unsaved change", no request |
| `adding_a_rule_starts_it_as_a_shadow_rule` | the Shadow switch is on |
| `locked_switch_only_exists_for_a_deny_and_is_cleared_when_the_outcome_changes` | |
| `invalid_rule_fields_show_field_errors_and_do_not_apply` | |
| `removing_and_duplicating_rules_work_and_a_locked_removal_warns_about_the_password` | |
| `check_rules_shows_validation_conflicts_locks_and_the_simulation_and_changes_nothing` (`preview-invalid`, `preview-blocking`) | |
| `saving_without_locked_changes_previews_then_saves_with_no_password` | `POST preview` then `PUT` without `password` or `preview_hash`; toast "Draft saved." |
| `saving_a_locked_change_shows_the_changes_the_simulation_and_asks_for_the_password` (`preview-blocking`) | dialog lists "no-prune" with its lock-change pill, the simulation tiles, the widening warning; no `PUT` before the password is confirmed; then `PUT` with `password` and the preview's `preview_hash` |
| `a_wrong_password_is_a_field_error_and_the_dialog_stays` | 403 gives "That password is not right." |
| `a_lockout_shows_the_retry_sentence` | 429 with `Retry-After` |
| `a_stale_preview_on_save_is_explained_and_can_be_checked_again` | 409 |
| `a_draft_changed_elsewhere_blocks_the_save_and_offers_a_reload_that_asks_first` | |
| `leaving_the_editor_with_unsaved_edits_asks_first_and_says_draft` | the guard text names "draft" |
| `discarding_the_draft_asks_first_and_sends_the_revision` | `DELETE`, toast, navigation |
| `save_is_the_primary_action_while_dirty_and_review_and_publish_otherwise` | exactly one primary; Review and publish disabled with "Save the draft first." while dirty |
| `publish_page_lists_blocking_findings_with_boxes_and_informational_notes_apart` (`preview-blocking`) | three boxes, one note, the shadow pair not blocking |
| `publish_is_disabled_with_a_reason_until_every_conflict_is_acknowledged` | the three reasons in order; no request |
| `publishing_asks_for_the_password_and_sends_exactly_the_blocking_keys_and_the_shown_hash` | `POST` body `{ revision, preview_hash, acknowledged: preview.blocking, password }`; nothing sent before confirm; success toast "Published policy version 4." and navigation to `/policy` |
| `a_clean_review_publishes_with_an_empty_acknowledgement_list` (`preview-clean`) | |
| `publish_failures_read_in_the_hubs_words_inside_the_dialog_and_refetch_the_review` | 403, 429, 409 and 400 each |
| `a_changed_review_prunes_ticks_that_no_longer_apply` | |
| `the_locked_changes_block_and_the_widening_warning_are_always_visible` | |
| `the_publish_page_for_invalid_rules_lists_them_and_cannot_publish` (`preview-invalid`) | |
| `no_draft_to_publish_is_an_empty_state_with_a_link` | |
| `decision_page_explains_a_locked_deny_with_what_the_rules_alone_would_have_said` (`decision-locked`) | the **Locked Deny** pill, "Locked by no-destroy", the would-have-been pill, no approval control |
| `decision_page_marks_the_dangerous_exception_and_the_constraints` (`decision-exception`) | the warning titled "Allow overrides a broader Deny", the overridden line, the constraint sentences with the zone |
| `decision_page_shows_hard_limits_apart_from_rules_and_the_missing_inputs` (`decision-limits`) | the "Hard limits" card with three limits, "Rules alone … After the limits …", the "Decided with missing inputs" message with three gaps, the typed-confirmation line |
| `decision_page_says_cannot_decide_for_an_unavailable_outcome` (`decision-unavailable`) | the outcome words, both warnings, "Not protected: …" paths |
| `a_deny_decision_is_never_offered_for_approval` | text "To allow it, change the policy"; no button named Approve or Deny |
| `an_unevaluable_step_says_why_and_shows_no_explanation` (`decision-unevaluable`) | |
| `an_evaluation_error_is_one_warning_and_the_other_side_still_renders` (`decision-error`) | |
| `an_unrecorded_step_is_an_empty_state_worded_by_the_recording_state` | 404 with recording on and off |
| `every_recorded_decision_is_labelled_not_enforced` | |
| `simulator_defaults_to_the_draft_both_selections_and_recorded_inputs_and_runs_on_click` | nothing posted before Run; the body matches the form |
| `simulator_labels_current_inputs_as_a_what_if_and_recorded_inputs_with_their_gaps` | `simulator.basis` text for each |
| `simulator_shows_baseline_simulated_and_comparison_per_item_and_the_summary` (`simulate-both`) | pills, "Changed", "Cannot replay", the error item, the tiles |
| `simulator_warns_when_decisions_would_become_allow_automatically` | `newly_allowed: 1` |
| `simulator_shows_notes_truncation_and_not_replayable_counts` | |
| `selecting_a_result_shows_recorded_and_simulated_explanations_side_by_side` | `?item=` |
| `simulator_can_be_started_for_one_step_from_the_decision_page` | `?execution=` becomes `execution_ids` |
| `simulator_without_a_draft_or_version_says_nothing_to_simulate_and_disables_the_draft_option` | |
| `simulator_failures_are_inline_in_the_hubs_words` | 404 and 400 |
| `password_prompt_shows_extra_content_and_a_custom_title_and_keeps_its_old_behaviour` (`components/passwordPrompt.test.tsx`) | |
| `policy_demos_render_every_state` (`dev/policyDemos.test.tsx`, `skipAxe()` with a reason) | |
| `routes_and_navigation_list_policy` (`routes.test.tsx`, `layout.test.tsx`, `palette.test.tsx`) | "Policy" in the System group before Settings; the six routes render their pages |

### 12.5 Browser smoke test (extends 30's and 37's `npm run smoke`)

`scripts/smoke/server.mjs` answers `GET /api/v1/policy`, `/policy/versions`, `/policy/versions/{number}`, `/policy/draft`, `/policy/shadow`, `/policy/shadow/{execution_id}` and `POST /policy/preview`, `/policy/draft/publish`, `/policy/simulate` from the fixtures through `data.mjs`; the "any other `/api` request fails the run" rule stays. New checks, after the existing ones (each prints one line; the first failure exits 1 with the page text and a screenshot in `smoke-out/`):

12. **Policy overview**: `/policy` shows the notes, the active version, a rule row with the text "Locked Deny" and its lock icon, the Decisions tab lists the rows; `documentElement.scrollWidth <= clientWidth` at 375 and 1280 px; axe in the real browser reports nothing, **colour contrast included**, in both colour schemes.
13. **Decision**: `/policy/decisions/<locked id>` shows "Locked Deny", the hard-limits card for the limits fixture, "Not enforced"; no button named Approve; axe clean.
14. **Publication**: `/policy/draft/publish` with the blocking fixture: Publish is disabled until the three boxes are ticked; click Publish: a dialog opens and **nothing was posted**; type the password and confirm: the captured `POST .../publish` body has `acknowledged` equal to `blocking`, the fixture's `preview_hash` and a `password`; the page leaves for `/policy`.
15. **Simulator and profiles**: run a simulation: the widening warning and the table appear; with the mock's preference set to Essential, `/policy` and `/policy/draft/publish` still show the notes, the **Locked Deny** pill, the acknowledgement boxes and the Publish button (P0 content), and `/policy/decisions/<id>` still shows the hard-limits card; no `console.error` and no unexpected request.

### 12.6 Visibility (37's helpers; `src/features/policy/policyVisibility.test.tsx`)

| Test | Expected |
|---|---|
| `p0_visible_in_every_profile_on_every_policy_screen` | for the overview (Rules and Decisions tabs), a version page, the editor, the publication page, the decision pages (`decision-locked`, `decision-limits`, `decision-unavailable`, `decision-unevaluable`) and the simulator after a run, `expectP0Everywhere` renders the screen in Detailed, Standard and Essential with `expectedIds` computed by an independent helper from the fixture (for example `policy.shadow`, `policy.active` and `policy.rules` for `overview-active`; `publish.findings` and `publish.locks` for `preview-blocking`; `decision.locked` for `decision-locked`; `decision.limits` and `decision.gaps` for `decision-limits`). The expected set must be non-empty for every screen and at least one screen must produce each of `policy.shadow`, `draft.banner`, `draft.validation`, `publish.findings`, `publish.locks`, `publish.widening`, `decision.locked`, `decision.exception`, `decision.limits`, `decision.gaps`, `simulator.basis` and `simulator.widening`, so the test cannot pass vacuously |
| `p0_visible_in_every_profile_with_a_page_override` | the same for the two page keys `policy-draft` and `policy-decision`, default Detailed with an Essential override and the reverse |
| `standard_collapses_decision_facts_and_inputs_and_essential_hides_metadata_behind_plus_details` | on `decision-exception`: "Facts consulted" and "Recorded inputs" are toggles with counts in Standard and Essential; Essential shows "+4 details" and the metadata appears after the click |
| `the_active_policy_the_rules_and_the_locked_deny_pill_are_identical_in_every_profile` | `textContent` of `policy.rules`, `draft.rules` and `decision.locked` equal across profiles |
| `publishing_still_asks_for_the_password_in_essential` | the dialog, the field and the body of the request are the same as in Detailed |
| `the_profile_never_changes_which_requests_are_made` | the calls of the overview, the editor and the decision page are the same set in Detailed and Essential apart from the preferences request |

### 12.7 Gates (`src/features/policy/gates.test.ts`, reading sources with `import.meta.glob("../../**/*.{ts,tsx}", { query: "?raw", eager: true })`, excluding tests)

| Test | Expected |
|---|---|
| `visibility_never_gates_policy_publication_code` | `PublishControls.tsx`, `LockedChangeDialog.tsx`, `useSaveDraftFlow.tsx`, `StartDraft.tsx` contain none of `useVisibility`, `useProfile`, `Visible`, `presentationFor`, `DetailsScope`, `visibilityAttrs` |
| `policy_pages_offer_no_approval_control` | nothing under `src/features/policy` imports `useApprove`, `useApproveStep`, `useDeny`, `useDenyStep`, `ApprovalCard`, `ApproveControl` or `StepApproveControl` |
| `the_password_is_never_stored_or_logged` | no file under `src/features/policy` mentions `localStorage`, `sessionStorage`, `console.`, a `useState` that holds a password, or puts `password` in a query key or a URL; `password` appears only as a mutation variable and a prop of `PasswordPrompt` |
| `policy_words_come_from_the_vocabulary` | no non-test source under `src/features/policy` contains a string literal or JSX text equal to a **whole label** of the outcome, mark, reason, limit, finding or lock-change tables (computed from the tables, not typed again) |
| `the_policy_hooks_are_the_only_policy_network_path` | `/api/v1/policy` appears only in `src/api/hooks/policy*.ts` |
| `every_policy_p0_id_of_the_table_appears_in_the_sources` | each P0 id of 6.6 occurs as an `id` in a source file (the code and the table agree) |
| `policy_visible_usage_follows_the_rules` | covered by 37's global `visible_usage_follows_the_rules`; add the four page keys to its expectations if it lists keys |

### 12.8 Guardrails that must keep passing unchanged

`guardrails.test.ts`, `cssGuard`, `contrast`, `voice`, `tone`, `messages`, the 250-line cap, no inline style, no native `title`, no raw `<button>`, `disabled` needs a reason, the mutation-feedback rule, 37's gates and every test of 30 and 37. Existing tests change only where they list navigation items or assert the exact content of `PasswordPrompt`'s callers (none should).

## 13. Acceptance criteria

1. The six routes of 6.5 exist, render from `PAGES` as lazy pages, appear in the navigation as "Policy" (System group, before Settings, no count) and in the command palette, and give their breadcrumb details.
2. `/policy` shows the hub's standing notes verbatim, the recording status with its last pass, last error and last-24-hours tiles, the active version and its rules (or the no-policy copy), the draft card, and tabs for Rules, Versions and Decisions; a failed load is a `LoadError`, never an empty state.
3. A **locked Deny** reads as the lock icon plus the words "Locked Deny" in the rules table, the editor, the locked-change dialog, the publication report, the decision explanation and the simulator, from one vocabulary entry; shadow and disabled rules carry their own marks.
4. The editor creates a draft from the active version or empty, adds, edits, duplicates and removes rules in a working copy, starts new rules as shadow rules, shows unsaved changes, guards navigation, and saves by **preview then save**: with no locked change it saves with no password; with a locked change it shows the changes and the simulation and saves only with the password and that preview's `preview_hash`.
5. `TimeWindow` inputs and every sentence about a window are labelled with the zone the hub reports as `time_window_zone`; nothing in the web hard-codes a zone; with no zone the inputs are disabled with a reason.
6. "Check rules" shows the hub's validation problems, finding counts with notes, locked changes and the simulation summary, and changes nothing.
7. The publication page lists every finding with its kind, rules, resolution and note; each blocking finding has an acknowledgement box; Publish stays disabled with a stated reason until the validation is empty and every box is ticked; confirming sends `{ revision, preview_hash, acknowledged: <exactly preview.blocking>, password }` and nothing is sent before the password is confirmed; every failure reads in the hub's words inside the dialog; success leaves for `/policy`.
8. The locked-rule changes and the simulation of affected recorded decisions are shown before a locked change is saved and before publication, and a "would become Allow automatically" warning appears whenever the hub's summary says so.
9. The decision page explains a recorded step with the stages visible: outcome, why (reason, winners, overridden rules), the dangerous exception, the locked Deny and what the rules alone would have said, the hard limits in a separate block, the missing inputs, the constraints, the matching rules, the facts, the recorded inputs and the metadata; a Deny is never offered for approval; every recorded decision is labelled "Not enforced".
10. The simulator evaluates the draft or a published version over recorded decisions with recorded or current inputs, shows baseline versus simulated per item with the comparison and a summary, always labels a current-input run as not a reproduction, shows the hub's notes, truncation and cannot-replay counts, and never writes anything.
11. Every element of 6.6 carries its category through `Visible` or `visibilityAttrs`; `p0_visible_in_every_profile_on_every_policy_screen` passes with a non-empty, non-vacuous expected set and again with a page override in both directions; the profile never changes which requests are made.
12. No policy file imports an approval hook or card, contains a visibility primitive in publication or save code, stores or logs the password, or writes a whole vocabulary label as a literal (the gates of 12.7).
13. The word tables of 6.3 exist, are covered by `tone.test.ts`, are re-exported from `tone.ts`, and every value in the fixtures has words; unknown values render neutral with the raw text.
14. `PasswordPrompt` accepts `children` and `title` with its old callers unchanged; `stepUpError` words a wrong password and a lockout; `useLeaveGuard` names the thing (the notes page keeps its text).
15. The fixtures of 12.1 exist under `src/test/fixtures/policy/` and are shared by the vitest tests and the smoke test.
16. `npm run smoke` passes with checks 12 to 15 against the built bundle, including colour contrast, 375 and 1280 px, both colour schemes and Essential.
17. `npm run gen:api` leaves no diff in `src/api`; no Rust file, migration, `Cargo.toml`, OpenAPI file or hub code changed.
18. Every new source file is at most 250 lines; no raw colour, inline style, native `title` or raw `<button>` was added outside `src/components`; icons only through `Icon`; every mutation reports through `useActionFeedback`; error words come from `describeError` and `stepUpError`; `npm run format:check`, `lint`, `typecheck`, `test`, `build` (size within budget, or raised with a reason) and `smoke` pass.
19. `DESIGN.md`, the book page and `/dev/design` are updated (section 15); no prompt number, plan reference or agent mention is in any committed file, commit message or PR text.

## 14. Migration and compatibility

- **Existing installs and data.** Nothing to migrate: the pages read what prompt 40's routes return. With `[policy] shadow = false` (the default) the overview says decisions are not being recorded and the Decisions tab is empty with that explanation; the editor, the review, the publication and the simulator work regardless. A fresh install has no version and no draft: the overview offers a draft.
- **Existing tests and screens.** The navigation lists in `routes.test.tsx`, `layout.test.tsx` and `palette.test.tsx` gain "Policy". `PasswordPrompt`'s three existing callers (`settings`, `setup`, the catalogue) are untouched in behaviour. The notes page keeps its leave-guard text. No operations, approval, incident or settings test is edited otherwise.
- **Prompt 37's smoke mock and visibility registry.** This prompt appends to `VISIBILITY_PAGES` and extends the smoke mock; any prompt that adds a page does the same.
- **Prompt 45 (permits, autonomy).** It will change what "Not enforced" means: it updates the hub's standing notes and the `not_enforced` mark together, and may add the autonomy pause switch to the overview (a P0 element, id `policy.autonomy`). Nothing here blocks it; `DecisionExplanation` already shows hard limits apart from rules.
- **Prompts 48 to 51.** 49 may later offer a contract picker for the rule editor; 51 may embed `DecisionExplanation` and the Decisions list. Neither needs a change to this prompt's contracts.
- **A follow-up** (named here so it is not forgotten): a block in the operation inspector that links a step to its decision page (a P2 `inspector.policy` element, with 37's block-count tests updated in the same change), and a publication finding for "a specific Allow overrides a broader Deny" (a hub change).

## 15. Documentation requirements

Write for the product's readers; never mention prompts, prompt numbers, the build plan or agents in any committed file.

- **`web/DESIGN.md`** (same PR): (a) "Tones": add the policy outcomes with the rationale (Allow automatically success, Ask for approval pending, Deny neutral, Cannot decide warning) and the **Locked Deny** treatment (lock icon and the words, danger tone, never colour alone), and **remove** the "locked rule" sentence and "policy outcome" from the "Planned" section (the statuses and the treatment now exist; keep the other planned statuses). (b) "Components": `PasswordPrompt` now takes `children` and `title`. (c) "How people use it": a short "Change the policy" journey: start a draft, edit, Check rules, review and acknowledge, publish with the password; saving a locked rule asks for the password; decisions are recorded for comparison and not enforced. (d) "Layout patterns": the **policy editor pattern** (rules table beside an in-page rule editor, one primary action, floating save bar, no modal as the only way to the editor). (e) the **category tables of 6.6** under 37's "Classify an element" section, as the worked example for a safety screen (acknowledgement and password controls are P0, the simulation tiles P1, facts and inputs P2, fingerprints P3). (f) "Add a status": add the policy tables to the list of vocabularies. (g) "Checks that will fail your PR": rows for "An approval control, a visibility primitive in publication code, a stored or logged password, or a whole policy label literal under `features/policy`" (`npm test`, fix: remove it, with the gate names). (h) "Performance budget": the new `npm run size` numbers.
- **Book** `docs/book/src/concepts/policy.md` (prompt 40 creates it): a section "The Policy pages" for users: the overview and what the recording status means; writing a draft (rules, shadow rules, locked rules and why changing one asks for your password); "Check rules"; publication, the conflicts you must acknowledge and the password; reading a decision (why it won, hard limits, missing inputs, "Not enforced"); the simulator (recorded versus current inputs and why current inputs are only a what-if); the time zone shown beside every time window. Link only to pages that exist; run `cargo xtask docs-links` (needs Rust; if you cannot run it, say so).
- **`web/scripts/screens.mjs`**: the pages of 4.4.
- **No** `ARCHITECTURE.md`, config reference, OpenAPI, API reference, ADR or `AGENTS.md` change: no hub, config or API change and no new command or rule (the new gates are named in `DESIGN.md`). **`/dev/design`** shows every policy status, the rule marks, `DecisionExplanation` and the `PasswordPrompt` with content, in both themes.

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

Also state in this report: the zone `time_window_zone` reported by the hub (D-30, expected `UTC`); the exact generated schema names that differed from section 6.1; how the generated `DecisionDto` represents an evaluation error and its conditions (section 6.2); the new `npm run size` numbers; and whether `screens.mjs`, `npm run smoke` and `cargo xtask docs-links` were run.
