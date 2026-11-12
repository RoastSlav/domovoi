# 15 — Interaction, animation and live visualization specification

**Status:** target interaction contract. Timing figures below are recommended starting values to prototype and tune, not immutable pixel-perfect design decisions.

## 1. Motion is functional feedback

Domovoi should feel modern, fast and alive. Motion must explain a state change, spatial relationship, progress or result. Avoid animation used only to decorate an otherwise static page.

Existing `web/DESIGN.md` defines short 120–180 ms motion tokens and reduced-motion support. Preserve that calm baseline for ordinary controls. Complex graph interactions may need longer transitions, but they must remain interruptible and non-blocking.

## 2. Recommended motion tokens

| Token / behavior | Starting duration | Recommended easing | Use |
|---|---:|---|---|
| Hover/focus/pressed | 100–140 ms | Ease out | Buttons, rows, chips, selection affordances. |
| Small disclosure | 140–180 ms | Ease out | Expand/collapse a short section or inline detail. |
| Popover/menu | 120–180 ms | Ease out | Search palette, menus, small pickers. |
| Side panel | 180–240 ms | Ease out | Operator panel and contextual inspector. |
| Dialog | 160–220 ms | Ease out | Focused dialog; scrim fades in slightly faster. |
| Page content transition | 140–200 ms | Ease out | Route content change while shell remains stable. Avoid transition that delays navigation. |
| Graph node activation | 160–220 ms | Ease out | Node state/selection/entry. |
| Graph edge progression | 250–500 ms | Linear or slight ease | Directional active-path reveal; use sparingly. |
| Status settle | 180–260 ms | Ease out | Running → succeeded/failed/waiting. |
| Graph playback | User-controlled | Consistent playback clock | Historical timeline replay, not a live operation. |

All values are starting points. Use CSS motion tokens or a centralized motion layer; do not scatter hard-coded durations through components.

## 3. Accessibility and reduced motion

- Respect `prefers-reduced-motion` everywhere.
- In reduced-motion mode, remove non-essential transforms, pulsing, animated edges and parallax. Replace them with static state, directional arrows or concise text.
- Never communicate status with movement or color alone; retain labels/icons/text.
- Avoid flashing, rapid brightness changes or continuous pulses on warning/error states.
- Do not animate large content in a way that causes layout shift or moves keyboard focus.
- Every animation must be compatible with keyboard navigation and screen readers; use appropriate live-region announcements for meaningful state changes without reading every telemetry event.

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
- Provide a compact list/timeline representation as an alternate view for accessibility, mobile and graph density.
- If graph complexity exceeds usable space, collapse logical groups while showing counts and status breakdown; do not silently drop nodes.
- Default framing should keep the current active path visible, but users can disable auto-follow and explore history without being pulled back.

### Node selection and inspector

- Single click selects a node and opens/updates the inspector.
- Selection is independent of execution; it never changes an operation.
- Selecting a node highlights its upstream evidence and downstream dependents.
- Double click or explicit expand affordance opens nested child events where available.
- Inspector can be pinned, resized and closed.
- New live events must not clear the selected node or close the inspector.
- If the selected node becomes unavailable due to filtering, indicate that it is filtered out and offer “show selected node”; do not silently switch selection.

### Graph updates and animation

- A new node appears with a subtle 160–220 ms opacity/position transition.
- The active edge may reveal direction over 250–500 ms; avoid a permanent high-frequency flow animation.
- Running node state uses a restrained activity indicator, not a large spinning animation.
- Completion transitions to final state once, then becomes still.
- Failure gets a brief emphasis once, then remains visible as a stable error state.
- Waiting nodes show the waiting reason (approval, question, rate limit, provider unavailable, scheduled delay, etc.).
- When an operation loops, visually connect the new cycle to the prior failed/verified state and mark what changed in the re-plan.
- Live-follow may scroll/pan only when explicitly enabled or when the user is already following the active path.

### Graph actions

- Inspect node.
- Open linked host/service/capability/policy/incident/knowledge entity.
- Show arguments and outputs (with secrets redacted and safe formatting).
- Retry/re-run only through the operation's permitted action path; do not provide a generic retry button on every failed node.
- Stop/cancel/pause only when supported by the underlying operation and policy; clearly state whether stopping prevents further work or only cancels future steps.
- Compare cycles/plans.
- Enter historical playback mode for completed operations.

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

- Opens over or beside the current page depending on viewport.
- Does not reset the current page scroll, selection, filters or open inspector.
- Composer remains accessible; sending creates a durable message and, when needed, an operation.
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

A response that creates a proposed action/operation renders a structured action card, not a free-text promise. It shows target, intended effect, permission outcome, constraints, approval state and verification plan. Chat must not simulate success before execution has returned.

## 7. Policy simulator interactions

- Selecting a historical incident loads the stored snapshot and policy version used at the time.
- Current-policy comparison uses today's policy, but clearly states which environmental facts are historical and which are current.
- Proposed-policy comparison operates on a draft; it does not save automatically.
- Results show exact matched rules and constraints, including the winning deny/allow rule and any unknown/missing context.
- Every scenario run is read-only. Applying a draft policy is a separate explicit action with review and audit.
- The “simulate” result must call the same policy evaluator as real execution, using a simulation context that prohibits side effects.

## 8. Capability installation interactions

- Installation uses preflight → review → target selection → deploy → verify → configure → enable → authorize.
- Progress is shown per target agent, not only as a global spinner.
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

## 10. Empty, loading, error and offline interactions

Every data-driven surface needs a meaningful state model:

- Initial loading: skeleton shaped like the eventual content; avoid blank page and layout shift.
- Refreshing with stale data: keep last known data and show freshness/refresh indicator.
- Empty: state why there is nothing and how to create/populate it.
- Error: distinguish first-load error from refresh error; preserve usable stale data and offer retry.
- Offline/unavailable: distinguish host/agent unavailable from “no data” and from provider not configured.
- Partial: show which sections succeeded and which failed; a failed source must not be silently omitted.
- Permission denied: explain which permission or role is missing and how an authorized person can fix it.
- Unknown schema/status: render safely as an unknown value; never crash on a newer provider/agent response.

## 11. Global navigation and search interactions

- Global search and Operator are separate launchers with separate jobs.
- Search should debounce appropriately, show loading/no-results/failure, group result types, and keep keyboard selection stable as results update.
- Search respects authorization and scope; never return object names or snippets the user is not allowed to inspect.
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
