# UI modernization prompts

A full modernization of the web UI: a muted brown and green visual identity, status
colour coding used consistently, feedback for every action, hints on everything
non-obvious, a fast and smaller bundle, and a design system with guardrails so later
work cannot quietly degrade it.

**This series runs before every prompt in `../`.** It is separate work. It absorbs the
old prompts 23 (denser layouts, performance) and 42 (host selector everywhere, error
states), which are deleted. The prompts in `../` were updated to build on the new kit
(`StatusPill`, `useActionFeedback`, `QueryBoundary`, `PageHeader`, `DataTable`,
`Field`, `Tooltip`, `InfoTip`, …) and on `web/DESIGN.md`.

Written for a strong model (Fable 5.2 or similar). It surveys the UI on `main`.

## How to use

1. Start a fresh session for each part.
2. Paste [`../00-rules.md`](../00-rules.md), then [`00-design-brief.md`](00-design-brief.md),
   then one part.
3. One branch and one PR per part (`feat(web): …`). Merge it before starting the next
   part: each part builds on the one before.
4. After each merge, check the result yourself at phone width and on a desktop, in both
   themes. Tell the next session about anything you want changed in the direction; the
   brief is the place to record a decision that should hold for the rest of the series.

## The parts

| Part | Prompt | What it delivers | Size |
| --- | --- | --- | --- |
| 01 | [Foundation](01-foundation.md) | tokens, light/dark/system theme switch, the tone system, icons, type, lint guardrails, `DESIGN.md` v1, `/dev/design` | M |
| 02 | [Component kit](02-component-kit.md) | status pills, error catalogue, toasts, `useActionFeedback`, tooltips, info tips, loading/empty/error states, layout, data, form and overlay components | L |
| 03 | [Shell and data](03-shell-and-data.md) | sidebar, top bar, nav groups with counts, host selector everywhere, lazy routes, narrow invalidation | M |
| 04 | [Respond](04-incidents-approvals-chat.md) | incidents list and detail, actions and approvals, chat | L |
| 05 | [Fleet](05-fleet-hosts-skills-logs.md) | hosts, host detail tabs, skills, logs | M |
| 06 | [Knowledge and system](06-knowledge-settings-onboarding.md) | known conditions, notes, digest, settings, first run, sign-in, updates | L |
| 07 | [Polish and guardrails](07-polish-and-guardrails.md) | contrast test, bundle budget, accessibility and responsive audits, messaging and hint sweeps, final `DESIGN.md` | M |

## Decisions taken

- **Colours**: muted brown (frame) and green (act) on warm paper/charcoal; seven status
  tones (`danger warning success pending progress info neutral`), one file maps every
  status to a tone.
- **Themes**: light and dark, following the system by default, with a System/Light/Dark
  switch saved in the browser.
- **Dependencies**: the model may add npm packages where they do the job better, each
  justified with its bundle size, inside the performance budget.
- **API**: the model may add or change endpoints wherever a page needs it (counts,
  server-side filters, better error messages), following rule 9.
- **Maintainability**: `web/DESIGN.md` plus lint and test guardrails, so smaller models
  can extend the UI without drift.
