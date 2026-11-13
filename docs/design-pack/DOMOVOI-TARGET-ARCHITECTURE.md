# Domovoi — Combined Target Architecture and Product Design

This is a convenience reading copy, regenerated from the numbered files, which remain the canonical editable documents. Technical decisions and contracts live in `docs/rework/` (see README, "Relationship to docs/rework").

---

# 00 — Vision and scope

## Product vision

Domovoi should evolve from a monitoring and human-approved remediation tool into a **self-hosted, context-aware operations agent** that can investigate unfamiliar incidents, choose among available capabilities, and take owner-authorized actions within explicit safety boundaries.

It should not become a giant collection of hardcoded repair scripts, nor an unrestricted AI shell. Its intelligence should be able to reason about situations that were not anticipated in advance; its actual powers must remain explicit, typed, bounded and enforced outside the model.

## The target experience

When something happens, Domovoi should:

1. Detect and correlate the event into an incident.
2. Gather relevant evidence from the affected host, related services, infrastructure and its persistent knowledge.
3. Form and test hypotheses, explicitly separating facts from assumptions and unknowns.
4. Request more evidence when needed instead of guessing or receiving one enormous context dump.
5. Construct a plan suited to the actual situation from capabilities that are available on the relevant hosts.
6. Evaluate every proposed operation against owner-defined policy.
7. Either observe, notify, ask a question, request human approval, execute an authorized operation, or escalate.
8. Verify the underlying problem, not merely whether a command returned exit code zero.
9. Continue investigation and re-plan when a fix fails or the issue returns, within bounded budgets.
10. Record what happened and offer useful knowledge to retain, without silently changing permissions.

## User experience and control plane

The web UI is a first-class control plane, not only a dashboard. It must expose the real work Domovoi performs: phases, evidence, capability calls, policy evaluation, execution, verification and history. The detailed view is the design baseline; users can choose Standard, Essential or Custom presentation without changing system behavior or hiding the underlying audit record.

The Operator is available both as a global side panel and a dedicated full workspace. Operations are a first-class concept separate from incidents: an incident is a detected abnormal condition; an operation is work Domovoi performs and may exist without an incident.

The user interface should be information-dense, technically transparent and enjoyable to use, while explaining terminology in context for people new to self-hosting. The current visual identity is a valued reference, but existing page layouts are not constraints. New layouts must be designed from the target product's information hierarchy and real-estate requirements.

## Product properties

- **Adaptive:** reasons from current evidence and available capabilities; playbooks accelerate common cases but do not define the only possible paths.
- **Extensible:** new host tools, scripts, service integrations and infrastructure providers can expose capabilities through a common contract.
- **Context-aware:** understands host purpose, dependencies, operational constraints, preferences and recovery paths.
- **Progressively autonomous:** can move from observe/propose to bounded remediation and narrowly scoped protective actions when the owner explicitly grants them.
- **Verifiable:** actions have preconditions, bounded effects, postconditions and, where possible, rollback or expiry.
- **Auditable:** the incident timeline explains what Domovoi knew, asked, decided, attempted and verified.
- **Self-hosted and operable:** prefer a small number of dependable components and avoid requiring an external database or message broker without measured need.

## Scope

### In scope

- General server and self-hosted infrastructure operations.
- Linux host operations as the initial target, with capabilities that can later be supplied by routers, NAS appliances, hypervisors, cloud APIs or other integrations.
- Multi-host relationships and dependencies.
- Monitoring, diagnosis, service recovery, resource and storage operations, network diagnosis and defensive security response.
- Owner-defined per-host and per-capability autonomy.
- Long-lived environment knowledge and operational experience.

### Not assumed to be in scope

- Unrestricted root shell or arbitrary model-generated scripts.
- Automatic data deletion, destructive cleanup or irreversible changes.
- Replacing all existing monitoring tools.
- A mandatory graph database, vector database, workflow engine or external broker.
- A fixed workflow for every incident category.
- Autonomous modification of Domovoi's own security policy, permissions, skills or installed code.

## UI design test

A user should be able to inspect exactly what Domovoi is doing and why, while choosing how much detail is expanded at once. A new user should understand the same page through clear labels and contextual explanations without losing access to technical detail.

## Design test

A design is on the right track when it answers **both** questions:

- Can the reasoner discover and investigate a situation that was not anticipated when Domovoi was built?
- Can deterministic code prove that every requested operation is available, correctly scoped, authorized and bounded before it affects a host?

If the first answer is no, the system is too workflow-bound. If the second is no, the system is too permissive.

---

# 01 — Principles and decisions

This is the compact decision ledger for the target product. Each choice includes the reason; examples are illustrative unless explicitly marked as requirements.

| ID | Status | Decision | Why |
|---|---|---|---|
| D-01 | Agreed direction | Build an **adaptive operations agent**, not a catalogue of predefined fixes. | Unforeseen incidents cannot all be encoded as scripts. The reasoner must investigate and compose responses from what is actually available. |
| D-02 | Agreed direction | Put a deterministic control plane between reasoning and execution. | Model output is probabilistic and untrusted; authorization, validation, dispatch and state transitions must be enforceable and testable. |
| D-03 | Agreed direction | Use a general **capability** abstraction; skills are one provider. Contracts are **hub-owned and typed**; providers and agents advertise availability and implementation facts only (owner decision D-02, RW-07). | Native tools, scripts and service APIs expose different implementations of useful operations. The reasoner needs a consistent typed contract, and a provider must never define its own authority. |
| D-04 | Agreed direction | Playbooks are optional guidance, not mandatory workflows. | Known patterns benefit from reusable hints, but playbooks must not fence in novel investigations. |
| D-05 | Agreed direction | Orchestrate incidents through durable, resumable phases. | Separating detection, context, assessment, planning, authorization, execution and verification keeps work focused and recoverable. |
| D-06 | Agreed direction | Support multiple investigation and execution cycles. | A failed fix is new evidence. Re-diagnose and re-plan; never blindly retry. |
| D-07 | Agreed direction | Keep **knowledge**, **policy** and **experience** separate. | Facts about the environment, permission to act and records of past events answer different questions and must not substitute for one another. |
| D-08 | Agreed direction | Build editable structured environment knowledge alongside Markdown notes. | Notes are good for human runbooks; facts, dependencies, constraints and provenance need typed searchable records. |
| D-09 | Recommended default | Start with one SQLite database: relational tables, typed graph edges, JSON attributes and FTS5. | It fits the self-hosted single-hub footprint and avoids operating a second database before there is a measured need. |
| D-10 | Agreed direction (**changed by owner decision D-05**) | Make autonomy granular by host, capability/category and operation constraints. A strictly more specific matching rule overrides a broader one, **including a broader Deny**; at equal specificity Deny wins. Hard limits are not rules and cannot be overridden (RW-10). Incomparable conflicting rules resolve Deny > RequireApproval > Allow, and publication requires acknowledging them (owner decision D-19). The owner may declare a **locked Deny** (default off) that no narrower rule overrides and that needs step-up to change (owner decision D-26). A policy Deny is never offered for approval: the owner must change the policy (owner decision D-23). | A host-wide switch is too coarse: the NAS may allow a service restart but never file deletion. The original principle "explicit deny always wins" was deliberately replaced: an owner must be able to say "deny everything on this host except restarting this one container" and have the narrower rule take effect. |
| D-11 | Agreed direction | Treat defensive security containment as a distinct autonomy class. In v1 the first and only containment capability is `firewall.block_source` with a kernel-enforced timeout; `container.pause` and `unit.stop_temporary` are DEFERRED (owner decision D-17; `docs/rework/06` "Deferred and after-v1"). | A fast protective action can also lock out the owner; scope, trusted access paths, expiry and recovery must be first-class. |
| D-12 | Agreed direction | Keep local agent enforcement; design offline autonomy deliberately. In v1 the agent holds a **host-local unattended ceiling** (owner decision D-03, RW-12); offline emergency permits are DEFERRED past v1 (owner decision D-04). | A hub outage or compromise must not create a path around local validation. The ceiling is an enforcement limit the hub cannot widen; it is not a cryptographic proof of owner approval, `authorization_kind` is a hub label, not proof: a compromised hub can label a request `human` and bypass the ceiling (accepted v1 limitation, owner decisions D-03, D-18), but every mutating execution, human- or policy-labelled, is still limited by the agent's local catalogue, manifest, argument and `allow_risk` checks and its mutation rate limiter (owner decision D-27). Owner-signed bundles, approvals and offline permits are DEFERRED for that reason (`docs/rework/06` "Deferred and after-v1"). |
| D-13 | Agreed direction | Suggest learnings; never infer permissions. | Repeated user behavior may indicate a preference, but cannot grant authority to automate it. |
| D-14 | Agreed direction | Make knowledge, policy and experience user-visible, editable and reviewable. | The user must be able to correct the system, understand decisions and revoke information or authority. |
| D-15 | Recommended default | Keep the hub database canonical; agents cache only the scoped data needed for local execution and explicitly permitted offline response. | One source of truth avoids conflicting edits while preserving narrowly bounded local operation. |
| D-16 | Agreed direction | Do not expose raw SQL, unrestricted shell, arbitrary filesystem writes or model-generated executables as capabilities. | Extensibility must add reviewed typed operations, not an alternate route around authorization and execution controls. |
| D-17 | Agreed direction | Capability selection is dynamic; authority is not. | The model can decide which available evidence/capabilities are relevant, but cannot invent capabilities or permissions. |
| D-18 | Agreed direction | Every consequential operation needs preconditions, explicit target/scope, bounded effects and meaningful postconditions. | An exit code alone does not prove the intended system state was achieved. |
| D-19 | Agreed direction | Build the smallest usable version of each foundation, then expand by provider and domain. | A complete graph ontology, universal workflow engine or huge integration catalogue would delay useful delivery and add unvalidated abstractions. |
| D-20 | Agreed direction | Make failure, uncertainty and partial completion visible. | Hiding unavailable evidence, ambiguous actions or failed rollback creates false confidence and unsafe retries. |
| D-21 | Agreed direction | Treat the web UI as the primary operational control plane, not merely a dashboard. | Users must be able to inspect operations and manage almost all product configuration in one coherent interface. |
| D-22 | Agreed direction | Design from a detailed information model first; allow users to choose a simpler presentation. | Technical transparency must not be sacrificed to make the UI appear simple. |
| D-23 | Agreed direction | Provide the Operator as both a global side panel and a full workspace over the same conversation/operation state. | Quick requests should be possible anywhere, while complex investigations need substantial room. |
| D-24 | Agreed direction | Make Operation a first-class concept separate from Incident: its own aggregate with a coarse status and typed, immutable steps (owner decision D-02, RW-04/RW-05). | A detected problem is not the same as work performed; user requests and scheduled work may have no incident. Execution is a typed step of an operation, not a second source of truth. |
| D-25 | Agreed direction | Use a page-planning specification with priority and space requirements before layout implementation. | Important large visualizations (especially the orchestration graph) need deliberate real-estate allocation; page boundaries must follow content and workflows. |
| D-26 | Agreed direction | Preserve the valued current color identity and semantic status language, but do not preserve current layouts by default. | Visual continuity is desirable; existing layout decisions must not constrain a better target experience. |
| D-27 | Agreed direction | UI management is the default for supported configuration; external-only controls need a deliberate security/recovery reason. | Users should not need CLI/config edits for ordinary supported product management. |
| D-28 | Agreed direction | Policy simulation uses the same deterministic policy evaluator as real authorization. | “What would Domovoi have been allowed to do?” must be reliable enough to guide permission design, without causing side effects. |

## Target security properties

These are product-level properties to preserve through whatever implementation is chosen; they are not a demand to keep the current mechanism unchanged.

1. The model proposes and reasons; deterministic code authorizes and executes.
2. No arbitrary model-generated shell, SQL or executable code runs on a host.
3. Every provider and operation is typed, scoped, validated and bounded.
4. Agents validate operations locally, including target, arguments, policy version and expiry where applicable.
5. Notification and diagnosis failures should not suppress the original alert; uncertainty about a mutation must fail closed.
6. Logs, alerts, command output, webhook bodies and remote API content are untrusted input.
7. Secrets are redacted from model context, logs, chat, UI and audit payloads; credentials are scoped and rotatable.
8. Consequential state transitions and actions are traceable; audit history is tamper-resistant.
9. Destructive and irreversible actions are never policy-allowed in v1 (owner decision D-06): the evaluator treats this as a hard limit. A human can still approve one, with typed confirmation.
10. Budgets, timeouts, rate limits, output limits, cancellation and recovery are part of capability contracts.

## Owner decisions since the first snapshot

On 2026-11-13 the owner answered the open decisions in [`docs/rework/99-open-decisions.md`](../rework/99-open-decisions.md), including the later entries D-16..D-29. They are binding product direction and are reflected in the documents named below. Technical detail lives in the linked `RW` decision.

| Owner decision | Outcome | Reflected in | Technical detail |
|---|---|---|---|
| D-01 | No separate v0.1 release. Foundation work comes first inside the rework. | 10 | RW-02 |
| D-02 | Hub-owned typed capability contracts; Operation aggregate distinct from Incident with coarse status and typed immutable steps; transactional unit of work; the journal is the experience record; past operations are searchable and shown as evidence, never authorization. | 03, 04, 06, 07 | RW-03, 04, 05, 07, 14, 15, 25 |
| D-03 | Host-local unattended ceiling on the agent; the hub cannot widen it; the UI shows it read-only; owner-signed policy bundles DEFERRED past v1. | 02, 05 | RW-12 |
| D-04 | L3 containment ships in v1 (milestone M9) as a general Contain class, with `firewall.block_source` as its first and only capability (D-17); offline emergency permits DEFERRED past v1. | 05, 10, 11 | RW-13 |
| D-05 | Most specific rule wins, even over a broader Deny; equal specificity: Deny wins; incomparable: Deny > RequireApproval > Allow (D-19); hard limits stay hard; host ids in v1, host tags DEFERRED. | 01, 05, 09 | RW-10 |
| D-06 | Destructive is never policy-allowed in v1. | 01, 05 | RW-10 |
| D-07 | Unattended targets are limited to the operation's pinned subjects plus targets the matching rule names. | 05 | RW-23 |
| D-08 | Step timeline and interactive graph both ship in v1; timeline first, on one frozen trace schema. | 10 | RW-21 |
| D-09 | All retention is configurable. | 07, 09 | RW-28 |
| D-10 | Detailed, Standard and Essential presentation profiles ship in v1. | 10 | RW-21 |
| D-11 | Capability install and deploy from the UI in v1: signed packages verified by the agent against locally trusted keys. | 03, 10 | RW-26 |
| D-12 | Longer motion is allowed for visualization and transitions; reduced motion is mandatory. | 10 | RW-21 |
| D-13 | Single owner through v1; actor identity is designed for later roles. | 09, 10 | RW-29 |
| D-14 | Audit hash chain in v1, with an honest threat model; legacy unchained history is anchored by a digest, chained history follows, and verification and the UI report the two separately. | 05, 14, 17 | RW-27 |
| D-15 | `ARCHITECTURE.md` is updated once per milestone. | README | none (process rule) |
| D-16 | Package trust: first-party release key plus owner keys installed locally on the agent; the hub cannot add or replace keys; a detached signature over a canonical message, verified before extraction; a rollback must be signed by a currently trusted key. Third-party registries are DEFERRED. | 03, 09, 14, 17 | RW-26 |
| D-17 | The first and only v1 containment capability is `firewall.block_source`. `container.pause` and `unit.stop_temporary` are DEFERRED until independent expiry and recovery are proven. | 05, 10, 11 | RW-13 |
| D-18 | Ceiling scope: exact capability id, explicit targets, maximum rate, declared constrainable arguments, maximum TTL and maximum active for containment, mandatory management-path exclusions. No entry, no policy-authorized execution. | 02, 05 | RW-12 |
| D-19 | Incomparable conflicting rules resolve Deny > RequireApproval > Allow; publication shows the conflicts, equal-scope ones as well, and needs acknowledgement. | 05, 09, 14 | RW-10 |
| D-20 | Exact protocol major.minor match, checked only after the agent has proven its identity: older and newer minors are rejected, same-protocol patch releases are compatible, and application release numbers are not a compatibility dimension. No legacy adapter and no support window. Coordinated upgrade: hub first, then agents through the install procedure. The `Incompatible` host state is persisted, survives a hub restart, is visible and keeps history; an incompatible agent idles instead of reconnecting. One protocol bump before v1 (M1). No agent self-update. | 02, 10, 13, 14, 15, 17, 18 | RW-17, RW-42 |
| D-21 | Retention defaults, all configurable. Deleted data leaves the live database, search and model context, and stays in backups until they rotate. | 07, 14 | RW-28 |
| D-22 | Every containment expires independently of the main agent; connectivity-affecting containment also needs an independent watchdog; nothing is enabled unattended until expiry, recovery, watchdog, outcome-unknown handling and drills exist. | 05, 10 | RW-13 |
| D-23 | A policy Deny is not offered for approval; the owner must change the policy. No bypass. | 05, 14 | RW-10 |
| D-24 | No general Docker socket write access for the agent; a narrow first-party helper with its own ADR; Docker write capabilities are unavailable for unattended use until then (the helper is part of D-29's design). | 03, 05, 10, 11 | RW-39 |
| D-25 | Historical graph playback ships last in U2; the timeline and history are fully useful without it. | 10, 14, 15, 17 | RW-21 |
| D-26 | Locked Deny (default off): an owner-declared prohibition that narrower rules cannot override; visible in the editor, simulator, explanation and trace; step-up to change. | 05, 14, 17 | RW-10 |
| D-27 | Agent-side mutation rate limits apply to all mutating executions, human- and policy-labelled, separately from the ceiling. | 02, 05, 14 | RW-38 |
| D-28 | Unattended containment on a host requires owner-confirmed, current management paths covered by the exclusions the agent reports; unknown, stale, conflicting or uncovered means refused. The agent enforces local exclusions independently; the UI shows paths, the agent-reported exclusions, provenance, freshness and what is blocked. | 05, 06, 09, 14 | RW-37 |
| D-29 | The main agent runs as the dedicated unprivileged user `domovoi-agent`; privileged host operations go only through one narrow, root-owned helper installed and updated by the owner-controlled OS path, never by capability packages or skills. Root installs are labelled and get no unattended execution. Skills that need another `run_as` user are an open owner question (O-30; recommended unsupported). | 02, 03, 05, 10, 13, 14 | RW-39, RW-12, RW-26 |

Ledger rows above that an owner decision changed say so in their Status column.

## Existing behavior versus target behavior

The current code uses authenticated human approval for mutating skills and has a separate local risk allowlist. That is a fact about the current implementation, not a permanent product limitation. The target adds explicit owner-created autonomous authorization alongside human approval. It must not fake a human approval or route around existing checks: implement a separate, typed policy decision and local execution gate, with migration and tests that prove the replacement is at least as safe. The enforcement is honest about its limits: `HumanApproval` is restricted by lint rules and a line-based scan, not sealed by the compiler, and a hub permit is not a cryptographic proof of owner approval (RW-11, RW-12).

## Decision recording

For a significant new design, record: context, decision, why, alternatives, consequences, security impact, migration, observability and tests. Use an ADR or the relevant feature specification as convenient. Existing ADR status labels do not make a decision immutable; validate old claims against code, tests and current product intent.


## UI and terminology rules

- Use a consistent product vocabulary across UI, chat, notifications and book; explain precise technical terms rather than hiding them.
- Detailed is the reference presentation. Standard/Essential are alternate displays over the same data and authority model.
- Assign independent display priority (P0–P3) and space requirement (Dominant/Large/Medium/Compact/Contextual) to every significant page element.
- Use page, subpage, tab, section, expansion, inspector, drawer and dialog based on workflow and space—not convenience or component availability.
- The current UI overhaul is a reference for visual identity and current functionality, not a target layout mandate.

---

# 02 — Target architecture

## Component model

```mermaid
flowchart TB
  SIG[Checks / webhooks / probes / integrations] --> INC[Incident orchestrator]
  INC --> CTX[Context builder]
  CTX <--> KNOW[Knowledge service]
  CTX <--> EXP[Experience store]
  CTX --> REASON[Reasoning engine / LLM]
  REASON -->|evidence requests| CTX
  REASON -->|typed plan proposal| POLICY[Deterministic policy engine]
  CAP[Capability registry] --> REASON
  POLICY -->|deny / ask / approve / allow| ROUTE[Plan router]
  ROUTE -->|authorized operation| EXEC[Execution coordinator]
  EXEC --> PROVIDER[Capability provider]
  PROVIDER --> AGENT[Host agent / local enforcement]
  PROVIDER --> INTEG[External integrations]
  AGENT --> VERIFY[Verification and monitoring]
  INTEG --> VERIFY
  VERIFY --> INC
  INC --> EXP
  INC --> UI[Web UI / chat / notifications]
  KNOW --> UI
  EXP --> UI
```

> **Implementation direction.** The technical form of this architecture (aggregates, one transactional unit of work, effect runner, crate placement) is in [`docs/rework/04-target-architecture.md`](../rework/04-target-architecture.md) and decisions RW-03, RW-04, RW-07 and RW-08. This document stays at product level.

## Responsibilities and boundaries

| Component | Owns | Must not own |
|---|---|---|
| **Incident orchestrator** | Durable work state of an **Operation** (an aggregate distinct from the incident): coarse status, typed immutable steps, deadlines, budgets, retries/loops, interruption and resume. The incident remains the detected condition. | Diagnosis, permission decisions or direct host execution. |
| **Context builder** | Selects relevant evidence, knowledge, capabilities and history; tracks missing/failed/stale data. | Inventing facts or making policy decisions. |
| **Reasoning engine** | Hypotheses, interpretation, evidence requests, plan proposals and concise explanations. | Authorization, raw database access, arbitrary shell or direct state mutation. |
| **Capability registry** | Holds hub-owned typed contracts (schema, effect class, risk, reversibility, verification, version) joined with the availability that agents and providers advertise. | Deciding whether a requested operation is allowed in the current context; letting a provider define its own risk or authority. |
| **Policy engine** | Deterministic authorization decision using owner policy and current context. | Learning or changing permissions from model output/history. |
| **Execution coordinator** | Validated dispatch, idempotency, operation state, timeouts, result capture and rollback/expiry coordination. | Bypassing the policy engine or provider-specific validation. |
| **Capability provider** | Implements an operation using its specific host/API mechanism. | Granting its own authority or broadening its scope. |
| **Host agent** | Runs as an unprivileged OS user (owner decision D-29, RW-39). Local discovery, local manifest revalidation, bounded execution, the host-local unattended ceiling, a mutation rate limiter for every mutating execution (owner decision D-27), an action journal for idempotency and reconciliation, and a local audit buffer. | Trusting a hub/model authorization flag without validating its own contract and ceiling. |
| **Verification service** | Checks postconditions and monitors for regression over a defined window. | Treating process exit code as proof of recovery. |
| **Knowledge service** | Structured facts, entities, relationships, provenance, lifecycle and search. | Storing permissions or silently converting inference into confirmed fact. |
| **Experience service** | The append-only journal of typed operational events, plus a searchable resolution summary per operation. There is no separate experience event store (owner decision D-02, RW-15, RW-25). | Becoming the source of current environment truth or permissions. |

## Core interaction rule

The reasoner communicates through typed requests and responses. The engine validates every request before collecting evidence or executing an operation. The model never receives raw access to SQL, process spawning, privileged APIs or arbitrary filesystems.

A capability request should carry at least: capability ID/version, target host/entity, typed arguments, intended effect, relevant evidence references, deadline and a correlation ID. The policy decision and execution receipt are separate records.

## Hub and agents

- The **hub** is the canonical coordinator and persistent store for incidents, policies, structured knowledge and experience.
- Each **agent** exposes the host capabilities it can safely provide, validates host-scoped operations locally and reports results/evidence.
- Capabilities may also be provided by hub-side or external integration adapters (for example, a router API); they must declare their enforcement point and security boundary.
- The agent enforces a **host-local unattended ceiling**: owner-controlled configuration on the host that lists which capabilities may run without a human, on which explicit targets, how often and with which argument limits. The hub cannot modify or widen it, the UI shows it read-only, and hub policy can only narrow it (owner decision D-03, RW-12). The ceiling covers the exact capability id, explicit targets, maximum rate, declared constrainable arguments and, for containment, maximum TTL, maximum active containments and management-path exclusions; no entry means no policy-authorized execution (owner decision D-18). The ceiling is an enforcement limit, not a signature: a hub permit is not a cryptographic proof of owner approval. `authorization_kind` is a hub label; a compromised hub can label a request `human` and bypass the ceiling (accepted v1 limitation). It is still limited by the agent's local catalogue, manifest, argument and `allow_risk` checks and by the mutation rate limiter, which applies to all mutating executions (owner decision D-27).
- Owner-signed policy bundles and approvals, and cached offline emergency permits, are DEFERRED past v1: they need owner key custody and a replay and revocation design (`docs/rework/06` "Deferred and after-v1"). Until then an agent does not invent or cache new policy while offline.
- **Unprivileged agent and root-owned helper (owner decision D-29, RW-39).** The agent runs as the dedicated user `domovoi-agent`. Privileged host operations (Docker, systemd control, nftables, SMART, kernel messages, listening sockets with their owning process) go only through one narrow, root-owned helper (`domovoi helper serve`): socket-activated, it checks the caller's uid and validates every typed request against a root-owned allowlist (`/etc/domovoi/helper.toml`), independently of the agent and hub. The helper is installed and updated only by the owner-controlled OS install path; capability packages and skills can never install or modify it, and packages run as the agent user, never root. An agent still running as root is labelled "privilege boundary not in effect" and gets no unattended execution. Skills whose manifest names another `run_as` user are an open owner question (O-30; recommended: unsupported).
- **Version match (owner decision D-20, RW-17, RW-42).** Hub and agent speak the same protocol major.minor or not at all. The check happens only after the agent has proven its identity; there is no support window, no previous-minor adapter, no legacy execution path and no application-release floor (a release number is not a compatibility dimension). Same-protocol patch releases are compatible. A mismatched agent is rejected with the existing `Incompatible` reason, and its host is shown as `Incompatible` (upgrade required, history kept), not as offline; the state is persisted on the agent record and survives a hub restart, and after a hub upgrade a host that has not reconnected shows "expected incompatible". An incompatible agent idles instead of reconnecting (agents older than the F3 release restart-loop under systemd until upgraded). Exactly one protocol bump is planned before v1 (1.1 → 1.2 in M1); F3 changes no wire format and later milestones use capabilities over the frozen 1.2 wire. Upgrades are coordinated: hub first, then each agent through the install procedure. The agent never updates itself (DEFERRED, not planned; see `docs/rework/06` "Deferred and after-v1"). A revoked-build list is DEFERRED and distinct from protocol compatibility.
- When a host disconnects, the orchestrator records that the operation is unavailable or uncertain. It must not treat missing responses as success.

## Durable state and concurrency

Persist the incident work item and each phase transition. It must be possible to stop the hub, restart it and resume from the latest durable state without repeating a non-idempotent action blindly.

Use explicit state owners/single-writer patterns for aggregates where concurrent decisions could race, as the current project already does for incidents and actions. A state change, its journal entries, its audit record and any new pending side effects commit in **one transaction** (owner decision D-02, RW-03); a transition whose audit record cannot be written does not happen. Use optimistic store guards where appropriate. Notifications and UI updates should be derived from committed state transitions, not emitted before a write succeeds. Notification delivery is at-least-once: after a crash window a duplicate is possible, remote idempotency is used where the platform offers it, and the product never claims exactly-once (RW-35).

## Resilience and budgets

Every incident run has limits for wall time, model turns, evidence requests, capability calls, tokens/cost, plan steps and repeated cycles. Budgets are shared across the incident session, not reset merely because a new model turn begins. Exceeding a budget results in a clear escalation or pause, never an unbounded loop.

Context is layered: concise incident summary first; relevant evidence and knowledge next; detailed logs/documents retrieved on demand. Do not place an entire host profile, graph or event history in every prompt.

## Product interaction surfaces

The UI and conversational Operator are entry points into the same application services; neither owns a parallel execution engine.

- **Control Room:** summarizes environment state, attention and active operations.
- **Operations:** durable work records independent of incident records.
- **Incidents:** detected abnormal conditions, with links to one or more operations.
- **Operator:** a global side panel and full workspace that may create an operation or answer an informational question.
- **Capability Center:** catalog, install, enable, configure and update providers/capabilities; authorization remains separate.
- **Knowledge and policy interfaces:** allow users to inspect/correct facts and define/simulate deterministic policy.
- **Orchestration visualization:** renders durable operation events/relationships; it is a view of the orchestrator, not a source of execution truth.

The UI can change layout, view state and presentation profile, but all consequential actions call the same authorized domain/application services used by other entry points. A user closing a panel or navigating away does not stop durable work.

## Extension path

Add capabilities through providers/adapters behind the common hub-owned contract. Preserve current `ReadTool`, skill and action behavior during migration by wrapping them first; existing action records stay as they are and are shown as legacy (RW-32). Avoid coupling orchestration or policy to Docker, systemd, a particular firewall, or one script format. Capability packages can be installed from the UI, but only as signed packages that the agent verifies against locally trusted keys (the first-party release key plus keys the owner installed on that host); the hub merely relays them and cannot add keys (owner decisions D-11, D-16, RW-26). Owner-signed packages arrive by manual import. Packages install into the agent-owned skills directory and run as the agent user. Third-party registries are DEFERRED.

---

# 03 — Capability model

## Definition

A **capability** is a named, versioned, typed operation that a provider can perform within a declared scope. It may read information, change state, contain a threat or verify a condition.

A capability is not the same thing as a skill:

- **Capability:** the logical operation and its contract (for example, restart one named service).
- **Provider:** the implementation that can fulfil the contract (local Linux agent, skill, Docker adapter, router integration, hypervisor API).
- **Skill:** one existing provider format: a locally installed program with a manifest and validated arguments.
- **Playbook:** optional reusable advice about how to investigate or sequence capabilities. It is not the only permitted way to solve an incident.
- **Check:** an observation that can produce a signal; it may also provide evidence to the context builder.

## Why this abstraction

If the model sees only a hardcoded set of tools, new integrations require changes to the reasoning contract. If it sees unrestricted commands, the safety model collapses. A capability registry gives it discoverable options while keeping implementation and authority outside the model.

## Capability descriptor — conceptual contract

The eventual schema should contain:

| Field | Purpose |
|---|---|
| `id`, `version` | Stable identity and contract version. |
| `name`, `description`, `examples` | Clear human/model explanation of the operation and appropriate use. |
| `provider`, `provider_version` | Which adapter implements it. |
| `scope` | Host, service, container, network, router or other target scope. |
| `input_schema` | Typed arguments, required values, bounds, enums and validation rules. |
| `effect` | Read, mutate, contain, verify or other explicitly named operation class. |
| `risk` | Impact classification independent of whether it is a script or native tool. |
| `preconditions` | Conditions that must be established before execution. |
| `constraints` | Resource limits, allowed targets, rate limits, TTL, management-path exclusions. |
| `reversible` / `rollback` | Whether and how the effect can be undone. |
| `verification` | Postconditions and how to test them. |
| `availability` | Host/platform prerequisites, connectivity and provider state. |
| `timeout`, `output_limit` | Bounded execution and result size. |
| `required_authority` | Policy category/permission needed; never an authority grant itself. |
| `manifest_hash` or equivalent | Bind an approved/authorized plan to the exact provider contract. |

This is a conceptual contract, not a final serialized schema. Avoid requiring every provider to implement fields that are genuinely not applicable; represent not-applicable explicitly rather than guessing.

**Who owns which fields (owner decision D-02, [RW-07](../rework/03-architecture-decisions.md)).** The contract is **hub-owned and typed**: identity, version, input schema, effect class (`Read`, `Mutate`, `Contain`, `Destructive`), risk, reversibility, idempotency, timeout, output limit, verification and the arguments a host ceiling may constrain are defined by the hub, not by the provider. Providers and agents advertise only implementation facts: which contract ids and versions they implement, an implementation hash, availability and a summary of the local ceiling. An advertised id the hub does not define is shown as unknown and is never dispatchable. Skills are the exception that proves the rule: their effect class comes from the owner's local manifest (the existing trust boundary), with a hub floor the owner may raise but never lower; an unknown class is treated as `Destructive`.

## Discovery and invocation

1. The hub holds the typed contracts; providers register the implementations they can offer against those contracts.
2. Agent handshake reports which contracts and versions it implements, with an implementation hash and availability, not just script filenames. Availability is `Known`, `Unknown` (no session yet since hub start), `Absent` or `Incompatible` (the agent authenticated but speaks another protocol minor; owner decision D-20, RW-17). An unknown host is waited for, never rejected; an incompatible host is shown with an upgrade-required explanation and its history, never as offline (also after a hub restart, because the state is persisted on the agent record), and nothing is dispatched to it. A capability that needs the privileged helper is unavailable when the helper is absent or in error, and each affected check says so instead of failing silently (owner decision D-29, RW-39).
3. Hub builds a scoped catalogue for the incident: only capabilities that exist for the target and whose basic prerequisites are met.
4. Reasoner may ask to inspect a capability's schema or request an operation using typed arguments. The model sees a closed set of meta-tools (list and describe capabilities, request evidence, search experience, propose a plan, ask the user, finish); none of them approves or executes anything.
5. Engine checks the schema, scope, freshness, budget and current policy before dispatch.
6. Provider and agent validate again at the enforcement point.
7. Result returns as structured outcome plus bounded, redacted evidence.

Capability discovery must not expose secrets or imply permission. **Available does not mean authorized.** A capability may be visible to the model while policy still denies its execution.

## Capability families

Initial broad groups (examples, not a closed list):

- **Host observation:** processes, memory, disk, filesystems, temperatures, journal, listening sockets, service state.
- **Application/container operations:** inspect, logs, restart, health checks, queue state, deployment metadata. Docker write operations go only through the narrow, root-owned first-party helper (typed `docker.read` and `docker.write` operation groups against a root-owned allowlist; owner decisions D-24, D-29, RW-39); the agent runs unprivileged and gets no general Docker socket write access, and Docker write capabilities stay unavailable for unattended use until the helper and its ADR exist. Container targets are bound by full id and systemd unit targets by canonical id plus a hash of the fragment and drop-ins; generated and transient units are not valid mutation targets, and files a unit references are not bound (RW-34).
- **Storage:** pool/device health, mount state, backup state, free space, scrub/sync status; destructive storage operations require strict treatment.
- **Network:** routes, DNS, gateway reachability, connection state, firewall inspection and narrowly scoped firewall changes.
- **Security:** authentication events, suspicious-source analysis, temporary blocks, isolation and exposure restriction.
- **Platform integrations:** router, hypervisor, NAS, reverse proxy, DNS, certificate manager, monitoring systems.
- **Verification:** application health, HTTP/TCP probes, service readiness, dependency state, expected postconditions.

Do not model every log line or metric as a capability. Capabilities are operations. Observations are typed evidence returned by operations/checks.

## Skills and generated extensions

- Existing skills should be wrapped as a capability provider first; keep their local manifest, argv-only execution and agent-side validation.
- Native read tools should be exposed through the same discovery surface over time, rather than remaining a separate privileged pathway.
- Integrations should register capabilities with explicit ownership, target scope and authentication boundaries.
- The model may draft a new skill/provider definition as a **proposal**. It must not install, enable or grant authority to that extension itself.
- A proposed extension needs human review, static validation, sandboxed tests, manifest/hash review, provider installation and an explicit policy decision before use.

### Installing capabilities from the UI (owner decision D-11, RW-26)

Capability install and deploy from the UI is part of v1, with these properties:

- A capability package is signed. The agent installs it only if the host opted in (`allow_install`, off by default), the signature verifies against a key in the agent's **locally** trusted keys, and the package is compatible with the agent. The hub only relays the bytes and cannot add or replace trusted keys, so a compromised hub cannot make an agent accept an unsigned package.
- **Sources (RW-26).** First-party packages come from a signed index. Owner-signed packages arrive by **manual import**: the owner uploads the archive and signature in the Capability Center or runs `domovoi package import` on the hub; there is no other discovery in v1. The agent applies archive limits, rejects links and verifies before extracting, and the signer's `key_id` is persisted per installed package so a revoked key finds the packages it signed. The package backend (P1) is separate from the install wizard (U4, after U3).
- **Trust (owner decision D-16, RW-26).** v1 trusts two kinds of key, both installed locally on the agent: the first-party release key and keys the owner adds. The signature is **detached** (a separate file, never inside the archive) and covers a canonical message naming the package, version, archive digest and key; the agent verifies it before extracting anything. Any install, including a rollback to an older version, must be signed by a key that is trusted now.
- Installing never enables or authorizes anything. Install, enable (hub) and unattended use (policy plus the host ceiling) remain three separate steps.
- **Privileges (owner decision D-29, RW-26, RW-39).** Packages install under the agent-owned skills directory and run as the agent user, never root. They can never install or modify the root-owned helper or its allowlist; that is a separate step through the owner-controlled OS install path. Signing grants execution as the agent user only. A root install is labelled and gets no unattended execution.
- **DEFERRED:** third-party or community packages and registries. Anyone publishing a package could run code as the agent after one click, so v1 has none (`docs/rework/06` "Deferred and after-v1").
- **DEFERRED, not planned:** agent self-update. It violates the rule that the hub cannot push code to a host; upgrades follow the install procedure (owner decision D-20; see the catalogue flags in `11`).

## Missing capabilities

If the needed operation does not exist on the relevant host or through an integration, the reasoner should say so, explain which part of the response is blocked, and propose a typed capability/provider extension. It must not pretend that an unavailable action exists or fall back to arbitrary shell execution. The proposed extension follows the review, test, install and explicit-policy path above.

## Playbooks

Playbooks are advisory assets that can describe known diagnostic patterns, useful evidence requests, safe sequencing and verification ideas. The orchestrator and policy engine remain in control. The reasoner may depart from a playbook when evidence requires it, but it may not bypass capability validation or policy.

## Open design items

The ownership split above is decided; the exact schema and version negotiation are specified in `docs/rework` (RW-07, RW-17). Package trust is decided (owner decision D-16) and protocol compatibility is exact-match (owner decision D-20). Still open: discovery refresh details, the provider lifecycle for hub-side integrations (DEFERRED, `docs/rework/06`), and how generated capability drafts are reviewed. See [`09-open-questions.md`](09-open-questions.md).

---

# 04 — Phase-based incident orchestration

## Principle

The work done about an incident is a durable operational task, not a single LLM prompt. That task is an **Operation**, an aggregate distinct from the incident (owner decision D-02, RW-04): the incident is the detected condition, the operation is the work performed, and an operation may also start from a user request or a schedule with no incident at all. The orchestrator owns lifecycle and state; the reasoner handles interpretation and proposes next steps. Phases are responsibilities and contracts, not a rigid linear workflow. The engine may complete simple phases without an LLM call, and paths may loop or branch.

**How phases are stored.** An operation has a **coarse status** (for example running, waiting, paused, succeeded, failed, escalated, cancelled). Its work is a growing list of **typed, immutable steps** (reasoning turn, evidence request, plan, policy decision, approval, execution, verification, notification, question); a step's progress is recorded as append-only events. A phase in this document is therefore a label over a group of step kinds, not a stored state. The orchestrator decides on typed step outcomes, never on model text. State machines and outcomes are in [`docs/rework/04 §3`](../rework/04-target-architecture.md) and RW-05.

## Lifecycle

```mermaid
flowchart TD
  D[Detect / correlate] --> E[Enrich context]
  E --> A[Assess]
  A -->|need more evidence| E
  A -->|understood| P[Plan]
  P --> R{Route plan}
  R -->|observe / notify| M[Monitor]
  R -->|ask owner| U[Wait for user]
  R -->|manual work| H[Human intervention]
  R -->|approval required| AP[Wait for approval]
  R -->|policy allows| X[Execute]
  R -->|unsafe / unsupported| ES[Escalate]
  U --> E
  AP --> X
  X --> V[Verify / monitor]
  V -->|resolved| RES[Resolve]
  V -->|not resolved or regressed| E
  M -->|new evidence| E
```

## Phase contracts

| Phase | Responsibility | Output / exit condition |
|---|---|---|
| **Detect** | Receive checks, probes, webhooks and integration signals; correlate duplicates and update/create incident. | Durable incident identity, initial signal set and source metadata. |
| **Enrich** | Collect relevant host, service, network and dependency evidence; retrieve knowledge and past experience. | Context bundle with evidence references, timestamps, source, freshness and collection status. |
| **Assess** | Build hypotheses; distinguish established facts, inferences, unknowns and contradictory evidence. | Structured assessment and optional requests for additional evidence. |
| **Plan** | Propose one or more possible next steps, including observation, more investigation, notification, question, manual steps, approval, authorized action, verification or escalation. | Typed plan proposal with rationale, target, expected effect, preconditions and postconditions. |
| **Authorize / route** | Apply deterministic policy to each proposed operation and choose the route. | Allowed, denied, approval required, ask user, manual intervention or escalation. |
| **Execute** | Dispatch only authorized operations through the capability provider. | Execution receipt, bounded output, status, timing and side-effect record. |
| **Verify / monitor** | Check whether the original condition is gone and whether dependent services remain healthy. | Verified, verification failed, inconclusive or regression detected. |
| **Resolve / escalate** | Close the incident only when the condition is demonstrably resolved; otherwise continue or escalate. | Terminal state or a new bounded cycle. |

A phase may return `complete`, `needs_evidence`, `waiting`, `failed`, `not_applicable` or `escalate`. Store these outcomes explicitly on the steps; do not treat missing output as a successful phase. An execution whose result is lost (a dropped link, a restart mid-dispatch) is `OutcomeUnknown`, not `Failed`: it is reconciled against the agent's action journal, and it blocks unattended re-execution of the same capability on the same target until a human or a verification step resolves it (RW-09).

**Outcome vocabulary.** The single definition is in [`docs/rework/04 §3.2`](../rework/04-target-architecture.md); the UI and API use it verbatim.

- **Succeeded** and **Failed:** it ran and reported success, or an attempted operation reported failure. `Failed` is only for attempted-and-failed work.
- **NotRun:** only with positive evidence that it never started, or when its authorization or approval expired before dispatch.
- **Unavailable:** the work could not be attempted (host absent past the wait limit, host `Incompatible`, capability not implemented or disabled, evidence source down).
- **OutcomeUnknown:** it may have run and the result cannot be established.
- **Refused:** rejected before running by validation, policy (`Deny`), the agent (ceiling, `allow_risk`, rate limit, exclusion) or a human.

A step is never shown as `NotRun` or `Failed` when `OutcomeUnknown` is the honest answer.

## Context model

Do not create one universal questionnaire or giant prompt. Build context in layers:

1. **Common incident context:** incident/host identity, triggering signal, severity, time, recent changes and current work state.
2. **Domain context:** relevant Docker, storage, network, security, database or application details.
3. **Knowledge context:** applicable facts, constraints, dependencies, preferences and recovery paths.
4. **Experience context:** similar past operations (found through their resolution summaries and the host-scoped `search_experience` read capability), prior plans, failures and successful verification evidence. Each item supplied to the reasoner carries its operation id, date, verified flag and source, inside a block labelled historical evidence, not authorization (owner decision D-02, RW-25).
5. **Action-specific context:** only facts needed to evaluate a specific operation (for example, trusted management addresses before firewall changes).

Every requested field/evidence item should have a state: `available`, `unknown`, `unavailable`, `failed`, `stale` or `not_applicable`. Missing data is not equivalent to `false`. Include source and observed time. Distinguish user-confirmed, automatically observed and inferred information.

## Dynamic evidence requests

The reasoner may request further evidence at any assessment stage. The engine validates that the requested capability is read-only/appropriate, in scope and within collection/time/token budgets. Deduplicate repeated requests and retain references so the same expensive query is not run without reason.

A request should identify what is needed and why, not dictate an arbitrary shell command. The engine maps it to an allowed capability and supplies a concise result or a structured failure.

## Planning and routing

A plan may contain several independent steps and multiple routes. For example: notify the owner immediately, collect more logs, then request approval for a service restart; or execute an owner-authorized temporary IP block and still ask the owner to review the broader exposure.

The plan is a proposal, not authorization. The policy engine evaluates each consequential operation separately. One denied step must not silently authorize another. Plans should state dependencies and ordering explicitly.

## Repeated cycles and no blind retries

- Verification failure returns to assessment with the failed operation and its output as evidence.
- A failed or uncertain operation is recorded and not blindly repeated.
- A revised plan must explain what changed and why the next attempt is different.
- Re-evaluate policy and preconditions before every new operation.
- Apply per-incident/per-capability cycle and rate limits.
- Support delayed verification and observation windows for transient conditions.
- If evidence, budget or confidence is insufficient, pause, ask the owner or escalate.

## Durable experience timeline

Record phase transitions and meaningful events with timestamp, actor/component, incident, input/output reference, evidence references, decision/rationale, policy result, operation identity and outcome. Preserve enough to reconstruct the lifecycle without persisting secrets or unlimited raw logs.

The experience record **is the journal**: one append-only table of typed events that also feeds live UI replay, not a separate experience store (RW-15). Security-relevant facts (who authorized what, policy and settings changes, refusals) go to the separate hash-chained audit record, written in the same transaction. When an operation ends it gets a **resolution summary**: deterministic fields filled by code (subjects, capabilities executed, verification results, outcome) and narrative fields (problem, cause, fix) drafted by the model, marked as model-sourced and editable or confirmable by the owner. Summaries belong to the Operation aggregate and the operation writer is their only writer; the Knowledge UI sends edit commands to it (RW-41). The UI shows similar past operations on incident and operation pages. A past approval or success never satisfies policy.

## Delivery semantics

Notification delivery is **at-least-once** (RW-35). If the platform accepts a message and the hub crashes before recording it, the message is retried and a duplicate is possible; each message carries a short stable id so duplicates are recognisable, and remote idempotency is used where the platform supports it. The product never claims exactly-once delivery. Notification state is written only after a confirmed success, and a failure to read it fails the delivery rather than becoming default state. Side effects are claimed under a lease with a claim token: a worker whose lease expired and was reclaimed can no longer commit any local state, so only remote delivery can repeat, which is why it stays at-least-once (RW-35).

## Example: a fix reveals a second problem

A Plex container is unhealthy. Domovoi inspects logs and dependencies, then proposes or performs an owner-authorized Plex restart. The restart command succeeds, but the application health check still fails. Domovoi records that failure as new evidence, returns to assessment, notices database-connection errors, checks the database service and its recent history, then proposes a separate database restart. If that operation is not covered by autonomous policy, it asks for approval. After the database restart it verifies both the database and Plex, watches for regression, and resolves only when the original Plex condition is healthy.

The important behavior is not the Plex-specific sequence: it is **failed postcondition → new evidence → revised diagnosis → new authorization decision → verification**. No blind retry and no assumption that a successful process exit means the incident is fixed. A retry is always a new execution step with its own authorization; reconciling an unknown outcome is not a retry.

## LLM efficiency

An LLM conversation may span multiple phase transitions when useful; phase boundaries do not require one model call each. Use concise summaries and on-demand detail. Enforce incident-wide limits for turns, tokens, tool calls, time, cost and execution cycles.

---

# 05 — Autonomy and safety

## Autonomy model

Autonomy is a permission to perform a defined operation under defined conditions, not a global switch that makes the model trusted.

| Level | Meaning | Typical behavior |
|---|---|---|
| **L0 — Observe** | Read-only. | Detect, collect evidence, diagnose, report and monitor. |
| **L1 — Assisted** | The system can prepare plans, but a human authorizes changes. | Propose restart/configuration/security actions and wait for approval. |
| **L2 — Bounded autonomy** | Owner-authorized, low-risk and recoverable operations can run without per-action approval. | Restart an approved service, renew a certificate, clear a bounded queue, unload an unused model. |
| **L3 — Security autonomy** | Owner-authorized, narrowly scoped protective operations can run quickly. | Temporarily block a clearly identified external source. Isolating a workload is DEFERRED. |

These are capability categories, not a single host-wide ranking. A host may allow L2 service recovery and L3 network containment while explicitly denying deletion and host reboot.

**v1 scope (owner decisions D-03, D-04).** L2 ships as a reviewed set of reversible operations. L3 also ships in v1, as milestone M9, under a general **Contain** effect class whose first and only v1 capability is `firewall.block_source` (owner decision D-17). Every unattended step is bounded by a **host-local ceiling** (below). Offline emergency permits and owner-signed policy bundles are DEFERRED past v1 (`docs/rework/06` "Deferred and after-v1").

## Policy scope and precedence

**Agreed direction:** support rules scoped by host, capability/category and target, plus defaults. **Changed by owner decision D-05:** the first draft said "explicit deny always wins". The owner replaced that principle: a more specific rule overrides a broader one, even a broader Deny, so that "deny everything on this host, except restarting this one container" works as written.

**Precedence (owner decision D-05; formalisation in [RW-10](../rework/03-architecture-decisions.md)):**

1. A rule's scope is its host, capability and target dimensions. Rule A is **strictly more specific** than rule B when A's scope is a subset of B's on every dimension and a strict subset on at least one. This is a partial order; there is no numeric score.
2. The strictly more specific matching rule overrides the broader one, **including a broader Deny**.
3. At **equal specificity, Deny wins**.
4. Conflicting rules that are **incomparable** (neither is more specific) resolve by **Deny > RequireApproval > Allow** (owner decision D-19). Publishing a policy version warns about both kinds of conflict, equal-scope rules with different outcomes as well as incomparable ones, shows how each resolves, and requires the owner to acknowledge them (RW-10, `docs/rework/04 §7.6`).
5. No matching rule → approval required or denied according to the operation class; never implicitly allow.
6. Host ids only in v1; host tags are DEFERRED (`docs/rework/06` "Deferred and after-v1").
7. **Locked Deny (owner decision D-26).** The owner may mark a Deny rule as locked (off by default). A locked Deny is a hard prohibition: no narrower rule overrides it, it is shown distinctly from ordinary rules in the editor, simulator, explanation and trace (with what ordinary resolution would have decided), and changing it needs step-up authentication.
8. **A policy Deny is never offered for approval (owner decision D-23).** The owner must change the policy; there is no approval bypass. A `RequireApproval` outcome, by contrast, is offered to the owner.

**Hard limits are not rules.** They apply after rule resolution and cannot be overridden by any rule, however specific: the host-local ceiling, Destructive operations never being policy-allowed in v1 (owner decision D-06), the mandatory containment exclusions, unknown or stale safety-critical facts never yielding Allow, and a host whose agent still runs as root never yielding an unattended Allow (owner decision D-29). A locked Deny (below) behaves the same way for the one prohibition the owner wrote it for.

The evaluator is pure and deterministic, produces an explainable decision record naming the winning rule, and is the same function the simulator calls.

Policy should constrain more than a risk label:

- Host and target scope (including allowed entities/paths/IP ranges).
- Operation and capability version.
- Preconditions and acceptable evidence freshness.
- Maximum effect, rate, frequency and concurrency.
- TTL/automatic expiry for temporary changes.
- Reversibility and rollback requirements.
- Trusted management networks, VPN paths and critical services.
- Allowed time windows or maintenance state where relevant.
- Whether the operation remains allowed when the hub is offline (offline operation is DEFERRED past v1; in v1 nothing runs unattended while the hub is unreachable).

## Illustrative per-host policy profile

This example captures the intended granularity; it is **not** a default policy or an instruction to enable these permissions automatically. Because a more specific rule beats a broader Deny, a "deny" written at host scope can be overridden by a narrower Allow; where the owner wants a denial to be final for a capability, it should be written at that capability's scope, or as a locked Deny (owner decision D-26), or be a hard limit (Destructive operations always are).

| Scope | Automatically allowed | Approval required | Explicitly denied |
|---|---|---|---|
| **NAS** | Restart the Plex container/service; restart other specifically allowlisted application containers; temporary block of a clearly identified external source through an approved firewall provider. | Restart SSH; OS/package updates; host reboot; changes to storage layout. | Delete user files, media, backups or storage pools; destructive cleanup. |
| **AI host** | Restart Ollama; unload a model from memory when the target is known and the operation is safe. | Host reboot; GPU driver changes; model downloads or environment changes with broad impact. | Delete model files or user data. |

The exact capability names and constraints would be configured per environment. The example demonstrates that one machine can have high autonomy for selected operations while retaining explicit denials for data loss, and another can allow only a narrow subset of local AI operations.

## Unattended execution guardrails

Beyond the policy decision, an unattended (policy-authorized) step is bounded by three further controls.

- **Host-local ceiling (owner decision D-03, [RW-12](../rework/03-architecture-decisions.md)).** Owner-controlled configuration on the agent's host lists, per capability, the explicit targets, a maximum rate per hour, constrained arguments where the contract allows them, and for Contain capabilities a maximum lifetime, a maximum number of active containments and mandatory management-path exclusions (owner decision D-18). No entry means no policy-authorized execution. The hub cannot change or widen it, the UI shows it read-only, and hub policy may only narrow it. Human-approved and policy-authorized executions stay distinguishable. A separate agent-side **mutation rate limiter** applies to all mutating executions, human- and policy-labelled (owner decision D-27), which bounds the damage a lying hub could do. It survives a reboot: entries are aged by the boot's monotonic clock and entries from before a reboot are kept for a full window, so a reboot or a wall-clock change never opens an unrestricted window (RW-38). A request it refuses is recorded as `Refused` with the reason (for example the rate limit), and the approval card shows that reason.
- **Target binding (owner decision D-07, RW-23).** An unattended step may target only the operation's **pinned subjects** plus targets **explicitly named by the matching owner-authored rule**. A target outside that set can only be routed to human approval, flagged as outside the operation's scope. Pinned subjects change only by owner command.
- **Operational brakes.** Per-rule and global rate limits, a human-only global **pause autonomy** switch, a circuit breaker that suspends a rule after repeated failed or unknown results, and shadow mode for every new rule.

**What the ceiling does not do.** It is an enforcement limit, not a signature. `authorization_kind` is a label the hub sets, not proof. A compromised hub can label a request `human` and so bypass the ceiling; this is an **accepted v1 limitation** (owner decisions D-03, D-18, D-27). It is still limited by everything the agent checks for every mutating request: its local capability catalogue and manifest, the plan hash, the arguments, `allow_risk` and the mutation rate limiter. A hub permit is therefore not a cryptographic proof of owner approval. Owner-signed approvals and policy bundles are the fix and are DEFERRED past v1 (`docs/rework/06` "Deferred and after-v1").

## Knowledge is not authority

User preferences, environment facts, recurring fixes and LLM confidence do not grant permission. A statement such as “restarting Plex usually fixes this” may influence planning; only a separate explicit policy rule can authorize that restart automatically.

Learning and permission changes are separate user actions. An experience suggestion may say “you have approved this three times; consider allowing it automatically”, but must not apply the rule.

## Separate authorization types

Keep the current human approval path distinct from autonomous authorization. The target introduces a typed policy permit that records the owner-defined rule, policy version, relevant conditions and execution scope (RW-11). Do not create a fake `HumanApproval` or allow model output to satisfy a human gate. Be precise about what enforces this: `HumanApproval` is a public type restricted by lint rules and a line-based scan in CI, not sealed by the compiler. Those controls stop careless code paths in review; they are not a runtime boundary against a compromised hub, which is why the plan hash, the agent's local checks and the audit chain exist.

Every action should carry a decision trace: requested operation, relevant policy rule/version, decision, conditions evaluated, actor/principal, plan hash, execution outcome and verification. The UI must make clear whether a human approved it or a policy authorized it. Past approvals and past successes are evidence for the reasoner and never authorization: they cannot satisfy a policy condition.

**Audit tamper evidence (owner decision D-14, RW-27).** Audit entries are hash-chained, computed inside the same transaction as the state change. The honest threat model: the chain detects accidental corruption, out-of-band edits and deletion in the middle of the chain; it does **not** stop an attacker who can rewrite the whole database and recompute the chain. As a cheap external anchor, the current head hash appears in the daily digest, so the owner holds off-host copies. Stronger anchoring is DEFERRED (`docs/rework/06` "Deferred and after-v1"). History that predates the chain stays unchained and is anchored once by a digest; verification and the audit page report that legacy part and the chained part separately.

Approval binding is staged (RW-11). The current action flow (F4) binds an approval to the existing plan hash v1: the manifest, the host and the arguments, not the entry-script bytes or the target's identity. M1 introduces plan hash v2 (script bytes, resolved target identity, execution id), and actions created before that keep v1 binding until they finish, labelled as such. M3 moves approval onto Operation steps. Systemd unit targets are identified by canonical id plus a hash of the fragment and drop-ins; generated and transient units are not valid mutation targets, and referenced files such as `EnvironmentFile=` and binaries are not bound, which the approval card states (RW-34).

The current implementation is human-approval-only for mutations. Treat that as a migration starting point, not as a permanent design limit. The target adds a separate owner-policy authorization path; never fabricate a human approval. Replace the current gate only when a typed policy decision, local enforcement, audit, migration and failure tests are ready together.

## Offline emergency authorization (DEFERRED past v1)

**Owner decision D-04:** offline emergency permits are **not part of v1**. In v1 an agent runs nothing unattended without a live hub permit inside its host ceiling. The design below is the direction for the later release; it is not open for v1. It is DEFERRED because it needs signed bundles first and carries clock, replay, revocation and lockout risks (`docs/rework/06` "Deferred and after-v1").

For security or protective operations that must work during hub disconnection, the agent may hold a small, signed, time-limited emergency policy cache. It must be:

- Explicitly created and authorized by the owner while connected.
- Narrow in host, capability, target and effect.
- Versioned, expiry-bound and revocable on reconnect.
- Unable to grant new capabilities or broaden existing scope.
- Audited locally in a tamper-evident spool and reconciled with the hub later.
- Safe under restart, clock uncertainty and duplicate execution.

When the cached permit is stale, expired or ambiguous, fail closed. This mechanism is not general offline autonomous reasoning with unrestricted powers.

## Containment in v1 (owner decision D-04, RW-13)

Containment (L3) ships in v1 as milestone M9, after bounded L2 autonomy is complete. It is a general **Contain** class, so the safeguards below apply to any contain capability, not just firewall rules.

- **First and only v1 capability (owner decisions D-17, D-22):** `firewall.block_source`, a temporary inbound block of one external address or small prefix. The block carries a kernel-enforced nftables timeout, so it expires even if the agent, the hub or the helper is down. Applying it is a two-step confirmation: the first element lives 30 s, and only a hub confirmation that management reachability still works extends it to the full TTL, by one atomic delete-and-add in a single nftables transaction (never by assuming that adding an existing element refreshes it); an unconfirmed block expires by itself. Only the root-owned helper's `nft` group touches nftables, and containment is not production-ready until the nftables research item R-2 is complete (`docs/rework/04 §9.1`).
- **DEFERRED:** `container.pause` and `unit.stop_temporary`. Their only expiry today would be an agent-side timer, which is not reliable automatic expiry (a paused container stays paused if the agent is down). They may be reconsidered in v1.x once expiry is independent of the agent and recovery is proven (the checklist in `docs/rework/04 §9.3`). Container network isolation, SSH session termination and account lock are likewise deferred for weaker recovery or lockout stories (`docs/rework/06` "Deferred and after-v1").
- **Triggering.** Automatic containment is eligible only when a deterministic detector emitted a typed signal whose structured fields name the target (for example a source address) and that target is the operation's pinned subject. **Free-form model output never triggers or targets automatic containment.** The model may investigate, explain and recommend; its recommendations go through human approval.
- **Management paths (owner decision D-28).** Unattended containment on a host requires owner-confirmed, current management paths, and every confirmed path must be covered by the exclusions the agent itself reports (a hash and a summary sent to the hub). Unknown, stale, conflicting or uncovered paths mean it is refused, a mismatch or any network or exclusion change blocks it until the owner re-confirms, and the agent and helper enforce their own local exclusions regardless of the hub (RW-37). This affects neither monitoring nor other human-approved work. The UI shows the confirmed paths beside the agent-reported exclusions, where each came from, how fresh it is and which containment is blocked.
- **Safeguards.** Expiry independent of the main agent on every containment, a watchdog for connectivity effects (the 30 s two-step above), caps on simultaneous containments, verification that management access still works, outcome-unknown handling, and drills before any host is enabled unattended (owner decision D-22). The hub cannot empty the mandatory exclusions.

## Security containment example: suspected SSH brute force

A safe flow is evidence-first and protects the management path:

1. Correlate authentication failures and identify source addresses, destinations, timing and whether any login succeeded.
2. Determine whether sources are external or internal; compare against explicitly trusted admin/VPN ranges and active management sessions.
3. Identify where SSH is exposed and which provider controls the relevant firewall (host firewall, router or cloud rule).
4. Choose the narrowest owner-authorized containment, preferably a temporary block of a specific external source with a TTL.
5. Enforce exclusions for trusted management addresses, VPN endpoints, loopback and critical infrastructure; cap the number and duration of simultaneous blocks.
6. Verify the block is active and that trusted management access still works.
7. Monitor for recurrence or displacement to other sources; report the outcome and expiry.

Do not reflexively stop SSH or apply a broad firewall policy: that can lock the owner out and may not contain the real source. If the enforcement point or trusted path cannot be established, ask the owner or escalate.

## Hard safety boundaries

- Destructive data operations (delete files, pools, backups, databases or user data) are **never policy-allowed in v1** (owner decision D-06): the evaluator enforces this as a hard limit that no rule can override. A human can still approve one, with typed confirmation.
- Policy/configuration changes, host reboot, updates, credential changes and broad network changes require explicit handling; they are not ordinary L2 actions.
- The model cannot modify its own policy, grant itself capabilities, approve its plan or disable audit.
- A capability must declare target, effect, prerequisites, rollback/expiry and verification. If it cannot be bounded, it is not eligible for autonomous execution.
- Agent-side validation is mandatory even after hub authorization.
- The agent gets no general Docker socket write access, which would be root on the host. The agent runs as an unprivileged user (owner decision D-29); Docker write operations and other privileged host operations go through one narrow, root-owned first-party helper with a root-owned allowlist and its own ADR, installed only by the owner-controlled OS path and never by a package or skill. Docker write capabilities are unavailable for unattended use until it exists (owner decisions D-24, D-29, RW-39).
- Packages and skills run as the agent user, never root. A skill that needs another `run_as` user is an open owner question (O-30; recommended: unsupported). A root install is labelled and gets no unattended execution.
- Unknown, stale, conflicting or unavailable safety-critical context means no autonomous execution.

---

# 06 — Knowledge and experience

## Keep three stores conceptually separate

| Store | Answers | Examples | Must not be used as |
|---|---|---|---|
| **Knowledge** | What is true or believed about this environment? | NAS mount location, service owner, app dependency, trusted VPN subnet, user preference, recovery path. | Permission to perform an operation. |
| **Policy** | What is Domovoi allowed to do, under which conditions? | Allow `restart_service` on `plex`; deny all file deletion; require approval for host reboot. | A fact about the environment or a historical observation. |
| **Experience** | What happened, what did Domovoi decide/do, and what was the outcome? | Incident timeline, evidence requests, plan attempts, approval/denial, execution, verification, operator correction. | A guaranteed current fact or an authorization grant. |

They inform one another but cannot be merged semantically. They stay separate in meaning, not necessarily in storage: **the experience record is the operational journal** (one append-only table of typed events, RW-15), not a third event store, and the knowledge facts reference inventory that already exists rather than copying it. An observed fact may inform a policy proposal; a human must explicitly accept a policy change. An experience may suggest a knowledge update; it does not overwrite facts without provenance-aware validation.

## Knowledge categories

- **Environment facts:** host role, OS, mount points, service manager, storage pools, network interfaces, deployed applications.
- **Infrastructure relationships:** app depends on database; reverse proxy routes to service; container belongs to compose project; backup targets a separate device.
- **Operational constraints:** never interrupt a backup, keep VPN available, do not reboot during a migration.
- **User preferences:** preferred maintenance windows, notification style, known acceptable restarts.
- **Recovery information:** console access, alternate management route, backup/restore path, recovery commands/runbooks.
- **Historical decisions:** update rejected, migration deferred, incident accepted as expected.
- **Learned patterns:** recurring failure after a certain update, a known symptom/cause relationship (marked as inferred until confirmed).
- **Temporary state:** backup running, maintenance in progress, temporary IP block. Store as expiring operational state, not as durable environment truth.

## Entity and relationship model

Represent meaningful things as subjects: host, service, container, database, filesystem, disk, storage pool, network, router, domain, integration, user-defined operational concept. Connect them with typed relationships such as `runs_on`, `depends_on`, `routes_to`, `stores_on`, `backs_up_to`, `exposed_through`, `managed_by` and `managed_from`.

**Owner decision D-02 / [RW-14](../rework/03-architecture-decisions.md).** Subjects are **typed references** (a host, a container on a host, a unit on a host, or a user-defined entity), not a second copy of the inventory. The learned baselines remain the owner of observed containers, units and ports; facts and relations point at them. A small entities table exists only for subjects the user defines (for example "VPN" or "NAS pool"). The set of predicates is a closed, typed list in v1.

Do not create graph nodes for every log line or metric. Those belong in event/measurement records; link only useful incidents, facts and experiences to relevant entities.

## Fact provenance and lifecycle

A fact/claim should carry, at minimum:

- Subject entity and typed predicate/value.
- Scope (global, host, entity or service).
- Source (user-confirmed, agent-discovered, integration-reported, inferred, imported).
- Verification status (for example observed, user-confirmed, inferred). There is **no numeric confidence** field: model confidence is not a calibrated probability, and a number would suggest otherwise.
- Observed/valid-from time and optional valid-until time.
- Last-verified time and freshness policy.
- Creator/updater and timestamps.
- Status (current, stale, disputed, superseded, retracted).
- Optional link to the fact it supersedes and supporting evidence/experience.

Missing or unreadable data remains `unknown`/`unavailable`; do not write false or empty values as confirmed facts. When sources conflict, preserve provenance and surface the conflict instead of silently selecting the newest text.

Lifecycle: discovered or supplied → validated → used in context → reverified/updated → superseded, expired or retracted. AI inference cannot become user-confirmed merely because it is repeated.

## Management paths are safety-critical knowledge (owner decision D-28)

The facts that say how the owner reaches a host (management networks, VPN endpoints, remote management routes, recovery access) decide whether unattended containment may run on it. They must be owner-confirmed and current: unknown, stale or conflicting entries mean unattended containment is refused on that host. A person can still approve work there and monitoring is unaffected. Each path shows its provenance, its freshness and what it blocks, and the agent keeps local exclusions the hub cannot empty. The agent reports a hash and a summary of those exclusions; every hub-confirmed path must be covered by them, and a mismatch, or any network or exclusion change, blocks unattended containment until the owner re-confirms (RW-37, `docs/rework/04 §7` and `§9`).

## Experience: finding past operations (owner decision D-02, RW-25)

Past incidents and how they were solved must be easy to query.

- The journal holds typed events linked to subjects, so an operator can ask "what happened to this container" and get a precise answer, not a text search over prose.
- Every finished operation gets a **resolution summary**: deterministic fields filled by code (subjects, capabilities executed, verification results, outcome) and narrative fields (problem, cause, fix) drafted by the model, marked as model-sourced and editable or confirmable by the owner. Summaries belong to the Operation aggregate and the operation writer is their only writer: the Knowledge UI sends edit commands to it, and edits to code-owned fields are rejected (RW-41). Summaries are full-text indexed.
- The model has a host-scoped `search_experience` read capability. The UI shows **similar past operations** on incident and operation pages.
- When experience is supplied to the reasoner, each item carries its operation id, date, verified flag and source, inside a block labelled historical evidence, not authorization. **Past approvals and past successes are evidence and never satisfy policy.**
- Retention of raw step output and of durable summaries are separate, both configurable (owner decisions D-09, D-21; defaults in `07`).

## Onboarding and discovery

Use two layers:

**Hub/environment onboarding:** overall infrastructure shape, host criticality, trusted networks and VPN, remote management paths, services that must never stop, backup locations, notification preferences and initial autonomy choices.

**Host onboarding:** host purpose and criticality, recovery access, storage layout, important services/dependencies, network exposure, backups, maintenance constraints and local restrictions.

Let discovery fill what it can from facts, checks and read capabilities. Ask the owner only about important unknowns or ambiguous discoveries. Permit “unknown / not sure”; do not force guesses. Store answers so the same question is not repeatedly asked. Allow later correction, deletion and scope changes.

## Knowledge proposals

At incident resolution or during a repeated pattern, Domovoi may propose a fact, relationship, preference or constraint to remember. Show the proposed statement, source/evidence, scope and confidence. User confirmation is required for preferences and operational constraints; automatically discovered facts may be saved with their unverified source and can later be corrected. No proposal can alter policy implicitly.

Deletion must remove the record from search indexes, graph edges, caches and retrieval paths. Audit may keep minimal deletion metadata but should not preserve deleted sensitive content. A deleted or retracted fact leaves the live database, the full-text index and model context at commit (retracted content is purged after 30 days, leaving a tombstone), but it remains in backups until they rotate (up to 7 nightly and the last 3 pre-migrate copies); restoring a backup brings it back, and the UI and book say so (owner decision D-21, `docs/rework/04 §14`).

## Notes remain useful

Keep freeform Markdown notes/runbooks. They are good for procedures and context that do not fit a structured fact. Allow notes to link to entities, incidents and capabilities. Do not replace notes with a brittle universal fact table.

## Management UI

Make knowledge a first-class, editable area:

- Search/browse by host, entity, category, source, status and freshness.
- Edit, confirm, dispute, supersede and delete records.
- View an entity's facts, relationships, notes, incidents, experiences and relevant policy.
- Provide a graph explorer for relationships and an experience view of past operations (timelines, cycles, resolution summaries, similar operations).
- Show provenance and status wherever a fact influences a diagnosis or action.

The UI should make it easy to fix incorrect knowledge; users should not need SQL or file edits.

---

# 07 — Storage and search

## Recommendation

**Use the existing SQLite store as the initial home for structured knowledge and experience. Model relationships as a graph, but do not introduce a separate graph database yet.**

This is a recommended default, not a permanent technology commitment.

## Why this fits Domovoi

- The current product already uses one embedded SQLite database, WAL mode, migrations and automatic backups.
- The deployment goal is self-hosted and operationally small; a second database creates backup, upgrade, monitoring, security and consistency work.
- Relational tables are appropriate for policies, actions, incidents and audit. Typed relationship tables plus recursive CTEs support useful graph traversals. JSON attributes handle genuinely variable metadata. FTS5 covers lexical search.
- The decision can be hidden behind repository/service ports. If real workload later demands a graph engine, the storage implementation can evolve without exposing a database-specific API to the model. SQLite supports recursive CTEs for graph/tree traversal, JSON functions for flexible attributes, and FTS5 for full-text search ([recursive CTEs](https://www.sqlite.org/lang_with.html), [JSON functions](https://www.sqlite.org/json1.html), [FTS5](https://www.sqlite.org/fts5.html)).

A pure document/NoSQL store is not automatically more flexible: it can make integrity, typed relationships, policy evaluation and transactional action state harder. Keep the stable/query-critical structure relational and use JSON only for genuinely variable properties.

## Logical data areas

Keep these as logically distinct modules even if they share one SQLite file:

1. **Operational state:** hosts, agents, incidents, signals, actions, approvals, policies, settings, audit and orchestration state.
2. **Knowledge graph:** entities, typed relationships and sourced facts/claims.
3. **Experience = the journal:** one append-only table of typed events for incidents, operations, conversations and knowledge, with a global sequence number. It is also the change stream the live UI replays, so there is no separate experience event store (owner decision D-02, RW-15). Beside it sit the hash-chained **audit** record (the security record, written in the same transaction) and the **effects** outbox (pending side effects, not an event store).
4. **Documents:** existing Markdown notes/runbooks and their FTS index.
5. **Search structures:** ordinary indexes, FTS5 and (later, if justified) vector embeddings.

## Conceptual schema

Not final DDL; the authoritative schemas are in [`docs/rework/04 §10`](../rework/04-target-architecture.md) (RW-14, RW-15, RW-25). IDs, constraints, normalization and lifecycle fields require that specification.

```sql
-- Subjects are typed refs: host:<id>, container:<host>/<name>,
-- unit:<host>/<name>, entity:<uuid>. Observed inventory stays in the
-- baselines tables, which own it; facts and relations only point at it.
entities(                       -- user-defined subjects only
  id, kind, name, attributes_json, lifecycle, created_at, updated_at
)

relations(
  id, subject_ref, relation_type, object_ref,
  properties_json, source, status,
  valid_from, valid_until, observed_at, last_verified_at
)

facts(
  id, subject_ref, predicate, value_json,
  source, status, observed_at, valid_until, last_verified_at,
  supersedes_id, evidence_ref, created_at, updated_at
)                               -- no numeric confidence column

journal(                        -- the experience record and the SSE change stream
  seq, occurred_at, actor, kind, topic,
  operation_id, incident_id, subject_refs, payload_json
)

resolution_summaries(           -- one per terminal operation, full-text indexed; stored as field revisions (below)
  operation_id, subjects, capabilities_executed, verification, outcome,
  problem, cause, fix, narrative_source, confirmed_by, updated_at
)
```

Resolution summaries are append-only field revisions owned by the Operation aggregate; the operation writer is the only writer, and the Knowledge UI sends edit commands to it, which reject user edits to code-owned fields and stale revisions (RW-41). The earlier draft had `experience_events` and `experience_links` tables and a numeric `confidence`. Both are dropped: the journal is the experience record, links to subjects live on the journal entry as typed refs, and a fact carries a status and a source, not a number. The `digest_facts` table, which holds metric samples and collides with the word "facts", is renamed `metric_samples` first.

Use typed definitions/validation for relation types and predicates (a closed enum in v1). Do not create unconstrained EAV where every field is a free-form string. Keep stable/query-critical fields relational; put only variable optional properties in JSON. Store evidence references/hashes and bounded summaries rather than unlimited raw logs or secrets.

## Search strategy

Start with the simplest search that solves real user queries:

- Exact and filtered SQL search for host, type, status, source and dates.
- FTS5 for notes, titles, entity descriptions and experience summaries.
- Bounded recursive CTE traversal for relationships and small relevant subgraphs.
- Structured predicates for current facts, freshness and provenance.
- Later: local embeddings/vector similarity only when evaluation shows lexical/graph retrieval is insufficient. Vector search and a graph database are DEFERRED until a measured need (`docs/rework/06` "Deferred and after-v1").

Do not introduce a vector database just because the system is AI-powered. Do not introduce a dedicated graph database until measured multi-hop query complexity, scale, concurrency or required graph algorithms justify the operational cost.

## Retrieval contracts

Expose domain operations rather than raw database queries, for example:

- `search_knowledge(query, scope, filters)`
- `get_entity(entity_id)`
- `get_facts(entity_id, predicates, freshness)`
- `get_relationships(entity_id, relation_types, direction)`
- `traverse_graph(start_entity, relation_filter, max_depth, max_nodes)`
- `search_experience(query, scope, time_range)` — a host-scoped read capability over resolution summaries and journal entries; the model sees it as a meta-tool, and results carry operation id, date, verified flag and source
- `get_incident_experience(incident_id)`
- `get_notes(query, entity/host scope)`
- `propose_knowledge_change(...)`

Every traversal must be bounded by depth, node count, relationship filters and relevance. Context retrieval returns a compact, relevant subgraph; never serialize the whole graph into an LLM prompt. Mutation tools must be authenticated/authorized and schema-validated. The model can propose changes, but the service owns validation and confirmation workflow.

## Consistency and retention

- Hub is canonical for long-lived knowledge, policy and experience.
- Agent caches contain only the versioned subset required for local enforcement (offline emergency actions are DEFERRED past v1, `docs/rework/06` "Deferred and after-v1").
- Track policy/knowledge versions and reconcile cached data on reconnect.
- Keep journal entries append-only for auditability, with explicit retention/redaction rules for sensitive payloads.
- **All retention is configurable** (owner decisions D-09, D-21, RW-28). Raw data and durable resolution summaries are separate retention classes, and the interaction with full-text indexes, backups and the audit chain is defined in `docs/rework/04 §14`. The defaults are in the table below.
- Audit entries are hash-chained (owner decision D-14); pruning writes a chained checkpoint row, so retention never silently breaks verification. History that predates the chain is anchored once by a digest and verified separately from the chained part.
- Keep current facts mutable through superseding/retraction, not destructive overwrites that erase provenance.
- A deletion workflow must clean derived FTS/vector/cache data and prevent retrieval of the deleted content.

### Retention defaults (owner decision D-21; all configurable)

| Data | Default |
|---|---|
| Signals | 30 days |
| Incident timeline (raw) | 90 days |
| Operation journal (raw) | 180 days |
| Raw execution outputs | 30 days |
| Operations, steps and resolution summaries | retained |
| Audit | retained (checkpoint pruning only if configured) |
| Retracted fact content | purged after 30 days, tombstone kept |
| Conversations | 180 days |
| Notifications and notification state | 30 days |
| Nightly backups | 7 |
| Pre-migrate backups | last 3 |

**What deletion guarantees.** Deleted data is gone from the live database, the full-text index and model context at commit. It remains in backups until they rotate, and restoring a backup brings it back; the retention settings and the book say so.

## Migration approach

1. Keep current relational operational tables unchanged initially. New tables are additive migrations; existing action records are retained and shown through a read-only legacy projection rather than rewritten (RW-32).
2. Add a new knowledge repository/service alongside `NoteRepo`; do not overload the current `knowledge.rs` module that currently implements notes, baselines, acknowledgements and digest facts.
3. Add typed tables and indexes through append-only migrations.
4. Link existing hosts, skills, notes, incidents and signals to typed subject refs incrementally; do not copy baseline inventory into new tables.
5. Retain existing notes and `notes_fts`; add entity linkage without forcing Markdown into structured facts.
6. Add graph and semantic search only behind service interfaces.

## Reconsider a dedicated graph store only if

- Production queries require deep/many-hop traversal or graph algorithms that recursive SQL cannot serve efficiently.
- The knowledge graph grows beyond a practical embedded workload or needs independent scaling.
- Multiple services/clients need concurrent graph-native access.
- A graph query language materially simplifies maintenance and the operational trade-off is justified.

Until then, a SQLite-first hybrid model is simpler and sufficient for the target scale.

---

# 08 — Current-state gap analysis

> **Snapshot only:** verified code baseline `393694b` (UI overhaul merged). Existing architecture docs and ADRs may be outdated; they are navigation clues, not binding design authority.
>
> **Corrections (2026-11-13):** the repository investigation in [`docs/rework/01-repository-reality.md`](../rework/01-repository-reality.md) found four statements below to be wrong or imprecise. They are corrected in place: the agent link is not mutual TLS; the plan hash covers the manifest, not the script bytes, and the approve API carries no hash; verification and the triage queue are in memory and not restart-safe; and `HumanApproval` is restricted by a line-based scan, not sealed by the compiler. The rest of this table is unchanged and still a point-in-time inspection.

## Current state vs target

## Baseline inspected

- Repository: [`RoastSlav/domovoi`](https://github.com/RoastSlav/domovoi)
- Branch: `main`
- Commit: [`393694b0d16ad722961748744226e80b8cbac518`](https://github.com/RoastSlav/domovoi/commit/393694b0d16ad722961748744226e80b8cbac518)
- Commit time: 2026-11-12 17:25:41 UTC
- Project status in README: v0.1, before first release.

This is a point-in-time inspection. Re-check the repository before using this document for implementation.

## Executive comparison

Domovoi already has a strong safety-oriented foundation: hub/agent separation, typed read tools, local skill manifests and revalidation, deterministic action lifecycle, incident/action single-writer patterns, SQLite, notes search and host profiles. The target may reuse, refactor or replace these foundations. The current design documents are not assumed to accurately describe all current behavior.

The largest missing pieces are: a general capability contract beyond skills/read-tool enums; durable multi-phase orchestration with dynamic evidence requests; owner-defined autonomous authorization; structured, provenance-aware environment knowledge and relationships; a first-class experience model across cycles; and UI/workflows for managing these systems.

## Feature-by-feature map

| Area | Current `main` | Target | Gap |
|---|---|---|---|
| Product role | Monitoring, LLM triage, chat notifications and human-approved skill execution. | Adaptive operations agent with bounded autonomy. | **Major product expansion**, while preserving the current monitoring loop. |
| Deployment | One Rust binary with hub and agent roles; agent dials out over a TLS WebSocket. The agent pins the hub's certificate; the agent's identity is an ed25519 challenge bound to the TLS channel (this is **not** mutual TLS). | Same basic hub/agent split, with external capability providers as needed. | **Mostly reuse**; extend provider/protocol discovery. |
| Process safety | `domovoi-runner` is the only process spawner; argv-only, no shell; skills have typed manifests, limits and redaction. | Same invariant applies to legacy skills; general operations use typed capability providers. | **Preserve and generalize**, do not weaken. |
| Skill system | Local `skill.toml`; read/write/destructive risk; typed args; hosts/applies_to; timeout/output; optional verify/revert; hub per-host enabled state; agent local `allow_risk` and manifest/hash/arg revalidation. | Skills become one capability provider among native tools and integrations. | **Partial**: strong script execution boundary, but not a cross-provider capability catalogue. |
| Read tools | Closed `ReadTool` / `ReadToolCall` enum: Docker, host overview, journal, listening ports, SMART, incident history, notes search and read skills. | Discoverable typed observation and operation capabilities. | **Partial**: extensible only through code/schema changes; no general provider registry. |
| Discovery | Agent handshake shares host facts, check inventory and skill summaries/catalogue metadata. | Agent/provider announces typed operations, prerequisites, effects, verification, scope and version. | **Partial**: inventory exists, but capability semantics are incomplete. |
| Checks and signals | Built-in host/container/system/network/security checks; webhook and probe ingestion; incident correlation and deduplication. | Same detection layer feeds phase-based orchestration and richer context. | **Reuse**; likely add domain-specific checks and context sources incrementally. |
| Incident lifecycle | Durable incident state machine, signal history/timeline, confirmation, ack/mute/resolve/reopen, incident manager single writer. Incident, signal and timeline writes are separate statements, not one transaction. | Incident becomes durable work item with resumable phase/cycle state. | **Partial**: durable incident/timeline exists; orchestration phase state does not. |
| Triage | Bounded model conversation with read-only tools, token/tool/time budgets, prebuilt context, fallbacks and report schema. The triage queue and its deferred retries live in memory and are **not restart-safe**. | Dynamic request/response investigation, structured hypotheses/uncertainty, phase outputs and loopable plans. | **Partial**: bounded tool-use foundation; context and conversation are still one triage-oriented workflow. |
| Context collection | Current context assembles incident snapshot, signals, recent changes, similar incidents, host profile note, notes and skill catalogue. | Layered, domain-specific context builder with explicit missing/unknown/unavailable/failed/stale statuses and on-demand evidence requests. | **Partial**: context assembly exists; generalized evidence model and dynamic requests are missing. |
| Action lifecycle | Proposed → human approved → dispatched → success/fail → verify; plan hash; manifest revalidation; audit; loop guard; action writer single task. No automatic retry. The plan hash covers the skill manifest but **not the entry script's bytes**, and the approve API **carries no hash** (the plan hash is checked on the hub and the agent, but the approving human does not submit the one they saw). The audit record is appended after the state change as a separate statement whose failure is only logged. Post-action **verification timers are in memory** and are lost on restart. | Per-step authorization, policy permits, idempotency, resumable plans, re-plan after failure, postcondition and monitor windows. | **Partial**: robust approval/execution base; autonomy and multi-cycle plans are absent. |
| Autonomy | Human-only approvals; destructive typed confirmation; local risk allowlist is not an autonomous owner policy. | Granular per-host/category/capability policy; L2 and L3; offline emergency authorization. | **Missing**. Must add a separate owner-policy authorization path; the current human-only mechanism is a migration starting point, not the target. |
| Authorization invariants | `HumanApproval` construction is restricted to auth code by a **line-based text scan in `xtask arch-check`**, not by compile-time sealing (the type and its constructors are public in `domovoi-core`, so a renamed import or macro would evade the scan); `ReadTool` has no mutation/approval tool; agent revalidates every skill request. | Separate human approval and owner-granted autonomous permit types; deterministic policy evaluator. | **Preserve then extend via a deliberate authorization redesign**. Do not treat a `System` principal or allow-risk list as sufficient autonomy. |
| Knowledge — notes | Global/host Markdown notes; CRUD; tags; SQLite FTS5 search; triage can retrieve relevant notes. | Keep notes, link them to typed entities and structured knowledge. | **Present** for documents; add entity linkage and a structured store. |
| Knowledge — profiles | Hub collects host facts, overview, containers, listening ports, kernel messages and selected read-skill results; stores a generated host profile as a tagged note. | Editable, provenance-aware, structured facts with freshness/conflict lifecycle. | **Partial**: useful discovery/profile foundation, but stored as generated Markdown rather than a fact graph. |
| Knowledge — facts/graph | Host facts, baselines (containers/units/ports), digest facts, ack rules and maintenance windows exist as separate concepts/tables. No general entity/relationship/fact model found. | Typed entity graph and claim/fact records with source, confidence, validity and supersession. | **Missing as a general model**; reuse IDs and host/incident relationships where possible. |
| Experience | Incident events/timeline, actions, append-only audit, triage cache, prior incident search and follow-up context. | Explicit experience across phase transitions, hypotheses, evidence requests, plans, policies, actions, verification and learning. | **Partial**: core operational history exists; richer cross-cycle experience model and explorer are missing. |
| Persistence | SQLite + SQLx, WAL, append-only migrations, backups, FTS5 for notes/incidents. | Relational operational core + graph-shaped typed tables + experience events + lexical search; vector search optional later. | **Good fit already**; no reason to add a separate DB now. |
| UI | Hosts/profile, incidents/timelines, approvals, skills, notes, settings and service status. | Knowledge browser/entity detail/graph, experience explorer, autonomy policy editor and explainable policy outcomes. | **Partial / missing new views**. |
| Suggestions and learning | A design proposal in the existing docs describes deterministic suggestions from history (new skill, ack rule, check/threshold); verify whether any of it was implemented before relying on it. Suggestions should never silently change permissions. | Suggest knowledge improvements and possible policy changes without applying authority. | **Related foundation in design only**; preserve human review. |
| Workflows/playbooks | No general incident playbook runtime described as the core execution model. | Optional guidance that the reasoner can use or depart from. | **Not a blocker**; add later if recurring patterns demonstrate value. |

## Existing code anchors

These are starting points in the verified source tree, not prescribed future module boundaries. Future design work should inspect their current contents, tests and call sites before deciding what to retain:

| Concern | Current location |
|---|---|
| Enforced boundaries and security invariants | [`ARCHITECTURE.md`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/ARCHITECTURE.md) |
| Skill manifest and usage | [`docs/book/src/guide/skills.md`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/docs/book/src/guide/skills.md) |
| Action and approval lifecycle | [`docs/book/src/concepts/actions.md`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/docs/book/src/concepts/actions.md) |
| Closed set of model read tools | [`crates/domovoi-core/src/triage/tool_schema.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-core/src/triage/tool_schema.rs) and [`tools.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-engine/src/triage/tools.rs) |
| Triage context assembly | [`crates/domovoi-engine/src/triage/context.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-engine/src/triage/context.rs) |
| Agent-side skill revalidation | [`crates/domovoi-agent/src/requests/skills.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-agent/src/requests/skills.rs) |
| Host profile collection/UI API | [`crates/domovoi-hub/src/profile/collect.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-hub/src/profile/collect.rs), [`api/profile.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-hub/src/api/profile.rs) |
| Notes CRUD and search | [`crates/domovoi-hub/src/api/notes.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-hub/src/api/notes.rs), [`crates/domovoi-store/src/fts.rs`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-store/src/fts.rs) |
| Database schema | [`crates/domovoi-store/migrations/0001_init.sql`](https://github.com/RoastSlav/domovoi/blob/c367e54ecc348f534b32a5bef89146747625104d/crates/domovoi-store/migrations/0001_init.sql) |

## UI baseline — verified after the 2026-11-12 overhaul

The latest main commit merges PR [#111](https://github.com/RoastSlav/domovoi/pull/111), `feat(web): redesign the UI around what the operator needs to see and decide`, at `393694b0d16ad722961748744226e80b8cbac518`.

### What the overhaul currently provides

- A rewritten `web/DESIGN.md` and stronger semantic token system. The warm charcoal/sand dark theme, paper/ink light theme, moss accent and walnut brand mark are deliberate. Semantic tones cover danger, warning, success, pending, progress, info and neutral.
- A new app shell with grouped collapsible sidebar, top bar/breadcrumbs, host selector, live connection state, user/theme menu and Ctrl/Cmd+K command palette.
- Command palette results for pages, hosts, open incidents, settings sections and basic actions (theme, sidebar, logout). **This is a useful command palette foundation, not yet the complete cross-domain search** for operations, services, capabilities, knowledge and policies described by the target.
- A new Overview page with summary tiles, high-priority incidents, approvals waiting and hosts with problems.
- A denser incident list and detail page: severity stripes, responsive table/cards, triage summary, evidence, action groups, tools used, timeline and incident chat.
- A more usable Approvals page with compact waiting rows when the list is busy.
- A more information-rich Hosts page: cards/table, filters, health/fact overview, storage, containers, network/profile and SSH login details.
- Better settings rows, source labels, unsaved-change bar and safer field behavior.
- Shared UI components, design demos, tests for layout and component behavior, contrast/accessibility checks and a documented bundle budget.
- Browser checks reported by PR #111 at 375, 768 and 1440 px in both themes, with a busy sample fleet; PR validation reports 735 tests, axe checks, lint/typecheck/format/build and cargo fmt/clippy for the dev hub.

### Important current UI behavior and limits

| Area | Current implementation | Target direction |
|---|---|---|
| Navigation | Overview, Incidents, Approvals, Chat; Hosts, Skills, Logs; Known conditions, Notes, Digest; Settings. | Reorganize around Operations, Infrastructure, Capabilities, Knowledge, Policies and Integrations. Existing groupings are not binding. |
| Overview | Counts, attention list, approval list, problem hosts. | Configurable Control Room with active operation visibility, richer environment status, current work and attention center. |
| Incident detail | Human-readable triage result, evidence, proposed/running/earlier actions, tool-use summary, timeline and incident chat. | Keep incident as detected problem; link to one or more full Operations. Add graph/trace context without duplicating the entire operation workspace. |
| General Chat | Host-scoped read-only Q&A. Conversation is stored in `sessionStorage`, limited recent history sent to the hub; no mutating action interface. | Operator side panel + full workspace, durable conversations, task/operation creation and shared orchestration/policy/capability path. |
| Operations | No unified first-class Operations list/detail currently found in the web routes. Current `actions` are skill action records, not the full operation concept. | First-class Operations index and detail with dominant orchestration graph, history, plan, evidence, calls, policy, execution and verification. |
| Orchestration visualization | No live phase/branch/cycle graph currently found. Incident page has a timeline and tools-used summary. | Interactive graph with phases, branches, cycles, waits, parallel calls, node inspector and historical playback (a late U2 deliverable, owner decision D-25; the timeline and history are fully useful without it) and a list alternative. |
| Hosts | Current host detail tabs are Overview, Profile, Checks, Logs and SSH logins. | Redesign from scratch around Overview, Workloads, Capabilities, Monitoring, Network/Security, Knowledge/Dependencies and Activity (subject to prototypes). |
| Skills/capabilities | Per-host skill list; enable/disable; details open in a dialog; scripts/manifest are installed on agent. | Capability Center catalog, installed inventory, deployments, capability detail and guided installation to agents from the UI. Skills become one provider. |
| Knowledge | Markdown Notes, FTS search, host profiles, learned baselines, known-condition rules. | Structured entities/facts/relationships with provenance/freshness/confidence, graph explorer, editable experience and knowledge suggestions. |
| Policies/autonomy | Current skill risk/local allow-risk and human approval path; no complete granular owner policy UI or historical what-if simulator identified. | Granular policy editor/matrix and read-only historical/current/proposed policy simulation using the same evaluator as live execution. |
| Search | Ctrl/Cmd+K command palette covers a useful but limited set. Page-specific searches exist for incidents, hosts and notes. | Permission-filtered global search across all important entities and command actions, grouped by object type. |
| UI preferences | Light/dark/system theme, sidebar rail, cards/table view on Hosts and some saved URL filters. | Detailed/Standard/Essential/Custom presentation profiles, per-page density, customizable dashboards and graph preferences. |

### Current visual/design anchors to reuse selectively

- `web/DESIGN.md` is the current design rulebook; its principles include crisp/quiet/dense/precise presentation, a 4 px grid, semantic tone/color, contextual help, truthful loading/error/offline states, same-origin assets and short motion.
- `web/src/styles/tokens.css` contains palette and semantic tokens, spacing, radii, type, control sizes, layout sizes, layers and motion tokens.
- `web/src/design/tone.ts`, vocabulary files and `icons.tsx` define consistent statuses, terminology and icon names.
- `web/src/components/` contains reusable controls, dialogs, menus, tables, tabs, timelines, feedback, loading/error and chat building blocks.
- `web/src/dev/DesignPage.tsx` and the `/dev/design` page provide an interactive component/design-system catalogue.
- `web/src/test/` and feature tests cover accessibility, contrast, UI behavior and performance/guardrail contracts.

These are potential foundations, not a command to reuse existing page layout. A new orchestration canvas, policy simulator, capability deployment flow or Operator workspace should get components designed for its actual needs.

### UI gap summary

The current UI is now a credible v0.1 operator interface, but it is still centered on the existing incident/approval/host/skill model. The target UI is broader: it must manage durable operations, show the orchestration engine working, provide a full conversational Operator, install/manage capabilities, edit structured knowledge, configure granular autonomy and simulate policy outcomes. The main missing work is not cosmetic polish; it is new product surfaces and a new information architecture.

## Migration implications

- Do not replace working components wholesale without understanding their invariants and migration implications; the target may still justify substantial redesign where the current structure limits the product.
- Wrap existing read tools and skills as initial capability providers where useful; decide which manifest/protocol contracts should be generalized, versioned or replaced.
- Reuse incident timeline and audit data where it fits, while designing a complete orchestration experience/event model rather than forcing all history into the current timeline schema.
- Add structured knowledge and experience models; migrate or link notes, profiles, baselines and incident data where their semantics fit.
- Introduce autonomy only after its policy and authorization model, migration, UI, agent-side enforcement tests and failure drills are designed and validated together.
- Preserve a tested compatibility path during migration where useful. New autonomous permissions should default off until explicitly configured and validated.

---

# 09 — Open questions and recommended defaults

The high-level architecture is coherent enough to proceed. On 2026-11-13 the owner answered the questions that blocked the rework (`D-01`..`D-29` in [`docs/rework/99-open-decisions.md`](../rework/99-open-decisions.md)); they are listed first and are not open. One owner question is open: **O-30**, skills whose manifest sets `run_as` to another user (below). What remains besides it are detailed-design questions (kept open so later design work does not silently invent answers) and research and evidence items (`R-1`..`R-8`, [`99-open-decisions.md`](../rework/99-open-decisions.md) §C, summarised below). Technical detail for every answered item is in the linked `RW` decision in [`docs/rework/03-architecture-decisions.md`](../rework/03-architecture-decisions.md).

## Answered by owner

| Pack question or topic | Owner decision | Answer | Technical detail |
|---|---|---|---|
| Release strategy | **D-01** | No separate v0.1 release. Foundation work (atomic writes, exact protocol rule, approval freshness, reliability fixes) comes first inside the rework. | RW-02 |
| **Q0.1** What is an autonomous authorization? | **D-02, D-03** | A typed policy permit, distinct from a human approval, valid only inside a host-local ceiling on the agent that the hub cannot widen. A hub permit is not a cryptographic proof of owner approval; owner-signed bundles come after v1. (The exact permit fields are a proposal.) | RW-11, RW-12 |
| **Q0.2** Policy precedence | **D-05, D-06** | The strictly more specific rule wins, **even over a broader Deny**; equal specificity: Deny wins. Hard limits (host ceiling, Destructive never Allow in v1, containment exclusions, unknown safety facts) are not rules and cannot be overridden. Host ids in v1, tags after. This replaces the earlier "explicit deny anywhere wins" by owner decision. | RW-10 |
| Destructive operations | **D-06** | Never policy-allowed in v1; a human can still approve with typed confirmation. | RW-10 |
| **Q0.3** L2 versus L3 | **D-04** | L3 ships in v1 as milestone M9 under a general Contain class, not only firewall. (The first set is decided by D-17, below.) | RW-13 |
| **Q0.4** What can run offline? | **D-03, D-04** | In v1, only what the host-local ceiling permits under a live hub permit. Offline emergency permits are DEFERRED past v1 (`docs/rework/06` "Deferred and after-v1"). | RW-12, RW-13 |
| Unattended targets | **D-07** | The operation's pinned subjects plus targets named by the matching owner-authored rule. | RW-23 |
| **Q1.1** First capability schema (ownership) | **D-02** | Hub-owned typed contracts; providers and agents advertise availability only; skills keep owner-manifest risk with a hub floor. | RW-07 |
| **Q1.2** How incidents resume | **D-02** | Operation aggregate distinct from Incident; coarse status plus typed immutable steps; one transactional unit of work; unknown outcomes reconciled through the agent's journal. | RW-03, RW-04, RW-05, RW-09, RW-30 |
| **Q1.4** Knowledge ontology (shape) | **D-02** | Typed facts and relations that reference inventory through typed refs; a small entities table for user-defined subjects only; a closed predicate enum; no numeric confidence. | RW-14 |
| Experience and search | **D-02** | The journal is the experience record (no separate experience event store). A resolution summary per operation, full-text search, a host-scoped `search_experience` read capability, similar past operations in the UI, provenance whenever experience is supplied to reasoning. Past approvals are evidence, not authorization. | RW-15, RW-25 |
| **Q1.6** Retention (configurability) | **D-09** | All retention is configurable; raw data and durable summaries are separate classes. Default values are decided by D-21, below. | RW-28 |
| Orchestration visualization | **D-08** | Step timeline and interactive graph both ship in v1, timeline first, on one frozen trace schema. | RW-21 |
| Presentation profiles | **D-10** | Detailed, Standard and Essential ship in v1. | RW-21 |
| Capability install from the UI | **D-11** | In v1: signed packages, agent verifies against locally trusted keys, agent opt-in `allow_install`, hub only relays, install never enables or authorizes. Third-party packages are decided by D-16, below (DEFERRED). | RW-26 |
| Motion | **D-12** | Longer durations allowed for visualization and transitions; reduced motion mandatory. | RW-21 |
| Users and roles | **D-13** | Single owner through v1; actor identity is designed so roles can follow. | RW-29 |
| Tamper evidence | **D-14** | Audit hash chain in v1, with an honest threat model; head hash in the daily digest as a cheap anchor. | RW-27 |
| `ARCHITECTURE.md` updates | **D-15** | Once per milestone. | none (process rule) |
| **Q0.2a** Tie-break for incomparable conflicting rules | **D-19** | Deny > RequireApproval > Allow. Publishing a policy version shows the conflicts and requires acknowledgement. | RW-10 |
| Policy Deny and locked Deny | **D-23, D-26** | A policy Deny is never offered for approval and has no bypass. A locked Deny (default off) is an owner-declared prohibition that no narrower rule overrides; it is shown distinctly in the editor, simulator, explanation and trace, and changing it needs step-up. | RW-10 |
| **Q0.3a** Exact first containment set | **D-17, D-22** | `firewall.block_source` only, with a kernel-enforced nftables timeout and a 30 s two-step confirmation. `container.pause` and `unit.stop_temporary` are DEFERRED: an agent timer is not reliable automatic expiry; reconsider in v1.x when the checklist in `docs/rework/04 §9.3` is met. Every containment expires independently of the main agent, and nothing is enabled unattended before the drills pass. | RW-13 |
| **Q0.5** How is management access protected during containment? | **D-28** | Resolved. Unattended containment on a host requires owner-confirmed, current management paths; unknown, stale or conflicting means refused; the agent enforces local exclusions independently; the UI shows paths, provenance, freshness and what is blocked. Monitoring and other human-approved work are unaffected. | RW-37 |
| **Q0.6** Trust model for external providers and third-party packages | **D-16** | First-party release key plus owner keys installed locally on the agent; the hub cannot add or replace keys; a detached signature over a canonical message, verified before extraction; a rollback needs a currently trusted key. Third-party registries are DEFERRED; hub-side integrations are DEFERRED (`docs/rework/06`). | RW-26 |
| Ceiling scope and honesty | **D-18, D-27** | The ceiling covers the exact capability id, explicit targets, rate, constrainable arguments, and for containment max TTL, max active and exclusions; no entry means no policy-authorized execution. `authorization_kind` is a hub label, and a compromised hub can label a request `human` and bypass the ceiling (accepted v1 limitation), but the agent's catalogue, manifest, argument, `allow_risk` and mutation rate-limiter checks apply to all mutating executions. Owner-signed approvals are DEFERRED. | RW-12, RW-38 |
| **Q1.1a** Agent support window | **D-20** | There is no support window. Exact protocol major.minor match, checked only after the agent has proven its identity: older and newer minors are rejected with the existing reason, same-protocol patch releases are compatible, and application release numbers are not a compatibility dimension. No legacy adapter. `Incompatible` is persisted on the agent record, survives a hub restart, keeps history, and an incompatible agent idles instead of reconnecting. Coordinated upgrade, hub first, then agents through the install procedure. Exactly one protocol bump before v1 (1.1 → 1.2 in M1); F3 changes no wire format. No agent self-update. A revoked-build list is DEFERRED and separate. | RW-17, RW-42 |
| **Q1.6** Default retention values | **D-21** | Signals 30 d; incident timeline 90 d; operation journal 180 d; raw execution outputs 30 d; operations and summaries retained; audit retained; retracted fact content purged after 30 d; conversations 180 d; notifications 30 d; 7 nightly and the last 3 pre-migrate backups. Deleted data stays in backups until they rotate. | RW-28 |
| Docker access | **D-24** | No general Docker socket write for the agent; a narrow first-party helper with its own ADR; Docker write capabilities unavailable for unattended use until then. | RW-39 |
| Agent privilege | **D-29** | The main agent runs as the dedicated unprivileged user `domovoi-agent`. Privileged host operations go only through one narrow, root-owned helper installed and updated by the owner-controlled OS path, never by capability packages. Root installs are labelled and get no unattended execution. | RW-39, RW-12, RW-26 |
| Graph playback | **D-25** | Ships last in U2; the timeline and history are fully useful without it. | RW-21 |

## Open owner question

| ID | Question | Recommended default | Blocks |
|---|---|---|---|
| **O-30** | What happens to skills whose manifest sets `run_as` to a user other than the agent user, now that the agent is unprivileged (D-29)? Options: (a) unsupported, reported `Unavailable(run_as_unsupported)`; (b) a helper operation that switches users for allowlisted pairs, which reintroduces a root path controlled by package content; (c) owner-configured sudoers rules, which gives the agent user sudo and undermines D-29. | (a) unsupported | H1 migration (skills impact) and package guidance |

## Priority 0 — resolve before implementing autonomous actions

None. The four questions that used to sit here (Q0.2a, Q0.3a, Q0.5, Q0.6) were answered by owner decisions D-19, D-17, D-28 and D-16 (listed above). What remains before autonomous actions are enabled is evidence, not decisions: the nftables and helper privilege spike (R-2) and the drills that owner decision D-22 requires.

## Priority 1 — resolve during detailed architecture design

| Question | Recommended default | Why it remains open |
|---|---|---|
| **Q1.1 What are the exact schema fields?** | Ownership is decided (hub-owned) and protocol compatibility is decided (D-20: exact match). Remaining: stable ID/version, typed input/output, prerequisites, constraints, timeout, verification, rollback/TTL. | Need the exact type system, error model and discovery refresh behavior (RW-07, RW-17). |
| **Q1.3 What is the incident/plan step budget?** | Configurable global and per-host defaults plus per-incident caps; conservative defaults; share budgets across cycles. | Exact numbers need measurement and adversarial drills; the total prompt cap needs real prompt sizes (R-5). |
| **Q1.4 Which predicates and relation types ship first?** | A small closed set for hosts, services, containers, storage, network, integrations and dependencies; extend through versioned definitions. | Shape is decided (typed refs, closed enum); the first list is still a proposal. |
| **Q1.5 How are inferred facts confirmed or disputed?** | Preserve each source claim; user-confirmed facts have explicit provenance; conflicts are surfaced as disputed, not overwritten; no numeric confidence. | Need merge policy, trust ordering, expiry and UI workflow. |
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

## Research and evidence still needed (not owner decisions)

These are tracked in [`docs/rework/99-open-decisions.md`](../rework/99-open-decisions.md) §C, where each names what it blocks and how it is resolved. They gate implementation prompts, not product direction.

| ID | Question | Needed before |
|---|---|---|
| R-1 | A graph library that works under the content policy, the no-inline-style rule and the bundle budget | U2 |
| R-2 | nftables containment on supported platforms: table, hook and packet path, coexistence with firewalld, ufw and Docker, set limits, TTL validation, atomic delete-and-add replacement; containment is not production-ready until verified | M9 |
| R-3 | Helper delivery on supported platforms: socket activation, peer credential check, sandboxing, containers, Unraid and no-systemd supervisors, Docker API access | H1 |
| R-4 | Discord deduplication behaviour (wording only; delivery stays at-least-once) | F2 delivery |
| R-5 | Real prompt sizes, to set the total prompt cap | F4 |
| R-6 | Cost of durable writes on slow storage | F1, M1 (informational) |
| R-7 | Container names validated against Docker's name grammar; all helper operations take ids (a test, no longer research) | M2 (RW-34) |
| R-8 | Effective-configuration identity of systemd units (fragment and drop-ins) across supported systemd versions | M8 `unit.restart` |

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
- Historical graph playback ships last in U2; the timeline and history are fully useful without it (owner decision D-25).
- A host whose agent speaks another protocol minor is shown as `Incompatible` with an upgrade-required explanation and its history kept, never as offline, also after a hub restart; after a hub upgrade a host that has not reconnected shows "expected incompatible" (owner decision D-20).
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
| What is the initial level of dashboard customization? | DEFERRED to v1.x (demand unvalidated, `docs/rework/06` "Deferred and after-v1"): v1 ships one excellent default layout. | Persisted layouts add implementation/state complexity; validate demand first. |
| How should UI profiles customize detail? | Global profile plus per-page overrides. Detailed remains reference and default for implementation. | Profiles ship in v1 (D-10); the exact persistence scope and defaults need UX tests. |
| Which global shortcuts are final? | Keep Ctrl/Cmd+K for search; use a conflict-checked shortcut for Operator, proposed Ctrl/Cmd+J. | Shortcut conflicts vary by browser/OS and must be user configurable. |
| What is the final navigation taxonomy? | Separate Operations, Incidents, Infrastructure, Capabilities, Knowledge and Policies. | Exact grouping should be validated with complete page inventory. |

## Decision rule

Do not block general architecture work on Priority 2 questions. Do not implement L2 execution before the policy evaluator, host ceiling and target binding exist, and do not enable L3 containment on any host before the owner has confirmed that host's management paths and they are covered by the agent-reported exclusions (D-28), and the drills required by D-22 pass. For Priority 1, specify only the parts required by the next vertical slice; keep the rest extensible.

---

# 10 — Capability and release roadmap

This roadmap is deliberately ambitious about the **capability universe** and conservative about what becomes autonomous. It is a product direction, not a date promise. Release labels describe quality and scope, not calendar deadlines. Milestone names (F1–F4, M1–M9, U1–U4, P1, H1) follow the implementation direction in [`docs/rework/06-implementation-roadmap.md`](../rework/06-implementation-roadmap.md) and [`07-implementation-prompt-index.md`](../rework/07-implementation-prompt-index.md); this document states the product outcome of each, and `docs/rework` holds the technical scope. Whether a milestone is ready for an implementation prompt is a separate question from whether its architecture is defined: see "Readiness" in [`docs/rework/06-implementation-roadmap.md`](../rework/06-implementation-roadmap.md).

## 1. Release strategy at a glance

**Owner decision D-01 (RW-02): there is no separate v0.1 release.** The earlier plan to stabilize the current product as a v0.1.0 release before the rework is withdrawn. Its quality requirements are kept, as foundation slices that come first inside the rework. The product on the branch stays usable after every slice, but nothing is tagged before the first rework release.

| Stage | Product outcome | Autonomy | Scope posture |
|---|---|---|---|
| **Foundation slices F1–F4** | The current product becomes trustworthy and restart-safe: atomic state and audit, recovery order, exact protocol rule, reliability and operational readiness. | Observe + human-approved changes only. | The former v0.1.0 quality bar, delivered as slices; no release tag. |
| **Milestones M1–M7 and the UI/package track** | Build the extensible foundations: execution boundary, capability contracts, operations, dynamic investigation, knowledge and experience, the Operator, policy in shadow, the Operations UI, graph, profiles and capability install. | Human approval remains default; policy runs in shadow/dry-run. | Incremental vertical slices; no autonomous authority until M8 gates pass. |
| **H1 Unprivileged agent and root-owned helper** | The agent runs as the unprivileged `domovoi-agent` user and privileged host operations go through one narrow root-owned helper, installed only by the owner-controlled OS path (owner decisions D-24, D-29; RW-39). M8 and M9 rely on it; the agent never gets a general Docker socket write. | None by itself. | Needs F1, not P1 (packages never install the helper). After research R-3, the helper ADR and the open owner question O-30; before M8's helper-backed contracts and M9. |
| **M8 and M9 (inside v1.0.0)** | Bounded L2 autonomy, then L3 containment, which in v1 is `firewall.block_source` only. | L2 for a reviewed operation set; L3 under a general Contain class. | Gated by drills and the host-local ceiling. |
| **v1.0.0** | A genuinely adaptive self-hosted operations agent that dynamically investigates, plans, executes explicitly authorized low-risk repairs and contains narrowly scoped threats, and verifies outcomes. | L2 and L3 as above; nothing Destructive is ever policy-allowed. | Complete end-to-end product for Linux + common homelab services, not every possible integration. |
| **Post-v1 (DEFERRED)** | The deferred items in section 5 (offline emergency permits, owner-signed bundles and approvals, host tags, multi-user and roles, stronger audit anchoring, third-party registries and more). | Grow L2/L3 by explicit capability and evidence, never by a global switch. | Each needs its own design; none is promised for v1. |
| **v1.x** | Expand domain coverage, integrations, operational learning, security response and automation quality. | As above. | Add useful providers based on demand and tested contracts. |
| **v2+ / later** | Multi-site, large-scale, advanced automation and broad ecosystem. | Optional advanced autonomy with organization-grade controls. | Defer heavy infrastructure until real scale or users justify it. |

**Important release rule:** v1.0.0 should not be declared merely because more screens and scripts exist; the adaptive investigation → policy → operation → verification loop must work end to end. Foundation work is never skipped to reach a visible feature sooner.

**Readiness rule.** An architecture being defined is not the same as a milestone being ready for an implementation prompt. A milestone is prompt-ready only when its architecture, owner decisions, research evidence and predecessor milestones are all in place. H1 still waits for the helper-delivery spike (R-3), its ADR and the open owner question O-30; M9 waits for the nftables spike (R-2); U2 waits for the graph-library spike (R-1). See "Readiness" in `docs/rework/06`, which holds the per-milestone readiness table.

## 2. Foundation slices — the quality bar of the former v0.1.0

These requirements used to define a separate v0.1.0 release. Under owner decision D-01 they are delivered as the first slices of the rework, in four groups. Each is also a prerequisite for something later, which is why they come first.

| Slice | Product outcome | Technical scope |
|---|---|---|
| **F1** | A state change, its journal entries and its audit record commit together or not at all; the audit chain exists; crash tests exist. | One transactional unit of work, hash-chained audit, atomic action and incident writes, crash harness (RW-03, RW-27). |
| **F2** | After a restart nothing stale runs and nothing waits forever: approvals are re-checked for freshness, hosts that have not reconnected are waited for, verification and triage resume, notifications are not lost. | Recovery order, durable timers, delivery durability (RW-09, RW-30). |
| **F3** | Hub and agent talk only on the exact same protocol major.minor. The check happens only after the agent has proven its identity; a mismatched agent is rejected with the existing reason, its host is shown as `Incompatible` (upgrade required, history kept, not offline) and the state survives a hub restart; after a hub upgrade a host that has not reconnected shows "expected incompatible". An incompatible agent idles instead of reconnecting. Same-protocol patch releases communicate; application release numbers are not a compatibility dimension. | Exact protocol version, durable authenticated report and idle agent (RW-17, owner decision D-20); no support window, no legacy adapter and no release floors. **F3 changes no wire format.** Agents released before F3 restart-loop under systemd until upgraded. A revoked-build list is a separate, DEFERRED mechanism. |
| **F4** | The model path is bounded and attributable, approvals show exactly what will run, and the hub can say whether it is ready. | LLM reliability, structured approval surfaces, operational readiness (RW-06, RW-22, RW-16). |

The detail below is the product quality requirement each slice must satisfy, kept from the former v0.1.0 definition.

### Required product capabilities

- Hub and agent install, enrollment, authentication, reconnect and useful host-down versus agent-down reporting.
- Linux host facts and a usable host overview.
- Built-in monitoring for system resources, disk/SMART, kernel errors, power, containers, systemd units, ports and the existing security checks.
- External alert ingestion (Alertmanager, Uptime Kuma and generic signed webhooks) and hub-side network probes.
- Signal correlation, incident lifecycle, acknowledgements, mute/maintenance windows, history and notifications.
- LLM triage with supported provider configurations, bounded read-only tools, budgets, provider failure fallback and redaction.
- Human-visible incidents, host/profile information, notes, skill catalogue and approval history in the web UI.
- Host-local skill manifests, typed args, argv-only execution, time/output limits, local validation, manifest/plan validation, verification and audit.
- Manual approval for write/destructive skills; destructive confirmation remains explicit in the UI.
- Daily digest, advisories, health/metrics and useful error reporting.

### Required operational quality (carried by the foundation slices)

- Reproducible CI, tests for critical transitions and documented supported deployment modes.
- A clean first-run path, clear configuration and secret setup, safe enrollment and recovery instructions.
- Document backup, restore, migration, upgrade, downgrade/rollback expectations and how to recover a lost hub.
- Test hub restart, agent restart, offline host, reconnect, duplicate signals, expired actions, failed skill and failed verification.
- Verify API schema and UI stay in sync; migrations are tested from an empty DB and the previous release DB.
- Define supported Linux distributions/architectures and explicitly document unsupported platforms.
- Audit and redact outputs; avoid logging tokens, secrets, skill args containing secrets or raw credentials.
- No known critical security defect; dependency and container release pipeline checks pass.
- Document action risk and the fact that current mutating actions require human approval.

### UI scope for the foundation slices

The current main-branch UI overhaul is the visual and usability baseline for the foundation slices. They prioritize correctness and completeness over introducing the entire v1 control plane:

- Finish and validate the current Overview, Incidents, Approvals, Hosts, Chat, Skills, Notes, Logs, Settings and onboarding flows.
- Keep the warm light/dark visual identity, semantic color-coded messages, accessibility, responsive layouts and documented component/design rules.
- Keep the current Ctrl/Cmd+K command palette and page/entity navigation foundation.
- Ensure every current mutating action has clear target/effect/approval/verification feedback and every page has truthful loading/empty/error/offline states.
- Add/retain browser-level tests for the real workflows, both themes and busy-data conditions.
- Do not block the foundation slices on the new orchestration canvas, full Operations model, capability install, policy simulator or full Operator; those arrive in later milestones, before v1.0.0.

### Small foundations that are worth including if they do not destabilize the slice

- A capability inventory view derived from current read tools and skills (even if internally still backed by the existing types).
- Stable capability IDs/categories as metadata for later migration.
- Clear distinction between **unknown**, **unavailable**, **failed** and **not applicable** in host profiles where current UI can support it.
- Collect capability/provider version and host suitability metadata during agent registration.

### Explicitly not part of the foundation slices

- Autonomous mutation or security containment (L2 is M8, L3 is M9).
- A universal workflow/playbook engine.
- A full knowledge graph or dedicated graph database.
- Vector search, embeddings or a separate vector database.
- Dynamic LLM-created capabilities, self-installing skills or generated code execution.
- Kubernetes, Windows/macOS, cloud orchestration or full router management.
- Multi-user role-based access control beyond the current intended deployment model (single owner through v1: owner decision D-13).

**Why:** the foundation slices prove that monitoring, triage, approval and a constrained execution primitive are dependable before increasing the intelligence or authority of the system.

## 3. Milestones — build the foundations for v1

These are not separate releases; order by dependency and deliver in usable slices. Each milestone is one or more reviewed pull requests, and `ARCHITECTURE.md` is updated once per milestone (owner decision D-15).

| Milestone | Product outcome | Sections below |
|---|---|---|
| **F1–F4** | Foundation slices (section 2). | 2 |
| **M1 Execution foundation** | No execution is duplicated or silently lost: idempotency, `OutcomeUnknown`, agent action journal, plan hash v2, one execution boundary. It carries the **one and only protocol bump before v1 (1.1 → 1.2, a coordinated fleet upgrade)**; later milestones use capabilities over the frozen 1.2 wire (RW-42). | 3.6 |
| **M2 Capability contracts** | One hub-owned registry the model, UI and executor share; available is not authorized. Starts after M1 (needs the catalogue call of wire 1.2). | 3.1 |
| **M3 Operation aggregate, journal and SSE v2** | Durable, resumable operations; triage becomes an Investigate operation with identical behaviour; live updates that replay after a gap. | 3.2 |
| **M4 Dynamic investigation** | Typed meta-tools, validated plans, layered context, no text-protocol proposals. | 3.2 |
| **M5 Knowledge and experience** | Typed facts and relations over inventory; resolution summaries, full-text search and similar past operations. | 3.3 |
| **M6 Operator** | Durable conversations as side panel and workspace over the same state. | 3.7 |
| **M7 Policy in shadow** | Versioned policy, pure evaluator, shadow decisions, historical simulator. | 3.5 |
| **UI track U1–U4, P1** | Operations list/detail with step timeline first (U1), interactive graph (U2, with historical playback last), presentation profiles (U3), policy, knowledge and capability pages (U4), capability packages and install from the UI (P1 backend; the install wizard is part of U4, after U3). All in v1. | 3.7 |
| **H1 Unprivileged agent and root-owned helper** | The agent runs unprivileged; one first-party root-owned helper with a root-owned allowlist and one ADR is the only privileged path and the prerequisite for Docker write capabilities and for containment (D-29, RW-39). | 3.6 |
| **M8 Bounded autonomy (L2)** | Policy permits inside the host-local ceiling, target binding, circuit breaker, pause switch, first reviewed L2 operations. | 3.6 |
| **M9 Containment (L3)** | A general Contain class whose first and only v1 capability is `firewall.block_source`: kernel-enforced expiry, a 30 s two-step confirmation, management-path protection and drills. | 3.9 |

### Milestone dependencies (mirror of `docs/rework/06`)

- H1 depends on F1, not on P1: the helper is installed by the OS install path, never by a package.
- M2 starts after M1, because it needs the catalogue call carried by wire 1.2.
- P1 is the package backend (after M1 and M2); the install wizard is part of U4, after U3.
- Approval chain: F4 binds approvals to the existing plan hash v1 (RW-11), M1 introduces hash v2, M3 moves approval to Operation steps.
- Protocol: F3 changes no wire format; M1 is the single 1.1 → 1.2 bump; M2 onward uses capabilities over that wire (RW-42).
- H1 precedes M8's helper-backed contracts and M9; M8 precedes M9.
- Per-milestone readiness for an implementation prompt is the "Readiness" table in `docs/rework/06`.

### 3.1 Capability platform

- Define capability descriptors: stable ID, version, provider, description, input/output schemas, target scope, effects/risk, prerequisites, supported platforms, timeout, rate limit, idempotency, verification, rollback/expiry and policy hooks.
- Define the contracts in the hub (typed, versioned, hub-owned effect class and risk); wrap existing native read tools and skills as the first implementations, with agents advertising availability only (RW-07).
- Add provider discovery and capability catalogue snapshots; version compatibility and deprecation rules.
- Let the reasoner request capabilities dynamically from a bounded catalogue, not a hardcoded workflow list.
- Keep execution behind provider adapters and the deterministic policy/dispatch path.
- Support native operations, local scripts, service APIs, router APIs and remote provider capabilities as distinct provider types.
- Add provider health, availability state and per-capability last-seen status.

### 3.2 Durable incident orchestration

- Introduce persisted phases and cycles: Detect → Enrich → Assess → Plan → Authorize/Route → Execute → Verify/Monitor → Resolve/Escalate.
- Add typed, validated and budgeted evidence requests. The model can ask for relevant facts; the engine decides whether and how to collect them.
- Track evidence freshness and status: available, unknown, unavailable, failed, stale, not applicable.
- Save structured assessments, hypotheses, supporting/contradicting evidence and uncertainty.
- Allow plans to mix investigation, notification, user questions, human approvals, autonomous steps, monitoring and escalation.
- Resume an incident after hub restart; reconcile uncertain in-flight operations before any continuation.
- Support multiple reasoning/execution cycles, bounded retries of the overall investigation (not blind command retries), idempotency and cancellation.
- Use layered context; give the model summaries and evidence references, with on-demand details and hard budgets.

### 3.3 Knowledge and experience

- Keep existing Markdown notes as editable runbooks and add entity links.
- Add typed facts and relations over existing inventory, using typed subject refs and a small entities table only for user-defined subjects (RW-14), with provenance, status, scope, freshness, validity and supersession. There is no numeric confidence.
- Start entities with hosts, services, containers, storage, networks, routers, volumes, integrations, capabilities and users/owners.
- Add relationship types such as runs_on, depends_on, exposes, stores_data_on, reachable_through, protected_by, backed_up_to and managed_by.
- Make the journal the experience record: typed events for phase transitions, evidence requests, hypotheses, plans, policy outcomes, action attempts, verification, rollbacks and user decisions, with a resolution summary per finished operation, full-text search, a host-scoped `search_experience` read capability and similar past operations in the UI (RW-15, RW-25).
- Build entity detail pages, knowledge search/edit, provenance/conflict views and incident experience views.
- Keep knowledge, policy and experience as separate data models and APIs.
- Add deletion/correction/supersession semantics that also clean FTS, caches and future vector indexes.

### 3.4 Onboarding and environment model

- Hub onboarding: environment purpose, trust boundaries, criticality tiers, notification preferences, remote management access, VPN/trusted ranges and global constraints.
- Agent onboarding: host role, criticality, recovery route, storage, backups, important services, external exposure and maintenance windows.
- Discover first; ask only for high-value unknowns. Never block basic monitoring on unanswered optional questions.
- Support answers of unknown/not sure and record who confirmed each fact.
- Provide per-host profiles and reusable environment templates without silently granting autonomy.

### 3.5 Policy and autonomy foundations

- Introduce a deterministic policy evaluator and a human-readable explanation for every allow/deny/approval-required outcome.
- Rules scoped by host, capability and target. **The strictly more specific matching rule wins, even over a broader Deny; at equal specificity Deny wins** (owner decision D-05, RW-10). This replaces the earlier "explicit deny wins" principle. Incomparable conflicts resolve Deny > RequireApproval > Allow, and publication requires acknowledging them (owner decision D-19). An optional locked Deny (default off) cannot be overridden by any narrower rule (D-26), and a policy Deny is never offered for approval (D-23). Host ids only in v1; host tags are DEFERRED.
- Hard limits are not rules and cannot be overridden: the host-local ceiling, Destructive never policy-allowed in v1 (owner decision D-06), containment exclusions, and unknown safety facts never yielding Allow.
- Model target constraints, source conditions, time windows, operation limits, TTL, rate limits, blast radius and required recovery path.
- Separate human approval from autonomous policy authorization.
- Provide shadow mode to show what would have happened without executing.
- Add policy test fixtures, simulation and an autonomy UI showing inherited versus explicit decisions.
- Make policy versioned and revocable. Local verification in v1 is the host-local unattended ceiling that the agent enforces and the hub cannot widen (owner decision D-03); owner-signed policy bundles are DEFERRED past v1.

### 3.6 Execution foundation (M1), the helper (H1) and first safe automation (M8)

- F4 (current action flow): approvals bind to the existing plan hash v1 (manifest, host, arguments); it does not bind entry-script bytes or target identity (RW-11).
- M1: idempotency keys, an agent action journal, `OutcomeUnknown` for lost results, plan hash v2 (binding the entry-script bytes, resolved target identity and execution id; pre-upgrade actions keep v1 and are labelled), one execution boundary with an enforced rule that nothing else dispatches, and the single protocol bump 1.1 → 1.2 (RW-42). M3 then moves approval to Operation steps (RW-11).
- M8: choose reversible low-impact operations with reliable postconditions (for example, restart one known service/container, reload a specific config, renew a certificate through a supported provider, unload an unused inference model).
- M8: policy permits, target revalidation and binding to the operation's pinned subjects, concurrency locks, operation budgets, postcondition checks, a circuit breaker and a human-only pause switch.
- Add rollback or automatic expiry wherever possible.
- Use per-host opt-in through the host-local ceiling; default to human approval; Destructive operations are never policy-allowed.
- H1: the agent runs as the unprivileged `domovoi-agent` user; one root-owned helper (socket-activated, peer-uid checked, typed operation groups, a root-owned allowlist) is the only privileged path. It is installed and updated only by the owner-controlled OS install path; capability packages and skills can never install or modify it, and packages run as the agent user, never root (owner decision D-29, RW-26, RW-39). One ADR, after the helper-delivery spike R-3; the nftables group also needs R-2. Root installs are labelled in the UI and get no unattended execution. Skills whose manifest names another `run_as` user are an open owner question (O-30; recommended: unsupported).
- The agent gets no general Docker socket write access, which would be root on the host. Docker write capabilities, including `container.restart`, stay unavailable for unattended use until the helper and its ADR exist (owner decision D-24); `unit.restart` also goes through the helper (`systemd.write`). Unit targets are bound by canonical id plus a hash of the fragment and drop-ins; generated and transient units are not valid mutation targets, and files a unit references are not bound (RW-34).
- Every mutating execution, human- or policy-labelled, is bounded by the agent's mutation rate limiter (owner decision D-27); the host ceiling covers the exact capability, targets, rate, constrainable arguments and, for containment, TTL, maximum active and exclusions (D-18).
- Run drills before granting autonomy to real hosts.

### 3.7 UI and interaction foundations

- Introduce the target UI vocabulary and canonical object routes for Incident, Operation, Host, Capability, Knowledge, Policy and Integration.
- Build the shared operation activity/event presentation model so graphs, timelines and chat can render the same durable state.
- Add the global Operator side panel and full workspace over the same conversation and operation state; do not create a chat-specific executor.
- Build the Operations list/detail view with a step timeline first (U1), on a trace schema frozen with golden fixtures. The interactive orchestration graph follows on the same schema (U2); **both ship in v1** (owner decision D-08). Historical playback is the last U2 deliverable (owner decision D-25); the timeline and history are fully useful without it. The accessible outline stays as the non-canvas representation.
- Add the capability catalog/installed inventory and the install flow through the UI (P1): signed packages, verified by the agent against locally trusted keys, with the agent opted in; the hub only relays; install never enables or authorizes (owner decision D-11). Trusted keys are the first-party release key and keys the owner installed locally on the agent; third-party registries are DEFERRED (owner decision D-16).
- Add structured Knowledge browser/detail and initial provenance editing.
- Add granular policy editor and read-only policy simulator, including historical scenario reconstruction.
- Add the detailed/standard/essential presentation profiles (U3; owner decision D-10). Safety, approval and audit content appears in every profile. Customizable Control Room widgets are DEFERRED to v1.x (demand unvalidated; `docs/rework/06` "Deferred and after-v1").
- Allow longer motion for graph and layout transitions while keeping the short micro-interaction tokens; reduced motion is mandatory (owner decision D-12).
- Add a page design-spec and interaction/motion quality gate to implementation planning.
- Show a host whose agent speaks another protocol minor as `Incompatible`, with an upgrade-required explanation, the exact required protocol and the agent version, history kept and never shown as offline; after a hub upgrade, hosts that have not reconnected show "expected incompatible" (owner decision D-20, RW-17).
- Policy editor: locked-Deny styling and a publication validation report listing incomparable conflicts that need acknowledgement (D-19, D-26). Host pages: management-path confirmation with provenance, freshness and which containment is blocked (D-28). Approval cards show an agent-side rate-limit refusal (D-27) and, during the migration, whether an action carries v1 or v2 plan binding (RW-11). Host pages show a warning when the agent runs as root, "helper unavailable" on affected checks and capabilities (RW-39), and the agent-reported exclusions beside the confirmed management paths (RW-37). The Capability Center includes owner package import (RW-26). The audit page shows legacy and chained history separately (D-14).

### 3.8 First provider expansion

- Improve Linux host and systemd provider coverage.
- Docker and Docker Compose lifecycle, logs, stats, inspect and controlled stack operations.
- Network diagnostics: DNS, gateway, TCP/HTTP, TLS expiry, routes and listeners.
- Backup systems and storage visibility (read/verify first; no broad delete operations).
- Optional read-first providers for reverse proxies, VPNs, monitoring systems and router/firewall APIs.

### 3.9 Containment — L3 (M9, inside v1.0.0)

- **Owner decision D-04:** L3 ships in v1 as milestone M9, under a general **Contain** class whose first and only v1 capability is `firewall.block_source` (owner decision D-17). Prerequisites: M8 complete (permits, ceiling, target pinning, journal, verification, circuit breaker), H1 (the helper's `nft` group), the nftables spike R-2, owner-confirmed management-path facts (D-28) and the drill suite. Refreshing a block's TTL is one atomic delete-and-add in a single nftables transaction, never an `add` that is assumed to refresh; containment is not production-ready until R-2 is complete (04 §9.1).
- **First and only v1 capability (owner decisions D-17, D-22):** `firewall.block_source`, an nftables set element with a kernel-enforced timeout and a 30 s two-step confirmation: the first element lives 30 s and is extended to the full TTL only when the hub confirms, so an unconfirmed block expires by itself.
- **DEFERRED:** `container.pause` and `unit.stop_temporary` (an agent timer is not reliable automatic expiry; reconsider in v1.x when the checklist in `docs/rework/04 §9.3` is met) and the other containment candidates (container network isolation, SSH session termination, account lock). See `docs/rework/06` "Deferred and after-v1".
- **Triggering:** a deterministic detector's typed signal whose structured fields name the target, and that target is the operation's pinned subject. Free-form model output never triggers or targets automatic containment; model recommendations go through human approval.
- Every containment has an expiry, mandatory management-path exclusions, a cap on simultaneous containments, verification that management access still works, and outcome-unknown handling. Unattended containment on a host also requires owner-confirmed, current management paths that are covered by the exclusions the agent itself reports (hash and summary); a mismatch, or any network or exclusion change, blocks unattended containment while monitoring is unaffected (owner decision D-28, RW-37). Offline emergency permits are DEFERRED (not in v1).

**Why this stage:** it establishes the common contracts, durable memory and policy machinery that make new capabilities additive instead of one-off LLM prompts and scripts.

## 4. v1.0.0 — target adaptive operator

v1.0.0 should ship when the end-to-end target loop is complete and production-safe for the supported platform set.

### v1.0 product requirements

1. **Discoverable capabilities:** agents and integrations advertise typed, versioned capabilities with availability, prerequisites, effects and safety metadata.
2. **Dynamic investigation:** the reasoner can ask for evidence beyond the initial context; the engine validates, budgets, executes and returns it.
3. **Durable orchestration:** phase/cycle state, requested evidence, plan steps, deadlines and outcomes survive restart and disconnection.
4. **Adaptive plans:** the model composes plans from available capabilities and can choose monitor, ask, escalate, human approval or autonomous execution.
5. **Deterministic authorization:** every mutation gets a policy decision independent of LLM reasoning; human and autonomous authority are distinct.
6. **Granular autonomy:** per host/category/capability/operation, with most-specific-rule-wins precedence (a narrower rule overrides a broader one, including a broader Deny; equal specificity: Deny wins), hard limits no rule can override, constraints, rate limits, expiry and transparent explanation.
7. **L2 operation set:** at least a practical set of low-risk, reversible operations can execute automatically on explicitly opted-in hosts.
8. **L3 security response (M9):** the Contain class and its first capability, `firewall.block_source`, ship in v1 (owner decisions D-04, D-17). A containment capability is enabled on a host only when owner-confirmed, current management paths (owner decision D-28), TTL, exclusions, enforcement-point verification and recovery drills all pass; until then the framework ships with that capability disabled on that host.
9. **Knowledge base:** persistent editable environment knowledge with provenance, relationships, freshness, conflict handling and onboarding.
10. **Experience model:** queryable record of investigations, plans, decisions, outcomes and verification; failed attempts inform re-planning.
11. **User control:** UI for capability availability, knowledge editing, policy configuration, autonomy status, incident timeline and action explanation.
12. **Operational resilience:** hub/agent restart, offline behavior, duplicate messages, uncertain action outcomes and rollback/expiry are tested.
13. **Safe extensibility:** new providers/capabilities can be added without adding special-case logic to the reasoner or bypassing the policy boundary.

### Initial supported capability set for v1.0

Prioritize a reliable built-in set rather than a large shallow integration catalogue:

- Linux: host inventory, CPU/memory/disk, processes, systemd, journal, filesystem/mounts, ports, network and package/update status.
- Docker/Compose: container and stack inventory, logs, events, health, resource usage, controlled restart/recreate actions with constraints and verification.
- Storage: disk/SMART, mount and filesystem state, available capacity, RAID/ZFS/SnapRAID/MergerFS awareness where provider support exists, backup job status and restore-test reporting.
- Network: DNS/TCP/HTTP/TLS probes, route/gateway and listener discovery; one chosen firewall provider for narrowly scoped protective action.
- Common services: reverse proxy/certificate status, database health as read-only initially, backup service status, VPN reachability.
- Notifications and external signals: webhook sources, alert correlation, chat and web UI.
- Local AI workloads: GPU/VRAM and inference endpoint health, loaded model state and safe unload/restart where supported.

### v1.0 UI commitments

- Operations is a first-class area separate from Incidents; user-requested and scheduled work appears even when no incident exists.
- The Operator works both as a global side panel and full workspace; both share conversations, context and operation state.
- Operation detail has an interactive orchestration visualization showing real phases, branches/cycles, capability calls, policy evaluations, execution, verification, waits and failures. A list/timeline alternative remains available.
- Users can inspect node inputs/outputs, evidence, source, timestamps, duration, errors, policy matches, model/provider usage and results, with secrets redacted.
- The Capability Center supports discovering, installing, enabling, configuring, updating and uninstalling supported capabilities on agents through a guided UI flow. Installation, enabled state and authorization remain separate.
- Policy management supports granular host/capability/operation permissions and historical/current/proposed simulation using the live evaluator without side effects. The host-local unattended ceiling is shown read-only.
- Knowledge UI supports entities, facts, relationships, provenance, freshness, notes and suggestions; permission/authority remains separate.
- Detailed is the design baseline; Standard and Essential (and Custom) presentation profiles ship in v1 and do not remove audit/authority data or change behavior.
- Operation detail offers the step timeline and the interactive graph on one shared trace schema (timeline first); both are v1.
- Major pages have approved page specifications covering P0–P3 priority, space class, content containment, interactions, live states, motion, responsive behavior and acceptance criteria.
- Cross-page workflows are browser-tested: incident → operation → capability → policy result; Operator → operation; capability install → enable → authorize; historical scenario → policy simulation → apply draft.

### v1.0 acceptance gates

- A novel incident can be investigated without a prewritten playbook, using only discoverable capabilities.
- Missing evidence is represented honestly; failed/unavailable sources are not interpreted as healthy.
- A plan cannot execute an operation that is absent, stale, out of scope or not authorized.
- A policy decision cannot be overridden by the LLM, a playbook, a learned preference or a historical success; only a strictly more specific owner-authored rule changes the outcome, a locked Deny cannot be overridden by any narrower rule, and hard limits cannot be changed at all.
- A failed verification starts a new reasoning cycle with the failed attempt in context; it does not repeat the same action blindly.
- A hub restart or agent reconnect cannot duplicate a non-idempotent action.
- In v1, nothing runs unattended without a live hub permit inside the host-local ceiling; offline emergency permits are not part of v1.
- A Destructive operation is never policy-allowed; the evaluator refuses it however specific the rule.
- An unattended step cannot target anything outside the operation's pinned subjects and the targets its rule names.
- A past approval, past success or retrieved experience never satisfies a policy condition.
- Every operation has a traceable proposal/authorization/dispatch/result/verification sequence.
- Security containment cannot block configured management paths; if the management paths are unknown, stale or conflicting, unattended containment is refused (owner decision D-28).
- Hub and agent communicate only on the exact same protocol major.minor; the check follows identity verification, an older or newer agent is rejected with a reason, and its host is shown as `Incompatible` with history kept, also after a hub restart; the agent idles instead of reconnecting, and no release-number floor exists (owner decision D-20, RW-17, RW-42).
- The agent runs as an unprivileged user on a fresh install; privileged operations go only through the root-owned helper, which no package or skill can install or modify (owner decision D-29, RW-39). A host still running the agent as root is labelled and gets no unattended execution.
- Every mutating execution, human- or policy-labelled, is bounded by the agent's mutation rate limiter, and a refusal is shown with its reason (owner decision D-27).
- Notification delivery is at-least-once: duplicates after a crash window are possible and nothing claims exactly-once.
- The agent upgrade procedure (hub first, then agents; no agent self-update) is documented and tested.
- The audit chain verifies at startup, and the head hash appears in the daily digest; legacy unchained history and chained history are verified and reported separately.
- Policy changes, knowledge changes and experience records are visibly distinct.
- Backup/restore and data migration tests pass for all persistent stores.
- A defined suite of realistic positive and negative incident drills passes.

## 5. Post-v1 capability expansion

The following are high-value directions, not v1.0 blockers. Prioritize using real incidents and user demand.

### DEFERRED and after-v1 (mirrors `docs/rework/06` "Deferred and after-v1")

None of these is a v1 blocker or a v1 promise; each needs its own design. `docs/rework/06` holds the conditions, the architecture's room for each, and the impact on protocol, migrations, UI and retention.

| DEFERRED item | Why not v1 | Reconsider |
|---|---|---|
| Owner-signed approvals and policy bundles (D-03) | Needs owner key custody, device signing, rotation and recovery UX; the v1 ceiling is a label-based limit | v1.x |
| Offline emergency permits (D-04) | Clock, replay, revocation and lockout risks; needs signed bundles first | After signed bundles |
| Third-party package registries (D-16) | Anyone publishing a package could run code as the agent after one click | v2 at earliest |
| Agent self-update (D-20) | Violates the rule that the hub cannot push code to a host; upgrade is the install procedure | Not planned |
| `container.pause` and `unit.stop_temporary` containment (D-17) | Their expiry would depend on the agent | v1.x, once the `docs/rework/04 §9.3` checklist is met |
| Other containment candidates (network isolation, SSH session termination, account lock) | Weaker recovery or lockout stories | v1.x+ |
| Protocol support for older agent generations (D-20) | Owner decision: exact match only | Not planned |
| Revoked-build list (security, distinct from protocol) | No current need | If a vulnerable agent build ships |
| Host tags in policy (D-05) | v1 uses host ids | v1.x |
| Multi-user and roles (D-13) | Single owner in v1; actor identity is already designed for it | v2 |
| Advanced audit anchoring (D-14) | v1 anchors with the head hash in the daily digest only | v1.x |
| Customizable Control Room widgets | Demand unvalidated | v1.x |
| Integrations and hub-side providers (router, hypervisor, NAS, cloud) | Each needs hub-held credentials and its own contracts | v1.x+ by demand |
| Vector search, graph database | Not needed at target scale | Only with a measured need |

Historical graph playback (D-25) is **not** deferred past v1: it ships last in U2.

### v1.x — deepen core operations

- More Linux distributions and package managers.
- Podman, containerd, Kubernetes and lightweight k3s providers.
- Proxmox/Libvirt and NAS appliance integrations.
- Router/firewall integrations for OPNsense, pfSense, MikroTik, UniFi and OpenWrt.
- WireGuard/Tailscale status and controlled peer/key operations.
- DNS provider and reverse-proxy integrations for safe record/certificate/config operations.
- Backup systems (Restic, Borg, Kopia, PBS, rsync) with backup verification and restore workflows.
- Storage ecosystems (ZFS, Btrfs, mdraid, SnapRAID, MergerFS, LVM) with deeper scrub/snapshot/repair guidance.
- Application-aware profiles: Plex/Jellyfin, Arr stack, qBittorrent, Minecraft, game servers, databases, Git services, Home Assistant and common self-hosted apps.
- Maintenance windows, controlled updates, staged restarts and dependency-aware maintenance plans.
- More checks: SMART trends, pool degradation, certificate expiry, DNS drift, backup freshness, service dependency failures, container image drift and resource saturation.
- Experience search, recurring-incident analysis and user-approved knowledge suggestions.

### v1.x — operational intelligence

- Multi-host dependency and blast-radius reasoning.
- Change correlation: deployments, updates, config changes, DNS/firewall changes, host reboot and backup windows.
- Baselines for normal resource use, process/service behavior and network exposure.
- Trend forecasting for disk exhaustion, memory pressure, certificate expiry and capacity planning.
- Flap detection and adaptive check intervals with deterministic bounds.
- Incident clustering across hosts and shared dependencies.
- Planned maintenance impact analysis.
- Cost/usage tracking for LLM providers and local inference resources.
- Human feedback capture: correct cause, mark evidence irrelevant, approve/deny reason, confirm proposed fact.
- Improvement suggestions for missing checks, read capabilities and recurring remediations.

### v2+ — broader infrastructure and organization scale

- Multi-site hub federation or regional hubs.
- Multiple users, roles, teams, approval quorum and delegated ownership.
- Windows and macOS agents; BSD support if demanded.
- Cloud provider inventories and operations (AWS/Azure/GCP and smaller providers).
- Kubernetes cluster operations, node maintenance and workload autoscaling controls.
- Hypervisor, IPMI/Redfish and out-of-band management operations.
- Hardware/network-controller integrations and topology discovery.
- Multi-tenant isolation, SSO/OIDC/SAML, fine-grained RBAC and audit export.
- Policy-as-code, signed organization policy bundles and staged rollout.
- External provider SDK, plugin marketplace or signed capability packages.
- Optional remote execution relays for segmented networks.
- Advanced graph algorithms and dedicated graph backend if scale warrants it.
- Optional local embeddings/vector retrieval if measured semantic search gaps remain after lexical + graph retrieval.
- Advanced anomaly detection and user-trained environment baselines.
- Multi-agent collaboration with explicit task delegation and shared evidence contracts.
- Disaster recovery orchestration and tested failover workflows.

## 6. Capability investment priorities

A capability should be prioritized by user value, safety, frequency and provider readiness—not by how impressive it sounds.

| Priority | Domains | Rationale |
|---|---|---|
| **P0 — foundation** | Linux inventory, systemd, Docker/Compose, storage and network diagnostics, logs, incident handling, policy, verification | Covers the majority of single-server/homelab failures and provides reusable primitives. |
| **P1 — v1/v1.x high value** | Backups, router/firewall, VPN, reverse proxy/TLS, NAS/storage stacks, databases, media and local AI services | Extends operations to the infrastructure relationships that cause real incidents. |
| **P2 — later** | Kubernetes, hypervisors, broader cloud, Windows/macOS, advanced dev platforms, Home Assistant and niche appliances | Valuable, but increases provider and test matrix size. |
| **P3 — research/scale** | Multi-site federation, multi-tenant marketplace, graph algorithms, vector search, advanced multi-agent coordination | Adds operational complexity; build only when concrete usage demands it. |

## 7. Safety classification for roadmap features

- **Read:** observe, inspect, compare, test, query. Eligible for automatic use within read budgets and privacy scope.
- **Low-risk write:** restart/reload one known target, unload a model, renew a certificate through a validated provider. Eligible for L2 only with explicit per-operation policy and verification.
- **Protective/security (Contain):** temporary block (v1: `firewall.block_source` only; pause and isolation are DEFERRED), restrict exposure. L3 (M9) only with dedicated conditions, trusted-path protection, TTL, rate limits and recovery.
- **High-impact write:** upgrade, deploy, reconfigure network, rotate credentials, change storage topology. Human approval by default; may require staged plans and rollback.
- **Destructive:** delete data, format, prune, wipe, destroy volume, remove backup. Never policy-allowed in v1 (a hard limit in the evaluator; owner decision D-06). A human may still approve one with typed confirmation.

Risk is contextual. Restarting a disposable container is not the same as restarting a database during a migration. Capability metadata and policy must consider target, dependencies, environment state and recovery—not only a static label.

## 8. Delivery rules

- Ship capabilities as end-to-end slices: discover → explain → authorize → execute → verify → audit.
- Implement read-only observation before mutation for every new provider.
- Add capability-specific tests and at least one incident drill before enabling it in autonomy.
- Do not expose a capability for autonomy merely because a script exists or because the model has used it successfully with approval.
- Track operation idempotency, concurrency, timeout, cancellation, partial failure, uncertain result and compensation.
- Prefer an integration with a stable API over shelling out to fragile CLI parsing; keep CLI providers typed and bounded when no API exists.
- Do not add a database, broker, workflow engine or embedding service without a measured requirement.

---

# Roadmap moved

The previous staged evolution roadmap is superseded by [`10-capability-roadmap.md`](10-capability-roadmap.md). Use that file as the only release/capability roadmap. The superseded version is not retained to avoid conflicting milestone guidance.

---

# 11 — Capability catalogue and opportunity map

This is an intentionally broad inventory of things Domovoi could eventually observe, diagnose, explain or operate. It is not a v1.0 commitment. A listed item becomes a real capability only when there is a hub-owned typed contract, an implementation, clear scope, policy, verification and tests.

**Priority shorthand:** `v0.1` = exists in the current product (there is no separate v0.1 release; owner decision D-01); `v1` = initial adaptive operator; `v1.x` = useful expansion; `later` = scale/niche/research. `R` = read, `W` = bounded write, `S` = security/protective (the **Contain** effect class), `D` = destructive/high-impact. A risk label is indicative; actual policy must consider context, and the hub, not a provider, owns the effect class (RW-07).

## How to read this catalogue after the rework

### The v1 first set

Most `v1` rows are read capabilities and are part of the initial supported set in `10`. The **mutating and containing** capabilities that ship in v1 are deliberately few. Marked below with **first set**; everything else that mutates or contains is `v1.x` or later.

| Capability | Class | Milestone | Status |
|---|---|---|---|
| Restart one named container (not recreate) | W | M8, L2 | First set. Goes through the root-owned first-party helper's `docker.write` group (H1, owner decisions D-24, D-29); unavailable for unattended use until the helper and its ADR land. |
| Restart one named systemd unit | W | M8, L2 | First set. |
| Temporary inbound block of one external address or small prefix (`firewall.block_source`, kernel-enforced timeout, 30 s two-step confirmation) | S | M9, L3 | **First set, and the only v1 containment capability** (owner decisions D-17, D-22). |
| Pause one named container with a time limit (`container.pause`) | S | v1.x at earliest | **DEFERRED candidate.** An agent timer is not reliable automatic expiry. Needs the `docs/rework/04 §9.3` checklist and the helper ADR. |
| Stop one named systemd unit with a time limit (`unit.stop_temporary`) | S | v1.x at earliest | **DEFERRED candidate.** Same reason. Needs the `docs/rework/04 §9.3` checklist and a protected-unit list. |

`container.pause`, `unit.stop_temporary` and the other containment candidates (container network isolation, SSH session termination, account lock) are DEFERRED: weaker recovery or lockout stories, or an expiry that would depend on the agent (`docs/rework/06` "Deferred and after-v1"; `docs/rework/04 §9.3`). Unattended containment on a host also requires owner-confirmed, current management paths (owner decision D-28). Automatic containment is triggered only by a deterministic detector's typed signal naming a target that is the operation's pinned subject, never by free-form model output (owner decision D-04, RW-13). Destructive entries are never policy-allowed in v1 (owner decision D-06); a human can still approve one with typed confirmation.

### Entries flagged unsafe until a prerequisite lands

The repository investigation ([`docs/rework/investigations/D-protocol.md`](../rework/investigations/D-protocol.md)) judged the following entries unsafe or contradictory with current invariants. Each is marked `[FLAG]` in its row. They stay in the catalogue as ideas; none may be built before its prerequisite.

| Flag | Entries | Why | Prerequisite |
|---|---|---|---|
| **Self-upgrade** | Agent and hub upgrade/restart; self-upgrade plan. | Pushes executable code to a host, contradicting the invariant that the hub cannot push scripts (INV-3) and that the hub never executes commands; restarting the agent mid-action destroys the channel its result travels on. Signed capability packages (owner decision D-11) are for capabilities, not for upgrading the agent or hub. | DEFERRED, not planned (owner decision D-20): upgrades follow the install procedure, hub first and then agents, and a mismatched agent shows as `Incompatible`. Changing the no-push rule would need its own ADR. |
| **Lost result** | Host reboot/shutdown, update-then-reboot, power cycle, IPMI/PDU control, UPS-driven shutdown, cloud instance reboot, reboot coordination. | The connection drops mid-action by design. | `OutcomeUnknown` handling and boot-id based reconciliation (RW-09), plus a dependency model that knows what powers the hub. |
| **Docker socket write** | Pull/redeploy/rollback, container stop, pause, network disconnect, container isolation, restart/recreate. | Write access to the Docker socket is root on the host, so the existing read-only rationale and the automatic default no longer hold. | The narrow root-owned first-party helper with its own ADR (owner decisions D-24, D-29, RW-39); the agent runs unprivileged and gets no general Docker socket write. Docker write capabilities stay unavailable for unattended use until then. |
| **Secret handling** | Rotate an API key, certificate or credential; disable an account; revoke a token; rotate VPN keys. | No secret store or redaction design exists for values that must be generated, delivered and replaced; secret handling contradicts the redaction invariant until one does. | A secret-store design (not planned for v1). |

The investigation also flagged file and volume destruction (never a typed-path capability without a path-scope model), third-party deployment and smart-home entries (outside the overseer's trust envelope until third-party trust is decided), message retry and failover (need real idempotency keys), and hub-side router/hypervisor/cloud providers (a different security design that holds third-party credentials). Treat those as non-goals for v1.

## 1. Generic Linux host operations

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Host identity, OS, kernel, architecture, uptime, boot ID, CPU/memory inventory | R | v0.1 | Foundation for profiles and platform compatibility. |
| CPU, load, memory, swap, pressure stall information, process count | R | v0.1/v1 | Time-series and trend context later. |
| Process list, resource-heavy processes, process tree, open files/sockets | R | v0.1/v1 | Sensitive command-line arguments must be redacted. |
| Systemd unit list/status/show/dependencies/timers | R | v0.1/v1 | Include enabled, active, failed and restart history. |
| Journal and application logs; bounded query by unit/time/severity | R | v0.1/v1 | Prefer structured log providers; strict size/time limits. |
| Filesystem usage, inode usage, largest directories, mount table, mount options | R | v0.1/v1 | Directory scans must have path allowlists, depth and time caps. |
| Kernel messages, OOM events, hardware errors, reboot-required state | R | v0.1 | Current checks provide a base. |
| Network interfaces, addresses, routes, neighbors, DNS config and resolver status | R | v1 | Do not expose secrets from network config. |
| Listening sockets, connection counts, firewall state and packet counters | R | v0.1/v1 | Firewall changes are separate privileged capabilities. |
| Package inventory, repository status, pending updates, held packages | R | v1 | Cross-distribution providers. |
| Controlled package update, rollback and reboot orchestration | W | v1.x | Approval by default; maintenance windows and snapshot/backup prerequisites. **[FLAG: lost result]** the reboot step needs `OutcomeUnknown` handling. |
| Time sync, NTP offset, clock drift, timezone | R/W | v1 | Time drift affects TLS and offline authorization. |
| Users, groups, sudoers summary, login sessions, SSH config summary | R | v1 | Never return password hashes or private keys. |
| Host reboot/shutdown | W | v1.x | Human approval; recovery path and dependency impact mandatory. **[FLAG: lost result]** the link drops mid-action; needs `OutcomeUnknown` and boot-id reconciliation first. |
| Agent self-diagnostics, version, config validation, queue/spool state | R | v0.1/v1 | Domovoi must be able to diagnose itself. |
| Agent upgrade/restart and hub upgrade/restart | W | later | **[FLAG: self-upgrade]** contradicts INV-3 (the hub cannot push scripts) and destroys the result channel. DEFERRED, not planned (owner decision D-20): upgrades follow the install procedure, hub first, then agents. Not part of the capability-package install path. |

## 2. Service and workload management

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Discover services and relationships (systemd, container, supervisor) | R | v0.1/v1 | Feed the entity graph. |
| Restart one named systemd unit | W | v1 | **First set (M8, L2)** only for opted-in, low-impact units; dependency-aware. |
| Stop one named systemd unit temporarily (time limit, management units excluded) | S | v1.x | **DEFERRED candidate** (`unit.stop_temporary`): its only expiry would be an agent timer. See the `docs/rework/04 §9.3` checklist. |
| Start, stop or reload other systemd units | W | v1.x | Human approval initially; no time-limit-free unattended stop. |
| Inspect service dependencies and reverse dependants | R | v1 | Essential before planned restarts. |
| Compare service config fingerprints/mtime and known-good baseline | R | v1.x | Avoid storing secret config contents. |
| Validate service config before reload | R | v1.x | Use native validation commands with fixed argv. |
| Schedule service restart in maintenance window | W | v1.x | Delay/recheck incident state before dispatch. |
| Detect crash loops, restart storms and failed health checks | R | v0.1/v1 | Suppress destructive remediation loops. |
| Supervisor, cron and systemd timer inventory/status | R | v1 | Later support for other service managers. |

## 3. Containers and orchestration

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Docker container list, inspect, logs, events, stats, health and restart history | R | v0.1 | Current foundation. |
| Docker Compose project/stack inventory and service dependencies | R | v1 | Link containers to projects and volumes. |
| Restart one selected container | W | v1 | **First set (M8, L2).** Verify health and dependent services. **[FLAG: Docker socket write]** needs the root-owned helper and its ADR first (owner decisions D-24, D-29). Recreate is later. |
| Start/stop a Compose service or restart a selected stack | W | v1.x | Approval initially; concurrency and downtime constraints. |
| Compose config validation and rendered diff | R | v1.x | Avoid exposing secrets in rendered config. |
| Image inventory, digest drift, outdated image and vulnerability metadata | R | v1.x | Scanning can be an external provider. |
| Pull image, redeploy service, rollback to previous digest | W | v1.x | Human approval by default, known-good image required for rollback. **[FLAG: Docker socket write]** |
| Docker network/volume inventory and dangling-resource reporting | R | v1 | Never infer that unused means safe to delete. |
| Pause one named container with a time limit | S | v1.x | **DEFERRED candidate** (`container.pause`): its expiry would depend on the agent. **[FLAG: Docker socket write]** |
| Container isolation, stop or network disconnect | S/W | v1.x | Security action with scope, TTL/recovery and owner policy. Deferred: weaker lockout/recovery story. **[FLAG: Docker socket write]** |
| Podman and containerd providers | R/W | v1.x | Reuse the container capability family. |
| Kubernetes cluster/node/pod/deployment/service/ingress inventory | R | later | Need cluster and namespace scope. |
| Kubernetes rollout status, restart deployment, scale replicas | W | later | Requires workload ownership and availability budgets. |
| Kubernetes network policy / isolate workload | S | later | Requires cluster-specific safety model. |
| k3s and lightweight cluster operations | R/W | v1.x/later | Follow Kubernetes provider contracts. |

## 4. Storage, filesystems and data protection

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Block devices, SMART/NVMe health, wear, errors, temperature | R | v0.1 | Current SMART base; trend remaining life cautiously. |
| Mounts, filesystem type, capacity, inode and read-only state | R | v1 | Link path to device, pool and host. |
| RAID/ZFS/Btrfs/mdraid/LVM pool and volume health | R | v1.x | Provider-specific health states and supported repairs. |
| SnapRAID status, scrub age, sync state and parity health | R | v1.x | Homelab-specific, valuable as optional provider. |
| MergerFS pool composition, branch capacity and policy | R | v1.x | Detect uneven capacity and full branches. |
| Disk SMART short/long self-test scheduling | W | v1.x | Schedule carefully; avoid concurrent tests. |
| Scrub, resilver, balance, repair and pool expansion | W | later | High impact; approval, health prerequisites and recovery plan. |
| Snapshot inventory and snapshot creation | R/W | v1.x | Keep retention and storage capacity limits. |
| Backup job status, last success, duration, bytes and destination health | R | v1 | Include freshness and source/destination relationship. |
| Backup verification, repository check and sample restore test | R/W | v1.x | A verified restore is stronger than a successful backup exit code. |
| Restore a selected file/directory to an alternate path | W | v1.x | Avoid overwriting the live source by default. |
| Restore an entire host/service from backup | W | later | Guided human-approved disaster recovery. |
| Backup policy coverage and RPO/RTO compliance reporting | R | v1.x | Requires owner-provided importance and target objectives. |
| Safe cleanup candidates (cache/temp/log retention) | R | v1 | Report candidates first; never broad recursive deletion. |
| File deletion, volume destruction, formatting, backup pruning | D | later/deny-default | Never policy-allowed in v1 (owner decision D-06). Human approval with typed confirmation only. A generic typed-path deletion capability would need a path-scope model; prefer documented manual runbooks. |

## 5. Network and connectivity

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Ping/TCP/HTTP/HTTPS and gateway probes | R | v0.1 | Current checks/probes. |
| DNS lookup, resolver chain, authoritative response, reverse lookup | R | v1 | Check split DNS and local resolvers. |
| Route tracing, MTU/path diagnostics, packet loss and latency | R | v1.x | Bound output and collection time. |
| TLS certificate expiry, chain validity, hostname mismatch and issuer | R | v1 | Certificate context should link to endpoint and service. |
| Port exposure and listener changes versus baseline | R | v0.1/v1 | Current detection should evolve with asset context. |
| Network interface, bond, VLAN, bridge and virtual network inventory | R | v1.x | Include hypervisor/container relationships. |
| Reverse proxy route/health/config status (Nginx, Traefik, Caddy, NPM) | R | v1.x | Start read-only; config validation before reload. |
| Reload/restart reverse proxy after validated config | W | v1.x | Verify routes and management access. |
| VPN tunnel and peer health (WireGuard, Tailscale, OpenVPN) | R | v1.x | Know whether the tunnel is a management path. |
| Add/remove or rotate VPN peers/keys | W | later | Approval and identity/recovery model required. **[FLAG: secret handling]** |
| Router inventory, WAN status, routes, DHCP, DNS and port forwards | R | v1.x | API provider for selected router vendor. |
| Temporary block of a confirmed external source IP | S | v1 limited | **First set (M9, L3):** `firewall.block_source`, with a kernel-enforced timeout and a 30 s two-step confirmation (an unconfirmed block expires by itself). Only with owner-confirmed, current management paths covered by the agent-reported exclusions (owner decision D-28), agent- and helper-enforced exclusions, rate limits and `OutcomeUnknown` handling, applied through the helper's `nft` group (not production-ready until R-2), because the agent dials out and a rule can sever its own control link. Triggered only by a deterministic detector's typed signal. |
| Remove/restrict a port forward or WAN exposure | S/W | v1.x | Higher blast radius; stage and verify alternate access. |
| Firewall rule changes, network segmentation and VLAN changes | W/S | later | Human approval until robust simulation and rollback exist. |
| ISP/WAN outage diagnosis and failover status | R/W | later | Depends on router dual-WAN support. |
| Wi-Fi AP/controller health and client inventory | R | later | Ubiquiti/UniFi and other ecosystems. |

## 6. Defensive security

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| SSH auth failures, successful remote logins and brute-force trends | R | v0.1 | Existing check foundation. |
| Sudo/auth failures, new users/groups and privilege changes | R | v0.1/v1 | Distinguish normal admin work from suspicious access. |
| Fail2ban status, jail health and ban list | R | v0.1/v1 | Existing check foundation. |
| Unexpected listeners, new public ports and exposure drift | R | v0.1/v1 | Correlate with service owner and ingress path. |
| Package/image vulnerability findings | R | v1.x | Integrate scanners; explain exploitability carefully. |
| File integrity / critical config drift | R | v1.x | Hash important files; do not collect secrets. |
| Suspicious process, persistence, cron/service and startup change | R | v1.x | Heuristic findings require evidence and uncertainty labels. |
| Secret leak detection in logs/config/repository | R | v1.x | Never echo the secret value; show location and type. |
| Container privilege/capability/mount exposure audit | R | v1.x | Policy recommendations, not automatic hardening at first. |
| Temporary block offending external IP at host firewall | S | v1 limited | **First set (M9, L3)**, same capability as above. Must exclude trusted ranges, gateway, VPN and current management session. |
| Add temporary Fail2ban jail/ban with expiry | S | v1.x | Prefer native expiry and verify the rule. |
| Isolate one compromised container/network namespace | S | v1.x | Preserve forensic evidence and dependencies. Deferred containment candidate. **[FLAG: Docker socket write]** |
| Disable compromised account or revoke session/token | S/W | later | Requires identity provider integration, owner controls and recovery. |
| Rotate exposed API key/certificate/credential | W/S | later | Secret handling and dependent service reload make this high impact. **[FLAG: secret handling]** no secret store design exists. |
| Restrict WAN SSH to VPN/trusted IPs | S/W | later | Safe only with tested alternate management route. |
| Incident evidence collection and forensic snapshot | R/W | v1.x | Preserve bounded, integrity-protected artifacts. |
| Host isolation / quarantine | S | later | Emergency response with explicit break-glass recovery. |
| Automatic patch installation or vulnerability remediation | W | later | Separate detect, stage, approve, apply and verify. |

## 7. Updates, maintenance and lifecycle

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Pending OS/package updates and security updates | R | v1 | Group by host/service and reboot need. |
| Container image update availability and changelog/release info | R | v1.x | Pin digest, don't blindly use latest. |
| Maintenance calendar and maintenance-window awareness | R/W | v1 | Avoid planned changes during backup or active workloads. |
| Pre-maintenance health and dependency check | R | v1 | Block/ask when prerequisites are unknown or unhealthy. |
| Create backup/snapshot before a supported update | W | v1.x | Verify backup is usable, not just scheduled. |
| Stage an update and show a diff/plan | W | v1.x | Human approval for applying. |
| Apply updates, restart affected services and verify | W | later/v1.x | Explicit host policy, rollback and outage budget. |
| Reboot coordination across dependent hosts | W | later | Respect quorum and criticality. **[FLAG: lost result]** |
| Certificate renew and service reload | W | v1.x | Good L2 candidate when provider is deterministic. |
| Log rotation and bounded cache cleanup | W | v1.x | Exact age/path/size constraints, never broad delete. |
| Retire/decommission host or service | W/D | later | Must preserve dependencies, secrets and backups. |

## 8. Databases, caches and stateful services

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Database endpoint health, replication, connections, locks and storage | R | v1.x | Begin with health/observability. |
| PostgreSQL, MySQL/MariaDB, Redis, SQLite service diagnostics | R | v1.x | Separate read/query credentials from admin credentials. |
| Slow query and lock analysis with redaction | R | later | Query text may contain sensitive data. |
| Controlled service failover or replica promotion | W | later | Requires topology, quorum and recovery semantics. |
| Backup and restore validation for database | W | v1.x | Restore into isolated target by default. |
| Vacuum/compaction/maintenance operations | W | later | May affect latency and storage. |
| Cache flush or key eviction | W/D | later | Explicit namespaces and impact preview; no broad clear by default. |

## 9. Common self-hosted and homelab applications

These should be application profiles layered over generic capabilities. Do not create a different orchestration engine for each app.

| Application/domain | Read capabilities | Possible operations | Priority |
|---|---|---|---|
| Plex / Jellyfin / Emby | service health, libraries, transcode/session state, logs, disk access | restart service, refresh metadata/task queue | v1.x |
| Sonarr / Radarr / Lidarr / Readarr | API health, queue, download client, root paths, indexer health | retry/import/refresh queue actions, restart service | v1.x |
| Prowlarr / Bazarr / Overseerr | API health, indexer/subtitle/request status, logs | refresh indexers, retry metadata/subtitle task | later |
| qBittorrent / Transmission | queue, connection, tracker, disk and download path health | pause/resume selected job, restart service | v1.x |
| Minecraft / Vintage Story / Project Zomboid servers | process, player count, world save health, logs, version | announce, graceful save/stop/restart, backup world | v1.x |
| Home Assistant | API health, integrations, automations and logs | reload selected integration/automation, restart with approval | later |
| Nextcloud / Immich / photo/file services | health, background jobs, storage, database and queue | restart worker, requeue bounded task | later |
| Git forge / CI runners | runner health, queue, webhook delivery, disk | restart runner, drain queue, scale workers | later |
| Reverse proxy / DNS ad blocker | routes, upstream health, certs, DNS query errors | validated reload, temporary route disable | v1.x |
| Download/indexer/media stack as a dependency group | end-to-end data flow across services | investigate and repair the failing link in the chain | v1.x |

## 10. Local AI and GPU workloads

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| GPU inventory, driver, temperature, utilization, memory and power | R | v1 | NVIDIA first; abstract provider interface. |
| Inference server health (Ollama, llama.cpp server, vLLM, LM Studio) | R | v1.x | Endpoint health, loaded model and active requests. |
| Model inventory, model metadata and disk/VRAM footprint | R | v1.x | Do not expose private prompt content. |
| Active model/session/queue and saturation | R | v1.x | Context for overload and latency incidents. |
| Unload an unused model | W | v1 | Possible L2 candidate with active-session check. |
| Restart inference service | W | v1 | Possible L2 candidate with request drain and health check. |
| Change model/quantization/parallelism settings | W | later | High impact; human approval and benchmark verification. |
| Download/cache model | W | later | Disk capacity, provenance and checksum validation. |
| Delete models/cache | D | later/deny-default | Do not grant autonomy by default. |
| Schedule model loads based on demand | W | later | Avoid oscillation and ensure adequate VRAM. |
| Benchmark throughput/latency and recommend model placement | R | later | Keep recommendations separate from policy changes. |

## 11. Virtualization, hardware and power

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Hypervisor and VM inventory, resource, state and host pressure | R | later |
| Start/stop/restart VM; snapshots and controlled migration | W | later | Respect service dependencies and storage. |
| Proxmox/Libvirt API integration | R/W | v1.x/later | Start read-only. |
| IPMI/Redfish power, temperature, fan, PSU and sensor health | R | later |
| Remote power cycle or reset | W | later | Break-glass only, explicit owner authorization. **[FLAG: lost result]** needs a model of what powers the hub. |
| UPS/NUT status, battery runtime, load and test | R | v1.x |
| Graceful shutdown on low runtime / power loss | W | v1.x | Policy plus dependency-aware shutdown order. **[FLAG: lost result]** |
| Smart PDU outlet status/control | R/W | later | Risk of cutting off networking or hub. |

## 12. External systems and integration providers

Hub-side integrations and providers (router, hypervisor, NAS, cloud) are DEFERRED: each needs hub-held credentials and its own contracts (`docs/rework/06` "Deferred and after-v1"). The rows below are an idea inventory, not a v1 commitment.

| Provider family | Useful capabilities | Priority |
|---|---|---|
| Alertmanager, Uptime Kuma, generic webhook | receive signals, correlate and enrich alert | v0.1 |
| Prometheus-compatible metrics | query metrics, compare baseline, collect alert evidence | v1.x |
| Grafana | dashboard/link context, alert state, annotation | later |
| Docker API / Compose | container and stack lifecycle | v0.1/v1 |
| systemd / D-Bus | service manager capabilities | v0.1/v1 |
| Nginx Proxy Manager, Nginx, Traefik, Caddy | routes, certs, upstream and validated reload | v1.x |
| OPNsense, pfSense, MikroTik, UniFi, OpenWrt | network topology, firewall, DHCP, DNS and containment | v1.x/later |
| WireGuard, Tailscale, OpenVPN | tunnel/peer health and scoped management | v1.x |
| Proxmox, Libvirt, TrueNAS, Synology, QNAP | host/VM/NAS/storage inventory and operations | later/v1.x |
| Restic, Borg, Kopia, Proxmox Backup Server | backup jobs, repository health, verification and restore | v1.x |
| GitHub / GitLab | CI run status, repository release, deploy metadata, issue/PR linkage | later |
| Cloud APIs | inventory, health, budget and selected operations | v2+ |
| Home Assistant | state and service health, selected home automation | later |
| Email/chat systems | notifications, interaction, user approvals/questions | v0.1/v1 |

## 13. Domovoi's own operations (self-observability)

- Hub health, memory/CPU/DB size, task queue, scheduler lag and request latency.
- Agent heartbeat, spool size, capability inventory drift and local policy version.
- LLM provider health, latency, token usage, budget and fallback frequency.
- Failed/redacted tool calls, context size, phase duration and cycle count.
- Database backup age, integrity-check status, migration version and restore test.
- Action queue, outstanding approvals, policy denials and expired permits.
- Notification delivery failure and webhook signature failure.
- Config validation, rollback and post-upgrade self-test. Self-upgrade itself is **[FLAG: self-upgrade]**: out-of-band only until an ADR designs signed host-pinned artifacts.
- Detect a misbehaving agent/provider and automatically disable its capability from discovery (not silently alter its owner policy).

## 14. Knowledge and experience capabilities

- Discover entities from agent inventory, provider APIs, configs and user onboarding.
- Propose facts and relationships with source, confidence, timestamp and freshness.
- Confirm, correct, reject, supersede, expire or delete facts.
- Explain why a fact is believed and show conflicting sources.
- Link service-to-host, container-to-volume, endpoint-to-certificate, service-to-database, backup-to-source, host-to-router and service-to-dependency.
- Search exact values, text, host scope, tags, incident history and graph relationships.
- Search experience by symptom, capability, host, time, policy decision, outcome and failed verification.
- Suggest reusable runbook notes, checks, capability gaps and onboarding questions.
- Import existing notes and profile data without silently elevating inferred data to user-confirmed knowledge.

## 15. Orchestration and user interaction capabilities

- Ask a focused user question when the missing answer changes risk or likely diagnosis.
- Request additional data from the relevant host/provider, with a clear reason and budget.
- Run parallel independent read operations; serialize conflicting mutations.
- Build a dependency-aware plan with explicit preconditions and postconditions.
- Pause a plan for approval, user input, maintenance window or external dependency.
- Resume without losing phase/cycle state; revalidate stale evidence and policy.
- Cancel, amend or reprioritize a plan when new evidence arrives.
- Verify immediate outcome and monitor for recurrence over a configured window.
- Escalate with the exact blocker and a safe manual recovery path.
- Notify via incident thread, digest, webhook and future chat providers.
- Render a human-readable explanation of what is known, uncertain, planned, authorized and completed.

## 16. Cloud and remote infrastructure

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---:|---|
| Cloud account/project/region inventory and asset discovery | R | later | Read-only first; scope credentials to selected accounts. |
| VM, instance, disk, load balancer and network health | R | later | Link cloud resources to services and owners. |
| Cloud metrics, logs, events, audit events and service health | R | later | Add provider-specific evidence adapters. |
| Cloud budget, quota, usage and spend anomaly detection | R | later | Do not autonomously scale cost-bearing resources by default. |
| Start/stop/reboot a selected instance | W | later | Approval, dependency and cost controls. **[FLAG: lost result]** for reboot; needs hub-side provider design with third-party credentials. |
| Resize instance or managed database | W | later | High-impact operation with cost and downtime preview. |
| Manage security group / firewall ingress | S/W | later | Same trusted-path and temporary-rule safety model. |
| Object storage inventory, capacity and lifecycle policy status | R | later | Avoid collecting object contents. |
| Managed database health, backup, replica and failover status | R | later | Start with health and backup verification. |
| Cloud DNS and certificate state | R/W | later | Validate record ownership and TTL/propagation. |
| Cross-region backup and disaster-recovery tests | R/W | v2+ | Require explicit RPO/RTO and cost ceilings. |

## 17. Developer platforms and delivery pipelines

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| GitHub/GitLab repository, branch, release and deployment inventory | R | later |
| CI workflow status, queued jobs, failed steps and runner health | R | v1.x |
| Link incident start to recent commits, PRs, deployments and config changes | R | v1.x | Strong change-correlation source. |
| Re-run a failed workflow or selected safe CI job | W | later | Must distinguish idempotent tests from deployment workflows. |
| Drain/restart/scale a self-hosted runner | W | later | Preserve queued jobs and avoid interrupting active deployments. |
| Build image and compare digest/SBOM before deploy | R/W | later | Build is not the same as deployment authorization. |
| Deploy a selected version to a selected environment | W | later | Require environment policy, health checks and rollback. |
| Roll back deployment to last known-good version | W | later | Verify compatibility with schema/data migrations. |
| Detect config drift between Git and live host | R | v1.x |
| GitOps status and reconciliation health | R | later | Avoid fighting the controller by changing live resources directly. |
| Create issue/PR with incident evidence or a proposed fix | W | later | Human-visible draft first; never commit secrets or raw logs. |
| Release-note/changelog summaries and dependency risk summaries | R | later | Model-generated summaries must be sourced and reviewable. |

## 18. Identity, secrets and access infrastructure

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Identity provider availability and authentication error diagnostics | R | later |
| User, group, role and session inventory (metadata only) | R | later | Never retrieve password hashes or session secrets. |
| Certificate authority, ACME account and certificate lifecycle status | R | v1.x |
| Secret manager health, secret version age and rotation status | R | later | Do not fetch secret values into model context. |
| Detect overbroad or expiring API tokens and SSH keys | R | later | Report fingerprint/owner/scope, not private material. |
| Revoke selected token or SSH key | S/W | later | High impact; require identity confirmation and recovery. |
| Rotate a service credential and update dependent services | W | later | Multi-step operation with staged rollback and secret-safe execution. **[FLAG: secret handling]** no secret store design exists. |
| SSO/OIDC/SAML integration for Domovoi users | R/W | v2+ | Organization-scale access control. |
| Break-glass user and recovery access validation | R/W | later | Ensure emergency access remains available and audited. |

## 19. Smart home, IoT and edge devices

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Home Assistant health, integration availability and automation errors | R | later |
| MQTT broker health, topic rates and client status | R | later | Topic payloads may contain personal data. |
| Zigbee/Z-Wave coordinator and device availability | R | later |
| Camera/NVR health, recording gaps and storage status | R | later | Strong privacy boundaries; avoid viewing or retaining streams by default. |
| NAS/IoT/smart plug/AP/UPS firmware and uptime inventory | R | later |
| Toggle a selected smart plug or restart a selected IoT device | W | later | Can cut power to networking or critical equipment; require dependency model. |
| Environmental sensor alerting (temperature, humidity, leak, smoke) | R | later | Sensor quality and alarm severity must be represented. |
| Automate energy-saving schedules or load shedding | W | later | Owner-defined safety and comfort constraints. |
| Smart lock, alarm and access-control operations | S/W | later/strictly constrained | High consequence; require explicit security and identity model. |

## 20. Messaging, observability and incident ecosystem

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Prometheus metrics queries and time-window comparison | R | v1.x |
| Grafana dashboard/panel links and annotations | R/W | later |
| Loki, Elasticsearch/OpenSearch and journald log search | R | later | Query caps, tenant scope and secret redaction. |
| OpenTelemetry traces and service dependency evidence | R | later |
| Alert deduplication, enrichment, routing and silence status | R/W | v1.x |
| PagerDuty/Opsgenie incident status and on-call schedule | R/W | later |
| Status page updates and post-incident report drafts | W | later | Human approval for public communications. |
| Metrics/alert rule suggestions from repeated incidents | R/W | v1.x | Suggest only; user applies. |
| SLO/error-budget tracking and service-level health | R | later | Needs service ownership and SLO definitions. |
| Scheduled operational reports and weekly environment health summaries | R | v1.x |
| Multi-channel notifications and escalation policy | W | v1.x | Avoid duplicate/noisy notification loops. |

## 21. Data services, messaging and application queues

| Capability ideas | Typical class | Priority | Notes |
|---|---:|---|---|
| Queue depth, consumer lag and dead-letter inventory (RabbitMQ, Redis, Kafka, NATS) | R | later |
| Retry or requeue a selected failed message | W | later | Message replay may duplicate side effects. |
| Pause/resume a consumer or worker pool | W | later | Respect backpressure and maintenance state. |
| Object/file processing queues and stuck-job diagnosis | R | v1.x |
| Search index health, lag and disk usage | R | later |
| Reindex or rebuild a selected index | W | later | Capacity and downtime preview required. |
| Data pipeline freshness, schema drift and failed ETL jobs | R | later |
| Trigger a bounded pipeline rerun | W | later | Require idempotency and source watermark control. |

## 22. User-facing operational assistance

- Explain a host or service in plain language using verified knowledge, not guesses.
- Answer “what changed?”, “what depends on this?”, “what is exposed?”, “what is currently unsafe?” and “what happens if I restart it?” from current evidence.
- Show planned action impact before authorization: affected entities, dependencies, expected downtime, rollback and confidence gaps.
- Compare alternative remediation plans and explain why one is safer.
- Let the owner set maintenance intent conversationally, then present a structured confirmation before storing it.
- Turn a resolved incident into a human-editable runbook proposal.
- Produce a post-incident summary with timeline, hypotheses, evidence, actions, policy decisions, verification and follow-up items.
- Provide a “why did you not act?” explanation showing unavailable evidence, missing permissions, stale policy or safety gates.
- Support user correction of entity identity, dependency, criticality and fact provenance.
- Provide a clear emergency stop / pause-all-mutations control, separate from read-only monitoring.
- Provide a per-host capability test/self-check and a policy simulation (“what would Domovoi do if this happened now?”).

## 23. Capability quality checklist

Before adding a capability, specify:

1. Stable ID, version, provider and supported host/platform.
2. Plain-language purpose and when it is appropriate.
3. Typed inputs and outputs; strict target scoping.
4. Read/write/security/destructive effect and likely blast radius.
5. Required credentials, permissions, prerequisites and availability detection.
6. Time, output, concurrency and rate limits.
7. Idempotency and behavior when the result is unknown.
8. Precondition checks and postcondition verification.
9. Rollback, compensation or expiry strategy; explicitly say when none exists.
10. Policy dimensions and the default authorization level.
11. Audit fields, secret redaction and user-visible explanation.
12. Unit, integration, failure, restart, offline and negative-target tests.

A capability is not ready for autonomous use until this checklist is complete and its relevant incident drills pass.

---

# 12 — UI product principles and experience goals

**Status:** agreed product direction plus recommended design rules. This is a target specification, not a requirement to preserve the current page layout.

## 1. The UI is a product surface, not a status dashboard

Domovoi's web UI is its primary operational control plane. It must let a user:

- Understand the state of the environment and what Domovoi is doing.
- Inspect evidence, hypotheses, decisions, capability calls, policy evaluations and results.
- Initiate investigations and operations from the UI or Operator.
- Manage hosts, capabilities, policies, knowledge, integrations and settings.
- Review history and learn why something happened.
- Configure almost every supported product behavior without editing files or using a CLI.

The UI should feel like a polished, information-rich operations environment: fast, responsive, modern and pleasant to use. It must not resemble a generic SaaS admin template or hide the system's real behavior behind vague summaries.

## 2. Design from requirements, not from existing markup

The current UI overhaul is a useful reference for brand, design tokens, current functionality, component quality and implementation constraints. It is **not** the target layout and must not anchor new information architecture.

For future design work:

1. Start from the user's goals, the product model and the information that must be visible.
2. Decide page boundaries, priority and real-estate needs before choosing components or CSS.
3. Consider new layouts, navigation, visualizations and interaction patterns without assuming the current page structure is correct.
4. Reuse current components only when they genuinely fit the new interaction and information hierarchy.
5. Preserve valued visual identity deliberately; do not preserve layouts merely because they already exist.

This is intentional. Incrementally polishing an unsuitable page structure is not the goal.

## 3. Full transparency, configurable presentation

**Agreed direction:** full transparency, configurable presentation and progressive disclosure.

The detailed experience is the design baseline. It is not a hidden expert mode that must be discovered to access useful diagnostic information.

On operational pages, the detailed baseline exposes:

- Actual orchestration phases and transitions.
- Individual reasoning requests at the level of request/response metadata and structured conclusions.
- Capability calls, arguments, targets, prerequisites, results, errors and timings.
- Evidence, provenance, freshness, hypotheses and uncertainty.
- Policy evaluation, matched rules, constraints and authorization outcome.
- Operation plans, execution states, verification and monitoring.
- Model/provider metadata, latency, token usage and cost where available.
- Relevant history, audit records and links to related entities.

A user may choose to collapse, summarize or hide secondary information in their own presentation. The system must not omit underlying events from durable history merely because the current view is compact.

Do not expose hidden model chain-of-thought. Expose useful and auditable information instead: model request metadata, tool calls, evidence returned, structured hypotheses/conclusions, decisions, explanations, policy results and execution traces.

### Presentation profiles

Three profiles ship in v1, plus Custom. They are one set of components with a category-to-visibility table (section 5), not three layouts. The profiles, the table and the shared visibility mechanism are defined before any page is implemented.

| Profile | Behavior | Intended use |
|---|---|---|
| **Detailed** | Everything shown. The default for a new owner. | Reference design; operators who want to know exactly what is happening. |
| **Standard** | P0 and P1 shown; P2 and P3 collapsed (one click away, with a count). | Balanced everyday use. |
| **Essential** | P0 and P1 shown; P2 collapsed; P3 hidden, with a visible "+N details" indicator that reveals them. | People who prefer a quieter surface. |
| **Custom** | Per-page overrides of one of the profiles above. | Users who want a compact dashboard and detailed operations, or another combination. |

Rules:

- **P0 is never hidden or collapsed in any profile**, including Custom. A test renders each page in every profile with the same fixture and asserts that every P0 element is present and expanded.
- Hidden or collapsed content stays reachable and is not rendered until revealed; profiles never delete or omit durable history.
- These profiles are display preferences only. They must never change authorization, tool availability, audit retention, execution behavior or security controls. A setting that affects authority belongs in the policy system, not the display profile.
- The UI reads the profile through one provider. Whether it is stored on the hub or only in the browser is a backend decision (see doc 17, section 11); the behavior above does not depend on it.

## 4. Understandable terminology without dumbing down

The product should be approachable to someone new to self-hosting without concealing technical precision from an experienced operator.

Rules:

- Use consistent terms and explain unfamiliar concepts where they appear.
- Prefer a clear label plus a concise explanation over replacing a precise term with a vague euphemism.
- Keep domain-standard technical terms when they add precision: orchestration, capability, policy, verification, agent, host, container, TLS and so on.
- Explain terms contextually in page descriptions, inline hints, inspectors and the user guide. Do not force users through a glossary before they can operate the product.
- Never replace meaningful orchestration phases with a single generic “working” status in the detailed view.
- Keep the same term for the same concept across navigation, API-facing labels, chat, notifications and documentation.

### Product vocabulary

| Term | Meaning | Notes |
|---|---|---|
| **Host** | A machine or managed endpoint known to Domovoi. | The UI may explain it as “server or machine”; keep `host` in technical detail where accurate. |
| **Agent** | The local Domovoi process that reports from and executes permitted operations on a host. | Do not conflate a host with its agent. |
| **Service / workload** | An application, container, system service, database or other running component. | Show the concrete type when known. |
| **Incident** | A detected abnormal condition that needs investigation, monitoring or attention. | Existing project concept; not every user request creates an incident. |
| **Operation** | A durable unit of work Domovoi performs. It may be triggered by an incident, user request, schedule or automation. | The broad execution/history concept. The main page is **Operations**. |
| **Task** | A plain-language word for work Domovoi has been asked to do. | May be used in friendly copy; do not create a second backend abstraction unless needed. |
| **Operator** | The conversational interface for asking questions, initiating operations and following their progress. | Available as a global side panel and a full-page workspace. |
| **Capability** | A typed operation Domovoi can discover and invoke, such as inspect logs or restart a service. | Skills are one implementation/provider, not the whole capability model. |
| **Policy** | Owner-defined rules that decide what Domovoi may do and under which constraints. | Policy is configuration/authority; a permission is the result or grant. |
| **Knowledge** | Current or persistent information about the environment, with provenance and freshness. | Not the same as notes, history or policy. |
| **Experience** | Historical records of investigations, decisions, operations and outcomes. | Evidence of what happened, not automatically current truth or permission. |
| **Integration** | A connected external system or service that provides data or operations. | For example router API, monitoring system or notification channel. Hub-side integrations are DEFERRED past v1 (`docs/rework/06` "Deferred and after-v1"). |
| **Incompatible** | A host state: its agent speaks a different protocol minor than the hub. | Shown with an upgrade-required explanation, the required protocol and the agent's version; its history is kept, also after a hub restart. After a hub upgrade, a host that has not reconnected shows "expected incompatible". Never presented as offline. |
| **Helper** | The root-owned first-party process through which the unprivileged agent performs privileged host operations (owner decision D-29, RW-39). | Installed only by the owner-controlled OS install path, never by a package. Where it is missing or in error, affected checks and capabilities say "helper unavailable", not "failed". A host whose agent still runs as root is labelled "privilege boundary not in effect".
| **Locked Deny** | An owner-declared prohibition (default off) that no narrower policy rule overrides. | Shown distinctly from ordinary rules; changing it needs step-up. A policy Deny is never offered for approval. |

The exact navigation labels can evolve after page planning, but this vocabulary should remain coherent.

## 5. Information hierarchy: priority and space are separate

Every meaningful page element receives two independent classifications.

### Display priority

| Priority | Definition |
|---|---|
| **P0 — Safety and state** | Safety, approval, audit and current-state content: what is happening now, what needs a decision, what authority applies, what was recorded. Never hidden or collapsed in any profile. |
| **P1 — Primary** | The page's main content and frequent actions. Visible without navigating away; may share space with P0. |
| **P2 — Technical detail** | Useful diagnostics, raw inputs and outputs, history and secondary actions. Collapsed in Standard and Essential. |
| **P3 — Metadata** | Rarely needed, verbose or low-frequency metadata (ids, timings, model parameters). Collapsed in Standard; hidden behind "+N details" in Essential. |

Every page element carries exactly one category, written in its page specification and in the code through the shared visibility primitive. Visibility per profile follows from the category alone:

| Category | Detailed | Standard | Essential |
|---|---|---|---|
| P0 | shown | shown | shown |
| P1 | shown | shown | shown |
| P2 | shown | collapsed | collapsed |
| P3 | shown | collapsed | hidden, "+N details" indicator |

A page that needs a different result for one element changes the element's category, not the table.

### Space requirement

| Size | Definition |
|---|---|
| **Dominant** | Needs a major share of the page to be usable, e.g. a complex orchestration graph. |
| **Large** | Needs substantial room, e.g. a detailed evidence table, history grid or rich editor. |
| **Medium** | A standard page section or side panel. |
| **Compact** | A small summary, status, toolbar or focused control. |
| **Contextual** | Appears only in response to selection/action: inspector, popover, dialog or side drawer. |

High priority does not imply large size. For example, “approval required” may be P0 but compact; the orchestration graph may be P0 and dominant. Page design must budget for both classifications.

## 6. Decide the right container, don't default to cards or tabs

Use the interaction and content to choose the presentation:

- **Dedicated page:** the content has a distinct purpose, several workflows or a dominant visualization.
- **Subpage:** a selected object needs a focused workspace of its own.
- **Tabs:** several peer views describe the same object, and users are likely to switch among them.
- **Sections:** related information belongs together and should be scanned as one page.
- **Expandable group:** information is useful but secondary on most visits; keep a clear count/status and preserve a discoverable entry point.
- **Inspector/drawer:** details must be examined without losing the user's current position or graph context.
- **Popover/menu:** a compact choice or small group of contextual actions.
- **Dialog:** a focused decision, destructive confirmation or short multi-step interaction. Do not put a large workflow in a modal merely to avoid page planning.

Tabs must not be used as a hiding place for P0 information. If the user needs two large areas simultaneously, use a split view, separate page or linked workspace instead.

## 7. The UI should be user-configurable, but not chaotic

Recommended customization:

- Detailed/Standard/Essential presentation profiles, with Custom as per-page overrides of one of them.
- Overview dashboard widgets can be added, removed, rearranged, resized and saved per user. DEFERRED to v1.x (demand unvalidated; `docs/rework/06` "Deferred and after-v1"): v1 ships one carefully designed default layout.
- Per-page preferences for density and default expanded sections.
- Persist filters, table columns, sort order and selected views where it helps repeat workflows.
- A user may reset to a strong default layout at any time.

Do not require the user to customize the UI before it becomes useful. Defaults must be carefully designed and coherent. The default profile for a new owner is Detailed; Standard and Essential are one setting away and remembered. Because the default shows everything, the Detailed layouts of every page are designed first and tested on a phone and a laptop, not only on a wide desktop.

## 8. Control-plane principle

The UI should be the normal place to configure product behavior. If a supported setting can be safely changed in the UI, expose it there.

A control may remain outside ordinary UI management only for a deliberate security or recovery reason (for example initial root-of-trust establishment or recovery from a compromised hub). Even then, the UI should explain the reason and offer safe lifecycle actions where possible. Secret values and private key material may remain write-only/non-readable while still supporting set, replace, test, rotate or revoke actions.

## 9. Product quality is functional, not decorative

A beautiful screen that has stale state, lost context, confusing actions or flaky interactions is not a good UI.

Every important control must have predictable behavior and feedback. Every data view must distinguish the states in the canonical taxonomy (doc 15, section 10): loading, empty, first-load error, refresh error, stale, offline, unavailable, partial and unknown value (a host also has `Incompatible`). Live updates must not reorder or replace content in a way that loses user focus or an inspector selection.

The UI should feel dynamic because the product is genuinely active, not because it constantly animates. Motion must communicate state changes, direction, hierarchy or spatial relationships.

## 10. One owner in v1

Through v1 Domovoi has a single owner and no roles. The UI therefore has no permission-denied states and does not filter search or lists by permission. Where a future role would change what is shown, the specification names the place as a hook "for future roles" (for example the single function every action check goes through) and builds nothing more. Actor identity is still shown wherever an action is attributed (owner, system component, agent, policy rule).

## 11. Design review questions

Before approving a page specification, answer:

1. What user question does this page answer?
2. What is P0, P1, P2 and P3 here (category, not size)?
3. What is dominant/large/medium/compact/contextual?
4. Is there enough screen area for every dominant item at the minimum sizes in doc 16, section 5?
5. Should content be a page, subpage, tab, section, expansion or inspector—and why?
6. What can the user do here, and what happens after each action?
7. What changes live while the page is open?
8. Which states from the canonical taxonomy apply, and what does each look like?
9. What must survive navigation, reload, collapse/expand and reconnect?
10. How does the same page work in each presentation profile and at different viewport sizes?
11. What is the acceptance test for a correct, polished implementation?

---

# 13 — UI information architecture and navigation

**Status:** target proposal based on agreed product concepts. Page names and grouping are recommended defaults; page responsibilities are more important than the final labels.

## 1. Top-level model

The UI should organize around the user's work, not mirror internal Rust crates or service boundaries.

Recommended top-level navigation:

| Area | Main purpose | Primary entries |
|---|---|---|
| **Control Room** | Understand the environment and current attention needs. | Overview |
| **Operations** | See and manage all work Domovoi performs. | Operations, operation detail, operation history |
| **Incidents** | Understand problems detected in the environment. | Incident list, incident detail |
| **Infrastructure** | Understand and manage hosts and the things running on them. | Hosts, host detail, service/workload detail, topology |
| **Capabilities** | Discover, install, configure and manage what Domovoi can do. | Catalog, installed capabilities, deployment history, capability detail |
| **Knowledge** | Inspect and maintain what Domovoi knows about the environment. | Knowledge browser, graph explorer, notes, suggestions |
| **Policies & Autonomy** | Define authority and understand what Domovoi is allowed to do. | Policies, per-host policy, simulator, decision history |
| **Integrations** | DEFERRED past v1 (hub-side providers need hub-held credentials; `docs/rework/06` "Deferred and after-v1"). Connect external systems and manage their providers. | Connected integrations, add integration, integration detail |
| **System** | Manage the hub and account-wide behavior. | Settings, models, notifications, audit, backups, updates, diagnostics |

Do not force the final sidebar to contain every possible subpage. Important detail views can be reached through the parent page, global search and contextual links. The hierarchy should be reviewed after full page specifications exist.

## 2. Global application shell

### Persistent shell elements

| Element | Priority | Space | Required behavior |
|---|---:|---:|---|
| Main navigation | P0 | Medium / collapsible | Clear grouping; active route; counts for attention states; keyboard and narrow-screen support. |
| Environment/host context | P0 | Compact | Makes it obvious whether the view covers all hosts or a specific host/environment. Contextual object pages can set it deliberately. |
| Live connection/system state | P1 | Compact | Truthful connection state with explanation and retry/status detail. |
| Global search / command palette | P1 | Contextual | Search destinations and objects; command actions are clearly separated from search results. |
| Operator launcher | P0 | Compact | Available from anywhere; opens the persistent side panel without losing current page state. |
| Attention center | P0 | Compact | The one durable list of approvals, questions, manual interventions, failures and completed operations, with unread state; never rely only on toasts. Control Room "Waiting for you" and the Approvals page are views over this same list. |
| Autonomy state | P0 | Compact | Shows when autonomy is paused or any rule is suspended by its circuit breaker; links to Policies & Autonomy. The pause switch is human-only. |
| Incompatible-agent banner | P0 | Compact | Appears while any host is `Incompatible` or "expected incompatible" after a hub upgrade (owner decision D-20, RW-17): names the host(s), the exact required protocol and the agent's version, and says to upgrade the hub first and then each agent through the install procedure. Links to the host; history stays readable and the host is never shown as merely offline. |
| Root-agent warning banner | P1 | Compact | Appears while any host's agent runs as root (owner decision D-29, RW-39): the privilege boundary is not in effect, unattended execution is off on that host, and the owner can rerun the installer. Links to the host. |
| Account/user menu | P2 | Compact | Profile, preferences, theme and sign out. |

The shell may evolve from the current sidebar/topbar layout. Do not assume all actions must live in the top bar or that the current sidebar groups must remain.

### Global keyboard model

Recommended defaults:

- `Ctrl/Cmd+K`: global search and command palette (the current overhaul already uses this shortcut).
- `Ctrl/Cmd+J`: open/focus the Operator side panel.
- `Escape` closes one thing per press, in this order: tooltip, popover or menu; dialog or palette; then the panel or inspector that has focus (if none has focus, the one opened most recently); then the graph selection. Escape never stops or cancels an operation; those need their own control and confirmation.
- `Ctrl/Cmd+K` and `Ctrl/Cmd+J` open separate launchers (search and Operator); neither replaces the other.
- `/` may focus the current page's search field only when focus is not already inside an editable control.
- Shortcuts must be discoverable and user-configurable where conflict is possible.

### Global context behavior

- A host/environment selector should be consistent across pages that support scoping.
- **Host scope rule.** Pages follow the host selector, or say why they do not ("all hosts" note). Opening a page about one host shows that host and sets the selector to it; leaving restores the choice the person had before. Changing the selector while on a host page navigates to the chosen host's page.
- Search results and deep links must preserve scope where needed.
- The Operator must state its current context (all hosts, selected host, selected incident/operation) and allow changing it explicitly.
- A contextual link should open the correct target and preserve a useful route back to the originating workflow.

## 3. Operator surfaces

The Operator is one product feature with two presentations over the same durable conversations and operation state.

### Right-hand slot precedence

The shell has one right-hand slot, shared by the Operator panel and the contextual inspector. It never overlaps page content and never shifts it while animating (panels move by transform; content is padded, not reflowed mid-transition).

- **Wide screens** (the page keeps at least 640 px of content width after the sidebar and slot): the Operator panel and the inspector can sit side by side.
- **Narrower screens:** the surface opened most recently wins the slot; the other collapses to a tab in the slot's header and keeps its state. Opening the collapsed one swaps them.
- **Phones:** the slot is a full-screen sheet, one surface at a time.
- Closing the surface in front reveals the collapsed one; neither is destroyed by the other opening.

### Global side panel

- Available from every signed-in page.
- Supports short questions, one-step requests, quick investigation starts and monitoring existing work.
- Has an explicit context selector: current page/object, selected host, all hosts or another chosen scope.
- Shows recent conversations and active operations without becoming a separate operations engine.
- Expands into the full Operator workspace while preserving conversation, message history, selected context and operation IDs.
- Collapsing the panel does not stop or detach work.
- Conversations are durable and stored on the hub. Host or global scope is explicit per conversation and shown in the header. A response that arrives for a superseded request is discarded, not shown. The composer draft is kept locally until the send is committed, so a failed send or a reload loses nothing.

Recommended behavior: a non-modal, resizable panel on wide screens; a full-screen sheet/page-like panel on small screens. Initial width is a design choice to test; reserve enough space for real transcripts and operation cards.

### Full Operator workspace

- Dedicated route/area for long conversations, complex troubleshooting and user-guided tasks.
- Conversation and operation detail are linked, but they are not the same object.
- It can show a conversation transcript alongside the active operation graph, evidence and inspector.
- Users can move between conversation and operation views without losing either.
- Supports multiple conversations and long-lived operations.

## 4. Operations and incidents are separate first-class concepts

**Incident:** a detected abnormal condition.

**Operation:** work performed by Domovoi. An operation may be started by an incident, a user request, a schedule, a policy or another operation. Not all operations belong to an incident.

Relationships:

- One incident may have multiple operations (investigate, repair, monitor, follow-up).
- One operation may relate to one or more incidents, hosts, services or knowledge entities.
- A user-requested operation may exist without any incident.
- An operation may discover a new incident as a result, but these are still distinct records.
- A conversation can initiate or discuss an operation; the conversation is not the operation itself.

The Operations page is a unified view of work. The Incidents page remains a view of detected problems. Pre-existing actions appear in the Operations list as read-only **Legacy** entries; no steps or approvals are invented for them.

### One home for approvals

An approval is a step of an operation, so **operation detail is its canonical home**. It is rendered by one structured approval card component (doc 14, section 5) that shows contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, whether the approval binds the v1 or the v2 plan hash (labelled during the migration, RW-11) and the policy decision; model text is secondary and labelled. Incident detail, the Control Room, the Operator and the attention center embed that card or link to the operation; none builds its own version. Incident controls (acknowledge, mute, dismiss) stay separate from operation controls (stop, cancel, retry) and from approval controls.

## 5. Page map

```text
Domovoi
├── Control Room
│   └── Overview
├── Operations
│   ├── All operations
│   ├── Running / waiting / scheduled / completed views
│   └── Operation detail
│       ├── Orchestration graph
│       ├── Findings and plan
│       ├── Evidence and capability calls
│       ├── Execution, verification and monitoring
│       ├── Step timeline / tree (built first) and orchestration graph
│       ├── Similar past operations
│       ├── Historical playback (the last U2 deliverable)
│       └── Timeline / technical trace
├── Incidents
│   ├── Incident list / filters
│   └── Incident detail
│       ├── Summary and evidence
│       ├── Related operations
│       ├── Current status and recommended response
│       ├── Similar past operations
│       └── History
├── Infrastructure
│   ├── Hosts
│   ├── Host detail
│   │   ├── Overview
│   │   ├── Services and workloads
│   │   ├── Capabilities
│   │   ├── Monitoring and checks
│   │   ├── Network and security (management paths: provenance, freshness, what is blocked)
│   │   ├── Knowledge and dependencies
│   │   └── Activity
│   ├── Service/workload detail
│   └── Infrastructure topology
├── Capabilities
│   ├── Discover catalog (first-party and owner-signed packages; third-party registries DEFERRED)
│   ├── Import package (owner-signed archive and signature)
│   ├── Installed
│   ├── Deployments / updates
│   ├── Capability detail
│   └── Install / configure flow
├── Knowledge
│   ├── Knowledge browser
│   ├── Entity detail
│   ├── Relationship / graph explorer
│   ├── Notes and runbooks
│   └── Suggested knowledge
├── Policies & Autonomy
│   ├── Policy overview
│   ├── Policy detail / editor (locked rules, publication validation report)
│   ├── Host policy
│   ├── What-if simulator
│   └── Decision history
├── Attention
│   └── Approvals, questions, failures, completions
├── Integrations
│   ├── Connected integrations
│   ├── Catalog / add integration
│   └── Integration detail
└── System
    ├── Settings and models
    ├── Notifications
    ├── Audit log (legacy and chained history verified separately)
    ├── Data retention
    ├── Backups / restore
    ├── Updates (versions and upgrade-required status; no agent self-update)
    └── Diagnostics
```

Not every item must be a permanent sidebar entry. Some are nested routes or tabs on the relevant object.

## 6. Navigation principles

- Distinct work areas deserve distinct destinations; do not overload one generic “Dashboard” page.
- Detail views should be deep-linkable and restore the selected tab, filter and meaningful view state when safe.
- **Depth rule:** at most three levels: area, object, tab. Anything deeper is a section on the object page, an inspector or a linked page, not a fourth level. A user should normally reach any object in a few steps through navigation, contextual links or search.
- Tabs on an object page never hold P0 content that a different question needs at the same time (for example an approval request or current state); that stays above the tabs.
- The same entity should have one canonical detail page. Other pages can preview it but should not create competing copies of its configuration.
- Use tabs for peer views of one entity, not as a substitute for information architecture.
- Large graph workspaces, long-running operation workspaces and complex installers should receive the space of a dedicated page.
- Provide breadcrumbs or a clear back path for nested object pages.
- Do not use a modal as the only way to access a detailed capability manifest, full policy editor or long diagnostic workflow.

## 7. Universal search versus command palette

The current `Ctrl/Cmd+K` palette is an early foundation, not the completed search product.

Target search should find:

- Pages and commands.
- Hosts, agents, services, containers and other infrastructure entities.
- Active and historical incidents and operations.
- Capabilities and installed providers.
- Knowledge facts, entities, relationships, notes and runbooks.
- Policies and relevant decision records.
- Integrations and settings.

Search results should be grouped by type, show distinguishing metadata and status, support keyboard navigation, and open the canonical detail page. Commands (for example “open Operator”, “add host” or “install capability”) must be clearly distinguished from objects returned by search.

Do not imply a full search index exists until the data sources, freshness, ranking and indexing behavior are implemented; each result group says what it covers. With one owner there is no permission filtering in v1; the search layer keeps a single filter point as a hook for future roles. The palette needs an asynchronous result source (loading, no results, failure) in addition to its current static groups. Keep search provider-independent so semantic search may be added later without changing the UI contract.

## 8. Cross-page entity links

Important objects should form a navigable network:

- Host ↔ agent ↔ capabilities ↔ services/workloads ↔ incidents ↔ operations.
- Operation ↔ incident(s) ↔ evidence ↔ capabilities ↔ policy decisions ↔ experience records.
- Knowledge entity ↔ facts ↔ relationships ↔ notes ↔ incidents/operations.
- Capability ↔ provider/package ↔ installed agents ↔ policies ↔ runs/operations.
- Policy rule ↔ affected capability/host ↔ simulation ↔ historical decision.

This interconnectedness is a core usability feature, not merely a graph visualization requirement.

## 9. Open architecture choices for later design

- Control Room customization (saved workspaces or a customizable layout) is DEFERRED to v1.x (demand unvalidated; `docs/rework/06` "Deferred and after-v1"); v1 ships one default layout.
- Whether infrastructure topology is a subpage of Infrastructure or a global visualization entry point.
- Whether Experience gets a dedicated global explorer. v1 surfaces it as "similar past operations" on incident and operation pages, with resolution summaries, marked historical and never an authorization; a global explorer is not in v1.
- The final sidebar grouping and whether “Incidents” is grouped under “Respond” or has its own top-level section.
- Whether Integrations and Capabilities are separate top-level sections (recommended) or combined in a catalog with clear subtypes.

---

# 14 — Page specifications and screen-space planning

This document defines what each target page must contain and do, before visual layout is designed. It is intentionally not a pixel-level wireframe. Every page should be prototyped from these requirements, with the detailed experience as the starting point.

## 1. Priority and space legend

**Priority (category):** P0 safety, approval, audit and current state · P1 primary · P2 technical detail · P3 metadata. The category decides what each presentation profile shows (doc 12, section 5); P0 is shown in every profile.

**Space:** Dominant · Large · Medium · Compact · Contextual.

The space classification is the amount of useful screen area required, not a fixed pixel measurement. It must be validated at realistic viewport sizes and with real data volumes.

## 2. Page inventory at a glance

| Page / surface | Main purpose | Largest element | P0 content |
|---|---|---|---|
| Control Room / Overview | What is happening across the environment? | Active work visualization or attention workspace (Large) | Critical attention, active operations, environment health |
| Operations list | What work is Domovoi doing or has done? | Filterable operation list (Large) | Running, waiting, blocked and failed work |
| Operation detail | Exactly how did / is this work progressing? | Orchestration graph (Dominant) | Graph, current state, approval card when a decision is due, operation controls |
| Incidents list | What problems were detected? | Incident table/list (Dominant) | Severity, state, target, host, age, needed attention |
| Incident detail | What happened and what response is being taken? | Evidence/story with related operations (Large) | Problem statement, evidence, state, linked operations/actions |
| Operator side panel | Ask or request something from anywhere | Conversation and current operation summary (Large within panel) | Context, transcript, composer, active work |
| Operator workspace | Manage complex conversations and requested work | Conversation + operation workspace split (Dominant) | Transcript, operation state/graph, evidence, composer |
| Hosts list | Which machines are managed and in what state? | Host table/cards (Dominant) | Connectivity (including `Incompatible`: upgrade required), role, health, active incidents, agent status |
| Host detail | Understand and manage one host | Overview dashboard (Large) | Health, workloads, active issues, capability/policy state, management-path confirmation |
| Capability Center | What can Domovoi do and where is it installed? | Catalog / installed inventory (Large) | Discover, install, enable, update, compatibility and trust |
| Capability detail | What does this capability do and what authority does it need? | Manifest/operation contract (Large) | Purpose, provider, effects, schema, targets, verification, permissions |
| Knowledge browser | What does Domovoi know? | Searchable entity/fact inventory (Large) | Fact, source, scope, freshness, status (current/stale/disputed/superseded/retracted), edit controls |
| Knowledge graph | How are entities connected? | Graph canvas (Dominant) | Selected entity, relationships, provenance, filters |
| Policies & Autonomy | What is Domovoi allowed to do? | Policy matrix/editor (Large) | Inheritance, allow/approval/deny, locked rules, constraints and effective result |
| Policy simulator | What would this policy allow in a real situation? | Scenario/result comparison (Large) | Selected scenario, policy versions, exact deterministic result |
| Integrations (DEFERRED past v1; `docs/rework/06`) | Which external systems are connected? | Integration inventory (Large) | Connection health, scope, actions, reauthentication |
| Attention center | What needs a decision or a look? | Attention list (Large) | Approvals, questions, failures, manual interventions |
| Audit log | Who did what, and is the record intact? | Audit table (Large) | Actor, action, target, outcome, chain verification status (legacy and chained reported separately) |
| System settings | Configure hub-wide behavior | Settings sections/forms (Medium–Large) | Editable settings, source, validation, save/restart impact |

## 3. Control Room / Overview

### Purpose

Answer at a glance: “Is anything wrong?”, “What is Domovoi doing right now?”, and “What needs my attention?” It is a real control room, not merely four statistic tiles and a short list.

### Content plan

| Content | Priority | Space | Behavior |
|---|---:|---:|---|
| Critical attention queue | P0 | Large | Most urgent incidents, blocked operations, failed agents and security warnings; direct links to the right workspace. |
| Active operations overview | P0 | Large | Active operation count and, per operation, only what the engine reports: current phase name, waiting reason and elapsed time. Select one to open it. Do not squeeze a full complex graph into a tiny card. |
| Environment health | P0 | Medium | Hosts online, offline or `Incompatible` (upgrade required; never counted as offline), services with issues, storage pressure and other high-value health indicators. |
| Waiting for you | P0 | Medium | The first items of the attention center: each approval is the shared approval card (compact), others link to their canonical page; age and expiry shown. |
| Recent activity | P1 | Medium | Chronological events across incidents and operations; filter by host/type. |
| Fleet/service summary | P1 | Medium–Large | Host groups, service status, key resource trends. |
| LLM/provider status and usage | P2 | Compact–Medium | Provider availability, usage, latency, tokens/cost and budget pressure. |
| Suggestions and maintenance reminders | P2 | Medium | Knowledge suggestions, capability updates, stale backups, maintenance recommendations. |
| Custom dashboard widgets (DEFERRED to v1.x; `docs/rework/06`) | P1 | Variable | Add/remove/rearrange/resize; user layouts persist. v1 ships one excellent default layout. |

### Interaction requirements

- Clicking any status/count opens a correctly filtered destination.
- Active operation entries update live from reported state; do not claim a stage is active, and show no percentage, unless the engine reported it.
- Users can pin a favorite host, service or operation.
- Widget configuration is separate from operational permissions.
- The overview must not become a wall of charts. Each widget needs a user question it answers.
- A count appears once within a widget and once in the default layout. User-rearranged layouts may repeat a widget; that is the user's choice and not an acceptance failure.
- The autonomy state (paused, rule suspended) is part of environment health and links to Policies & Autonomy.

### Space decision

Start with a responsive multi-column composition. The active work area and attention queue need enough area to be useful. If a full orchestration graph is too large for the available width, show a meaningful active-operation list/preview and navigate to the dedicated operation page rather than rendering an unreadable mini graph.

## 4. Operations list

### Purpose

A unified index of every operation, independent of whether it came from an incident, a chat request, a schedule, an automation or a manual UI action.

### Primary controls

- Status filters: Running, Waiting, Scheduled, Completed, Failed, Cancelled/Stopped and All.
- Origin filters: Incident, User request, Schedule, Automation, System.
- Host/target, operation type, date, initiator, capability and outcome filters.
- Text search and saved views.
- Bulk actions only for safe management actions such as archive/label; never bulk-approve or execute high-impact changes without an explicit reviewed flow.

### List fields

Priority P0: name/goal, status, origin, target/host, started/updated time, current phase or waiting reason, next required action. Priority P1: initiator, related incident(s), cycle count, progress summary and duration. Priority P2: model usage, provider, capability count and historical outcome details.

### Space and navigation

- The list is Large/Dominant; use a table or dense grouped list at wide widths, responsive row cards at narrow widths.
- Selecting an operation opens a dedicated detail page, not a modal.
- Provide saved filters/views and deep links.
- Running/waiting items should not be hidden below completed history by default.

## 5. Operation detail

### Purpose

The primary workspace for understanding exactly what Domovoi is doing or did. Two representations of one trace are in v1: a **step timeline/tree** (built first) and the **interactive orchestration graph**, which is the primary visualization and receives dominant screen space. Both read the same trace schema, frozen with golden fixtures before either is built; neither invents data the other lacks. A non-canvas **outline** of the graph is always available and is the screen-reader path.

### Space allocation (1280 x 720 reference)

| Region | Content | Size |
|---|---|---|
| Header strip | Goal, status, origin, targets, phase/cycle, duration, budget use, what it waits for, operation controls | Compact-Medium, always above everything else |
| Approval card | When a decision is due: the structured approval card, directly under the header strip | Medium, only while a decision is pending |
| Graph canvas | Orchestration graph (timeline/tree selectable as the alternative view) | Dominant; at least 640 x 400 CSS px; below that, outline plus node sheet |
| Right slot | Node inspector or Operator panel (precedence rule, doc 13) | Contextual, 320-480 px |
| Below the graph | Tabs: Findings, Plan, Conversation, Capability calls, Evidence, Logs/output, Model usage, Policy decisions, Audit/events | P1 and P2 content; one tab visible at a time |

P0 content (header strip, approval card) is never in a tab. Findings and plan are P1 and sit in the first two tabs; on screens too small for graph and tabs together, the graph yields to outline plus sheet before P0 yields to anything.

### P0 / dominant

**Orchestration graph** (Dominant):

- Show actual phases, branch structure, parallel activity, loops/cycles, waits, retries/replans, execution and verification.
- Show active node(s), completed and failed nodes, skipped nodes and nodes waiting for external input/approval.
- Preserve individual event/node identity as new events arrive.
- Support zoom, pan, fit-to-view, minimap/overview for large graphs, cycle filter, branch filter and a live-follow toggle.
- Collapsed node groups must communicate count/status and be expandable into exact activity.
- Selecting a node opens an inspector without navigating away.
- Do not flatten a branched/parallel graph into a fake linear progress bar.
- Keyboard navigation and the outline are specified in doc 15, section 4.
- Historical playback of a completed operation is the last U2 deliverable (owner decision D-25). The timeline, the outline and the history are fully useful without it.

**Current state and next action** (Compact-Medium):

- Operation goal, status, origin, target(s) and the pinned subjects they came from, start time/duration, current phase/cycle, budget use and what it is waiting for.
- A step outside the operation's scope is flagged "outside operation scope" and can only be approved, never run automatically.
- If user input/approval is required, present the exact request and controls at the point of need.

**Structured approval card** (one component, used everywhere an approval appears; canonical home is this page):

- Renders from deterministic data: contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, and the policy decision with its winning rule.
- Says what the approval binds. During the migration it carries a label, "v1 binding" (manifest, host, arguments) or "v2 binding" (also script bytes, target identity and execution id; RW-11). The card never claims more than its version binds, and it states what is not bound, for example files a systemd unit references (RW-34).
- Shows an agent refusal: when the agent refuses a step (its mutation rate limit, ceiling, `allow_risk` or an exclusion), the step is `Refused` with the agent's reason, and approving again offers no way around it (owner decision D-27). A policy `Deny` is never offered for approval; the card names the rule and says the owner must change the policy (owner decisions D-23, D-26).
- Model-written text is secondary and labelled as model text.
- One primary action per context; destructive approvals name the target and consequence and ask for confirmation.
- Embedded or linked from incident detail, the Control Room, the Operator and the attention center; they never reimplement it.

### P1 / large or medium

- Findings/hypotheses: facts, inferred hypotheses, uncertainties, evidence links and discarded hypotheses with reasons.
- Current/approved plan: actions, intended effects, preconditions, dependencies, risk, policy outcome, verification and rollback/expiry.
- Related incidents, hosts, services, capabilities and knowledge.
- **Similar past operations:** earlier operations with their resolution summaries (outcome, what was done, verified or not, source of each narrative field). Each is marked "historical, not authorization"; past approvals and successes never count toward the current decision.
- Operation conversation and user messages.

### P2 / tabs or expandable sections

Technical views: Timeline, Capability calls, Evidence, Logs/output, Model usage, Policy decisions, Audit/events. Large raw data lives in tabs and the inspector rather than around the graph.

### Node inspector

Contextual panel; P1 when a node is selected, otherwise closed. It shows:

- Node type/name/status and why it exists.
- Owner component (orchestrator, reasoner, context builder, policy engine, executor, provider, verifier, user).
- Start/end/duration and correlation IDs (P3).
- Inputs and outputs with evidence/provenance references.
- Capability ID/version, target and typed arguments where applicable.
- Result in the shared outcome vocabulary (`docs/rework/04 §3.2`): `Failed` only for attempted-and-failed work, `NotRun` only with positive evidence it never started, `Unavailable` when it could not be attempted (host absent or `Incompatible`), `OutcomeUnknown` when it may have run; plus error, retry/replan relation and next node(s).
- LLM provider/model/token/latency data when relevant.
- Policy matched rules and constraint result when relevant.

The user may inspect any completed or running node; opening an inspector must never change the operation.

## 6. Incidents list and detail

### Incident list

Retain the current ability to filter/group by status/severity and quickly find an incident, but plan the future around links to operations.

P0 fields: severity, title/problem, host/target, current incident status, last firing/opened time, whether action is needed, related active operation. P1: check/source, recurrence count, acknowledged state, last update. P2: detailed signal/evidence count.

A dense table/list should support saved filters, grouping, sort, pagination/virtualization and responsive rendering. Avoid repeated counts/status labels in multiple places.

### Incident detail

The incident describes the problem, not the work engine.

P0: problem statement, severity/state, target/host, current measured facts, key evidence, whether problem persists, related active operation and user attention request.

P1: related operations (one or several), incident timeline, recurrence, similar past operations with resolution summaries (marked historical, not authorization), relevant knowledge and recommended next step. A pending approval for any related operation is shown as the shared approval card, embedded or linked to its operation.

P2: raw signal payloads, complete logs, check metadata and detailed source information.

The page should show the operation graph as a substantial linked preview or embedded workspace only when the operation is central to resolving the incident. Do not duplicate the entire operation page inside incident detail. Selecting “Open operation” leads to the canonical operation workspace.

Keep incident-specific actions such as acknowledge/mute/dismiss clearly separate from operation controls such as stop/cancel/retry. Distinct concepts must not share ambiguous buttons.

## 7. Operator side panel

### Purpose

Fast conversational access from any page, without replacing the dedicated Operator workspace.

P0: current conversation, composer, selected context, request/stop control, active operation status and approvals/questions relevant to this conversation (approvals render the shared approval card).

P1: recent conversations, recent operations, starter prompts, scope selector and “open full workspace”.

P2: model/provider metadata and technical tool details, accessible by expanding the relevant message/operation.

### Behavior

- Opening the panel does not navigate away or discard current page state.
- Starting a request creates/attaches to a durable operation when work is required; pure informational conversation need not create an operation.
- User can start a task, then continue browsing; the operation continues independently.
- Expand into full workspace with same conversation/task IDs and scroll position where sensible.
- Side panel can be resized on desktop; mobile uses full-screen sheet/workspace behavior.
- It must communicate the active scope. “All hosts” is not the same as one host or selected incident. Scope is explicit per conversation.
- Conversations are durable. A stale response for a superseded request is discarded, and the composer draft is kept until the send is committed.
- It shares the right-hand slot with the inspector under the precedence rule in doc 13.

## 8. Operator full workspace

### Purpose

A full environment-assistant workspace for extended investigation, guided configuration and multi-step operations.

P0/Dominant: conversation transcript/composer and operation view. Recommended split: conversation column plus operation/graph workspace, with a user-resizable divider. If there is no active operation, conversation receives the available area and contextual suggestions are shown.

P1: operation list/context selector, evidence panel, selected object context, findings and next requested user input.

P2: conversation metadata, model usage, complete tool call traces and export/share controls.

Tabs or view switch: Conversation, Operation, Evidence/Results. Avoid making the user choose between seeing the conversation and understanding active work; wide layout should support both when a task is active.

## 9. Hosts list and host detail

### Hosts list

P0: name, agent/connectivity state (including `Incompatible` and "expected incompatible": upgrade required; and a root-agent warning where the agent runs as root), host role/criticality, health summary, active incidents and version warning. P1: resource summary, tags/groups (display only; host tags in policy are DEFERRED, `docs/rework/06`), recently active capability/operation. P2: inventory details.

Support cards and dense table as alternative user views; filters by state, role, tag, agent version, incident state and capability coverage. Adding/enrolling a host remains an obvious primary action.

### Host detail

One canonical page per host. Recommended high-level tabs (exact grouping to validate in prototype):

1. **Overview** — P0 health, host identity, live agent state, active incidents and operations, resource/health highlights, important services, storage and network summary.
2. **Workloads** — services, containers, processes and their dependencies; actions link to capability/operation surfaces.
3. **Capabilities** — installed, enabled, compatible and available capabilities; install/configure/update/uninstall paths.
4. **Monitoring** — checks, schedules, thresholds, alert sources and health history.
5. **Network & security** — interfaces, ports, SSH activity, firewall/security capabilities, management-path confirmation (below) and security events.
6. **Knowledge & dependencies** — confirmed facts, assumptions, upstream/downstream relationships, notes and recovery instructions.
7. **Activity** — operations, incidents, capability runs, configuration changes and agent events.

Logs may remain a standalone global view with host filter and also be reachable from Workloads/Activity. Do not create a separate tab solely to expose a large raw log dump unless user workflows justify it.

The host page shows the **host-local unattended ceiling** read-only (the most the host itself allows to run without a person), with the explanation that it is edited in the agent's configuration on the host, not here. Policy can only narrow it.

**Incompatible agent (owner decision D-20, RW-17).** When the agent speaks another protocol minor than the hub, the host page leads with an upgrade banner above the tabs (P0): the exact required protocol, the agent's version and the upgrade order (hub first, then each agent through the install procedure). The state is read from the persisted agent record, so it survives a hub restart; after a hub upgrade a host that has not reconnected shows "expected incompatible (last reported X, hub requires Y)". History, incidents and operations stay readable; nothing is dispatched and the host is never shown as offline. Steps waiting on it show `Unavailable` or keep waiting for a compatible agent.

**Root agent and helper (owner decision D-29, RW-39).** When the agent runs as root, the host page shows a warning above the tabs: the privilege boundary is not in effect, the host gets no unattended execution, and rerunning the installer moves it to the unprivileged user. When the helper is absent or in error, the Monitoring and Capabilities tabs mark each affected check or capability "helper unavailable" with the reason, distinct from a failure. The helper and its allowlist are shown read-only (they are edited on the host).

**Management paths (owner decision D-28, RW-37).** The host page shows its management paths (management networks, VPN endpoints, remote routes) with where each came from, how fresh it is, whether the owner has confirmed it, and which containment is blocked because paths are unknown, stale, conflicting or not covered. The owner confirms or corrects a path here (an audited knowledge edit). The **agent-reported exclusions** (their hash and summary) are shown read-only beside the confirmed paths, with each confirmed path marked covered or uncovered; a mismatch, or any network or exclusion change, blocks unattended containment until the owner re-confirms. Monitoring and human-approved work are unaffected.

Host actions (rename, enroll/revoke, maintenance, agent upgrade guidance since the agent never updates itself) should be discoverable in page-level actions, with destructive actions contextual and confirmed.

## 10. Capability Center

### Main page

P0: Discover catalog, Installed inventory, deployment/installation status and capability health. These can be tabs/views under one Capability Center because they are peer views of the capability inventory.

Recommended tabs:

- **Discover** — searchable catalog of first-party and owner-signed packages (third-party registries are DEFERRED) with categories, trust/provenance, compatibility, requested permissions, version and available targets. An **Import package** action lets the owner upload an owner-signed archive and its detached signature (the same as `domovoi package import` on the hub); that is the only way an owner-signed package enters the catalog (RW-26).
- **Installed** — capabilities installed on agents/providers, status, version, enabled state, last used, updates and policy coverage.
- **Deployments** — install/update/uninstall attempts, target agents, progress, failures and rollback history.

P1: capability groups, filters by host/platform/provider/effect/risk, compatibility warnings, dependency graph and update channel.

### Capability detail page

Dedicated page for non-trivial capabilities; not just a dialog.

P0: purpose, provider/source, trust/signature, version, supported targets, prerequisites, effects/risk, typed inputs/outputs, privileges/permissions, timeouts, verification and rollback/expiry.

P1: compatibility matrix, dependencies, configuration, example invocation, documentation, installed targets and policy mapping.

P2: raw manifest, source files/package provenance, developer diagnostics and version history.

### Install/configure flow

A guided wizard or dedicated deployment workspace. The package backend (P1) is built separately; the wizard is part of the Capability Center work in U4, after the visibility infrastructure (U3).

1. Select capability/package and version/channel.
2. Review provenance, signature, requested privileges/effects, data access and network needs.
3. Select targets (one/many agents or integrations) and rollout strategy.
4. Preflight compatibility, dependencies, resources and existing versions.
5. Show exact planned changes and possible interruption/side effects.
6. Install, verify package integrity and register locally.
7. Configure parameters and enable/disable separately.
8. Set policy permissions separately from installation and enabled state.
9. Run a safe test/health check.
10. Show result and allow rollback/uninstall where supported.

**Critical distinctions:** installed ≠ enabled; enabled ≠ authorized; authorized ≠ skipped local validation. No control installs and enables in one step; each is its own action, and enabling and starting an installation each ask for password re-entry.

**Trust model shown in the UI:** packages are signed archives. The hub only relays the bytes; the **agent** verifies the signature against keys in its own local trusted-key list and installs only if the host has opted in (off by default) and the package is compatible. The UI shows, per host: opted in or not (with where to change it), which key signed the package (the signer's `key_id` is kept per installed package, so a revoked key shows which packages it signed), and the verification result reported by the agent, including a refused archive (limits exceeded, a link, an unsafe path). A host that has not opted in is shown as "does not accept installs", not as a failure. v1 scope (owner decision D-16): first-party signed packages plus packages signed with a key the owner added locally on the agent. The hub cannot add or replace keys, the signature is detached and verified before extraction, and a rollback must be signed by a currently trusted key. Third-party registries are DEFERRED (`docs/rework/06` "Deferred and after-v1"). Packages run as the agent user, never root, and cannot install or modify the root-owned helper; the UI says so on the package review step (owner decision D-29, RW-26).

**Progress and recovery:** the deployment view shows one row per host (queued, delivered, verifying, installed, failed with the agent's reason, pending because offline), the overall result as "N of M hosts", retry for failed hosts only, and uninstall/rollback where the package declares it.

AI-generated capability drafts may be reviewed and tested, but the model cannot self-install, grant itself new privileges or alter its own trust roots.

## 11. Knowledge browser, entity detail and graph explorer

### Knowledge browser

P0: searchable list of entities/facts, source/provenance, host/scope, verification/freshness, status (no numeric confidence, RW-14) and direct edit/correct/delete controls.

P1: filters by category, entity type, source, scope, freshness, inferred/confirmed/disputed state and related incident/operation.

P2: bulk review, merge suggestions and data lifecycle tools.

The UI must clearly distinguish confirmed environment fact, automatically discovered fact, inferred hypothesis, user preference, operational constraint and temporary observation.

Resolution summaries are shown here and on operation detail with the source of each field. They belong to the Operation, and the operation writer is their only writer: an edit in the Knowledge UI is sent as an edit command with the revision it was based on, a stale edit is rejected and shown as such, and code-owned fields cannot be edited by a person (RW-41).

### Entity detail

P0: entity identity and type, current known facts, source, freshness, verification and relationships. P1: linked notes, incidents, operations, capabilities, policy references and history. P2: raw metadata/attributes and edit history.

### Knowledge graph explorer

Dominant canvas plus contextual inspector. Support bounded traversal, entity-type and relationship filters, search, layout controls, focus mode and selection. Never render every log line/metric as graph nodes. The graph is for meaningful entities and relationships.

### Notes/runbooks

Keep Markdown notes as a distinct document type. Add entity/host references, tags, full-text search, editing, revision awareness and links from incidents/operations/capabilities. Do not force unstructured runbook text into fact records.

### Suggested knowledge

Present proposed learnings with evidence and suggested scope. User can accept, edit, reject or defer. Accepting a fact must never grant operational permission.

## 12. Policies & Autonomy

### Policy overview/editor

P0: effective policy by host and capability/operation, clear Allow automatically / Ask for approval / Deny semantics, inherited versus explicit values, rule precedence, locked rules shown distinctly and a "why" explanation (below); the autonomy pause switch and any rule suspended by its circuit breaker; rules in shadow mode (new rules start there) clearly marked as not yet acting.

P1: conditions, target restrictions, time windows, TTL, rate limits, blast-radius caps, trusted paths, recovery requirements and policy versioning. Publishing a policy version asks for password re-entry and first shows the **publication validation report**: equal-scope rules with different outcomes (resolved to Deny) and incomparable conflicting rules (resolved Deny > RequireApproval > Allow), each of which the owner must acknowledge, specific Allows that override a broader Deny, and any locked rule affected (owner decisions D-19, D-26).

P2: raw rule representation, import/export and policy-as-code support later.

A matrix view is recommended for host-by-capability review, but complex condition editing needs a dedicated policy detail/editor rather than an unreadable mega-table.

### Effective-policy explanation

For any host, capability and context the page answers "why this result?" using the production evaluator, never a re-implementation. It shows:

- **All matching rules**, each with its effect (Allow, Ask, Deny) and specificity.
- **The winning rule** and **why it won**, as one of: strictly more specific than the others; equal specificity where Deny wins; incomparable specificity resolved by Deny > RequireApproval > Allow; or a locked Deny.
- **Which broader rules it overrides.**
- **A locked Deny (owner decision D-26) shown distinctly** from ordinary rules, with a lock icon and the text "Locked Deny" (never colour alone), in the matrix, the editor, the simulator, the explanation and the trace, together with what ordinary resolution would have decided. No narrower Allow can override it; changing or unlocking it asks for step-up.
- **Deny is never offered for approval (owner decision D-23).** A `Deny` result says the owner must change the policy and shows no approval control; `RequireApproval` is offered to the owner.
- **Hard limits shown separately from rules**: the host-local ceiling (read-only, edited on the host), the global pause, rate limits and target binding. A hard limit is never presented as a rule that could be edited here.
- **Dangerous exceptions made obvious**: a specific Allow that overrides a broad Deny is marked with a warning tone, an icon and the text "Allow overrides a broader Deny", in the matrix, the explanation and the editor's diff before publishing.
- Missing or unavailable context is named; the result may be "cannot determine".

### What-if policy simulator

Dedicated page/panel with:

- Scenario selector: historical incident/operation, saved scenario or manually defined context.
- Policy selector: original policy at the time, current policy, draft/proposed policy.
- Reconstructed context: target, evidence, available capabilities, policy version, relevant host/operation metadata and missing data.
- Deterministic evaluation output for each proposed operation: allow automatically, require approval, deny, unavailable or cannot determine, with the full effective-policy explanation above.
- Side-by-side differences and consequence summary.
- Save/apply draft policy after review, not from the simulator result alone.

The simulator calls the production evaluator through a side-effect-free path. It is read-only and must never execute a capability. A historical scenario needs the stored context snapshot and policy version recorded when the decision was made (a backend requirement, doc 17 section 11); missing historical facts are labelled, and the page never silently simulates with today's environment and claims it was historically exact.

## 13. Integrations (DEFERRED past v1)

Hub-side integrations need hub-held credentials and their own contracts, so they are not in v1 (`docs/rework/06` "Deferred and after-v1"). The page is specified so that navigation and contracts leave room for it.

P0: connected state, provider type, configured scope, last successful access, errors and primary actions (configure, test, reconnect, disconnect).

P1: catalog/add integration flow, permissions/scopes, credentials set/replace without exposing stored secret values, capability mapping and health history.

P2: detailed provider logs, rate limits, API quotas and advanced auth settings.

Integrations are providers, not a second execution engine. Their capabilities feed the common capability registry and operations system.

## 14. System and settings

Settings should be grouped by the user's goal, not by implementation crate:

- General hub behavior and URL/listener/network configuration.
- Models and AI providers (primary/fallback, context limits, budgets, cost controls).
- Notifications and channels.
- Agent enrollment/trust and upgrade-required status (agents never update themselves).
- Data, retention, backups and restore. Every retention value is editable (raw data and durable summaries are separate classes); the page states what each change deletes and when, says that deleted data remains in backups until they rotate (up to 7 nightly and the last 3 pre-migrate copies; owner decision D-21), and changing retention asks for password re-entry.
- Account/security/session management.
- UI preferences (theme, density, presentation profile, dashboard layouts, shortcuts).
- Updates, diagnostics and system health.

P0: UI-editable fields with validation, current value/source, defaults and save feedback. P1: advanced controls and safe restart/roll-back flows. P2: raw config export and advanced diagnostics.

A value should be read-only in the UI only for a documented safety/trust reason or because config-as-code mode explicitly owns that value. When possible, expose safe UI lifecycle operations even for protected secrets/certificates (set, replace, test, rotate, revoke) without displaying the private material.

## 15. Logs, audit and history

### Logs

P0: source/host, severity, time range, search, live/pause, clear stale indicator, and safe presentation of redacted output. P1: operation/incident correlation and structured field filters. P2: raw structured event export and advanced queries.

### Audit log

P0: who/what/when/target/outcome, exact policy/action transition, and the **hash-chain verification status**, reported in two parts: **legacy unchained history** (anchored by a digest: count and digest match, or "not verifiable (pruned)") and **chained history** (last verified time, result, head hash, the first broken link and any checkpoint left by pruning). A failed chained verification is a page-level danger banner naming the first bad entry; a legacy mismatch is shown as such and does not say which row changed. P1: filters by actor, host, operation, capability, policy and date; links back to the original object. P2: export, retention and a "verify now" control. The page states what the chain detects (accidental corruption, edits that do not recompute it, deletion mid-chain) and what it does not (an attacker who rewrites the whole database).

The audit view is not the same as the user-facing operation timeline. Operation timeline explains work; audit captures security-sensitive changes and authority decisions. They should link to each other.

## 16. Notifications and attention center

A dedicated attention center is the single list of pending approvals, questions, manual interventions, warnings and important completion/failure notices. The Control Room "Waiting for you" widget and the Approvals page are views over this same list, not separate queries with separate rules. An approval item renders the shared approval card or links to its operation; every other item links to its canonical object. Users can configure channels, priority, grouping, quiet hours and preferences. A toast is transient feedback, not the only durable record of something requiring action.

## 17. Page design deliverable template

Every implementation-ready page specification must include:

1. Page purpose and user questions.
2. Entry points, exit points, canonical route and related objects.
3. Page sections/controls with P0–P3 priority and space class.
4. Recommended containment: page, subpage, tabs, section, expansion, inspector, dialog or popover—with rationale.
5. Interaction matrix: action, trigger, result, confirmation, state change and navigation.
6. Live update contract and what may update without user action.
7. The states from the canonical taxonomy (doc 15, section 10) that apply. Permission-denied states are not part of v1 (single owner); name the hook for future roles instead.
8. Responsive behavior and presentation-profile behavior: the P0-P3 category of every element, and what Standard and Essential collapse or hide.
9. Keyboard, focus, screen-reader and reduced-motion behavior.
10. Animation/motion references from the global motion specification.
11. Data needed, freshness and source of truth.
12. Performance constraints and acceptance criteria.
13. Open questions and explicit decisions.

---

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
| Graph playback | User-controlled | Consistent playback clock | Replay of a completed operation; speed is chosen by the user. The last U2 deliverable (owner decision D-25). |

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
- Refused (by validation, policy or the agent, with the reason, for example a rate limit).
- Not run (positive evidence it never started, or its approval expired) and Unavailable (it could not be attempted, for example the host is absent or `Incompatible`), each distinct from Failed.

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
- Enter historical playback mode for completed operations: replay of the stored trace through the same renderer, with play/pause, scrub and a user-chosen speed. Playback is the last U2 deliverable (owner decision D-25), built after the live graph; the timeline, the outline and the history are fully useful without it, and it never changes the operation.

## 5. Operation and incident state transitions

- A state transition must be visible where the user is currently looking and recorded in durable history.
- Avoid optimistic UI that says an operation succeeded before the server/agent reports a confirmed result.
- Distinguish “command returned successfully”, “verification passed” and “incident resolved”.
- On a lost connection after dispatch, show “outcome unknown / checking state” rather than immediately showing success or issuing a duplicate action.
- A host that becomes `Incompatible` after dispatch leaves its in-flight step awaiting a compatible agent; the step is neither failed nor shown as offline (owner decision D-20). The same holds after a hub restart: the state is read from the persisted agent record, not from a live connection.
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

A response that creates a proposed action/operation renders the shared structured approval card (doc 14, section 5), not a free-text promise. It shows contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, whether the approval binds the v1 or the v2 plan hash during the migration, the policy decision and approval state; any model prose is secondary and labelled. Chat must not simulate success before execution has returned. An agent-side refusal (for example a rate limit) shows on the card with its reason.

## 7. Policy simulator interactions

- Selecting a historical incident loads the stored snapshot and policy version used at the time.
- Current-policy comparison uses today's policy, but clearly states which environmental facts are historical and which are current.
- Proposed-policy comparison operates on a draft; it does not save automatically.
- Results show exact matched rules and constraints, including the winning deny/allow rule and any unknown/missing context.
- Every scenario run is read-only. Applying a draft policy is a separate explicit action with review and audit.
- The “simulate” result must call the production policy evaluator through a side-effect-free path.
- Every result renders the full effective-policy explanation (doc 14, section 12): all matching rules, the winning rule and why it won, broader rules overridden, hard limits shown separately, a locked Deny shown distinctly (with what ordinary resolution would have decided), dangerous exceptions marked.
- Loading a historical scenario requires the context snapshot and policy version stored at decision time; if absent, say so and offer only the current-policy comparison.

## 8. Capability installation interactions

- Installation uses preflight → review → target selection → deploy → verify → configure → enable → authorize.
- Progress is shown per target agent, not only as a global spinner. The agent verifies the package signature against its local trusted keys; the UI reports what the agent said and never claims verification itself.
- A host that has not opted in to installs is shown as such, with where to change it, not as a failure.
- Partial success must be clearly represented; allow retry only for failed targets, not duplicate the entire fleet deployment.
- Signature/source verification and privilege changes must be conspicuous.
- A package signed by a key the agent does not trust, or an older version whose key is no longer trusted, is rejected by the agent and shown with that reason (owner decision D-16).
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
| Incompatible | A host's agent speaks another protocol minor than the hub (owner decision D-20), or has not reconnected since a hub upgrade changed the protocol ("expected incompatible") | Upgrade-required banner naming the required protocol and the agent's version; history stays readable; never presented as offline | last known, marked |
| Helper unavailable | The privileged helper is absent or in error on a host (owner decision D-29, RW-39) | The affected check or capability says "helper unavailable" with the reason; distinct from failed and from offline | last known, marked |
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

---

# 16 — UI design system, visual language and quality requirements

**Status:** preserve the useful visual identity and engineering discipline established in the current overhaul; redesign layouts and components where product requirements demand it.

## 1. What to preserve from the current visual identity

The current `web/DESIGN.md` and `src/styles/tokens.css` establish a deliberate identity that the user likes. Preserve its roles and semantic approach unless a new design prototype proves a better option:

- Warm charcoal/sand dark theme and paper/ink light theme.
- Moss green as the action/accent color.
- Walnut as a restrained brand marker.
- Distinct semantic status tones: danger, warning, success, pending, progress, info and neutral. New statuses (the `Incompatible` host state, the root-agent warning, "helper unavailable", the v1 versus v2 approval-binding label, the locked-Deny treatment, and the `Refused`, `NotRun`, `Unavailable` and `OutcomeUnknown` outcomes) each get a label, tone, hint and icon through the shared status vocabulary (see the planned section of `web/DESIGN.md`).
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

---

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
- A host whose agent is `Incompatible` (or "expected incompatible" after a hub upgrade) appears in the attention queue and in environment health as upgrade required, distinct from offline.
- A host whose agent runs as root appears with a warning that the privilege boundary is not in effect and that it gets no unattended execution.
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
- A locked Deny is shown distinctly in the editor, simulator, explanation and trace, with what ordinary resolution would have decided, and changing it needs step-up. A policy Deny shows no approval control: the owner must change the policy.
- Publishing shows the publication validation report, and both equal-scope and incomparable conflicts must be acknowledged.

## 5. Journey: install a capability from the UI

**Goal:** add new functionality to one or more agents safely.

1. Open Capability Center and search the catalog, or import an owner-signed package (archive plus detached signature).
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
- A package signed by an untrusted key is rejected by the agent and shown with that reason; the trusted keys are the first-party key and keys the owner installed on that host (owner decision D-16). An archive that breaks the limits or contains a link is likewise rejected and shown with its reason.
- A package never installs or changes the root-owned helper; it runs as the agent user (owner decision D-29).
- Failed agents can be retried without reinstalling successfully updated agents.
- Model-generated capability drafts cannot self-install or self-authorize.

## 6. Journey: inspect what happened after an operation completed

**Goal:** understand all decisions and outcomes after the fact.

1. Open Operations and filter Completed/Failed.
2. Open an operation.
3. View the historical graph, switch to the timeline/tree or the outline, or, once it ships, play it back (the last U2 deliverable, owner decision D-25; the timeline and history do not depend on it).
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
2. Open the item; the shared approval card shows contract, target, arguments, effect class, reversibility, expiry, plan-hash prefix, the plan-hash version it binds (v1 or v2 during the migration) and the policy decision.
3. If the target is outside the operation's scope, the card says so and offers approval only.
4. Open the operation from the card to see the graph and the evidence behind the step.
5. Approve (or deny); the card and the operation show the decision, who made it and when.

**Acceptance:**
- The same component renders the approval on operation detail, incident detail, the Control Room, the Operator and the attention center.
- Model-written text is labelled and secondary.
- Only an authenticated person can approve; nothing in the UI, the Operator or chat approves on a person's behalf.
- An expired approval is shown as expired with the reason, not as pending.
- When the agent refuses a step (for example its mutation rate limit), the card shows `Refused` with the agent's reason and offers no way around it (owner decision D-27).

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
- An `Incompatible` host shows the same upgrade-required banner and explanation on its host page, in the attention center and in environment health, also after a hub restart; its history stays readable and it is never shown as offline (owner decision D-20).
- A host page shows management paths with provenance, freshness and which containment is blocked, beside the agent-reported exclusions (hash and summary) with each path marked covered or uncovered, and the owner can confirm or correct them (owner decision D-28).
- A host page warns when its agent runs as root, and marks checks and capabilities "helper unavailable" where the helper is absent or in error (owner decision D-29).
- The audit page reports legacy unchained history and chained history separately (owner decision D-14).

## 11. Backend contracts the UI depends on, and build order

The UI is built against contracts, not against the live engine. These must exist (as schemas with fixtures) before the pages that need them:

- **Trace schema:** operations, nodes, branches, cycles, phases, planned-versus-happened, uncertain outcome, versioning. Frozen with golden fixtures that both the Rust side and the UI test, before the timeline or the graph is built.
- **Operations record and list/detail endpoints**, with durable ordered events per operation.
- **Live events with ids and resume** per topic (operations, hosts, attention), replacing invalidate-by-prefix; keep invalidation as the fallback.
- **Conversations:** durable messages, explicit scope, request ids for discarding stale responses.
- **Policy decisions** stored with the context snapshot and policy version used, so the simulator can reproduce a historical decision.
- **Attention feed**, **audit API with chain verification reporting legacy and chained history separately**, **retention settings**, **resolution summaries and similar-operation search**, **capability deployment state per host**.
- **Host protocol state:** `Incompatible` or "expected incompatible" with the required and reported protocol and the agent's version, read from the persisted agent record so the banner and host page render from data (owner decision D-20).
- **Host privilege state:** whether the agent runs as root and the helper's status (`absent`, `ok`, `error`), so the root warning and "helper unavailable" markers render from data (owner decision D-29).
- **Management-path facts** with source, confirmation, freshness and the containment they block, the agent-reported exclusions (hash and summary) with per-path coverage, plus the host's read-only ceiling summary (owner decisions D-28, D-18).
- **Policy publication validation report** (equal-scope and incomparable conflicts, overrides of a broader Deny, locked rules) and the locked flag on rules in the trace (owner decisions D-19, D-26).
- **Agent refusal reasons**, including rate-limit refusals (owner decision D-27), and the shared outcome vocabulary (`docs/rework/04 §3.2`).
- **Presentation profile and dashboard preferences:** where they are stored (hub or browser) is a backend decision; the UI reads them through one provider.

Build order for UI work:

1. Shared foundations: live layer, freshness and stale states, motion tokens, virtualised list, right-hand slot and inspector shell, and the shared visibility infrastructure.
2. **Presentation profiles and the category table**, before any page is implemented.
3. Trace schema and fixtures; then the step timeline/tree; then the graph, outline and keyboard navigation; playback last (a late U2 deliverable, owner decision D-25).
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

---

# Lessons from Domovoi's Closed Pull Requests

**Repository reviewed:** `RoastSlav/domovoi`  
**Review date:** 2026-11-12  
**Scope:** all 113 closed pull requests in this repository (PRs #1–#113; every number is a closed PR). The first pass (2026-11-12) covered #1–#111 and counted 110; #112 and #113 were merged afterwards and #108 was missed (it is the merged `chore: release v0.0.10`). Titles and descriptions were reviewed across the full set. Conversation comments were checked where present; the substantive maintainer follow-up comments on #107 and #110 were included. Code changes were inspected selectively where the description exposed concurrency, security, persistence, protocol, UI-system or operational-recovery risks. Routine dependency-only and release-only changes were classified but not treated as product-design evidence.

**Verification pass.** The lessons below were re-checked against the code at baseline `64117f0` (investigation G, with E and F). Each lesson carries a status:

- **VERIFIED**: the PR text and the current code or the PR itself agree (code or PR evidence cited).
- **STATIC**: established by reading code only. Nothing was executed and no failing test reproduces it.
- **UNVERIFIED**: taken from PR text; not checked against the code.

Several claims in the first version overstated what the repository enforces. They are corrected in place and listed in "Corrections from verification" below. Target rules ("Target rule:") describe what the rework should do; they are not claims about current behaviour.

This document is a lessons-learned record, not a requirement to preserve the current implementation. The rework may replace mechanisms, but should not repeat known failure modes or discard proven product requirements without a reason. Decisions that carry each lesson into the target design are in `docs/rework/03-architecture-decisions.md` (`RW-NN`); the "Rework invariants" section at the end maps invariants to them.

## Corrections from verification

| Earlier claim | Corrected statement | Status |
|---|---|---|
| "110 closed PRs", "#108 no closed PR" | 113 closed PRs. #108 is the merged v0.0.10 release. #112 and #113 are added below. | VERIFIED (`gh pr view`) |
| The mutation-feedback lint enforces feedback on every mutation | The ESLint rule is file-level: a file passes if it imports `useActionFeedback`, `InlineMessage` or `InlineError` anywhere, so one reported mutation excuses an unreported one in the same file (`web/eslint/domovoi-rules.js:10,128`). Four files carry a `// feedback-ok:` exemption. | STATIC |
| Unsaved-change warnings exist for settings and forms | The leave guard (`useLeaveGuard`) is used by the notes editor only (`web/src/features/notes/useLeaveGuard.tsx:16`). No other form blocks navigation while dirty. | STATIC |
| #97 "checksum-verified" upgrade | `domovoi upgrade` compares the archive with a `.sha256` file fetched from the same release origin (`crates/domovoi/src/cmd/upgrade/package.rs:13-30`). That detects corruption, not a hostile or compromised release. The SLSA attestation produced by the release workflow is not checked by the upgrade command (no attestation or signature code under `cmd/upgrade/`). | STATIC |
| `HumanApproval` is sealed by the type system | It is not. `HumanApproval::new` and `restore` are `pub` in `domovoi-core` (`core/src/action/approval.rs:54`). The restriction is a line-based text scan in `xtask arch-check` for the strings `HumanApproval::new(` and `HumanApproval::restore(` outside allowed path prefixes (`xtask/src/arch/scan.rs`, allow-lists at `xtask/src/arch/rules.rs:169-175`). A renamed import or a macro evades it. The type has no `Default` and no `Deserialize`, and gate tests exist. RW-11 in `docs/rework/03-architecture-decisions.md` records this correction. | VERIFIED (code) |
| Approval binds to "the exact plan" | The plan hash covers the serialised manifest, not the entry script's bytes (`core/src/skill/manifest.rs:145-151`; `runner/src/catalog/checks.rs:100-111` re-checks mode and path, not contents). The approve API (`hub/src/api/actions.rs:104-123`) takes only `confirm_text`; it carries no plan hash, so the server cannot tell what the operator saw. | STATIC |
| Single writer "guarantees" serialised decisions | True for action approve, deny, expiry, dispatch and completion. Not true for proposal token rotation (called from the triage task, `triage/proposals.rs:115`, `followup.rs:252`). The incident store has an unguarded `IncidentRepo::update`; its single-writer rule is a convention (ADR 0011). | STATIC |
| Restart recovery "resumes approved actions" | The mechanism exists and is tested with a fake link, but a real hub probably rejects them (D36 in `docs/rework/01-repository-reality.md`). See lesson 2. | STATIC (not reproduced at runtime) |
| Fallback "records model identity" | Only failure text carries provider and model. A successful `TriageReport` carries neither, and usage rows store only the provider id. | STATIC |
| #36 findings "to evaluate against current main" | They are still present on main (lesson 6). | STATIC |

## Executive summary

The repository's history contains several important lessons for the target design:

1. **Concurrency and durable state need explicit ownership.** Action decisions and action transitions were vulnerable to races until a single-writer model and guarded updates were introduced (#107, #109). Cached proposals also needed to be revalidated and converted back into live actions (#110).
2. **Restart recovery is part of correctness, not an operational afterthought.** Do not blindly replay actions whose execution outcome is unknown. Reconstruct rate limits and pending state from durable history; distinguish approved-but-not-started from dispatched-but-uncertain work (#109). The action loop guard is rebuilt from audit; the triage hourly cap (#112) is not, and catch-up of approved actions probably runs before agents reconnect (STATIC, lesson 2).
3. **An AI answer, cached report, notification or UI card is not the source of truth.** Revalidate against current capabilities, policy and target state at the point of action. UI affordances must reflect the current action token/state (#93, #107, #110).
4. **The UI needs explicit state design and quality gates.** Loading, empty, no-match, offline, partial failure, retry, unsaved changes, pending mutation and mutation failure are distinct states, not variations of an empty page (#100–#105, #111).
5. **A design system must be enforceable.** Semantic tokens, centralized status vocabulary, accessible components, contrast tests, axe checks, mutation feedback and bundle budgets prevent visual and interaction drift (#98, #100, #105). Enforcement today is partial: the mutation-feedback lint is file-level, not per call site, and only the notes editor blocks navigation with unsaved changes.
6. **Long-running and unreliable dependencies are normal.** Local models can be slow; providers can fail or return unusable answers; agents can disconnect; notification delivery can fail. Timeouts, streaming, fallback chains, durable queues and honest user-visible status must be designed in (#36, #50, #52, #54, #71, #87–#90).
7. **The agent must remain an enforcement boundary.** Hub requests are untrusted proposals; the agent revalidates local catalogue/policy and the runner constrains process execution (#6, #10, #13).
8. **Tests should prove invariants and contracts, not just happy paths.** Pure state machines, repository contract suites, protocol snapshots, adversarial gate tests, fake clocks and explicit regression tests make later redesign safer (#5–#13, #15–#16, #105, #107, #109–#110).
9. **The new product needs stronger abstractions than the old UI, but must keep proven user-facing behaviors.** Per-host capability enablement, host profiles, readable known-condition rules, rich logs, visible tools used, follow-up conversation history, manual steps and update guidance are useful foundations to carry forward conceptually (#63, #65–#69, #79–#84, #87–#95, #96–#97).
10. **A feature is not complete when its backend endpoint exists.** It also needs clear feedback, recovery paths, docs, tests, generated API artifacts and behavior on mobile/offline/error states.

## Lessons that directly affect the target architecture

### 1. Single-writer ownership for mutable workflows

**Evidence:** #16 established a single-writer incident manager; #107 moved action approve/deny/expiry decisions behind one writer; #109 extended that ownership to every action transition and startup recovery.

**Failure mode:** when multiple request handlers, expiry sweeps or executors independently read and write the same action, stale copies can overwrite a newer decision. The audit log can then describe a transition that the database did not actually retain. Approve and deny can race; expiry can overwrite a human decision; shutdown can close the database while queued decisions are still pending.

**Target rule:** every important state machine has one authoritative transition owner. Callers submit commands; the owner loads current state, validates the transition, performs a guarded write, records the corresponding audit event and publishes the resulting state. Database updates must be conditional on the expected prior state/version. A stale decision returns a conflict, not a silent overwrite.

**Status: VERIFIED for actions; STATIC for the caveats.** `actions/writer.rs:95,132`, guarded SQL `store/repos/actions.rs:173-178` (`WHERE id = ? AND json_extract(status,'$.type') = ?`), `update_from` is the only action write in the port. Caveats: the guard compares the status name, not a version, so ordering between two writes from the same status depends on the single writer; proposal token rotation calls `update_from` from the triage task (`triage/proposals.rs:115`) and bypasses the writer; the incident store has an unguarded `IncidentRepo::update` (`manager/actor.rs:259`), so incident single-writer is a convention (ADR 0011). Rework decisions: RW-03, RW-04.

**Apply to the rework:** incidents, operations, approvals, autonomy decisions, capability installation, agent enrollment, and long-running orchestration should each have explicit transition ownership. Do not automatically put all domains in one global writer; use the smallest ownership boundary that guarantees serializable decisions.

### 2. Crash recovery must distinguish safe resume from unknown execution

**Evidence:** #109 recovers approved actions once at startup, fails actions still marked dispatched with an explicit “check the host before running it again” reason, and rebuilds the hourly dispatch guard from audit history. #36 (closed without merge) proposed durable spool/notification hardening and corruption handling.

**Failure mode:** a process may crash after an external command started but before its result was persisted. Replaying it can duplicate a destructive or non-idempotent action. Resetting an in-memory rate limiter at restart can bypass safety limits. Conversely, failing every pending item loses approvals that were durable but not yet dispatched.

**Target rule:** persist operation checkpoints and action lifecycle transitions. On restart, classify work as safe-to-resume, safe-to-retry, needs-reconciliation, or outcome-unknown. Never replay outcome-unknown external actions automatically unless the operation is explicitly idempotent and its idempotency contract is proven. Reconstruct safety counters from durable events. Surface reconciliation needs in the UI.

**Status: VERIFIED for the mechanism (`actions/recover.rs:27,139,204`; `executor.rs:88`; tests `tests/services/recovery.rs`). STATIC for the gaps, none reproduced at runtime:**

- Catch-up runs before any agent reconnects (`runtime/mod.rs:230`). `AgentLink::skills` is empty for a host not yet connected (`agent_server/registry.rs:263-269`), so `revalidate` rejects valid approvals with "the skill is no longer on the host". The recovery tests use a fake link that hides this. Code reading only; a failing test is needed to confirm.
- "Outcome unknown" is a string inside `Failed { reason }`, not a status. A link error or deadline during `run_skill` is recorded as plain `Failed` although the command may have run.
- An `Approved` action has no age limit and no check that the incident is still open.
- The loop-guard slot is claimed before `wait_for_agent`, so an offline agent consumes a slot (`executor.rs:244`).
- The rate limiter rebuilt from audit covers the action loop guard only. The triage hourly window (#112) is in memory.

Rework decisions: RW-09, RW-30, RW-08.

**Design consequence:** operation execution requires durable step identity, idempotency classification, start/result records, deadlines, cancellation semantics and recovery behavior. A command's exit code alone is not proof of the external system's final state.

### 3. Revalidate at every trust boundary

**Evidence:** #6 validates skill manifests and typed arguments (its plan hash covers the manifest, not the script); #10 rechecks executable paths and permissions immediately before running; #13 makes the agent revalidate incoming requests locally; #22 revalidates before dispatch; #93 constrains model-proposed skills and arguments; #110 revalidates cached proposals against current skills.

**Failure mode:** a capability may have been removed or changed since a model generated a proposal. A cached report may be stale. A script path may have been replaced after catalogue loading. A plan may change between approval and execution.

**Target rule:** treat LLM output, cache entries, chat buttons, hub requests and UI state as untrusted or potentially stale. Revalidate capability identity/version, target, typed arguments, current policy, plan hash and local host constraints immediately before execution. Approval must bind to the exact plan that was reviewed. Any material change invalidates approval and requires a new decision.

**Status: VERIFIED for revalidation at the agent and for cached proposals; the "exact plan" binding is overstated.** The agent re-does catalogue lookup, risk, args and plan hash locally (`agent/src/requests/skills.rs:61-125`); #110 re-checks cached proposals against current enabled skills (`triage/run.rs:88-93`, `proposals.rs:48-60`). But the plan hash covers the serialised manifest, not the entry script's bytes, so editing `run` after approval executes under the approved hash (STATIC). The approve API carries no plan hash (STATIC). Weaker points in #110 (STATIC): the duplicate lookup is `Page::first(50)` and a listing error becomes an empty list, so a store error can store duplicates (`proposals.rs:67-91`). Human-only approval (#22) is enforced by a text scan plus a type with no `Deserialize`, not by compile-time sealing (see "Corrections from verification"). Rework decisions: RW-08, RW-11, RW-07.

**Design consequence:** a plan is a versioned immutable object or snapshot; the execution request references that exact plan. The UI should clearly show when a plan changed and why previous approval no longer applies.

### 4. Keep policy, knowledge, and experience separate

**Evidence:** #22 and #107 show the need for explicit approval and guarded state transitions; #81/#82 add host profile context; #66/#63 add known-condition rules; #91 stores conversation history; #110 shows cached triage output must not itself authorize a new action.

**Lesson:** preferences, profiles, notes, learned patterns, historical approvals and model conclusions are useful context, but none should silently grant authority. A prior successful action is not permission to repeat it. A known condition is not the same as an autonomy policy.

**Status: UNVERIFIED** as a description of current behaviour (#63, #66, #81, #82, #91 not re-read against code). The negative claim holds in the code reviewed: nothing the model returns reaches `HumanApproval`, and a report becomes `Proposed` actions only through `proposals::store` (E section 3). Rework decisions: RW-14, RW-10.

**Target rule:** maintain distinct stores and APIs for environment knowledge, authorization policy and operational experience. Policy evaluation is deterministic and separately auditable. Knowledge can explain why an action is useful, but cannot make it allowed.

### 5. Model failure is a normal control-flow path

**Evidence:** #54 increased API timeouts for slow local models; #71 added streaming and configurable timeout; #89 introduced provider fallback; #90 added ordered model fallback chains; #94 handles providers without tool calling by gathering read output before asking the model.

**Target rule:** model calls need explicit deadlines, cancellation, streaming, provider/model attribution, usage accounting, retry classification and fallback policy. Distinguish transport failure, timeout, rate limit, authentication failure, missing model, invalid structured output and tool-call incompatibility. Do not retry every error identically.

**Status: failure attribution VERIFIED; the rest of the target rule is not met today (STATIC).** `triage/run.rs` joins failure reasons with provider and model, and #113 redacts and clips them. Success attribution is absent: `TriageReport` has no provider or model field and usage rows store only `provider.id()`. A failed call records no usage. Every error falls through to the next provider identically (no classification). The only whole-call limit is `max_seconds`, checked between turns; the client has only an idle read timeout, so there is no hard per-call deadline. Rework decision: RW-06.

**Design consequence:** an operation can remain in a visible “waiting for model” or “provider unavailable” state. The UI must not imply that reasoning is progressing when the provider is stalled. Fallback model identity must be recorded in experience history.

### 6. Durable delivery and offline behavior

**Evidence:** #13 implements a signal spool; #36 proposes making check-state commits contingent on durable spool writes, validating/repairing corrupted spool state, retaining notification state until delivery succeeds and preserving scheduler errors. #50/#52 address Discord delivery and quiet-hour behavior. #87/#88 add bounded log retrieval over the agent link.

**Target rule:** distinguish “created”, “queued”, “sent”, “acknowledged”, “failed” and “expired” for messages and events. Persist work before acknowledging it. Use bounded retries with backoff and dead-letter/error visibility. Delivery is at-least-once: a duplicate is possible after a crash window, so messages carry a stable id and the product never claims exactly-once (RW-35). Do not discard state merely because delivery failed. Offline agent data must be bounded and have explicit retention.

**Status: ack-after-persist VERIFIED; #36 findings STATIC and still present on main.** The hub acks signals only after the store accepts them (`agent_server/session.rs:90-105`) and the spool is bounded (`agent/src/spool.rs:14-22`). Still open: the agent scheduler saves check state even when the spool push fails (`agent/src/scheduler.rs:333-344`); the hub saves notification state before delivery succeeds and treats a state read error as default state (`runtime/delivery.rs:55-67,111-125,242-248`); a corrupt `spool.json` becomes default meta (`spool.rs:66-95`). The earlier text "not implemented" for #36 is corrected to "confirmed present". Rework decisions: RW-03, RW-15, RW-30.

**Design consequence:** the hub/agent link, operation event stream, notifications and emergency local autonomy need durable delivery semantics and reconciliation after reconnect.

### 7. Protocol and deployment compatibility are product behavior

**Evidence:** #9 defines a typed, versioned protocol with frame bounds and explicit unknown-message handling; #86 adds minimum supported release checks with a human-readable reason; #72 detects Docker/Podman; #34 documents/handles unavailable host checks in container mode.

**Target rule:** hub and agent speak the exact same protocol major.minor; an older or newer minor is rejected with a typed explanation, and there is no support window, previous-minor adapter or legacy execution path (owner decision D-20, RW-17). Same-protocol patch releases are compatible, and application release floors (#86's `MIN_AGENT_VERSION` and `MIN_HUB_VERSION`) are removed: a release number is not a compatibility dimension (a revoked-build list is DEFERRED and separate). The protocol check happens only after the agent has proven its identity, and the incompatibility is persisted on the agent record so it survives a hub restart. Frames are bounded and explicitly versioned, golden frames are tested, and unknown values decode without crashing. Do not let incompatibility turn into an endless reconnect loop: the agent idles instead of reconnecting (agents older than the F3 release still restart-loop under systemd until upgraded) and the hub shows the host as `Incompatible` with its history kept. Exactly one protocol bump is planned before v1 (RW-42). Detect runtime capabilities rather than assuming the installation mode.

**Status: VERIFIED.** `proto/src/version.rs:17-26`, `hub/src/agent_server/handshake.rs:140-148,194-215`, frame cap `proto/src/envelope.rs:11`. Notes: `at_least` lets an unparseable agent version pass the floor (`version.rs:104-108`); a new `ReadTool` variant or a new `Risk` variant in `hello` still breaks old peers (D13 in `docs/rework/01-repository-reality.md`). Rework decision: RW-17.

**Design consequence:** UI should show hub/agent versions, compatibility state (`Incompatible` with the exact required protocol and an upgrade-required explanation, never offline), runtime type, missing host permissions and unsupported capabilities with actionable remediation. Upgrade is hub first, then agents through the install procedure; there is no agent self-update.

## Lessons that directly affect the UI plan

### 8. Page and component states must be designed, not improvised

**Evidence:** #100 established a component kit; #103 explicitly added loading, empty, error and offline states; #104 distinguishes no notes from no search matches, adds retry, unsaved-change warnings and mutation outcomes; #105 adds disabled reasons and mutation feedback (the unsaved-change guard is in the notes editor only); #111 refines overview, incident, host and settings hierarchy.

**Target rule:** every page, tab, table, graph, form and async action must specify at least: initial/loading, loaded, empty, no-results, partial-data, stale-data, offline, permission-denied, error/retry, submitting, success and failure states where relevant. “Empty” must never be used to conceal a failed request.

**Status: VERIFIED in part.** `QueryBoundary` and the state kit exist (#100, #103, #104). A failed refetch is dropped when data exists and there is no stale or refreshing indicator (STATIC, `QueryBoundary.tsx:143-189`). Only 24 of 94 data-using files use `QueryBoundary` (D34), so "every load" is a convention, not a check. Rework decision: RW-21.

**UI specification addition:** page specs must include state matrices and transitions, not only static content and happy-path interactions.

### 9. Mutations need clear feedback, reversibility and protection against accidental loss

**Evidence:** #104 adds a single save bar, unsaved counts, discard, field-level errors, connection-test feedback, typed host-name revocation, delete confirmation and Undo with exact expiry restoration. #103 reports mutation results and requires typed confirmation for agent revocation. #105 adds a lint that flags mutations without feedback unless the file carries a documented exemption (file-level, see lesson 14).

**Target rule:** every mutation must tell the user what changed, whether it succeeded, and what remains pending. Destructive or hard-to-reverse actions require clear consequence previews and confirmation appropriate to risk. Prefer Undo for reversible changes. Preserve user input on failure. Warn before navigating away from unsaved work.

**Status: PARTIALLY VERIFIED.** Undo with exact expiry restoration exists (`features/rules/DeleteRuleDialog.tsx`, test `rules.test.tsx:369`). Corrected claims: the guard that warns before navigating away from unsaved work is used by the notes editor only (`useLeaveGuard`, imported by `NoteEditor.tsx` only), not by settings or other forms (STATIC); and the lint that bans mutations without feedback is file-level, not per call site (lesson 14). `ChatPage.tsx:56` uses `mutateAsync` and reports through the thread instead (test `chat.test.tsx:108`). Typed host-name revocation (#103/#104) was not re-checked (UNVERIFIED).

**UI specification addition:** each action definition includes preconditions, confirmation level, pending state, success state, failure state, retry behavior and undo/rollback availability.

### 10. Context should follow the user, but global scope must remain explicit

**Evidence:** #64/#101 introduce global host selection and a selector that follows page navigation; #94 gives host-scoped chat read tools only when a host is selected and no tools for an all-hosts conversation; #103 makes host tabs deep-linkable through query parameters.

**Target rule:** selected host/environment context should persist across related pages and be visible. Global/all-hosts scope must be explicit and must not silently inherit a previous host's tools or permissions. Deep links should preserve meaningful tabs/filters and be shareable/bookmarkable where safe.

**Status: VERIFIED.** `hub/src/api/chat.rs:118-133` (`tool_host` is `None` for no host or an offline host), `engine/src/triage/chat.rs:149-162` (no skills without a host; only read-risk skills offered), tests in `tests/hub/api_chat.rs`. Web: the chat conversation is keyed by host (`ChatPage.tsx`); the selector is a view filter and mutations take explicit ids. Strengthening from the verification: the selected host persists in `localStorage` across sessions (`HostContext.tsx:6-27`), so a new session starts host-scoped; a deleted host name stays stored and gives a 404 from chat; the general chat has no server conversation (history is client `sessionStorage` per host). See INV-H1 and INV-H2. Rework decision: RW-20.

**Design consequence:** global Operator context, task scope, incident scope and capability target must be separately visible. Never infer a destructive target only from a hidden global selector.

### 11. Show what the AI actually used and preserve the conversation

**Evidence:** #69 exposes skills/tools used on incident details; #79 adds a manual-steps checklist; #81/#82 provide host profile context; #91 stores follow-up conversation with generation/race protection and chronological ordering; #92 shows the conversation, pending state and clear confirmation; #93 allows structured action proposals; #94/#95 create general host chat.

**Target rule:** the user should be able to inspect evidence, tools/capabilities invoked, manual steps, model answers and action proposals as separate structured elements. Preserve conversation context across turns, but make clearing/reset behavior explicit. A request still in flight must not be silently erased or resurrected by a late response.

**Status: VERIFIED for incident follow-up chat; the general chat has no server conversation.** Generation check `manager/annotate.rs:132-150`, `triage/history.rs` (`count_clear`, `asked_at` ordering), clear disabled while sending (`web/src/components/ChatThread.tsx:234`), the answer shown from the response when the turn is not yet remembered, history-load failure distinct from empty (`IncidentChat.tsx:35-45,127-129`), tests `incidentChat.test.tsx:291,306`. Weak points (STATIC): when the generation check drops an answer, `answer()` still returns `Ok(answer)`, so a client can show a reply to a conversation another tab cleared; a failed timeline read captures generation 0, which wrongly drops the answer if any earlier clear exists (`followup.rs:182-188`); the client matches a landed turn by question text, so a repeated question can match an older turn (`IncidentChat.tsx:38`); tool use from a failed follow-up is not recorded (`followup.rs:235`). Rework decision: RW-20 (request-id keyed pending turn, stale responses discarded, composer draft kept until the send commits). See INV-Q1 to INV-Q3.

**Design consequence:** Operator workspace should be built around a durable task/conversation identity, with linked orchestration, evidence and action history. Chat is not just a transient message list.

### 12. Live views need bounded refresh, useful controls and performance budgets

**Evidence:** #87/#88 bound in-memory logs, redact before storage and again at the API/agent boundary, and fetch remote logs in chunks. #103 adds live toggle, search/highlighting, jump-to-latest, wrapping, copy and show-older; polls host list only while waiting for enrollment. #105 memoizes large tables and sets bundle/CSS budgets.

**Target rule:** real-time views need explicit live/pause state, bounded windows, pagination/virtualization, search, copy/export and clear freshness indicators. Poll only while a user-visible condition requires it; prefer event streams where appropriate. Keep the interface responsive under large histories and frequent updates.

**Status: VERIFIED for logs (#87/#88); UNVERIFIED for the #103 live-view controls.** Ring capacity 2000, redacted at capture and again in the API (`domovoi/src/log_ring.rs:15-75`, `hub/src/api/logs.rs:54`), agent cap 2000 lines (`agent/src/requests/mod.rs:43`). SSE has no event ids and a slow client silently loses events (D14). Rework decision: RW-15.

**Design consequence:** orchestration graph and event timeline need incremental updates, bounded rendering and stable selection/scroll behavior. New events must not unexpectedly move the user's current inspection target.

### 13. Technical language should be explained, not erased

**Evidence:** #65 adds explanations for pages and jargon; #111 keeps detailed evidence and operational labels while improving hierarchy.

**Status: UNVERIFIED** (#65, #111 wording not checked against the UI).

**Target rule:** retain precise terminology (orchestration, capability, policy, verification) and pair it with concise explanations. Do not dumb down the detailed view. Use configurable information density and progressive disclosure, with the detailed experience as the reference design.

### 14. A design system needs automated enforcement

**Evidence:** #98 centralizes design tokens, themes and status tones; #100 creates shared feedback/hint/state/form components; #105 adds WCAG AA contrast checks, axe checks, voice/tone lint, mutation-feedback rules, disabled-reason requirements, bundle-size budgets and memoization checks.

**Target rule:** define semantic tokens and shared component contracts, then enforce them in CI. Avoid raw colors and one-off status interpretations. Components should expose accessible names, focus behavior, disabled reasons and consistent feedback. Accessibility and performance are acceptance criteria, not final polish.

**Status: PARTIALLY VERIFIED; "enforced in CI" is overstated.** Token, status-vocabulary, icon, inline-style, raw-button and file-length rules are lint or test checks (`web/eslint/domovoi-rules.js`, `src/guardrails.test.ts`, `styles/contrast.test.ts`). Weaker than described (STATIC): the mutation-feedback rule passes a file if it imports `useActionFeedback`, `InlineMessage` or `InlineError` anywhere (`domovoi-rules.js:10,128`), so one reported mutation excuses an unreported one in the same file; four files use a `// feedback-ok:` header exemption that does not prove the wrapping; the disabled-reason check is likewise structural; axe runs in jsdom with colour contrast excluded (contrast is covered by the token-pair test instead); `api/health.ts` uses raw `fetch`. Rework decision: RW-21.

**Important nuance:** shared components should support consistent behavior without forcing every page into the same layout. Reuse the design language and interaction contracts; don't let the component kit dictate the information architecture.

## Capability, onboarding and operations lessons

### 15. Capability lifecycle needs more than a list of scripts

**Evidence:** #83 exposes skill README, limits and recent runs; #84 allows enable/disable per host; #103 shows risk, timeout, applies-to and last-used; #44 installs starter skills by default and suggests restart after upgrade.

**Status: #84 VERIFIED; the rest UNVERIFIED.** `PolicyLink` (`hub/src/agent_server/skill_policy.rs:157-221`) filters skills and refuses `run_tool` read skills and `run_skill` for disabled ones, with a mutex serialising read-modify-write (`:32-45,93-107`); every consumer is handed `policy_link` (`runtime/mod.rs:216,220,235,269`, `profile/collect.rs:51,193`). Weak points (STATIC): enforcement is a wrapper the consumer must be handed; `server.link()` stays public and `AgentAdmin::skills` is deliberately unfiltered, with no arch-check rule against bypass; `run_check` is not policy-filtered (`skill_policy.rs:186-195`); the policy is hub-side only and keyed by host name. Rework decisions: RW-07, RW-26.

**Target rule:** future capability management should distinguish discover, inspect, compatibility, install, enable, configure, authorize, update, disable, uninstall and audit. These are different operations. Installing a capability must not grant it permission. Per-agent/host compatibility and enablement must be explicit.

### 16. Host profiles and environment discovery are valuable, but provenance matters

**Evidence:** #81 collects and stores host profile; #82 injects profile into triage/chat/notes; #67 adds expected SSH logins and a login time window; #72 detects runtime type.

**Status: #81/#82 snapshot-diff behaviour VERIFIED; the rest UNVERIFIED.** `hub/src/profile/diff.rs:26-31,75` compares snapshots only when both sides answered (tests `tests/hub/profile.rs:541,566`). Not re-checked: #67 default login window, #72 runtime detection, #81 fence escaping. Rework decision: RW-14.

**Target rule:** use discovered host facts to tailor questions and diagnostics, but show source, freshness and uncertainty. Auto-discovered information must not silently become user-confirmed constraints or permissions. Profiles should be editable and linked to the evidence that produced them.

### 17. Known conditions should be understandable and scoped

**Evidence:** #63 provides UI management with plain-language condition rules; #66 supports dismissing an incident as a known condition; #104 shows expiry, creator, readable phrases, operator-specific inputs and Undo.

**Status: #66 VERIFIED (rule stored before dismiss, `hub/src/api/incidents.rs:343-398`; the rule audit runs after dismiss, so its failure returns an error for an already-done dismiss); #63 and #104 UNVERIFIED.**

**Target rule:** known conditions are explicit, scoped, time-bounded when appropriate and reviewable. The UI should explain exactly what will be suppressed and until when. A known condition suppresses expected noise; it must not conceal a security event or authorize a remediation action.

### 18. Installation and upgrade flows need operational safety

**Evidence:** #33 deployment methods; #32 guided first run and UI-managed settings; #43 LAN/integration setup; #44 starter capabilities and restart guidance; #96 safe release discovery; #97 in-place upgrade that compares the archive with a `.sha256` file from the same release origin (integrity, not authenticity) and does not silently restart services.

**Status: #96 VERIFIED; #97 overstated.** The release-tag allowlist `is_release_tag` (`proto/src/version.rs:97-103`) is used in `cmd/upgrade/fetch.rs:48`. The #97 upgrade is integrity-checked, not authenticated: the archive is compared with a `.sha256` file from the same release origin (`cmd/upgrade/package.rs:13-30`), which detects corruption but not a compromised release, and the attestation the release workflow produces is not verified by the command (STATIC). `install.sh` also verifies SHA-256 (F). Rework decisions: RW-26, RW-31.

**Target rule:** setup and upgrade flows must explain prerequisites, what will change, how to recover and whether a restart is required. Pin versions, verify downloaded artifacts and never execute untrusted release metadata as shell input. Separate downloading/replacing a binary from restarting services.

## Unmerged closed PRs: useful signals, not current behavior

Seven closed PRs were not merged: #1–#4 and #14 are dependency updates; #36 is a substantive hardening/audit PR; #38 is an attempted `v0.1.0` release PR. Do not assume their changes are in `main`.

- **#36 — signal and notification delivery hardening (not merged):** proposed durable-spool ordering, corrupted-spool repair/rejection, notification retention until delivery succeeds, repair of missing discussion threads, digest paging, scheduler error visibility, generated API artifact checks and documentation/link audits. These are valuable review findings. Verification against `main` (STATIC) found the spool-before-state, notification-state-before-delivery and state-read-error-as-default findings still present (lesson 6); do not treat them as implemented.
- **#38 — release `v0.1.0` (not merged):** release-plz generated a release PR that was closed. It is historical release automation, not evidence that a `v0.1.0` release shipped.
- **#1–#4 and #14:** dependency bumps closed without merge. No product behavior should be inferred from them.

## PR groups reviewed

The following index records the complete closed-PR set and the primary topic inferred from each title/description. Routine dependency and release PRs are included so the audit scope is explicit.

| PRs | Primary topic / lesson |
|---|---|
| #113 | Redact a provider failure reason and clip it to 500 characters before it leaves the engine (provider error text is a leak channel). |
| #112 | One admission gate caps triage, questions and chat: shared hourly and monthly limits, runs reserved for incident triage, free refusal, 429 with `Retry-After`. |
| #111 | Current UI overhaul: operator-oriented hierarchy, dense evidence, host views, responsive shell. |
| #110 | Revalidate cached proposals; deduplicate actions; rotate stale chat approval tokens. |
| #109 | Single writer for all action transitions; crash recovery; rebuild rate limits from audit. |
| #108 | Release PR `chore: release v0.0.10`, merged. Release-only; no product lesson. |
| #107 | Serialize approve/deny/expiry; guarded writes; drain queued decisions on shutdown. |
| #106 | Playwright patch dependency update. |
| #105 | UI guardrails: contrast, axe, voice, mutation feedback, bundle budgets and rendering performance. |
| #104 | Settings/notes/known-condition forms; unsaved changes, retry, undo, validation and explicit read-only sources. |
| #103 | Host/skill/log pages; offline/error states, host-scoped navigation, live log controls and mutation feedback. |
| #102 | Incident/approval/chat pages; needs-attention grouping, mobile behavior and action-focused presentation. |
| #101 | App shell, grouped navigation, host selector, lazy-loaded pages. |
| #100 | Shared UI kit for feedback, hints, states and forms. |
| #99, #85, #70, #55, #51, #48, #45, #42, #40 | Release PRs; generated version/changelog history. |
| #98 | Design tokens, light/dark/system themes, semantic tone/status system and icons. |
| #97 | In-place binary upgrade checked against a same-origin `.sha256` file (integrity, not authenticity); no implicit restart. |
| #96 | Release update discovery and safe, pinned update instructions. |
| #95 | General Chat UI, host scope and per-host conversation persistence. |
| #94 | General host chat with scoped read tools and bounded/redacted context. |
| #93 | Structured chat action proposals, deduplication and no direct execution. |
| #92 | Incident follow-up chat UI, pending state, chronological conversation and clear confirmation. |
| #91 | Persisted follow-up conversations, generation/race handling and ordered replay. |
| #90 | Ordered model fallback chain within and across providers. |
| #89 | Secondary provider fallback and provider-specific connection testing. |
| #88 | Retrieve agent logs over hub-agent protocol in bounded chunks. |
| #87 | Bounded hub log ring, redaction and Logs page. |
| #86 | Minimum supported hub/agent release and explicit incompatibility reason. |
| #84 | Per-host skill enablement. |
| #83 | Skill detail view: README, limits, parameters and recent runs. |
| #82 | Host profile supplied to triage, chat and notes. |
| #81 | Host profile collection, persistence and UI. |
| #80, #73 | Additional starter skills. |
| #79 | Manual-step checklist in triage report and incident UI. |
| #78, #77, #76, #75, #74 | Dependency updates (metrics exporter, Rust, Node types, web dependencies, BusyBox). |
| #72 | Docker/Podman runtime detection and hub selection. |
| #71 | LLM response streaming and configurable timeout. |
| #69 | Show skills/tools used on incident details. |
| #68, #49, #37 | Release pipeline/attestation/changelog fixes. |
| #67 | Expected SSH logins and login time window. |
| #66 | Dismiss an incident as a known condition. |
| #65 | Explain page purpose and jargon in the UI. |
| #64 | Sidebar shell and global host selector. |
| #63 | Manage known conditions through the UI. |
| #62 | Group incidents by state. |
| #61 | Rerun triage from scratch without duplicate proposals. |
| #60 | Bound retries across mixed timelines and freed concurrency slots. |
| #59 | Retry skipped/failed triage under hourly limits. |
| #58 | Correct resolved-incident status presentation. |
| #57 | Document local testing and CI checks. |
| #56 | Distinguish hub-only host from offline agent hosts. |
| #54 | Longer API request timeout for slow local models. |
| #53 | Discord test uses a placeholder approver when none is configured. |
| #52 | Deliver open-incident cards when quiet hours end. |
| #50 | Discord digest and one-way message delivery. |
| #47, #46 | Retry triage from incident page; setup/enrollment fixes. |
| #44 | Install starter skills by default and suggest restart on upgrade. |
| #43 | Improve LAN and integration configuration. |
| #41, #39 | Consolidated CI and checks scoped to changed areas. |
| #38 | Closed, unmerged `v0.1.0` release attempt. |
| #36 | Closed, unmerged signal/notification durability and audit hardening proposal. |
| #35 | Starter skills, triage drills and documentation. |
| #34 | Handle checks unavailable in container mode. |
| #33 | Installer, systemd, container images and install guides. |
| #32 | UI-managed hub settings and guided first run. |
| #31 | CLI roles, single-box mode, metrics and self-test. |
| #30 | Hub runtime lifecycle assembly. |
| #29 | Discord threads/buttons/follow-ups and webhook notifier. |
| #28, #24 | ADR/documentation proposals; not treated as binding decisions in this design pack. |
| #27 | Initial host/enrollment/skills/notes/settings/digest UI pages. |
| #26 | Redact triage reports and inbound signal text. |
| #25 | Initial app shell, authentication, live incidents and approvals. |
| #23 | JSON API, sessions, CSRF, SSE and generated OpenAPI. |
| #22 | Human-only approvals, revalidated dispatch, verification and audit. |
| #21 | Triage orchestrator with read-only tool loop, caching and provider fallbacks. |
| #20 | LLM provider adapters. |
| #19 | Signed webhook ingestion and agent relay. |
| #18 | Agent server, enrollment, pinned TLS, presence and host-down detection. |
| #17 | Notification policy, cards, digest and advisories. |
| #16 | Single-writer incident manager, correlation and suppression. |
| #15 | SQLite repositories, FTS5, append-only audit and backups. |
| #14 | SHA-2 dependency update closed without merge. |
| #13 | Agent enrollment, pinned TLS, scheduler, spool and local request handling. |
| #12 | Container/unit/SMART/security/network checks. |
| #11 | Check framework, bounded runs and system checks. |
| #10 | Sole process runner, sandbox constraints and skill catalogue revalidation. |
| #9 | Typed/versioned hub-agent protocol, frame bounds and compatibility tests. |
| #8 | Typed configuration, validation and secret indirection. |
| #7 | Ports, fakes and repository contract suites. |
| #6 | Typed skills/actions, plan hashes, approval model and closed read-tool catalogue. |
| #5 | Pure incident state machine and stable fingerprints. |
| #1–#4 | Dependency update PRs closed without merge. |

## Concrete requirements to carry into the rework

- [ ] Every mutable domain has one authoritative transition owner and guarded state updates.
- [ ] Restart recovery classifies pending work; dispatched-but-uncertain operations are not blindly replayed.
- [ ] Action approval binds to an immutable/versioned plan; any material plan change invalidates approval.
- [ ] Cached model output is never authority; revalidate it against current capability and policy state.
- [ ] Capability installation, enablement, configuration and authorization are separate lifecycle steps.
- [ ] Agent-side enforcement remains mandatory even when the hub has a richer policy engine.
- [ ] Model provider failure, timeout, fallback and streaming are explicit operation states.
- [ ] Agent signals and notifications have durable delivery/acknowledgement semantics.
- [ ] UI page specs include complete async/error/offline/empty/partial states and mutation feedback.
- [ ] Destructive operations have risk-appropriate confirmation, typed target confirmation where justified, and undo/rollback when possible.
- [ ] Host/environment context is visible and scoped; global chat does not inherit host-only tools implicitly.
- [ ] Conversations, operation events, evidence and action history remain linked and race-safe.
- [ ] Live views are bounded, incremental, searchable and do not jump the user's selection/scroll unexpectedly.
- [ ] Semantic tokens, accessible controls, contrast/axe tests, performance budgets and mutation-feedback checks run in CI.
- [ ] Documentation, generated API artifacts, examples and acceptance tests ship with the feature, not as cleanup later.
- [ ] Every model call passes one admission gate whose limits survive restart and declare fail-open or fail-closed; a cache hit costs no model slot.
- [ ] Every error string that reaches a user, a chat reply or an external log passes the redactor and a length cap.
- [ ] Approval binds to implementation bytes (not only a manifest), carries the plan hash the user saw, and expires.
- [ ] Event kinds that drive control flow are typed, not parsed from strings.

## What this review does not decide

- Whether to retain Rust, SQLite, the current agent protocol or any current UI component. The PR history supplies evidence, not a veto on the target architecture.
- The exact future operation/incident schema, workflow engine, graph store, capability package format or policy language. Those remain design tasks in the relevant pack documents.
- Whether every #36 hardening proposal should be merged into the future design. Its durability findings should be checked against the current code and tests before implementation.

## Additional lessons found in PR review comments

The PR descriptions often summarize the intended change; the maintainer's follow-up comments record edge cases discovered during review. These are particularly useful because they show where an apparently complete feature was still unsafe or misleading.

- **Never treat malformed or quoted model text as structured intent.** **Status: VERIFIED.** The follow-up parser takes one complete JSON array after the last `PROPOSALS:` line, `ActionProposal` has `deny_unknown_fields`, and count and per-line caps apply (`triage/followup.rs:16-24,116-142,242-249`; `core/src/triage/report.rs:37`). Weak point (STATIC): malformed output goes through `unwrap_or_default()`, so a broken payload silently yields no proposals, and the marker is out-of-band text rather than provider-native structure. RW-06 replaces it with schema-validated meta-tool calls. Cached proposals (#110) are revalidated through the same `check` (lesson 3). #93 found that extracting the first `[` through the last `]` could turn quoted prose into an action proposal. Parse a complete, explicitly delimited structured payload; reject surrounding prose and unknown fields. Prefer provider-native structured output when available. Also enforce proposal count/size limits, deduplicate within a batch, and address cross-request uniqueness at the store/serialization boundary.
- **Bound every model input, not only the visible prompt.** **Status: VERIFIED with a difference.** General chat bounds and redacts every replayed turn (`triage/chat.rs:20-35,60-67`). The incident follow-up replays stored turns without re-redacting, relying on redaction at write time (`followup.rs:84-107`, STATIC), and there is no total prompt cap across context, tool results and history (D11 in `docs/rework/01-repository-reality.md`). #94/#95 found that historical turns and skill catalogues could exceed budgets and that historical user/assistant messages could carry secrets. Redact every historical turn before sending it to a provider; cap each turn, total history, question, catalogue and host overview; drop oldest history deliberately and preserve complete question/answer pairs where possible. A global chat without a selected host should not receive host tools; offline hosts should receive no tools.
- **A chat must never lose a successful answer or user input.** **Status: VERIFIED for the incident chat** (lesson 11 lists the remaining weak points; the general chat keeps history client-side only). #92 found a race where the response could arrive before history refreshed, leaving “Thinking…” stuck or making the answer disappear. Pending state should follow the request mutation; render the answer from the response if persistence lags; clear the composer only on success; retain input on failure; distinguish history-load failure from an empty conversation; block clear while a request is active and prevent a stale answer from reappearing after clear. Enforce the server's character limit in the client too.
- **Conversation clear needs generation semantics.** **Status: VERIFIED for incident follow-ups; STATIC weak points in lesson 11.** The general chat has no server conversation, so no generation exists to protect. #91 prevents a slow in-flight answer from restoring a conversation after the user clears it. Requests capture a conversation generation and asking time; persistence rejects stale generations and history replays in asking order even if responses finish out of order. Apply the same pattern to any resettable long-running UI or task context.
- **Provider fallback must preserve diagnostic information.** **Status: VERIFIED for failures, STATIC for successes.** Failure reasons carry provider and model and are now redacted and clipped (#113); a successful output records neither (lesson 5). #89 found that fallback could hide the primary provider's failure and that invalid model output could omit provider identity. Aggregate provider-specific failure reasons and include provider/model attribution in diagnostics and operation history.
- **Never trust release metadata just because it parses as a version.** **Status: VERIFIED** (`is_release_tag`, `proto/src/version.rs:97-103`, used in `cmd/upgrade/fetch.rs:48`). #96 found tag-injection risk: a parser that stops at a prerelease/build separator can accept a version prefix while ignoring unsafe suffix characters. Validate the entire release tag against a strict allowlist before using it in URLs, UI or copyable shell commands. Pin the exact image version in generated Docker upgrade commands. Bound HTTP response bodies at the shared HTTP layer, not just one call site.
- **Protocol additions must be understood by old peers.** **Status: VERIFIED** (`handshake.rs:140-148,194-215`). #86 found that sending a newly introduced rejection reason to an old agent could leave it in a reconnect loop. Only send a reason the peer version can understand; otherwise use a compatible terminal rejection. Authenticate the peer before recording a version refusal, so unauthenticated traffic cannot create misleading host events; the existing `Incompatible` rejection is used, because released agents already treat it as final (RW-17).
- **A disabled capability must be disabled on every path.** **Status: VERIFIED for current consumers; STATIC for the structure.** Every consumer is handed the policy wrapper, but the raw link stays reachable and `run_check` is unfiltered (lesson 15). #84 found host-profile collection bypassing the skill policy wrapper even though triage and action execution used it. All consumers (profile collection, chat, incident investigation, scheduled jobs, etc.) must use the same policy-enforcing capability access layer. Concurrent per-host enable/disable updates need serialized read-modify-write behavior so one update cannot silently erase another.
- **Missing data must not be interpreted as deletion or recovery.** **Status: VERIFIED for host profiles** (`profile/diff.rs:26-31,75`). **STATIC: the same lesson is broken elsewhere.** An offline or not-yet-connected agent looks like "no skills" (lesson 2); a notification-state read error becomes default state (lesson 6); a failed duplicate lookup becomes an empty list (lesson 3); a budget read error becomes "not spent" (lesson 5 and #112). #82 found that a failed container or listening-port query could make the next profile claim all previous entities had disappeared. Compare snapshots only when both sides contain valid data; represent collection failure/unknown separately from an empty successful result. Do not infer removals from missing observations.
- **Generated host profiles need explicit limits and honest semantics.** **Status: UNVERIFIED** (PR text only; fence escaping, refresh concurrency and note editability were not re-checked). #81 found read skills with required parameters being called with empty arguments, generated profile notes overwriting prior content, tool output breaking Markdown fences, profile refresh being synchronous/expensive, and editable generated notes being ambiguous. Skip skills that cannot be called safely without parameters; escape/fence arbitrary tool output; label generated data as a latest snapshot rather than a history if it is overwritten; decide whether generated records are editable; bound refresh work and add backoff for failed sections.
- **Tool-use history must distinguish not-run from failed.** **Status: VERIFIED in part.** Names are validated against `ReadTool::ALL` and records carry `failed` and `refused` flags (`triage/agent.rs:20-33`); there is no skipped or cancelled outcome, and tool use from a failed follow-up is not recorded (STATIC, `followup.rs:235`). #69 found invented model-supplied tool names, budget-refused calls counted as failures, missing prefetched calls for providers without tool support, and identical names across tool/skill categories being merged. Validate names against registries, record category and run identity, and use distinct outcomes such as succeeded, failed, refused/not-run, skipped and cancelled. Record partial/in-flight runs if the UI is expected to show them.
- **Network-security defaults must preserve access.** **Status: UNVERIFIED** (#67 defaults not re-checked). #67 found that a login-allowance window could accidentally suppress all SSH logins and that one bare IP address was rejected. Default an “expected login” window to LAN-only, require explicit opt-in for internet-origin logins, confirm before creating broad rules, support /32 and /128 single-address semantics, and derive active-window UI state from persisted rules so reloads do not lie.
- **Create suppression rules before closing the incident they suppress.** **Status: VERIFIED** (`hub/src/api/incidents.rs:343-398`; see lesson 17 for the audit-after-dismiss caveat). #66 found a race where a new signal could arrive between dismissing an incident and storing its known-condition rule. Validate and persist the rule first, then close/dismiss; if the later step fails, leave the rule visible and explain it. The UI must state that a rule can cover all severities for that check/target and must not offer dismissal while triage is still pending.
- **Retry budgets need typed reasons.** **Status: VERIFIED** (reason enum `triage/mod.rs:180`, per-reason limit `triage/queue.rs:206-222`). STATIC: retry state is rebuilt on startup from timeline kinds that are plain strings, so the typing stops at the memory boundary. #60 found that cap-deferred retries and failure retries were being conflated, so an old failure chain could block a later retry that was only deferred by a concurrency cap. Track why a run was deferred and apply each limit only to the retry class it governs. Test mixed timelines and slot release behavior.
- **Do not let optional integration setup block a connection test for the wrong reason.** **Status: UNVERIFIED** (#53 not re-checked). #53 found that a Discord test could fail validation because no approver list had yet been saved, even though the test itself should be able to reach Discord. Use narrowly scoped test-only placeholders where appropriate, explain their scope and ensure they are not persisted as real authorization.
- **Hardware diagnostics must not create side effects or leak identifiers.** **Status: VERIFIED in part.** `smartctl -n standby` (`examples/skills/hardware_health/run:53`) and the "could not list" versus "no UPS detected" distinction (`power_status/run`) were confirmed; serial-number filtering before model exposure was not re-checked (UNVERIFIED). #73 found that SMART health checks should not wake sleeping disks, serial numbers should be filtered before model/timeline exposure, and UPS command failures must not be rendered as “no UPS detected.” Use non-waking commands, bounded execution time and precise “unavailable/failed/not detected” distinctions.
- **Web fixtures must not depend on the real clock (UNVERIFIED as a current defect; recorded in the #107 comments).** A web test failed because fixtures were relative to the real date. Tests of time-dependent behaviour, web as well as Rust, use a fixed or fake clock.
- **Review findings should be classified, not silently ignored.** **Status: process lesson, not tested against code.** It applies to this document too: every item above carries a status so "reviewed" is not read as "resolved". Several comments explicitly left non-blocking items as separate design decisions (for example, profile refresh concurrency, generated-note editability, per-section size caps, API body bounds and cross-request proposal uniqueness). Preserve these as open design questions when the affected feature is selected; do not assume “reviewed” means “resolved.”

## Lessons added by #112, #113 and the verification pass

- **#112, one admission gate caps triage, questions and chat (VERIFIED, PR text and `triage/admission.rs`).** Every model call (triage, follow-up question on web and Discord, general chat) goes through one `admit(shared, Purpose)` gate with one hourly count and one monthly budget. Questions may not use the last `reserved_for_incidents` runs, so a few questions cannot starve triage of a new incident. A busy refusal takes no run of the hour; a refused question answers 429 with `Retry-After`. Lessons: put one gate in front of every model path, reserve capacity for the safety-critical path, and make a refusal free. Gaps (STATIC): the hourly window is in memory and resets on every restart and every settings save; a budget read error fails open (`admission.rs:52-59`); a start is counted before the call and never refunded; the gate runs before the cache lookup, so a cache hit spends a model slot (lesson below).
- **#113, redact a provider failure reason before it leaves the engine (VERIFIED, PR text and tests such as `a_failed_follow_up_never_shows_a_secret_from_the_provider_error`).** `LlmError::Transport` and `InvalidResponse` carry provider free text (response excerpts, URLs). Both question paths now build the reason through one `provider_failure` helper that redacts once and clips to 500 characters. Lesson: provider error text is a leak channel, separate from answer and history redaction. Every error string that reaches a user, a chat reply or a log shipped elsewhere passes the redactor and a length cap. The fix covers the two question paths; other places that surface provider text were not audited.
- **Cache hits spend a model slot (STATIC).** The cache is consulted after admission (`triage/run.rs:59` before `:88`), so a cache hit consumes an hourly slot even though no model call happens. A cache must cost no model capacity.
- **Timeline kinds are stringly typed and carry control flow (STATIC).** Retry limits, failure-chain length and conversation boundaries are derived by parsing `incident_events.kind` prefixes (`triage.skipped`, `triage.failed`, `triage.followup`, `followup.cleared`; `triage/queue.rs:189-222`, `triage/history.rs`). Give event kinds a closed type; do not drive control flow from display strings.
- **Admission limits must be durable and say how they fail (STATIC).** The hourly cap is memory-only and resets on restart; the monthly budget is durable but fails open on a read error. State per limit whether it fails open or closed, and rebuild counters from durable history.
- **Model attribution and accounting are incomplete (STATIC).** A stored report, follow-up or chat answer does not record the model that produced it (`core/src/triage/report.rs:52-76`; usage stores `provider.id()` only, `triage/agent.rs:110-120`). A call that fails records no usage (`provider.complete(req).await?` returns first).
- **No hard per-call deadline (STATIC).** The LLM client has only an idle read timeout that restarts on each byte (`domovoi-llm/src/http.rs:20-30`); `max_seconds` is checked between turns (`triage/agent.rs:146-148`). One streaming call can run unbounded while bytes keep arriving.
- **Spool before state (STATIC; the #36 finding still present).** The agent scheduler applies `set_firing` in memory, logs a spool push failure, then saves check state anyway (`domovoi-agent/src/scheduler.rs:333-344`). A transition can be committed to state and lost from the spool, and the check will not re-emit it.
- **Notification state must follow delivery, and a read error is not default state (STATIC; #36).** The hub saves notification state (`first_alert_at`, `held`, `last_reminder_at`) before delivery succeeds (`hub/src/runtime/delivery.rs:55-67`; a failed delivery leaves state saying it was sent, `:111-125`). A state read error becomes default state (`:242-248`), which can produce a duplicate card. A quiet-hours release is persisted before sending (`dispatcher.rs:207-212`).
- **Restart catch-up runs before agents reconnect (STATIC; not reproduced at runtime).** `ActionService::spawn` runs at `hub/src/runtime/mod.rs:230`, before listeners serve. `AgentLink::skills` returns empty for an agent that has not connected (`agent_server/registry.rs:263-269`), so `revalidate` (`actions/executor.rs:325-332`) finds no skill and rejects the approval as "the skill is no longer on the host". The recovery tests use `FakeAgentLink`, which keeps skills while connected, so they cannot see it. This is code reading; a failing test is needed to confirm it (D36 in `docs/rework/01-repository-reality.md`).
- **Approvals have no age (STATIC).** An `Approved` action carries no age limit at dispatch; catch-up can run an approval made days earlier without checking that the incident is open (`executor.rs:219-262`).
- **Audit and state diverge in one direction (STATIC).** `record_from` writes state, then appends audit; an audit failure is only logged (`actions/mod.rs:127-143`). The loop-guard reload reads `dispatched` audit rows (`recover.rs:139`), so a lost row undercounts after a restart.
- **Capability policy by wrapper is a convention (STATIC).** `PolicyLink` filters skills for every consumer that is handed it, but `server.link()` stays public and no arch-check rule prevents a new consumer from using the raw link.

## Review process for future implementation

For every substantial PR in the rework, use a repeatable review checklist:

1. Read the full PR description and discussion, not just the diff summary.
2. Identify invariants the change touches and name the authoritative owner for each state transition.
3. Ask what happens under duplicate requests, concurrent requests, stale cache, disconnect, process restart and partial persistence.
4. Ask whether missing/failed data is distinguishable from valid empty data.
5. Check untrusted inputs: model output, history, tool arguments, external metadata, logs, webhook payloads and generated shell commands.
6. Verify that every UI async path has loading, success, failure, retry and stale-response behavior.
7. Require regression tests for each discovered edge case; do not weaken existing security gates to make a test pass.
8. Record unresolved review findings in the relevant design document/open-question list, with an owner or decision milestone.

## Rework invariants

Testable invariants taken from investigation G, with the test each one needs. Each is phrased as a test name. "Carried by" names the decision in `docs/rework/03-architecture-decisions.md` (`RW-NN`) that carries it; where 03 has no decision for it, the target-architecture document that does is named (`04` section numbers), and where nothing carries it the row says so. "Evidence" is the status of the underlying finding at baseline `64117f0`: VERIFIED (the safeguard exists and was confirmed in code or PR text), STATIC (the gap was found by code reading; no failing test reproduces it) or UNVERIFIED.

| ID | Test name | Carried by | Evidence |
|---|---|---|---|
| INV-W1 | `stale_write_conflicts_and_changes_nothing` (two concurrent commands on one aggregate) | RW-03 | VERIFIED for actions (status-name guard); STATIC for incidents (unguarded update) |
| INV-W2 | `each_aggregate_has_exactly_one_writer_module` (arch-check rule) | RW-03 (best-effort scan), RW-04 | STATIC |
| INV-W3 | `token_rotation_on_a_proposed_action_goes_through_the_owner` | RW-04 | STATIC |
| INV-A1 | `audit_failure_leaves_the_action_row_unchanged` (audit store fails on append) | RW-03, RW-27 | STATIC |
| INV-R1 | `no_dispatched_action_is_dispatched_again_after_restart` (fake agent `runs == 0`) | RW-09, RW-30 | VERIFIED (mechanism and tests exist) |
| INV-R2 | `approved_action_runs_once_after_the_agent_reconnects` (link reports skills as unknown, not empty, while disconnected) | RW-09, RW-30 | STATIC (D36; not reproduced) |
| INV-R3 | `outcome_unknown_is_a_distinct_status_shown_to_the_operator` | RW-09 | STATIC |
| INV-R4 | `approval_older_than_its_ttl_or_for_a_resolved_incident_is_rejected_at_dispatch` | RW-08 | STATIC |
| INV-C1 | `admission_survives_restart` (hourly and monthly limits) | `04` section 16 (persisted `llm_admission`); no RW in 03 | STATIC |
| INV-C2 | `a_limit_declares_fail_open_or_fail_closed_when_its_history_is_unreadable` | `04` section 16; no RW in 03 | STATIC |
| INV-P1 | `cached_and_chat_proposals_pass_the_same_check_with_the_hosts_current_enabled_skills` | RW-06, RW-07, RW-08 | VERIFIED (#110) |
| INV-P2 | `a_failed_duplicate_lookup_stores_nothing` | no named decision; RW-03 supplies the mechanism | STATIC |
| INV-P3 | `approve_request_carries_the_displayed_plan_hash_and_a_mismatch_conflicts` | RW-08 | STATIC |
| INV-S1 | `editing_the_entry_script_after_approval_yields_plan_mismatch` (plan hash v2 includes entry bytes) | RW-08 | STATIC |
| INV-X1 | `a_disabled_capability_is_refused_by_every_consumer` (one test per consumer) | RW-07, RW-08 | VERIFIED for current consumers |
| INV-X2 | `the_raw_agent_link_is_unreachable_outside_the_policy_wrapper` | RW-08 covers callers of the execution function only; reads are not covered | STATIC |
| INV-X3 | `installing_a_capability_never_enables_it_for_a_host` | RW-26 | UNVERIFIED |
| INV-E1 | `agent_refuses_any_request_outside_its_local_catalogue_or_hash` (hostile hub fake) | RW-09, RW-12 | VERIFIED |
| INV-H1 | `a_request_without_a_host_gets_no_host_tools_and_an_offline_host_gets_none` | RW-20 | VERIFIED |
| INV-H2 | `no_mutation_derives_its_target_from_the_host_selector` | RW-20, RW-23 | VERIFIED |
| INV-D1 | `check_state_commits_only_after_the_spool_write` (failing spool, next run re-emits) | `04` section 15 durability fixes; no RW in 03 | STATIC |
| INV-D2 | `notification_state_records_sent_only_after_the_platform_accepted` | `04` section 15; RW-15 (`effects` outbox) | STATIC |
| INV-D3 | `unreadable_notification_state_is_an_error_not_default_state` | `04` section 15 | STATIC |
| INV-Q1 | `an_answer_for_a_cleared_conversation_is_rejected_with_a_typed_result` | RW-20 | STATIC |
| INV-Q2 | `a_failed_history_read_fails_the_request` | RW-20 | STATIC |
| INV-Q3 | `a_pending_question_is_keyed_by_request_id_not_by_text` | RW-20 | STATIC |
| INV-M1 | `every_stored_model_output_records_provider_and_model` | RW-06 | STATIC |
| INV-M2 | `call_deadline_enforced` (wall clock, independent of byte arrival) | RW-06 | STATIC |
| INV-M3 | `failed_call_records_usage` | RW-06 | STATIC |
| INV-M4 | `auth_and_missing_model_errors_are_classified_and_surfaced` | RW-06 | STATIC |
| INV-V1 | `a_reject_reason_is_sent_only_in_a_form_the_peers_version_can_decode` (matrix over versions) | RW-17 | VERIFIED |
| INV-V2 | `version_refusals_are_recorded_only_for_authenticated_peers` | RW-17 | VERIFIED |
| INV-U1 | `a_failed_read_returns_unknown_or_error_never_an_empty_value` (one injected failure per reader) | RW-09 covers agent availability only; otherwise no named decision | STATIC |
| INV-U2 | `removal_or_recovery_is_inferred_only_from_two_successful_observations` | no named decision; RW-14 holds facts and provenance | VERIFIED for host profiles |
| INV-K1 | `a_dismiss_that_stores_a_rule_never_leaves_the_incident_closed_without_it` (failing close) | no named decision | VERIFIED (rule first); STATIC for the audit-after-dismiss error |
| INV-K2 | `a_retry_limit_applies_only_to_its_own_reason` | RW-15 (typed events) | VERIFIED |
| INV-I1 | `everything_sent_to_a_provider_is_redacted_and_capped_at_send_time` (history included) | RW-06; `04` section 16 (prompt budget) | STATIC (follow-up replay is not re-redacted) |
| INV-I2 | `structured_output_is_parsed_whole_unknown_fields_rejected_and_count_capped` | RW-06, RW-07 | VERIFIED |
| INV-I3 | `one_admission_gate_admits_every_model_call_and_a_refusal_consumes_no_slot` | `04` section 16; no RW in 03 | VERIFIED (#112, test `a_question_refused_as_busy_takes_no_run_of_the_hour`) |
| INV-F1 | `every_mutation_call_site_is_wrapped_by_the_feedback_helper` (per call, not per file) | not carried by 03; `web/DESIGN.md` lint | STATIC |
| INV-F2 | `every_editable_form_blocks_navigation_while_dirty` | not carried by 03 | STATIC |
| INV-F3 | `a_failed_load_renders_a_load_error_never_an_empty_state` (page tests inject a failing request) | RW-21 covers presentation only; not a named rule | STATIC |

Added by the #112, #113 and verification pass:

| ID | Test name | Carried by | Evidence |
|---|---|---|---|
| INV-N1 | `provider_error_redacted` (every error string that reaches a user, chat reply or external log is redacted and clipped) | RW-06 | VERIFIED for the two question paths (#113); STATIC for other paths |
| INV-N2 | `cache_hit_uses_no_slot` | `04` section 16; no RW in 03 | STATIC |
| INV-N3 | `timeline_event_kinds_are_a_closed_type_not_parsed_strings` | RW-15 | STATIC |
| INV-N4 | `date_sensitive_tests_use_a_fixed_clock` (web and Rust) | AGENTS.md rule 11 for Rust; web not carried | UNVERIFIED |
