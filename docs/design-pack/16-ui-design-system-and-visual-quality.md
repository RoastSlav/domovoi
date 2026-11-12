# 16 — UI design system, visual language and quality requirements

**Status:** preserve the useful visual identity and engineering discipline established in the current overhaul; redesign layouts and components where product requirements demand it.

## 1. What to preserve from the current visual identity

The current `web/DESIGN.md` and `src/styles/tokens.css` establish a deliberate identity that the user likes. Preserve its roles and semantic approach unless a new design prototype proves a better option:

- Warm charcoal/sand dark theme and paper/ink light theme.
- Moss green as the action/accent color.
- Walnut as a restrained brand marker.
- Distinct semantic status tones: danger, warning, success, pending, progress, info and neutral.
- Color-coded messages/statuses with text labels/icons; never rely on color alone.
- Inter Variable/system sans typography and monospace for technical values, code, IDs, arguments and logs.
- A 4 px spacing rhythm, consistent control geometry, semantic tokens and a controlled z-layer model.
- Calm surfaces and readable contrast; visual effects should improve hierarchy or interaction.

Do not force the exact current card/table/sidebar composition into future pages. Preserve brand and semantic vocabulary; reinvent composition and interaction when that is the better solution.

## 2. Current design system is a strong base, not a prison

Current implementation includes:

- `web/DESIGN.md` as the design source of truth for web implementation rules.
- `web/src/styles/tokens.css` and `global.css` for palette, semantic colors, spacing, typography, controls, layout, motion and layers.
- `web/src/design/tone.ts`, vocabulary modules and `icons.tsx` for statuses, wording and icons.
- Shared components for cards, tables, buttons, menus, overlays, tabs, timelines, forms, loading, errors and feedback.
- `/dev/design` as a component/token demonstration surface.
- UI tests for accessibility, contrast, layout behavior, interactions and guardrails.

Use these as a code/quality starting point after checking that they fit the new product. New graph/inspector/operation components may need a distinct design system extension rather than forcing existing generic components to do work they were not designed for.

## 3. Modern feel without visual noise

Recommended visual characteristics:

- Clear depth and layering for surfaces, overlays, graph canvas and inspectors.
- Strong information hierarchy and excellent typography.
- Compact data presentation where it helps scanning, with room for dominant workspaces.
- Polished feedback for loading, selection, hover, focus, progress and state change.
- Restrained accent color and semantic status colors.
- Clear node/edge grammar for graphs and clear grouping for operational traces.
- Minimal redundant borders, repeated cards and decorative metric tiles.
- Carefully considered empty states and onboarding.

Avoid:

- Generic “AI dashboard” aesthetic (neon gradients, glowing everything, gratuitous blur, huge empty metric cards).
- Unnecessary glassmorphism, excessive shadows and constant motion.
- Animations that hide content, delay interaction or make status hard to parse.
- Color-only states or status without a textual explanation.
- Dense pages that achieve density by shrinking text or clickable targets below usable sizes.

## 4. Information density modes

Detailed is the reference design. Build layouts with the complete information model, then allow alternate presentation preferences.

### Detailed

- Full status metadata, exact timestamps where useful, phase/step detail, evidence, inputs/outputs, policy rationale, tool results and history.
- P0 and P1 items expanded by default where space permits.
- Raw/verbose payloads remain available in inspectors/tabs, with secrets redacted.

### Standard

- Phase-level execution and important results visible.
- Individual capability calls, long outputs and secondary metadata grouped/expandable.
- Primary action and waiting state remain obvious.

### Essential

- High-level outcome, current status and next action emphasized.
- Detailed information is never deleted or inaccessible; it is collapsed/available through a clear details path.

### Custom

- Per-area visibility/expanded defaults, table columns, dashboard layout, saved filters and operation graph defaults.
- The user can choose a compact overview but a detailed operation workspace.

These are not separate application versions. They are view preferences over the same information and controls.

## 5. Responsive design and real-estate rules

Design references to validate:

- Wide desktop: 1440–1920 px.
- Standard laptop: 1280 px.
- Narrow laptop/tablet: 1024/768 px.
- Phone: 375–430 px.

Requirements:

- Do not simply scale desktop columns down. Reconsider which areas become tabs, stacked sections, sheets or full-screen inspectors.
- Dominant graphs need a minimum practical viewport. On phones, use a focused graph canvas plus a node list/inspector sheet; do not render a tiny unreadable desktop graph.
- Keep high-priority state/actions visible; use sticky action summaries only when they do not obstruct content.
- Avoid horizontal page scrolling. A graph may have its own pan/zoom canvas; a data table may have a deliberate responsive stacked form.
- Side-by-side Operator/operation workspaces should become a tab/split toggle on narrow screens while retaining the same underlying context.
- Tables must support responsive column selection or card/list transformation without losing key fields.
- Dialogs must fit touch screens and allow the content/action area to scroll.

## 6. Interaction and component library direction

Use appropriate components by job:

- Graph canvas and node inspector for orchestration.
- Timeline/activity stream for history.
- Data grid/table for large sortable/filterable inventories.
- Split view for conversation plus work/evidence.
- Inspector drawer for contextual detail.
- Side panel for the global Operator.
- Wizard/stepper for install, recovery and multi-step configuration.
- Policy matrix plus rule editor and evaluation trace for autonomy.
- Knowledge browser plus graph explorer for environment facts/relationships.

Don't make every entity a card. Don't put a graph in a generic Card just because a Card component exists. Don't make a tab out of every section. Component reuse should follow appropriate behavior and semantics.

## 7. Interaction and feedback standards

- One clear primary action per context; secondary actions remain visible and dangerous actions use clear labels and consequence previews.
- Every mutation shows progress and an outcome where the user is looking.
- Never claim a host-side operation succeeded before the actual result arrives.
- Preserve typed input on recoverable failure.
- Warn before navigation only when unsaved user input would be lost; do not interrupt for normal navigation while an operation runs.
- A running operation is durable; closing a page/panel does not cancel it.
- Disabled controls explain why and how to enable them.
- Destructive or high-impact UI actions display exact target, effect, scope and rollback/expiry where applicable.
- Secrets are entered/replaced via safe controls and never re-displayed as plaintext.

## 8. Accessibility requirements

- WCAG AA contrast as baseline for text and interactive controls.
- Full keyboard navigation, visible focus and sensible focus restoration on closing dialogs/panels.
- Correct semantic landmarks, headings, labels, tables, tabs, tree/graph alternatives and status announcements.
- Every icon-only action has a name/tooltip, with visible text in critical workflows.
- Color is never the only signal.
- Support zoom and text scaling without clipping controls or losing graph access.
- Respect reduced motion.
- Provide a non-canvas/list representation of graph structure and a keyboard-accessible node navigation model.
- Test with automated accessibility checks and manual keyboard/screen-reader review for complex interactions.

## 9. Performance and stability requirements

- Avoid layout shift during initial load and live updates.
- Keep global shell responsive even when a graph or table is large.
- Use route-level lazy loading and component-level lazy loading for heavy visualizations/editors where useful.
- Virtualize long lists, logs, event streams and graph detail lists when measurements justify it.
- Memoize stable list/table rows and avoid re-rendering the full graph for a single node status update.
- Batch live event processing and throttle high-frequency rendering without losing durable events.
- Keep graph layout and physics work off the main UI thread where necessary; use deterministic/cached layout for stable historical traces.
- Ensure search/index data is permission-filtered and never stale without an indicator.
- Keep the hub as the only required origin for bundled web assets unless an external service is explicitly required; do not introduce remote fonts or untrusted scripts by default.
- Maintain an explicit bundle/performance budget and test at realistic fleet sizes.

## 10. Quality requirements for every page

Every page must explicitly design and test:

- Loading, empty, first-load error, refresh error, offline, partial, stale and permission-denied states.
- Dense and sparse data.
- Long names, long logs, missing values and unknown enum values from newer agents/providers.
- Keyboard and screen-reader behavior.
- 375, 768, 1280 and 1920 px layouts (or justified equivalents).
- Both light and dark themes.
- Detailed, Standard and Essential presentation profiles.
- Live updates while the user is interacting with or inspecting content.
- Long-running work, reconnects, uncertain outcomes and duplicate event delivery.

A UI is not accepted only because the happy path looks good in a screenshot.
