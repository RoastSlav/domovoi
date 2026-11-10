# UI 07 · Polish, audits and the guardrails that keep it this way

Branch: `ui-guardrails`. PR: `feat(web): …` (or `chore(web): …` if no user-visible
change). Needs 06.

This part proves the result and locks it in, so later changes by smaller models cannot
quietly degrade it.

## Steps

1. **Contrast test.** `web/src/styles/contrast.test.ts` parses `tokens.css` (both
   themes, including the system-dark block) and asserts WCAG ratios: text and muted text
   on bg, surface and raised surface ≥ 4.5; each tone's `-fg` on its `-bg` and on the
   page bg ≥ 4.5; `-on-solid` on `-solid` ≥ 4.5; primary on bg ≥ 3 (UI) and
   on-primary on primary ≥ 4.5; focus ring on bg and surface ≥ 3; borders that delimit
   controls ≥ 3 against their background. Fix any token that fails.

2. **Bundle budget.** A script (`npm run size`, run in CI's web job through
   `npm run build` or the lint step; check `.github/workflows` and `cargo xtask ci
   --web` to see which runs, and update `AGENTS.md`/`TESTING.md` if a command changes)
   that fails when: initial JS gzip exceeds the budget recorded in `DESIGN.md` (set it
   to the achieved size + 10 %), any chunk exceeds 120 kB gzip, or CSS exceeds 30 kB
   gzip. Record the final numbers against the 01 baseline in `DESIGN.md` and the PR.

3. **Accessibility audit.** Add an automated check (for example `axe-core` through
   `vitest-axe` or similar: your call) run over every page test's rendered output, or at
   least one render per page; fix violations. Then a manual keyboard pass over every
   page (Tab order, focus visible, Esc closes overlays, no traps, skip link works) and
   note it in the PR.

4. **Responsive and theme audit.** With Playwright (installed) against
   `cargo run -p domovoi-hub --example dev_hub` and `npm run dev`, screenshot every page
   at 375, 768, 1280 and 1920 px in light and dark. Fix horizontal scrolling, clipped
   text, misaligned controls and inconsistent spacing. Commit the script (for example
   `web/scripts/screens.ts`, not run in CI) so later changes can re-run it; do not
   commit the screenshots.

5. **Messaging sweep.** Grep every user-facing string in `src/features` and `src/app`;
   fix anything that breaks the voice rules (raw codes, "Error:", "Oops", exclamation
   marks, title case, vague "Something went wrong" without a next step). Every
   `useMutation` call site goes through `useActionFeedback` or renders an inline error:
   add a lint rule or test that fails when a mutation hook result is used with neither
   (a simple static check, for example an ESLint `no-restricted-syntax` on
   `.mutate(` outside files that import `useActionFeedback` or `InlineMessage`, with a
   documented escape comment).

6. **Hint sweep.** Every `IconButton` has a label (enforced by its type); every disabled
   control in the app has a `disabledReason` (add a dev-only console warning in
   `Button` when `disabled` is used without one, and a lint rule if practical); every
   status is a `StatusPill`; every abbreviation and non-obvious column header has a
   tooltip.

7. **Performance pass.** React Profiler on the incidents list with 200 incidents and
   live SSE events: no full-list re-render per event (memoize rows, stable keys, select
   only needed data from queries). Lighthouse (or Playwright tracing) on login and
   incidents: note LCP, CLS and TBT in the PR; fix any CLS above 0.05.

8. **Guardrail review.** Make sure each of these fails a check, and list them in
   `DESIGN.md` under "Checks that will fail your PR" with the command and the fix:
   raw colours outside tokens, palette tokens in components, JSX `style`, JSX `title`,
   raw `<button>` in features (add the rule now if not done), status labels/tones
   outside `tone.ts` (for example an ESLint rule against string literals like
   `"ACTION_NEEDED"` in JSX outside `design/`), file length, contrast, bundle size,
   axe violations, mutation feedback.

9. **`DESIGN.md` final pass.** Under 300 lines, rules first. Includes: principles,
   brand roles, tokens, tones, typography, spacing, motion, messaging, hints, components
   (one line each), layout patterns (list page, detail page, settings page, dialog
   form), recipes (Add a page, Add a nav item, Add a status, Show an action's result,
   Show a load error, Add a setting, Add a table column, Add a tab), checks, and the
   performance budget. A short "Before you open a PR" checklist at the end: both themes,
   375 px, keyboard, all four states, messages, `/dev/design` updated.

10. **Cleanup.** Remove dead CSS classes and unused components (`ui.module.css` leftovers,
    old layout classes); the lint override list for file length is empty; no TODOs left
    from the series.

## Tests

- The contrast test, the size check, the axe checks and the mutation-feedback rule run
  in CI and pass.
- A deliberately broken fixture (raw colour, `title=`, raw `<button>`) fails lint (a
  test or a documented manual check in the PR).

## Done when

- All checks pass in CI; `DESIGN.md` is complete; the PR lists before/after bundle
  sizes, LCP/CLS, and the audits done.
- A new page built by following `DESIGN.md` recipes alone looks and behaves like the
  rest of the app.
