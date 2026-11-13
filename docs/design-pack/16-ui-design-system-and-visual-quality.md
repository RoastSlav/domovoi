# 16 — UI design system, visual language and quality requirements

**Status:** preserve the useful visual identity and engineering discipline established in the current overhaul; redesign layouts and components where product requirements demand it.

## 1. What to preserve from the current visual identity

The current `web/DESIGN.md` and `src/styles/tokens.css` establish a deliberate identity that the user likes. Preserve its roles and semantic approach unless a new design prototype proves a better option:

- Warm charcoal/sand dark theme and paper/ink light theme.
- Moss green as the action/accent color.
- Walnut as a restrained brand marker.
- Distinct semantic status tones: danger, warning, success, pending, progress, info and neutral. New statuses (the `Incompatible` host state, the root-agent warning, "skill runner missing", "helper unavailable", the v1 versus v2 approval-binding label, the locked-Deny treatment, and the `Refused`, `NotRun`, `Unavailable` and `OutcomeUnknown` outcomes) each get a label, tone, hint and icon through the shared status vocabulary (see the planned section of `web/DESIGN.md`).
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

The guardrails in `web/DESIGN.md` stay in force. Where the graph genuinely needs something they forbid, the amendment is written in `web/DESIGN.md` (its "planned, added with the operation UI" section) before the code, not worked around:

- **Computed geometry** (node positions, viewBox, transforms) uses SVG attributes and CSS custom properties set through one kit helper; there is no blanket exemption from the inline-style rule.
- **File size:** the 250-line cap is not relaxed. The graph is split by responsibility (layout, rendering, interaction, keyboard, outline).
- **Motion** gets the second class and tokens described in doc 15, section 2.
- **Layout worker:** if layout runs in a worker, it is a same-origin hashed asset (the content policy does not allow `blob:` workers).

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

## 4. Presentation profiles

Detailed is the reference design and the default for a new owner. Build layouts with the complete information model, then derive the other profiles by hiding or collapsing, never with a different layout. The mechanism (a profile provider, one visibility primitive keyed by category, and the table below) is built before any page, after the shared visibility infrastructure.

| Category | Detailed | Standard | Essential |
|---|---|---|---|
| P0 safety, approval, audit, current state | shown | shown | shown |
| P1 primary | shown | shown | shown |
| P2 technical detail | shown | collapsed | collapsed |
| P3 metadata | shown | collapsed | hidden, "+N details" indicator |

- **Detailed:** full status metadata, exact timestamps where useful, phase/step detail, evidence, inputs/outputs, policy rationale, tool results and history. Raw/verbose payloads remain in inspectors/tabs, with secrets redacted.
- **Standard:** primary content and the primary action and waiting state are obvious; capability calls, long outputs and secondary metadata are grouped and one click away.
- **Essential:** outcome, current status and next action; technical detail stays reachable through collapsed sections and the "+N details" indicator. Nothing is deleted or inaccessible.
- **Custom:** per-page overrides of one of the profiles (and table columns, saved filters, graph defaults; a customizable dashboard layout is DEFERRED to v1.x, `docs/rework/06` "Deferred and after-v1"). A person can choose a compact overview and a Detailed operation workspace.

P0 is never hidden or collapsed in any profile, Custom included; a test asserts it for every page. Collapsed and hidden content is not rendered until revealed, so profiles also keep cost down. These are view preferences over the same information and controls, never separate application versions and never a change to authority.

## 5. Responsive design and real-estate rules

Design references to validate:

- Wide desktop: 1440–1920 px.
- Standard laptop: 1280 px.
- Narrow laptop/tablet: 1024/768 px.
- Phone: 375–430 px.

Requirements:

- Do not simply scale desktop columns down. Reconsider which areas become tabs, stacked sections, sheets or full-screen inspectors.
- Dominant graphs need at least 640 x 400 CSS px of canvas. Below that, and on phones, show the outline plus a node sheet; do not render a tiny unreadable desktop graph. P0 safety actions (approve, deny, stop) work on a phone.
- Keep high-priority state/actions visible; use sticky action summaries only when they do not obstruct content.
- Avoid horizontal page scrolling. A graph may have its own pan/zoom canvas; a data table may have a deliberate responsive stacked form.
- Side-by-side Operator/operation workspaces should become a tab/split toggle on narrow screens while retaining the same underlying context. The right-hand slot follows the precedence rule in doc 13.
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
- Provide the non-canvas outline of graph structure (the screen-reader path) and the keyboard navigation model defined in doc 15, section 4.
- Test with automated accessibility checks and manual keyboard review; complex interactions get a manual screen-reader pass with NVDA and Firefox and with VoiceOver and Safari.

## 9. Performance and stability requirements

Budgets win over presentation: when Detailed would exceed a budget on a given device, it degrades by rendering less at once (outline instead of canvas, collapsed groups, virtualised lists), never by dropping information.

- Avoid layout shift during initial load and live updates.
- Keep global shell responsive even when a graph or table is large.
- Use route-level lazy loading and component-level lazy loading for heavy visualizations/editors. The graph is its own lazy chunk under the existing 120 kB gzip chunk limit; initial JavaScript stays inside the budget in `web/DESIGN.md`.
- Virtualize long lists, logs, event streams and graph detail lists.
- Memoize stable list/table rows and avoid re-rendering the full graph for a single node status update.
- Batch live event processing and throttle high-frequency rendering without losing durable events; coalesce animation as in doc 15, section 2.
- Keep graph layout and physics work off the main UI thread where necessary; use deterministic layout for stable historical traces. A layout version is recorded so a historical trace renders the same after a release.
- Search results carry a freshness indicator; nothing is presented as current when it is stale.
- Keep the hub as the only required origin for bundled web assets unless an external service is explicitly required; do not introduce remote fonts or untrusted scripts by default.
- Test at fixed fleet sizes: 50 hosts, 5,000 operations, 2,000 nodes in one operation and 100,000 log lines. Interaction stays responsive and a single node update re-renders only that node.

## 10. Quality requirements for every page

Every page must explicitly design and test:

- Every state in the canonical taxonomy that applies (doc 15, section 10): loading, empty, first-load error, refresh error, stale, offline/unavailable, partial and unknown value (plus `Incompatible` and "helper unavailable" for hosts). Permission-denied is not a v1 state (single owner).
- Dense and sparse data.
- Long names, long logs, missing values, unknown enum values (shown as the raw value) and the host `Incompatible` state.
- Keyboard and screen-reader behavior.
- Both light and dark themes.
- Detailed, Standard and Essential presentation profiles, with the P0-never-hidden assertion.
- Live updates while the user is interacting with or inspecting content.
- Long-running work, reconnects, uncertain outcomes and duplicate event delivery.

### Sampling rule for the review matrix

The full product of profiles, themes, widths and states is not run for every page. Required:

- **Every page:** all three profiles x both themes at two widths (375 and 1280 px).
- **Default profile (Detailed) only:** the four widths 375, 768, 1280 and 1920 px, both themes.
- **States:** each applicable state from the taxonomy is covered once per page, in the default profile, in one theme and one width; the visual snapshot uses the layout that is most at risk.
- **Graph pages:** the graph states in doc 15 are covered by component tests on fixtures, not repeated per page and profile.

A UI is not accepted only because the happy path looks good in a screenshot.
