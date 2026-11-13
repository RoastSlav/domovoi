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
  `CopyBlock`, `Markdown`, `ChatThread`, **`StatTile`** (a dashboard number: 12 px/500 muted label with an optional tone dot, a 24 px/600 tabular value, an optional hint line, and `to` to make it a link card).
- **Forms:** `Field` (around every input: a 13 px/500 label with badges inline to its right,
  a 12 px muted hint 6 px under the control, the error below), `TextInput`, `NumberInput`
  (digits right-aligned), `Textarea`, `Select` (custom chevron), `PasswordInput`,
  `Checkbox`, `Switch` (32 by 18 px track, accent when on, label and hint to its right),
  `SegmentedControl`, `DropZone`, `FormErrorSummary`, `SubmitBar`. Text inputs, selects and
  textareas are 32 px (a textarea at least that), surface background, strong border, radius
  6, subtle placeholder; hover brightens the border (`--color-border-hover`), focus turns it
  accent and adds `--shadow-focus`.
- **Overlays:** `Dialog` (overlay surface, radius 12, 16 px/600 header, buttons right-aligned
  in the footer, a scrim of `--color-overlay` with a `--scrim-blur` blur), `ConfirmDialog`
  (names the object and the consequence; the button repeats the verb), `PasswordPrompt`,
  `Menu` and `OptionPicker` (overlay surface, 4 px inner padding, 30 px items with radius 6,
  hover raised, the chosen option shows a check at the right, shortcut hints at the right in
  subtle mono; menus are radius 8, popovers 12; optional `icon`, `caption` and `valueMeta`
  dress the picker's button), `Popover` (kit-internal), **`CommandPalette`** (`open`, `onClose`, `groups` of `{ title, items: { id, label, icon?, hint?, keywords?, onSelect }[] }`; a search combobox over a listbox; lazily loaded, `preloadCommandPalette()` warms it).

**Newer kit components.** Both are built to this contract and join the kit, this list and
`/dev/design` together.

- `StatTile`: a label (12 px/500 muted), a value (24 px/600 tabular), an optional tone dot, an
  optional delta or hint line, and an optional `to` that turns it into a link card. For
  dashboards: a row of tiles in a `Grid`.
- `CommandPalette`: a dialog with a search input and grouped results (Pages: every navigation
  item; Hosts: from `useHosts`, opening the host; Actions: toggle theme, collapse the
  sidebar, log out). Opened by Ctrl/Cmd+K and by the sidebar search button; arrow keys move,
  Enter runs, Escape closes; it filters by substring and shows `Kbd` hints. It is its own
  lazily loaded chunk, so it never counts toward the first download.

Layout patterns. **List page:** `PageHeader` with `intro`, compact `FilterChips` (the
counts live on the chips, not also in the header), then `QueryBoundary` around a
`DataTable` or one bordered list with an `EmptyState`; in a list the whole row is the click
target. **Detail page:** `PageHeader` with `meta` and `actions` (visible secondary buttons;
only the dangerous one hides in a menu), then `Section`s or `Tabs`; facts as a `KeyValueList`
in a bordered card, two columns when wide. **Overview page:** a row of `StatTile`s, then two
columns (what needs attention, what waits for approval; stacked when narrow), then a `Grid`
of host cards. **Settings page:** a sticky section list at the left; each section is a
heading and description, then one bordered radius-8 card in which every setting is a row (the
label, source badge and hint on the left; the control, at most 320 px wide, on the right;
switches right-aligned; 1 px rules between rows; 16 px padding; rows stack under 900 px).
Unsaved changes show a floating bar at the bottom centre of the content (overlay surface,
radius 12: "3 unsaved changes", what saving does, Discard and Save) that is hidden when clean
and never covers a field; switching section moves nothing. **Dialog form:** `Dialog` with
`Field`s, the error inside the dialog, a primary verb button and Cancel. **Auth pages:** a
centred 360 px card on the page background, the logo tile above, a 20 px/600 title, fields
and one full-width primary button.

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
  secondary. An Approve button inside an action card is secondary (with the accent icon)
  while another primary is on screen; on the Approvals page each card's Approve is that
  card's primary.
- A destructive action's Approve is a danger button that opens a `ConfirmDialog` naming the
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

- The overview answers it at a glance: what needs attention, open incidents, what waits for
  approval, hosts online, and the first few items of each.
- A count appears once per view: on the filter chip, not also in the header and the heading.
- In a list, the whole row is the click target (for the keyboard the title is the link; the
  row has a pointer cursor and a hover state and forwards the click).
- An empty column shows "–" with a tooltip saying why, or is hidden when no row has a value.
- Links inside tables are not underlined until hover.

**Approve a fix.** An action card follows the rules above. Under a "Waiting for you" heading
the "Waiting for approval" pill is redundant, so it is not drawn. "Recently decided" shows the
decision, who decided and when, in a compact list.

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

## Planned (added with the operation UI)

Nothing in this section exists in the code yet, and no test reads it. It records the agreed
direction from `docs/design-pack` so work on operations, the graph and the new pages starts from
one set of rules. Each item moves into the sections above in the PR that builds it, together
with its tokens, kit component, `/dev/design` demo and test.

**Visualization and transition motion** (a second class beside the 120/180 ms tokens).

| Planned token / behavior                            | Value                    | Use                                                              |
| --------------------------------------------------- | ------------------------ | ---------------------------------------------------------------- |
| `--motion-panel` (side panel, inspector)            | 240 ms, `transform` only | Open and close; content is padded, never reflowed mid-transition |
| `--motion-graph` (node entry, state, status settle) | 240 ms                   | Graph nodes and status changes                                   |
| `--motion-graph-edge`                               | up to 400 ms             | Active-path reveal, used sparingly                               |
| `--motion-layout`                                   | 240 to 400 ms            | Group collapse and expand, re-layout, fit to view                |
| Playback                                            | user-controlled speed    | Replay of a completed operation; the last U2 deliverable (D-25)  |

- Recommended 240 to 400 ms. A single transition never exceeds 600 ms; a test reads the tokens
  and fails above it.
- A JS reader of the same tokens returns 0 under reduced motion, so script-driven motion obeys it.
- Event bursts are batched: animation never delays showing state, at most five nodes animate at
  once, the rest appear in their final state. "Once" emphasis is remembered by node id and state
  version, so a reload or reconnect does not replay it.
- Reduced motion: transitions are instant, state changes stay visible (label, icon, outline, text),
  and no information is carried only by motion. No page transition on route change; lazy pages
  show their skeleton in place.

**Presentation profiles.** Detailed (default for a new owner), Standard and Essential, with Custom
as per-page overrides. One set of components; every element carries a category, and visibility
follows from it:

| Category                                  | Detailed | Standard  | Essential                      |
| ----------------------------------------- | -------- | --------- | ------------------------------ |
| P0 safety, approval, audit, current state | shown    | shown     | shown                          |
| P1 primary                                | shown    | shown     | shown                          |
| P2 technical detail                       | shown    | collapsed | collapsed                      |
| P3 metadata                               | shown    | collapsed | hidden, "+N details" indicator |

P0 is never hidden or collapsed in any profile (a test asserts it for every page). Profiles are
built before any page that uses them, after the shared visibility infrastructure. They change what
is shown, never authority.

**Shell additions.** One right-hand slot shared by the Operator panel and the inspector: side by
side when the page keeps at least 640 px of content, otherwise the surface opened most recently wins
and the other collapses to a tab; a full-screen sheet on phones; never over content. Escape closes
one thing per press: tooltip, popover or menu; dialog or palette; the panel or inspector with
focus; then the graph selection. An attention-centre entry and an autonomy-state indicator join
the shell.

**New kit components** (each added to the kit, this list and `/dev/design` together): a graph
canvas with an outline (real DOM, the screen-reader path) and keyboard navigation (arrows move
between connected nodes, Enter opens the inspector, Home and End go to the first and last node, a
key jumps to the active node); an inspector; a split pane with a keyboard-operable divider; a
virtual list; the one structured approval card (contract, target, arguments, effect class,
reversibility, expiry, plan-hash prefix, a label for which plan hash the approval binds ("v1 binding" or "v2 binding" during the migration, RW-11), policy decision, and an agent refusal reason such as a rate limit; model text secondary and labelled); a
freshness label; the profile provider and visibility primitive.

**Lint and size amendments the graph will need, each written here before the code.** Computed
geometry goes through one kit helper that sets SVG attributes and CSS custom properties; there is
no blanket exemption from the inline-style rule. The 250-line file cap is not relaxed: split the
graph by responsibility. A layout worker, if used, is a same-origin hashed file (no `blob:`). The
graph is its own lazy chunk inside the 120 kB chunk limit.

**Freshness.** `QueryBoundary` gains refreshing, stale and refresh-error states and a "last
updated" label, so a failed refetch no longer hides behind old data. "Live" will describe the
event stream the page uses, not only incidents and actions.

**New statuses** (operation, node including uncertain and planned, policy outcome, capability
install, enable and authorize, knowledge state, and the step outcomes `Refused`, `NotRun`,
`Unavailable` and `OutcomeUnknown`) follow the "Add a status" recipe, each with a label, tone,
hint and icon. More are planned. The host state **`Incompatible`** (agent upgrade required:
the agent speaks another protocol minor than the hub, D-20) is its own status with an
upgrade-required hint and is never shown with the offline status; its banner and host page keep
history readable. It is read from the persisted agent record, so it survives a hub restart, and
after a hub upgrade a host that has not reconnected reads "expected incompatible" (RW-17). The
privilege states (D-29, RW-39) are statuses too: a **root-agent warning** (the agent runs as root,
the privilege boundary is not in effect, no unattended execution; shown as a banner and on the host
page), **skill runner missing** (skills are unavailable; they never fall back to the control agent)
and **helper unavailable** (the root-owned helper is absent or in error; shown on each
affected check and capability, never as a failure or as offline).
The host page shows the helper's effective exclusion set (hash and summary, as reported by the agent;
the UI has no second editable list) read-only beside the
confirmed management paths, each path marked covered or uncovered (RW-37), and the Capability
Center gets an owner package import (upload of an owner-signed archive and its signature, RW-26)
before the install wizard, which is built after the visibility infrastructure. The **locked rule** treatment (D-26) makes a locked Deny read differently from
an ordinary Deny: a lock icon and the text "Locked Deny", never colour alone, in the policy
editor, simulator, explanation and trace.

Playback of a completed operation is the last U2 deliverable (D-25); the timeline and the outline
do not depend on it, and its motion follows the same visualization class and reduced-motion rule
as the graph.

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
| Initial JavaScript             | 179.0 kB, one chunk      | 154.8 kB | 170 kB (achieved +10%) |
| Largest chunk                  | 179.0 kB                 | 110.7 kB | 120 kB                 |
| Total CSS                      | 9.9 kB                   | 14.6 kB  | 30 kB                  |

The Inter font file (latin subset, about 48 kB) is not counted by `npm run size`. It loads
from the hub's own origin, is cached across upgrades, and uses `font-display: swap`, so it
never delays the first paint.

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
tag, the read-only state with its reason, and the validation issue, laid out as a settings
row. A new section is one entry in `SECTIONS`. Outside Settings wrap inputs in
`<Field label hint path="the.path">`.

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
