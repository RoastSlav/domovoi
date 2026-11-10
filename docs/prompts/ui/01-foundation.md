# UI 01 · Foundation: tokens, themes, the tone system, icons and the rules

Branch: `ui-foundation`. PR: `feat(web): …`. No API changes expected.

After this part the whole app renders in the new palette in light and dark, with a
theme switch. Page layouts and components do not change shape yet; that is parts 02–06.

## Steps

1. **Baseline.** On `main`, run `cd web && npm run build` and write down the gzip
   size of the initial JS, the largest chunk and total CSS (Vite prints them; or
   `gzip -c file | wc -c`). Put the numbers in the PR description and in
   `web/DESIGN.md` under "Performance budget" as the baseline later parts compare to.

2. **Tokens** in `web/src/styles/tokens.css`, the only file allowed to contain raw
   colour values:
   - *Palette primitives* (`--palette-moss-600`, `--palette-walnut-700`,
     `--palette-brick-600`, …): the raw hues from the brief, light and dark ramps.
   - *Semantic tokens*, the only ones components use:
     `--color-bg`, `--color-surface`, `--color-surface-raised`, `--color-border`,
     `--color-border-strong`, `--color-text`, `--color-text-muted`,
     `--color-text-subtle`, `--color-primary`, `--color-primary-hover`,
     `--color-on-primary`, `--color-frame`, `--color-sidebar-bg`, `--color-sidebar-text`,
     `--color-focus`, `--color-overlay`;
     tone tokens `--tone-<tone>-bg|fg|border|solid` for the seven tones
     (`danger warning success pending progress info neutral`) and
     `--tone-<tone>-on-solid`;
     `--space-1…8` (4 px grid), `--radius-sm|md|lg|pill`, `--shadow-raised|overlay`,
     `--text-xs…2xl` with matching line heights, `--font-sans`, `--font-mono`,
     `--z-sticky|dropdown|overlay|toast|tooltip`,
     `--motion-fast` (120 ms), `--motion-base` (180 ms), `--ease-out`.
   - Themes: light values on `:root`, dark on `:root[data-theme="dark"]` and inside
     `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { … } }`, so
     "System" works with no attribute set.
   - `@media (prefers-reduced-motion: reduce)` sets the motion durations to `0ms`.
   - `global.css` keeps resets, base element styles and `.visually-hidden`, and imports
     `tokens.css`. Remove every old variable (`--accent`, `--action-needed`, …) and
     migrate every CSS module to the semantic tokens. Replace the hard-coded
     `#ffffff`/`rgb(...)` in `Layout.module.css`, `Badge.module.css`,
     `hosts.module.css`, `skills.module.css` and `ui.module.css`.

3. **Theme switch.**
   - A tiny inline script in the `<head>` of `index.html`, before the module script,
     reads `localStorage["domovoi.theme"]` (`system | light | dark`, wrapped in
     try/catch) and sets `document.documentElement.dataset.theme` for light/dark before
     first paint. Keep it a few lines with no dynamic content, so its hash is stable.
   - **CSP change** (`CSP` in `crates/domovoi-hub/src/middleware.rs`): add
     `script-src 'self' 'sha256-…'` with the hash of exactly that script. Do not
     hard-code the hash by hand: compute it at build time from the built
     `web/dist/index.html` (for example in the hub's build script or where the embedded
     assets are loaded, or in a small Vite plugin that writes the hash to a file the hub
     embeds). Never `'unsafe-inline'`, no other directive changes. Update the CSP
     assertion in `crates/domovoi-hub/tests/hub/api_routes.rs`, add a test that the
     served `index.html`'s inline script matches the hash in the header, and mention
     the change in `docs/book/src/security.md` if it lists the CSP.
   - `web/src/design/theme.ts`: `useTheme()` returns `{ theme, setTheme }`, persists the
     choice, updates `data-theme`, and updates `<meta name="theme-color">`.
   - The control (System / Light / Dark, a small segmented control with icons and
     labels) goes in the user menu; part 03 restyles the menu, so a plain placement is
     fine here.

4. **The tone system** in `web/src/design/tone.ts`, the single place where a status
   becomes a label, a tone, a hint and an icon:
   ```ts
   export type Tone = "danger" | "warning" | "success" | "pending" | "progress" | "info" | "neutral";
   export interface StatusMeta { label: string; tone: Tone; hint: string; icon: IconName }
   export function verdictMeta(v: Verdict): StatusMeta
   export function severityMeta(s: Severity): StatusMeta
   export function incidentStatusMeta(s: IncidentStatus): StatusMeta
   export function actionStatusMeta(s: ActionStatus): StatusMeta
   export function riskMeta(r: Risk): StatusMeta
   export function hostConnectionMeta(h: …): StatusMeta
   export function triageMeta(t: …): StatusMeta
   export function liveMeta(l: LiveStatus): StatusMeta
   export function incidentBadgeMeta(i: Pick<IncidentSummary, "status" | "verdict" | "severity">): StatusMeta
   ```
   - Types come from the generated schema where it has unions; where the schema says
     `string`, define the union here from the values listed in the brief and fall back
     to a `neutral` meta with the raw value as label for anything unknown (the hub may
     be newer than the UI).
   - Move the labels and hints from `components/Badge.tsx` and the logic in
     `features/incidents/badge.ts` here; keep their wording unless it is unclear.
   - Write a hint for every value (what it means and, where useful, what to do).
   - `Badge` keeps working for now by reading `tone.ts` (part 02 replaces it with
     `StatusPill`).

5. **Icons** in `web/src/design/icons.tsx`: one `Icon` component with a closed
   `IconName` union, `size` 14/16/20, `aria-hidden` by default, an optional `label` that
   makes it `role="img"`. Either extend the hand-drawn 16×16 paths in `app/NavIcon.tsx`
   or adopt a tree-shakeable icon set (your call, per the brief). Include what the
   kit and pages will need: navigation, status (check, x, alert, clock, pause, play,
   loader, dot), actions (copy, external, refresh, more, close, chevrons, search,
   filter, plus, trash, edit, eye, eye-off), theme (sun, moon, monitor), info, help.
   `NavIcon` becomes a thin use of `Icon` or is removed.

6. **Typography.** Pick the sans and mono stacks (or one self-hosted variable font, with
   the justification and size in the PR). Set the type scale tokens; base size 15 px on
   desktop, 16 px on phones (prevents iOS zoom on inputs). Tabular numbers utility
   class.

7. **Guardrails** (they fail `npm run lint` or `npm test`):
   - A stylelint setup (or a Vitest test that scans `src/**/*.css`) that rejects hex,
     `rgb()`, `hsl()` and named colours outside `styles/tokens.css`, and rejects
     `--palette-*` use outside `tokens.css` (components use semantic tokens only).
   - ESLint `no-restricted-syntax` rules for files under `src/features/` and `src/app/`:
     no JSX `style` attribute (allow-list a comment-marked exception for computed
     values), no JSX `title` attribute (turn this rule on in part 02 when `Tooltip`
     exists; add it now as a warning).
   - ESLint `max-lines` 250 (skip blank lines and comments) for `src/**/*.{ts,tsx}`
     except generated files and tests. Files on `main` above it: `setup/FirstRun.tsx`,
     `hosts/SshLoginsTab.tsx`, `settings/sections.ts`, `settings/fields.tsx`,
     `settings/form.ts`: split them in the part that redesigns their page, and list them
     in an explicit, shrinking override until then.
   - Wire new checks into `npm run lint` so CI runs them with no workflow change.

8. **`web/DESIGN.md`** (first version) and **`AGENTS.md`**:
   - `DESIGN.md`: principles (from the brief, short), brand roles, the token table
     (name → use), the tone table (tone → meaning → examples), "where colour comes
     from" rules, the performance baseline, and empty headings for the components,
     messaging, hints and recipes sections that later parts fill. Under 300 lines when
     finished; rules first, rationale in one line each.
   - `AGENTS.md`: a short "Web UI" section: read `web/DESIGN.md` before changing
     `web/`; use the kit; colours and statuses only through tokens and `tone.ts`; the
     guardrail commands.

9. **Dev catalogue skeleton**: route `/dev/design`, registered only when
   `import.meta.env.DEV` (verify the production bundle does not contain it), showing the
   palette, the semantic tokens, the seven tones as pills, the type scale and the icons,
   with a theme switch. Parts 02–06 add to it.

## Tests

- `tone.test.ts`: every known value of every vocabulary has a label, a tone, a non-empty
  hint and an icon; unknown values fall back to `neutral` with the raw label;
  `incidentBadgeMeta` gives "Resolved" for a resolved incident with any verdict.
- `theme.test.ts`: `setTheme("dark")` sets `data-theme` and persists; `"system"` removes
  the attribute; a throwing `localStorage` does not break it.
- The guardrail checks fail on a fixture with a raw colour (a small test, or a documented
  manual check in the PR if the tool cannot be unit-tested).
- Existing tests pass (update ones that asserted old badge colours or class names).

## Done when

- The app renders in the muted brown/green palette in light, dark and system mode,
  switchable from the user menu, with no flash of the wrong theme on load.
- No colour value exists outside `tokens.css`; every status label and colour comes from
  `tone.ts`; lint enforces both.
- `web/DESIGN.md` exists with the foundation sections; `AGENTS.md` points to it.

## Out of scope

- New components, layouts and page redesigns (02–06). Do not restyle pages beyond
  swapping tokens.
