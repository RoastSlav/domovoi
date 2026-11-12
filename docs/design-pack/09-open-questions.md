# 09 — Open questions and recommended defaults

The high-level architecture is coherent enough to proceed. These questions are deliberately kept open so later design work does not silently invent answers.

## Priority 0 — resolve before implementing autonomous actions

| Question | Recommended default | Why it remains open |
|---|---|---|
| **Q0.1 What exactly is an autonomous authorization?** | A typed owner-policy decision distinct from human approval, bound to host, capability/version, target scope, conditions, limits, expiry and policy version. | The current implementation only authorizes mutations through human approval; the target must add a separate authority path without impersonating a person. |
| **Q0.2 What is the policy precedence?** | Explicit deny anywhere wins; otherwise most-specific rule wins (capability → category → host → global); no grant means approval-required or denied according to operation class. | Exact conflict behavior and default response still need to be specified and tested. |
| **Q0.3 Which operations qualify for L2 versus L3?** | L2 for reversible, low-blast-radius operations; L3 only for narrowly scoped protective containment with expiry and recovery safeguards. | Risk depends on target, host role, current state and recovery access, not only operation name. Need a reviewed initial catalogue with safe defaults. |
| **Q0.4 What can run offline?** | Only a small signed emergency permit cache, owner-authorized while connected, narrow, expiring and locally audited. | Need clock/expiry semantics, revocation, cache limits, restart behavior, duplicate suppression and reconciliation rules. |
| **Q0.5 How is management access protected during containment?** | Maintain owner-confirmed trusted networks/VPN/endpoints; require evidence of enforcement point; temporary scoped blocks; verify management reachability. | This is a safety-critical user/environment contract. The product must define what happens when trusted paths are unknown or stale. |
| **Q0.6 What is the trust model for external providers?** | Providers declare identity, ownership, target scope, auth source, contract version and effects; third-party integrations start read-only. | A capability registry becomes a security boundary. Registration, signing, secrets and upgrades need a clear model. |

## Priority 1 — resolve during detailed architecture design

| Question | Recommended default | Why it remains open |
|---|---|---|
| **Q1.1 What is the first capability schema?** | Stable ID/version, typed input/output, provider, scope, effect/risk, prerequisites, constraints, timeout, verification, rollback/TTL, permissions. | Need to choose exact type system, compatibility rules, error model and discovery refresh behavior. |
| **Q1.2 How are incidents resumed?** | Persist phase, cycle, outstanding evidence requests, plan IDs, operation states and deadlines; resume only after reconciling uncertain operations. | Need state machine details, idempotency keys, cancellation semantics and recovery after hub/agent restart. |
| **Q1.3 What is the incident/plan step budget?** | Configurable global and per-host defaults plus per-incident caps; conservative defaults; share budgets across cycles. | Exact numbers need measurement and adversarial drills. |
| **Q1.4 What is the knowledge ontology?** | Start with a small set of entities/relationships for hosts, services, containers, storage, network, integrations and dependencies; extend through versioned definitions. | Too much ontology is premature; too little becomes arbitrary JSON. Need first implementation scope. |
| **Q1.5 How are inferred facts confirmed or disputed?** | Preserve each source claim; user-confirmed facts have explicit provenance; conflicts are surfaced, not overwritten. | Need merge policy, trust ordering, expiry and UI workflow. |
| **Q1.6 What experience data is retained?** | Store event metadata, summaries, references and hashes; keep raw output only within existing bounded retention/redaction policy. | Need retention by event class, data deletion semantics and privacy controls. |
| **Q1.7 How are hub and agent knowledge synchronized?** | Hub canonical; agents cache only scoped policies and required local facts; version/expiry/reconcile on reconnect. | Need which facts are needed locally and how offline conflict/updates are handled. |
| **Q1.8 What does onboarding ask?** | Discover first, then ask only high-value unknowns about role, criticality, recovery, dependencies, backups, exposure and constraints. | Need minimal initial questionnaire versus progressive questions and the point at which autonomy can be configured. |
| **Q1.9 Which integrations come first?** | Start with native host/container/service/network operations, then add router/firewall integration for containment. | Capability model should be general, but implementation needs a small, testable provider set. |
| **Q1.10 How should a plan represent compensation/rollback?** | Every mutating step declares idempotency, postconditions, rollback or expiry, and uncertainty handling. | Some operations are not reversible; need a consistent model for compensating actions and partial failure. |

## Priority 2 — defer until real usage justifies it

| Question | Recommended default | Why defer |
|---|---|---|
| **Q2.1 Do we need vector search?** | No initially; evaluate FTS5 + structured filters + graph traversal first. | Embeddings add models, indexes, refresh and tuning complexity. |
| **Q2.2 Do we need a dedicated graph database?** | No initially; SQLite typed edges and recursive CTEs. | No measured graph scale/algorithm requirement yet. |
| **Q2.3 How sophisticated should the graph UI be?** | Start with entity detail and bounded relationship explorer; add visual graph layout later. | Correct editing/provenance is more important than a large graph visualization. |
| **Q2.4 Should generated skills be sandboxed and tested automatically?** | Yes eventually, but require human promotion and install; do not permit self-installation. | Need execution isolation and package provenance design. |
| **Q2.5 How far should cross-platform support go?** | Keep capability contracts platform-neutral where practical; implement Linux first. | Avoid abstracting for unvalidated platforms. |
| **Q2.6 Do we need a separate workflow/playbook engine?** | Not now; represent playbooks as optional guidance/data. | The phase orchestrator should be the canonical lifecycle owner. |

## Product-level questions to answer with usage, not speculation

- Which host/service profiles should be offered as starter policy templates?
- Which operations do users most often approve repeatedly and would genuinely benefit from L2?
- How much context/history is useful before it harms speed, model accuracy or privacy?
- Which containment signals are sufficiently trustworthy for a temporary automatic block?
- What should the UI show first: current facts, dependencies, incident history or policy explanation?

## UI decisions already agreed

These are no longer open questions:

- The Operator exists as both a global side panel and a dedicated full workspace over shared conversation/operation state.
- Operations are first-class work records and are not synonymous with incidents.
- The detailed information experience is the design baseline; users may select Standard, Essential or Custom presentation.
- The UI should expose the real orchestration flow, individual capability activity, evidence and policy decisions. Do not replace it with a simplified progress label.
- The current visual identity is a valued reference, but page layouts/navigation are not binding. Design new page structure from product needs.
- Every significant page element gets a display priority and screen-space requirement before layout is designed.
- The historical/current/proposed policy simulator calls the same deterministic evaluator as real operations, without executing anything.

## UI questions to resolve during page design

| Question | Recommended default | Why it remains open |
|---|---|---|
| How much of the active operation appears on the Control Room? | Show a compact active-operation list/preview; keep the full graph on operation detail. | A full graph may consume too much of the overview page and is not useful at miniature size. Prototype both. |
| Which host-detail views are tabs versus separate pages? | Tabs for peer host facets; separate pages for large service/workload or capability workspaces. | Need realistic data and navigation testing before final split. |
| Does Experience need a global explorer? | Start with operation history, incident history and audit links; add a cross-object experience explorer if usage shows a gap. | Avoid redundant history destinations. |
| What is the initial level of dashboard customization? | Reorder/hide/resize core widgets with one default layout and saved user layout. | Persisted layouts add implementation/state complexity; validate initial demand. |
| How should UI profiles customize detail? | Global profile plus per-page overrides. Detailed remains reference and default for implementation. | Exact persistence scope and defaults need UX tests. |
| Which global shortcuts are final? | Keep Ctrl/Cmd+K for search; use a conflict-checked shortcut for Operator, proposed Ctrl/Cmd+J. | Shortcut conflicts vary by browser/OS and must be user configurable. |
| What is the final navigation taxonomy? | Separate Operations, Incidents, Infrastructure, Capabilities, Knowledge and Policies. | Exact grouping should be validated with complete page inventory. |

## Decision rule

Do not block general architecture work on Priority 2 questions. Do not implement L2/L3 execution while Priority 0 questions remain unresolved. For Priority 1, specify only the parts required by the next vertical slice; keep the rest extensible.
