# 33 — R-1 graph library spike

| | |
|---|---|
| Branch | `graph-spike` |
| Milestone / research item | U2 / R-1 (graph library under the CSP, the no-inline-style rule and the bundle budget) |
| Depends on (must be merged) | 30 and, through it, 26, 27, 28, 29, 59 (the trace schema, its five golden fixtures and the operation UI vocabularies must be in the checkout). Verify in step 1; if the fixtures are missing, stop and report |
| Can run in parallel with | 31, 32, 34, 35, 36, 37, 39, 40, 41, 42 (order-flexible, never concurrent). It edits nothing under `web/`, `crates/` or `xtask/`, so it conflicts with none of them |
| Produces (shared contracts) | R-1 evidence report `docs/rework/investigations/R-1-graph-library.md`; a `web/DESIGN.md` amendment **proposal** `docs/rework/investigations/R-1-design-md-amendment-proposal.md` (a proposal document, not an accepted change); the throwaway probe harness `docs/rework/investigations/probes/r1-graph/`. No product contract |
| Consumes | 26: the trace types `web/src/api/operationTrace.ts` and the golden fixtures `web/src/test/fixtures/operation-trace/*.json`. 30: the step outcome and kind vocabulary (words only; copied, not imported). Existing: `crates/domovoi-hub/src/csp.rs`, `web/vite.config.ts`, `web/scripts/size.mjs`, `web/eslint.config.js`, `web/src/styles/tokens.css`, `web/DESIGN.md` |
| Launch gate | none for this prompt. **Prompt 38 (`graph-canvas`) is gated on R-1 being accepted by the owner**; this prompt only supplies the evidence for that decision |
| Suggested model | medium: the work is mechanical and fully specified, the judgement is in measuring honestly and in not over-claiming |

## 1. Objective

Produce the evidence the owner needs to decide **research item R-1**: which graph approach, if any, can draw the
operation trace as an interactive DAG in the Domovoi web UI while respecting three hard constraints:

1. **The hub's Content-Security-Policy**, as the hub code actually sends it (`crates/domovoi-hub/src/csp.rs`).
2. **The no-inline-style rule** of `web/DESIGN.md`. Many graph libraries write `style` attributes; the spike must
   **measure** this, not assume it.
3. **The bundle budget** (`npm run size`): the graph must be its own lazy chunk inside the 120 kB (gzip) chunk limit
   and must add nothing to the first download.

You build **throwaway prototypes** of every candidate, in an isolated directory that no product tool sees, render the
five golden trace fixtures from prompt 26 (plus generated large traces) as a DAG with keyboard navigation and reduced
motion, measure each candidate with a Playwright harness against a server that sends **the hub's exact policy**,
and write two documents:

- an **evidence report** with a recommendation (`docs/rework/investigations/R-1-graph-library.md`);
- a **`web/DESIGN.md` amendment proposal** (`docs/rework/investigations/R-1-design-md-amendment-proposal.md`): exact
  before and after text for the owner to accept or reject. It is a proposal. `web/DESIGN.md` itself is **not edited**.

**This prompt does not accept R-1.** The owner accepts R-1 by recording it in `docs/rework/99-open-decisions.md`
section C. Your report says "evidence submitted; R-1 remains open". Prompt 38 stays blocked until then. You change no
product code on the main path: nothing under `web/`, `crates/` or `xtask/`, no `web/package.json`, no lockfile, no
config, no CI file.

## 2. Architectural context

Everything needed is here; the cited documents are background.

**D-08 / RW-21 (what the graph is for).** Operation detail has two views of **one frozen trace contract**
(`schema_version: 1`): the step timeline/tree (prompt 30, already built) and an interactive orchestration graph
(prompt 38, later). The graph is the primary visualisation on the page; the non-canvas outline stays as the
accessible representation (real DOM, the screen-reader path). Both render from the same JSON, so a library must take
a plain `{ steps, edges }` and nothing else.

**RW-40 (the shape of the data).** A trace has `steps[]` (each with `id`, `parent`, `kind`, `phase`, `cycle`,
`outcome`, `started_at`, `ended_at`, `summary`, `refs`) and `edges[]` (`{ from, to, kind }`). An edge points from the
**dependent** step to its **prerequisite**, and `from` always has the higher ordinal, so the graph is acyclic by
construction. Edge kinds: `depends_on`, `caused_by`, `verifies`, `retry_of`, `answers`. `parent` is grouping only (the
tree); the graph is drawn from `edges`. Investigation cycles are new steps with `cycle + 1`, never graph cycles. A
newer hub may send an unknown kind, outcome or edge kind: it must render generically, never throw.

**D-12 (motion).** Visualisation and layout transitions may use longer motion than micro-interactions: planned tokens
`--motion-graph` (240 ms node entry and status settle), `--motion-graph-edge` (up to 400 ms), `--motion-layout` (240
to 400 ms). A single transition never exceeds 600 ms. **Reduced motion is mandatory**: transitions are instant, state
changes stay visible without motion (label, icon, outline, text), and a JavaScript reader of the same tokens returns 0
so script-driven animation obeys it too. This applies to library-driven animation (a canvas `animate` call, a CSS
transition a library injects) exactly as to our own.

**D-25.** Historical playback is the last U2 deliverable (prompt 52) and is **not** evaluated here, except that the
report notes whether a candidate could drive state changes over time without a full re-layout.

**The three constraints, in plain words.**

- *CSP.* The hub sends one policy on every response, including the embedded UI. Scripts are `'self'` plus the SHA-256
  hash of each inline script of `index.html`; there is **no `unsafe-eval`, no `blob:` source and no
  `wasm-unsafe-eval`**; `default-src 'self'` makes workers, fonts, images (plus `data:`) and `connect` same-origin.
  Consequences to be measured, not assumed: a candidate that needs `eval`/`new Function`, a `blob:` worker or
  WebAssembly compilation **fails**; a layout worker must be a same-origin hashed file (`web/DESIGN.md`, "Lint and
  size amendments").
- *No inline style.* `web/DESIGN.md` says colours, spacing, motion and layers come only from tokens, and that "computed
  geometry goes through one kit helper that sets SVG attributes and CSS custom properties; there is no blanket
  exemption from the inline-style rule". Note two facts about today's enforcement (section 3): the lint rule bans JSX
  `style=` only under `src/features` and `src/app`, and the hub's policy currently allows inline styles
  (`style-src 'self' 'unsafe-inline'`). So the question the spike answers is not only "does it violate the CSP" but
  "what does the library write into the DOM, how much of it is per-node geometry, and would it survive a stricter
  `style-src 'self'`". The owner decides what to do with that answer; you report it.
- *Bundle budget.* Initial JS 170 kB gzip, any one chunk 120 kB gzip, all CSS 30 kB gzip (`web/DESIGN.md`,
  "Performance budget"). The graph is its own lazy chunk (imported only when operation detail shows it), so it must
  fit the chunk limit by itself and leave CSS headroom.

**Decisions made here (do not revisit).**

1. **Isolation.** The probes live in `docs/rework/investigations/probes/r1-graph/`, **outside `web/`**. Reason: CI
   classifies a path by its prefix; `web/*` turns on the web job (lint over `.`, `prettier --check .`, `vitest run`
   over every `*.test.*`, plus CodeQL and Dependabot), while `docs/rework/*` falls through to the catch-all case and
   turns on none of the code jobs (web, Rust, architecture, embedded UI). A directory inside `web/` would need edits to `eslint.config.js`, `.prettierignore` and the vitest
   config to stay green, and those are product files. Outside `web/` nothing needs changing. The directory has its
   own `package.json`, lockfile and `.gitignore`; `node_modules`, `out` and `dist` are never committed. It is never
   imported by product code and never shipped. It is an internal investigation record, so like all of
   `docs/rework/` it is never promoted to `main`.
2. **Candidates.** Exactly these eight **variants** are built and measured (section 4.2): a baseline, three
   hand-rolled SVG renderers that differ only by layout engine (own layered layout, `dagre`, `elkjs`; `elkjs` in two
   forms), React Flow with `dagre` layout, and Cytoscape in two forms. Other libraries get an "also considered" table
   without prototypes.
3. **Same inputs, same rules for all.** One fixture loader, one synthetic-trace generator, one pure keyboard model,
   one outline component, one stylesheet of tokens. A candidate differs only in how it lays out, draws and hosts
   focus. This keeps the comparison fair and keeps each prototype small.
4. **The policy is read, not copied.** The harness extracts the hub's `BASE_POLICY` from `csp.rs` at run time and
   fails if it cannot, so the evidence cannot drift from the code. A **negative control** page must provoke CSP
   violations under that policy; zero violations from the control invalidates the whole run.
5. **The CSP is not widened.** If a candidate only works with `unsafe-eval`, `blob:` or `wasm-unsafe-eval`, it
   fails the CSP gate and you say so. You do not propose a wider policy and you do not edit `csp.rs`.
6. **Evidence labels.** Every finding in the report is exactly one of **demonstrated** (observed in the named browser
   and OS, with the command or number), **not demonstrated**, or **needs follow-up** (say what and where, for example
   Firefox, Safari, a real screen reader, the real hub binary). Never write "R-1 is resolved".
7. **Thresholds are labelled.** The budget numbers 170, 120 and 30 kB are fixed by `web/DESIGN.md`. Every other
   threshold in this brief is a **spike convention**: exceeding it is flagged in the report, it is not a fail unless
   the brief says "gate".

## 3. Repository context

Verify each item in step 1. Marks: **(existing, path:line)**, **(new)**, **(introduced by NN — verify)**. Line numbers
for `web/` files are from before prompt 30 merged and may have moved; find the symbol, not the line.

### 3.1 The hub's policy (the thing to be measured against)

- `crates/domovoi-hub/src/csp.rs:11` **(existing)**: `pub const BASE_POLICY: &str = "..."` holds every directive
  except `script-src`:

```rust
pub const BASE_POLICY: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'";
```

- `csp.rs:110` `policy(hashes)` **(existing)** returns `{BASE_POLICY}; script-src 'self'` plus one `'sha256-…'` source per
  inline script; `csp.rs:122` `header_value()` builds it from the embedded `index.html` (today one inline script, the
  theme bootstrap). `csp.rs:14` `SCRIPTS_FROM_SELF_POLICY` is the same text with `script-src 'self'` and no hashes.
- `crates/domovoi-hub/src/middleware.rs:42` `security_headers()` **(existing)** sets, on every response, the policy,
  `X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin` and `Permissions-Policy: interest-cohort=()`.
- `crates/domovoi-hub/src/spa.rs:94` `file()` **(existing)** serves embedded files with the MIME type from the
  embedder and `Cache-Control: public, max-age=31536000, immutable` under `assets/`.
- `crates/domovoi-hub/tests/hub/csp.rs` **(existing)** tests hashing and the policy shape; it asserts the policy
  contains no `unsafe-eval` and no `script-src 'unsafe-inline'`.

What the policy means for a graph: inline `<script>` is blocked unless hashed; `eval`, `new Function`, `blob:` workers
and `WebAssembly.compile` are blocked (no `unsafe-eval`, no `blob:`, no `wasm-unsafe-eval`); workers must be
same-origin files; **inline styles and `<style>` elements are allowed today** (`'unsafe-inline'` in `style-src`).

### 3.2 The web build, lint and budget

- `web/vite.config.ts:22-44` **(existing)**: `build.rolldownOptions.output.codeSplitting.groups` has two groups.
  `vendor` (`:27-28`, priority 20) holds `react`, `react-dom`, `react-router`, `scheduler`, `@tanstack`,
  `openapi-fetch`. `markdown` (`:34-35`, priority 10) matches **every other `node_modules` package** except those
  names and `lucide-react`. **A newly added graph library, dynamically imported, would therefore land in the
  `markdown` chunk and be counted with the Markdown stack against the 120 kB chunk limit.** The product build needs a
  separate `graph` group ahead of that catch-all; prompt 38 adds it. Your probe build mirrors the `vendor` group and
  gives each library its own group so you measure it alone.
- `web/scripts/size.mjs` **(existing)**: gzips every file under `dist/assets` with Node's `gzipSync` defaults;
  `CHUNK_LIMIT = 120 * KB` (`:18`, `KB = 1000`), `CSS_LIMIT = 30 * KB` (`:19`); the initial budget is read from
  `<!-- budget:initial-js-gzip-kb 170 -->` in `web/DESIGN.md`; **initial JS** is the sum of the scripts and preloads
  `dist/index.html` names; `--report` prints the numbers and never fails. Use the **same method** (`gzipSync`
  defaults, 1000 bytes per kB) for every number in your report.
- `web/DESIGN.md`, "Performance budget" **(existing)**: initial JS 154.8 kB of 170, largest chunk 110.7 kB of 120,
  CSS 14.6 kB of 30 (numbers recorded before prompt 30; step 3 re-measures them).
- `web/eslint.config.js:47-60` **(existing)**: the rule `no-restricted-syntax` on `JSXAttribute[name.name='style']`
  applies to `src/features/**` and `src/app/**` only. The kit (`src/components/`) is not covered, and does use inline
  `style` today (`DataTable.tsx`, `Popover.tsx`, `Tooltip.tsx`, `ChatComposer.tsx` set widths, positions and heights).
  `eslint.config.js:42` caps every non-test `src` file at 250 lines. `web/DESIGN.md` says the graph will need "no
  blanket exemption" and "The 250-line file cap is not relaxed".
- `web/src/styles/cssGuard.test.ts:1-6` **(existing)** scans `web/src/**/*.css` only: raw colours are forbidden
  outside `tokens.css`. A library's own stylesheet imported from `node_modules` is not scanned, but DESIGN.md's rule is
  that no other stylesheet has a raw colour, so a library CSS that ships colours must be unused or overridden.
- `web/package.json` **(existing)** pins: `react ^19.3.1`, `react-dom ^19.3.1`, `vite ^8.3.5`, `@vitejs/plugin-react
  ^6.2.1`, `typescript ~5.9.3`, `vitest ^5.1.0`, `playwright ^1.55.1`, `axe-core ^4.10.2`, `eslint ^10.14.0`,
  `typescript-eslint ^8.72.1`, `prettier ^3.9.10`, `jsdom ^30.1.3`. Node 22 (`engines`), CI uses Node 22. The probe
  uses the same ranges so it behaves like the product toolchain.
- `web/scripts/screens.mjs` **(existing)**: a Playwright **library** script (`import { chromium } from "playwright"`)
  that launches an installed Chrome (`SCREENS_CHANNEL`, default `chrome`; empty means Playwright's own Chromium after
  `npx playwright install chromium`). Copy that launch pattern.
- `.github/workflows/ci.yml` **(existing)**: the `changes` job classifies changed paths (`web/*` sets `web` and
  `embedded_ui`; `crates/*` sets `rust`; a path under `docs/rework/` matches the final `*)` case and sets nothing).
  `.github/workflows/codeql.yml` and `.github/dependabot.yml` look at `web/**` and `/web` only, so the probe's
  dependencies are **not** tracked by Dependabot or scanned by CodeQL: they are throwaway, and the report says so.

### 3.3 Tokens and motion

- `web/src/styles/tokens.css` **(existing)**: `--motion-fast: 120ms`, `--motion-base: 180ms` (`:175-176`),
  `--ease-out: cubic-bezier(0.2, 0.8, 0.2, 1)` (`:177`), zeroed in the `@media (prefers-reduced-motion: reduce)` block
  (`:398-404`); status colours as `--tone-<tone>-bg`, `-fg`, `-border`, `-solid`, `-on-solid` for `danger`, `warning`,
  `success`, `pending`, `progress`, `info`, `neutral` (for example `--tone-success-bg: var(--palette-sage-100)` at
  `:230`); surfaces `--color-bg`, `--color-surface`, `--color-surface-raised`; `--color-border`,
  `--color-border-strong`, `--color-accent`, `--color-text`, `--color-text-muted`; spacing `--space-1`…`--space-8`;
  radii `--radius-sm`, `--radius-md`; `--font-sans`, `--font-mono`; `--text-xs`…`--text-2xl`. The dark theme is
  selected by `data-theme="dark"` on `<html>` (and by `prefers-color-scheme` when `data-theme` is unset). The tokens
  `--motion-graph`, `--motion-graph-edge`, `--motion-layout` **do not exist**; they are "Planned" in `web/DESIGN.md`
  (`## Planned`, motion table). Prompt 30 adds `--motion-panel` (introduced by 30 — verify).
- `web/src/styles/global.css` **(existing)**: imports `tokens.css`, sets the page font, background, focus ring and
  `::selection`. The probe stylesheet imports both by relative path and defines the three planned graph motion tokens
  **locally, in the probe only**, in a block that mirrors the real file's pattern (a value in `:root` and `0ms` in the
  reduced-motion block).
- `web/DESIGN.md` **(existing)** passages the proposal quotes: "Motion has two classes" (`:29-33`); "Planned"
  section (`:363`) with the motion table (`:370-388`); "New kit components" (`:412-418`, the graph canvas with an
  outline and keyboard navigation: arrows between connected nodes, Enter opens the inspector, Home and End go to the
  first and last node, a key jumps to the active node); "Lint and size amendments the graph will need" (`:420-424`);
  Escape order (`:405-411`, "then the graph selection" last); "Checks that will fail your PR" (`:464`); "Performance
  budget" (`:488-507`). Prompt 30 edits DESIGN.md; **quote the text on your branch**, not the lines above.

### 3.4 The trace the graph draws

- `web/src/api/operationTrace.ts` **(introduced by 26 — verify)**: hand-written types `OperationTrace`,
  `TraceOperation`, `TraceStep`, `TraceEdge`, `TraceRef`, with string unions plus `(string & {})` so an unknown value
  type-checks. 27 may have re-pointed it at generated types; use whatever is exported.
- `web/src/test/fixtures/operation-trace/{waiting-approval,verified-success,failed-verification-new-cycle,outcome-unknown-paused,question-and-budget-pause}.json`
  **(introduced by 26 — verify)**: five hand-built traces that together hold every step kind, outcome and edge kind. A
  step is `{ id, parent, kind, phase, cycle, outcome, started_at, ended_at, summary, refs }`; an edge is
  `{ from, to, kind }`. Fixtures are small (a handful to a dozen steps), which is why the spike also generates large
  traces (section 6.4). The fixtures are the **only** product files the probe reads, by relative path.
- Step kinds: `reasoning_turn`, `evidence_request`, `assessment`, `plan`, `policy_decision`, `approval`, `execution`,
  `verification`, `notification`, `question`, `note`. Outcomes: `pending`, `running`, `succeeded`, `failed`,
  `refused`, `not_run`, `unavailable`, `outcome_unknown`, `skipped`, `cancelled`. Edge kinds: `depends_on`,
  `caused_by`, `verifies`, `retry_of`, `answers`.
- Prompt 30 maps outcomes to tones in `web/src/design/operationVocab.ts` (introduced by 30 — verify): `pending`
  Not started (neutral); `running` Running (progress); `succeeded` Succeeded (success); `failed` Failed (danger);
  `refused` Refused (danger); `not_run` Never ran (neutral); `unavailable` Unavailable (warning); `outcome_unknown`
  Result unknown (warning); `skipped` Skipped (neutral); `cancelled` Cancelled (neutral). Kind labels: Model turn,
  Evidence, Assessment, Plan, Policy decision, Approval, Action, Check, Notification, Question, Note. **Copy these
  words into the probe** (`src/harness/vocab.ts`); do not import product vocab (it drags in icons and the kit and would
  pollute the size measurement). If the product file differs from this list, use the product's words and say so.
- Prompt 30's outline (`web/src/features/operations/StepOutline.tsx`, introduced by 30) marks the selected step with
  `aria-current="true"` and selects through the URL parameter `?step=<id>`. The probe copies the `aria-current`
  convention so a graph and the outline can later share one selection.

### 3.5 What does not exist

No graph, chart, animation or virtualisation library is in `web/package.json` (`docs/rework/01-repository-reality.md`),
no `web/spikes` directory, no `docs/rework/investigations/probes/r1-graph`, no `R-1-*` file in
`docs/rework/investigations/`. The directory holds `00-inventory.md`, `A-backend.md` … `H-wsl2-research-environment.md`
and possibly newer reports; never overwrite a file there.

## 4. Exact scope

### 4.1 Files (all **(new)**; every path is under `docs/rework/investigations/` and nothing else changes)

Deliverables:

| File | Content |
|---|---|
| `R-1-graph-library.md` | the evidence report (6.8) |
| `R-1-design-md-amendment-proposal.md` | the amendment proposal (6.9), marked "PROPOSAL, not accepted" at the top |

Probe harness under `probes/r1-graph/` (the committed part; `node_modules/`, `out/` and `dist/` are git-ignored):

| File | Purpose |
|---|---|
| `README.md` | what it is, that it is throwaway and never imported by product code, how to run each script, the Node and browser used |
| `package.json`, `package-lock.json`, `.gitignore` | own dependencies, exact versions (6.10); scripts `build:all`, `serve`, `measure`, `tables`, `licences`, `check:css`, `lint`, `typecheck`, `test` |
| `tsconfig.json`, `eslint.config.js`, `vite.config.ts` | `strict` like `web/tsconfig.json`; the two lint rules of 6.11; one build per variant |
| `index.html`, `control.html` | the harness page (one file; the variant is chosen at build time through `import.meta.env`) and the negative-control page |
| `scripts/csp.mjs`, `scripts/serve.mjs` | read the hub's policy from `csp.rs`; static server sending the hub's headers (6.6) |
| `scripts/build-all.mjs`, `scripts/size.mjs` | build every variant; gzip sizes by the `web/scripts/size.mjs` method |
| `scripts/measure.mjs`, `scripts/measure/*.mjs` | the Playwright checks (6.5, 6.6); results to `results/results.json` |
| `scripts/tables.mjs`, `scripts/licences.mjs`, `scripts/check-css.mjs` | markdown tables from results; licence and dependency count per variant; colour guard over probe CSS |
| `src/harness/` | `main.tsx`, `Outline.tsx`, `fixtures.ts`, `synthetic.ts`, `vocab.ts`, `theme.ts`, `motion.ts`, `keyboardModel.ts`, `spike.css`, `controls.tsx` |
| `src/layout/` | `types.ts`, `layered.ts`, `dagre.ts`, `elk.ts`, `elkWorker.ts` |
| `src/svg/`, `src/reactflow/`, `src/cytoscape/` | one renderer each (6.1) |
| `tests/*.test.ts` | vitest tests of 12.1 |
| `results/results.json`, `results/shots/*.png` | machine-readable evidence; screenshots (commit only those the report embeds, at most twelve) |

Every `.ts`/`.tsx` file in `probes/r1-graph/src` stays at or under 250 lines, like product web code, so a winning
prototype can be moved into the kit without a split.

### 4.2 The eight variants

| Id | Renderer | Layout engine | Notes |
|---|---|---|---|
| `baseline` | none (outline only) | none | the harness and React with no graph; the cost of a candidate is its build minus this |
| `svg-layered` | hand-rolled SVG | **own** layered layout (`src/layout/layered.ts`, no dependency) | the zero-dependency reference; the trace is a DAG whose ordinals already give a topological order |
| `svg-dagre` | hand-rolled SVG | `@dagrejs/dagre` (layout only) | the maintained fork; record what the registry shows for the old `dagre` name |
| `svg-elk` | hand-rolled SVG | `elkjs`, **main-thread bundled build** (`elkjs/lib/elk.bundled.js`) | |
| `svg-elk-worker` | hand-rolled SVG | `elkjs` API plus its worker file loaded as a **same-origin hashed file** (`new Worker(new URL(..., import.meta.url))`) | the DESIGN.md rule for workers; test it under the policy |
| `reactflow` | `@xyflow/react` (React Flow) | `@dagrejs/dagre` through the shared adapter | React Flow draws, it does not lay out |
| `cytoscape-core` | `cytoscape` (canvas) | its built-in `breadthfirst` layout | no extension |
| `cytoscape-dagre` | `cytoscape` (canvas) | `cytoscape-dagre` extension | record the size delta against `cytoscape-core` |

Install each package at its **latest stable release at run time with exact versions** (`npm install --save-exact`),
and record the version in the report. Do not take any version number from this brief.

### 4.3 What each variant must demonstrate

Every graph variant renders any of the five fixtures or a generated trace, as nodes with kind label, outcome label and
tone, truncated summary, and typed edges; supports **select** (click), **open** (Enter or double-click), **pan and
zoom** with mouse and keyboard, **fit to view**, the **keyboard contract** (6.3), the **motion behaviour** (6.5), and a
**light/dark theme switch** that restyles it from tokens alone. Edges differ by kind by dash pattern and a label
available to assistive technology, never by colour alone.

## 5. Explicit non-goals

| Not in this prompt | Where it belongs |
|---|---|
| Accepting R-1, editing `docs/rework/99-open-decisions.md`, `07-implementation-prompt-index.md` or any other planning document | the owner; the orchestrator updates the index |
| Any edit under `web/` (source, `package.json`, `package-lock.json`, `vite.config.ts`, `eslint.config.js`, `size.mjs`, `tokens.css`, `DESIGN.md`, tests), `crates/`, `xtask/`, `.github/`, `AGENTS.md`, `TESTING.md`, the book | the proposal lists what 38 would change; this prompt changes none of it |
| Applying the amendment to `web/DESIGN.md` | owner acceptance, then prompt 38 |
| Adding any dependency to the product, or choosing one by installing it in `web/` | 38, after R-1 is accepted |
| Building the real graph canvas, kit component, `/dev/design` demo, route, inspector wiring, visibility profiles | 38 (37 for profiles) |
| Historical playback | 52 |
| Changing the hub's Content-Security-Policy, proposing a wider one, or editing `csp.rs` | out of scope; a candidate that needs a wider policy fails the CSP gate |
| Changing the trace schema or the fixtures; reading product code other than the files listed in section 3 | 26 owns the trace |
| Testing the embedded UI in the real hub binary, or any cargo build | **needs follow-up**; the probe replicates the headers from `csp.rs`, it does not run the hub |
| Firefox, Safari, touch devices, real screen readers (NVDA, VoiceOver, Orca) | record as **not demonstrated**; Chromium through Playwright is the required browser, Firefox is optional (6.6) |
| Pixel-perfect design, final node anatomy, final keyboard model, final colours | 38 and the owner; the probe uses a minimal node and a minimal keyboard model so candidates can be compared |
| A decision on strict `style-src` for the hub | the owner; the report only says which candidates survive it |
| A legal opinion on any licence | the owner; the report states the licence identifier and flags what needs review |

## 6. Required interfaces and contracts

Marks: **fixed** (must match exactly, because the measurement scripts and the report depend on it) or
**convention** (follow repository style).

### 6.1 Harness page contract **fixed**

`index.html` loads `src/harness/main.tsx`, which reads the URL query and renders the **outline** (always) and one graph.
The variant is chosen at **build time** (`VITE_VARIANT`, one build per variant, 6.10), so each build contains only the
code of its own candidate; the renderer and layout engine are loaded with dynamic `import()` so they land in their own
chunks (this mirrors the product requirement that the graph is a lazy chunk).

| Query | Meaning | Default |
|---|---|---|
| `fixture` | one of the five fixture file names without `.json` | `waiting-approval` |
| `synthetic` | an integer 50, 200 or 500: use `makeTrace(n, 1)` instead of a fixture | absent |
| `theme` | `light` or `dark`: sets `document.documentElement.dataset.theme` | system |
| `anim` | SVG variants only: `css` (class transition on a `transform` driven by custom properties) or `raf` (script tween of the `transform` attribute). Other variants ignore it | `css` |

Real DOM controls in the harness (outside the graph; each a native `<button>` or `<select>` with a visible label):

| `data-testid` | Control | Effect |
|---|---|---|
| `fixture` | `<select>` | switch fixture |
| `theme` | button | toggle light and dark through `data-theme` |
| `advance` | button | advance the **selected** step's outcome `pending → running → succeeded` in harness state (a status change the graph must show) |
| `relayout` | button | toggle `compact` (spacing x 0.75), forcing a new layout (a layout transition the graph must show) |
| `fit` | button | fit the whole graph in view |

The graph root element carries these attributes (all candidates, all variants **fixed**; the measurement reads them):

```html
<div data-graph data-variant="svg-dagre"
     data-selected-step="<step id or empty>" data-opened-step="<step id or empty>"
     data-layout-ms="<number>" data-ready="true|false" aria-label="Step graph" ...>
```

`window.__spike` **fixed**: `{ variant: string, metrics: { layoutMs: number, firstRenderMs: number, domNodes: number } | null,
nodePositions(): { id: string, x: number, y: number }[] }` where `x, y` are the **client-space centre** of each node
(so Playwright can click a canvas node). `metrics` is set once the first layout has been drawn (two animation frames
after mount); `layoutMs` is `performance.now()` around the layout call including any await; `domNodes` is
`document.querySelectorAll("*").length` after first render.

The **outline** (`Outline.tsx`) is the accessible representation the graph complements: an `<ol aria-label="Steps">`
of `<li>` each holding one native `<button data-outline-step="<id>">` with the kind label, outcome label and summary;
the selected step's button has `aria-current="true"`. Clicking a button selects the step (the same state the graph
uses). It is identical in every variant.

Selection and opening are harness state: `selected` (one id or none) and `opened` (the last id the user opened with
Enter or double-click; shown in a visible `<output data-testid="opened">` as the stand-in for the inspector). A graph
renderer receives `{ trace, selected, onSelect, onOpen, layout: LayoutFn, compact }` and calls the callbacks; it does
not own the selection.

### 6.2 Layout adapter **fixed**

```ts
// src/layout/types.ts
export const NODE_W = 208, NODE_H = 56;          // multiples of 4, the design grid
export const RANK_SEP = 64, NODE_SEP = 24;       // px; mirror --space-8 and --space-5 (layout engines need numbers)
export interface LayoutNode { id: string; ordinal: number; width: number; height: number }
export interface LayoutEdge { id: string; from: string; to: string; kind: string }   // from = dependent, to = prerequisite (as in the trace)
export interface LayoutInput { nodes: LayoutNode[]; edges: LayoutEdge[]; compact: boolean }
export interface PlacedNode { id: string; x: number; y: number; width: number; height: number }   // top-left
export interface PlacedEdge { id: string; points: [number, number][] }                            // polyline, endpoints included
export interface Layout { nodes: PlacedNode[]; edges: PlacedEdge[]; width: number; height: number }
export type LayoutFn = (input: LayoutInput) => Layout | Promise<Layout>;
```

Direction: **left to right**; prerequisites are on the left, dependents on the right; arrows are drawn from the
prerequisite (`to`) to the dependent (`from`), so time flows right. `compact` multiplies `RANK_SEP` and `NODE_SEP` by
0.75. Every engine gets the same node sizes. A layout is **deterministic** (same input, same output) and places no two
nodes overlapping. Edges between non-adjacent ranks may route as a polyline; if an engine returns no route points,
draw a straight segment and record that. Cytoscape variants do their own layout inside the library; they still expose
the same `nodePositions()` and compute `layoutMs` the same way.

`layered.ts` (own layout) algorithm, so every executor builds the same thing: rank = longest path from any node with
no prerequisite (rank of `from` = 1 + max rank of its `to` nodes; steps with no edges get rank 0); within a rank order
by the mean position of already-placed neighbours (barycentre, two sweeps), ties by ordinal; `x = rank * (NODE_W +
RANK_SEP)`, `y = index * (NODE_H + NODE_SEP)`; each edge is a three-point polyline (right-middle of the prerequisite,
mid-gap, left-middle of the dependent). No crossing minimisation beyond the two barycentre sweeps: say so in the
report. It is a baseline, not a product.

### 6.3 Keyboard contract **fixed** (spike convention; prompt 38 owns the final model)

The pure function in `src/harness/keyboardModel.ts` defines the semantics once; every renderer calls it with the
positions its own engine produced.

```ts
export interface NavNode { id: string; x: number; y: number; ordinal: number; outcome: string }   // x, y = centre
export interface NavEdge { from: string; to: string }                                              // from = dependent, to = prerequisite
export type NavKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End" | "a";
/** The id to select next, or null when the key moves nothing. `current` null means nothing is selected. */
export function nextNode(input: { nodes: NavNode[]; edges: NavEdge[]; current: string | null; key: NavKey }): string | null;
```

Rules (a **layer** is a set of nodes whose centre `x` differ by at most 8 px):

- `ArrowRight`: among the **dependents** of `current` (nodes with an edge whose `to` is `current`) that lie in a layer
  strictly to the right, the one with the smallest vertical distance, ties by the lowest ordinal; none: `null`.
- `ArrowLeft`: the same over **prerequisites** (`current`'s own `to` ends) in a layer strictly to the left.
- `ArrowDown` / `ArrowUp`: the nearest node in the same layer below or above `current`; none: `null`.
- `Home` / `End`: the node with the lowest / highest ordinal.
- `a`: "the active step": the lowest-ordinal node whose outcome is `running`; else the lowest-ordinal `pending`; else the
  highest ordinal.
- With nothing selected, an arrow key selects the lowest-ordinal node; `Home`, `End` and `a` as above.
- `Enter` opens the selected node (`onOpen`); `Escape`, with focus inside the graph, clears the selection and does
  nothing else (it must not close anything outside the graph).
- `+`, `-` and `0` zoom in, zoom out and fit. Arrow keys never pan.

The checks (each runs on `waiting-approval` and `failed-verification-new-cycle`; results go into the report as
pass, fail, or "pass with wrapper (N lines)"):

| Id | Check | Oracle |
|---|---|---|
| K1 | **One tab stop**: pressing Tab from the page start enters the graph once, and the next Tab leaves it; no per-node tab stops | count the elements inside `[data-graph]` that are reachable by Tab (cycle Tab until focus leaves); must be exactly 1 |
| K2 | ArrowRight from a node with a dependent moves to a dependent | `data-selected-step` |
| K3 | ArrowLeft from a node with a prerequisite moves to a prerequisite | `data-selected-step` |
| K4 | ArrowUp / ArrowDown move within a layer | `data-selected-step` |
| K5 | Home and End select the first and last step by ordinal | `data-selected-step` |
| K6 | `a` selects the running step, else the first pending, else the last | `data-selected-step` |
| K7 | Enter opens the selected step | `[data-testid=opened]` text and `data-opened-step` |
| K8 | Escape clears the selection and leaves `document.activeElement` inside the graph | `data-selected-step` empty |
| K9 | Reachability (reported, not a gate): starting at Home and using only arrow keys, every node of each fixture is reached; list the unreachable ids | walk breadth first |
| K10 | Selection is shared with the outline: choosing a node by keyboard sets `aria-current="true"` on its outline button, and clicking an outline button sets `data-selected-step` | both attributes |
| K11 | Accessible name: the focused or active element announces `"<kind label>, <outcome label>: <summary>"` (or the equivalent accessible name) and the graph region has a label | `locator.ariaSnapshot()` of the graph root, stored in the report appendix |

For a canvas variant (Cytoscape) the root is the single tab stop and the selected node is drawn highlighted; the
outline remains the screen-reader path. For a DOM variant use a roving tabindex or `aria-activedescendant`. In both
cases state in the report **which** you used, what the library did by default (for example how many tab stops it
creates per node, whether it moves nodes with the arrow keys on its own, which `aria-*` it sets) and what you had to
override to meet K1 to K8. Library behaviours are findings: record them as observed, do not predict.

### 6.4 Large traces **fixed**

`makeTrace(n, seed)` in `src/harness/synthetic.ts` returns an `OperationTrace`-shaped object (`schema_version: 1`) and
is deterministic (use mulberry32 with the seed). Steps `s0`…`s{n-1}` in ordinal order, `parent: null`, kinds cycling
through `evidence_request, reasoning_turn, assessment, plan, policy_decision, approval, execution, verification`,
`cycle = floor(i / 40)`. Edges: every step except `s0` gets `depends_on` to `s{i-1}` with probability 0.8 (so some
steps are disconnected, as `note` and `notification` steps are in real traces); with probability 0.5 a `caused_by` to a
random step among the previous six; every 11th step that is an `execution` gets `retry_of` to the previous
`execution`. No `from` has an ordinal lower than its `to`; no duplicate `(from, to, kind)`. Outcomes: `succeeded`
except the last step `running`, every 17th `failed`, every 23rd `pending`. Sizes 50, 200 and 500. The product plans to
show at most about 200 steps before windowing (prompt 30); 500 is a stress figure, not a requirement.

### 6.5 Motion and reduced motion **fixed**

`src/harness/spike.css` imports `../../../../../../../web/src/styles/global.css` (which imports `tokens.css`).
`src/harness/` is seven directories below the repository root and `scripts/` is six (count them from
`docs/rework/investigations/probes/r1-graph/`; verify the depth by opening the file, and use the same depth for the
fixture glob and for `csp.rs`). It defines, **in the probe only**:

```css
:root { --motion-graph: 240ms; --motion-graph-edge: 400ms; --motion-layout: 320ms; }
@media (prefers-reduced-motion: reduce) { :root { --motion-graph: 0ms; --motion-graph-edge: 0ms; --motion-layout: 0ms; } }
```

`src/harness/motion.ts` exports `motionMs(name: "graph" | "graph-edge" | "layout"): number`, which reads the computed
custom property from `document.documentElement` and returns milliseconds (it parses `ms` and `s`). It does **not**
call `matchMedia`: reduced motion works because the token is zero, which is the mechanism the design guide plans. Every
script-driven animation (a canvas `animate` duration, a requestAnimationFrame tween) must take its duration from
`motionMs`. CSS transitions and animations use the tokens directly.

Required motion in every graph variant: (a) a **status settle**: when a node's outcome changes (the `advance`
control), its fill and outline transition over `--motion-graph`; (b) a **layout transition**: when `compact` toggles,
nodes move to their new positions over `--motion-layout`, and `fit` animates over `--motion-layout`. No other motion.

| Id | Check | Method |
|---|---|---|
| R1 | Token reaches script | with `emulateMedia({ reducedMotion: "reduce" })`, `motionMs("graph") === 0`; with `"no-preference"`, it is 240 |
| R2 | Status change is **instant** under reduced motion | after `advance`, `document.getAnimations().length === 0` within 50 ms (Cytoscape: `cy.animated() === false`); under `no-preference` the same check finds at least one animation or a transition of 240 ms (so R2 is not vacuous) |
| R3 | Status change stays **visible** under reduced motion | the node's accessible name or label and the outline button show the new outcome label within one frame |
| R4 | Layout transition and fit are instant under reduced motion | after `relayout` and `fit`, positions from `nodePositions()` equal the final layout at the next frame |
| R5 | Longest single transition is at most 600 ms under `no-preference` | the longest animation duration observed (from `getAnimations()` timing, or the `animate` duration passed) |

Record, as notes only, whether the library could batch an event burst so that at most five nodes animate at once and
the rest appear in their final state (design guide rule), and whether it could change one node's state without
re-laying out the graph (playback, D-25).

### 6.6 The policy server and the measurements **fixed**

`scripts/csp.mjs` reads `crates/domovoi-hub/src/csp.rs` (path relative to the repository root, found from the script
location), extracts the string literal assigned to `pub const BASE_POLICY: &str =` with a regular expression, and
exports two header values:

- `real` = `${BASE_POLICY}; script-src 'self'` (exactly what `policy(&[])` builds; the probe pages have no inline
  script, so no hash is needed);
- `strict-style` = `real` with `'unsafe-inline'` removed from `style-src` (so `style-src 'self'`): a **hypothetical
  future tightening**, measured only to show which candidates would survive it. It is not a proposal.

If the regular expression finds nothing or finds more than one literal, the script throws. A vitest test
(`csp_header_is_read_from_the_hub_source`) asserts the extracted value starts with `default-src 'self'`, contains
`style-src 'self' 'unsafe-inline'`, and contains neither `unsafe-eval` nor `blob:`.

`scripts/serve.mjs` is a Node `http` server bound to `127.0.0.1` on an ephemeral port that serves `out/<variant>/` at
`/<policy>/<variant>/…` (`policy` is `real` or `strict-style`). It adds the policy as `Content-Security-Policy`,
`X-Content-Type-Options: nosniff`, `Referrer-Policy: same-origin`, a correct `Content-Type` for `.js` (`text/javascript`),
`.css`, `.html`, `.json`, `.wasm` (`application/wasm`) and `.png`, and `Cache-Control: no-cache`. It serves nothing
outside `out/`. The Vite config uses `base: "./"` so assets resolve below the variant path.

`scripts/measure.mjs` launches Chrome through Playwright the way `web/scripts/screens.mjs` does (`SPIKE_CHANNEL`,
default `chrome`; empty means Playwright's Chromium). It **never** sets `bypassCSP`. Every page gets an init script
that records `securitypolicyviolation` events (`violatedDirective`, `blockedURI`, `effectiveDirective`,
`sourceFile`), and the run also records console messages of type `error` and `warning`. It then does, per variant that
has a graph, per fixture (five), per policy (two) and, for the SVG variants, per `anim` value (two):

1. load the page, wait for `window.__spike.metrics`, record violations and console errors;
2. run the **interaction script**: click a node, press ArrowRight, ArrowLeft, ArrowDown, ArrowUp, Home, End, `a`,
   Enter, Escape; zoom with `+` and `-` and the mouse wheel; drag-pan; `fit`; `advance`; `relayout`; toggle the
   theme twice; resize the viewport to 375 x 812 and back; record violations again;
3. the **DOM audit** at the end and after each interaction group: the count of elements with a `style` attribute inside
   `[data-graph]`, the CSS property names found in those attributes (parse `getAttribute("style")`), how many of those
   elements carry only custom properties (`--…`), the number of `<style>` elements in the document that were not in the
   initial HTML, the number of rules inserted into stylesheets at run time (wrap `CSSStyleSheet.prototype.insertRule` in
   the init script), and the number of `style` attribute mutations during the interaction script (a
   `MutationObserver` on `[data-graph]` with `attributes: true, attributeFilter: ["style"], subtree: true`);
4. run **axe-core** over the page: read `node_modules/axe-core/axe.min.js` in Node and run its text with
   `page.evaluate` (evaluation through the browser's debugging protocol is not subject to the page's policy, whereas
   `page.addScriptTag` creates a script element and may itself be blocked by `script-src 'self'`; check which you
   observe and record it), then call `axe.run` on the page and record violations with impact `serious` or `critical`;
5. take light and dark screenshots at 1280 x 800 and 375 x 812 of `waiting-approval` and
   `failed-verification-new-cycle`, to `results/shots/`;
6. run K1 to K11 (6.3), R1 to R5 (6.5) and X1 (section 11);
7. run the **stress timing**: `synthetic` 50, 200 and 500, six loads each, discard the first, record the median of
   `layoutMs` and `firstRenderMs` and `domNodes`, and `performance.memory.usedJSHeapSize` (Chromium only, labelled
   indicative);
8. optional, if Firefox is installed for Playwright: repeat steps 1 and 2 for `real` only and record violations; label
   everything else **not demonstrated**.

The **negative control** (`control.html`, built once, served under both policies) runs these attempts and stores the
outcomes in `window.__control`: an inline `<script>` block (written in the HTML), `eval("1")`, `new Function("return 1")()`,
`new Worker(URL.createObjectURL(new Blob(["1"])))`, `WebAssembly.compile` of the eight bytes `00 61 73 6d 01 00 00 00`,
creating and appending a `<style>` element, `element.setAttribute("style", "color: red")`, `element.style.color = "red"`
(CSSOM) and `sheet.insertRule` on an existing sheet. **Control gate**: under `real` the harness must record at least
one violation for each of inline script, eval-or-Function and `blob:` worker; under `strict-style` it must also record a
style violation (`style-src-elem` or `style-src-attr`). If not, **the harness is invalid**: stop, fix it, and do not
write the report from its results. Whatever the control records for the other rows (CSSOM assignment, `insertRule`,
WebAssembly) is a finding you report; do not assume them.

`results/results.json` **fixed** shape (one file, written by `measure.mjs`, `size.mjs` and `licences.mjs` merging their
parts):

```json
{
  "env": { "date": "", "os": "", "node": "", "chrome": "", "playwright": "", "repo_commit": "", "csp": { "real": "", "strict-style": "" } },
  "control": { "real": [], "strict-style": [] },
  "size": { "<variant>": { "js_gz": 0, "css_gz": 0, "worker_gz": 0, "files": [{ "file": "", "gz": 0 }], "delta_js_gz": 0 } },
  "runs": [ { "variant": "", "fixture": "", "policy": "", "violations": [], "console": [],
              "dom": { "style_elements": 0, "style_attrs": 0, "style_props": {}, "custom_prop_only": 0, "inserted_rules": 0, "style_mutations": 0 },
              "axe": [] } ],
  "keyboard": { "<variant>": { "K1": { "pass": true, "detail": "" } } },
  "motion": { "<variant>": { "R1": { "pass": true, "detail": "" } } },
  "safety": { "<variant>": { "X1": { "pass": true, "detail": "" } } },
  "perf": { "<variant>": { "50": { "layout_ms": 0, "render_ms": 0, "dom_nodes": 0, "heap": 0 } } },
  "licences": { "<variant>": { "packages": [{ "name": "", "version": "", "licence": "" }], "runtime_deps": 0 } },
  "maintenance": { "<package>": { "latest": "", "released": "", "weekly_downloads": 0, "observed": "" } }
}
```

### 6.7 Gates and the recommendation rule **fixed**

| Gate | Statement | Kind |
|---|---|---|
| G1 CSP | zero `securitypolicyviolation` events and zero CSP console errors under `real`, over every fixture and the whole interaction script; the `svg-elk-worker` worker must actually produce a layout under `real` | **gate** |
| G2 Size | the variant's graph JavaScript (each emitted chunk, and the worker file) is at most 120 kB gzip **per chunk**; none of it is in the initial scripts of the baseline page (all behind `import()`); baseline CSS from step 3 plus the variant's CSS is at most 30 kB gzip | **gate** |
| G3 Own style | the variant's own source has zero JSX `style=` (lint) and its CSS has no raw colour (`check:css`) | **gate** |
| G3b Library style | classify each variant: **S0** no `style` attribute written in the graph; **S1** only custom properties; **S2** the library writes layout properties (position, size, transform) as inline style; **S3** violations or a broken graph under `strict-style`. The classification is reported, never a gate | report |
| G4 Keyboard | K1 to K8, K10 and K11 pass (a wrapper is allowed; say how many lines it took) | **gate** |
| G5 Motion | R1 to R5 pass | **gate** |
| G6 Licence | each package that ends up in the graph chunk has an SPDX identifier. Permissive (MIT, BSD-2-Clause, BSD-3-Clause, ISC, Apache-2.0, 0BSD) is listed as clear; anything else (EPL, LGPL, GPL, MPL, unknown) is **flagged for owner review**, not failed. Do not give legal advice | flag |
| G7 Maintenance | latest version, its date, weekly downloads and open-issue count from the registry and the repository page, **as observed on the run date**; flag a last release older than 12 months | flag |
| G8 Accessibility | zero axe `serious` or `critical` violations on the variant page; K9 listed | **gate** (axe), report (K9) |
| G9 Performance | median `layoutMs + firstRenderMs` at 50, 200 and 500 steps; flag above 300, 1000 and 3000 ms | flag |

Recommendation rule (write it into the report as you apply it):

1. Remove every variant that fails a gate, and say which gate for each.
2. Rank the rest by, in this order: lower style class (S0 before S1 before S2), fewer runtime packages in the graph
   chunk, smaller gzip cost over `baseline`, fewer lines of integration code to pass K1 to K11 and R1 to R5, no licence
   flag, no maintenance flag. Say which criterion separated each pair.
3. Name **one recommended variant**, **one runner-up**, and "what would change this". If no variant passes every
   gate, the recommendation is "none yet", with what the closest variant lacks. Never soften a gate to reach a
   recommendation.
4. A combination (for example React Flow drawing with the own layered layout) may be recommended **only if that
   combination was built and measured**; otherwise list it as an untested idea.

### 6.8 The evidence report **fixed** (structure)

`docs/rework/investigations/R-1-graph-library.md`, in this order. Short sentences; tables where numbers are compared.

1. **Status** (first lines): "Evidence for research item R-1. R-1 is **not accepted**: the owner accepts it in
   `99-open-decisions.md` section C. Prompt 38 stays blocked until then."
2. **Summary and recommendation**: at most 15 lines: the recommended variant, the runner-up, the one-line reason, the
   decisions requested from the owner, and the top three risks.
3. **Environment**: date, repository commit, OS, Node, npm, Chrome version (`browser.version()`), Playwright version,
   every candidate package and exact version, and the two policy strings **verbatim** as served.
4. **Method in one page**: the variants, the harness contract, the control and its result (table of what each
   attempt did under each policy), what was not tested.
5. **Results**, one table per topic, one row per variant: size (js, css, worker, delta over baseline, chunk count,
   whether lazy); CSP (violations under `real`, under `strict-style`); inline-style audit (G3b class, style
   attributes, properties, mutations, `<style>` elements, inserted rules); keyboard (K1 to K11); motion (R1 to R5);
   accessibility (axe counts, and the ARIA snapshot of the graph region in an appendix); performance (50, 200, 500);
   licences and runtime package counts; maintenance observations.
6. **Candidate notes**: one short paragraph per variant: what was easy, what was awkward, lines of integration code (count
   non-blank, non-comment lines of the renderer, adapter and keyboard glue with a script and say which files), library
   defaults found (tab stops, built-in keyboard handling, `aria-*`, injected styles and stylesheets), anything surprising.
7. **Also considered, not prototyped**: a table (name, licence per the registry as observed, last release as observed,
   why not tried). Candidates to look up: `d3-dag`, `reaflow`, `vis-network`, `sigma` with `graphology`, `mermaid`,
   `jointjs`, `gojs`, `yfiles`, and the unscoped `dagre` package. Spend at most a few minutes on each; read the
   registry page and the licence file; **do not copy claims from memory into the table**, only what you observed, and
   write "not observed" where you did not.
8. **Recommendation** (the rule of 6.7 applied step by step), runner-up, what would change it, and risks.
9. **Findings by label**: three lists, **demonstrated**, **not demonstrated**, **needs follow-up** (each item says what and
   where: real hub binary with the embedded UI, Firefox, Safari, touch, a screen reader, a very large trace).
10. **Decisions requested from the owner**: each as a question with the options the evidence supports. At least:
    whether a library that writes inline `style` (class S2) is acceptable under the rule as worded; whether any
    licence flagged for review is acceptable; whether to keep `style-src 'unsafe-inline'` unchanged (the spike does not
    ask to change it); whether the graph chunk gets its own build group and size check.
11. **Consequences for prompts 38 and 52**: the files 38 would add or change (a `graph` build group, a graph chunk size
    check, an ESLint scope for the graph directory, tokens, kit component, `/dev/design` demo, DESIGN.md text) and what
    its brief must state (library, version, loading, keyboard model, motion tokens).
12. **Reproduce**: the exact commands, in order.
13. **Appendix**: ARIA snapshots, the control table, the results file location, the screenshots embedded (relative links).

### 6.9 The amendment proposal **fixed** (structure)

`docs/rework/investigations/R-1-design-md-amendment-proposal.md`. First line after the title: "**PROPOSAL, not accepted.
`web/DESIGN.md` is unchanged by this change. Written from the evidence in `R-1-graph-library.md`.**" Every proposed
change is an exact **Before** (quoted from `web/DESIGN.md` on your branch, with the heading it sits under) and **After**
(the text as it would be committed, in the guide's voice: plain, short, each rule with its reason). If the evidence
supports no recommended variant, say so and propose only the changes that hold regardless (items 3, 4 and 5). Sections:

1. **The graph library**: a new "Graph" paragraph for the Components section or a new subsection: the chosen library and
   layout engine with exact versions, its licence, that it is imported only from the graph directory of the kit and only
   through a dynamic import, the chunk name, and that adding or replacing it follows the dependency rules.
2. **Inline style, written precisely**: replace the sentence "Computed geometry goes through one kit helper that sets
   SVG attributes and CSS custom properties; there is no blanket exemption from the inline-style rule." with text that
   states (a) the rule governs our source, (b) the exact ESLint scope to add so the rule covers the graph directory
   (today it covers only `src/features` and `src/app`), (c) the one sanctioned helper and the only calls it may make,
   (d) what the chosen library writes into the DOM (the G3b class) and whether that is accepted, and (e) the strict
   `style-src` result as information. Include the ESLint config diff as text.
3. **Bundle**: the graph chunk rule: its own group in `vite.config.ts` ahead of the catch-all `markdown` group (give the
   exact group text), a size check in `scripts/size.mjs` for it, the measured gzip number and the headroom it leaves
   against 120 kB, the CSS headroom against 30 kB, and the "Performance budget" table row to add.
4. **Workers and the policy**: the rule "a layout worker, if used, is a same-origin hashed file (no `blob:`)" confirmed
   or amended by the `svg-elk-worker` result; one sentence that the hub's policy is unchanged and that nothing in the
   graph needs it widened (or, if a candidate did, that it was rejected for that reason).
5. **Motion tokens**: the exact lines to add to `tokens.css` (values, and `0ms` in the reduced-motion block), the
   rows of the "Planned" motion table that become real, the JS reader (`motionMs`) contract, and whether the chosen
   library's own animation obeys it (R1 to R5).
6. **Keyboard model**: the table of keys as proven (K1 to K11 plus what the library did by default), the accessible
   structure (graph region label, outline as the screen-reader path, `aria-current` shared with the outline), and the
   Escape order (the graph selection is last, as the guide already says).
7. **What moves out of "Planned"** when 38 merges, and what stays (playback).
8. **Files prompt 38 would change** as a list (no diffs applied): `web/package.json`, lockfile, `vite.config.ts`,
   `eslint.config.js`, `scripts/size.mjs`, `tokens.css`, `motion.test.ts`, the kit, `/dev/design`, `DESIGN.md`.

### 6.10 Probe dependencies **convention**

`dependencies`: `react`, `react-dom` (same range as `web/package.json`). `devDependencies`: `vite`, `@vitejs/plugin-react`,
`typescript`, `vitest`, `playwright`, `axe-core`, `eslint`, `@eslint/js`, `typescript-eslint`, `globals`,
`eslint-plugin-react-hooks`, `@types/react`, `@types/react-dom`, `@types/node` (all same ranges as `web/package.json`).
Candidates (exact versions, `--save-exact`): `@dagrejs/dagre`, `elkjs`, `@xyflow/react`, `cytoscape`, `cytoscape-dagre`,
and `@types/cytoscape` only if the package does not ship types. Install with `npm ci --ignore-scripts` from the lockfile
after the first `npm install --ignore-scripts`; if a package truly needs an install script, record which and why in the
README. No other dependency without a line in the README saying why. This brief names the probe's dependencies, so the Rust
`[workspace.dependencies]` rule of the standard blocks does not apply to the probe, and no Rust dependency is added.
The vite config mirrors the `vendor` group of
`web/vite.config.ts` and adds one group per candidate library (`dagre`, `elk`, `xyflow`, `cytoscape`) so each is
measured alone; the report says the product build will need the same separation.

### 6.11 Probe lint rules **fixed**

`eslint.config.js` over `src/**/*.{ts,tsx}`: the rule `no-restricted-syntax` with selector
`JSXAttribute[name.name='style']` (the product rule's selector and message, applied to **all** probe source including
renderers, which is stricter than the product's `src/features` and `src/app` scope), `max-lines` 250 (skip blank lines and
comments), and the typescript-eslint strict preset. `npm run lint` must pass with no disable comments except in
`src/harness/geometry.ts`, the **one** file allowed to call `element.style.setProperty` or set a `style` attribute: it
exports the helper the DOM-based variants use for any computed value, and the report counts how many times each variant
had to call it. SVG geometry uses attributes (`x`, `y`, `transform`, `viewBox`), never style.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies before building anything.**
   - Open every file named in section 3. Confirm `csp.rs` `BASE_POLICY` is still the string quoted there (copy the
     current value into your notes), that `web/vite.config.ts` still has the `vendor` and catch-all `markdown` groups,
     that `web/scripts/size.mjs` and `web/eslint.config.js` still behave as described, and that
     `web/src/styles/tokens.css` has the tokens listed.
   - Confirm prompt 30 is merged: `ls web/src/features/operations web/src/design/operationVocab.ts
     web/src/app/rightSlot` all exist. Confirm prompt 26's files exist: `web/src/api/operationTrace.ts` and the five
     JSON files in `web/src/test/fixtures/operation-trace/`. **If the fixtures or `operationTrace.ts` are missing, stop and
     report**: the spike must render the real traces. Open each fixture and note its step and edge counts.
   - Read `web/DESIGN.md` as it is on your branch, in full, and copy the exact current text of the passages listed in
     section 3.3 (they will be the **Before** text of the proposal).
   - Confirm nothing named `R-1-*` and no `probes/r1-graph` exists under `docs/rework/investigations/`.
   - Run `git status` and `git branch --show-current`; the tree must be clean and the base must be the latest
     `overhaul-investigation`. Check `node --version` is 22 or newer and that Chrome is installed (or run
     `npx playwright install chromium` in the probe later and set `SPIKE_CHANNEL=` empty).
2. Create branch `graph-spike` from the latest `overhaul-investigation`.
3. **Baseline of the product budget (read-only).** `cd web && npm ci && npm run build`, then
   `node scripts/size.mjs --report`; record initial JS, largest chunk (with its file name and which group it belongs
   to) and CSS, in gzip. `web/dist` is git-ignored; run `git status` afterwards and confirm `web/` is unchanged. These
   are the numbers the report quotes for headroom.
4. **Scaffold the probe** (`probes/r1-graph/`): `package.json`, `.gitignore` (`node_modules/`, `out/`, `dist/`,
   `results/shots/` except embedded files, via explicit un-ignores), `tsconfig.json`, `eslint.config.js` (6.11),
   `vite.config.ts` (6.10), `README.md`. Install the non-candidate dependencies with `npm install --ignore-scripts`.
   Commit: `docs(web): scaffold the graph library probe`.
5. **Policy and control first.** Write `scripts/csp.mjs`, `scripts/serve.mjs`, `control.html` and its script, and the
   control part of `scripts/measure.mjs`. Run the control under both policies and make the **control gate** (6.6) pass
   before anything else. Add the vitest test `csp_header_is_read_from_the_hub_source`. If the control cannot be made to
   record violations, stop and report. Commit.
6. **Shared harness.** `src/harness/`: `fixtures.ts` (loads the five files with `import.meta.glob` by relative path
   from `web/src/test/fixtures/operation-trace/`), `synthetic.ts`, `vocab.ts`, `theme.ts`, `motion.ts`,
   `geometry.ts`, `keyboardModel.ts`, `Outline.tsx`, `controls.tsx`, `main.tsx`, `spike.css`, and the vitest tests of
   section 12.1 for the pure modules. Make `npm test`, `npm run lint`, `npm run typecheck` and `npm run check:css`
   pass for the `baseline` variant. Build `baseline` and open it once by hand. Commit.
7. **Layout adapters.** `src/layout/types.ts`, `layered.ts`, then install `@dagrejs/dagre` and `elkjs` and write
   `dagre.ts`, `elk.ts` and `elkWorker.ts`; the layout tests of 12.1 run on the five fixtures and on `synthetic(200)`
   for every adapter. Commit.
8. **SVG renderer** (`src/svg/`): nodes as SVG groups placed with the `transform` attribute, edges as paths with
   `marker-end`, edge labels as text for assistive technology (visually available on selection), selection and
   focus, the keyboard contract, pan and zoom through the `viewBox` attribute, fit, and the two motions. A CSS
   transition cannot animate the SVG `transform` **attribute**, so animate node movement one of two ways and record
   which: (a) a class with `transform: translate(var(--x), var(--y)); transition: transform var(--motion-layout)
   var(--ease-out)` where `--x` and `--y` are set per node through `src/harness/geometry.ts` (this is the "CSS custom
   properties" route the design guide names; it writes a `style` attribute holding only custom properties, which the
   DOM audit counts as S1), or (b) a `requestAnimationFrame` tween of the `transform` attribute with its duration from
   `motionMs` (no style written). **Implement both routes in every SVG variant**, selected at run time by the query
   `anim=css` (default) or `anim=raf` (6.1); the measurement runs the DOM audit, K and R checks for each route and the
   report gives one G3b class per route (the SVG variants are therefore measured twice for style). Build `svg-layered`,
   `svg-dagre`, `svg-elk`, `svg-elk-worker`. Make K1 to K11 and R1 to R5 pass for `svg-layered` before copying the
   renderer to the others. Commit.
9. **React Flow** (`src/reactflow/`): install `@xyflow/react`; custom node component using the shared node markup and
   token CSS; nodes positioned from the shared layout adapter; decide on and record how its own CSS is handled (not
   imported, or imported and overridden), because DESIGN.md allows raw colours only in `tokens.css`; wire the keyboard
   contract; record every default you override. Build `reactflow`. Commit.
10. **Cytoscape** (`src/cytoscape/`): install `cytoscape` and `cytoscape-dagre`; canvas styling from tokens (resolve
    custom properties with `getComputedStyle`, and re-resolve on theme change; record whether a token that computes to a
    colour function needed normalising before the library accepted it); keyboard on the container; selection drawn on
    the canvas; animation durations from `motionMs`. Build `cytoscape-core` and `cytoscape-dagre`. Commit.
11. **Build and size.** `npm run build:all` builds every variant plus `control`; `node scripts/size.mjs` writes the
    `size` part of the results using the method of `web/scripts/size.mjs` (gzip each file; delta over `baseline`).
    Check each variant's `index.html` preload list does not include graph chunks (G2 lazy). Commit.
12. **Measure.** `npm run measure` runs everything in 6.6 for every variant, writes `results/results.json` and
    screenshots. Run `node scripts/licences.mjs` (reads each variant's installed packages' `package.json`
    `license` field and counts runtime packages in the graph chunk by walking `npm ls --all --omit=dev --json`) and
    the maintenance lookups (registry `https://registry.npmjs.org/<name>` for latest version and its date,
    `https://api.npmjs.org/downloads/point/last-week/<name>` for downloads, the repository page for open issues; write
    "not observed" where a call fails). If a variant crashes or cannot be built, record it as "could not evaluate",
    keep its log in the README, and continue. Commit the results file.
13. **Look at the pictures.** Open every screenshot you will embed; reject any that is blank. Check by eye for edges
    crossing nodes, clipped labels, unreadable contrast in either theme, and a graph that overflows at 375 px; note
    each in the candidate notes.
14. **Write the report** (6.8), applying the recommendation rule of 6.7 step by step, then **the proposal** (6.9). Run
    `node scripts/tables.mjs` and paste its output; do not retype numbers. Every number in the report must be in
    `results.json`. Commit: `docs(web): record graph library evidence` and `docs(web): propose design guide changes for
    the graph`.
15. **Checks and completion report.** Run, in the probe: `npm run lint`, `npm run typecheck`, `npm test`,
    `npm run check:css`. Confirm isolation with `git status` and `git diff --stat overhaul-investigation -- web
    crates xtask .github AGENTS.md TESTING.md docs/book docs/rework/99-open-decisions.md
    docs/rework/07-implementation-prompt-index.md` (**empty**), confirm `git ls-files docs/rework/investigations/probes/r1-graph |
    grep -E 'node_modules|/out/|/dist/'` is empty, and `grep -rn "R-1" docs/rework/investigations/R-1-graph-library.md
    | grep -i "accepted"` shows only the "not accepted" statements. You need not run `cargo`, `cargo xtask` or the web
    checks, because nothing they cover changed. End with the completion report.

## 8. Database and persistence

Not applicable: no database, table, migration or persisted application state is touched. The only files written are
under `docs/rework/investigations/`. The probe's own `results/results.json` is a committed evidence file, not
application data.

## 9. Protocol and compatibility

Not applicable: no wire frame, HTTP route, OpenAPI document or generated type changes, and no hub or agent behaviour
changes. The one contract the probe **consumes** is the operation trace, `schema_version: 1` (prompt 26): the
renderers must tolerate an unknown step kind, outcome or edge kind by drawing it generically (neutral tone, raw value
as the label) and the probe's `vocab.ts` falls back accordingly; add one vitest case that feeds a fixture-shaped trace
with an unknown kind, outcome and edge kind through each layout adapter and the keyboard model without throwing.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Prompt 26's fixtures or `operationTrace.ts` are missing | stop at step 1 and report; do not substitute made-up traces for the five fixtures (generated traces are only for stress timing) |
| `csp.rs` cannot be read, or the `BASE_POLICY` regular expression matches zero or several literals | `csp.mjs` throws and every script that needs the policy exits non-zero; do not paste the policy by hand |
| The negative control records no violation of a required kind | the harness is invalid: stop, fix, re-run; never write the report from an invalid run |
| A candidate installs but fails to build or run | record "could not evaluate" with the error and the log path; keep going with the other variants; the report says what was tried |
| A candidate only works with `unsafe-eval`, a `blob:` worker or `wasm-unsafe-eval` | it fails G1; say so; do not weaken the served policy, do not add `bypassCSP`, do not propose a wider policy |
| A candidate's keyboard behaviour cannot meet K1 to K8 without a large wrapper | implement the wrapper, count its lines, and record it; if it cannot be met at all it fails G4 |
| Chrome is not installed | set `SPIKE_CHANNEL=` empty and run `npx playwright install chromium` in the probe; record which browser the numbers come from |
| Firefox is not available | skip step 8 of 6.6 and label Firefox **not demonstrated** |
| A registry or repository lookup fails | write "not observed" for that field; never fill it from memory |
| A package's licence field is missing or non-SPDX | record "unknown" and flag it for owner review; do not guess |
| A measurement is noisy (two runs of the same variant differ by more than 20 percent in `layout_ms`) | take six runs and report the median and the range; say it was noisy |
| `git status` shows a change under `web/`, `crates/` or any other forbidden path | revert that change before committing; the isolation check of step 15 must be empty |
| A screenshot is blank or the graph is not drawn | fix the variant or record it as broken; do not embed it as evidence |
| The measured chunk is over 120 kB gzip | the variant fails G2; report the number; do not trim features to pass without saying so |
| A step of this brief cannot be done as written | stop and report the difference, as the execution constraints require |

## 11. Security requirements

- **The policy is the contract and it is not touched.** The harness never edits `csp.rs`, never sets Playwright's
  `bypassCSP`, never adds `unsafe-eval`, `blob:` or `wasm-unsafe-eval` to what it serves, and reads the served policy
  from the hub source so it cannot drift. The negative control proves the browser enforces it (a run without a passing
  control gate is invalid).
- **Step text is untrusted.** A trace's `summary` is derived from model and host output. Every variant must draw it as
  **text** (React child, SVG `<text>`, canvas label), never as HTML: no `dangerouslySetInnerHTML`, no `innerHTML`, no HTML
  label or HTML node extension. Check **X1**: load a fixture-shaped trace whose summaries are `<img src=x onerror="window.__x=1">`,
  `<script>window.__x=1</script>` and `"><svg onload=window.__x=1>`; in every variant the string appears literally, no
  element is created from it, `window.__x` stays undefined and there is no CSP violation. Store under `results.safety`.
- **Supply chain.** The probe's dependencies are throwaway but still run code on the developer machine at build time.
  Install with `--ignore-scripts`; commit the lockfile; pin candidate versions exactly; record whether any package
  declares an install script (`scripts.install`, `preinstall` or `postinstall`); record the `npm audit` summary for the
  probe without fixing anything; never run `npm publish`. State in the README and the report that Dependabot and CodeQL
  do not watch this directory, so it must not be deployed or imported by product code. The number of runtime packages
  in the graph chunk is part of the evidence because every one is attack surface for the product later.
- **Local only.** The server binds `127.0.0.1`, serves nothing outside `out/` (resolve the requested path and refuse
  anything that leaves the root), needs no credentials and talks to no hub. Fixtures are the hand-built golden
  traces: no real operator data, hostnames or secrets enter the probe or the report. The only outbound requests are the
  npm install and the registry and repository metadata reads of step 12, which send package names and nothing else.
- **No authority.** The probe has no approval, action or mutation code path of any kind. Nothing in it can approve or
  run anything.
- **No over-claiming.** The report never states that R-1 is accepted, resolved or "safe"; it lists what was and was not
  demonstrated (6.8, section 9 of the report).
- **No secrets in committed files**, including screenshots (they show fixtures only).

## 12. Testing requirements

This is a research change; its tests protect the **validity of the evidence**. Tests are named after behaviours and live
in `probes/r1-graph/tests/` (vitest, run with `npm test` in the probe). There is no Rust test and no `domovoi-testkit`
fake: no Rust crate changes. The probe's jsdom is not used for layout or CSP; those are measured in a real browser.

### 12.1 Vitest (pure modules)

| Test | Setup, action, expected |
|---|---|
| `csp_header_is_read_from_the_hub_source` | run `csp.mjs` against the real `csp.rs`; the `real` value starts with `default-src 'self'`, contains `style-src 'self' 'unsafe-inline'` and `script-src 'self'`, and contains neither `unsafe-eval` nor `blob:`; `strict-style` equals it without `'unsafe-inline'` |
| `csp_extraction_fails_loudly_on_a_file_without_the_constant` | feed a string with no `BASE_POLICY` and one with two; both throw |
| `fixtures_load_and_satisfy_the_trace_invariants` | the five files load; `schema_version === 1`; step ids unique; every edge endpoint is a step; every edge `from` has a higher ordinal than its `to`; together they contain every kind, outcome and edge kind listed in section 3.4 |
| `synthetic_trace_is_acyclic_and_deterministic` | `makeTrace(n, 1)` for 50, 200, 500: same twice; edges obey the ordinal rule; no duplicate `(from, to, kind)`; step count `n`; at least one disconnected step; at least one `retry_of` |
| `every_layout_places_prerequisites_left_of_dependents` | for each adapter (`layered`, `dagre`, `elk`) on each fixture and `makeTrace(200, 1)`: for every edge the dependent's `x` is greater than the prerequisite's `x` |
| `no_layout_overlaps_two_nodes` | same inputs: no two placed rectangles intersect |
| `layouts_are_deterministic` | two runs of each adapter on the same input are deeply equal |
| `layered_layout_ranks_by_longest_path` | a hand-built diamond plus a chain: ranks are the longest-path lengths; an edge-free step is rank 0 |
| `compact_layout_is_tighter_than_normal` | `compact: true` gives a smaller `width` and `height` than `false` on a trace with at least two ranks and two rows |
| `unknown_kind_outcome_and_edge_kind_do_not_throw` | a fixture-shaped trace with `kind: "future_kind"`, `outcome: "future_outcome"`, edge `kind: "future_edge"` goes through every adapter, the keyboard model and `vocab.ts`; the label is the raw value and the tone is neutral |
| `arrow_right_selects_a_dependent_in_a_layer_to_the_right` | hand-built nodes and edges; expected id per 6.3 |
| `arrow_left_selects_a_prerequisite_in_a_layer_to_the_left` | same |
| `arrow_down_and_up_stay_in_the_layer` | same; at the first and last node of a layer the result is `null` |
| `home_and_end_select_the_first_and_last_ordinal` | same |
| `a_selects_running_then_pending_then_the_last_step` | three small traces, one per case |
| `an_arrow_with_nothing_selected_selects_the_first_step` | `current: null` |
| `ties_break_by_vertical_distance_then_ordinal` | two dependents at equal distance: the lower ordinal; unequal: the nearer |
| `a_key_that_moves_nothing_returns_null` | ArrowRight on a node with no dependents |
| `parse_duration_reads_ms_and_seconds` | `"240ms"`, `"0.24s"`, `"0ms"`, `" 240ms "` give 240, 240, 0, 240; garbage gives 0 |
| `probe_source_passes_the_inline_style_rule` | run ESLint's API over `src/**` with the probe config (as `web/src/guardrails.test.ts` does for the product): zero messages |
| `probe_css_has_no_raw_colour` | run `check-css.mjs` logic over `src/**/*.css`: zero findings. Try importing the product's `findCssProblems` from `web/src/styles/cssGuard.ts`; if the installed Node cannot load TypeScript, copy its regular expressions into `check-css.mjs` and say so in the README |

### 12.2 Browser checks (Playwright, `scripts/measure.mjs`)

Each check has a stable id in `results.json`; a failure is recorded, not thrown, so one broken variant does not hide
the others, but the script exits non-zero if the control gate fails.

| Id | Behaviour | Where defined |
|---|---|---|
| `control_real`, `control_strict_style` | the negative control records violations of the kinds required in 6.6 | 6.6 |
| `G1` per variant, fixture, policy, anim | no violation and no CSP console error under `real` | 6.6, 6.7 |
| `K1`…`K11` per variant | keyboard contract | 6.3 |
| `R1`…`R5` per variant | motion contract | 6.5 |
| `X1` per variant | summary markup is drawn as text | section 11 |
| `axe` per variant | no serious or critical violation | 6.6 step 4 |
| `perf` per variant and size | median layout and first render time, DOM nodes | 6.6 step 7 |
| `size` per variant | gzip bytes by the product method | 6.6, step 11 |

Each check must be able to fail: before trusting a pass, run it once against a deliberately broken input (for example
remove the `Escape` handler, or serve `real` with `style-src 'self'` for a variant that writes inline style) and see it
fail; record in the README that you did.

## 13. Acceptance criteria

1. `git diff --stat overhaul-investigation` lists only files under `docs/rework/investigations/` (the two
   deliverables and `probes/r1-graph/`). Nothing under `web/`, `crates/`, `xtask/`, `.github/`, `docs/book/`,
   `AGENTS.md`, `TESTING.md`, `docs/rework/99-open-decisions.md` or `docs/rework/07-implementation-prompt-index.md`
   changed, and `web/DESIGN.md` is byte-identical.
2. No `node_modules/`, `out/` or `dist/` directory of the probe is committed; `package-lock.json` is.
3. The served policy in the report is, character for character, `BASE_POLICY` read from `csp.rs` on the branch plus
   `; script-src 'self'`, and the report quotes it; the `strict-style` string is labelled hypothetical.
4. The negative control passes its gate under both policies, and the report tabulates what each attempt did.
5. All eight variants were built; each is either measured or listed as "could not evaluate" with the reason.
6. Every size in the report equals a number in `results/results.json` computed with gzip defaults and 1000 bytes per kB;
   the baseline numbers of step 3 are quoted; the report states for each variant whether its code is lazy.
7. Every G1 result states the number of violations under `real` and under `strict-style` for each variant, route
   and fixture, and the worker variant states whether the worker ran under `real`.
8. Every variant has a G3b class (S0 to S3) per `anim` route where it applies, with the properties found and the
   mutation count, and the report states how many times each DOM variant used `geometry.ts`.
9. K1 to K11, R1 to R5 and X1 are in the report for every graph variant, with the ARIA snapshot of the graph root in
   the appendix, and each "pass with wrapper" states its line count.
10. Licences and runtime package counts are listed per variant; every flagged licence is called out as needing owner
    review; no legal advice is given.
11. Maintenance fields are marked "observed on <date>" with their source, or "not observed".
12. Stress timings for 50, 200 and 500 steps are in the report as medians with the run count.
13. The "also considered" table contains only observed facts.
14. The report applies the recommendation rule of 6.7 visibly, names one variant and one runner-up (or "none yet"), and
    lists what would change it.
15. The report has the three labelled lists (demonstrated, not demonstrated, needs follow-up) and the list of decisions
    requested from the owner, including the inline-style acceptability question.
16. The report's first lines say R-1 is **not accepted**, and no sentence anywhere says it is accepted or resolved.
17. The amendment proposal exists, is marked "PROPOSAL, not accepted", gives exact **Before** (quoted from
    `web/DESIGN.md` on the branch) and **After** text for every item of 6.9, includes the ESLint scope diff, the build
    group text and the token lines, and states the files prompt 38 would change.
18. In the probe, `npm run lint`, `npm run typecheck`, `npm test` and `npm run check:css` pass, and the README says each
    browser check was seen to fail once against broken input.
19. No variant weakens the policy: the report states that no served policy differs from the two quoted strings.
20. The completion report lists every command run with its real result and every item not run (including `cargo` and the
    web checks, with the reason: nothing they cover changed).

## 14. Migration and compatibility

Not applicable: nothing ships. No schema, configuration key, API, agent or installed data is affected, there is no N-1
case, and the probe is not part of any build, image or release. Any dependency the owner later accepts is added to
`web/package.json` by prompt 38, with its own review. The probe's lockfile is evidence of the versions measured, not a
supply-chain commitment.

## 15. Documentation requirements

- Write `docs/rework/investigations/R-1-graph-library.md` (6.8) and `docs/rework/investigations/R-1-design-md-amendment-proposal.md` (6.9).
- Write `probes/r1-graph/README.md`: what the directory is, that it is throwaway and unreachable from product code,
  that Dependabot and CodeQL do not watch it, how to run each script in order, the Node and browser used, the
  broken-input checks you ran, and a "could not evaluate" log if any.
- **Do not** edit `web/DESIGN.md`, `ARCHITECTURE.md`, `AGENTS.md`, `TESTING.md`, the book, the configuration reference,
  OpenAPI, `docs/rework/99-open-decisions.md` or `docs/rework/07-implementation-prompt-index.md`. Acceptance of R-1, the
  index status line and the DESIGN.md change are separate steps by the owner and by prompt 38.
- No book page, no config reference regeneration (`cargo xtask docs-config`), no OpenAPI regeneration, no
  `cargo xtask docs-links` run: nothing they cover changes.

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
