# UI modernization · design brief (paste before every part)

Paste `../00-rules.md`, then this brief, then exactly one part (`01-…` to `07-…`).
Every part is one branch and one pull request (`feat(web): …`), started in a fresh
session, merged before the next part begins.

Survey the current UI on **`main`** (`git show main:web/src/...` or a checkout of
`main`). Other branches may carry an older `web/`.

---

## What we are building, and for whom

Domovoi's web UI is the operator's console for a self-hosted server overseer: a hub that
watches hosts through agents, opens incidents, lets a language model triage them,
proposes actions that only a human approves, and keeps notes, rules, skills and a daily
digest. The person using it is a homelab owner or a small-team sysadmin. They open it
when something is wrong, often on a phone, sometimes at night. They need to see in two
seconds **what needs them**, act with confidence, and trust that every click did what
they think it did.

The goal of this series is a **complete modernization of the frontend**: a calm,
mature, professional interface with the best UX we can give it. Concretely:

1. **A real visual identity.** Muted, mature **brown and green**. Warm, earthy, quiet.
   Never saturated, never playful.
2. **Colour that carries meaning.** Status colour coding ("Action needed", "Resolved",
   "Waiting for approval", "Failed", "Offline", …) is used consistently and more widely
   than today, so the UI is not monotone, but colour is never decoration and never the
   only signal: every coloured thing also has a word and, where it helps, an icon.
3. **Every action answers back.** Success, failure, progress and "nothing changed" are
   always communicated, in plain words, in the right place.
4. **Explain on demand.** Hover or focus on anything non-obvious (a status, an icon
   button, a disabled button, a column header, a setting) and it explains itself.
   Longer explanations are one click away. Nothing explains itself unasked if it is
   obvious.
5. **Never blank, never lying.** Every view has a loading, empty, error and offline
   state. A failed request never reads as "nothing here".
6. **Fast.** Loads quickly on a slow phone over a VPN. Smaller initial bundle than
   today, no layout shift, no spinners where a skeleton fits, no refetch storms.
7. **Hard to degrade.** Later work will be done by smaller AI models adding pages and
   features one prompt at a time. The design system, its rules and its guardrails must
   make the right thing the easy thing and make drift fail a check, not a review.

**Professional, not gimmicky.** Every effect must earn its place by making something
clearer or faster. No mascots, illustrations, gradient blobs, glassmorphism, parallax,
confetti, bouncy springs, animated backgrounds or emoji in UI text. Motion is short
(120–200 ms), eases out, explains a change (something appeared, moved, or finished), and
is removed under `prefers-reduced-motion: reduce`.

## Freedom you have in this series

- **Structure.** You may restructure `web/src` (new `design/`, `components/`, `styles/`
  folders, moved files, renamed components). Keep `web/src/api` as the only place that
  calls `fetch` (rule 8) and keep `features/<area>/` as the home of page code.
- **Dependencies.** Rule 7 is relaxed for this series: add npm dependencies where they
  clearly do the job better than hand-written code (for example headless positioning
  for tooltips, an icon set that tree-shakes, an accessibility test helper, a
  self-hosted variable font). For each one, the PR description states what it is for,
  its gzip size in the built bundle, and why writing it was worse. Stay inside the
  performance budget below. Prefer one well-chosen library over three overlapping ones.
  Never load anything from a CDN at runtime: the hub is embedded and may be offline.
- **API.** You are **not** limited to the existing API. Add or change endpoints
  wherever the UI needs it to do the job properly: summary/count endpoints for the
  navigation, server-side filters instead of client-side filtering of paged data,
  richer error details, fields a page needs. Follow rule 9 for every API change
  (`#[utoipa::path]`, `routes()`, `cargo xtask openapi`, `npm run gen:api`, the
  OpenAPI snapshot), add write routes to the `writes` list in
  `crates/domovoi-hub/tests/gates.rs`, keep the hard rules and security invariants in
  `ARCHITECTURE.md` (above all: only an authenticated human approves an action), and
  write hub tests for the new behaviour. Changing an existing response shape is fine
  when you update every caller in the same PR.

## Constraints that stay

- `00-rules.md` and `AGENTS.md` apply, including the 500-line Rust file limit, the
  commit and PR conventions, and: **no committed file, commit or PR mentions prompts,
  prompt numbers, the build plan or AI assistance.** `web/DESIGN.md` (created in part
  01) is a committed file, so write it as normal project documentation.
- The hub's Content-Security-Policy (`crates/domovoi-hub/src/middleware.rs`, `CSP`) is
  `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; …`.
  **Inline `<script>` is blocked.** Anything that must run before React (the theme
  pre-paint) is a small static file served by the hub, not inline code. Do not loosen
  the CSP.
- Tests find things by role and accessible name. Keep accessible names stable, or
  update the tests in the same PR. Never delete a test to make a change pass; rewrite it
  for the new structure.
- Accessibility is part of "done": WCAG 2.2 AA contrast (4.5:1 text, 3:1 large text and
  UI boundaries), full keyboard use, visible focus, `aria-live` for asynchronous
  results, touch targets at least 24×24 px (44 px for primary mobile controls).

## Visual direction

### Brand roles

- **Green acts.** Primary buttons, links, focus ring, the selected navigation item,
  toggles that are on, progress. A deep, muted moss/olive.
- **Brown frames.** The sidebar, the brand mark, page and section headers' accent rule,
  neutral chips, table header text, borders with warmth. A walnut/umber brown.
- **Paper and ink.** Light theme: warm off-white paper, near-black warm ink. Dark theme:
  deep warm charcoal (not blue-black), warm light text.

Starting values (tune them; every pair you ship must pass contrast, and part 07 adds a
test that proves it):

| Role | Light | Dark |
| --- | --- | --- |
| Page background | `#f6f4ef` | `#1a1816` |
| Surface (cards, tables) | `#fdfcf9` | `#22201d` |
| Raised / hover surface | `#efebe3` | `#2c2925` |
| Border | `#ddd6ca` | `#3a3631` |
| Text | `#2a2622` | `#ece6dc` |
| Muted text | `#6b635a` | `#a89f93` |
| Primary (green) | `#4f6b4a` | `#8fae86` |
| On primary | `#ffffff` | `#1a1816` |
| Frame (brown) | `#6f5440` | `#c09a7a` |
| Sidebar background | `#3b3029` | `#141210` |

### Status tones: the colour-coding system

One closed set of **tones**. Every status in the product maps to exactly one tone in
one file (`web/src/design/tone.ts`, part 01). Each tone has four tokens: `-bg` (soft
tint), `-fg` (text on the tint and on the page), `-border`, `-solid` (for counts and the
rare filled element). Suggested hues, all muted:

| Tone | Hue (light `-fg`) | Means | Examples |
| --- | --- | --- | --- |
| `danger` | brick `#9b3b2a` | broken, needs a human now | verdict `ACTION_NEEDED`, severity `critical`, action `failed`/`verify_failed`/`rejected`, host offline |
| `warning` | ochre `#8a5f17` | off, watch it | verdict `MONITOR`, severity `warning`, update available, triage `skipped` |
| `success` | sage `#3f6e45` | fine, done, recovered | verdict `FIXED`, incident `resolved`, action `succeeded`/`verified`, host connected, check ok |
| `pending` | muted plum `#6b4f6e` | waiting for a person | action `proposed`, incident `pending`, approvals count |
| `progress` | slate teal `#3c6663` | something is running | action `approved`/`dispatched`, triage running, live "connecting" |
| `info` | slate blue `#46617a` | neutral information | severity `info`, maintenance window, notes |
| `neutral` | taupe `#6f675e` | intended, quiet, closed | verdict `EXPECTED`, incident `acked`/`muted`, action `denied`/`expired`, disabled check |

`success` and the primary green must be told apart at a glance: success appears as a
soft tinted pill with a dot or check icon, primary as a filled button. Do not use the
`-solid` success colour for buttons.

**Pills, not paint.** A status reads as a pill: soft `-bg`, `-fg` text, a 6 px dot or a
14 px icon, the label. Filled `-solid` is reserved for counts (nav badges, filter chip
counts) and a page-level critical banner. Rows and cards may carry a 3 px left stripe in
their tone; whole rows are never tinted, except a very faint `danger` tint for incidents
that need action.

The real status vocabularies (verify against `web/src/api/schema.d.ts` and
`crates/domovoi-core`): incident status `pending | open | acked | muted | resolved`;
verdict `ACTION_NEEDED | MONITOR | EXPECTED | FIXED`; severity `info | warning |
critical`; action status `proposed | approved | denied | expired | dispatched | rejected
| succeeded | failed | verified | verify_failed`; skill risk `read | write |
destructive` (read → `info`, write → `warning`, destructive → `danger`); triage
`report | cached | skipped | failed`; live status `live | connecting | offline`; issue
severity `error | warning`; host kind `agent | container_agent | external`.

### Typography, space, shape

- One sans family (system stack, or one self-hosted variable font if you can justify it
  in the budget), one mono stack for commands, logs, args and IDs.
  `font-variant-numeric: tabular-nums` for numbers in tables, counts and times.
- A small type scale (about 6 steps), sentence case everywhere, no all-caps labels
  except tiny table headers with letter-spacing.
- 4 px spacing grid; radii 6/10 px; one subtle shadow for raised surfaces, one for
  overlays. Dense but breathable: this is a console, not a landing page.

## Messaging: how the UI talks

Voice: plain, calm, specific, sentence case, no exclamation marks, no "Oops", no
blame, no jargon a homelab owner would not use. Say what happened, then what to do.

| Situation | Where | Example |
| --- | --- | --- |
| A quick action succeeded (ack, mute, approve, save) | toast, auto-dismiss ~4 s, with Undo where the API allows | "Incident muted for 1 hour. Undo" |
| An action failed | toast that stays until dismissed, or inline next to the control if the control stays on screen | "Could not approve the action: the approval window closed 2 minutes ago." |
| A form has problems | inline, under each field, plus a summary at the top of the form that links to the fields | "Port must be between 1 and 65535." |
| A page or panel cannot load | in place of the content, with Retry | "Could not load hosts. The hub did not answer. Retry" |
| The hub is unreachable / live updates lost | one banner at the top of the main area | "Live updates paused. Reconnecting…" |
| Something long is running | inline progress in the place the result will appear | "Triage is reading the logs…" |
| Rate limited (429) | inline or toast with a live countdown from `retryAfter` | "Too many attempts. Try again in 28 s." |
| Destructive or irreversible | confirm dialog naming the object and the consequence; the confirm button repeats the verb | "Revoke agent nas-01? It disconnects now and needs a new invite to come back." [Revoke agent] |
| Nothing to show | empty state that says why and offers the next step | "No incidents. Everything your agents watch is healthy." |

Error codes from the hub (`crates/domovoi-hub/src/api/error.rs`): `unauthorized`,
`setup_required`, `bad_credentials`, `forbidden`, `not_found`, `bad_request`,
`invalid` (with `issues`), `conflict`, `too_many_requests` (with `retryAfter`),
`unavailable`, `triage_off`, `budget_spent`, `delivery_failed`, `internal`, plus
network failure and timeout on the client. Every one of them gets a human sentence in
one catalogue (part 02); the server's own message is shown as the detail when it adds
something. Never show a raw code or a stack trace as the main text.

## Hints: how the UI explains itself

- **Tooltip** (hover after ~400 ms, focus immediately, Escape closes): icon-only
  buttons, status pills (what the status means), abbreviations, truncated values (the
  full value), relative times (the absolute time), column headers whose meaning is not
  obvious, and **every disabled control (why it is disabled and how to enable it)**.
- **InfoTip** (a small "?" or info icon that opens a popover on click or Enter):
  longer explanations of concepts (what "Known condition" means, what a risk level
  allows), with an optional "Learn more" link to the book.
- **Field hint**: one line under an input saying what it expects, replaced by the error
  when invalid.
- **Page intro**: one or two sentences under each page title (keep `PageIntro`'s idea).
- On touch devices, tooltips do not appear on hover; anything a tooltip says that
  matters must also be reachable by tapping (InfoTip, or visible text).

## Performance budget

Measure `main` first (`npm run build`, record the gzip size of the initial JS, the
largest chunk, total CSS). Then, at the end of the series:

- Initial route JS (gzip) is **smaller than on `main`**; no single chunk over 120 kB
  gzip; total CSS under 30 kB gzip.
- Pages other than Incidents, Login and Setup load lazily; the Markdown stack is its own
  chunk.
- No layout shift from fonts, images or late data (skeletons reserve space).
- A mutation refetches only what it changed; polling stops when nothing can change.
- A web font, if any, is self-hosted, subset, `woff2`, preloaded, under 40 kB, with
  `font-display: swap` and a metric-matched fallback.

Part 07 adds a size check that fails the build when a budget is exceeded.

## Built for the next contributor (smaller models)

The design must survive dozens of later changes by less capable models. So:

1. **One source of truth per decision.** Colours, spacing, radii, shadows, motion and
   z-index exist only as tokens in `web/src/styles/tokens.css`. Status → tone → label →
   hint → icon exists only in `web/src/design/tone.ts`. User-facing error sentences
   exist only in the error catalogue.
2. **A small, closed component kit** in `web/src/components/` with a barrel
   `index.ts`. Feature code composes kit components and lays them out with a few
   layout primitives (`Stack`, `Cluster`, `Grid`, `PageHeader`, `Section`); it does not
   invent new buttons, pills, cards, dialogs or tooltips.
3. **`web/DESIGN.md`**: the short, rule-first guide a model reads before touching the
   UI: principles, the token table, the tone table, the messaging table, the component
   list with one-line "use it for", and recipes ("Add a page", "Add a status", "Show an
   action's result", "Show a load error", "Add a setting", "Add a table column").
   `AGENTS.md` gets a "Web UI" section that points to it.
4. **Guardrails that fail CI**, not advice: no raw colour values outside `tokens.css`;
   no inline `style` in feature code except for computed values; no `title=` attribute
   (use `Tooltip`); no hand-rolled `<button>` in features (use `Button`/`IconButton`);
   no status label or colour decided outside `tone.ts`; a maximum file length for web
   source files; contrast test over every token pair; bundle size budget.
5. **A living catalogue**: a dev-only `/dev/design` page rendering every component in
   every state and tone, in both themes, excluded from production builds. A model
   checks its work against it.

## What exists on `main` (survey it before part 01)

- Stack: React 19, React Router 8, TanStack Query 5, `openapi-fetch`, `react-markdown`,
  Vite 8, Vitest + Testing Library, CSS Modules. About 8.5k lines of TS/TSX, 1.2k lines
  of CSS.
- Tokens: `app/global.css` (blue accent, grey surfaces, `--action-needed`, `--monitor`,
  `--expected`, `--fixed`, `--critical`, `--warning`, `--info`), dark mode by
  `prefers-color-scheme` only.
- Kit: `components/` has `Badge` (labels, tones and hints hard-coded in the component),
  `Button`, `ConfirmDialog`, `Dialog`, `CopyBlock`, `Countdown`, `EmptyState`,
  `Markdown`, `PageIntro`, `PasswordPrompt`, `RelativeTime`, `Tabs`, `ChatThread`,
  `useDebounced`; layout classes `.card`, `.toolbar`, `.grid`, `.table`, `.tableWrap`,
  `.pageHead` in `components/ui.module.css`.
- Shell: `app/Layout.tsx`, `app/Sidebar.tsx` (nav from `app/NavIcon.tsx` `NAV`, inline
  SVG icons, Approvals count, live indicator, user menu), `app/HostContext.tsx` +
  `HostSelect.tsx` (global host selector), `features/updates/UpdateBanner.tsx`,
  `features/setup/SetupBanner.tsx`. `app/routes.tsx` imports every page eagerly.
- Pages: incidents (list grouped by state, detail with card, chat, controls, actions,
  timeline), approvals, chat, known conditions (rules), hosts (list, detail with checks,
  Docker, profile, SSH logins, logs tabs, enroll dialog), skills, notes (list, editor,
  import), digest, logs, settings (sections, editable fields with sources, apply and
  restart, test connections), first-run setup wizard, login, setup.
- Messaging today: errors are mostly `<p className={styles.error} role="alert">` next to
  the control or an `EmptyState` with no retry; successes are mostly silent; about 70
  native `title=` tooltips; no toasts.
- Known defects to fix along the way (from an audit; parts 03–06 list them where they
  belong): pages that ignore the host selector, errors that read as empty lists, missing
  loading states, a blank screen while the session loads, tabs whose arrow keys do not
  move focus, unencoded host and skill names in paths, broad query invalidation, eager
  route imports.

## What is coming after this series (leave room for it)

These features are planned as separate work after the modernization. The kit, shell and
pages must have a natural place for each, so they slot in without new patterns:

- **Maintenance windows**: a "Maintenance" dialog on the host page (duration presets,
  note), "In maintenance until 14:30" + "End now" in the host header, a maintenance tag
  in the host selector and Hosts list, a maintenance list beside known conditions. Tone:
  `info`.
- **Audit log page** (`/audit`): a filterable table (time, who, what, subject link),
  kind-group filter chips, a "since" filter, expandable JSON details, Load more.
- **Who proposed an action**: "Suggested by triage" / "Suggested in chat" on approvals.
- **Account**: change password, sign out other sessions (Settings → Account), with
  password re-entry.
- **Backups**: "Last backup: 03:00 today" and a failed-backup advisory in Settings.
- **Skill editor**: Add/Edit/Delete skill on the Skills page, a form with a parameter
  editor, live validation issues, a monospace script field, a copy-commands block.
- **Markdown without remote images**: model-written Markdown shows a link instead of
  loading a remote image.
- **Later ideas**: more notifiers and gateways in Settings, a read-only second user
  (controls disabled with a "Your role cannot approve actions" tooltip), agent
  self-update from the Hosts page, more checks (certificate expiry, ZFS/mdraid/btrfs,
  backup heartbeats, pending package updates), a cost line in the digest, chat history
  saved on the hub.

## Out of scope for the whole series

- Engine, triage, notifier or agent behaviour, except where a UI feature needs an API
  field or endpoint.
- Changing what a feature does for the user beyond presentation, feedback and the listed
  defects, unless the part says so.
- Internationalization (write strings so they could be extracted later: no string
  concatenation of sentences).
