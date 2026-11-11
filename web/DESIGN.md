# Web UI design guide

Read this before changing anything under `web/`. Rules come first; each has its reason in
one line. The living catalogue at `/dev/design` (dev server only) shows every token, tone,
status, icon and kit component as it renders now: check your work against it in both themes.

## Principles

1. **Calm and professional.** Muted brown and green, warm paper and ink. Nothing saturated,
   nothing playful, no decoration that does not make something clearer or faster.
2. **Colour carries meaning.** A colour always says a status, and is never the only signal:
   every coloured thing also has a word and, where it helps, an icon.
3. **Every action answers back.** Success, failure, progress and "nothing changed" are said
   in plain words, in the place the person is looking.
4. **Explain on demand.** Anything non-obvious explains itself on hover or focus. Nothing
   obvious explains itself unasked.
5. **Never blank, never lying.** Every view has loading, empty, error and offline states. A
   failed request never reads as "nothing here".
6. **Fast.** It must load quickly on a slow phone over a VPN: no layout shift, no refetch
   storms, nothing fetched from another origin.
7. **One source of truth per decision.** Colours, spacing and motion live in
   `src/styles/tokens.css`. Statuses live in `src/design/tone.ts`. Icons live in
   `src/design/icons.tsx`. Nothing else decides them.

Motion is 120–180 ms, eases out, explains a change, and is off under
`prefers-reduced-motion: reduce` (the motion tokens become `0ms`, so always use them).

## Brand roles

| Role          | Colour                                          | Used for                                                                                            |
| ------------- | ----------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| Green acts    | `--color-primary` (moss)                        | Primary buttons, links, the focus ring, the selected navigation item, toggles that are on, progress |
| Brown frames  | `--color-frame`, `--color-sidebar-*` (walnut)   | The sidebar, the brand mark, the accent of page and section headings                                |
| Paper and ink | `--color-bg`, `--color-surface`, `--color-text` | Everything else: warm off-white and near-black in light, warm charcoal and sand in dark             |

`success` and the primary green must be told apart at a glance: success is a soft tinted
pill with an icon, primary is a filled button. Never fill a button with a tone's `-solid`.

## Where colour comes from

- `src/styles/tokens.css` is the only file with raw colour values (hex, `rgb()`, `hsl()`,
  named colours). A test fails on any other stylesheet that has one.
- Stylesheets use the **semantic tokens** below. The `--palette-*` primitives are for
  `tokens.css` only; the same test rejects them elsewhere.
- A status's colour is its tone, and its tone comes from `tone.ts`. Do not pick a tone token
  by hand for something that is a status.
- The dark theme is written twice in `tokens.css` (explicit choice and "System"), between
  `dark-theme:start` and `dark-theme:end` markers. Change both; a test compares them.
- Inside the sidebar the semantic tokens are remapped to the sidebar values
  (`Layout.module.css`), so kit components placed there need no special styles.

## Tokens

| Token                                                                         | Use it for                                                                               |
| ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `--color-bg`                                                                  | The page background                                                                      |
| `--color-surface`                                                             | Cards, tables, dialogs, inputs                                                           |
| `--color-surface-raised`                                                      | Hover and selected rows, table headers, raised chips                                     |
| `--color-border`                                                              | Dividers and the edges of cards                                                          |
| `--color-border-strong`                                                       | Edges that must be seen to use the thing: inputs, swatches (3:1)                         |
| `--color-text`                                                                | Body text                                                                                |
| `--color-text-muted`                                                          | Secondary text: meta lines, hints, timestamps                                            |
| `--color-text-subtle`                                                         | Tertiary text: placeholders, table header labels                                         |
| `--color-primary`, `--color-primary-hover`, `--color-on-primary`              | Things that act (see Brand roles) and the text on them                                   |
| `--color-frame`                                                               | The brown accent of headings and the brand                                               |
| `--color-sidebar-bg`, `-text`, `-text-muted`, `-border`, `-raised`, `-accent` | The sidebar only                                                                         |
| `--color-focus`                                                               | The focus ring (set globally; do not remove outlines)                                    |
| `--color-overlay`                                                             | The scrim behind dialogs                                                                 |
| `--tone-<tone>-bg`, `-fg`, `-border`                                          | A status pill or banner: tint, text, edge. `-fg` also passes on the page and on surfaces |
| `--tone-<tone>-solid`, `-on-solid`                                            | Counts (navigation badges, filter chips) and a page-level critical banner. Nothing else  |
| `--tone-danger-faint`                                                         | The one row tint: a table row that needs a person now (`DataTable` `rowState="urgent"`)  |
| `--space-1` … `--space-8`                                                     | 4, 8, 12, 16, 24, 32, 48, 64 px. All gaps, padding and margins                           |
| `--radius-sm`, `-md`, `-lg`, `-pill`                                          | 6 px controls, 10 px cards and dialogs, 14 px large panels, pills                        |
| `--shadow-raised`, `--shadow-overlay`                                         | A card that floats; a dialog, menu or toast                                              |
| `--text-xs` … `--text-2xl`, `--leading-xs` … `--leading-2xl`                  | The six type steps with their line heights                                               |
| `--font-sans`, `--font-mono`                                                  | UI text; commands, logs, arguments and IDs                                               |
| `--z-sticky`, `-dropdown`, `-overlay`, `-toast`, `-tooltip`                   | Layers, lowest to highest. Never a bare `z-index` number                                 |
| `--motion-fast`, `--motion-base`, `--ease-out`                                | 120 ms, 180 ms, the one easing curve                                                     |
| `--sidebar-width`, `--sidebar-rail-width`, `--content-max`                    | The shell's layout                                                                       |

Type: the system sans stack and a system mono stack (no web font, so nothing to download
and no layout shift). Base size is 15 px on desktop and 16 px on phones, which keeps iOS
from zooming into inputs. Sentence case everywhere. Use the `tabular-nums` class for
numbers in tables, counts and times.

## Tones

One closed set. Every status in the product maps to exactly one tone in `tone.ts`.

| Tone       | Means                      | Examples                                                                                                |
| ---------- | -------------------------- | ------------------------------------------------------------------------------------------------------- |
| `danger`   | Broken, needs a person now | Verdict "Action needed", severity critical, action failed or not fixed, host offline, destructive skill |
| `warning`  | Off, watch it              | Verdict "Monitor", severity warning, triage skipped, write skill, outdated agent                        |
| `success`  | Fine, done, recovered      | Verdict "Fixed", incident resolved, action succeeded or verified, host connected                        |
| `pending`  | Waiting for a person       | Action waiting for approval, incident pending, the approvals count                                      |
| `progress` | Something is running       | Action approved or running, live updates connecting                                                     |
| `info`     | Neutral information        | Severity info, read skill, maintenance, notes                                                           |
| `neutral`  | Intended, quiet, closed    | Verdict "Expected", incident acknowledged or muted, action denied or expired, anything unknown          |

**Pills, not paint.** A status reads as a pill: `-bg` tint, `-fg` text, an icon, the label.
`-solid` is for counts and one page-level critical banner. A row or card may carry a 3 px
left stripe in its tone. Whole rows are never tinted, with one exception: a table row for
something that needs a person now may carry the very faint `--tone-danger-faint`
(`DataTable`'s `rowState`). Do not use that token anywhere else.

A value the UI does not know (the hub may be newer) shows as a `neutral` pill with the raw
value as its label. Never throw on an unknown status.

## Icons

`<Icon name="…" />` from `src/design/icons.tsx` is the only way to draw an icon. Names are a
closed union; sizes are 14, 16 and 20 px. An icon is decorative unless you pass `label`,
which makes it an image with that name: do that only when no text sits beside it. To add an
icon, add one entry to the `ICONS` map; it then appears in the catalogue. Do not import
from the icon library anywhere else, and do not paste SVG into a component.

## Theme

The person picks System, Light or Dark with `ThemeSwitch`. `src/design/theme.ts` keeps the
choice in `localStorage["domovoi.theme"]` and sets `data-theme` on the root element; with
no attribute the device decides. A small inline script in `index.html` applies the saved
theme before first paint. The hub allows that one script by its hash, computed from the
built page, so it must stay static: no templating, no build-time values. Every colour you
add must work in both themes; check `/dev/design` in each.

## Guardrails

These fail `npm run lint` or `npm test`, not a review.

| Rule                                                                                                | Checked by                              |
| --------------------------------------------------------------------------------------------------- | --------------------------------------- |
| No raw colour and no `--palette-*` outside `tokens.css`                                             | `src/styles/cssGuard.test.ts`           |
| The two dark theme blocks are identical                                                             | `src/styles/cssGuard.test.ts`           |
| Every status value has a label, tone, hint and icon; unknown values fall back                       | `src/design/tone.test.ts`               |
| Every known timeline kind reads as words; an unknown one shows as it is                             | `src/design/timeline.test.ts`           |
| No inline `style` in `src/features` and `src/app`                                                   | ESLint `no-restricted-syntax`           |
| No native `title` attribute in `src/features` and `src/app`; kit buttons and inputs do not take one | ESLint `domovoi/no-native-title`, types |
| Kit components come from the `src/components` barrel; icons only through `Icon`                     | ESLint `no-restricted-imports`          |
| Every hub error code and every other failure has words, and no title shows a raw code               | `src/design/messages.test.ts`           |
| A source file is at most 250 lines (blank lines and comments not counted)                           | ESLint `max-lines`                      |
| `fetch` only inside `src/api`                                                                       | ESLint `no-restricted-globals`          |

A value computed at run time (a width from data, a position) may use inline `style` behind
an `eslint-disable-next-line` comment that says why. The list of files exempt from
`max-lines` in `eslint.config.js` only shrinks.

## Performance budget

Baseline, measured on the build before this design system existed:

| Measure            | Baseline (gzip)         |
| ------------------ | ----------------------- |
| Initial JavaScript | 168.25 kB, in one chunk |
| Largest chunk      | 168.25 kB               |
| Total CSS          | 5.08 kB                 |

Targets: initial route JavaScript smaller than the baseline; no chunk over 120 kB; total
CSS under 30 kB; nothing loaded from another origin at run time. The dev catalogue is not
part of production builds.

How the build stays inside it:

- Incidents, Login and Setup are in the first download. Every other page is listed in
  `src/app/pages.ts` and fetched when first opened; a navigation link starts the fetch on
  hover or focus.
- The Markdown renderer is its own file, fetched the first time `Markdown` is shown.
- The libraries (`vendor`) are split from the app, so they stay cached across hub upgrades.
- A mutation refreshes only what it changed (see the hooks in `src/api/hooks/`), and
  polling stops when nothing can change or the tab is hidden.

The first download is about 135 kB of JavaScript (gzip), the
largest file is the libraries at 94 kB, and Markdown is 35 kB on demand.

## The shell

`src/app/` is the frame around every signed-in page. Pages do not draw any of it.

| Part                | File                                | What it does                                                                                          |
| ------------------- | ----------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Sidebar             | `Sidebar.tsx`, `nav.ts`             | Brand, grouped navigation with counts, live state, account menu. A rail of icons on a tablet          |
| Top bar             | `TopBar.tsx`                        | Breadcrumb, the page's actions (`TopBarActions`), the host selector; on a phone the brand and Menu    |
| Host selector       | `HostSelect.tsx`, `HostContext.tsx` | The one app-wide host choice, kept in `?host=` and remembered. Pages read it with `useSelectedHost()` |
| Page title          | `PageTitle.tsx`                     | The browser tab's title, the breadcrumb, and focus on the heading after a navigation                  |
| Lazy pages          | `pages.ts`, `routes.tsx`            | Which pages are fetched on demand, and their routes                                                   |
| Skeletons           | `PageSkeleton.tsx`                  | The page's shape while its code loads; the whole frame's shape while the session loads                |
| "Showing all hosts" | `AllHostsNote.tsx`                  | The one line a page shows when it does not follow the host selector                                   |

Widths: under 768 px the navigation is a full-height sheet behind the Menu button; from
768 px it is a column, an icon rail by default up to 1023 px and full width above. The
person can switch with the button at the top of the sidebar, and the choice is remembered.

Every page either follows the host selector (filter on the hub with a `host` parameter
where the list is paged) or renders `<AllHostsNote>` saying why it shows every host. A
page that is about one host and has none selected asks for one; it never picks the first.

The navigation's counts come from one request, `useOverview(host)`, refreshed by live
events and at most every 30 seconds. Do not fetch a list to count it.

## Components

The kit lives in `src/components/`, one component per file, all exported from
`src/components/index.ts`. Page code imports from that barrel only
(`import { Button, StatusPill } from "../../components"`; a lint rule rejects deeper paths)
and composes these. If a page needs something the kit does not have, add it to the kit, to
this table and to `/dev/design`; do not style it inside the feature.

### Status

| Component    | Use it for                                                                                | Do not use it for                                    |
| ------------ | ----------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| `StatusPill` | Every status: `<StatusPill meta={verdictMeta(v)} />`. The hint is its tooltip             | Labels that are not a status; numbers                |
| `BadgePill`  | A chat card's badge, where one field holds a verdict, a severity or "resolved"            | Anything whose vocabulary you know: use `StatusPill` |
| `Tag`        | Non-status labels: host kind, "maintenance", "hub-managed", the source of a setting       | A status                                             |
| `Count`      | The filled number on navigation items, tabs and filter chips; `label` says what it counts | A number inside a sentence                           |
| `toneStripe` | A 3 px left stripe on a row or card: `<li {...toneStripe(meta.tone)}>`                    | Tinting a whole row                                  |

### Messages and feedback

| Component           | Use it for                                                                                    | Do not use it for                            |
| ------------------- | --------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `useActionFeedback` | Every mutation: declares its success toast, error context and Undo in one place               | Queries (use `QueryBoundary`)                |
| `useToast`          | A toast that is not the result of a mutation                                                  | Mutation results; anything inside a dialog   |
| `InlineMessage`     | A toned box next to what it is about, in a form or panel                                      | The result of a quick action; page-wide news |
| `InlineError`       | A failed action shown inline from the raw error: `<InlineError error={e} context="…" />`      | Load failures (use `LoadError`)              |
| `Banner`            | One page-wide or hub-wide message at the top of the main area                                 | A problem with one field or panel            |
| `ErrorDetail`       | The detail line of a `DescribedError` (live rate-limit countdown, issue list) in kit messages | Writing your own error text                  |

### Hints

| Component  | Use it for                                                                      | Do not use it for                       |
| ---------- | ------------------------------------------------------------------------------- | --------------------------------------- |
| `Tooltip`  | A few words on hover or focus                                                   | Anything essential or interactive       |
| `InfoTip`  | A sentence or two behind an info button, with an optional "Learn more"          | A few words (`Tooltip`); a field's hint |
| `DocsLink` | "Learn more" to a page of the book, after text that already carries the meaning | The only explanation of something       |
| `Kbd`      | A key in a shortcut hint                                                        | Code or commands                        |

### Buttons

| Component    | Use it for                                                                                | Do not use it for           |
| ------------ | ----------------------------------------------------------------------------------------- | --------------------------- |
| `Button`     | Anything that acts. Props: `variant`, `size`, `icon`, `loading`, `disabledReason`, `hint` | Navigation; an icon alone   |
| `IconButton` | A well-known action in a tight place; `label` is its name and its tooltip                 | Where a word fits           |
| `LinkButton` | Navigation that is the main action of a view ("New note")                                 | Anything that changes data  |
| `CopyButton` | Copying a value, confirmed with "Copied"                                                  | Long text (use `CopyBlock`) |

`primary` is the one main action of a view; `danger` is destructive; `ghost` is for quiet
toolbar actions; `secondary` is the rest. A button that is running shows `loading`, never a
changed label. A button that cannot be used gets `disabledReason`, which keeps it focusable
and explains itself; do not use the bare `disabled` attribute in new code.

### Loading, empty, error

| Component       | Use it for                                                                    | Do not use it for                         |
| --------------- | ----------------------------------------------------------------------------- | ----------------------------------------- |
| `QueryBoundary` | Everything a page loads: renders loading, error, empty, then content          | Mutations                                 |
| `Loading`       | "Loading…" where the content's shape is unknown (waits 300 ms before showing) | A wait on a button (`loading`)            |
| `Skeleton`      | A placeholder in the content's shape: `line`, `block`, `rows`                 | Anything that is not about to be replaced |
| `Spinner`       | A short wait next to its cause                                                | A page or panel                           |
| `Progress`      | Something long that is running, where its result will appear                  | A wait of a moment                        |
| `LoadError`     | Content that could not load: what, why, Retry                                 | A failed action                           |
| `EmptyState`    | Data loaded and there is none: why, and the next step                         | A failed load, ever                       |

### Layout

| Component                  | Use it for                                                                            | Do not use it for              |
| -------------------------- | ------------------------------------------------------------------------------------- | ------------------------------ |
| `PageHeader`               | The top of every page: `title` (the only `h1`), `meta`, `intro`, `docs`, `actions`    | Headings inside a page         |
| `Section`                  | A titled part of a page (`h2`), with a description and actions; `collapsed` closes it | Nesting inside another section |
| `Card`                     | One thing among several; `tone` adds a stripe, `to` makes it a link                   | Cards inside cards             |
| `Text`                     | A quieter, smaller or monospace run of text: `muted`, `mono`, `strong`, `size`        | A status, a label, a heading   |
| `Stack`, `Cluster`, `Grid` | A column, a wrapping row, equal columns; `gap` is a step of the spacing scale         | Margins on children            |

### Data

| Component      | Use it for                                                                            | Do not use it for                 |
| -------------- | ------------------------------------------------------------------------------------- | --------------------------------- |
| `DataTable`    | Many things with the same fields; stacks into cards in a narrow container             | The details of one thing          |
| `Timeline`     | The history of one thing, in words from `timelineMeta`; long ones collapse            | A list of different things        |
| `KeyValueList` | The facts of one thing; `mono` for arguments, paths and IDs                           | Many things                       |
| `FilterChips`  | The few filters people switch all the time, with counts; one scrolling row on a phone | Rare or long filters (`Select`)   |
| `Tabs`         | A few views of the same object; a tab may carry a `count`                             | Different places (navigation)     |
| `RelativeTime` | A time read at a glance; the exact time is its tooltip                                | Where the exact time is the point |
| `Countdown`    | A deadline minutes away, ticking                                                      | Past times                        |
| `CopyBlock`    | A command or value pasted elsewhere, in a monospace block                             | Prose                             |
| `Markdown`     | Text written by the model or in notes; raw HTML is never rendered                     | UI text you wrote                 |
| `ChatThread`   | A conversation with the model: turns, starters, a composer, Stop and Retry            | Anything else                     |

### Forms

| Component                                        | Use it for                                                        | Do not use it for                       |
| ------------------------------------------------ | ----------------------------------------------------------------- | --------------------------------------- |
| `Field`                                          | Around every input: label, hint, error, `required`, `path`        | Checkboxes and switches (self-labelled) |
| `TextInput`, `NumberInput`, `Textarea`, `Select` | The input inside a `Field`; `mono` for commands, paths and IDs    | Use outside a `Field`                   |
| `Checkbox`                                       | One of several independent choices, saved with the form           | An on/off setting                       |
| `Switch`                                         | A setting that is simply on or off                                | A choice inside a list                  |
| `SegmentedControl`                               | Two to four exclusive choices that fit in a word each             | Filters over a list (`FilterChips`)     |
| `FormErrorSummary`                               | The problems of a long form at its top, each linking to its field | Short forms                             |
| `SubmitBar`                                      | The sticky bottom of a long form: unsaved count, Discard, Save    | Dialogs and short forms                 |

### Overlays

| Component        | Use it for                                                                                                                    | Do not use it for                  |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- |
| `Dialog`         | A question or short form that must be answered before going on                                                                | Explanations; results              |
| `ConfirmDialog`  | Before something destructive: `title` names the object, `consequence`, a verb on the button; `requireText` for the worst      | Actions that can be undone         |
| `PasswordPrompt` | Password re-entry before a change that widens trust                                                                           | Logging in                         |
| `Menu`           | Secondary actions behind a "More" button; `icon`, `header` and `checked` entries make the account menu; `text` a labelled one | The only way to do something       |
| `OptionPicker`   | A select whose options carry a pill or tag (the host selector); more than eight options can be filtered by typing             | Plain choices in a form (`Select`) |
| `Popover`        | The floating panel under `InfoTip` and `Menu`                                                                                 | Direct use in page code            |

## Messaging

Voice: plain, calm, specific. Sentence case, no exclamation marks, no "Oops", no blame, no
jargon a homelab owner would not use. Say what happened, then what to do. Write whole
sentences as one string; never build a sentence by joining fragments.

### Where a message goes

| Situation                                           | Where                                                                      | Example                                                                                       |
| --------------------------------------------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| A quick action succeeded (ack, mute, approve, save) | Toast, gone after about 4 s, with Undo where the API allows                | "Incident muted for 1 hour" [Undo]                                                            |
| An action succeeded but changed nothing             | Info toast                                                                 | "You are on the latest version"                                                               |
| An action failed and its control is gone            | Toast that stays until dismissed                                           | "Could not approve the action. The approval window closed 2 minutes ago."                     |
| An action failed and its control stays on screen    | `InlineMessage` next to the control (`errorDisplay: "inline"`)             | "Could not save the note. The note is too long."                                              |
| Anything fails inside a dialog                      | Inside the dialog (`ConfirmDialog`'s `error`): a toast would sit behind it | "Could not revoke the agent. The hub did not answer."                                         |
| A form has problems                                 | Under each field, plus `FormErrorSummary` at the top of a long form        | "Port must be between 1 and 65535."                                                           |
| A page or panel cannot load                         | `LoadError` in place of the content, with Retry                            | "Could not load hosts. The hub did not answer." [Retry]                                       |
| The hub is unreachable, live updates lost           | One `Banner` at the top of the main area                                   | "Live updates paused. Reconnecting…"                                                          |
| Something long is running                           | `Progress` where the result will appear                                    | "Triage is reading the logs…"                                                                 |
| Something the person started finished later         | Toast, when the page sees it end (`useActionOutcomes`)                     | "restart_container succeeded on nas-01"                                                       |
| What is on screen changed without the person        | Info toast, once                                                           | "This incident resolved"                                                                      |
| Rate limited                                        | Inline or toast; the kit counts down from `retryAfter`                     | "Too many attempts. Try again in 28 s."                                                       |
| Destructive or irreversible                         | `ConfirmDialog` naming the object and the consequence                      | "Revoke agent nas-01? It disconnects now and needs a new invite to come back." [Revoke agent] |
| Nothing to show                                     | `EmptyState` saying why, with the next step                                | "No incidents. Everything your agents watch is healthy."                                      |

### Errors

Every failure becomes words in `src/design/messages.ts`, and nowhere else.
`describeError(error, context)` returns a title, a detail, and the hub's `retryAfter` and
`issues`. `context` is a short verb phrase ("approve the action"), so the title reads "Could
not approve the action". The hub's own message is the detail where it is specific; the
catalogue's sentence is used where it is not. A raw code or a stack trace is never shown.
You rarely call it yourself: `useActionFeedback`, `LoadError`, `InlineError` and
`QueryBoundary` take the error and the context.

### Good and bad

| Good                                                                     | Bad                                     | Why                                |
| ------------------------------------------------------------------------ | --------------------------------------- | ---------------------------------- |
| "Incident muted for 1 hour"                                              | "Success!"                              | Says what happened                 |
| "Could not load hosts. The hub did not answer."                          | "Error: Failed to fetch"                | Words, not the raw error           |
| "Could not approve the action. The approval window closed."              | "409 conflict"                          | Never a code                       |
| "Revoke agent nas-01?" with the button "Revoke agent"                    | "Are you sure?" with the button "OK"    | Names the object, repeats the verb |
| "No notes here yet. Write one, or import a folder of Markdown runbooks." | "No data"                               | Says why and what next             |
| "Port must be between 1 and 65535."                                      | "Invalid input"                         | Says what is expected              |
| "Give the note a title first." on a disabled Save                        | A greyed-out button with no explanation | A disabled control says why        |
| "Checks saved. The agent is offline. They apply when it reconnects."     | "Saved" when the agent never got them   | Never claim more than happened     |
| "Too many attempts. Try again in 28 s."                                  | "Oops, slow down!"                      | Calm, no blame, says when          |
| "Triage is reading the logs…"                                            | A spinner alone for half a minute       | Long work says what it is doing    |

## Hints

| The thing                                           | The hint                                               |
| --------------------------------------------------- | ------------------------------------------------------ |
| An icon-only button                                 | `IconButton`'s `label` (name and tooltip in one)       |
| A status                                            | `StatusPill`, whose tooltip is the hint from `tone.ts` |
| A button whose label does not say what it does      | `Button`'s `hint`                                      |
| A disabled control                                  | `disabledReason`: why, and how to enable it            |
| A relative time, a truncated value, an abbreviation | `Tooltip` with the exact time or the full value        |
| A column header that is not obvious                 | The column's `hint` in `DataTable` (an `InfoTip`)      |
| A concept that needs a sentence or two              | `InfoTip`, with `learnMore` to the book                |
| What an input expects                               | The `Field` hint, replaced by the error while invalid  |
| What a page is for                                  | `PageHeader`'s `intro`                                 |

Rules:

- Never the native `title` attribute: it does not show on touch or keyboard focus. The lint
  rule rejects it on elements, and kit buttons and inputs do not accept it.
- A tooltip shows on hover after 400 ms and on keyboard focus at once; Escape closes it. It
  does not show for touch. So a tooltip may only repeat or refine what is on screen;
  anything a person needs to act goes in visible text or an `InfoTip`.
- Do not explain the obvious. A "Save" button needs no hint.

## Recipes

### Add a page

1. Put it in `src/features/<area>/`, start it with `<PageHeader title intro docs />`. The
   title is the page's only `h1`: focus moves to it after a navigation.
2. Add it to `PAGES` in `src/app/pages.ts` and give it a `<Route>` in `src/app/routes.tsx`
   with `element={<PAGES.name.Page />}`. Do not import the page into `routes.tsx`: that
   would put it in the first download.
3. Load data with a hook from `src/api/hooks/` and wrap what it renders in `QueryBoundary`
   with a `context`, an `EmptyState` and, where the shape is known, a `Skeleton`.
4. Decide what the host selector means for it: filter by `useSelectedHost()`, or render
   `<AllHostsNote>` with the reason it shows every host.
5. Lay it out with `Section`, `Stack`, `Cluster`, `Grid`, `Card` and `DataTable`. The page's
   own CSS module only places things; it has no colours, borders or type of its own.
6. The browser tab and the breadcrumb name the page from its navigation link. A page about
   one thing calls `usePageName(thing.title)` once it knows the name, which gives
   "nas-01 · Hosts · Domovoi".
7. Check it at 375, 768 and 1280 px and in both themes.

### Add a navigation item

1. Add one entry to the right group of `NAV_GROUPS` in `src/app/nav.ts`: `to`, `label`,
   an `icon` name, and `preload: PAGES.name.preload` so hovering the link fetches the page.
2. For a number beside it, add the count to the hub's overview (`GET /api/v1/overview`,
   `crates/domovoi-hub/src/api/overview.rs`), regenerate the API types, and set `count` to
   its field, a tone and the word read after the number ("waiting"). It shows only above
   zero, and follows the host selector because the overview does.
3. Keep labels to one or two words: on the rail the label is the tooltip.

### Add a status

The incident page (`src/features/incidents/`) is the worked example: every pill on it comes
from `tone.ts`, and every line of its timeline from `timeline.ts`.

1. Add the value to its vocabulary's type and table in `src/design/tone.ts`, with a label,
   one of the seven tones, a hint that says what it means (and what to do, if anything)
   and an icon name. If it is a new vocabulary, add it to `VOCABULARIES` too.
2. Run `npm test`: the tone test covers it and the catalogue shows it.
3. Render it with `<StatusPill meta={…Meta(value)} />`. Do not write its label or pick its
   colour anywhere else. A row or card takes its stripe from the same meta:
   `rowTone={(row) => incidentStateMeta(row).tone}`.
4. If the hub records it in an incident's timeline under a new kind, add the kind to
   `KINDS` in `src/design/timeline.ts` with a phrase, a tone and an icon. Until then the
   timeline shows the raw kind, which is the signal that the entry is missing.
5. If the list should filter on it, the hub does the filtering: add the filter to the
   endpoint (see `group` on `GET /api/v1/incidents`), regenerate the API types, and pass it
   through the hook. Do not filter loaded pages in the browser.

### Show an action's result

The incident page is the worked example again: Acknowledge, Mute and Dismiss in
`IncidentControls.tsx` and `useIncidentActions.ts`, Approve and Deny in
`src/features/approvals/ApproveControl.tsx`.

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

`success` may be a function of the result, and may return `{ title, detail, tone: "info" }`
for "nothing changed" or `null` when the page itself shows the result. Inside a dialog, or
where the person must fix their input, add `errorDisplay: "inline"` and render `ack.error`
with `InlineMessage` (or pass it to `ConfirmDialog`'s `error`).

Three more cases, each with one place that handles it:

- **The same action from two places** (a row's menu and the page's button): put the
  `useActionFeedback` calls in one hook, as `useIncidentActions` does, so both say the same
  words.
- **The result arrives later** (an approved action runs on a host): the mutation's toast
  says what was started ("Approved restart_container. It runs now."), the card shows
  `Progress` while it runs, and `useActionOutcomes(actions, hosts)` on the page announces
  the end. Call it once per page with every action the page has loaded.
- **Something long the person started** (triage): show `Progress` where the result will
  appear while the mutation is pending, and return `null` from `success` for an outcome the
  panel itself explains, so it is not said twice.

### Show a load error

Use `QueryBoundary`. If you cannot, render `<LoadError error={query.error} context="load
hosts" onRetry={query.refetch} />` when the query has an error and no data. Never render an
`EmptyState` for a failed request. A 404 for one object is the exception: say "No such
host" with a link back.

### Add a setting

Wrap the input in `<Field label hint path="the.settings.path">`. The hint says what the
value is for and its range; the hub's validation issue for that path becomes the field's
`error`, and `FormErrorSummary` links to it. A value fixed in `hub.toml` is shown disabled
with a `Tag` naming its source.

### Add a table column

Add one entry to the `columns` of the `DataTable`: a `key`, a `label` (also the label in
the stacked layout), a `cell`, `numeric` for numbers that should line up, and a `hint` if
the header is not obvious. A status cell is a `StatusPill`; a time is a `RelativeTime`.

### Add a component to the kit

One file in `src/components/` with a JSDoc comment that says what it is for and what it is
not for, styles from tokens only in a CSS module, an export in `index.ts`, a row in the
table above, a demo in `src/dev/` in every variant and state, and a test for its behaviour.

### Add a colour

Do not, unless a new role truly has no token. Then add the primitive and the semantic token
to `tokens.css` for light and for both dark blocks, check its contrast (4.5:1 for text, 3:1
for boundaries), and add a row to the token table above.
