# Web UI design guide

Read this before changing anything under `web/`. Rules come first, each with its reason in a
line. `/dev/design` (dev server only) renders every token, tone, status, icon and kit
component: check your work there in both themes.

## Principles

1. **Crisp, quiet, dense, precise.** Warm charcoal and paper neutrals, one green accent, thin
   borders, layered surfaces, small strong type. Colour is for status, the accent and the
   brand mark. Nothing decorative: no effect that does not make something clearer or faster.
2. **One grid.** Every size sits on a 4 px grid, every row of controls shares one height
   (`--control-h`), every column of text shares one left edge. If two things in a row are not
   the same height or do not share a baseline, it is a bug.
3. **Colour carries meaning, never alone.** Every coloured thing also has a word.
4. **Every action answers back** in plain words, where the person is looking: success,
   failure, progress, "nothing changed".
5. **Explain on demand.** Non-obvious things explain themselves on hover or focus; obvious
   things do not. Page intros are one short sentence; the long explanations live in an
   `InfoTip` or the docs.
6. **Never blank, never lying.** Every view has loading, empty, error and offline states. A
   failed request never reads as "nothing here".
7. **Fast.** Slow phone, VPN: no layout shift, no refetch storms, nothing from another origin.
8. **One source of truth per decision.** Colours, spacing, radii, shadows, sizes, motion and
   layers: `src/styles/tokens.css`. Statuses: `src/design/tone.ts`. Icons:
   `src/design/icons.tsx`. Error sentences: `src/design/messages.ts`. Nothing else decides
   them.

Motion has two classes. **Micro-interactions** (hover, focus, disclosure, menus, dialogs) are
120 to 180 ms. **Visualization and transitions** (graph nodes and edges, layout changes, side
panels, playback) are a separate class, specified under "Planned" below. Both ease out, explain a
change, and become instant under `prefers-reduced-motion` (the motion tokens become `0ms`: always
use them); state changes stay visible without motion.

## Brand roles, tokens, type

**Green acts** (`--color-accent`: links, the focus ring, selected items, a switch that is on,
progress) and fills the one primary button (`--color-primary`). **Walnut brands**
(`--color-brand`, `--color-on-brand`): the logo tile and, at most, a small marker such as the
active settings section. **Warm charcoal (dark) and paper (light)** is everything else,
including the sidebar: it is the page surface with a hairline border, never a coloured slab.
Success and accent green must differ at a glance: success is a soft pill with a dot, the
primary button is a solid fill. Never fill a button with a tone's `-solid`.

Surfaces stack from the page up: `bg` (the page), `surface` (cards, tables, inputs, the
sidebar), `surface-raised` (hover, selected navigation, table headers, tags),
`surface-overlay` (menus, popovers, dialogs, toasts; these also get `--shadow-overlay`). In
the dark theme each step is lighter; in the light theme overlay and surface are both paper.

- `tokens.css` is the only file with raw colours (hex, `rgb()`, named). Stylesheets use the
  semantic tokens; `--palette-*` primitives are for `tokens.css` only.
- The dark theme is written twice in `tokens.css` (explicit and "System"), between
  `dark-theme:start` and `dark-theme:end`. Change both; a test compares them. Tokens that
  only follow another token (`--color-accent`, `--color-sidebar-*`, `--color-accent-tint`,
  `--color-brand`) are written once, in the light block, and need no dark value.
- No component remaps tokens. If a part of the page needs another colour, it needs a token.

| Token                                                           | Use it for                                                                            |
| --------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `--color-bg`, `-surface`, `-surface-raised`, `-surface-overlay` | Page; cards, tables, inputs, sidebar; hover, selected, table header; floating layers  |
| `--color-border`, `-border-strong`, `-border-hover`             | Hairlines and card edges; edges needed to use a control (3:1); a hovered control edge |
| `--color-text`, `-text-muted`, `-text-subtle`                   | Body; meta lines, hints, times; placeholders, table header labels, shortcuts          |
| `--color-accent`, `-accent-hover`, `-accent-tint`               | Links, focus, selected indicator, switch on; their hover; a selected row              |
| `--color-primary`, `-primary-hover`, `-on-primary`              | The primary button fill, its hover, and the text on it                                |
| `--color-brand`, `-on-brand`, `--color-frame`                   | The logo tile and the text on it; walnut used as text or a small marker               |
| `--color-focus`, `--shadow-focus`, `--color-overlay`            | The focus ring colour; the 3 px accent ring on a focused input; the dialog scrim      |
| `--color-sidebar-*`                                             | Aliases of surface, border, text and accent for the sidebar                           |
| `--tone-<tone>-bg`, `-fg`, `-border`                            | A pill or banner: tint, text, edge (`-fg` also passes on the page)                    |
| `--tone-<tone>-solid`, `-on-solid`                              | Dots, stripes, counts and one page-level critical banner. Nothing else                |
| `--tone-danger-faint`                                           | The one row tint: `DataTable` `rowState="urgent"`                                     |
| `--space-1` … `--space-8`                                       | 4, 8, 12, 16, 24, 32, 48, 64 px: every gap, padding and margin                        |
| `--radius-xs`, `-sm`, `-md`, `-lg`, `-pill`                     | 4 px tags and keys; 6 px controls; 8 px cards and tables; 12 px dialogs; pills        |
| `--shadow-raised`, `--shadow-overlay`                           | A faint top highlight on a raised card; the ring and soft shadow of a floating layer  |
| `--control-h`, `-control-h-sm`, `--item-h`, `--row-h`           | 32, 28, 30 and 40 px; 40, 36, 40 and 48 px on touch screens                           |
| `--topbar-h`, `--sidebar-width`, `--sidebar-rail-width`         | 48 px, 240 px, 52 px                                                                  |
| `--content-max`, `--content-pad`                                | 1200 px page width; 32 px page padding (16 px on phones)                              |
| `--text-xs` … `--text-2xl`, `--leading-*`, `--tracking-*`       | Seven type steps and their line heights; heading letter-spacing                       |
| `--text-control`, `--weight-medium`, `--weight-semibold`        | Text size inside a control (16 px on phones); the two weights beyond regular          |
| `--font-sans`, `--font-mono`                                    | UI text; commands, logs, arguments, IDs and shortcuts                                 |
| `--z-sticky`, `-dropdown`, `-overlay`, `-toast`, `-tooltip`     | Layers. Never a bare `z-index` number                                                 |
| `--motion-fast`, `--motion-base`, `--ease-out`, `--scrim-blur`  | 120 ms, 180 ms, the one easing curve; the 2 px blur behind a dialog                   |
| `--motion-graph`, `--motion-graph-edge`, `--motion-layout`      | 240, 400 and 320 ms: graph node entry and status settle; edge reveal; relayout, fit   |

**Type.** One sans, Inter Variable, bundled with the app (`@fontsource-variable/inter`,
`src/styles/fonts.css`). It is served from the hub's own origin like every other asset, so
nothing leaves the network and the hub's content policy stays `default-src 'self'`. Only the
latin subset is shipped (one file of about 48 kB, cached across hub upgrades) and
`font-display: swap` shows the system sans at once, so text is never blocked. The stack is
`"Inter Variable", Inter, system-ui, -apple-system, "Segoe UI", sans-serif`, with the
`cv11`, `ss01` and `ss03` features on. Mono (`ui-monospace` first) is for machine text.

| Step   | Size  | Line | Used for                                          |
| ------ | ----- | ---- | ------------------------------------------------- |
| `xs`   | 12 px | 16   | Hints, meta lines, pills, tags, table headers     |
| `sm`   | 13 px | 18   | Buttons, field labels, nav items, banners, tables |
| `base` | 14 px | 20   | Body text on desktop                              |
| `md`   | 15 px | 22   | Lead text in a card or dialog                     |
| `lg`   | 16 px | 24   | Body on phones; dialog titles                     |
| `xl`   | 20 px | 28   | Page title on phones, auth titles                 |
| `2xl`  | 24 px | 32   | Page title on desktop, stat values                |

Headings are weight 600 with `--tracking-tight` (`--tracking-tighter` for the 24 px page
title). Field labels, buttons and table headers are weight 500; everything else is 400.
Phones use 16 px body text and 16 px inside inputs (iOS does not zoom into inputs at 16 px):
inputs read `--text-control`, which is already 16 px there. Sentence case everywhere, table
headers included. Use the `tabular-nums` class for numbers in tables, counts and times;
numeric columns are right-aligned in header and cell.

**Focus and links.** The global focus ring is 2 px accent with a 2 px offset; never remove an
outline. A focused input also shows the 3 px `--shadow-focus` ring and an accent border.
Links are accent, with no underline until hover; links inside tables and lists are text
coloured, underlined on hover. Text selection is `--color-accent-tint`. Scrollbars are thin
and use the strong border colour.

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

**Policy outcomes** (`POLICY_OUTCOMES` in `design/policyVocab.ts`, read through `tone.ts`) use the
tones above for what a person should do next, not for whether a rule is right:

- `Allow automatically` is `success`: the step runs without asking, the outcome that needs no one.
- `Ask for approval` is `pending`: a person decides, the same tone as every other wait for a person.
- `Deny` is `neutral`: nothing runs and nothing is offered. A Deny is intended, so it is not a fault.
- `Cannot decide` is `warning`: a safety input could not be confirmed, so the step stops and someone
  should look. It is not a failed step.

**Locked Deny** is the one rule mark with its own treatment: the `danger` tone, a lock icon and the
words "Locked Deny", never colour alone. A locked rule is an absolute prohibition and changing it
needs the password, so it reads as the strongest rule on the page.

**Pills, not paint.** A status is a pill: `-bg` tint (a low-strength wash), `-fg` text, a 6 px
dot in the tone's solid colour, then the label, with no heavy border. A row or card may carry
a 2 px left stripe (`toneStripe`). Whole rows are never tinted, except the faint danger tint
above. An unknown value (the hub may be newer) is a `neutral` pill with the raw value; never
throw on one.

`tone.ts` is the entry point for statuses (`verdictMeta`, `actionStatusMeta`, …); its tables
sit in `respondVocab.ts`, `actionVocab.ts` and `fleetVocab.ts`, split only to keep files
short. Import from `tone.ts`. Icons: `<Icon name="…" />` only, 16 px in a row of controls; to
add one, add an entry to `ICONS`. Theme: `ThemeSwitch` and `design/theme.ts` set
`data-theme`; a static inline script in `index.html` applies it before paint and the hub
allows it by hash, so keep it static.

## The shell

`src/app/` is the frame around signed-in pages; pages draw none of it.

- **Sidebar** (240 px, rail 52 px up to 1023 px): the surface colour with a 1 px right
  border. Head: a 24 px walnut logo tile (rounded 6 px, a light "D"), "Domovoi" at 14 px/600,
  and the collapse `IconButton`. Under it a full-width "Search…" button (28 px, muted text,
  magnifier, a `Kbd` "Ctrl K") that opens the command palette. Navigation: groups with an
  11 px/500 subtle title, items 30 px high with radius 6 and 8 px side padding, a 16 px
  muted icon and a 13 px label; hover is the raised surface; the active item has the raised
  surface, text colour and an icon in text colour (no stripe). Counts sit at the right.
  Footer: live status as a dot and a short word ("Live", "Reconnecting"), then the user row
  (24 px initial avatar, name, menu). The rail shows centred icons with tooltips on the
  right, and search as a magnifier `IconButton`.
- **Top bar** (48 px, page background, 1 px bottom border): a breadcrumb at the left (muted
  section, text-coloured detail, chevron separators); at the right the page's actions, then
  the host selector.
- **Host selector** (`useSelectedHost()`): a 32 px control with a server icon, the value
  ("All hosts" or a name), the chosen host's status dot and a chevron. Its popover has a
  search box (always above 8 hosts), "All hosts" with a count, a divider, then each host
  with a status dot, its name and its status in muted text, and a check on the chosen one.
  Opening a page about one host shows that host in the selector, so it never says "All hosts"
  on a page about `nas`; leaving the page brings back the person's own earlier choice.
- **Content**: at most `--content-max` wide, centred, `--content-pad` padding,
  `scrollbar-gutter: stable` so nothing shifts when a scrollbar appears.
- **Phones** (under 768 px): a top bar with the logo, the host selector and a menu button;
  the sidebar is a sheet.

Lazy pages live in `pages.ts`. Every signed-in page follows the host selector (filter on the
hub with `host`) or renders `<AllHostsNote>` saying why not; the setup, login, welcome and
not-found pages are outside the shell's host scope and need no note. Navigation counts come from
`useOverview(host)`: never fetch a list to count it.

The **Respond** group starts with "Control room" (`/`) and "Attention" (`/attention`). Attention
carries a danger count from `attention_total`, shown only above zero, and follows the host selector.
The command palette offers "Search everything" first for any typed text; it opens `/search?q=`.

The **Knowledge** group starts with "Search" (`/search`), then holds Known conditions, Notes, Digest and three more items: Knowledge
(`/knowledge`: facts, proposals, relations and entities), Management paths (`/knowledge/paths`) and
Experience (`/knowledge/experience`). None carries a count. Fact and entity pages sit under
`/knowledge` and read "Knowledge" in the breadcrumb.

## Presentation profiles

Every element of an operations-style screen has one category, and the detail level decides
from the category alone whether it is shown, collapsed or hidden.

| Category                                  | Detailed | Standard  | Essential                   |
| ----------------------------------------- | -------- | --------- | --------------------------- |
| P0 safety, approval, audit, current state | shown    | shown     | shown                       |
| P1 primary                                | shown    | shown     | shown                       |
| P2 technical detail                       | shown    | collapsed | collapsed                   |
| P3 metadata                               | shown    | collapsed | hidden, behind "+N details" |

The table is data in `src/design/visibility.ts` (`PRESENTATION`); nothing else decides.

1. **P0 is never hidden or collapsed**, in any profile, and not by a per-page override.
2. **Collapsed** is a labelled toggle with a count. The content is not rendered until it is opened.
3. **Hidden** is one "+N details" button per `DetailsScope`. A P3 block outside a scope is shown collapsed instead, so it is never lost.
4. **A P0 element never sits inside a P2 or P3 container.**
5. **P0 and P1 look the same** in every profile. Durable history (what was recorded) is never left out of the page.
6. **A profile is display only.** It never changes authorization, tools, audit, retention or which requests are made.
7. **One provider, everything else asks.** `VisibilityProvider` holds the choice; components call `useVisibility`. Dialogs, toasts, menus and tooltips are exempt: they carry no category.

**Custom** is a per-page override on top of the default level. The pages that offer one are
listed in `VISIBILITY_PAGES` (`operations`, `operation`, `policy`, `policy-draft`,
`policy-decision`, `policy-simulator`, `knowledge`, `knowledge-detail`, `paths`, `experience`); a
page names itself with `VisibilityPage`. The Knowledge pages use `knowledge` (the browser),
`knowledge-detail` (fact and entity pages), `paths` and `experience`. The resolution summary and
the similar-operations panel sit on the operation page and follow its level; the incident page
has no key of its own, so its panel follows the default level. **The default is Detailed**, for a new owner, an unknown value and any
failure to load the choice: it fails visible.

**Where to choose.** The account menu has a "Detail level" group (Detailed, Standard,
Essential) and "Display settings". Settings, Display holds the default level, the per-page
overrides, a table of what each level does and "Reset to Detailed"; it works also when a file
manages the settings, because it is a preference, not a setting.

**Where it is stored.** The choice is a per-user preference on the hub
(`GET` and `PUT /api/v1/me/preferences`), so it is the same in every browser. A browser cache
(`domovoi.visibility` in local storage) only avoids a flash of the wrong level on load; the
hub's answer wins.

**The guarantee.** A test renders each operations screen in every profile and asserts that
every `data-visibility="P0"` element is present and not inside a collapsed container
(`p0_visible_in_every_profile`).

### Classify an element

Ask in this order and stop at the first yes.

1. **P0** if it says what is happening now, what waits for the person, what authority applies or what was recorded; if it is a failure, unknown, paused, limited, stale, load-error, empty or loading state; or if it is a decision control (approve, deny, confirm, a plan code).
2. **P1** if it is the main content of the screen or a frequent action.
3. **P2** if it is diagnostic: raw input or output, a relation, history, a secondary action.
4. **P3** if it is an identifier, a time, a model parameter or other rarely needed metadata.

Rules for code:

- No P0 inside a P2 or P3 `Visible`.
- A decision control is P0.
- A P3 block sits inside a `DetailsScope`, placed right after the blocks it covers.
- A list row or table cell takes `visibilityAttrs(category, id)`, not a wrapper `div`.
- Dialogs, toasts, menus, tooltips and visually hidden live regions carry no category.
- A page that needs a different result changes that element's category, never `PRESENTATION`.
- `id` names the kind of block (`operation.banner`), never data.

Operations screens, from the code:

| Category | Elements (`id`)                                                                                                                                                                                                                                                                       |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0       | `operation.banner`, `operation.title`, `operation.controls`, `operation.progress`, `operation.scope`, `operation.waiting`, `operation.stale-note`, `operation.schema-warning`, `operations.state`, `operations.table`, `approval.card`, `incident.investigation-status`               |
| P1       | `operation.steps-head`, `operation.incidents`, `operation.budgets`, `operation.cycle-tag`, `operation.cycle-label`, `operation.earlier-steps`, `operation.scope-note`, `operations.filters`, `operations.legacy-tag`, `approval.reason`, `approval.origin`, `incident.investigations` |
| P3       | `operation.metadata`                                                                                                                                                                                                                                                                  |

Control room, attention and search:

| Category | Elements (`id`)                                                                                                                                                                                                        |
| -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0       | `control.tiles`, `control.attention`, `control.running`, `control.approvals`, `control.hosts`, `attention.list`, `attention.item` (through `visibilityAttrs`), `search.scope`, `search.status`, `search.history-label` |
| P1       | `control.hosts-link`, `search.box`, `search.incidents`, `search.operations`, `search.past`, `search.notes`, `search.facts`, `search.hosts`                                                                             |

Step inspector: `inspector.outcome`, `inspector.reason` and `inspector.approval` are P0;
`inspector.kind` and `inspector.summary` are P1; `inspector.intent`, `inspector.output` and
`inspector.connections` are P2; `inspector.times` is P3.

Replay (inside `operation.graph`): `operation.replay-toggle` and `operation.replay-bar` are P1;
`operation.replay-note` ("Replay: showing"), `operation.replay-gap` (the pruned and capped notes) and
`inspector.replay-note` ("Showing the live record") are P0, because each says what the graph is
showing or that the record is incomplete. The history's loading and failure states are P0.

Steps (`operation.step`, from `stepVisibility.ts`):

| Rule                                                                                            | Category         |
| ----------------------------------------------------------------------------------------------- | ---------------- |
| Kind `approval`, `execution`, `verification`, `question`                                        | P0               |
| Kind `assessment`, `plan`, `policy_decision`, `notification`, `note`                            | P1               |
| Kind `evidence_request`, `reasoning_turn`                                                       | P2               |
| Unknown kind                                                                                    | P1               |
| Outcome `failed`, `refused`, `not_run`, `unavailable`, `outcome_unknown`, or an unknown outcome | P0, for any kind |

In the tree view a step takes the most important category of itself and its descendants, so a
folded parent never hides a more important child. Consecutive collapsed steps fold into one
"N technical steps" toggle.

### Worked example: a policy screen

The policy screens are a safety screen, so every control a person needs to decide is P0. The
ids below are the ones the Policy pages use.

| Category | Policy elements (`id`)                                                                                                                                                                                                                                                                                                                                                                            |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0       | `policy.state`, `policy.mode`, `policy.shadow`, `policy.active`, `draft.validation`, `publish.validation`, `publish.findings` (the acknowledgement ticks), `publish.locks`, `publish.widening`, `publish.controls` (the Publish button and its password step), `decision.outcome`, `decision.locked`, `decision.why`, `decision.limits`, `decision.gaps`, `simulator.basis`, `simulator.widening` |
| P1       | `draft.actions`, `draft.checks`, `publish.notes`, `publish.simulation` (the summary tiles), `simulator.summary`, `simulator.results`, `simulator.form`                                                                                                                                                                                                                                            |
| P2       | Facts and inputs: `decision.facts`, `decision.inputs`                                                                                                                                                                                                                                                                                                                                             |
| P3       | Fingerprints and identifiers: `policy.fingerprint`, `publish.metadata`, `draft.metadata`, `decision.metadata`                                                                                                                                                                                                                                                                                     |

The acknowledgement ticks and the password step are P0 because they are the decision. Their
simulation tiles are P1, because they are the main content. The facts and recorded inputs a decision
used are P2 blocks, collapsed at Standard. The fingerprints are P3, placed inside a `DetailsScope`.

## Components

All in `src/components/`, exported from `index.ts`; pages import from that barrel only. If a
page needs something the kit lacks, add it to the kit, to this list and to `/dev/design`.

- **Status:** `StatusPill` (every status; 20 px, `sm` 18 px, radius pill, 12 px/500, dot then
  label; the hint is its tooltip), `BadgePill`, `Tag` (non-status labels: radius 4, raised
  background, muted 12 px), `Count` (a filled number: 18 px minimum, pill, 11 px/600 tabular,
  a solid tone or neutral raised), `toneStripe`.
- **Feedback:** `useActionFeedback` (every mutation), `useToast` (bottom right, overlay
  surface, radius 8, tone dot, title, detail, Undo as a ghost button), `InlineMessage` and
  `Banner` (radius 8, low-strength tint, 1 px tone border, 16 px icon, title 13 px/600, body
  13 px; never full saturation; a `Banner` is the one page-wide message), `InlineError`,
  `ErrorDetail`.
- **Hints:** `Tooltip` (inverted colours, 12 px, radius 6, 4 by 8 px; a few words, hover or
  focus), `InfoTip` (a sentence or two on click), `DocsLink`, `Kbd` (18 px, raised, bordered,
  radius 4, 11 px mono), `Highlight`.
- **Buttons:** `Button` (`variant`, `size`, `icon`, `loading`, `disabledReason`, `hint`; 32 px,
  radius 6, 13 px/500, 16 px icon, 6 px gap), `IconButton` (`label` is required: name and
  tooltip), `LinkButton` (navigation), `CopyButton`. Variants: `primary` (accent fill, the one
  main action of a view; it must read as the button on the page), `secondary` (surface,
  strong border), `ghost` (transparent, raised on hover), `danger` (destructive, solid red,
  white text). A running button shows `loading` in place of its icon and its width never
  changes. A button that cannot be used gets `disabledReason` (it stays focusable and says
  why); the bare `disabled` is a type error.
- **Loading, empty, error:** `QueryBoundary`, `Loading`, `Skeleton` (raised with a soft
  shimmer, off under reduced motion), `Spinner`, `Progress`, `LoadError` (what, why,
  Retry), `EmptyState` (a dashed box with a 40 px icon tile, a 14 px/600 title, a muted
  body up to 48 characters wide and the next step; never for a failed load).
- **Layout:** `PageHeader` (the only `h1`: 24 px/600, 20 px on phones; meta pills under it; a
  muted one-sentence intro of at most 70 characters per line with the docs link inline at
  its end; actions right-aligned and centred with the title), `Section` (`h2` at 14 px/600
  with an optional 13 px muted description, content 12 px below, groups 32 px apart, no
  rule), `Card` (surface, 1 px border, radius 8, 16 px padding; an interactive card
  brightens its border and raises on hover, never lifts), `Text`, `AuthCard`, `BrandMark`,
  `Stack`, `Cluster`, `Grid`.
- **Data:** `DataTable` (memoized rows: keep `columns` stable; a bordered radius-8 surface,
  header 36 px on the raised surface with 12 px/500 subtle labels, rows 40 px with a 1 px
  rule between and raised on hover, an actions column that hugs its content, 16 px left
  padding on the first column; rows stack as cards on phones), `Stepper`, `Timeline`,
  `KeyValueList`, `FilterChips`, `Tabs` (underline style, 13 px/500, the active tab in text
  colour with a 2 px accent underline, counts as `Count`), `RelativeTime`, `Countdown`,
  `CopyBlock`, `Markdown`, `ChatThread`, **`OperatorThread`** (a durable Operator conversation: turns with a status, live text for the pending answer replaced by the saved one, Stop, Retry on the last failed or stopped answer, and a composer; it is not `ChatThread`, which stays the incident follow-up), **`StatTile`** (a dashboard number: 12 px/500 muted label with an optional tone dot, a 24 px/600 tabular value, an optional hint line, and `to` to make it a link card).
- **Forms:** `Field` (around every input: a 13 px/500 label with badges inline to its right,
  a 12 px muted hint 6 px under the control, the error below), `TextInput`, `NumberInput`
  (digits right-aligned), `Textarea`, `Select` (custom chevron), `PasswordInput`,
  `Checkbox`, `Switch` (32 by 18 px track, accent when on, label and hint to its right),
  `SegmentedControl`, `Scrubber` (a position along a short sequence: a native range input with a
  visible label, `aria-valuetext` and a 44 px touch height, tokens only, no ticks, no tooltip; use it to
  pick one of a few dozen ordered moments, such as a replay position; not for a free number, which
  takes `NumberInput`, and not for a volume or a zoom; arrows, Home, End and Page keys move it as any
  range input does, and it needs `disabledReason` when disabled), `DropZone`, `FormErrorSummary`,
  `SubmitBar`. Text inputs, selects and
  textareas are 32 px (a textarea at least that), surface background, strong border, radius
  6, subtle placeholder; hover brightens the border (`--color-border-hover`), focus turns it
  accent and adds `--shadow-focus`.
- **Overlays:** `Dialog` (overlay surface, radius 12, 16 px/600 header, buttons right-aligned
  in the footer, a scrim of `--color-overlay` with a `--scrim-blur` blur), `ConfirmDialog`
  (names the object and the consequence; the button repeats the verb), `PasswordPrompt` (`title`, `reason`, `confirmLabel`, `error`, `busy`, `onConfirm`, `onCancel`, and
  `children` for what is being confirmed, such as a list of changes; the password lives only in the
  dialog and is never stored or logged),
  `Menu` and `OptionPicker` (overlay surface, 4 px inner padding, 30 px items with radius 6,
  hover raised, the chosen option shows a check at the right, shortcut hints at the right in
  subtle mono; menus are radius 8, popovers 12; optional `icon`, `caption` and `valueMeta`
  dress the picker's button), `Popover` (kit-internal), **`CommandPalette`** (`open`, `onClose`, `groups` of `{ title, items: { id, label, icon?, hint?, keywords?, onSelect }[] }`; a search combobox over a listbox; lazily loaded, `preloadCommandPalette()` warms it).

- **Visibility:** `Visible` (`category`, `id`; a P2 or P3 also takes `label`, `count`, `reveal`: draws a block as the detail level says; for a block of a screen, not for dialogs, toasts, menus or tooltips, and never around a P0 inside a P2 or P3), `DetailsScope` (groups the P3 blocks of one place under one "+N details" button; draws nothing when none is hidden), `Disclosure` (a labelled toggle with an optional `count` whose content is not rendered until opened; for technical detail or a run of rows, not for a page section, where `Section collapsed` serves, and not for anything a person must see), `VisibilityPage` (`page`: names the page for its per-page override; for the root of a page in `VISIBILITY_PAGES`, not for sections). Hooks: `useVisibility(category)` answers `shown`, `collapsed` or `hidden`; `useProfile()` answers the level in force here and whether it is a page override. `visibilityAttrs` marks a row or cell.

**Newer kit components.** All of them are built to this contract and join the kit, this list and
`/dev/design` together.

- `StatTile`: a label (12 px/500 muted), a value (24 px/600 tabular), an optional tone dot, an
  optional delta or hint line, and an optional `to` that turns it into a link card. For
  dashboards: a row of tiles in a `Grid`.
- `CommandPalette`: a dialog with a search input and grouped results (Pages: every navigation
  item; Hosts: from `useHosts`, opening the host; Actions: toggle theme, collapse the
  sidebar, log out). Opened by Ctrl/Cmd+K and by the sidebar search button; arrow keys move,
  Enter runs, Escape closes; it filters by substring and shows `Kbd` hints. It is its own
  lazily loaded chunk, so it never counts toward the first download.
- `StepGraph`: the graph of an operation's steps, described in "The step graph" below. It is
  its own lazily loaded chunk too.

### The step graph

`StepGraph` (`src/components/graph/`) draws the steps of an operation and the edges between them. The
kit knows no vocabulary: the feature builds a `GraphModel` (nodes with a resolved title, status
label, tone and icon, and plain-text summary; edges with a kind and a label) and passes it in.
`OperationGraph` (`src/features/operations/graph/`) is the wiring for one operation: it applies the
detail level, folds runs of technical steps into group nodes, and reads and writes `?step=`.
`GraphView` puts it above the same steps as a list.

Props of `StepGraph`: `model`, `selectedId`, `label` (the region's accessible name, default "Step
graph"), `animate` (default true; false makes every change instant), `rememberKey`, `onSelect`,
`onOpen`, `onClear`, `onToggleGroup`, `onFoldAll` (given only while a group is open; the toolbar
then shows "Fold technical steps") and `openerRef` (the focusable root, so closing the inspector
returns focus to the graph). The shell (`StepGraph`) and the engine (`GraphEngine`) are split: the
shell owns the toolbar, the keyboard, the live line and the edge key and is always in the page's
chunk; the engine owns layout and drawing, is the only importer of the library and loads on first
use. `preloadGraph()` warms it.

- **Library.** The graph is drawn by our own SVG renderer. Its layout comes from `@dagrejs/dagre`
  (exact version 3.1.3, MIT; it brings `@dagrejs/graphlib` 4.0.6, MIT). The library is imported only
  inside `src/components/graph/`, only by the engine and only through a dynamic import, so its code is
  a separate lazy chunk named `graph`. Nothing else may import it. Node text is SVG `<text>`, never
  HTML, so step text is shown as text and nothing in it is interpreted.
- **Inline style: the graph writes no `style` attribute (class S0).** Positions, sizes and
  transitions are SVG attributes (`transform`, `x`, `y`, `d`, `viewBox`) or tokens in a CSS module.
  Movement is animated by a `requestAnimationFrame` tween of the `transform` attribute whose duration
  is `motionMs(...)`. The ESLint rule that bans JSX `style` covers `src/components/graph/` and the
  smoke test fails if any element inside `[data-graph]` carries a `style` attribute (its allowance is
  the empty list). If a later change needs a custom property for computed geometry, it goes through
  one helper, `src/components/graph/geometry.ts`, which sets custom properties only and is then the
  only file there allowed to touch `style`; that is a design change and is written here first.
- **Workers and the policy.** No web worker is used. If one is ever added it must be a same-origin
  hashed file; `blob:`, `unsafe-eval` and `wasm-unsafe-eval` stay forbidden. The hub's
  Content-Security-Policy is unchanged, and the smoke test runs the page under the hub's real policy.
- **Motion.** Durations are `--motion-graph` (240 ms: a node entering, a status settling),
  `--motion-graph-edge` (400 ms: an edge next to an entering node, used sparingly) and
  `--motion-layout` (320 ms: relayout, opening or folding a group, fit to view). No transition is
  longer than 600 ms, and a burst animates at most five nodes: the rest appear in their final state
  at once, and a reload or reconnect animates nothing. Selection, focus and hover are instant. Under
  reduced motion the three tokens are 0 ms, so a `0` duration means "do not animate at all". Script
  never uses a literal duration: it calls `motionMs("graph" | "graph-edge" | "layout")` from
  `src/design/motion.ts`, which reads the token when an animation starts, so a change of the setting
  applies to the next one and a missing token reads as 0. The state is always visible without
  motion: the node's label, icon and outline change at once.
- **Edge key.** Edge kinds differ by line pattern, never by colour alone: a solid line is "depends
  on", dashes are "caused by", dots are "verifies", dash-dot is "retry of", long dashes are
  "answers". An unknown kind is drawn solid and thin, and shows its raw name. `GraphLegend` lists the
  kinds present in the graph as real DOM, with the line samples drawn as SVG attributes.
- **Live line.** A polite status line announces the focused or selected node: "Title, outcome:
  summary. Depends on 2 steps. Needed by 1 step." with " Selected." when it is the selection, and for
  a folded group "3 technical steps folded: summary. Press Enter to show them." It changes only when
  focus or selection moves, never on a data refresh.
- **Keyboard.** The graph root is one tab stop (nodes are not tab stops) and carries the help text
  as its description. Arrow keys never change the address; Enter and a click select.

  | Key                      | Effect                                                                             |
  | ------------------------ | ---------------------------------------------------------------------------------- |
  | Tab                      | Enters the graph at the selected step, the last focused one, or the active step    |
  | Arrow Right / Arrow Left | A step that depends on the focused one / a step it depends on                      |
  | Arrow Down / Arrow Up    | The next or previous node in the same column                                       |
  | Page Down / Page Up      | The next or previous step in time order (reaches every node)                       |
  | Home / End               | The first / last step in time order                                                |
  | `a`                      | The active step: running, else waiting for a person, else not started, else last   |
  | Enter                    | Opens a step (selects it and moves focus into the inspector); opens a folded group |
  | Escape                   | Clears the selection; with no selection it does nothing and is not consumed        |
  | `+` or `=` / `-` / `0`   | Zoom in / zoom out / fit to view                                                   |

  A key that moves nothing is silent. The toolbar ("Fit to view", "Zoom in", "Zoom out", "Fold
  technical steps" when offered) comes before the graph in the tab order. Dragging the background
  pans; Ctrl or Cmd with the wheel zooms; a plain wheel scrolls the page.

- **Failure.** When the chunk does not download or the engine throws, the graph and toolbar are
  replaced by a warning, "The graph could not be drawn", with Try again (a fresh import per attempt)
  and, after a second failure, "If it keeps failing, reload the page." The step list under the graph is
  always on the page, so the page stays usable and no approval, banner or inspector depends on the graph.
- **Replay.** Above the graph a **Replay** button opens the replay bar (`ReplayBar`, inside
  `GraphView`); closed is the default and fetches nothing. The bar is a group named "Replay":
  Previous moment, Play or Pause, Next moment, a Speed control (1�, 2�, 4�), Back to live and a
  `Scrubber` named "Replay position". A position is a moment (one whole second in which something was
  recorded) or Live, the last stop; the status line says "Moment i of n: clock. caption", or "Live". While a
  moment shows, the graph frame carries `data-replay="true"` and a note above it, "Replay: showing
  {clock}", says so; the step list, the banner, **Waiting for you** and the inspector stay live, and
  the inspector says "Showing the live record". With the graph focused, `,` is the previous moment,
  `.` the next, Space plays or pauses and `l` goes back to live; Escape is not used. The replay is
  rebuilt from the final record and the journal: creation and final outcomes are exact to the second,
  a step reads `running` only where the journal recorded it, and step text is always the final text.
  An incomplete history says so (the oldest records were removed, or the replay stops before the
  latest changes). Reduced motion has no autoplay: Play is disabled with its reason, and Previous and
  Next work at once. The replay code has no mutation hook and no approval path, and runs one timer
  per tick, never an interval.
- **Not a place to decide.** The graph has no approve, deny, resume or cancel control and imports no
  hook that sends one. Selecting a step opens the inspector, where a decision is made as everywhere.

Layout patterns. **List page:** `PageHeader` with `intro`, compact `FilterChips` (the
counts live on the chips, not also in the header), then `QueryBoundary` around a
`DataTable` or one bordered list with an `EmptyState`; in a list the whole row is the click
target. **Detail page:** `PageHeader` with `meta` and `actions` (visible secondary buttons;
only the dangerous one hides in a menu), then `Section`s or `Tabs`; facts as a `KeyValueList`
in a bordered card, two columns when wide. **Control room page:** four `StatTile`s (Needs you, Running now,
Waiting for approval, Hosts online), then two columns (the first five items that need the person,
and the operations running now; stacked when narrow), then the approval cards (the first three,
decided in place, with a link to the rest), then the hosts that need a look as a `Grid` of host
cards with a link to all hosts. **Attention centre page:** one `Section` per urgency tier (Act now,
Waiting for you, Worth a look), an empty tier is left out; a tier holds link rows (status pill,
title, one plain sentence on what to do, host, time) and, under Waiting for you, the shared
approval cards with Approve primary; a failed source is named above the list with one Retry.
**Search page:** the search box, one sentence on what is searched and who matches, then one
`Section` per source in a fixed order (incidents, active operations, past operations, notes,
facts, hosts); each shows five rows and "Show all", its own loading and error state, and a
coverage sentence where the source is partial; past operations carry the hub's "historical
evidence, not authorization" label under the heading. **Settings page:** a sticky section list at the left; each section is a
heading and description, then one bordered radius-8 card in which every setting is a row (the
label, source badge and hint on the left; the control, at most 320 px wide, on the right;
switches right-aligned; 1 px rules between rows; 16 px padding; rows stack under 900 px).
Unsaved changes show a floating bar at the bottom centre of the content (overlay surface,
radius 12: "3 unsaved changes", what saving does, Discard and Save) that is hidden when clean
and never covers a field; switching section moves nothing. **Dialog form:** `Dialog` with
`Field`s, the error inside the dialog, a primary verb button and Cancel. **Auth pages:** a
centred 360 px card on the page background, the logo tile above, a 20 px/600 title, fields
and one full-width primary button.

**Wizard.** A guided flow with a `Stepper`, one decision per step, and a review step that shows the
plan code (the first 8 hex characters of the hash the hub computed). Confirming opens a dialog that
asks for the password and the typed package name and submits the hash captured when the dialog
opened; a changed plan blocks the confirm. A progress page follows and polls while anything works.
The wizard has no switch, schedule or "apply to all" control.

**Read-only host view.** A page that shows what a host reported about itself (its unattended
limits, for example): it says who owns the data, says how to change it on the host, and never
offers a control. "Not reported" and "unreadable" read as information, never as permission.

**Policy editor.** The rules table sits beside an in-page rule editor, opened by `?rule=` in the
address so a rule can be linked. The page has one primary action at a time: Review and publish, or
Save while there are unsaved changes, which then takes the primary role from it. Unsaved changes
show the floating save bar at the bottom centre, as in Settings. The editor is never reachable only
through a modal: the table and the Add rule link open it in place.

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

Icon-only button: `IconButton` `label` (every icon button has a tooltip). Status:
`StatusPill`. Unclear button: `hint`. Disabled control: `disabledReason` (why, and how to
enable it). Relative time, truncated value, abbreviation: `Tooltip` with the exact value.
Unclear column header: the column's `hint`. A concept: `InfoTip` with `learnMore`. What an
input expects: the `Field` hint, in plain text (no markdown), with the default as the
placeholder. What a page is for: `PageHeader` `intro`, one sentence. Never the native `title`
(not on touch or keyboard). A tooltip shows after 400 ms on hover and at once on focus,
Escape closes it, and it never shows on touch: it may only repeat what is on screen. Do not
explain the obvious.

## How people use it

These are requirements, found by walking the app as the person who runs the servers. Each
one is checked in the browser, not in code.

**An alert links to an incident ("disk /srv is 93% full").** The person must understand and
act within ten seconds.

- Lead with the facts: what is measured now, the threshold, since when, how often it fired,
  the host (a link) and the check. The title is never repeated in a card, and the status and
  severity pills sit next to it once.
- One primary action per view. On an incident, Acknowledge is primary; Mute and Dismiss are
  secondary. An Approve button inside a step approval card is secondary (with the accent icon)
  while another primary is on screen. Legacy actions are read-only history and carry no
  Approve or Deny.
- A destructive step's Approve is a danger button that opens a `ConfirmDialog` naming the
  skill, the host, the arguments and the consequence. A write action shows its arguments
  before approval.
- An action card shows the skill name, the host, the risk pill, the arguments (key = value,
  mono), why it was proposed (the reason, when the hub has one), who suggested it and when,
  and the time left to decide. Not only the skill name.
- No control that cannot be used: with no model set up, the chat box is replaced by one line
  ("Set up a model to ask about incidents") linking to Settings, Models; "Run triage" is
  hidden, or says why it is off.
- Nothing is said twice on one page ("Skills proposed" under the proposed actions is gone).

**Morning check: is anything wrong?**

- The Control room answers it at a glance: what needs the person, what is running, what waits
  for approval, hosts online, and the first few items of each.
- An incompatible host is its own state with its own word and reason; it is never shown as
  offline, in a tile hint, a card or a row.
- A count appears once per view: on the filter chip, not also in the header and the heading.
- In a list, the whole row is the click target (for the keyboard the title is the link; the
  row has a pointer cursor and a hover state and forwards the click).
- An empty column shows "–" with a tooltip saying why, or is hidden when no row has a value.
- Links inside tables are not underlined until hover.

**Decide an approval from the attention centre.** The attention centre shows the same approval
card as the operation page and the Control room, so the decision is made in one familiar place with
the same confirmation. Approve is the primary button there and secondary in the Control room,
where the page is a summary. Rows that are not approvals link to the place of decision: the
operation, the incident or the host.

**Approve a fix.** A step approval card follows the rules above and is decided on its operation's page. The Approvals page does not yet gather pending steps across operations: its "Waiting for you" area is an empty state that points to Operations. "Recently decided" shows the decision, who decided and when, in a compact list, and legacy actions appear there read-only.

**Choose how much to see.** The default level is Detailed, so nothing is missing for a new
owner. Someone who finds a screen busy picks Standard or Essential from the account menu, or
sets one page apart in Settings, Display. What needs the person (an approval, a failure, the
current state) is on screen at every level; the rest is one click away, behind a labelled
toggle or a "+N details" button. The choice follows the person to every browser.

**See how an operation unfolded.** On a screen wide enough for it the Steps section opens as a
graph of the steps, with the same steps as a list right under it; narrower screens open the
timeline, and the View switch lets the person pick any of the three (the choice lasts until they
leave the page). Moving around the graph with the arrow keys only moves focus, it does not select
anything or open anything: Enter, or a click, opens the step in the inspector, and Escape closes
it and brings focus back to the graph. Selecting a step in the list selects it in the graph and the
other way round. In Standard and Essential the technical steps (model turns, evidence) fold into
one node that opens with Enter or a click, and a step that needs the person is never folded.

**Check what Domovoi knows.**

- Facts show where each one came from (you, a host, an integration or Domovoi's own inference),
  its status and when it was last seen or checked. A fact Domovoi inferred is not yours until you
  accept it: those wait on the Proposals tab and a note on the Facts tab says how many.
- Retracted and replaced facts are hidden until you pick their status. Stale and disputed facts
  stay in the list with their label; a disputed pair is settled by choosing the right value.
- Accept, verify and retract are on the fact. Retracting asks first and never needs the password.
  Accepting, verifying or adding a management path or a trusted network asks for the password
  before anything is sent.
- Past operations are labelled "historical evidence, not authorization" wherever they appear.

**Check the management paths.**

- The page is per host and shows each path's state, where it came from and how fresh it is.
- Confirming a path needs the helper's own check, which this version does not have. The page says
  so, shows Confirm disabled with that reason and sends nothing. You can still add a path, accept
  one Domovoi found and retract a wrong one.
- The blocked line lists why unattended containment is blocked on the host. It is a reading of what
  the hub reports, not a control, and it says monitoring and actions you approve yourself are not
  affected. When nothing blocks it, the page says other conditions still apply.
- No path is ever called covered or protected here.

**Change the policy.**

- Start a draft from the active version. Edits stay in the draft until you publish, and the active
  version never changes under you.
- Saving a locked rule asks for the password first and shows what it would change. Nothing is stored
  before the person confirms.
- Check rules changes nothing. It shows the problems, the conflicts and the effect on recorded
  decisions; saving and publishing are separate steps.
- Review and publish: acknowledge each conflict, then publish with the password. A published version
  is fixed.
- Decisions are recorded for comparison and are not enforced. Every recorded answer says so.

**Replay how an operation unfolded.** Press Replay above the graph, then drag the "Replay position"
slider or step with Previous moment and Next moment; Play runs the moments at 1�, 2� or 4�, and Back
to live returns. The graph shows the operation at that moment and a note says so; everything else on
the page, including approvals, is live. The replay is honest about what it knows: it is rebuilt from
the final record and the journal, so only creation, `awaiting_approval`, `dispatched` and final outcomes
are placed in time, and a step that was running shows `running` only where that was recorded. When the
history is incomplete, a note says which end is missing. With reduced motion there is no autoplay.

**Install a package.** Import the owner-signed archive and its signature file on the Packages tab of
the Capability Center, then choose the package and the hosts. Review the plan code, then confirm
with your password and the package name typed by you. Watch each host on the progress page; a retry
needs a fresh approval, and a result marked unknown means: check the host first. Installed is not
enabled: nothing here turns a package's skills on.

**Check one host.**

- Anything scoped to one host shows that host in the selector, and leaving restores the earlier choice.
- Host actions (revoke, enroll, …) are visible secondary buttons in the header; the dangerous
  one sits in a menu, not behind a lone "…".
- Skills with "All hosts" shows one card per host; a host only the hub knows sorts last and its
  card is not a link, with the reason as text.

**Change a setting.**

- The save bar appears only with unsaved changes and never covers a field.
- A field shows its default as the placeholder; hints are plain text.
- Switching sections never moves the navigation or the page sideways.

**Everywhere.** Nothing is cut off at 375, 768, 1280 and 1920 px, and the page never scrolls
sideways. Every control in a row has the same height and baseline, and every page has the same
left edge. Every focusable thing shows a focus ring. The "Live" indicator is truthful: if the
event stream really drops, the UI says "Reconnecting", and the fault is fixed in the hub, not
hidden here.

## Right-hand slot and Operations

One right-hand slot is shared by every side panel. A surface registers under its own id and is opened or closed through the slot API; the slot keeps the open surfaces oldest first, so the last one is the most recent. When the page keeps at least 640 px (`--right-slot-content-min`) beside the slot, the panel sits beside the page at 400 px (`--right-slot-width`). Otherwise the panel opens as a sheet and the page behind it is inert. Escape closes one thing per press, the front-most panel first.

- `SidePanel` is the chrome of one panel: heading, header actions, close button and a body that scrolls on its own. It knows nothing about the slot.
- `ApprovalCard` is the one approval card for steps and actions. It takes `extras` (the effect, policy and refusal notes) and `readOnly`, which hides the decision buttons. A destructive or unknown-risk approval is always drawn as danger, and needs the skill name typed exactly before Approve works.
- `--motion-panel` (240 ms, `transform` only) drives opening and closing. It is zero under `prefers-reduced-motion`.

The operation detail page is laid out in this order: the header, one banner at a time, a **Waiting for you** section when a person must act, the steps, then the details. Choosing a step opens the step inspector in the slot.

## Planned (added with the operation UI)

Nothing in this section exists in the code yet, and no test reads it. It records the agreed
direction from `docs/design-pack` so work on operations, the graph and the new pages starts from
one set of rules. Each item moves into the sections above in the PR that builds it, together
with its tokens, kit component, `/dev/design` demo and test.

**Visualization and transition motion** (a second class beside the 120/180 ms tokens).

| Planned token / behavior                 | Value                    | Use                                                              |
| ---------------------------------------- | ------------------------ | ---------------------------------------------------------------- |
| `--motion-panel` (side panel, inspector) | 240 ms, `transform` only | Open and close; content is padded, never reflowed mid-transition |

- Recommended 240 to 400 ms. A single transition never exceeds 600 ms; a test reads the tokens
  and fails above it.
- A JS reader of the same tokens returns 0 under reduced motion, so script-driven motion obeys it.
- Event bursts are batched: animation never delays showing state, at most five nodes animate at
  once, the rest appear in their final state. "Once" emphasis is remembered by node id and state
  version, so a reload or reconnect does not replay it.
- Reduced motion: transitions are instant, state changes stay visible (label, icon, outline, text),
  and no information is carried only by motion. No page transition on route change; lazy pages
  show their skeleton in place.

**Shell additions.** One right-hand slot shared by the Operator panel and the inspector: side by
side when the page keeps at least 640 px of content, otherwise the surface opened most recently wins
and the other collapses to a tab; a full-screen sheet on phones; never over content. Escape closes
one thing per press: tooltip, popover or menu; dialog or palette; the panel or inspector with
focus; then the graph selection. An autonomy-state indicator joins
the shell.

**New kit components** (`OperatorThread` is in the kit; the Operator is a surface of the right slot, and its `turn status` vocabulary is `Answering`, `Answered`, `Not answered`, `Stopped`) (each added to the kit, this list and `/dev/design` together): an inspector; a split pane with a keyboard-operable divider; a
virtual list; the one structured approval card (contract, target, arguments, effect class,
reversibility, expiry, plan-hash prefix, a label for which plan hash the approval binds ("v1 binding" or "v2 binding" during the migration, RW-11), policy decision, an agent refusal reason such as a rate limit, and the unverified human variant for containment without verified helper coverage (reason, NOT PROTECTED path list, step-up then typed confirmation, RW-37); model text secondary and labelled); a
freshness label.

**Freshness.** `QueryBoundary` gains refreshing, stale and refresh-error states and a "last
updated" label, so a failed refetch no longer hides behind old data. "Live" will describe the
event stream the page uses, not only incidents and actions.

**New statuses** (operation, node including uncertain and planned, capability
install, enable and authorize, and the step outcomes `Refused`, `NotRun`,
`Unavailable` and `OutcomeUnknown`) follow the "Add a status" recipe, each with a label, tone,
hint and icon. More are planned. The knowledge statuses are in `design/knowledgeVocab.ts` and read
through `tone.ts`: `knowledgeStatusMeta` (current, stale, disputed, replaced, retracted),
`pathStateMeta` (not yours yet, waiting for you, confirmed, stale) and `coverageMeta`, which has two
states only, "Cannot be verified yet" and "Checked when you confirm". There is no state that reads
as covered. Words for sources, predicates and relations are plain lookups in
`design/knowledgeText.ts`, not statuses. The host state **`Incompatible`** (agent upgrade required:
the agent speaks another protocol minor than the hub, D-20) is its own status with an
upgrade-required hint and is never shown with the offline status; its banner and host page keep
history readable. It is read from the persisted agent record, so it survives a hub restart, and
after a hub upgrade a host that has not reconnected reads "expected incompatible" (RW-17). The
privilege states (D-29, RW-39) are statuses too: a **root-agent warning** (the agent runs as root,
the privilege boundary is not in effect, no unattended execution; shown as a banner and on the host
page), **skill runner unavailable** (`skill_runner_status` is `absent`, `unsupported{reason}` or `error{reason}`, RW-42: every skill is `Unavailable(skill_runner_*)` with the reason, and skills never fall back to the control agent)
and **helper unavailable** (the root-owned helper is absent or in error; shown on each
affected check and capability, never as a failure or as offline).
The host page shows the helper's verified effective set (computed and signed by the helper, relayed
unchanged by the agent, never an agent-reported summary; the UI has no second editable list)
read-only beside the confirmed management paths, each discovered path marked covered or uncovered
judged only against verified data, with the attestation's freshness, an unverified state when no
fresh verified report exists, and the instruction to edit `helper.toml` on the host (then Re-check)
for an uncovered path (RW-37). The enrollment screen shows the received helper key's full
fingerprint as unverified until the owner confirms it against the host's own printout (nothing is
pinned automatically; the key id is a label); a changed key reads "helper key changed: re-enroll" and blocks
containment; the helper's fail-closed reasons (`state_missing`, `state_corrupt`, `state_key_mismatch`,
`key_missing`, `uninitialized`, `generation_regressed`) show with the matching local action; a
containment step the helper refuses because its safety identity changed carries the reason
`safety_changed`. The owner confirms only covered paths. The Capability Center
reserves the Helper tab and `/capabilities/helper-key` as labelled placeholders until the hub
collects helper data.

## Checks that will fail your PR

| What                                                                                                                                                  | Command         | Fix                                                                              |
| ----------------------------------------------------------------------------------------------------------------------------------------------------- | --------------- | -------------------------------------------------------------------------------- |
| Raw colour or `--palette-*` outside `tokens.css`                                                                                                      | `npm test`      | Use a semantic token (`styles/cssGuard.test.ts`)                                 |
| The two dark theme blocks differ                                                                                                                      | `npm test`      | Make them identical                                                              |
| A token pair below WCAG AA (text 4.5, UI 3)                                                                                                           | `npm test`      | Adjust the token (`styles/contrast.test.ts`)                                     |
| A status without label, tone, hint, icon                                                                                                              | `npm test`      | Add it to its vocabulary in `src/design/`                                        |
| A hub error or failure without words                                                                                                                  | `npm test`      | Add it to `messages.ts`                                                          |
| "Oops", "Something went wrong", "Error:", "!" in text                                                                                                 | `npm test`      | Say what happened and what to do (`voice.test.ts`)                               |
| An axe violation in whatever a page test leaves on screen                                                                                             | `npm test`      | Fix the markup; `skipAxe()` only for a deliberate fragment                       |
| Inline JSX `style`                                                                                                                                    | `npm run lint`  | A CSS module with tokens; computed values need a disable comment                 |
| JSX `title=`                                                                                                                                          | `npm run lint`  | `Tooltip`, `InfoTip` or visible text                                             |
| A raw `<button>` in `src/features` or `src/app`                                                                                                       | `npm run lint`  | `Button` or `IconButton`                                                         |
| A verdict string literal (`"ACTION_NEEDED"`) in page code                                                                                             | `npm run lint`  | `verdictMeta` and `StatusPill`                                                   |
| `disabled` on a control without `disabledReason`                                                                                                      | `npm run lint`  | Pass `disabledReason`; or disable the line and name the hint                     |
| `.mutate(` in a file with no `useActionFeedback`/`InlineError`                                                                                        | `npm run lint`  | Wrap it; or `// feedback-ok: where it is reported` at the top                    |
| A deep import of a kit file; `lucide-react` outside `Icon`                                                                                            | `npm run lint`  | Import from the barrel; use `Icon`                                               |
| `fetch` outside `src/api`                                                                                                                             | `npm run lint`  | A hook in `src/api/hooks/`                                                       |
| The graph library imported outside `src/components/graph/`                                                                                            | `npm run lint`  | Only the engine imports it, through a dynamic import                             |
| Inline `style` in `src/components/graph/`                                                                                                             | `npm run lint`  | SVG attributes or a CSS module with tokens                                       |
| A source file over 250 lines (no exemptions)                                                                                                          | `npm run lint`  | Split by responsibility                                                          |
| Initial JS over budget, a chunk over 120 kB, CSS over 30 kB                                                                                           | `npm run build` | Load it lazily or drop the dependency (`npm run size`)                           |
| No graph chunk, or it is in the first download or statically imported by an initial chunk, or it imports more than `vendor`                           | `npm run build` | Import the library only through the engine's dynamic import (`npm run size`)     |
| A graph motion token above 600 ms, or not 0 ms under reduced motion                                                                                   | `npm test`      | Fix the token in `tokens.css` (`styles/motion.test.ts`)                          |
| A P0 element missing or collapsed in a profile                                                                                                        | `npm test`      | Take it out of the collapsed container (`p0_visible_in_every_profile`)           |
| Visibility primitive in approval, banner or API code                                                                                                  | `npm test`      | Remove it: the profile never gates authority                                     |
| An approval control, a visibility primitive in publication code, a stored or logged password, or a whole policy label literal under `features/policy` | `npm test`      | Remove it. Gate tests: `src/features/policy/gates.test.ts`                       |
| A placeholder helper view that gains a helper request or data                                                                                         | `npm test`      | When the hub gains helper endpoints, replace the placeholder and update the test |
| An install job created or retried outside the two authorization dialogs                                                                               | `npm test`      | Only the confirm buttons of the install and retry dialogs may call them          |
| A broken operations page, slot, focus, graph or contrast in a browser, or any violation of the hub's real Content-Security-Policy                     | `npm run smoke` | Fix the page; failure screenshots go to `web/smoke-out`                          |

The lint rules live in `eslint/domovoi-rules.js`; `src/guardrails.test.ts` proves they fire.

## Performance budget

<!-- budget:initial-js-gzip-kb 185 -->

| Measure (gzip, `npm run size`) | Main before the redesign | Now      | Limit                                                                  |
| ------------------------------ | ------------------------ | -------- | ---------------------------------------------------------------------- |
| Initial JavaScript             | 179.0 kB, one chunk      | 179.8 kB | 183 kB (policy, knowledge and package words are eager; pages are lazy) |
| Largest chunk                  | 179.0 kB                 | 111.7 kB | 120 kB                                                                 |
| Total CSS                      | 9.9 kB                   | 26.8 kB  | 30 kB                                                                  |

The Inter font file (latin subset, about 48 kB) is not counted by `npm run size`. It loads
from the hub's own origin, is cached across upgrades, and uses `font-display: swap`, so it
never delays the first paint.

Measured on a throttled phone (4× CPU, 1.6 Mbit/s, 150 ms): login LCP 1.4 s, CLS 0.000,
TBT 27 ms; incidents LCP 1.4 s, CLS 0.021, TBT 0 ms. Incidents, Login and Setup are in the
first download; every other page is in `pages.ts` and fetched when opened (a link fetches on
hover or focus). Markdown is its own chunk; `vendor` is split so it stays cached across hub
upgrades. A mutation refreshes only what it changed; polling stops when nothing can change.
Bundle size after this change: initial JS 183.1 kB gzip, largest chunk 111.7 kB, CSS 29.4 kB gzip (within budget). The initial JS grew by 2.4 kB because the kit now carries the graph wrapper (lazy loading, failure fallback), three toolbar icons and the right slot's `focus`; the budget rose from 183 to 185 kB for that.

Bundle size after the replay bar: initial JS 183.5 kB gzip, largest chunk 111.7 kB, graph chunk 15.7 kB, CSS 29.4 kB (within budget).

The step graph is its own lazy chunk (`graph-<hash>.js`, the `@dagrejs` packages), fetched when a
graph is first shown, so it is outside the first download and inside the 120 kB chunk limit.
Graph chunk: 15.7 kB gzip (graph library and its layout code only); the engine and shell are two more small lazy chunks (4.5 kB and 3.2 kB gzip).

Raise the budget only with a reason in the PR, and record the new number above.

## Recipes

**Add a page.** (1) Create it in `src/features/<area>/`, start with `<PageHeader title
intro docs />`. (2) Add it to `PAGES` in `src/app/pages.ts` and a `<Route>` with
`<PAGES.name.Page />`; never import it into `routes.tsx` (it would join the first
download). (3) Load data with a hook from `src/api/hooks/` inside `QueryBoundary` with a
`context`, an `EmptyState` and a `Skeleton`. (4) Follow the host selector or render
`<AllHostsNote>`. (5) Lay out with `Section`, `Stack`, `Cluster`, `Grid`, `Card`,
`DataTable`; its CSS module only places things. (6) A page about one thing calls
`usePageName(thing.title)`. (7) Check 375, 768, 1280 and 1920 px, both themes, keyboard.

**Add a navigation item.** One entry in `NAV_GROUPS` (`src/app/nav.ts`): `to`, `label` (one
or two words: on the rail it is the tooltip), `icon`, `preload: PAGES.name.preload`. For a
count, add the field to the hub's overview (`GET /api/v1/overview`), regenerate the API
types, and set `count` (field, tone, the word after the number). The command palette lists
every navigation item, so nothing else needs registering.

**Add a status.** (1) Add the value to its type and table in `src/design/` (`respondVocab`,
`actionVocab` or `fleetVocab`) with label, tone, a hint saying what it means, an icon; a new
vocabulary also goes in `VOCABULARIES` and gets a lookup in `tone.ts`. (2) `npm test` covers
it and `/dev/design` shows it. (3) Render `<StatusPill meta={…Meta(value)} />`; a row takes
its stripe from the same meta. (4) A new timeline kind goes in `KINDS` in `timeline.ts`.
(5) Filtering on it is the hub's job: add the filter to the endpoint, never filter loaded
pages in the browser.

The package tables (`design/packageVocab.ts`) are vocabularies too: install target, install job,
host package, key state, file status, privilege boundary and ceiling reporting. Failure codes of an
install are worded in `design/packageFailures.ts`, read through `describeTargetFailure`.

The policy tables (`design/policyVocab.ts`, `policyLimits.ts`, `policyRecord.ts` and `policyMeta.ts`)
are vocabularies too. Add a value there and read it through `tone.ts`; a feature never writes a
policy label itself. The same goes for `design/knowledgeVocab.ts`.

**Ask for the password.** Use `useStepUp` (`features/knowledge/useStepUp.tsx`) over the kit's
`PasswordPrompt` whenever the hub demands the password again. Call `start(variables, needsPassword)`:
it runs the action at once, or opens the prompt first. The password lives only in the prompt and
in the one request, never in a URL, a key, storage or a toast.

```tsx
const accept = useActionFeedback(useAcceptFact(), { success, error, errorDisplay: "inline" });
const stepUp = useStepUp({
  confirmLabel: "Accept",
  reason: () => "This kind of fact can affect protective actions. Enter your password to save it.",
  run: (v, password, callbacks) => accept.mutate({ ...v, password }, callbacks),
  busy: accept.isPending,
  error: accept.error,
  reset: accept.reset,
});
// render stepUp.dialog once; stepUp.inline is a failure to show next to the button
```

A 403 inside the prompt reads "That password is not right." and the prompt stays open; a 429 shows
the wait; any other failure shows the hub's words. A form dialog that needs the password is replaced
by the prompt with its draft kept, never stacked under it. Whether to ask is a rule in
`design/knowledgeText.ts` (`needsPassword`): safety-related facts, and every confirmation.

**Show knowledge.** Write a subject in words with `useSubjectLabels` and `subjectLabel` (a host
name, "plex on nas", an entity name; the id is never assumed to be a UUID), a value with
`valueText`, a source with `SOURCES` and a status with `knowledgeStatusMeta`. All of them accept a
value they do not know and show the raw text. Fact values, notes, names and evidence are text, never
HTML or Markdown; only evidence of a known kind with a plain id becomes a link.

**Show an action's result.** The worked examples are `useIncidentActions.ts` and
`StepApproveControl.tsx`.

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
tag, the read-only state with its reason, and the validation issue, laid out as a settings
row. A new section is one entry in `SECTIONS`. Outside Settings wrap inputs in
`<Field label hint path="the.path">`.

**Add a display choice.** A per-person display choice (the detail level, or a later one such as density) is a hub preference under `me/preferences`, not a settings field: it is never in `hub.toml`, never locked by a file and never restarts the hub. Add it to the preferences body and the Display section, not to `SECTIONS`.

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

- [ ] Both themes, checked on `/dev/design` and the page, at 375, 768, 1280 and 1920 px
      (`node scripts/screens.mjs` takes screenshots of every page at four widths in both
      themes): nothing cut off, no sideways scroll, controls in a row the same height
- [ ] Keyboard only: tab order, a visible focus ring, Escape closes overlays, no traps
- [ ] Loading, empty, error and offline states all exist and read right
- [ ] Every action answers back; every disabled control says why; every status is a pill
- [ ] Messages follow the voice rules; new components and states are in `/dev/design`
- [ ] `npm run lint && npm run typecheck && npm test && npm run build`
