# 17 — UI user journeys and acceptance criteria

This document cross-checks pages as workflows. The UI should be designed as one connected operating environment, not a set of isolated screens.

## 1. Journey: morning environment review

**Goal:** user understands environment state and what deserves attention.

1. Open Control Room.
2. See active incidents, hosts that need attention, operations currently running/waiting and approvals/questions.
3. Select an operation and open its detailed graph without losing the overview context.
4. Inspect a failed/waiting node, then navigate to its host/capability/policy details.
5. Return to the Control Room with filters and scroll position preserved.

**Acceptance:**
- Critical attention is distinguishable from informational activity.
- The overview shows active operations, not only incidents and approvals.
- In the default layout no counter is duplicated across tile, heading and filter (user-rearranged layouts are exempt).
- Clicking a metric opens the relevant filtered list.
- Returning preserves the useful context; live changes are reflected without a full-page reset.

## 2. Journey: troubleshoot an unfamiliar issue through Operator

**Goal:** ask for help without needing to know which page or capability is required.

1. Open global Operator from any page.
2. Ask: “Why is this container running out of memory?”
3. Operator resolves target/context or asks one concise clarification.
4. It creates a durable investigation operation when real work is required.
5. The panel shows progress; user can continue browsing.
6. Expand to full Operator workspace.
7. Inspect orchestration graph, tool calls, evidence, hypotheses and missing information.
8. Answer a follow-up question or approve an allowed operation.
9. See execution and verification results.
10. Close Operator; operation remains in Operations and is discoverable later.

**Acceptance:**
- Side panel and full workspace share conversation and operation state.
- Chat is not a parallel execution engine; it routes work through the common orchestrator.
- Pure questions do not needlessly create an operation.
- User can tell what context was used and which host/target is in scope.
- A panel close or route change never cancels durable work.

## 3. Journey: restart a service

**Goal:** carry out a straightforward user request safely and with minimal friction.

1. User asks: “Restart Plex for me.”
2. Domovoi resolves the correct host/service and confirms ambiguity if more than one Plex instance exists.
3. It checks capability availability and current policy.
4. If authorized automatically, it presents the intended effect and executes; if approval is required, it shows the shared approval card inline and the same item appears in the attention center; if denied, it explains the winning rule and why it won.
5. Operation graph records capability invocation and result.
6. Verification checks the service/health condition, not only command exit code.
7. The user receives a truthful result and the operation is stored in history.

**Acceptance:**
- Target, effect and permission outcome are explicit before a mutation, rendered by the shared approval card (contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, policy decision).
- Policy is evaluated in the shared engine; chat cannot bypass it.
- Execution and verification are separate states.
- Duplicate requests/dispatches are guarded against where operations are not idempotent.

## 4. Journey: review and tune autonomy

**Goal:** user gains confidence in which permissions are right for their environment.

1. Open Policies & Autonomy.
2. Review effective permissions by host and capability/operation.
3. Choose a historical incident/operation.
4. Open “What would Domovoi have been allowed to do?”
5. Compare policy at the time, current policy and a draft policy.
6. Inspect rule matches, inheritance, constraints, missing context and differences.
7. Edit the draft if needed, simulate again, then explicitly apply it (with password re-entry).
8. Audit records the policy change; no scenario simulation ever executes a capability.

**Acceptance:**
- Original/current/proposed policy views are clearly distinguished.
- Historical context and current facts are never silently mixed.
- Simulation calls the same deterministic evaluator as real operation authorization.
- All matching rules, the winning rule, why it won, the broader rules it overrides and any constraint failures are visible; hard limits (host ceiling, pause) are shown apart from rules; a specific Allow over a broad Deny is visibly flagged.
- Applying a draft is a distinct audited action.

## 5. Journey: install a capability from the UI

**Goal:** add new functionality to one or more agents safely.

1. Open Capability Center and search catalog.
2. Open capability detail; inspect source/trust, effect, permissions, schemas, prerequisites and rollback.
3. Select target agents and rollout strategy.
4. Run preflight and review exactly what will change.
5. Install on each agent that has opted in; the agent verifies the signature against its local trusted keys; show progress and partial failures per host.
6. Configure parameters.
7. Enable the capability if wanted.
8. Set policy permissions separately.
9. Run a safe test and inspect its operation record.

**Acceptance:**
- Installed, enabled and authorized are separate properties; no single control does more than one of them.
- Hosts that have not opted in to installs are shown as such, not as failed.
- An offline agent is shown as pending delivery, not installed.
- Package provenance and integrity checks are visible.
- Failed agents can be retried without reinstalling successfully updated agents.
- Model-generated capability drafts cannot self-install or self-authorize.

## 6. Journey: inspect what happened after an operation completed

**Goal:** understand all decisions and outcomes after the fact.

1. Open Operations and filter Completed/Failed.
2. Open an operation.
3. View the historical graph, switch to the timeline/tree or the outline, or play it back.
4. Select an earlier failed node and inspect its input/output/error.
5. Compare cycle 1 and cycle 2 to see which evidence changed the plan.
6. Open the policy evaluation and matched rule.
7. Follow links to capability, host, incident, related knowledge and audit entry.
8. Open a similar past operation and read its resolution summary.

**Acceptance:**
- Historical operation state is stable and reproducible.
- Failed steps remain visible even after later success.
- Event timestamps, correlation IDs, target and actor/component are available.
- Raw outputs are redacted and have clear truncation/freshness markers.

## 7. Journey: learn and correct environment knowledge

**Goal:** user can see and fix what Domovoi believes about their environment.

1. Open Knowledge and filter by host/source/freshness.
2. Inspect a fact, its source, verification state and linked evidence.
3. Correct or mark it stale/disputed/delete it.
4. Inspect relationships and linked notes/operations.
5. Review a suggested knowledge item from a resolved incident.
6. Accept/edit/reject it without changing autonomy permissions.

**Acceptance:**
- Facts never become user-confirmed without an explicit user action.
- Deletion removes the content from retrieval indexes/caches as defined by data policy.
- Provenance and conflicting claims remain inspectable.
- Knowledge edits are audited and do not imply permission changes.

## 8. Journey: decide an approval from the attention center

**Goal:** make a safe decision quickly without hunting for context.

1. A step needs approval; the attention center shows it and the shell indicator counts it.
2. Open the item; the shared approval card shows contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix and the policy decision.
3. If the target is outside the operation's scope, the card says so and offers approval only.
4. Open the operation from the card to see the graph and the evidence behind the step.
5. Approve (or deny); the card and the operation show the decision, who made it and when.

**Acceptance:**
- The same component renders the approval on operation detail, incident detail, the Control Room, the Operator and the attention center.
- Model-written text is labelled and secondary.
- Only an authenticated person can approve; nothing in the UI, the Operator or chat approves on a person's behalf.
- An expired approval is shown as expired with the reason, not as pending.

## 9. Journey: resolve an incident with past experience

**Goal:** reuse what worked before without treating it as permission.

1. Open an incident or operation and find "Similar past operations".
2. Read each resolution summary: outcome, what was done, whether it was verified, and which fields came from the model.
3. Confirm or edit a model-drafted summary on a finished operation.
4. Use it to guide the new work; any step still goes through current policy and approval.

**Acceptance:**
- Every item is marked historical, with its operation, date, verified flag and source.
- A past approval or success never satisfies policy or pre-approves anything.
- Model-drafted narrative fields are labelled until the owner confirms or edits them.

## 10. Cross-page interaction acceptance

- An object has one canonical detail route; previews link to it.
- Operator side panel can open from any page and expand without losing context.
- Operation can be started from chat, incident, host, capability detail or scheduled automation and appears in the same Operations system.
- Incident pages distinguish the detected problem from operations created to handle it.
- Policy decisions link to the operation and rule that produced them.
- Capability runs link to the installed capability and the operation that invoked it.
- Knowledge links connect relevant hosts, services, incidents, operations and notes.
- Global search can reach these canonical objects when implemented; search results are scoped to the sources it covers and say so. No permission filtering in v1 (single owner); the search layer keeps one filter point as a hook for future roles.
- An approval has one canonical home (operation detail); other surfaces embed or link the same card.

## 11. Backend contracts the UI depends on, and build order

The UI is built against contracts, not against the live engine. These must exist (as schemas with fixtures) before the pages that need them:

- **Trace schema:** operations, nodes, branches, cycles, phases, planned-versus-happened, uncertain outcome, versioning. Frozen with golden fixtures that both the Rust side and the UI test, before the timeline or the graph is built.
- **Operations record and list/detail endpoints**, with durable ordered events per operation.
- **Live events with ids and resume** per topic (operations, hosts, attention), replacing invalidate-by-prefix; keep invalidation as the fallback.
- **Conversations:** durable messages, explicit scope, request ids for discarding stale responses.
- **Policy decisions** stored with the context snapshot and policy version used, so the simulator can reproduce a historical decision.
- **Attention feed**, **audit API with chain verification**, **retention settings**, **resolution summaries and similar-operation search**, **capability deployment state per host**.
- **Presentation profile and dashboard preferences:** where they are stored (hub or browser) is a backend decision; the UI reads them through one provider.

Build order for UI work:

1. Shared foundations: live layer, freshness and stale states, motion tokens, virtualised list, right-hand slot and inspector shell, and the shared visibility infrastructure.
2. **Presentation profiles and the category table**, before any page is implemented.
3. Trace schema and fixtures; then the step timeline/tree; then the graph, outline and keyboard navigation; playback last.
4. Pages on top of these, with the structured approval card built once with operation detail and reused.

## 12. Implementation acceptance checklist

A page/component is not “done” until:

- Content model, P0–P3 category and screen-space class are approved.
- Page/subpage/tab/section/inspector decisions have rationale.
- Detailed view contains all meaningful information and controls.
- Standard/Essential profiles are derived from the detailed model by the category table; a test proves every P0 element is present and expanded in each profile.
- All interactive controls have action/result and failure behavior specified.
- Live updates, stale data, reconnect and uncertain execution are handled.
- Animations have triggers, timing/easing, interruption and reduced-motion behavior documented, and use the motion tokens of their class.
- Keyboard/focus/screen-reader behavior is tested (graph: the keyboard model and outline in doc 15, section 4).
- The sampling rule in doc 16, section 10 is met for themes, widths and profiles.
- Large and busy datasets at the fixed fleet sizes are tested; no clipping or excessive re-rendering.
- Browser-level tests cover at least one complete workflow through the page, against a deterministic hub fixture (the harness is built before the first page).
- Visual review confirms hierarchy, density and real-estate allocation are actually usable.
