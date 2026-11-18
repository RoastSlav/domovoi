# 38 — Interactive operation graph: kit component, keyboard model, motion, build plumbing

| | |
|---|---|
| Branch | `graph-canvas` |
| Milestone | U2 |
| Depends on (must be merged) | 33 (the R-1 evidence), 37 (visibility profiles) and 30 (operations UI), and through them 26, 27, 28, 29 and 59. All of 26–30, 33, 37 and 59 are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"**. If 30 or 37 is not merged, or the 26 fixtures are missing, stop and report |
| Can run in parallel with | 43, 48–51 (order-flexible, never concurrent: they all edit `web/` and `web/DESIGN.md`). 52 (playback) **follows** this prompt and consumes what it produces |
| Produces (shared contracts) | (1) Kit component `StepGraph` (`web/src/components/graph/`): a library-free **shell** (focus, keyboard, announcements, toolbar, legend, failure fallback) plus one **engine** file that is the only code touching the graph library. (2) Feature component `OperationGraph` and the pure model builder `buildGraphModel` (`web/src/features/operations/graph/`). (3) The keyboard model `graphNav.ts`. (4) Tokens `--motion-graph`, `--motion-graph-edge`, `--motion-layout`, the JS reader `motionMs` (`web/src/design/motion.ts`) and the pure `planTransitions`. (5) The `graph` build group, a graph-chunk check in `scripts/size.mjs`, the ESLint scope and import rule for the graph directory. (6) `useStepSelection` and the third "Graph" option of the Steps view switch. (7) `RightSlotApi.focus(id)` if 30 has no equivalent |
| Consumes | 33: `docs/rework/investigations/R-1-graph-library.md` and `R-1-design-md-amendment-proposal.md` **as accepted by the owner**. 30: `OperationDetailPage`, `StepOutline`, `steps.ts`, `operationVocab.ts` lookups, the right slot (`RightSlotSurface`, `useRightSlot`), the `?step=` selection rule, the smoke test. 37: `useProfile`, `visibilityOf`, `stepCategory`, `groupSteps`, `VisibilityPage`, `Visible`. 26: `web/src/api/operationTrace.ts` and the five golden fixtures |
| Launch gate | **R-1 accepted by the owner.** The evidence of prompt 33 exists, **and** the owner has recorded the acceptance and the chosen library, **and** the owner has answered 33's question on how the no-inline-style rule is worded for a graph library (the accepted text of item 2 of the amendment proposal). Step 1 checks all three and stops if any is missing |
| Suggested model | medium: the library-free parts are fully specified here; the judgement is in reading the accepted choice honestly and in keeping every web rule green |

## 1. Objective

Put an **interactive graph of an operation's steps** on the operation detail page, built from the frozen trace
(`schema_version: 1`) with the graph library the owner accepted in research item **R-1** (prompt 33 supplied the
evidence; this brief does not choose a library). Concretely:

- **A third view of the Steps section.** The view switch of the operation page gains "Graph" beside "Timeline" and
  "Tree". On wide screens (`WIDE_QUERY`, at least 768 px) the page opens in Graph; on narrow screens it opens in Timeline.
  The Graph view always keeps the step list (the timeline outline) directly under the graph: it is the screen-reader
  path and the fallback when the graph cannot load.
- **A kit component `StepGraph`** split into a library-free **shell** (single tab stop, keyboard model, spoken
  announcements, toolbar, edge key, failure fallback, test seams) and one **engine** (the only code that imports the
  library: layout, drawing, pan and zoom, animation). The engine is a lazy chunk.
- **A feature component `OperationGraph`** that turns a trace into a graph model, applies the visibility profile
  (technical steps fold into one node in Standard and Essential, problems never fold), keeps the selection in the URL
  (`?step=`, shared with the outline and the inspector), and opens the inspector.
- **The keyboard model** that prompt 33 left to this prompt, defined completely in 6.4.
- **Motion**: the tokens `--motion-graph`, `--motion-graph-edge`, `--motion-layout` (zero under reduced motion), the
  JavaScript reader `motionMs` that returns 0 under reduced motion so script-driven and library-driven animation obey it,
  and the burst-batching rule (at most five nodes animate at once).
- **Build plumbing the accepted amendment calls for**: a `graph` build group ahead of the catch-all `markdown` group in
  `web/vite.config.ts`, a graph-chunk check in `web/scripts/size.mjs`, an ESLint scope and import rule for the graph
  directory, guard tests, and the `web/DESIGN.md` update.
- **Tests**: all five trace fixtures render, the keyboard model, axe, reduced motion, the bundle budget, and a
  browser smoke test of the graph under the hub's real Content-Security-Policy.

Everything is browser-only: no Rust, no wire, no OpenAPI and no new hub route. With orchestration off (still the
default until prompt 58) nothing changes: there are no operations to open.

**Launch gate (R-1).** This prompt must not start until the owner accepted R-1. Step 1 verifies it and stops the run
if the evidence, the acceptance record or the owner's inline-style answer is missing. Do not pick a library yourself,
and do not treat the evidence report's *recommendation* as an acceptance.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**D-08 / RW-21 (graph and timeline, one trace).** Operation detail has two views of **one frozen trace contract**:
the step timeline/tree (prompt 30, built) and the interactive orchestration graph (this prompt). The graph is the
**primary** visualisation on operation detail; the non-canvas outline stays as the accessible representation (real DOM,
the screen-reader path). Both render from the same JSON. A trace carries `steps[]` in ordinal order
(`id, parent, kind, phase, cycle, outcome, started_at, ended_at, summary, refs`) and `edges[]`
(`{from, to, kind}`).

**RW-40 (the shape of the graph).** An edge points from the **dependent** step to its **prerequisite**, and `from`
always has the higher ordinal, so the graph is acyclic by construction. Edge kinds: `depends_on`, `caused_by`,
`verifies`, `retry_of`, `answers`. `parent` is grouping only (the tree); the graph is drawn from `edges`. An
investigation cycle is new steps with `cycle + 1`, never a graph cycle. A newer hub may send an unknown step kind,
outcome or edge kind: it renders generically (neutral, the raw value as the label) and never throws.

**D-12 (motion).** Visualisation and layout transitions may be longer than micro-interactions: `--motion-graph`
240 ms (node entry, status settle), `--motion-graph-edge` up to 400 ms, `--motion-layout` 240 to 400 ms; a single
transition never exceeds 600 ms. **Reduced motion is mandatory**: transitions are instant, every state change stays
visible without motion (label, icon, outline, text), no information is carried only by motion, and a JavaScript reader
of the same tokens returns 0, so script-driven and **library-driven** animation obey it (a canvas `animate` call, a
CSS transition the library injects). Event bursts are batched: animation never delays showing state; at most five
nodes animate at once, the rest appear in their final state; "once" emphasis is remembered by node id and state, so a
reload or a reconnect never replays it.

**R-1 (library, CSP, inline style, budget).** Prompt 33 measured candidate libraries against three hard constraints
and the owner then accepted one. The constraints still bind you:

- *Content-Security-Policy.* The hub sends one policy on every response, including the embedded UI: scripts are
  `'self'` plus the SHA-256 hash of each inline script of `index.html`; **no `unsafe-eval`, no `blob:` source, no
  `wasm-unsafe-eval`**; `default-src 'self'` makes workers, fonts, images (plus `data:`) and `connect` same-origin;
  `style-src 'self' 'unsafe-inline'` (inline styles are allowed **today**). You do not change the policy. A layout
  worker, if the accepted library uses one, is a same-origin hashed file, never `blob:`.
- *No inline style.* `web/DESIGN.md` says colours, spacing, motion and layers come only from tokens, and computed
  geometry goes through one kit helper that sets SVG attributes and CSS custom properties. How that rule is worded
  for a library that writes its own inline styles is the owner's accepted text (item 2 of the amendment). You implement
  that text exactly; you do not invent an exemption.
- *Bundle budget.* Initial JavaScript 170 kB gzip, any one chunk 120 kB gzip, all CSS 30 kB gzip. The graph is its own
  lazy chunk and adds nothing to the first download.

**RW-21 / D-10 / prompt 37 (visibility profiles).** One set of components with a category-to-visibility table:
P0 (safety, approval, audit, current state) is shown in every profile; P1 primary shown; P2 technical detail is
**collapsed** in Standard and Essential; P3 metadata is collapsed in Standard and hidden behind "+N details" in
Essential. *Collapsed* means a labelled stand-in is shown and the content is not drawn until opened. A step's
category is `stepCategory(kind, outcome)`: approvals, executions, verifications and questions are P0; assessments,
plans, policy decisions, notifications and notes are P1; evidence requests and reasoning turns are P2; **any problem
outcome** (failed, refused, not run, unavailable, result unknown) makes a step P0 whatever its kind, and an unknown
kind is P1, an unknown outcome P0. **Graph nodes follow step categories**: in Standard and Essential a run of
consecutive P2 steps becomes **one group node** "N technical steps", exactly as the timeline folds them, and the
group opens when the person asks or when the selected step is inside it. P0 is never folded.

**Selection and the right slot (prompt 30).** The inspector's selection is owned by the URL: `?step=<step id>`
(`replace` navigation). Selecting a step opens the **step inspector** in the shared right slot; the outline marks the
selected step with `aria-current="true"`. The slot's Escape rule: one press closes one thing; the inspector closes when
focus is inside it; the **graph selection is last** in the order (tooltip, popover or menu; dialog or palette; the
panel with focus; then the graph selection).

**D-25 (playback is later).** Historical playback is prompt 52 and is **not** built here. Design so it can follow: the
graph is a pure function of the `trace` it is given (no data fetching inside the kit), a status-only change updates
nodes **without a new layout**, and the "remembered emphasis" set can be reset by a key.

**Untrusted text.** A step's `summary` is derived from model and host output. It is drawn as **text** (React child,
SVG `<text>`, canvas label) and never as HTML in every engine.

**Decisions made here (do not revisit).**

1. **Two layers.** The kit shell is library-free and fully tested in jsdom; the engine is the only file family that
   imports the library. The shell talks to the engine through the fixed `GraphEngineProps` / `GraphEngineHandle`
   (6.2). A library swap later touches only the engine.
2. **Focus is not selection.** Arrow keys move **focus** between nodes (drawn as a ring, announced); **Enter** or a
   click makes the focused step the **selection** (`?step=`) and opens the inspector. Arrow keys never write the URL,
   so keyboard browsing never opens a modal sheet or steals focus. (DESIGN.md: "arrows move between connected nodes,
   Enter opens the inspector".)
3. **Layout direction is left to right**: prerequisites left, dependents right, arrows drawn from prerequisite to
   dependent, so time flows right. The keyboard model assumes it. If the accepted library cannot do this, stop and
   report.
4. **Folded groups are contiguous.** Groups are runs of consecutive steps in **global ordinal order** (also split when
   the cycle changes). This keeps the contracted graph acyclic (6.3).
5. **The step list stays in Graph view**, directly under the graph, not collapsed.
6. **Default view is Graph on wide screens, Timeline on narrow**, not persisted (as 30 decided for the choice).
7. **Windowing is the outline's rule** (the newest 200 steps plus every step that is not final, every result-unknown
   step and the selected one, with a "Show N earlier steps" button).
8. **No new hub route, no new dependency except the accepted library** (and its layout engine if the acceptance names
   one), at the exact version the evidence recorded.
9. **Inline style follows the accepted amendment text exactly.** If it names one sanctioned helper file, that file is
   the only place that sets a `style` attribute or a custom property.

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, **(introduced by NN — verify in code before use)**.
Line numbers of `web/` files are from before prompts 30 and 37 merged and may have moved: find the symbol, not the line.

### 3.1 The gate files (written by prompt 33 and the owner)

- `docs/rework/investigations/R-1-graph-library.md` **(introduced by 33)**: the evidence report. Sections you read:
  "Environment" (exact package versions), "Recommendation" (section 8), "Consequences for prompts 38 and 52" (section 11),
  "Decisions requested from the owner" (section 10). Its first lines say R-1 is not accepted; the owner changes that
  when accepting (step 1 checks how).
- `docs/rework/investigations/R-1-design-md-amendment-proposal.md` **(introduced by 33)**: eight items: (1) the graph
  library paragraph, (2) inline style written precisely with the ESLint scope diff, (3) bundle (the exact `graph` group
  text, size check, budget row), (4) workers and the policy, (5) motion tokens, (6) keyboard model, (7) what moves out
  of "Planned", (8) files this prompt changes. Its first line says "PROPOSAL, not accepted" until the owner accepts.
- `docs/rework/99-open-decisions.md` section C: where the owner records the R-1 decision.
- `docs/rework/investigations/probes/r1-graph/` **(introduced by 33)**: the throwaway probes. They are **evidence, not
  product code**: never import from them and never copy a file wholesale; reading them to see how the accepted
  library was driven is allowed. Their `src/layout/types.ts` constants (`NODE_W`, `NODE_H`, `RANK_SEP`, `NODE_SEP`,
  `LayoutFn`) and the probe `keyboardModel.ts` are the starting point of 6.2 and 6.4 here.

### 3.2 Build, lint, budget (all existing)

- `web/vite.config.ts:23-37`: `build.rolldownOptions.output.codeSplitting.groups` has two groups: `vendor`
  (`react|react-dom|react-router|scheduler|@tanstack|openapi-fetch`, priority 20) and `markdown`
  (`:34-36`, priority 10) whose `test` regex matches **every other `node_modules` package** except those names and
  `lucide-react`. **A newly added graph library, dynamically imported, would land in the `markdown` chunk** and be
  counted with the Markdown stack. Hence the `graph` group of 6.9.
- `web/scripts/size.mjs` (4-space indented): `CHUNK_LIMIT = 120 * KB` (`:18`, `KB = 1000`), `CSS_LIMIT = 30 * KB`
  (`:19`), `initialScripts()` (`:35`) reads `dist/index.html` for the scripts and preloads, `largest` (`:48`),
  `--report` (`:54`) prints and never fails, the problem list (`:65-71`). `npm run build` = `tsc --noEmit && vite build
  && npm run size`.
- `web/eslint.config.js`: base block (`:10`) has `no-restricted-globals` (`fetch`) and `no-restricted-imports`; the
  `max-lines` 250 block (`:39`); the **page-code block** (`:47`) applies `no-restricted-syntax` on
  `JSXAttribute[name.name='style']`, the `domovoi/*` rules and its own `no-restricted-imports` **only to
  `src/features/**` and `src/app/**`**; the kit (`src/components/`) is not covered and uses inline `style` today
  (`DataTable.tsx`, `Popover.tsx`, `Tooltip.tsx`, `ChatComposer.tsx`). In ESLint flat config a later block's
  `no-restricted-imports` **replaces** (does not merge with) an earlier block's: a new import rule must be added to
  every block that sets the option.
- `web/src/guardrails.test.ts` (existing): proves each lint rule fires by calling `new ESLint(...).lintText(code,
  { filePath })`. You extend it (12.9).
- `web/src/styles/tokens.css`: `--motion-fast: 120ms`, `--motion-base: 180ms`, `--ease-out` (`:175-177`), zeroed in the
  `@media (prefers-reduced-motion: reduce)` block (`:398-402`); spacing `--space-5: 1.5rem`, `--space-6: 2rem`,
  `--space-8: 4rem` (`:117-120`); `--color-focus`, `--shadow-focus` (`:208-209`); status colours `--tone-<tone>-bg|fg|border|solid|on-solid` for
  `danger warning success pending progress info neutral`; `--color-surface`, `--color-surface-raised`, `--color-border`,
  `--color-border-strong`, `--color-accent`, `--color-text`, `--color-text-muted`. The dark theme is `data-theme="dark"`
  on `<html>`. Prompt 30 adds `--motion-panel` **(introduced by 30)**. The three graph tokens **do not exist**.
- `web/src/styles/contrast.ts:142` `requiredPairs()` lists every `{fg, bg, min, why}` pair that
  `styles/contrast.test.ts` checks in both themes (text 4.5, UI 3). `styles/cssGuard.test.ts` forbids raw colours
  outside `tokens.css`. A library stylesheet imported from `node_modules` is not scanned, but the rule is that no other
  stylesheet has a raw colour: **never import a library stylesheet that ships colours**.
- `web/DESIGN.md`: "The shell" (`:141`), "Components" (`:174`), "How people use it" (`:303`), "Planned (added with the
  operation UI)" (`:363`; the motion table `:372-378`, Escape order `:405-411`, "New kit components" `:412-418`, "Lint and size
  amendments the graph will need" `:420-424`), "Checks that will fail your PR" (`:464`), "Performance budget"
  (`:488`, the machine-read line `<!-- budget:initial-js-gzip-kb 170 -->` at `:490`). Prompts 30 and 37 edit it too:
  **quote the text on your branch, not these lines**.
- `web/src/components/Markdown.tsx` (existing): the kit's lazy pattern: `const MarkdownView = lazy(() =>
  import("./MarkdownView"))` inside a component with a `Suspense` fallback. Copy this shape.
- `web/src/components/index.ts` (existing): the only import point for page code; kit files are imported from it.
- `web/src/app/useMediaQuery.ts` (existing): `WIDE_QUERY = "(min-width: 768px)"` (`:4`), `useMediaQuery(query)` (`:10`).
  `web/src/test/fixtures.ts:100` `stubMedia(matches)` makes every media query match or not (jsdom has none).
  In tests `matchMedia` is unstubbed by default, so `useMediaQuery` is false: **the default view in 30's tests stays
  Timeline** and their expectations do not change.
- jsdom has no layout, no canvas and no `ResizeObserver`; the shell guards `ResizeObserver` and every test that needs
  an engine uses the test engine of 12.2.

### 3.3 From prompt 30 (introduced by 30 — verify every name in code)

- `web/src/features/operations/OperationDetailPage.tsx`: composes the page; `Section title="Steps"` with
  `actions = <SegmentedControl label="View">` ("Timeline" | "Tree") and `StepOutline`; mounts the inspector as
  `<RightSlotSurface spec={{ id: "inspector", label: "Step inspector", icon: "search" }} onClose={clearSelection}>`; an
  effect keyed on the valid `?step=` value calls `open("inspector", { focus, opener })` where `focus` is true when the
  change came from a row click and false for a deep link; an absent or invalid param closes it and drops the invalid
  param (`replace`).
- `StepOutline({ trace, executions, selected, onSelect, view })`, `StepRow`, `steps.ts` (`byCycle`, `buildTree`,
  `windowSteps(steps, { limit, showAll, keep })`, `retryTarget`, `connections(trace, stepId)`).
- `web/src/design/operationVocab.ts` and lookups in `tone.ts`: `stepKindMeta(kind)` (label, icon, hint),
  `stepOutcomeMeta(outcome)` (label, tone, icon, hint), `edgeKindMeta(kind)` (label), `reverseEdgeLabel(kind)`; every
  lookup answers a neutral meta with the raw value for an unknown key.
- `web/src/app/rightSlot/`: `useRightSlot(): RightSlotApi { specs, openIds, isOpen, open(id, { opener, focus }), close,
  toggle }`, `useSlotModal()`, rule "opening an already open surface only brings it to the front: it never steals
  focus", Escape closes the surface that has focus and calls `preventDefault()`.
- `web/src/features/operations/operationFixtures.test.tsx` (`describe.each` over the five fixtures) and the browser
  smoke test `web/scripts/smoke.mjs` with `smoke/{server,data,checks}.mjs` (mock hub built from the five fixtures;
  checks 1 to 9; `npm run smoke` runs in CI job `web`).

### 3.4 From prompt 37 (introduced by 37 — verify every name in code)

- `web/src/design/visibility.ts` (pure): `Category`, `ProfileId`, `Presentation`, `PRESENTATION`, `presentationFor`,
  `visibilityOf(profile, category)`.
- `web/src/components/`: `useProfile(): { profile, page, custom }`, `useVisibility(category)`, `VisibilityPage`,
  `Visible`, `DetailsScope`, `Disclosure`, `visibilityAttrs(category, id)`.
- `web/src/features/operations/stepVisibility.ts`: `PROBLEM_OUTCOMES`, `stepCategory(kind, outcome)`,
  `reasonCategory`, `treeCategory`, `groupSteps(items, profile, categoryOf) -> OutlineItem<T>[]`.
- Test helpers `web/src/test/visibility.tsx`: `ProfileProvider`, `renderWithProfile`, `p0Elements`,
  `expectP0Everywhere`, and the test `p0_visible_in_every_profile`.
- Category ids already used on the operation page include `operation.steps-head` and `operation.step`; the page is
  wrapped in `<VisibilityPage page="operation">`.

### 3.5 From prompt 26 (introduced by 26 — verify)

- `web/src/api/operationTrace.ts`: `OperationTrace`, `TraceOperation`, `TraceStep`, `TraceEdge`, `TraceRef` with unions
  that accept an unknown string (27 may have re-pointed them at the generated types; use whatever is exported).
- `web/src/test/fixtures/operation-trace/{waiting-approval,verified-success,failed-verification-new-cycle,
  outcome-unknown-paused,question-and-budget-pause}.json`: five hand-built traces that together hold every step kind,
  outcome and edge kind. They are small (a handful to about a dozen steps).

### 3.6 What does not exist

No `web/src/components/graph/`, no `web/src/design/motion.ts`, no `--motion-graph*` token, no graph library in
`web/package.json`, no `graph` build group, no graph check in `size.mjs`.

## 4. Exact scope

All paths are under `web/` unless stated. Every source file stays at or under 250 lines (blank lines and comments are
not counted by the rule; plan for it). **(new)** means the file does not exist yet.

### 4.1 Dependency and build plumbing

| File | Change |
|---|---|
| `package.json`, `package-lock.json` | add the accepted graph library (and its layout engine, if the acceptance names one) to `dependencies`, **exact version** from the R-1 report's "Environment" section (no `^`). Install with `npm install --save-exact <pkg>@<version>`; then `npm ci` must pass |
| `vite.config.ts` | a `graph` group before `markdown` (6.9) |
| `scripts/size.mjs` | graph-chunk checks and report line (6.9) |
| `eslint.config.js` | graph-directory scope: no inline style, library import rule in **every** block that sets `no-restricted-imports` (6.9) |
| `src/guardrails.test.ts`, `src/components/graph/graphGuards.test.ts` **(new)** | the guard tests of 12.9 |

### 4.2 Tokens, motion, contrast

| File | Change |
|---|---|
| `src/styles/tokens.css` | `--motion-graph: 240ms`, `--motion-graph-edge: 400ms`, `--motion-layout: 320ms` beside the other motion tokens; the same three set to `0ms` in the reduced-motion block (6.6) |
| `src/design/motion.ts` **(new)** | `parseDuration`, `motionMs`, `MotionName` (6.6) |
| `src/design/motion.test.ts` **(new)** | 12.1 |
| `src/styles/motion.test.ts` (30) | extend: the three tokens exist, every `--motion-*` is at most 600 ms, the reduced block zeroes **every** `--motion-*` token |
| `src/styles/contrast.ts` | add the graph pairs of 6.7 to `requiredPairs()` |

### 4.3 Kit: `src/components/graph/` (all **(new)**)

| File | Content |
|---|---|
| `types.ts` | `GraphNodeModel`, `GraphEdgeModel`, `GraphModel`, `NodePosition`, `GraphEngineProps`, `GraphEngineHandle`, `StepGraphProps` (6.2) |
| `graphNav.ts` | the pure keyboard model `nextNode` (6.4) |
| `transitions.ts` | the pure `planTransitions` (6.6) |
| `edgeStyle.ts` | `edgeStyle(kind)`: dash pattern and weight per edge kind (6.7) |
| `palette.ts` | `readPalette`, `usePalette`, `toneVars`: resolves tokens for engines that cannot use CSS variables (6.7) |
| `geometry.ts` | **only if** the accepted amendment names a sanctioned helper for computed geometry; it is then the one place that sets a `style` attribute or custom property (6.9) |
| `announce.ts` | pure text builders for the live line, instructions and labels (6.8) |
| `StepGraph.tsx` | the lazy shell: `Suspense`, error boundary with retry, composition of 6.8 |
| `GraphShell.tsx`, `useGraphKeys.ts`, `GraphToolbar.tsx`, `GraphLegend.tsx` | the library-free parts the shell is made of |
| `GraphEngine.tsx` (+ `layout*.ts` or `engine*.ts` as needed) | the **only** files that import the library; default export of `GraphEngine.tsx` is the engine component |
| `graph.module.css` | placement and tokens only |
| `index.ts` | barrel: `StepGraph`, `preloadGraph`, the types, `edgeStyle` |
| `src/components/index.ts` | export `StepGraph`, `preloadGraph` and the types from `./graph` |
| `*.test.ts(x)` | 12.1 and 12.2 |

### 4.4 Feature: `src/features/operations/` and `graph/` subfolder

| File | Change |
|---|---|
| `graph/graphModel.ts` **(new)** | pure `buildGraphModel` (6.3) |
| `graph/foldRuns.ts` **(new)** | pure `foldRuns` (6.3), used by the model builder |
| `graph/OperationGraph.tsx` **(new)** | the wiring component (6.5) |
| `graph/useGraphState.ts` **(new)** | expanded groups, windowing, remembered emphasis key |
| `stepSelection.ts` **(new)** | `useStepSelection` (6.5): the one reader and writer of `?step=`; `OperationDetailPage` and the outline use it |
| `GraphView.tsx` **(new)** | Graph view composition: `OperationGraph`, then the step list (`StepOutline view="timeline"`) |
| `OperationDetailPage.tsx`, the Steps section file 30 created | third option "Graph" in `SegmentedControl label="View"`, default by `useMediaQuery(WIDE_QUERY)`, render `GraphView`; read and write `?step=` through `useStepSelection` (behaviour unchanged for the other two views) |
| `*.test.ts(x)` | 12.3 to 12.8 |

### 4.5 Right slot

| File | Change |
|---|---|
| `src/app/rightSlot/RightSlotContext.tsx`, `types.ts` | **only if** `RightSlotApi` has no way to move focus into an already open surface: add `focus(id: SurfaceId): void` (moves focus to the surface's `initialFocus()` or its heading; a no-op when the surface is closed or unregistered). Backward compatible: nothing else in the slot changes. A unit test in the slot's test file. This is the one permitted edit of 30's code beyond the page wiring above |

### 4.6 Demo, smoke, docs

| File | Change |
|---|---|
| `src/dev/GraphDemos.tsx`, `src/dev/DesignPage.tsx`, `src/dev/graphDemos.test.tsx` **(new)** | `/dev/design` demos (12.8) |
| `scripts/smoke/server.mjs`, `smoke/checks.mjs`, `smoke/data.mjs` (30) | graph checks and the hub's real policy on the smoke server (6.10); the axe injection of 30 changes to `page.evaluate` of the axe source (6.10) |
| `DESIGN.md`, `../AGENTS.md`, `../TESTING.md`, `../docs/book/src/concepts/operations.md` | section 15 |

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Choosing, evaluating or comparing graph libraries; re-running the spike; editing anything under `docs/rework/investigations/` | 33 and the owner. You **read** the accepted choice |
| Historical playback (a graph of the past, a scrubber, replay controls, a history endpoint) | 52. Only the three seams of D-25 (pure `trace` prop, status-only update without relayout, resettable emphasis key) are built here |
| Any Rust, hub route, wire, OpenAPI or migration change | none: stop and report if one seems needed |
| Changing the hub's Content-Security-Policy, `csp.rs`, or adding `unsafe-eval`, `blob:` or `wasm-unsafe-eval` | out of scope; a library that needs them was rejected by the owner's acceptance. If the accepted library turns out to need one, **stop and report** |
| Changing the trace schema, the five fixtures, the step or edge vocabularies of 30, the profile table or categories of 37 | 26, 30, 37 |
| A decision on strict `style-src` for the hub | the owner. You may only report what the accepted library writes into the DOM |
| Approving, denying or running anything from the graph; any mutation hook in the graph code | forbidden; the graph is read-only and opens the inspector |
| Virtualised or windowed canvas rendering beyond the outline's step window | a later kit component |
| Persisting the chosen view, the folded groups or the pan and zoom across reloads | per-viewer conveniences are not needed now |
| The Operator panel, attention centre, policy pages, knowledge pages | 36, 51, 43, 48 |
| Touch gestures beyond what the accepted library does by default (R-1 marked touch "not demonstrated"); a pinch-zoom guarantee | follow-up; the toolbar buttons are the guaranteed touch path |
| Exporting the graph as an image | not planned |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly; tests and prompt 52 depend on it) or **convention** (follow repository style).

### 6.1 Reading the accepted choice **fixed**

Do this in step 1 and write down what you found before building anything.

| Take from | What | Used for |
|---|---|---|
| `99-open-decisions.md` section C | the R-1 row says **accepted** (by the owner, with a date) and names the chosen library | the launch gate |
| `R-1-graph-library.md` "Environment" | the exact package name(s) and version(s) of the chosen library and layout engine | `package.json` (exact, no `^`) |
| `R-1-graph-library.md` "Candidate notes" for the chosen variant | how it hosts focus, what keyboard and `aria-*` it adds by default, what it writes into the DOM, whether it animates by itself, whether it uses a worker | the engine (6.2) |
| `R-1-design-md-amendment-proposal.md` item 1 | library paragraph for `DESIGN.md` | 15 |
| item 2 | the **accepted** inline-style wording, the ESLint scope diff, the name of the one sanctioned geometry helper (if any), the accepted class (S0 no `style` attribute, S1 custom properties only, S2 layout properties as inline style) and what the library writes | 6.9, the allowance constant of 6.10 |
| item 3 | the exact `graph` group text, the size check, the measured chunk size, the budget row | 6.9, 15 |
| item 4 | the worker rule | 6.9 |
| item 5 | the token lines | 6.6 |
| item 6 | the keyboard table as proven | 6.4 (this brief's table wins where they differ; say so in the report) |
| item 7 | what moves out of "Planned" | 15 |

If the owner edited the accepted text, **the accepted text wins over the proposal's original text**. If something this
brief fixes (for example a token value) contradicts the accepted text, stop and report the difference.

### 6.2 Kit types and the engine contract **fixed**

```ts
// web/src/components/graph/types.ts
import type { IconName } from "../../design/icons";

/** One node as the engine draws it. All words are resolved by the caller; the kit knows no vocabulary. */
export interface GraphNodeModel {
  id: string;                         // a step id, or "group:<first member step id>" for a folded group
  ordinal: number;                    // position in time order (a group: its first member's)
  kind: "step" | "group";
  title: string;                      // the kind label ("Evidence"); a group: "3 technical steps"
  icon: IconName;
  status: { label: string; tone: string; icon: IconName; value: string };   // value: the raw outcome ("running"), "" for a group
  summary: string | null;             // plain text; a group: the kinds counted ("Model turn ×2, Evidence")
  members: readonly string[];         // a group: its step ids in order; a step: []
  retry: boolean;                     // the step has a retry_of edge
}
export interface GraphEdgeModel { id: string; from: string; to: string; kind: string; label: string }   // from = dependent, to = prerequisite
export interface GraphModel {
  nodes: readonly GraphNodeModel[];   // in ordinal order
  edges: readonly GraphEdgeModel[];
  /** Changes exactly when the set of node ids or edge ids changes; a status-only change keeps it. */
  layoutKey: string;
}
export interface NodePosition { id: string; x: number; y: number }   // node centre in graph space (layout units), not client pixels

export interface GraphEngineProps {
  model: GraphModel;
  focusedId: string | null;           // drawn with the focus ring while `rootFocused`
  selectedId: string | null;          // a step id; drawn selected (6.7)
  rootFocused: boolean;
  /** Node ids that may animate for THIS update (at most five; planTransitions). Every other change is instant. */
  transitions: ReadonlySet<string>;
  animateLayout: boolean;             // false for large graphs and when the caller asks for none
  onPointerSelect(id: string): void;  // click or tap on a node
  onPointerOpen(id: string): void;    // double-click on a node
  onBackground(): void;               // click on empty space
  onLayout(positions: readonly NodePosition[], layoutKey: string): void;    // after every layout, and once at mount when empty
  onFailure(message: string): void;   // layout or draw failed: the shell shows the fallback
  ref?: Ref<GraphEngineHandle>;
}
export interface GraphEngineHandle {
  fit(): void;                        // fit all nodes in view, never above 100 % zoom, animated over motionMs("layout")
  zoomBy(factor: number): void;       // keeps the view centre; clamps to 25 %..200 %
  reveal(id: string): void;           // pan so the node is inside the view with a margin of --space-5; no zoom change
  positions(): readonly NodePosition[];
}
```

**Engine rules (fixed).**

1. The engine is the **default export** of `GraphEngine.tsx`; no other kit or feature file imports the library.
2. **Layout** runs left to right with node size `NODE_W = 224`, `NODE_H = 72`, `RANK_SEP = 64`, `NODE_SEP = 24` (layout
   units equal CSS pixels at 100 %; they mirror the design grid and `--space-8` / `--space-5`). A layout is
   deterministic and places no two nodes overlapping. It is recomputed **only when `model.layoutKey` changes**; a
   change of node status, summary or selection updates the drawn nodes in place and does not call the layout (D-25).
3. After each layout the engine calls `onLayout(positions, layoutKey)` with the centres of all nodes in layout units.
4. Pan by dragging the background; zoom with `ctrl`/`cmd` + wheel and with the handle; a plain wheel scrolls the page
   (the engine must not capture it). Arrow keys never pan (the shell owns the keyboard).
5. The initial view fits the whole graph (never above 100 %). After a relayout caused by new data, refit only if the
   person has not panned or zoomed since the last fit; `fit()` resets that.
6. Node text is **text**: never `innerHTML`, an HTML label, an HTML node extension or `dangerouslySetInnerHTML`.
7. All durations come from `motionMs("graph" | "graph-edge" | "layout")` read **when an animation starts**; a duration of
   0 means do not animate at all (no call to a library `animate`, no transition). No literal millisecond value appears in
   the engine. CSS transitions use the tokens: `transition: ... var(--motion-graph) var(--ease-out)`.
8. Colours come from tokens: through CSS variables when the library draws DOM or SVG, through `readPalette` (6.7)
   when it draws on a canvas; a theme change (`data-theme` on `<html>`, or the system scheme when it is unset)
   re-resolves them and redraws without a layout.
9. The engine imports no library stylesheet. If the accepted library cannot work without its own CSS, import it only if
   the accepted amendment says so and then override every colour through tokens (and say so in the report).
10. Failure: a thrown layout or draw error is caught and reported through `onFailure`; the engine never throws into React.

```ts
export interface StepGraphProps {
  model: GraphModel;
  selectedId: string | null;          // a step id or null
  label?: string;                     // accessible name of the region; default "Step graph"
  animate?: boolean;                  // default true; false = every change instant
  rememberKey?: string;               // default "live"; changing it clears the remembered emphasis (6.6)
  onSelect(id: string): void;         // click or tap on a step node
  onOpen(id: string): void;           // Enter on a step, double-click: select it and move focus into the inspector
  onClear(): void;                    // Escape with a selection, click on empty space
  onToggleGroup(groupId: string): void;   // Enter or click on a group node
  onFoldAll?: () => void;             // present when at least one group is open: the toolbar then shows "Fold technical steps"
  openerRef?: Ref<HTMLElement>;       // receives the focusable root, so the inspector can return focus to it
}
export function StepGraph(props: StepGraphProps): JSX.Element;
export function preloadGraph(): Promise<void>;   // starts the engine download; safe to call repeatedly
```

### 6.3 Building the model **fixed** (`graph/graphModel.ts`, `graph/foldRuns.ts`; pure, no React)

```ts
export interface BuildInput {
  trace: OperationTrace;
  profile: ProfileId;                       // from useProfile()
  expanded: ReadonlySet<string>;            // group ids the person opened
  selected: string | null;                  // a step id: the group containing it is open
  showAll: boolean;                         // "Show N earlier steps" pressed
  windowLimit?: number;                     // default 200 (the outline's rule)
}
export interface BuiltGraph { model: GraphModel; hiddenEarlier: number; groupOf: ReadonlyMap<string, string>; foldedGroups: number }
export function buildGraphModel(input: BuildInput): BuiltGraph;
export function foldRuns<T>(items: readonly T[], profile: ProfileId, categoryOf: (item: T) => Category,
  cycleOf: (item: T) => number): OutlineItem<T>[];     // OutlineItem from prompt 37
export function groupId(firstStepId: string): string;  // `group:${firstStepId}`
```

Algorithm (pseudo-code; the tests of 12.3 pin it):

```text
steps = trace.steps (ordinal order)
visible = windowSteps(steps, { limit: windowLimit ?? 200, showAll, keep: ids of steps that are not final (pending or running),
          steps whose outcome is outcome_unknown, and `selected` })           // prompt 30's function and rule
hiddenEarlier = steps.length - visible.length
items = foldRuns(visible, profile, s => stepCategory(s.kind, s.outcome), s => s.cycle)
   // foldRuns = prompt 37's groupSteps over the WHOLE ordinal list (not per cycle), then every group is split where cycle changes
   // in Detailed no item is a group; a group never holds a P0 or P1 step
nodes = []
for item in items:
  if item is a single step s                    -> nodes.push(stepNode(s))
  if item is a group g (steps m1..mk):
      if expanded.has(groupId(m1.id)) or (selected in g)  -> nodes.push(stepNode(m) for each m)       // open: members are ordinary nodes
      else nodes.push(groupNode(g))             // title "{k} technical step(s)", summary = kinds counted "Model turn ×2, Evidence"
                                                // (first-seen order, label from stepKindMeta), status = neutral "Folded", ordinal = m1's
rep(stepId) = the id of the node that stands for it (the step's own node, or its group node), or undefined if windowed out
edges = []; seen = Set()
for e in trace.edges:
  a = rep(e.from); b = rep(e.to)
  if a is undefined or b is undefined or a == b: continue            // windowed out, or inside one folded group
  key = `${a}>${b}:${e.kind}`; if seen.has(key): continue; seen.add(key)
  edges.push({ id: key, from: a, to: b, kind: e.kind, label: edgeKindMeta(e.kind).label })
layoutKey = join(sorted node ids) + "|" + join(sorted edge ids)
model = { nodes, edges, layoutKey }
safety net: if the contracted graph has a cycle (it cannot, section 2 decision 4), rebuild with profile "detailed" and no
  groups; the model is then valid and the test `contraction_never_creates_a_cycle` proves the net is never needed
```

`stepNode(s)`: `id = s.id`, `ordinal = index of s in trace.steps`, `kind: "step"`, `title = stepKindMeta(s.kind).label`,
`icon = stepKindMeta(s.kind).icon`, `status = { label, tone, icon, value: s.outcome }` from `stepOutcomeMeta(s.outcome)`,
`summary = s.summary` (plain text, may be null), `retry = trace.edges.some(e => e.from === s.id && e.kind === "retry_of")`.
Unknown kind, outcome or edge kind: the lookups already return a neutral meta with the raw value; do not special-case.

### 6.4 The keyboard model **fixed** (`graph/graphNav.ts` in the kit; pure; this prompt owns the final model)

```ts
export interface NavNode { id: string; x: number; y: number; ordinal: number; outcome: string; kind: "step" | "group"; stepKind: string }
export interface NavEdge { from: string; to: string }                 // from = dependent, to = prerequisite
export type NavKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" | "PageUp" | "PageDown" | "a";
/** The node id to focus next, or null when the key moves nothing. `current` null means nothing has focus yet. */
export function nextNode(input: { nodes: readonly NavNode[]; edges: readonly NavEdge[]; current: string | null; key: NavKey }): string | null;
export const LAYER_TOLERANCE = 8;       // layout units: two centres whose x differ by at most this are in one layer
```

A **layer** is a set of nodes whose centre `x` differ by at most `LAYER_TOLERANCE`.

| Key (focus inside the graph) | Effect |
|---|---|
| `Tab` / `Shift+Tab` | the graph root is **one** tab stop: Tab enters it, the next Tab leaves it. No node is a tab stop |
| `ArrowRight` | among the **dependents** of the focused node (nodes with an edge whose `to` is the focused node) that lie in a layer strictly to the right, the nearest vertically; ties by the lowest ordinal. None: nothing moves |
| `ArrowLeft` | the same over the focused node's **prerequisites** (the `to` end of its own edges) in a layer strictly to the left |
| `ArrowDown` / `ArrowUp` | the nearest node in the **same layer** below or above; none at the end of a layer: nothing moves |
| `PageDown` / `PageUp` | the next / previous node **in time order** (by ordinal). This reaches every node, including those with no edges |
| `Home` / `End` | the first / last node by ordinal |
| `a` | the **active step**: the lowest-ordinal step that is `running`; else the lowest-ordinal `approval` or `question` step that is `pending` (what waits for a person); else the lowest-ordinal `pending` step; else the highest-ordinal step. Group nodes are never chosen; a step inside a folded group is revealed first (the feature opens its group) |
| `Enter` | on a step node: open it (select it and move focus into the inspector). On a group node: open the group (`onToggleGroup`); focus then moves to the group's first member |
| `Escape` | with a selection: clear it (this also closes the inspector, which follows the selection). Without a selection: nothing, and the event is **not** consumed. Never closes anything else |
| `+` or `=` / `-` / `0` | zoom in / zoom out / fit |
| `Shift` + arrow | not used; arrow keys never pan. Focusing a node pans it into view (`reveal`) |

Rules (fixed):

- With nothing focused, any arrow key, `PageDown`, `PageUp` and `Home` focus the lowest-ordinal node; `End` and `a` as in the table.
- The first Tab into the root focuses, in order: the selected step if any, else the previously focused node, else the
  `a` target, else the lowest-ordinal node. This is announced.
- A key that moves nothing is silent (no announcement) and not an error; the shell does call `preventDefault()` for every
  key it handles so the page never scrolls under the graph.
- `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight` need positions: until the first `onLayout` they do nothing; the
  other keys work from the model alone.
- `nextNode` is deterministic and total: any input returns an id that is in `nodes` or `null`; unknown `outcome` and
  `stepKind` values never throw.
- Keys act only when `event.target` is the graph root, `event.defaultPrevented` is false and no modifier other than
  `Shift` (for `+`) is held; `ctrl`/`cmd`/`alt` combinations pass through to the browser.
- Reachability, reported by a test (12.1): from `Home`, arrow keys alone reach every node that has an edge path to
  or from the first node in the five fixtures; `PageDown` reaches all.

### 6.5 Selection, focus and the inspector **fixed** (behaviour), **convention** (file layout)

```ts
// web/src/features/operations/stepSelection.ts   (the ONE reader and writer of ?step=)
export type SelectionSource = "row" | "graph-pointer" | "graph-open" | "link" | "deeplink";
export interface StepSelection {
  selected: string | null;                         // a valid step id of `trace`, else null (an invalid param is dropped with `replace`)
  select(id: string, source: SelectionSource): void;     // navigate to ?step=<id>, replace
  clear(): void;                                   // remove ?step=, replace
  /** Whether the inspector should take focus for the LAST change: true for "row", "graph-pointer", "graph-open". */
  focusInspector: boolean;
}
export function useStepSelection(trace: OperationTrace | undefined): StepSelection;
```

Rules:

1. **One mechanism for "the change came from a click".** Prompt 30 decides inspector focus from the origin of the
   change. Find that mechanism and make it read `StepSelection.focusInspector`; do not add a second one. Arrow keys
   never call `select` (decision 2), so keyboard browsing never changes `?step=`.
2. **Click** on a step node: `onSelect` then `select(id, "graph-pointer")`; focus and the engine's ring move to it.
   Click on a group node: toggle the group (the group's id never goes into the URL). Click on empty space:
   `onClear` then `clear()`.
3. **Enter** (or double-click) on a step: `select(id, "graph-open")`; if `?step=` already equals `id` (the inspector is
   open and the param does not change), call `useRightSlot().focus("inspector")`. The opener passed to the slot is the
   graph root (`openerRef`), so closing the inspector returns focus to the graph.
4. **Deep link and outline selection** (`?step=` changes while the graph does not have focus): the graph's focused node
   follows the selected step, the engine `reveal`s it when it is outside the view, and the group containing it is
   opened (`expanded` gains its id; this is the same "reveal" 37 gives the outline).
5. **Escape order** (DESIGN.md): the slot closes the inspector first when focus is inside it; with focus on the graph,
   Escape clears the selection, which closes the inspector through the existing effect. One press, one thing.
6. `aria-current="true"` stays on the outline row (30). The graph marks the selected node through the engine and
   exposes `data-selected-step` (6.8). Both read the same URL value, so they cannot disagree.
7. A selected step that the profile folds, or the window hides, is **never** hidden: `selected` is an input of
   `buildGraphModel` (6.3).

`OperationGraph` (feature wiring, **fixed** props: prompt 52 reuses it):

```tsx
export interface OperationGraphProps {
  trace: OperationTrace;            // the trace to draw: today the live one; a later change may pass a reconstructed past state
  selected: string | null;          // a step id
  onSelect(id: string): void;       // = select(id, "graph-pointer")
  onOpen(id: string): void;         // = select(id, "graph-open") + slot focus rule above
  onClear(): void;
  animate?: boolean;                // default true
  rememberKey?: string;             // default "live"; passed through to StepGraph
  label?: string;                   // default "Step graph"
}
export function OperationGraph(props: OperationGraphProps): JSX.Element;
```

It owns: the profile (`useProfile().profile`), the `expanded` set (open groups; in memory only), `showAll`, the
`hiddenEarlier` button "Show {n} earlier steps" above the graph, the memoised `buildGraphModel`, `onToggleGroup`,
`onFoldAll` (clears `expanded`; passed only when `expanded` is not empty), `preloadGraph()` on mount, and an empty trace
(`steps.length === 0`) shows `EmptyState title="No steps yet"` instead of the graph.

`GraphView` (`features/operations/GraphView.tsx`) renders, in order: `OperationGraph` (in
`<Visible category="P1" id="operation.graph">`), then a heading-less muted line "The same steps as a list:" and
`StepOutline` with `view="timeline"` (the unchanged component of 30, unchanged selection behaviour). The Steps section
shows `GraphView` for view `graph`, the existing content for `timeline` and `tree`.

**Default view**: `const wide = useMediaQuery(WIDE_QUERY); const [view, setView] = useState<"timeline"|"tree"|"graph"|null>(null);
const current = view ?? (wide ? "graph" : "timeline");` (the person's explicit choice always wins and is kept in
memory while the page is mounted).

### 6.6 Motion **fixed**

Tokens (`tokens.css`, under "Motion"; none is a colour, so the two dark blocks stay identical):

```css
  --motion-graph: 240ms;        /* node entry and status settle */
  --motion-graph-edge: 400ms;   /* active-path reveal, used sparingly */
  --motion-layout: 320ms;       /* re-layout, group open and fold, fit to view */
/* and in @media (prefers-reduced-motion: reduce) { :root { ... } } */
  --motion-graph: 0ms;
  --motion-graph-edge: 0ms;
  --motion-layout: 0ms;
```

```ts
// web/src/design/motion.ts   (pure except the document read; no matchMedia: reduced motion works because the tokens are 0)
export type MotionName = "fast" | "base" | "panel" | "graph" | "graph-edge" | "layout";
export const MAX_TRANSITION_MS = 600;
/** "240ms" -> 240, "0.24s" -> 240, " 240ms " -> 240; anything else (empty, "auto", NaN, negative) -> 0. Never above MAX_TRANSITION_MS. */
export function parseDuration(text: string): number;
/** The token's computed value in milliseconds, read from `root` NOW (so a change of the media setting applies to the next animation). 0 when the token is missing. */
export function motionMs(name: MotionName, root?: HTMLElement): number;
```

Where an animation is scheduled:

| What | Duration | When |
|---|---|---|
| node entry (a node that did not exist in the previous model) and status settle (fill and outline change of a node whose outcome changed) | `motionMs("graph")` | only for ids in `transitions` |
| an edge appearing next to an entering node | `motionMs("graph-edge")` | only for edges whose both ends are entering or settling, at most the same five nodes |
| nodes moving to their new positions after a relayout, group open or fold, `fit()`, `reveal()` | `motionMs("layout")` | only when `animateLayout` is true |
| selection ring, focus ring, hover | none (instant) | always |

Burst batching, pure (`components/graph/transitions.ts`):

```ts
export const MAX_ANIMATED_NODES = 5;
export const ANIMATE_LAYOUT_MAX_NODES = 150;       // above this, layout changes are instant
export interface TransitionPlan { animate: ReadonlySet<string>; remembered: ReadonlySet<string> }
/** `prev` null (first render, reload, reconnect) animates nothing and remembers every current node state. */
export function planTransitions(prev: GraphModel | null, next: GraphModel, remembered: ReadonlySet<string>): TransitionPlan;
```

Rules: a candidate is a node that is **new** (id not in `prev`) or **changed** (same id, different `status.value`);
its state key is `${id}:${status.value}`; a candidate whose key is in `remembered` is skipped (emphasis is shown once per
node and state); candidates are ordered **changed before new, then by ordinal descending** (the newest first) and the
first `MAX_ANIMATED_NODES` animate; every other change is applied at once; `remembered` of the result contains the
previous set plus the key of **every** candidate (animated or not) and every key of `next`'s nodes that was already
present. `StepGraph` holds `remembered` in a ref and resets it when `rememberKey` changes. `animateLayout =
animate !== false && next.nodes.length <= ANIMATE_LAYOUT_MAX_NODES`. Animation never delays showing state: the engine
draws the final state at once and only animates the **difference** (opacity of an entry ring, fill and outline
transition); a status change is also visible in the node's label and icon immediately, with no motion.

### 6.7 Visual specification **fixed** (so every engine looks the same)

- **Node** 224 x 72, radius `--radius-md`, fill `--tone-<tone>-bg`, border 1 px `--tone-<tone>-border`, text
  `--tone-<tone>-fg`, where `<tone>` is `status.tone`. Content: first row, kind icon and `title` (12 px, 600, truncated
  to one line) and on the right the outcome label with its icon (12 px); second block, `summary` clamped to **two
  lines** with an ellipsis (12 px, `--font-sans`). A `retry` node shows a small "Retry" word after the title. A running
  step shows the loader icon (drawn **static**; a spinning icon would be motion carrying information). A group node
  uses `neutral` tone, a dashed border and the text `title` plus `summary`.
- **Selected** node: border 3 px `--color-accent`. **Focused** node (only while the root has focus): an additional
  outer ring 2 px `--color-focus` with a 2 px gap. Both can apply at once. Neither is the only signal: the live line
  and the outline carry the same facts.
- **Edge**: a line from the prerequisite's right side to the dependent's left side with an arrowhead at the dependent,
  stroke `--color-text-muted` 1.5 px. **Kinds differ by pattern, never by colour alone**, from `edgeStyle(kind)`:

  | kind | dash pattern (`stroke-dasharray`, px) | weight |
  |---|---|---|
  | `depends_on` | none (solid) | 1.5 |
  | `caused_by` | `6 4` | 1.5 |
  | `verifies` | `2 3` | 2 |
  | `retry_of` | `8 3 2 3` | 2 |
  | `answers` | `12 4` | 1.5 |
  | any other kind | none (solid) | 1 |

  The edge's `label` is drawn next to the edge only while its dependent or prerequisite is focused or selected, and is
  always in the edge key (`GraphLegend`) and the live line.
- **Palette for canvas engines** `palette.ts`: `readPalette(root: HTMLElement): GraphPalette` returns resolved colour
  strings for `surface`, `border`, `borderStrong`, `text`, `textMuted`, `accent`, `focus` and, for each tone,
  `{bg, fg, border, solid}`, read with `getComputedStyle(root).getPropertyValue("--…")` (a token that computes to a colour
  function or a `color-mix` must be normalised into a form the library accepts; record which). `usePalette()` re-reads
  on theme change (a `MutationObserver` on `<html data-theme>` and a `matchMedia("(prefers-color-scheme: dark)")`
  listener when `data-theme` is unset). `toneVars(tone)` returns the five custom-property names for DOM engines.
- **Contrast pairs** added to `requiredPairs()` (`why` strings in sentence case): `--color-accent` on `--color-surface` (min 3, "selected
  node border"), `--color-focus` on `--color-surface` (min 3, "focus ring"), `--color-text-muted` on `--color-surface`
  (min 3, "edge stroke"). The node text pairs (`--tone-*-fg` on `--tone-*-bg`) already exist: confirm they are there and
  add any that are missing. If a pair fails, change which token the graph uses, never a token value.
- **Frame**: the graph sits in a bordered surface (`--color-surface`, `--color-border`, `--radius-md`) with height
  `clamp(20rem, 60vh, 40rem)`, the toolbar strip above it, and the edge key below it.

### 6.8 The shell: DOM, text and failure **fixed**

```html
<div data-graph-frame>
  <div role="toolbar" aria-label="Graph controls"> Fit to view | Zoom in | Zoom out | (Fold technical steps) </div>
  <div data-graph role="group" aria-roledescription="graph" aria-label="Step graph" aria-describedby="<id>-help" tabindex="0"
       data-variant="<accepted library id>" data-ready="true|false"
       data-focused-node="<id|empty>" data-selected-step="<step id|empty>"
       data-graph-nodes="<node ids, space separated, ordinal order>" data-graph-groups="<group ids, space separated>"
       data-layout-key="<layoutKey>">   <!-- the engine renders here -->   </div>
  <p id="<id>-help" class="visually-hidden">Arrow keys move between connected steps. Page Down and Page Up move through steps in time order. Enter opens the step. Escape clears the selection. Plus, minus and zero zoom and fit.</p>
  <div role="status" aria-live="polite" class="visually-hidden"> the live line </div>
  <GraphLegend/>   <!-- Edge key -->
</div>
```

- The toolbar buttons are kit `IconButton`s with visible tooltips and labels exactly: "Fit to view", "Zoom in",
  "Zoom out", and (only when `onFoldAll` is given) a `Button` "Fold technical steps". They are ordinary tab stops
  **before** the graph; inside `[data-graph]` there is exactly one tab stop (the root).
- **Live line** (`announce.ts`, updated only when the focused node changes or the selection changes, never on data
  refresh): for a step `"{title}, {status.label}: {summary}. Depends on {n} {step|steps}. Needed by {m} {step|steps}."`;
  omit ": {summary}" when the summary is null, omit "Depends on" or "Needed by" when the count is 0, append " Selected."
  when it is the selection. For a group `"{k} technical steps folded: {summary}. Press Enter to show them."`. When the
  model is empty or the engine is not ready: nothing.
- The data attributes are the test and smoke seam: they are exposed by every engine because the shell writes them.
  `data-graph-nodes` lists the nodes **currently in the model**, so a test can assert that a P0 step is present in every profile
  without reading pixels.
- **Edge key** (`GraphLegend`, real DOM, wrapped by the feature in `<Visible category="P2" id="operation.graph-key"
  label="Edge key" count={kinds}>`): one row per edge kind **present in the model**: a small inline `<svg>` line sample
  (`stroke-dasharray` as an SVG attribute, never a `style`) and the label `edgeKindMeta(kind).label`; an unknown
  kind shows the raw string with a solid sample.
- **Failure** (the chunk did not download, the engine threw, `onFailure`): replace the graph and toolbar with
  `InlineMessage tone="warning" title="The graph could not be drawn"` and the body "The step list below has the same
  steps." plus a `Button` "Try again" that remounts the engine (a new `lazy()` per attempt, so a rejected import is
  retried) and, after the second failed attempt, adds "If it keeps failing, reload the page." The step list below the
  graph is always rendered, so the page stays fully usable.
- **Loading**: the `Suspense` fallback is a `Skeleton` of the frame's height; it announces nothing.
- The region label is a prop (`label`), default "Step graph".

### 6.9 Build plumbing **fixed**

`vite.config.ts`: add, **between** `vendor` and `markdown`:

```ts
// The graph library is only fetched when a graph is first shown (src/components/graph/StepGraph.tsx).
// It must not join the Markdown chunk, or the two would be downloaded and budgeted together.
{
  name: "graph",
  test: /node_modules[\\/](<the accepted package folder names, joined with |>)[\\/]/,
  priority: 15,
},
```

Take the package list from item 3 of the accepted amendment (the library, its layout engine and anything the R-1 report
lists as a runtime dependency that only the graph uses). Keep the `markdown` group's lookahead unchanged. After
`npm run build`, **prove** the separation: the graph chunk is named `graph-<hash>.js`, the markdown chunk does not
contain library code, and the graph chunk statically imports at most `vendor` (open the built file and read its
top-level `import` statements). A shared dependency that fell into `markdown` and made the graph chunk import it is a
failure: add that package to the `graph` group, or to `vendor` if it is already shared.

`scripts/size.mjs`: after the existing report lines add a **graph chunk** report and checks (same 4-space style; no new
dependency):

```text
graphChunks = chunks whose file name starts with "assets/graph-"
report line:  "Graph chunk (gzip):  <kb> (<file>)"   or   "Graph chunk (gzip):  none"
problems (not in --report mode):
  - no graph chunk exists                                    -> "No graph chunk was built. The graph library must be its own lazy chunk (vite.config.ts group `graph`)."
  - a graph chunk is in initialScripts()                     -> "The graph chunk is loaded by the first page. Import it only through StepGraph's dynamic import."
  - a graph chunk is reachable from an initial script by static imports (parse `from"./x.js"` and `import"./x.js"` in each initial
    chunk, transitively; a dynamic `import("./x.js")` does not count)
                                                            -> "The graph chunk is statically imported by <file>."
  - a graph chunk statically imports a chunk that is not `vendor-*`   -> "The graph chunk depends on <file>; keep the library self-contained."
the existing 120 kB per-chunk limit already covers the graph chunk; do not add a second number.
```

ESLint (`eslint.config.js`), exact intent (write the accepted amendment's text if the owner changed it):

```js
  {
    // The graph draws computed geometry; it follows the same inline-style rule as page code (web/DESIGN.md).
    files: ["src/components/graph/**/*.{ts,tsx}"],
    ignores: ["src/**/*.test.{ts,tsx}", "src/components/graph/geometry.ts"],   // geometry.ts only if the amendment names it
    rules: {
      "no-restricted-syntax": ["error", {
        selector: "JSXAttribute[name.name='style']",
        message: "No inline styles in the graph: use a CSS module with tokens, or the sanctioned geometry helper.",
      }],
    },
  },
```

and the **library import rule**: add a `{ group: [<accepted package names and their sub-paths, e.g. "pkg", "pkg/*">], message:
"Import the graph library only inside src/components/graph (web/DESIGN.md)." }` entry to the `patterns` of **every**
existing `no-restricted-imports` option (the base block and the page-code block), and a final block for
`src/components/graph/**` that sets `no-restricted-imports` to the same patterns **minus** that library entry (otherwise
the graph directory could not import it). Nothing outside `src/components/graph/` may import the library.

### 6.10 Smoke additions **fixed** (what is checked), **convention** (code)

All checks run in the existing `npm run smoke` (Chromium against the built bundle and the mock hub of prompt 30); keep
checks 1 to 9 of 30 green.

- **The hub's real policy.** `smoke/server.mjs` sends `Content-Security-Policy` on every response. The value is read at
  run time from `../crates/domovoi-hub/src/csp.rs`: extract the string literal of `pub const BASE_POLICY: &str =` with a
  regular expression (throw if it matches zero or several), append `; script-src 'self'` and one `'sha256-<base64>'` per
  inline `<script>` (no `src`) of `dist/index.html`, hashed over the exact text between the tags, as `csp.rs`
  `inline_script_hashes` does. The page records `securitypolicyviolation` events through an init script. **Control**:
  the run first proves enforcement by adding an inline script with `page.addScriptTag({ content: "window.__x = 1" })`
  and expecting a violation and `window.__x === undefined`; if it is not blocked the run fails (the policy is not
  being applied). Because the policy blocks `addScriptTag`, check 8 of 30 injects axe with
  `page.evaluate(readFileSync(<axe-core/axe.min.js>, "utf8"))` and then `page.evaluate(() => axe.run(...))`.
- New checks, after check 9 of 30:
  10. At 1440 px wide, open the waiting-approval operation: the Steps view is **Graph**; `[data-graph]` has
      `data-ready="true"` within 5 s; the number of ids in `data-graph-nodes` equals the trace's step count.
  11. **Keyboard**: Tab until focus reaches `[data-graph]`; one more Tab leaves it (exactly one tab stop); focus it again;
      `ArrowRight`, `ArrowLeft`, `PageDown`, `Home`, `End`, `a` change `data-focused-node` as `nextNode` predicts for the
      fixture; `?step=` does **not** change; `Enter` sets `?step=`, opens the inspector and moves focus into it; `Escape`
      with focus inside the inspector closes it and focus returns to `[data-graph]`; `Escape` with focus on the graph
      and a selection clears `?step=`.
  12. **Sync**: click an outline row below the graph: `data-selected-step` equals its step id and the graph's selected node
      is that step; reload at that URL: the same step is selected and the inspector is open without taking focus.
  13. **Reduced motion**: with `emulateMedia({ reducedMotion: "reduce" })`, push a journal frame that changes a step's
      outcome (the mock updates its data): within 50 ms `document.getAnimations().length === 0`, and the live line or the outline
      shows the new outcome label; with `"no-preference"` the same change produces at least one animation **or** the
      token read by `motionMs` is 240 (so the check is not vacuous).
  14. **Inline-style audit**: after the interactions of 10 to 13, collect, for every element inside `[data-graph]` with a
      `style` attribute, the CSS property names; they must be a subset of the **accepted allowance**: a constant
      `GRAPH_STYLE_ALLOWANCE` in `checks.mjs` that you fill from the accepted amendment (S0: the empty list; S1: only names
      starting with `--`; S2: the named properties). Also print the count of `<style>` elements added at run time and of
      `insertRule` calls (wrap `CSSStyleSheet.prototype.insertRule` in the init script) as information.
  15. **No CSP violation and no `console.error`** during the whole run (the existing check 9 extended with the violation list).
  16. At 375 px: no horizontal scroll with the Graph view chosen by hand; the toolbar buttons are reachable.
- The mock hub (`smoke/data.mjs`) already builds its data from the five fixtures; reuse them. It must list any new `/api` request (none is
  added by this prompt).

### 6.11 Visibility of the graph **fixed** (categories follow 37 section 6.5)

| id | element | cat | mark |
|---|---|---|---|
| `operation.graph` | the whole graph frame: toolbar, canvas, live line (`GraphView` wraps `OperationGraph`) | P1 | V |
| `operation.graph-key` | the edge key | P2, `label="Edge key"`, `count` = edge kinds present | V |
| (nodes) | a step node's category is `stepCategory(kind, outcome)`; a folded group is the P2 stand-in. Nodes are not DOM wrappers; the rule is applied by `buildGraphModel`, and `data-graph-nodes` proves it | P0, P1, P2 | model |
| (toolbar) | Fit, zoom, "Fold technical steps": controls of the P1 frame | P1 | inside the frame |
| (empty and failure states) | "No steps yet" and "The graph could not be drawn": P0 by 37's rule (a failure or an empty state is P0). They are rendered inside the `operation.graph` wrapper, which is P1 and therefore always shown in every profile, so they are never collapsed | P0 | none (no wrapper of their own) |

Hard rules (37): no P0 element inside a P2 or P3 `Visible`; the failure message and the live line carry no wrapper of
their own; dialogs, toasts and tooltips carry none. The graph has **no** decision control: nothing in it is an approve,
deny, resume or cancel.

## 7. Ordered implementation steps

Work in `web/` unless a path says otherwise. Commit in small green steps (one-line Conventional Commits, exactly one
scope, for example `feat(web): fold technical steps in the graph model`) after steps 4, 6, 7, 8, 10, 11 and 12. Run
`npm run lint`, `npm run typecheck` and the vitest files of the area after each step.

1. **Inspect and verify the repository context; report discrepancies before building anything.**
   - **Gate.** Open `docs/rework/99-open-decisions.md` section C and confirm the R-1 row says the owner **accepted** it and
     names the chosen library; open `docs/rework/investigations/R-1-graph-library.md` and
     `R-1-design-md-amendment-proposal.md` and confirm the amendment's first line no longer says "PROPOSAL, not accepted"
     (the owner replaced it with an acceptance note and, if they edited the text, the edited text is what you read), and
     that item 2 (inline style) holds a **statement** (the accepted wording, the ESLint scope, the named geometry helper
     or "none", the accepted class), not a question. **If any of the three is missing, stop and report.** Do not infer
     acceptance from the report's recommendation.
   - Write down, from 6.1: the library and layout engine, their exact versions, the licence identifiers, the accepted
     inline-style class and allowance, the worker rule, the chunk size the evidence measured.
   - Open every file of section 3: `vite.config.ts` groups, `scripts/size.mjs`, `eslint.config.js` blocks (note which
     blocks set `no-restricted-imports`), `tokens.css` motion and reduced-motion blocks, `styles/contrast.ts`
     `requiredPairs`, `components/Markdown.tsx`, `components/index.ts`, `app/useMediaQuery.ts`, `test/fixtures.ts`
     `stubMedia`.
   - Confirm prompts 30, 37 and 26 are merged: `ls src/features/operations src/app/rightSlot src/design/operationVocab.ts
     src/design/visibility.ts src/features/operations/stepVisibility.ts src/api/operationTrace.ts
     src/test/fixtures/operation-trace`. Read `OperationDetailPage.tsx`, the Steps section file, `StepOutline.tsx`,
     `steps.ts`, the inspector effect and how it decides focus, `RightSlotContext.tsx`, and 30's `smoke/*`. **If any is
     missing, stop and report.** Note every difference from 3.3 and 3.4 (names, signatures, props).
   - Confirm nothing named `graph` exists yet under `src/components` or `src/features/operations`, and
     `grep -rn "motion-graph\|motionMs" src` is empty.
   - `git status` clean, branch based on the latest `overhaul-investigation`, Node 22 or newer.
2. Create branch `graph-canvas` from the latest `overhaul-investigation`.
3. **Baseline.** `cd web && npm ci && npm test && npm run lint && npm run typecheck && npm run build`, then
   `node scripts/size.mjs --report`: record initial JS, largest chunk and CSS (gzip). All must be green before you start
   (if not, stop and report: the base is broken).
4. **Dependency and build plumbing.** `npm install --save-exact <library>@<version>` (and the layout engine if named);
   `npm ci` passes; `npm ls <library>` shows the exact version; run `npm audit --omit=dev` and note the summary
   (do not fix unrelated findings). Add the `graph` group, the `size.mjs` checks, the ESLint scope and import rule (6.9),
   a placeholder `src/components/graph/GraphEngine.tsx` that exports an engine rendering nothing (so the lazy import has a
   target), and the guard tests (12.9). `npm run build` must produce `graph-<hash>.js` and the new size lines. Commit
   `build(web): add the graph chunk, its size check and lint scope`.
5. **Tokens and motion.** The three tokens and their reduced-motion zeros, `design/motion.ts`, its test, the extended
   `styles/motion.test.ts`, the contrast pairs. Commit `feat(web): graph motion tokens and a reader that returns 0 under reduced motion`.
6. **Pure logic, tests first.** `graphNav.ts`, `transitions.ts`, `edgeStyle.ts`, `announce.ts`, `foldRuns.ts`,
   `graphModel.ts` with their tests (12.1 and 12.3), run against the five fixtures and the synthetic traces named
   there. Commit `feat(web): keyboard model, burst batching and the graph model`.
7. **Shell and the test engine.** `types.ts`, `GraphShell.tsx`, `useGraphKeys.ts`, `GraphToolbar.tsx`, `GraphLegend.tsx`,
   `StepGraph.tsx` (lazy, boundary, retry), `palette.ts`, `graph.module.css`, the barrel exports, and the shell tests
   with the **test engine** (12.2). Commit `feat(web): the graph shell with one tab stop and spoken announcements`.
8. **The engine** (`GraphEngine.tsx` and its helpers): layout left to right with the 6.2 constants, drawing from tokens,
   selection and focus rings, pan and zoom, `fit`, `zoomBy`, `reveal`, durations from `motionMs`, `onLayout`,
   `onFailure`, text-only labels. If the accepted amendment names a geometry helper, write `geometry.ts` and route every
   computed value through it. Open it in a browser with `npm run dev` (a throwaway route is **not** allowed in the
   product: use `/dev/design` after step 10, or a local file you do not commit) and check the five fixtures by eye
   in both themes before moving on. Commit `feat(web): the graph engine on the accepted library`.
9. **Feature wiring.** `stepSelection.ts` (and make `OperationDetailPage` and the outline read and write `?step=`
   through it with unchanged behaviour: 30's tests must pass untouched), `graph/useGraphState.ts`,
   `OperationGraph.tsx`, `GraphView.tsx`, the third view option and the default by media query, `RightSlotApi.focus` if
   needed. Run 30's and 37's whole web test files. Commit `feat(web): the Graph view of an operation`.
10. **Demos and tests.** `GraphDemos.tsx`, `graphDemos.test.tsx`, the feature tests of 12.4 to 12.8. Commit `test(web): graph rendering, profiles and selection`.
11. **Smoke.** The policy-serving smoke server, the axe injection change, checks 10 to 16 (6.10), the
    `GRAPH_STYLE_ALLOWANCE` constant. `npm run build && npm run smoke` green locally (Chromium). Commit `test(web): browser checks for the graph under the hub policy`.
12. **Look at it.** `node scripts/screens.mjs` for the operation page at the four widths in both themes (set
    `SCREENS_OPERATION`); look at every picture: nodes readable, edges not crossing node text, no clipped label,
    focus ring and selection visible, dark theme contrast, no horizontal overflow at 375 px, Standard and Essential
    profiles fold the technical steps. Fix what you see. Do not commit the screenshots.
13. **Docs.** `DESIGN.md` (the accepted amendment applied, section 15), `AGENTS.md`, `TESTING.md`, the book page.
    Commit `docs(web): document the graph view and its rules`.
14. **Checks and completion report.** Run `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`,
    `npm run build` (includes `size`), `npm run smoke`, and `npm run gen:api` followed by `git diff --exit-code
    src/api` (must be empty: this change touches no API). Fill the budget numbers into `DESIGN.md` from `node
    scripts/size.mjs --report`. No `cargo` command is needed (nothing under `crates/` or `xtask/` changed). End with the
    completion report.

## 8. Database and persistence

Not applicable: no database, table, migration or persisted state. Nothing is stored in `localStorage`; the view
choice, open groups, pan and zoom live in memory only.

## 9. Protocol and compatibility

Not applicable to the wire: no frame, route, OpenAPI document or generated type changes, `PROTOCOL_VERSION` is
untouched (RW-42), and the hub's `Content-Security-Policy` is untouched.

The one contract consumed is the operation trace `schema_version: 1`. Compatibility rules, each with a test:

- An unknown step kind, outcome or edge kind renders generically (neutral tone, raw value as the label; a solid, thin
  edge) and never throws, in the model builder, the keyboard model, the live line and the engine.
- A trace with a `schema_version` other than 1 keeps the warning prompt 30 shows; the graph still draws what it can.
- A node whose step has `summary: null` shows no second block; a trace with no steps shows "No steps yet".
- A trace whose edge names a step that is not in `steps` (a corrupt or windowed trace) drops that edge; it never
  throws.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| R-1 is not accepted, the amendment is still a proposal, or item 2 is a question | stop in step 1 and report; build nothing |
| The accepted library cannot lay out left to right, needs `unsafe-eval`, `blob:` or `wasm-unsafe-eval`, or writes styles outside the accepted allowance | stop and report; do not widen the policy, do not widen the allowance, do not choose another library |
| The graph chunk fails to download (offline, hub restarted with new asset names) | the shell shows "The graph could not be drawn" with "Try again" (a new `lazy()` per attempt); the step list below is untouched and fully usable |
| The engine throws while laying out or drawing | caught, `onFailure`, the same fallback; the page and the inspector keep working |
| `?step=` names a step that is not in the trace | `useStepSelection` returns `selected: null` and drops the param (`replace`), as 30 specifies; the graph shows no selection |
| `?step=` names a step that is folded or windowed out | it is revealed: its group opens, the window includes it (`selected` is an input of the builder) |
| The selected step disappears after a refresh | selection drops as above; the inspector closes through 30's effect |
| A key that moves nothing (ArrowRight on a node with no dependent, ArrowUp at the top of a layer) | nothing happens, nothing is announced, `preventDefault()` is still called |
| An arrow key before the first layout finished | ignored; `Home`, `End`, `PageUp`, `PageDown` and `a` work from the model alone |
| Reduced motion is on | every duration reads 0: no animation is scheduled; status changes show through label, icon and outline at once |
| The reduced-motion setting changes while the page is open | the next animation reads the new value (the token is read when an animation starts) |
| A burst of changes (more than five nodes entering or settling) | five animate, the rest appear in their final state at once; nothing is delayed |
| A reload or reconnect re-delivers the same data | `planTransitions(null, …)` animates nothing and remembers every node state |
| More than 150 nodes | layout changes are instant (`animateLayout` false); status settle still follows the five-node rule |
| A trace over 200 steps | the window of 6.3 applies, with "Show {n} earlier steps" above the graph |
| `ResizeObserver` is missing (jsdom) | the shell and engine read the element size once and on `window` resize; tests set it explicitly |
| The system colour scheme or `data-theme` changes | the engine re-resolves tokens and redraws without a layout |
| A theme token computes to a colour function the library rejects | normalise it in `palette.ts` and note which token in the report |
| The accepted library tries to fetch a font, image or other resource from another origin | it is blocked by the policy; the smoke run fails on the violation; remove the use, never the policy |
| A step to open with Enter is inside a folded group | Enter on the group opens it and moves focus to its first member; `a` reveals the group and focuses the step |
| `Escape` with no selection | nothing, not consumed (it may still close something outside the graph) |
| Any step of this brief cannot be done as written | stop and report the difference, as the execution constraints require |

## 11. Security requirements

- **The graph never decides.** No file under `src/components/graph/` or `src/features/operations/graph/` imports an
  approve, deny, resume, cancel, pause or answer hook, or the mutation hooks of `api/hooks/operationControls.ts`. Enter
  and double-click only select a step and open the inspector. A source-scan test proves it (12.9). Nothing in the graph
  can approve or run anything (INV-1).
- **Step text is untrusted.** `summary`, `title` and labels are drawn as text by every engine: no `innerHTML`, no
  `dangerouslySetInnerHTML`, no HTML label or node extension, no Markdown, no links built from model text. Test X1: a
  trace whose summaries are `<img src=x onerror="window.__x=1">`, `<script>window.__x=1</script>` and
  `"><svg onload=window.__x=1>` shows those strings literally; no element is created from them; `window.__x` stays
  undefined (smoke) and the live line contains them as plain text.
- **The policy is the contract and it is not touched.** No `unsafe-eval`, `blob:` or `wasm-unsafe-eval`, no inline script,
  no `eval` or `new Function` in our code, no remote font, image or stylesheet. The smoke test serves the **real** policy
  read from `csp.rs` and fails on any violation; a control proves enforcement. A worker, if the library uses one, is a
  same-origin hashed file (the accepted amendment's rule 4).
- **Inline style follows the accepted text.** The graph directory has no JSX `style` (lint); what the library writes at run
  time is checked against `GRAPH_STYLE_ALLOWANCE` in the smoke test. A write outside the allowance fails the run.
- **No new network path.** No `fetch`, no `XMLHttpRequest`, no `WebSocket`, no `sendBeacon` in the graph code or the
  library's use. The graph receives its data as props.
- **No secrets.** The trace has no output, intent or plan hash; the graph adds none. The URL carries only a step id,
  validated against the trace. Nothing is logged or stored.
- **Supply chain.** One new runtime dependency set, exact versions from the evidence, lockfile committed, installed with
  `npm ci`; its licence identifier is recorded in `DESIGN.md`; Dependabot watches `web/` already. If the accepted
  licence needed owner review, the acceptance already covers it: do not re-argue it, but copy the identifier.
- **Fail closed on state, never on safety text.** A failure of the graph degrades to the step list; the banner, the
  approval cards and the inspector are separate and unaffected (they are P0 and not inside the graph).
- **Audit** is the hub's; the UI adds none.

## 12. Testing requirements

Tests are named after behaviour. Web tests are vitest files next to the code. Every page-level test is also an axe check
(`setup.ts` runs axe over what a test leaves on screen). There is no Rust test, no `domovoi-testkit` fake and no
`insta` snapshot. The five fixtures are loaded by explicit path from `src/test/fixtures/operation-trace/`; the
synthetic trace helper `makeTrace(n, seed)` is a small **(new)** test utility in `src/test/syntheticTrace.ts`
(deterministic mulberry32; steps `s0..s{n-1}` cycling kinds `evidence_request, reasoning_turn, assessment, plan,
policy_decision, approval, execution, verification`; `cycle = floor(i / 40)`; each step except `s0` gets `depends_on` to
`s{i-1}` with probability 0.8, `caused_by` to a random one of the previous six with probability 0.5, every 11th
`execution` a `retry_of` to the previous execution; no `from` below its `to`; no duplicate `(from, to, kind)`; outcomes
`succeeded` except the last `running`, every 17th `failed`, every 23rd `pending`).

### 12.1 Pure logic (`src/components/graph/*.test.ts`, `src/design/motion.test.ts`)

| Test | Setup, action, expected |
|---|---|
| `parse_duration_reads_ms_and_seconds` | `"240ms"`, `"0.24s"`, `"0ms"`, `" 240ms "` give 240, 240, 0, 240; `""`, `"auto"`, `"-5ms"`, `"NaN"` give 0; `"900ms"` is clamped to 600 |
| `motion_ms_reads_the_token_now_and_is_zero_when_missing` | stub `getComputedStyle` to return `"320ms"` for `--motion-layout` then `"0ms"`: two calls give 320 then 0; an absent token gives 0 |
| `arrow_right_selects_the_nearest_dependent_in_a_layer_to_the_right` | hand-built nodes and edges: expected id per 6.4; ties go to the lowest ordinal |
| `arrow_left_selects_a_prerequisite_in_a_layer_to_the_left` | same |
| `arrow_up_and_down_stay_in_the_layer` | same; at the first and last node of a layer the result is `null` |
| `page_down_and_page_up_walk_every_node_in_time_order` | start at ordinal 0 and press `PageDown` n-1 times: every id once, then `null` |
| `home_and_end_select_the_first_and_last_ordinal` | group nodes count by their first member's ordinal |
| `a_selects_running_then_a_pending_approval_or_question_then_pending_then_the_last` | four small graphs, one per branch; a group node is never chosen |
| `an_arrow_with_nothing_focused_focuses_the_first_node` | `current: null` for each of the four arrows, `PageDown`, `PageUp`, `Home` |
| `a_key_that_moves_nothing_returns_null` | ArrowRight on a node with no dependent |
| `nav_never_throws_on_unknown_outcomes_and_kinds` | `outcome: "future"`, `stepKind: "future_kind"` through every key |
| `arrow_keys_reach_every_connected_node_of_each_fixture` | from `Home`, a breadth-first walk with the four arrows reaches every node connected to the first by an edge path, for the five fixtures laid out by a simple test layout (rank by longest path, rows in ordinal order); the unreachable ids are asserted to be exactly the edge-free ones |
| `planTransitions_animates_nothing_on_the_first_render` | `prev: null`: empty `animate`; `remembered` holds every `id:outcome` |
| `planTransitions_animates_at_most_five_newest_changed_first` | a burst of eight changed and four new nodes: the five animated are the five highest-ordinal **changed** nodes; the rest are not in `animate` but are in `remembered` |
| `planTransitions_skips_a_state_already_remembered` | the same node and outcome twice: second plan animates nothing |
| `planTransitions_replays_after_the_remember_key_changes` | `StepGraph` test of 12.2 covers the reset |
| `edge_style_gives_every_known_kind_a_distinct_pattern_and_unknown_kinds_a_solid_one` | the table of 6.7; no two known kinds share a pattern and weight pair |
| `live_line_text_matches_the_spec` | step with summary, without summary, selected, with 0 and with 1 and 2 prerequisites and dependents (singular and plural), group node, empty |
| `palette_reads_every_token_and_follows_the_theme` | jsdom with `getComputedStyle` stubbed per theme: `readPalette` returns the stubbed values for both themes; `usePalette` re-reads after `data-theme` changes |

### 12.2 Shell with a test engine (`src/components/graph/stepGraph.test.tsx`)

`vi.mock("./GraphEngine")` replaces the engine with a **test engine** `src/components/graph/testEngine.tsx` **(new, test
support only, excluded from the bundle by living beside tests and imported only in tests)** that renders each node as a
plain `<div data-node-id>` placed on a grid, answers `positions()`, calls `onLayout` once after mount with centres on a
left-to-right grid by ordinal rank, and records the props it received (`focusedId`, `selectedId`, `transitions`,
`animateLayout`).

| Test | Setup, action, expected |
|---|---|
| `graph_root_is_one_tab_stop` | render with the waiting-approval model; Tab from before the toolbar: the toolbar buttons, then the root; the next Tab leaves; the number of tabbable elements inside `[data-graph]` is exactly 1 |
| `graph_root_has_a_name_a_description_and_a_status_line` | `role="group"`, `aria-roledescription="graph"`, `aria-label="Step graph"`, `aria-describedby` points to the instructions text of 6.8, a `role="status"` line exists |
| `arrow_keys_move_focus_and_announce_without_selecting` | focus the root, press ArrowRight: `data-focused-node` changes, the live line names the new step, `onSelect` and `onOpen` were not called |
| `enter_opens_the_focused_step` | `onOpen` called once with its id; on a group node `onToggleGroup` is called instead |
| `focus_moves_to_the_first_member_when_a_focused_group_opens` | focus a group node, press Enter (`onToggleGroup` called), rerender with a model where the group's members replace it: `data-focused-node` equals the group's first member id and the live line names it |
| `double_click_on_a_node_opens_it_and_click_selects_it` | the test engine triggers `onPointerOpen` and `onPointerSelect`; the matching callbacks fire |
| `escape_clears_the_selection_and_is_not_consumed_without_one` | with `selectedId`: `onClear` called and the event default prevented; without: `onClear` not called and `event.defaultPrevented` false |
| `plus_minus_and_zero_zoom_and_fit` | the handle's `zoomBy(1.25)`, `zoomBy(0.8)` and `fit` were called |
| `keys_with_modifiers_pass_through` | `ctrl+ArrowRight`, `alt+Home` change nothing and are not prevented |
| `page_never_scrolls_under_a_handled_key` | every handled key calls `preventDefault` |
| `focus_follows_a_selection_made_elsewhere` | rerender with a different `selectedId` while the root is not focused: `data-focused-node` equals it and the handle's `reveal` was called with it |
| `selected_step_is_exposed_as_data_selected_step` | `data-selected-step` equals `selectedId`, empty without one |
| `data_graph_nodes_lists_the_model_nodes_in_order` | the attribute equals the ids of `model.nodes` |
| `toolbar_buttons_are_labelled_and_fold_appears_only_with_onFoldAll` | labels exactly as 6.8; "Fold technical steps" absent without the prop, calls it when pressed |
| `edge_key_lists_only_the_kinds_present_and_uses_no_inline_style` | five-kind fixture: five rows; a trace with two kinds: two; no element in the key has a `style` attribute |
| `engine_failure_shows_the_fallback_and_try_again_retries` | the test engine calls `onFailure`: message "The graph could not be drawn", body "The step list below has the same steps."; pressing "Try again" remounts the engine; after a second failure the text adds "If it keeps failing, reload the page." |
| `a_rejected_engine_import_is_retried_with_a_fresh_import` | mock the dynamic import to reject once then resolve: the fallback shows, "Try again" then renders the engine |
| `transitions_prop_follows_the_plan_and_resets_with_the_remember_key` | rerender with a changed outcome: `transitions` holds that node; rerender with the **same** model: empty; change `rememberKey`: the same change animates again |
| `animate_false_gives_no_transitions_and_no_layout_animation` | `transitions` empty, `animateLayout` false |
| `layout_animation_is_off_above_150_nodes` | `makeTrace(200, 1)` model: `animateLayout` false; `makeTrace(50, 1)`: true |
| `node_text_is_text_not_markup` | summaries from 11 (X1): the strings appear as text in the live line and the test engine's nodes; `document.querySelector("img[src='x']")` is null |
| `step_graph_has_no_axe_violations` | axe runs automatically; also with the failure fallback shown |

### 12.3 Graph model (`src/features/operations/graph/graphModel.test.ts`)

| Test | Setup, action, expected |
|---|---|
| `detailed_profile_draws_every_step_and_every_edge_of_each_fixture` | `describe.each` over the five fixtures: node count equals `steps.length`, edge count equals the number of distinct trace edges, no group node, every node has title and status label, `layoutKey` is stable across two builds |
| `status_only_change_keeps_the_layout_key` | change one step's outcome: `layoutKey` equal; `status.value` differs |
| `a_new_step_or_edge_changes_the_layout_key` | append a step; add an edge: each changes it |
| `standard_profile_folds_runs_of_technical_steps_into_one_group` | a trace with three consecutive `evidence_request` steps and one `reasoning_turn` that succeeded: one group node with 4 members, title "4 technical steps", summary lists the kinds in first-seen order with counts |
| `essential_profile_folds_like_standard` | same trace under `essential` |
| `problem_outcomes_are_never_folded` | an `evidence_request` with outcome `unavailable` between two succeeded ones: it stays a single node and splits the run into two groups |
| `a_group_never_holds_a_p0_or_p1_step` | over the five fixtures and `makeTrace(200, 1)` under Standard: every group member has category P2 |
| `a_run_is_split_where_the_cycle_changes` | two technical steps in cycle 0 followed by two in cycle 1: two groups |
| `edges_are_contracted_onto_group_nodes_without_duplicates_or_self_loops` | an edge between two members disappears; two edges from outside into one group collapse to one per kind; ids are `from>to:kind` |
| `contraction_never_creates_a_cycle` | `makeTrace(200, seed)` for seeds 1 to 20 under Standard and Essential: the contracted graph is acyclic (topological sort succeeds) and `foldedGroups` equals the number of group nodes |
| `an_open_group_shows_its_members_as_ordinary_nodes` | `expanded` holds the group id: no group node, members present, edges restored |
| `the_selected_step_opens_its_group` | `selected` inside a group: that group is open; others folded |
| `windowing_keeps_non_final_unknown_result_and_selected_steps` | `makeTrace(300, 1)`: 200 newest plus every pending, running and `outcome_unknown` step plus the selected one; `hiddenEarlier` is the rest; `showAll` shows all |
| `edges_to_windowed_out_steps_are_dropped` | no edge references a missing node |
| `unknown_kind_outcome_and_edge_kind_render_generically_and_never_throw` | mutate a fixture copy (`kind: "from_the_future"`, `outcome: "odd"`, edge `"links"`): neutral status with the raw value as label, title is the raw kind, edge label is the raw kind; `buildGraphModel` does not throw |
| `retry_marks_a_step_that_has_a_retry_of_edge` | `failed-verification-new-cycle`: the retry execution node has `retry: true` |
| `summary_null_gives_a_node_without_a_summary` | step with `summary: null` has `summary: null` |
| `a_trace_with_no_steps_builds_an_empty_model` | `nodes: []`, `edges: []` |

### 12.4 Profiles in the graph (`src/features/operations/graph/graphVisibility.test.tsx`; helpers of 37)

| Test | Setup, action, expected |
|---|---|
| `p0_steps_are_drawn_in_every_profile` | for each fixture and each of `detailed`, `standard`, `essential` (`renderWithProfile`): every step with `stepCategory(...) === "P0"` has its id in `data-graph-nodes` |
| `standard_profile_shows_a_folded_group_and_open_group_shows_members` | Standard on a trace with a technical run: `data-graph-groups` lists the group; pressing Enter on the group node (test engine) puts the members in `data-graph-nodes` and the group is gone; "Fold technical steps" folds it again |
| `detailed_profile_has_no_groups` | `data-graph-groups` empty for every fixture |
| `a_step_list_below_the_graph_always_lists_every_visible_step` | the outline under the graph has one row per step regardless of profile (the outline folds per 37; assert the P0 rows) |

### 12.5 Selection and the inspector (`src/features/operations/graph/operationGraph.test.tsx`; `renderAt` with the right slot provider)

| Test | Setup, action, expected |
|---|---|
| `renders_<fixture>_in_graph_view_without_violations` | `describe.each` over the five fixtures, with `stubMedia(true)` (wide): the Steps section opens in Graph; `[data-graph]` exists and `data-graph-nodes` has the step count (Detailed); the step list is also present; axe is clean |
| `narrow_screens_open_in_timeline_and_the_graph_option_is_still_offered` | `stubMedia(false)`: no `[data-graph]`; choosing "Graph" renders it |
| `view_switch_offers_timeline_tree_and_graph` | the `SegmentedControl label="View"` has the three options |
| `clicking_a_graph_node_selects_the_step_in_the_url_and_the_outline` | the test engine's `onPointerSelect(id)`: `?step=<id>`, the outline row has `aria-current="true"`, `data-selected-step` equals it, the inspector is open |
| `clicking_an_outline_row_selects_the_node_in_the_graph` | click a row link: `data-selected-step` and the engine's `selectedId` equal the id |
| `arrow_keys_do_not_change_the_url_or_open_the_inspector` | focus the root and press ArrowRight three times: `location.search` unchanged, the inspector closed |
| `enter_selects_opens_and_moves_focus_into_the_inspector` | Enter on a step node: `?step=<id>`, focus is inside the inspector; closing it with Escape returns focus to `[data-graph]` |
| `enter_on_the_already_selected_step_moves_focus_into_the_open_inspector` | with the inspector open and focus on the graph: Enter calls the slot's `focus("inspector")` and focus moves there |
| `escape_on_the_graph_with_a_selection_clears_it_and_closes_the_inspector` | `?step=` removed, inspector closed, focus still on the graph |
| `a_deep_link_to_a_step_in_a_folded_group_opens_the_group_and_selects_it` | Standard profile, `?step=<technical id>`: `data-graph-groups` no longer lists that group; `data-selected-step` set |
| `a_deep_link_does_not_take_focus` | initial render with `?step=`: `document.activeElement` is not inside the inspector |
| `an_invalid_step_param_is_dropped` | `?step=nope`: removed with `replace`; no selection |
| `show_earlier_steps_button_appears_for_a_long_trace_and_reveals_the_rest` | `makeTrace(300, 1)` as the trace |
| `empty_trace_shows_no_steps_yet` | a trace with no steps |
| `graph_code_has_no_decision_controls` | the rendered graph view has no "Approve" or "Deny" button inside `[data-graph-frame]` |

### 12.6 Slot focus (`src/app/rightSlot/*.test.tsx`, only if `focus(id)` was added)

`slot_focus_moves_focus_into_an_open_surface`, `slot_focus_is_a_no_op_for_a_closed_or_unregistered_surface`,
`opening_an_open_surface_still_does_not_steal_focus` (30's rule is unchanged).

### 12.7 Tokens and contrast

`src/styles/motion.test.ts` (extended): `graph_motion_tokens_exist_with_the_planned_values` (240, 400, 320),
`no_motion_token_exceeds_600ms`, `reduced_motion_zeroes_every_motion_token`.
`styles/contrast.test.ts` is unchanged and now covers the new pairs; `the_graph_pairs_are_required` asserts
`requiredPairs()` contains the three pairs of 6.7.

### 12.8 Demos (`src/dev/graphDemos.test.tsx`)

`/dev/design` gets a "Step graph" section: the five fixtures through a `<select>`-less list of buttons, the default and
the Standard-profile folded view, the failure fallback (a demo flag), the edge key; `graph_demos_render_without_violations`.
The demo mounts the real lazy engine only when the browser provides what the engine needs; in jsdom it uses the test engine
through the same `vi.mock`.

### 12.9 Guards (`src/guardrails.test.ts`, `src/components/graph/graphGuards.test.ts`)

| Test | Expected |
|---|---|
| `lint_rejects_an_inline_style_in_the_graph_directory` | `lintText` of `<div style={{}}/>` with `filePath: "src/components/graph/X.tsx"` fires `no-restricted-syntax`; a file `geometry.ts` (only if the amendment names it) does not |
| `lint_rejects_the_graph_library_outside_the_graph_directory` | `import … from "<library>"` at `src/features/x/X.tsx`, `src/app/X.tsx` and `src/components/Other.tsx` fires `no-restricted-imports`; at `src/components/graph/GraphEngine.tsx` it does not |
| `only_the_graph_directory_imports_the_library` | scan every `src/**` file's import specifiers: the accepted package names appear only under `src/components/graph/` |
| `the_engine_is_loaded_only_through_a_dynamic_import` | the only reference to `./GraphEngine` in non-test code is an `import("./GraphEngine")` expression in `StepGraph.tsx` |
| `graph_code_cannot_approve` | no file under `src/components/graph/` or `src/features/operations/graph/` imports `operationControls`, `useApprove`, `useApproveStep`, `useDenyStep`, `useCancelOperation`, `usePauseOperation`, `useResumeOperation` or `useAnswerStep`, or calls `.mutate(` |
| `no_graph_file_uses_html_injection` | no `dangerouslySetInnerHTML` or `innerHTML` under the graph directories (including the engine) |
| `no_literal_millisecond_durations_in_the_engine` | a scan of `GraphEngine.tsx` and its helpers finds no numeric duration passed to an animate call or CSS `ms` literal (durations come from `motionMs` and tokens) |

### 12.10 Build and browser

`npm run build` runs `size.mjs`: it must pass with the graph chunk present, lazy and self-contained (the new problems of
6.9 are exercised once by temporarily making `StepGraph` import the engine statically and confirming the build fails;
record in the completion report that you did, then revert). `npm run smoke` runs checks 1 to 16 of 30 and 6.10; run each
new check once against a deliberately broken input (for example remove the Escape handler, or serve the policy with
`script-src 'self'` plus an inline allowance) to see it fail; report that you did.

### 12.11 Existing suites

Every test of prompts 30 and 37 passes **unchanged**, except the edits the view switch and `useStepSelection` strictly
require (list each in the report). No snapshot, gate or guardrail is deleted or weakened.

## 13. Acceptance criteria

1. Step 1 verified the R-1 acceptance, the accepted inline-style text and the chosen library, and the report names
   them; nothing was built on a proposal.
2. `package.json` pins the accepted library (and layout engine) to the exact versions of the evidence; `npm ci` passes;
   no other dependency was added.
3. The graph library is imported only under `src/components/graph/`, only by the engine files, and the engine is
   reachable only through `import("./GraphEngine")` in `StepGraph.tsx` (guard tests pass).
4. `npm run build` produces a `graph-<hash>.js` chunk that is not in the first page's scripts, not statically imported
   by any initial chunk, imports at most `vendor`, is at most 120 kB gzip, and `size.mjs` prints its size; the budget
   rows in `DESIGN.md` carry the new measured numbers; CSS stays under 30 kB.
5. The three tokens exist with the values of 6.6 and `0ms` under reduced motion; `motionMs` reads them at animation time;
   no motion token exceeds 600 ms; the engine contains no literal duration.
6. `planTransitions` animates at most five nodes per update (changed before new, newest first), nothing on the first
   render, nothing for a remembered node state; layout animation is off above 150 nodes.
7. The Steps section offers Timeline, Tree and Graph; it opens in Graph on screens of 768 px and wider and in Timeline
   below; the step list stays under the graph; 30's tests pass unchanged.
8. The graph root is exactly one tab stop; every key of the table in 6.4 behaves as specified (pure tests and shell
   tests); arrow keys move focus only and never write `?step=`; Enter and double-click select, open the inspector and
   move focus into it; Escape follows the order of the design guide.
9. The selection is the URL's `?step=`: a graph click, an outline click and a deep link all agree, and the outline's
   `aria-current` and the graph's `data-selected-step` always match.
10. In Standard and Essential, runs of P2 steps fold into one group node per run (split by cycle), P0 steps are drawn
    in every profile (`p0_steps_are_drawn_in_every_profile`), the contracted graph is acyclic, a selected or deep-linked step
    opens its group, and Detailed has no groups.
11. A status-only change keeps `layoutKey` and the engine does not re-lay out; an unknown kind, outcome or edge kind
    renders generically without an error.
12. A graph that cannot load or draw shows "The graph could not be drawn" and a working "Try again"; the step list,
    banner, approval cards and inspector are unaffected.
13. Summaries are drawn as text in the shell and the engine (X1 passes, no HTML injection API is used).
14. The smoke test serves the real policy read from `csp.rs` (with a passing enforcement control), and checks 10 to 16
    pass: zero CSP violations, one tab stop, the keyboard flow, outline sync, reduced motion with no running animation
    after an outcome change, the inline-style audit within `GRAPH_STYLE_ALLOWANCE`, no overflow at 375 px.
15. The ESLint scope and library-import rule are in force (guard tests), the graph directory has no JSX `style`, and
    every source file is at most 250 lines.
16. The graph contains no decision control and imports no mutation hook (guard test).
17. `DESIGN.md` carries the accepted amendment applied as accepted, the new tokens, the kit component, the keyboard
    table, the Escape order, the budget numbers and the check rows; "Planned" no longer lists what now exists and still
    lists playback. `AGENTS.md`, `TESTING.md` and the book page are updated.
18. `npm run lint`, `npm run typecheck`, `npm test`, `npm run format:check`, `npm run build` and `npm run smoke` pass; `npm run gen:api` leaves
    `src/api` unchanged.
19. The completion report lists every command with its real result, every item not run, the library and versions, and the
    chunk size.

## 14. Migration and compatibility

- **Existing installs.** The UI is embedded in the hub binary and ships with it; nothing is migrated. Operations exist
  only behind `[orchestration] enabled` (off by default until prompt 58), so most installs see no change at all.
- **Default view.** On wide screens an operation page now opens in Graph rather than Timeline; the person can switch,
  and the choice is not remembered across loads. The step list is still on the page.
- **First download.** Unchanged: the graph is a separate lazy chunk, fetched when the Graph view is first shown (and
  prefetched by `OperationGraph` on mount).
- **Dependency.** One new runtime dependency set with exact pins; Dependabot will propose updates like any other.
- **Hub and agents.** No change; the hub's policy and API are untouched; N-1 is not a concern for the browser code.
- **Rollback.** Reverting the PR removes the graph, the tokens and the build group; no data depends on it.

## 15. Documentation requirements

- **`web/DESIGN.md`** (same PR; quote the text on your branch before editing):
  - apply the **accepted** amendment, items 1 to 7, as accepted: the library paragraph (library, exact versions, licence,
    imported only inside `src/components/graph/` and only through a dynamic import, chunk name `graph`), the inline-style
    paragraph, the bundle paragraph (group text, size check, the new "Performance budget" row with the measured numbers),
    the workers-and-policy rule, the motion tokens;
  - "Tokens": add `--motion-graph`, `--motion-graph-edge`, `--motion-layout` to the motion table; "Components": add
    `StepGraph` (props, the shell and engine split, `OperationGraph`, the keyboard table of 6.4, the live line, the edge
    key, the failure fallback, reduced motion, `motionMs`); "The shell" and "Planned": the Escape order unchanged, the
    graph selection still last;
  - "How people use it": a paragraph on the Graph view (default on wide screens, the list under it, focus is not
    selection, Enter opens, technical steps fold in Standard and Essential);
  - "Planned": remove what now exists (the `--motion-graph`, `--motion-graph-edge` and `--motion-layout` rows, the graph
    canvas in "New kit components", the "Lint and size amendments the graph will need" paragraph) and **keep** the
    playback row;
  - "Checks that will fail your PR": rows for the graph library imported outside the graph directory
    (`npm run lint`), inline style in the graph directory (`npm run lint`), the graph chunk missing, in the first
    download or statically imported (`npm run build`), a motion token above 600 ms or not zeroed under reduced motion
    (`npm test`), and the browser smoke checks (`npm run smoke`, including the real policy);
  - "Performance budget": the new numbers from `node scripts/size.mjs --report` (initial JS, largest chunk, graph chunk,
    CSS) and one sentence that the graph is its own lazy chunk.
- **`AGENTS.md`** (committed; keep in step): in "Web UI" add one bullet: the graph library is imported only inside
  `web/src/components/graph/`, only by the engine, through a dynamic import; graph motion durations come from `motionMs`.
- **`TESTING.md`** (web paragraph near the smoke test): the smoke test now serves the hub's real
  Content-Security-Policy (read from `crates/domovoi-hub/src/csp.rs`) and checks the graph; axe is injected with
  `page.evaluate` for that reason.
- **Book** `docs/book/src/concepts/operations.md` (30 added "The Operations page"): a section "The graph view" for users, no
  prompt numbers: what the graph shows, what the colours and line patterns mean, the keyboard table, how folded technical
  steps work, the step list under it. Link `DocsLink` targets only to pages that exist; run `cargo xtask docs-links`.
- **No** `ARCHITECTURE.md`, config reference, OpenAPI, ADR (the R-1 acceptance is the decision record) or hub change.

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

