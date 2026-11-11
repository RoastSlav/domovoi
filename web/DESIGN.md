# Web UI design guide

Read this before changing anything under `web/`. Rules come first; each has its reason in
one line. The living catalogue at `/dev/design` (dev server only) shows every token, tone,
status and icon as it renders now: check your work against it in both themes.

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
| `--space-1` … `--space-8`                                                     | 4, 8, 12, 16, 24, 32, 48, 64 px. All gaps, padding and margins                           |
| `--radius-sm`, `-md`, `-lg`, `-pill`                                          | 6 px controls, 10 px cards and dialogs, 14 px large panels, pills                        |
| `--shadow-raised`, `--shadow-overlay`                                         | A card that floats; a dialog, menu or toast                                              |
| `--text-xs` … `--text-2xl`, `--leading-xs` … `--leading-2xl`                  | The six type steps with their line heights                                               |
| `--font-sans`, `--font-mono`                                                  | UI text; commands, logs, arguments and IDs                                               |
| `--z-sticky`, `-dropdown`, `-overlay`, `-toast`, `-tooltip`                   | Layers, lowest to highest. Never a bare `z-index` number                                 |
| `--motion-fast`, `--motion-base`, `--ease-out`                                | 120 ms, 180 ms, the one easing curve                                                     |
| `--sidebar-width`, `--content-max`                                            | The shell's layout                                                                       |

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
left stripe in its tone; whole rows are never tinted.

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

| Rule                                                                                                | Checked by                       |
| --------------------------------------------------------------------------------------------------- | -------------------------------- |
| No raw colour and no `--palette-*` outside `tokens.css`                                             | `src/styles/cssGuard.test.ts`    |
| The two dark theme blocks are identical                                                             | `src/styles/cssGuard.test.ts`    |
| Every status value has a label, tone, hint and icon; unknown values fall back                       | `src/design/tone.test.ts`        |
| No inline `style` in `src/features` and `src/app`                                                   | ESLint `no-restricted-syntax`    |
| No native `title` attribute in `src/features` and `src/app` (a warning until the kit has a tooltip) | ESLint `domovoi/no-native-title` |
| A source file is at most 250 lines (blank lines and comments not counted)                           | ESLint `max-lines`               |
| `fetch` only inside `src/api`                                                                       | ESLint `no-restricted-globals`   |

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

## Components

## Messaging

## Hints

## Recipes

### Add a status

1. Add the value to its vocabulary's type and table in `src/design/tone.ts`, with a label,
   one of the seven tones, a hint that says what it means (and what to do, if anything)
   and an icon name.
2. Run `npm test`: the tone test covers it and the catalogue shows it.
3. Render it with `<Badge meta={…Meta(value)} />`. Do not write its label or pick its colour
   anywhere else.

### Add a colour

Do not, unless a new role truly has no token. Then add the primitive and the semantic token
to `tokens.css` for light and for both dark blocks, check its contrast (4.5:1 for text, 3:1
for boundaries), and add a row to the token table above.
