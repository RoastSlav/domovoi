# Domovoi — Target Architecture Design Pack

**Purpose:** durable product and architecture direction for future design, implementation, refactoring and feature work. It captures decisions, rationale, capability ideas, release scope and the difference between the target and the current codebase.

**Design snapshot:** 2026-11-12 (UI pass after current main overhaul)  
**Repository baseline checked:** [`RoastSlav/domovoi`](https://github.com/RoastSlav/domovoi), `main` at [`393694b`](https://github.com/RoastSlav/domovoi/commit/393694b0d16ad722961748744226e80b8cbac518)  
**Status:** evolving target design; not a claim that the described features are implemented.

## Start here

1. [`00-vision-and-scope.md`](00-vision-and-scope.md) — what Domovoi should become.
2. [`01-principles-and-decisions.md`](01-principles-and-decisions.md) — agreed decisions and why.
3. [`02-target-architecture.md`](02-target-architecture.md) — component boundaries and responsibilities.
4. [`03-capability-model.md`](03-capability-model.md) — how capabilities are described, discovered and executed.
5. [`04-incident-orchestration.md`](04-incident-orchestration.md) — dynamic investigation, plans, cycles and recovery.
6. [`05-autonomy-and-safety.md`](05-autonomy-and-safety.md) — permissions, autonomy levels and containment safeguards.
7. [`06-knowledge-and-experience.md`](06-knowledge-and-experience.md) — environment knowledge, onboarding and operational memory.
8. [`07-storage-and-search.md`](07-storage-and-search.md) — SQLite-first relational/graph/event storage.
9. [`08-current-state-gap-analysis.md`](08-current-state-gap-analysis.md) — what exists in the verified repository snapshot and what does not.
10. [`09-open-questions.md`](09-open-questions.md) — decisions still needed, ordered by urgency.
11. [`10-capability-roadmap.md`](10-capability-roadmap.md) — release plan from v0.1.0 through v1.0.0 and later.
12. [`11-capability-catalogue.md`](11-capability-catalogue.md) — broad capability inventory across host, homelab, network, storage, security, apps and integrations.
13. [`12-ui-product-principles.md`](12-ui-product-principles.md) — detailed transparency, terminology, configurable density and page-planning rules.
14. [`13-ui-information-architecture.md`](13-ui-information-architecture.md) — target navigation, global shell, Operator, Operations/Incidents relationship and page map.
15. [`14-ui-page-specifications.md`](14-ui-page-specifications.md) — page-by-page content, priority, screen space, tabs, inspectors and actions.
16. [`15-ui-interaction-and-motion.md`](15-ui-interaction-and-motion.md) — graph behavior, live updates, Operator interactions, motion and animation requirements.
17. [`16-ui-design-system-and-visual-quality.md`](16-ui-design-system-and-visual-quality.md) — visual identity, density modes, responsive, accessibility and performance quality.
18. [`17-ui-user-journeys-and-acceptance.md`](17-ui-user-journeys-and-acceptance.md) — cross-page workflows and implementation acceptance checks.
19. [`18-lessons-from-closed-pull-requests.md`](18-lessons-from-closed-pull-requests.md) — review of all 110 closed PRs, recurring failure modes and requirements to carry into the rework.
20. [`DOMOVOI-TARGET-ARCHITECTURE.md`](DOMOVOI-TARGET-ARCHITECTURE.md) — combined reading copy of the product and architecture documents.

## Repository-history lessons

Before changing architecture or rebuilding a feature, read [`18-lessons-from-closed-pull-requests.md`](18-lessons-from-closed-pull-requests.md). It records prior race conditions, recovery failures, stale AI proposals, UI state/feedback omissions, provider failures, delivery durability and testing practices. Re-check the linked PR and current code before assuming an old fix or a closed PR is present in `main`.

## UI design and implementation rule

- Treat the UI documents as target product requirements, not instructions to preserve the current page structure.
- The current UI overhaul on `main` is a visual/functional reference only. Start new page design from user goals, information priority and space requirements.
- Build from the detailed information experience first; Standard/Essential modes are presentations over the same underlying state and controls.
- Every significant item gets a priority (P0–P3) and space class (Dominant/Large/Medium/Compact/Contextual) before layout.
- A page specification must document sections, tabs, inspectors, interactions, dynamic states, animations, responsive behavior and acceptance criteria.
- The orchestration graph is a dominant element on operation detail; do not shrink it into an unreadable card. Use a meaningful preview on Control Room and dedicated workspace for full inspection.
- Operator side panel and full workspace are one feature sharing the same conversations/operations.
- Operations and Incidents are distinct concepts. Never force user-requested work into incident-only data structures.

## How future engineering agents should use this pack

- Treat this pack as the current record of the **intended product direction**. If a future user decision changes it, update the relevant document and the combined copy.
- Treat the actual repository source, tests, configuration and runtime behavior as the evidence for **what currently exists**. Re-check the target branch before every implementation task; never infer current behavior from documentation alone.
- Existing README, `ARCHITECTURE.md`, docs-book pages and ADRs may be incomplete or outdated. They are useful historical and implementation clues, **not automatic vetoes** over this target design. Inspect the code and tests before deciding what remains valid.
- Do not blindly preserve old architecture rules when they conflict with the target. Equally, do not remove an existing security boundary until a deliberate replacement is designed, tested and migrated. Preserve the security property, not necessarily the old mechanism.
- Distinguish **agreed direction**, **recommended default**, **open question**, and **current implementation**. Never silently upgrade an example or recommendation into a hard requirement.
- Do not implement the whole target as one rewrite. Deliver vertical slices, keep working behavior during migration where practical, and replace old mechanisms only when the new equivalent is proven.
- For consequential decisions, record the decision, rationale, alternatives, impact, security implications, migration and tests in the relevant design document. ADRs can be used as a recording format, but their existing contents are not binding by virtue of being labelled accepted.
- Never infer authorization from knowledge, user preferences, past actions, a playbook or an LLM's confidence. Only the deterministic policy/authorization layer grants permission.

## Status vocabulary

| Label | Meaning |
|---|---|
| **Agreed direction** | Direction explicitly aligned on in the design discussion. |
| **Recommended default** | Strong proposal; validate details before implementation. |
| **Open question** | Must be resolved before the affected feature is implemented. |
| **Current** | Verified in the stated repository snapshot. |
| **Partial** | A foundation exists, but not the target behavior or full abstraction. |
| **Missing** | Not found in the verified baseline. |
| **Later** | Valuable capability intentionally deferred beyond v1.0.0. |

## Scope note

This pack intentionally stays above exact Rust traits, SQL migrations, endpoint definitions and UI wireframes. Those should be designed from the relevant sections when a feature is selected. The capability catalogue is intentionally broad: it is an idea inventory, not a promise that every integration ships in v1.0.0.
