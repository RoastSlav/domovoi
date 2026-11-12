# 15 — Interaction, animation and live visualization specification

**Status:** target interaction contract. Timing figures below are recommended starting values to prototype and tune, not immutable pixel-perfect design decisions.

## 1. Motion is functional feedback

Domovoi should feel modern, fast and alive. Motion must explain a state change, spatial relationship, progress or result. Avoid animation used only to decorate an otherwise static page.

Two classes of motion exist and they cover different cases. `web/DESIGN.md` defines both:

- **Micro-interactions** keep the existing 120 ms and 180 ms tokens (hover, focus, disclosure, menus, dialogs).
- **Visualization and transition motion** is a separate class for the graph, layout changes, side panels and playback, with its own longer durations. It was not covered by the original DESIGN.md rule, so the two do not conflict.

Both must be interruptible and non-blocking, and both are off or instant under reduced motion.

## 2. Motion tokens

### Micro-interaction class (existing tokens)

| Token / behavior | Duration | Easing | Use |
|---|---:|---|---|
| Hover/focus/pressed | 120 ms (`--motion-fast`) | Ease out | Buttons, rows, chips, selection affordances. |
| Small disclosure | 180 ms (`--motion-base`) | Ease out | Expand/collapse a short section or inline detail. |
| Popover/menu | 120-180 ms | Ease out | Search palette, menus, small pickers. |
| Dialog | 180 ms | Ease out | Focused dialog; scrim fades in slightly faster. |

### Visualization and transition class (planned tokens, added with the operation UI)

| Token / behavior | Duration | Easing | Use |
|---|---:|---|---|
| Side panel and inspector open/close | 240 ms | Ease out | Operator panel and contextual inspector; `transform` only, content is padded not reflowed. |
| Graph node entry and state change | 240 ms | Ease out | Node enters, is selected, or changes state. |
| Status settle | 240 ms | Ease out | Running to succeeded/failed/waiting. |
| Graph edge progression | up to 400 ms | Linear or slight ease | Directional active-path reveal; use sparingly. |
| Layout transition | 240-400 ms | Ease in-out | Collapse/expand of a node group, re-layout after a cycle is added, fit-to-view. |
| Graph playback | User-controlled | Consistent playback clock | Replay of a completed operation; speed is chosen by the user. |

Rules for the class:

- Recommended 240-400 ms; **a single transition never exceeds 600 ms**.
- Event bursts are batched: animation never delays showing state. At most five nodes animate at once; further arrivals in the same batch appear in their final state without a transition.
- Page content transitions on route change are not used; lazy pages show their skeleton in place.
- Use the centralized motion tokens (a CSS custom property plus a JS reader that returns 0 under reduced motion); never hard-code durations in components.
- Reduced motion: every transition in both classes becomes instant, and every state change remains visible (label, icon, outline, text). No information is conveyed only by motion.

## 3. Accessibility and reduced motion

- Respect `prefers-reduced-motion` everywhere.
- In reduced-motion mode, remove non-essential transforms, pulsing, animated edges and parallax. Replace them with static state, directional arrows or concise text.
- Never communicate status with movement or color alone; retain labels/icons/text.
- Avoid flashing, rapid brightness changes or continuous pulses on warning/error states.
- Do not animate large content in a way that causes layout shift or moves keyboard focus. A test helper asserts that `document.activeElement` is unchanged across a simulated burst of live events.
- Every animation must be compatible with keyboard navigation and screen readers; use appropriate live-region announcements for meaningful state changes (approval needed, failure, completion) without reading every telemetry event.

## 4. Orchestration graph: the signature visualization

### Purpose and visual grammar

The graph shows the actual operation lifecycle and relationships between execution events. It is not a static workflow diagram and not an invented linear progress tracker.

Node types may include:

- Orchestration phase (detect, enrich, assess, plan, authorize, execute, verify/monitor, resolve/escalate).
- Reasoning request/structured response.
- Evidence/capability request and response.
- Policy evaluation.
- User question/approval/manual decision.
- Execution step/provider action.
- Verification check and monitoring window.
- Branch/condition, loop/cycle, wait, compensation or cancellation event.

The graph must visually distinguish **what happened** (durable node/event) from **what is planned** (future branch/step). A proposed node is not a completed event. A predicted next step should be visibly marked as planned, not drawn as if already executed.

### Required graph states

- Pending/not started.
- Running/active.
- Completed successfully.
- Failed.
- Waiting for user/input/approval/external provider.
- Skipped/not applicable.
- Cancelled/stopped.
- Retrying/replanned.
- Uncertain outcome (for example, agent disconnected after dispatch; result not confirmed).
- Monitoring/delayed verification.

Each state has text and an icon/tone; color is supplemental.

### Layout behavior

- Use a readable directed graph. Linear operations can use a simple flow; parallel and branching work must expose branches and joins.
- Cycles must be visually grouped and numbered; do not hide earlier failed cycles.
- Parallel branches should be legible and selection should not jump the viewport whenever one finishes.
- Long graphs need minimap or overview, fit-to-view, zoom controls, pan, cycle filters and a way to jump to the active node.
- Provide the step timeline/tree and the non-canvas outline as alternate views for accessibility, mobile and graph density. The timeline is built first; the graph and the timeline read the same frozen trace schema.
- Minimum graph canvas is 640 x 400 CSS px; below that (and on phones) show the outline with a node sheet instead of a shrunken graph.
- If graph complexity exceeds usable space, collapse logical groups while showing counts and status breakdown; do not silently drop nodes.
- Default framing should keep the current active path visible, but users can disable live-follow and explore history without being pulled back.
- **Following** means the live-follow toggle is on. Any manual pan, zoom or selection of a node outside the active path switches it off, and a visible "Jump to active node" control (also a key) turns it back on. The graph pans only while following.

### Keyboard navigation

The graph canvas is one tab stop with roving focus over nodes. Traversal follows the graph, not screen position:

- **Arrow keys** move to a connected node: Right/Down to a child (the next in branch order when there are several), Left/Up to a parent; when a node has several children or parents, repeated presses cycle through them in the order the outline lists them.
- **Enter** opens the inspector for the focused node (and moves focus into it); **Escape** returns focus to the node.
- **Home / End** move to the first and the last node in trace order.
- **A key (default `J`)** jumps to the active node, turning live-follow on; if nothing is active it goes to the last node.
- **`+`, `-`, `0`** zoom in, out and fit; **`Space`** expands or collapses a focused group.
- The focused node scrolls into view without changing zoom; focus is never moved by live events.
- Focus and selection are separate: arrows move focus, Enter selects.

The **outline** is the screen-reader path: a nested list (cycle, branch, node) in trace order, each item stating name, status, owner and its connections, with the same Enter-to-inspect and jump-to-active behavior. It is real DOM, not an overlay on the canvas, and stays in sync with the graph selection. Manual review uses NVDA with Firefox and VoiceOver with Safari before release.

### Node selection and inspector

- Single click selects a node and opens/updates the inspector.
- Selection is independent of execution; it never changes an operation.
- Selecting a node highlights its upstream evidence and downstream dependents.
- Double click or explicit expand affordance opens nested child events where available.
- Inspector can be pinned, resized and closed. It shares the right-hand slot with the Operator panel under the precedence rule in doc 13.
- New live events must not clear the selected node or close the inspector.
- If the selected node becomes unavailable due to filtering, indicate that it is filtered out and offer “show selected node”; do not silently switch selection.

### Graph updates and animation

- A new node appears with a subtle 240 ms opacity/position transition (visualization class, section 2); during a burst only the first five animate.
- The active edge may reveal direction over up to 400 ms; avoid a permanent high-frequency flow animation.
- Running node state uses a restrained activity indicator, not a large spinning animation.
- Completion transitions to final state once, then becomes still.
- Failure gets a brief emphasis once, then remains visible as a stable error state. "Once" is tracked in a seen-set keyed by node id and state version, so a reload, reconnect or historical view does not replay it.
- Waiting nodes show the waiting reason (approval, question, rate limit, provider unavailable, scheduled delay, etc.).
- When an operation loops, visually connect the new cycle to the prior failed/verified state and mark what changed in the re-plan.
- Live-follow may scroll/pan only while following (see layout behavior above).

### Graph actions

- Inspect node.
- Open linked host/service/capability/policy/incident/knowledge entity.
- Show arguments and outputs (with secrets redacted and safe formatting).
- Retry/re-run only through the operation's permitted action path; do not provide a generic retry button on every failed node.
- Stop/cancel/pause only when supported by the underlying operation and policy; clearly state whether stopping prevents further work or only cancels future steps.
- Compare cycles/plans.
- Enter historical playback mode for completed operations: replay of the stored trace through the same renderer, with play/pause, scrub and a user-chosen speed. Playback is built after the live graph and never changes the operation.

## 5. Operation and incident state transitions

- A state transition must be visible where the user is currently looking and recorded in durable history.
- Avoid optimistic UI that says an operation succeeded before the server/agent reports a confirmed result.
- Distinguish “command returned successfully”, “verification passed” and “incident resolved”.
- On a lost connection after dispatch, show “outcome unknown / checking state” rather than immediately showing success or issuing a duplicate action.
- A new investigation cycle should say why it started (failed verification, new evidence, user request, recurrence, timeout).
- A failed step must not disappear when a later step succeeds.
- Long-running work should support safe navigation away and return; page closure must not be interpreted as cancellation.

## 6. Operator interactions

### Side panel

- Opens beside the current page when there is room and as a sheet when there is not; it never overlaps content. It shares the right-hand slot with the inspector (precedence rule in doc 13: side by side when wide; otherwise the most recently opened wins and the other collapses to a tab).
- Does not reset the current page scroll, selection, filters or open inspector.
- Composer remains accessible; sending creates a durable message and, when needed, an operation. The draft survives a failed send and a reload until the send is committed.
- Each conversation has an explicit host or global scope. A response for a request that has since been superseded is discarded, not appended.
- Panel header shows context and a clear expand-to-workspace control.
- If active operation continues after panel close, show a small global activity indicator and preserve it in Operations/notification center.
- Panel can be resized, pinned/open by shortcut, collapsed and expanded.

### Full workspace

- Conversation and operation can be shown simultaneously on wide screens.
- Splitter is keyboard accessible and has sensible min/max widths.
- Switching view does not change the conversation's context or detach operation state.
- New messages should not steal scroll when user is inspecting older messages; show a “new activity” affordance.
- Stop message generation and cancel operation are different controls with different consequences.
- The composer should support links/attachments in later milestones, but all untrusted content must be handled as data.

### Action cards inside conversation

A response that creates a proposed action/operation renders the shared structured approval card (doc 14, section 5), not a free-text promise. It shows contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, the policy decision and approval state; any model prose is secondary and labelled. Chat must not simulate success before execution has returned.

## 7. Policy simulator interactions

- Selecting a historical incident loads the stored snapshot and policy version used at the time.
- Current-policy comparison uses today's policy, but clearly states which environmental facts are historical and which are current.
- Proposed-policy comparison operates on a draft; it does not save automatically.
- Results show exact matched rules and constraints, including the winning deny/allow rule and any unknown/missing context.
- Every scenario run is read-only. Applying a draft policy is a separate explicit action with review and audit.
- The “simulate” result must call the production policy evaluator through a side-effect-free path.
- Every result renders the full effective-policy explanation (doc 14, section 12): all matching rules, the winning rule and why it won, broader rules overridden, hard limits shown separately, dangerous exceptions marked.
- Loading a historical scenario requires the context snapshot and policy version stored at decision time; if absent, say so and offer only the current-policy comparison.

## 8. Capability installation interactions

- Installation uses preflight → review → target selection → deploy → verify → configure → enable → authorize.
- Progress is shown per target agent, not only as a global spinner. The agent verifies the package signature against its local trusted keys; the UI reports what the agent said and never claims verification itself.
- A host that has not opted in to installs is shown as such, with where to change it, not as a failure.
- Partial success must be clearly represented; allow retry only for failed targets, not duplicate the entire fleet deployment.
- Signature/source verification and privilege changes must be conspicuous.
- Rollback/uninstall availability is declared and displayed before installation.
- Enabling a capability and granting it permission are separate explicit decisions.
- If an agent is offline, show pending delivery, not installed.

## 9. Tables, filters and detail surfaces

- Filters should be URL-addressable when a view is shareable and safe.
- Keep sort/filter state while opening and returning from details.
- Large lists should paginate or virtualize; row updates should preserve scroll and focus.
- A selected row opens a dedicated detail page or inspector based on object complexity and user task.
- In compact density, reduce padding before hiding meaningful columns; users can configure columns.
- Tooltips explain short/ambiguous labels but do not contain critical information exclusively.
- Bulk actions must show their exact scope and a dry-run/count preview when potentially impactful.

## 10. State taxonomy: loading, empty, error, offline and the rest

This is the one list of data states. Other documents refer to it instead of keeping their own.

| State | Trigger | What the person sees | Keeps stale data? |
|---|---|---|---|
| Loading | First fetch, nothing cached | Skeleton shaped like the content; no blank page or layout shift | no |
| Refreshing | Refetch with data present | Last data stays; a freshness label shows a refresh in progress | yes |
| Empty | Loaded, nothing to show | Why it is empty and the next step; never used for a failed load | n/a |
| First-load error | Fetch failed, no data | What failed, why, and Retry | no |
| Refresh error | Refetch failed, data present | Last data stays with "last updated" time and a visible failure notice with Retry | yes |
| Stale | Data older than its freshness window, or live updates paused | Freshness label with age; never presented as current | yes |
| Offline / unavailable | A host or agent cannot be reached, or a provider is not configured | Says which source is unavailable; distinct from "no data" and from "not configured" | last known, marked |
| Partial | Some sources of a page loaded and others did not | Each section shows its own state; a failed source is named, never silently omitted | per section |
| Unknown value | A status or schema value this UI does not know (newer hub, agent or provider) | Neutral pill with the raw value; never crashes | n/a |

Permission-denied is not a v1 state (single owner). The one function every action check goes through is the hook for future roles.

## 11. Global navigation and search interactions

- Global search and Operator are separate launchers with separate jobs.
- Search should debounce appropriately, show loading/no-results/failure, group result types, and keep keyboard selection stable as results update.
- Search results are scoped to the sources the search actually covers, and each group says so. With one owner there is no permission filtering in v1; keep a single filter point in the search layer as a hook for future roles.
- Command actions require explicit confirmation if they change state; a search selection must not accidentally run a high-impact operation.
- Breadcrumbs and contextual links preserve a clear path back.

## 12. Motion quality gate

For every animation in a page spec, document:

- Trigger and state transition.
- Element(s) affected.
- Duration and easing or spring parameters.
- Whether it can be interrupted.
- What reduced-motion mode does instead.
- How the animation affects focus, screen-reader announcements and layout.
- Why movement communicates meaning.

If there is no good answer to the last question, the animation probably should not exist.
