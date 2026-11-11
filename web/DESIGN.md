# Web UI design guide

Read this before changing anything under `web/`. Rules come first, each with its reason in a
line. `/dev/design` (dev server only) renders every token, tone, status, icon and kit
component: check your work there in both themes.

## Principles

1. **Calm and professional.** Muted brown and green, warm paper and ink. Nothing saturated or
   playful; no effect that does not make something clearer or faster.
2. **Colour carries meaning, never alone.** Every coloured thing also has a word and, where
   it helps, an icon.
3. **Every action answers back** in plain words, where the person is looking: success,
   failure, progress, "nothing changed".
4. **Explain on demand.** Non-obvious things explain themselves on hover or focus; obvious
   things do not.
5. **Never blank, never lying.** Every view has loading, empty, error and offline states. A
   failed request never reads as "nothing here".
6. **Fast.** Slow phone, VPN: no layout shift, no refetch storms, nothing from another origin.
7. **One source of truth per decision.** Colours, spacing, radii, shadows, motion and layers:
   `src/styles/tokens.css`. Statuses: `src/design/tone.ts`. Icons: `src/design/icons.tsx`.
   Error sentences: `src/design/messages.ts`. Nothing else decides them.

Motion is 120–180 ms, eases out, explains a change, and is off under
`prefers-reduced-motion` (the motion tokens become `0ms`: always use them).

## Brand roles, tokens, type

**Green acts** (`--color-primary`: buttons, links, focus ring, selected navigation, toggles
that are on). **Brown frames** (`--color-frame`, `--color-sidebar-*`: sidebar, brand mark,
heading accents). **Paper and ink** (`--color-bg`, `--color-surface`, `--color-text`) is
everything else. Success and primary green must differ at a glance: success is a soft pill
with an icon, primary is a filled button. Never fill a button with a tone's `-solid`.

- `tokens.css` is the only file with raw colours (hex, `rgb()`, named). Stylesheets use the
  semantic tokens; `--palette-*` primitives are for `tokens.css` only.
- The dark theme is written twice in `tokens.css` (explicit and "System"), between
  `dark-theme:start` and `dark-theme:end`. Change both; a test compares them.
- Inside the sidebar the semantic tokens are remapped (`Layout.module.css`).

| Token                                                       | Use it for                                                               |
| ----------------------------------------------------------- | ------------------------------------------------------------------------ |
| `--color-bg`, `-surface`, `-surface-raised`                 | Page; cards, tables, inputs; hover rows and table headers                |
| `--color-border`, `-border-strong`                          | Dividers and card edges; edges needed to use a control (3:1)             |
| `--color-text`, `-text-muted`, `-text-subtle`               | Body; meta lines, hints, times; placeholders and table header labels     |
| `--color-primary`, `-primary-hover`, `-on-primary`          | Things that act, and the text on them                                    |
| `--color-focus`, `--color-overlay`                          | The focus ring (global; never remove outlines); the scrim behind dialogs |
| `--tone-<tone>-bg`, `-fg`, `-border`                        | A pill or banner: tint, text, edge (`-fg` also passes on the page)       |
| `--tone-<tone>-solid`, `-on-solid`                          | Counts and one page-level critical banner. Nothing else                  |
| `--tone-danger-faint`                                       | The one row tint: `DataTable` `rowState="urgent"`                        |
| `--space-1` … `--space-8`                                   | 4, 8, 12, 16, 24, 32, 48, 64 px: every gap, padding and margin           |
| `--radius-sm`, `-md`, `-lg`, `-pill`                        | 6 px controls, 10 px cards and dialogs, 14 px panels, pills              |
| `--shadow-raised`, `--shadow-overlay`                       | A floating card; a dialog, menu or toast                                 |
| `--text-xs` … `--text-2xl`, `--font-sans`, `--font-mono`    | Six type steps; UI text; commands, logs, arguments and IDs               |
| `--z-sticky`, `-dropdown`, `-overlay`, `-toast`, `-tooltip` | Layers. Never a bare `z-index` number                                    |
| `--motion-fast`, `--motion-base`, `--ease-out`              | 120 ms, 180 ms, the one easing curve                                     |

Type is the system sans and mono stacks (no web font: nothing to download, no layout shift),
15 px on desktop and 16 px on phones (iOS does not zoom into inputs). Sentence case
everywhere. Use the `tabular-nums` class for numbers in tables, counts and times.

## Tones

One closed set; every status maps to exactly one tone in `tone.ts`.

| Tone       | Means                      | Examples                                                             |
| ---------- | -------------------------- | -------------------------------------------------------------------- |
| `danger`   | Broken, needs a person now | Action needed, critical, action failed, host offline, destructive    |
| `warning`  | Off, watch it              | Monitor, warning, triage skipped, write skill, outdated agent        |
| `success`  | Fine, done, recovered      | Fixed, resolved, action succeeded or verified, host connected        |
| `pending`  | Waiting for a person       | Waiting for approval, incident pending, the approvals count          |
| `progress` | Something is running       | Action approved or running, live updates connecting                  |
| `info`     | Neutral information        | Severity info, read skill, maintenance, notes                        |
| `neutral`  | Intended, quiet, closed    | Expected, acknowledged or muted, denied or expired, anything unknown |

**Pills, not paint.** A status is a pill: `-bg` tint, `-fg` text, icon, label. A row or card
may carry a 3 px left stripe (`toneStripe`). Whole rows are never tinted, except the faint
danger tint above. An unknown value (the hub may be newer) is a `neutral` pill with the raw
value; never throw on one.

`tone.ts` is the entry point for statuses (`verdictMeta`, `actionStatusMeta`, …); its tables
sit in `respondVocab.ts`, `actionVocab.ts` and `fleetVocab.ts`, split only to keep files
short. Import from `tone.ts`. Icons: `<Icon name="…" />` only; to add one, add an entry to
`ICONS`. Theme: `ThemeSwitch` and `design/theme.ts` set `data-theme`; a static inline script
in `index.html` applies it before paint and the hub allows it by hash, so keep it static.

## The shell

`src/app/` is the frame around signed-in pages: sidebar (`nav.ts`), top bar, the host
selector (`useSelectedHost()`), page title and focus, lazy pages (`pages.ts`), skeletons.
Pages draw none of it. Under 768 px navigation is a sheet; above, a column (a rail up to
1023 px). Every page follows the host selector (filter on the hub with `host`) or renders
`<AllHostsNote>` saying why not. Navigation counts come from `useOverview(host)`: never
fetch a list to count it.

## Components

All in `src/components/`, exported from `index.ts`; pages import from that barrel only. If a
page needs something the kit lacks, add it to the kit, to this list and to `/dev/design`.

- **Status:** `StatusPill` (every status; the hint is its tooltip), `BadgePill`, `Tag`
  (non-status labels), `Count` (filled numbers), `toneStripe`.
- **Feedback:** `useActionFeedback` (every mutation), `useToast`, `InlineMessage`,
  `InlineError`, `Banner` (one page-wide message), `ErrorDetail`.
- **Hints:** `Tooltip` (a few words, hover or focus), `InfoTip` (a sentence or two on click),
  `DocsLink`, `Kbd`, `Highlight`.
- **Buttons:** `Button` (`variant`, `size`, `icon`, `loading`, `disabledReason`, `hint`),
  `IconButton` (`label` is required: name and tooltip), `LinkButton` (navigation),
  `CopyButton`. `primary` is the one main action of a view, `danger` is destructive. A
  running button shows `loading`, never a changed label. A button that cannot be used gets
  `disabledReason` (it stays focusable and says why); the bare `disabled` is a type error.
- **Loading, empty, error:** `QueryBoundary`, `Loading`, `Skeleton`, `Spinner`, `Progress`,
  `LoadError` (what, why, Retry), `EmptyState` (why, next step; never for a failed load).
- **Layout:** `PageHeader` (the only `h1`), `Section` (`h2`), `Card`, `Text`, `AuthCard`,
  `BrandMark`, `Stack`, `Cluster`, `Grid`.
- **Data:** `DataTable` (memoized rows: keep `columns` stable), `Stepper`, `Timeline`,
  `KeyValueList`, `FilterChips`, `Tabs`, `RelativeTime`, `Countdown`, `CopyBlock`,
  `Markdown`, `ChatThread`.
- **Forms:** `Field` (around every input: label, hint, error), `TextInput`, `NumberInput`,
  `Textarea`, `Select`, `PasswordInput`, `Checkbox`, `Switch`, `SegmentedControl`,
  `DropZone`, `FormErrorSummary`, `SubmitBar`.
- **Overlays:** `Dialog`, `ConfirmDialog` (names the object and the consequence; the button
  repeats the verb), `PasswordPrompt`, `Menu`, `OptionPicker`, `Popover` (kit-internal).

Layout patterns. **List page:** `PageHeader` with `intro`, filters (`FilterChips`, `Select`),
then `QueryBoundary` around a `DataTable` with an `EmptyState`. **Detail page:** `PageHeader`
with `meta` and `actions`, then `Section`s or `Tabs`. **Settings page:** one `FieldSpec` per
setting (see Add a setting) and a `SubmitBar`. **Dialog form:** `Dialog` with `Field`s, the
error inside the dialog, a primary verb button and Cancel.

## Messaging

Plain, calm, specific; sentence case; no exclamation marks, no "Oops", no blame, no jargon.
Say what happened, then what to do. A sentence is one string, never joined fragments.

| Situation                                     | Where                                                  | Example                                                     |
| --------------------------------------------- | ------------------------------------------------------ | ----------------------------------------------------------- |
| A quick action succeeded                      | Toast (~4 s), with Undo where the API allows           | "Incident muted for 1 hour" [Undo]                          |
| It succeeded but changed nothing              | Info toast                                             | "You are on the latest version"                             |
| An action failed, its control is gone         | Toast that stays until dismissed                       | "Could not approve the action. The approval window closed." |
| An action failed, its control stays           | `InlineMessage` (`errorDisplay: "inline"`)             | "Could not save the note. The note is too long."            |
| Anything fails inside a dialog                | Inside the dialog (a toast would sit behind it)        | "Could not revoke the agent. The hub did not answer."       |
| A form has problems                           | Under each field, `FormErrorSummary` on long forms     | "Port must be between 1 and 65535."                         |
| A page or panel cannot load                   | `LoadError` in place of the content, with Retry        | "Could not load hosts. The hub did not answer." [Retry]     |
| Live updates lost                             | One `Banner` at the top of the main area               | "Live updates paused. Reconnecting…"                        |
| Something long is running                     | `Progress` where the result will appear                | "Triage is reading the logs…"                               |
| It ended later, or changed without the person | Toast when the page sees it (`useActionOutcomes`)      | "restart_container succeeded on nas-01"                     |
| Rate limited                                  | Inline or toast; the kit counts down from `retryAfter` | "Too many attempts. Try again in 28 s."                     |
| Destructive or irreversible                   | `ConfirmDialog`                                        | "Revoke agent nas-01? It disconnects now…" [Revoke agent]   |
| Nothing to show                               | `EmptyState` saying why and the next step              | "No incidents. Everything your agents watch is healthy."    |

Every failure becomes words in `messages.ts` and nowhere else. `describeError(error,
context)` gives a title ("Could not approve the action"), a detail and the hub's
`retryAfter` and `issues`; you rarely call it: `useActionFeedback`, `LoadError`,
`InlineError` and `QueryBoundary` take the error and a verb-phrase `context`. A raw code or
stack trace is never shown. Bad: "Success!", "Error: Failed to fetch", "409 conflict", "Are
you sure?" with "OK", "No data", a greyed-out button with no reason.

## Hints

Icon-only button: `IconButton` `label`. Status: `StatusPill`. Unclear button: `hint`. Disabled
control: `disabledReason` (why, and how to enable it). Relative time, truncated value,
abbreviation: `Tooltip` with the exact value. Unclear column header: the column's `hint`.
A concept: `InfoTip` with `learnMore`. What an input expects: the `Field` hint. What a page
is for: `PageHeader` `intro`. Never the native `title` (not on touch or keyboard). A tooltip
shows after 400 ms on hover and at once on focus, Escape closes it, and it never shows on
touch: it may only repeat what is on screen. Do not explain the obvious.

## Checks that will fail your PR

| What                                                           | Command         | Fix                                                              |
| -------------------------------------------------------------- | --------------- | ---------------------------------------------------------------- |
| Raw colour or `--palette-*` outside `tokens.css`               | `npm test`      | Use a semantic token (`styles/cssGuard.test.ts`)                 |
| The two dark theme blocks differ                               | `npm test`      | Make them identical                                              |
| A token pair below WCAG AA (text 4.5, UI 3)                    | `npm test`      | Adjust the token (`styles/contrast.test.ts`)                     |
| A status without label, tone, hint, icon                       | `npm test`      | Add it to its vocabulary in `src/design/`                        |
| A hub error or failure without words                           | `npm test`      | Add it to `messages.ts`                                          |
| "Oops", "Something went wrong", "Error:", "!" in text          | `npm test`      | Say what happened and what to do (`voice.test.ts`)               |
| An axe violation in whatever a page test leaves on screen      | `npm test`      | Fix the markup; `skipAxe()` only for a deliberate fragment       |
| Inline JSX `style`                                             | `npm run lint`  | A CSS module with tokens; computed values need a disable comment |
| JSX `title=`                                                   | `npm run lint`  | `Tooltip`, `InfoTip` or visible text                             |
| A raw `<button>` in `src/features` or `src/app`                | `npm run lint`  | `Button` or `IconButton`                                         |
| A verdict string literal (`"ACTION_NEEDED"`) in page code      | `npm run lint`  | `verdictMeta` and `StatusPill`                                   |
| `disabled` on a control without `disabledReason`               | `npm run lint`  | Pass `disabledReason`; or disable the line and name the hint     |
| `.mutate(` in a file with no `useActionFeedback`/`InlineError` | `npm run lint`  | Wrap it; or `// feedback-ok: where it is reported` at the top    |
| A deep import of a kit file; `lucide-react` outside `Icon`     | `npm run lint`  | Import from the barrel; use `Icon`                               |
| `fetch` outside `src/api`                                      | `npm run lint`  | A hook in `src/api/hooks/`                                       |
| A source file over 250 lines (no exemptions)                   | `npm run lint`  | Split by responsibility                                          |
| Initial JS over budget, a chunk over 120 kB, CSS over 30 kB    | `npm run build` | Load it lazily or drop the dependency (`npm run size`)           |

The lint rules live in `eslint/domovoi-rules.js`; `src/guardrails.test.ts` proves they fire.

## Performance budget

<!-- budget:initial-js-gzip-kb 170 -->

| Measure (gzip, `npm run size`) | Main before the redesign | Now      | Limit                  |
| ------------------------------ | ------------------------ | -------- | ---------------------- |
| Initial JavaScript             | 179.0 kB, one chunk      | 154.4 kB | 170 kB (achieved +10%) |
| Largest chunk                  | 179.0 kB                 | 110.7 kB | 120 kB                 |
| Total CSS                      | 9.9 kB                   | 13.2 kB  | 30 kB                  |

Measured on a throttled phone (4× CPU, 1.6 Mbit/s, 150 ms): login LCP 1.4 s, CLS 0.000,
TBT 27 ms; incidents LCP 1.4 s, CLS 0.021, TBT 0 ms. Incidents, Login and Setup are in the
first download; every other page is in `pages.ts` and fetched when opened (a link fetches on
hover or focus). Markdown is its own chunk; `vendor` is split so it stays cached across hub
upgrades. A mutation refreshes only what it changed; polling stops when nothing can change.
Raise the budget only with a reason in the PR, and record the new number above.

## Recipes

**Add a page.** (1) Create it in `src/features/<area>/`, start with `<PageHeader title
intro docs />`. (2) Add it to `PAGES` in `src/app/pages.ts` and a `<Route>` with
`<PAGES.name.Page />`; never import it into `routes.tsx` (it would join the first
download). (3) Load data with a hook from `src/api/hooks/` inside `QueryBoundary` with a
`context`, an `EmptyState` and a `Skeleton`. (4) Follow the host selector or render
`<AllHostsNote>`. (5) Lay out with `Section`, `Stack`, `Cluster`, `Grid`, `Card`,
`DataTable`; its CSS module only places things. (6) A page about one thing calls
`usePageName(thing.title)`. (7) Check 375, 768, 1280 px, both themes, keyboard.

**Add a navigation item.** One entry in `NAV_GROUPS` (`src/app/nav.ts`): `to`, `label` (one
or two words: on the rail it is the tooltip), `icon`, `preload: PAGES.name.preload`. For a
count, add the field to the hub's overview (`GET /api/v1/overview`), regenerate the API
types, and set `count` (field, tone, the word after the number).

**Add a status.** (1) Add the value to its type and table in `src/design/` (`respondVocab`,
`actionVocab` or `fleetVocab`) with label, tone, a hint saying what it means, an icon; a new
vocabulary also goes in `VOCABULARIES` and gets a lookup in `tone.ts`. (2) `npm test` covers
it and `/dev/design` shows it. (3) Render `<StatusPill meta={…Meta(value)} />`; a row takes
its stripe from the same meta. (4) A new timeline kind goes in `KINDS` in `timeline.ts`.
(5) Filtering on it is the hub's job: add the filter to the endpoint, never filter loaded
pages in the browser.

**Show an action's result.** The worked examples are `useIncidentActions.ts` and
`ApproveControl.tsx`.

```tsx
const ack = useActionFeedback(useAck(id), {
  success: "Incident acknowledged",
  error: "acknowledge the incident", // "Could not acknowledge the incident"
  undo: () => unack.mutate(), // optional
});
<Button onClick={() => ack.mutate()} loading={ack.isPending}>
  Ack
</Button>;
```

`success` may be a function of the result, return `{ title, detail, tone: "info" }` for
"nothing changed", or `null` when the page shows the result itself. Inside a dialog, or when
the person must fix input, add `errorDisplay: "inline"` and render `ack.error` with
`InlineMessage`. The same action from two places goes in one hook. A result that arrives
later: the toast says what was started, the card shows `Progress`, `useActionOutcomes`
announces the end.

**Show a load error.** Use `QueryBoundary`, or `<LoadError error={q.error} context="load
hosts" onRetry={q.refetch} />` when there is an error and no data. Never an `EmptyState` for
a failed request; a 404 for one object says "No such host" with a link back.

**Add a setting.** Add it to the hub's configuration and regenerate the API types, then one
`FieldSpec` in `src/features/settings/sections/`: `path` (dotted, as in `hub.toml`), a
sentence-case `label`, a `kind` (`text`, `number`, `bool`, `select`, `list`, `id`, `ids`,
`secret`, `model`, `json`), optionally `visibleWhen`. The renderers add the hint, the source
tag, the read-only state with its reason, and the validation issue. A new section is one
entry in `SECTIONS`. Outside Settings wrap inputs in `<Field label hint path="the.path">`.

**Add a table column.** One entry in `columns`: unique `key`; `label` (also the label when
cards stack on a phone); `cell` (a status is a `StatusPill`, a time a `RelativeTime`);
`numeric`; `hint` for an unclear header; `hideLabel` for an actions column. Make a column
`sortable` by keeping `sort` in page state, passing `onSort` and handing over sorted rows; a
paged list is sorted by the hub. Keep the `columns` array stable (`useMemo`).

**Add a tab.** `Tabs` for a few views of one object. Give each a `label`, `content`, an `id`
when the URL names it, and a `count` only for what needs attention. Keep the choice in
`?tab=<id>` (`useSearchParams`, `replace`, first tab = no parameter). Inside a panel start at
`Section`; the page title is the only `h1`. Only the selected panel renders.

**Guard unsaved work.** `useLeaveGuard(dirty)` (`features/notes/useLeaveGuard.tsx`): render
its `guard`, call `allowLeaving()` before an intentional navigation, show an "Unsaved
changes" `Tag` while dirty.

**Add a component to the kit.** One file with a JSDoc saying what it is for and not for;
tokens-only CSS module; an export in `index.ts`; a line in the list above; a demo in
`src/dev/` for every variant and state; a test. **Add a colour:** do not, unless no token
fits; then add the primitive and semantic token for light and both dark blocks and pass
`contrast.test.ts`.

## Before you open a PR

- [ ] Both themes and 375 px, checked on `/dev/design` and the page (`node scripts/screens.mjs`
      takes screenshots of every page at four widths in both themes)
- [ ] Keyboard only: tab order, visible focus, Escape closes overlays, no traps
- [ ] Loading, empty, error and offline states all exist and read right
- [ ] Every action answers back; every disabled control says why; every status is a pill
- [ ] Messages follow the voice rules; new components and states are in `/dev/design`
- [ ] `npm run lint && npm run typecheck && npm test && npm run build`
