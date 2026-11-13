# 10 — Capability and release roadmap

This roadmap is deliberately ambitious about the **capability universe** and conservative about what becomes autonomous. It is a product direction, not a date promise. Release labels describe quality and scope, not calendar deadlines. Milestone names (F1–F4, M1–M9, U1–U4, P1, H1) follow the implementation direction in [`docs/rework/06-implementation-roadmap.md`](../rework/06-implementation-roadmap.md) and [`07-implementation-prompt-index.md`](../rework/07-implementation-prompt-index.md); this document states the product outcome of each, and `docs/rework` holds the technical scope. Whether a milestone is ready for an implementation prompt is a separate question from whether its architecture is defined: see "Readiness" in [`docs/rework/06-implementation-roadmap.md`](../rework/06-implementation-roadmap.md).

## 1. Release strategy at a glance

**Owner decision D-01 (RW-02): there is no separate v0.1 release.** The earlier plan to stabilize the current product as a v0.1.0 release before the rework is withdrawn. Its quality requirements are kept, as foundation slices that come first inside the rework. The product on the branch stays usable after every slice, but nothing is tagged before the first rework release.

| Stage | Product outcome | Autonomy | Scope posture |
|---|---|---|---|
| **Foundation slices F1–F4** | The current product becomes trustworthy and restart-safe: atomic state and audit, recovery order, protocol tolerance, reliability and operational readiness. | Observe + human-approved changes only. | The former v0.1.0 quality bar, delivered as slices; no release tag. |
| **Milestones M1–M7 and the UI/package track** | Build the extensible foundations: execution boundary, capability contracts, operations, dynamic investigation, knowledge and experience, the Operator, policy in shadow, the Operations UI, graph, profiles and capability install. | Human approval remains default; policy runs in shadow/dry-run. | Incremental vertical slices; no autonomous authority until M8 gates pass. |
| **H1 Narrow privileged helpers** | The Docker and nftables helpers that M8 and M9 rely on exist, each with its own ADR; the agent never gets a general Docker socket write (owner decision D-24). | None by itself. | After P1 and the research spikes R-2 and R-3; before M8's Docker contracts and M9. |
| **M8 and M9 (inside v1.0.0)** | Bounded L2 autonomy, then L3 containment, which in v1 is `firewall.block_source` only. | L2 for a reviewed operation set; L3 under a general Contain class. | Gated by drills and the host-local ceiling. |
| **v1.0.0** | A genuinely adaptive self-hosted operations agent that dynamically investigates, plans, executes explicitly authorized low-risk repairs and contains narrowly scoped threats, and verifies outcomes. | L2 and L3 as above; nothing Destructive is ever policy-allowed. | Complete end-to-end product for Linux + common homelab services, not every possible integration. |
| **Post-v1 (DEFERRED)** | The deferred items in section 5 (offline emergency permits, owner-signed bundles and approvals, host tags, multi-user and roles, stronger audit anchoring, third-party registries and more). | Grow L2/L3 by explicit capability and evidence, never by a global switch. | Each needs its own design; none is promised for v1. |
| **v1.x** | Expand domain coverage, integrations, operational learning, security response and automation quality. | As above. | Add useful providers based on demand and tested contracts. |
| **v2+ / later** | Multi-site, large-scale, advanced automation and broad ecosystem. | Optional advanced autonomy with organization-grade controls. | Defer heavy infrastructure until real scale or users justify it. |

**Important release rule:** v1.0.0 should not be declared merely because more screens and scripts exist; the adaptive investigation → policy → operation → verification loop must work end to end. Foundation work is never skipped to reach a visible feature sooner.

**Readiness rule.** An architecture being defined is not the same as a milestone being ready for an implementation prompt. A milestone is prompt-ready only when its architecture, owner decisions, research evidence and predecessor milestones are all in place. H1 and M9 still wait for the nftables and Docker privilege spikes (R-2, R-3) and their ADRs, and U2 waits for the graph-library spike (R-1); see "Readiness" in `docs/rework/06`.

## 2. Foundation slices — the quality bar of the former v0.1.0

These requirements used to define a separate v0.1.0 release. Under owner decision D-01 they are delivered as the first slices of the rework, in four groups. Each is also a prerequisite for something later, which is why they come first.

| Slice | Product outcome | Technical scope |
|---|---|---|
| **F1** | A state change, its journal entries and its audit record commit together or not at all; the audit chain exists; crash tests exist. | One transactional unit of work, hash-chained audit, atomic action and incident writes, crash harness (RW-03, RW-27). |
| **F2** | After a restart nothing stale runs and nothing waits forever: approvals are re-checked for freshness, hosts that have not reconnected are waited for, verification and triage resume, notifications are not lost. | Recovery order, durable timers, delivery durability (RW-09, RW-30). |
| **F3** | Hub and agent talk only on the exact same protocol major.minor. A mismatched agent is rejected with a reason and its host is shown as `Incompatible` (upgrade required, history kept, not offline); same-protocol patch releases communicate; unknown values degrade gracefully. | Exact protocol version and tolerant decoding (RW-17, owner decision D-20); no support window and no legacy adapter. Must land before any protocol minor bump. |
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
| **M1 Execution foundation** | No execution is duplicated or silently lost: idempotency, `OutcomeUnknown`, agent action journal, plan hash v2, one execution boundary. | 3.6 |
| **M2 Capability contracts** | One hub-owned registry the model, UI and executor share; available is not authorized. | 3.1 |
| **M3 Operation aggregate, journal and SSE v2** | Durable, resumable operations; triage becomes an Investigate operation with identical behaviour; live updates that replay after a gap. | 3.2 |
| **M4 Dynamic investigation** | Typed meta-tools, validated plans, layered context, no text-protocol proposals. | 3.2 |
| **M5 Knowledge and experience** | Typed facts and relations over inventory; resolution summaries, full-text search and similar past operations. | 3.3 |
| **M6 Operator** | Durable conversations as side panel and workspace over the same state. | 3.7 |
| **M7 Policy in shadow** | Versioned policy, pure evaluator, shadow decisions, historical simulator. | 3.5 |
| **UI track U1–U4, P1** | Operations list/detail with step timeline first (U1), interactive graph (U2, with historical playback last), presentation profiles (U3), policy, knowledge and capability pages (U4), capability packages and install from the UI (P1). All in v1. | 3.7 |
| **H1 Narrow privileged helpers** | First-party Docker and nftables helpers with allowlists and one ADR each; the prerequisite for Docker write capabilities and for containment. | 3.6 |
| **M8 Bounded autonomy (L2)** | Policy permits inside the host-local ceiling, target binding, circuit breaker, pause switch, first reviewed L2 operations. | 3.6 |
| **M9 Containment (L3)** | A general Contain class whose first and only v1 capability is `firewall.block_source`: kernel-enforced expiry, a 30 s two-step confirmation, management-path protection and drills. | 3.9 |

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

### 3.6 Execution foundation (M1), helpers (H1) and first safe automation (M8)

- M1: idempotency keys, an agent action journal, `OutcomeUnknown` for lost results, plan hash v2 (binding the entry-script bytes and resolved target), one execution boundary with an enforced rule that nothing else dispatches.
- M8: choose reversible low-impact operations with reliable postconditions (for example, restart one known service/container, reload a specific config, renew a certificate through a supported provider, unload an unused inference model).
- M8: policy permits, target revalidation and binding to the operation's pinned subjects, concurrency locks, operation budgets, postcondition checks, a circuit breaker and a human-only pause switch.
- Add rollback or automatic expiry wherever possible.
- Use per-host opt-in through the host-local ceiling; default to human approval; Destructive operations are never policy-allowed.
- H1: narrow first-party helpers (a Docker helper against a root-owned allowlist, an nftables helper limited to set elements in table `inet domovoi`), each with its own ADR, after the privilege-path spikes (R-2, R-3).
- The agent gets no general Docker socket write access, which would be root on the host. Docker write capabilities, including `container.restart`, stay unavailable for unattended use until the Docker helper and its ADR exist (owner decision D-24); `unit.restart` does not depend on it.
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
- Show a host whose agent speaks another protocol minor as `Incompatible`, with an upgrade-required explanation, the exact required protocol and the agent version, history kept and never shown as offline (owner decision D-20).
- Policy editor: locked-Deny styling and a publication validation report listing incomparable conflicts that need acknowledgement (D-19, D-26). Host pages: management-path confirmation with provenance, freshness and which containment is blocked (D-28). Approval cards show an agent-side rate-limit refusal (D-27). The audit page shows legacy and chained history separately (D-14).

### 3.8 First provider expansion

- Improve Linux host and systemd provider coverage.
- Docker and Docker Compose lifecycle, logs, stats, inspect and controlled stack operations.
- Network diagnostics: DNS, gateway, TCP/HTTP, TLS expiry, routes and listeners.
- Backup systems and storage visibility (read/verify first; no broad delete operations).
- Optional read-first providers for reverse proxies, VPNs, monitoring systems and router/firewall APIs.

### 3.9 Containment — L3 (M9, inside v1.0.0)

- **Owner decision D-04:** L3 ships in v1 as milestone M9, under a general **Contain** class whose first and only v1 capability is `firewall.block_source` (owner decision D-17). Prerequisites: M8 complete (permits, ceiling, target pinning, journal, verification, circuit breaker), H1 (the nftables helper), the privilege spike R-2, owner-confirmed management-path facts (D-28) and the drill suite.
- **First and only v1 capability (owner decisions D-17, D-22):** `firewall.block_source`, an nftables set element with a kernel-enforced timeout and a 30 s two-step confirmation: the first element lives 30 s and is extended to the full TTL only when the hub confirms, so an unconfirmed block expires by itself.
- **DEFERRED:** `container.pause` and `unit.stop_temporary` (an agent timer is not reliable automatic expiry; reconsider in v1.x when the checklist in `docs/rework/04 §9.3` is met) and the other containment candidates (container network isolation, SSH session termination, account lock). See `docs/rework/06` "Deferred and after-v1".
- **Triggering:** a deterministic detector's typed signal whose structured fields name the target, and that target is the operation's pinned subject. Free-form model output never triggers or targets automatic containment; model recommendations go through human approval.
- Every containment has an expiry, mandatory management-path exclusions, a cap on simultaneous containments, verification that management access still works, and outcome-unknown handling. Unattended containment on a host also requires owner-confirmed, current management paths (owner decision D-28). Offline emergency permits are DEFERRED (not in v1).

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
- Hub and agent communicate only on the exact same protocol major.minor; an older or newer agent is rejected with a reason and its host is shown as `Incompatible` with history kept (owner decision D-20).
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
