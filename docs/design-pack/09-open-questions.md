# 09 — Open questions and recommended defaults

The high-level architecture is coherent enough to proceed. On 2026-11-13 the owner answered the questions that blocked the rework (`D-01`..`D-15` in [`docs/rework/99-open-decisions.md`](../rework/99-open-decisions.md)); they are listed first and are not open. The remaining questions are deliberately kept open so later design work does not silently invent answers. Technical detail for every answered item is in the linked `RW` decision in [`docs/rework/03-architecture-decisions.md`](../rework/03-architecture-decisions.md).

## Answered by owner

| Pack question or topic | Owner decision | Answer | Technical detail |
|---|---|---|---|
| Release strategy | **D-01** | No separate v0.1 release. Foundation work (atomic writes, protocol tolerance, approval freshness, reliability fixes) comes first inside the rework. | RW-02 |
| **Q0.1** What is an autonomous authorization? | **D-02, D-03** | A typed policy permit, distinct from a human approval, valid only inside a host-local ceiling on the agent that the hub cannot widen. A hub permit is not a cryptographic proof of owner approval; owner-signed bundles come after v1. (The exact permit fields are a proposal.) | RW-11, RW-12 |
| **Q0.2** Policy precedence | **D-05, D-06** | The strictly more specific rule wins, **even over a broader Deny**; equal specificity: Deny wins. Hard limits (host ceiling, Destructive never Allow in v1, containment exclusions, unknown safety facts) are not rules and cannot be overridden. Host ids in v1, tags after. This replaces the earlier "explicit deny anywhere wins" by owner decision. | RW-10 |
| Destructive operations | **D-06** | Never policy-allowed in v1; a human can still approve with typed confirmation. | RW-10 |
| **Q0.3** L2 versus L3 | **D-04** | L3 ships in v1 as milestone M9 under a general Contain class, not only firewall. (The exact first set remains open, below.) | RW-13 |
| **Q0.4** What can run offline? | **D-03, D-04** | In v1, only what the host-local ceiling permits under a live hub permit. Offline emergency permits come after v1. | RW-12, RW-13 |
| Unattended targets | **D-07** | The operation's pinned subjects plus targets named by the matching owner-authored rule. | RW-23 |
| **Q1.1** First capability schema (ownership) | **D-02** | Hub-owned typed contracts; providers and agents advertise availability only; skills keep owner-manifest risk with a hub floor. | RW-07 |
| **Q1.2** How incidents resume | **D-02** | Operation aggregate distinct from Incident; coarse status plus typed immutable steps; one transactional unit of work; unknown outcomes reconciled through the agent's journal. | RW-03, RW-04, RW-05, RW-09, RW-30 |
| **Q1.4** Knowledge ontology (shape) | **D-02** | Typed facts and relations that reference inventory through typed refs; a small entities table for user-defined subjects only; a closed predicate enum; no numeric confidence. | RW-14 |
| Experience and search | **D-02** | The journal is the experience record (no separate experience event store). A resolution summary per operation, full-text search, a host-scoped `search_experience` read capability, similar past operations in the UI, provenance whenever experience is supplied to reasoning. Past approvals are evidence, not authorization. | RW-15, RW-25 |
| **Q1.6** Retention (configurability) | **D-09** | All retention is configurable; raw data and durable summaries are separate classes. Default values remain open, below. | RW-28 |
| Orchestration visualization | **D-08** | Step timeline and interactive graph both ship in v1, timeline first, on one frozen trace schema. | RW-21 |
| Presentation profiles | **D-10** | Detailed, Standard and Essential ship in v1. | RW-21 |
| Capability install from the UI | **D-11** | In v1: signed packages, agent verifies against locally trusted keys, agent opt-in `allow_install`, hub only relays, install never enables or authorizes. Third-party packages remain open, below. | RW-26 |
| Motion | **D-12** | Longer durations allowed for visualization and transitions; reduced motion mandatory. | RW-21 |
| Users and roles | **D-13** | Single owner through v1; actor identity is designed so roles can follow. | RW-29 |
| Tamper evidence | **D-14** | Audit hash chain in v1, with an honest threat model; head hash in the daily digest as a cheap anchor. | RW-27 |
| `ARCHITECTURE.md` updates | **D-15** | Once per milestone. | none (process rule) |

## Priority 0 — resolve before implementing autonomous actions

| Question | Recommended default | Why it remains open |
|---|---|---|
| **Q0.2a Tie-break for incomparable conflicting rules** | When the most specific matching rules conflict and none dominates the others: Deny > RequireApproval > Allow. | The owner decided specificity and the equal-specificity case; this last case is a recommended default awaiting confirmation (RW-10). |
| **Q0.3a Exact first containment set** | `firewall.block_source` (nftables set element with a kernel timeout), `container.pause` with a time limit, `unit.stop_temporary` with a time limit and management-unit exclusions. | Scope is decided (D-04); the specific capabilities need the per-capability threat, privilege and lockout analysis before they are confirmed (RW-13). Container network isolation, SSH session termination and account lock are deferred. |
| **Q0.5 How is management access protected during containment?** | Maintain owner-confirmed trusted networks/VPN/endpoints as facts; mandatory exclusions enforced on the agent; require evidence of the enforcement point; temporary scoped blocks; verify management reachability; unknown means refuse. | This is a safety-critical user/environment contract. The product must define what happens when trusted paths are unknown or stale, and how the owner confirms them. |
| **Q0.6 What is the trust model for external providers and third-party packages?** | v1: first-party signed packages plus packages signed with the owner's own key added locally; no third-party registries. Hub-side integrations hold credentials under a separate security design and start read-only. | D-11 settled the install mechanism, not whom to trust. A registry becomes a security boundary: signing, key rotation, secrets and upgrades need a clear model (RW-26). |

## Priority 1 — resolve during detailed architecture design

| Question | Recommended default | Why it remains open |
|---|---|---|
| **Q1.1 What are the exact schema fields and compatibility rules?** | Ownership is decided (hub-owned). Remaining: stable ID/version, typed input/output, prerequisites, constraints, timeout, verification, rollback/TTL. | Need the exact type system, compatibility rules, error model and discovery refresh behavior (RW-07, RW-17). |
| **Q1.1a What is the agent support window?** | Until 1.0, the hub supports agents from the current and previous two minor releases; after 1.0, same major, all minors. | Needs owner confirmation; it determines when old agents are rejected (RW-17). |
| **Q1.3 What is the incident/plan step budget?** | Configurable global and per-host defaults plus per-incident caps; conservative defaults; share budgets across cycles. | Exact numbers need measurement and adversarial drills. |
| **Q1.4 Which predicates and relation types ship first?** | A small closed set for hosts, services, containers, storage, network, integrations and dependencies; extend through versioned definitions. | Shape is decided (typed refs, closed enum); the first list is still a proposal. |
| **Q1.5 How are inferred facts confirmed or disputed?** | Preserve each source claim; user-confirmed facts have explicit provenance; conflicts are surfaced as disputed, not overwritten; no numeric confidence. | Need merge policy, trust ordering, expiry and UI workflow. |
| **Q1.6 What are the default retention values?** | Keep today's defaults where they exist; keep resolution summaries long-term and raw step output short (for example 90 days), all configurable. | Configurability is decided (D-09); the default numbers per event class, deletion semantics and privacy controls are not. |
| **Q1.7 How are hub and agent knowledge synchronized?** | Hub canonical; in v1 the agent holds only its own ceiling and journal, not knowledge or policy; version/expiry/reconcile on reconnect. | Narrowed by D-03 and D-04. Still needed: which facts the agent needs locally after v1, and how offline updates are handled. |
| **Q1.8 What does onboarding ask?** | Discover first, then ask only high-value unknowns about role, criticality, recovery, dependencies, backups, exposure and constraints. | Need minimal initial questionnaire versus progressive questions and the point at which autonomy can be configured. |
| **Q1.9 Which integrations come first?** | Start with native host/container/service/network operations, then add router/firewall integration for containment. | Capability model should be general, but implementation needs a small, testable provider set. |
| **Q1.10 How should a plan represent compensation/rollback?** | Every mutating step declares idempotency, postconditions, rollback or expiry, and uncertainty handling. | Some operations are not reversible; need a consistent model for compensating actions and partial failure. |

## Priority 2 — defer until real usage justifies it

| Question | Recommended default | Why defer |
|---|---|---|
| **Q2.1 Do we need vector search?** | No initially; evaluate FTS5 + structured filters + graph traversal first. | Embeddings add models, indexes, refresh and tuning complexity. |
| **Q2.2 Do we need a dedicated graph database?** | No initially; SQLite typed edges and recursive CTEs. | No measured graph scale/algorithm requirement yet. |
| **Q2.3 How sophisticated should the *knowledge* graph UI be?** | Start with entity detail and bounded relationship explorer; add visual graph layout later. | Correct editing/provenance is more important than a large graph visualization. (This is separate from the orchestration graph, which ships in v1: D-08.) |
| **Q2.4 Should generated skills be sandboxed and tested automatically?** | Yes eventually, but require human promotion and install; do not permit self-installation. The signed-package install path (D-11) is the only install route. | Need execution isolation and package provenance design. |
| **Q2.5 How far should cross-platform support go?** | Keep capability contracts platform-neutral where practical; implement Linux first. | Avoid abstracting for unvalidated platforms. |
| **Q2.6 Do we need a separate workflow/playbook engine?** | Not now; represent playbooks as optional guidance/data. | The phase orchestrator should be the canonical lifecycle owner. |

## Product-level questions to answer with usage, not speculation

- Which host/service profiles should be offered as starter policy templates?
- Which operations do users most often approve repeatedly and would genuinely benefit from L2?
- How much context/history is useful before it harms speed, model accuracy or privacy?
- Which containment signals are sufficiently trustworthy for a temporary automatic block?
- What should the UI show first: current facts, dependencies, incident history or policy explanation?

## UI decisions already agreed

These are no longer open questions (those marked with an owner decision were settled on 2026-11-13):

- The step timeline and the interactive orchestration graph both ship in v1, timeline first, sharing one frozen trace schema (owner decision D-08).
- Detailed, Standard and Essential presentation profiles ship in v1 (owner decision D-10); P0 safety, approval and audit content is present in every profile.
- Longer motion is allowed for visualization and layout transitions; reduced motion is mandatory (owner decision D-12).
- Capability install and deploy happen from the UI in v1 (owner decision D-11).
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
| How should UI profiles customize detail? | Global profile plus per-page overrides. Detailed remains reference and default for implementation. | Profiles ship in v1 (D-10); the exact persistence scope and defaults need UX tests. |
| Which global shortcuts are final? | Keep Ctrl/Cmd+K for search; use a conflict-checked shortcut for Operator, proposed Ctrl/Cmd+J. | Shortcut conflicts vary by browser/OS and must be user configurable. |
| What is the final navigation taxonomy? | Separate Operations, Incidents, Infrastructure, Capabilities, Knowledge and Policies. | Exact grouping should be validated with complete page inventory. |

## Decision rule

Do not block general architecture work on Priority 2 questions. Do not implement L2 execution before the policy evaluator, host ceiling and target binding exist, and do not enable L3 containment on any host before Q0.3a and Q0.5 are confirmed and the drills pass. For Priority 1, specify only the parts required by the next vertical slice; keep the rest extensible.
