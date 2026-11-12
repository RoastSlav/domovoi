# 10 — Capability and release roadmap

This roadmap is deliberately ambitious about the **capability universe** and conservative about what becomes autonomous. It is a product direction, not a date promise. Release labels describe quality and scope, not calendar deadlines.

## 1. Release strategy at a glance

| Milestone | Product outcome | Autonomy | Scope posture |
|---|---|---|---|
| **v0.1.0** | Reliable first public release of the current monitoring, triage, notification and human-approved skills product. | Observe + human-approved changes only. | Stabilize and document the existing product; do not delay it for the full target redesign. |
| **v0.2–v0.9** | Build the extensible foundations: capabilities, durable orchestration, knowledge, experience, policy, onboarding and provider integrations. | Human approval remains default; policy can run in shadow/dry-run. | Incremental vertical slices; no broad autonomous authority until gates pass. |
| **v1.0.0** | A genuinely adaptive self-hosted operations agent that dynamically investigates, plans, executes explicitly authorized low-risk repairs and verifies outcomes. | L2 for a reviewed operation set; first narrow L3 containment capability may ship only if safety prerequisites are met. | Complete end-to-end product for Linux + common homelab services, not every possible integration. |
| **v1.x** | Expand domain coverage, integrations, operational learning, security response and automation quality. | Grow L2/L3 by explicit capability and evidence, never by a global switch. | Add useful providers based on demand and tested contracts. |
| **v2+ / later** | Multi-site, large-scale, advanced automation and broad ecosystem. | Optional advanced autonomy with organization-grade controls. | Defer heavy infrastructure until real scale or users justify it. |

**Important release rule:** v0.1.0 should be a trustworthy release of what works today. Do not hold it hostage to building the v1 architecture. Conversely, v1.0.0 should not be declared merely because more screens and scripts exist; the adaptive investigation → policy → operation → verification loop must work end to end.

## 2. v0.1.0 — stabilize the current product

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

### Required release engineering and operational quality

- Reproducible CI, tests for critical transitions and documented supported deployment modes.
- A clean first-run path, clear configuration and secret setup, safe enrollment and recovery instructions.
- Document backup, restore, migration, upgrade, downgrade/rollback expectations and how to recover a lost hub.
- Test hub restart, agent restart, offline host, reconnect, duplicate signals, expired actions, failed skill and failed verification.
- Verify API schema and UI stay in sync; migrations are tested from an empty DB and the previous release DB.
- Define supported Linux distributions/architectures and explicitly document unsupported platforms.
- Audit and redact outputs; avoid logging tokens, secrets, skill args containing secrets or raw credentials.
- No known critical security defect; dependency and container release pipeline checks pass.
- Document action risk and the fact that current mutating actions require human approval.

### UI scope for v0.1.0

The current main-branch UI overhaul is the intended v0.1 visual and usability baseline. For v0.1.0, prioritize correctness and completeness over introducing the entire v1 control plane:

- Finish and validate the current Overview, Incidents, Approvals, Hosts, Chat, Skills, Notes, Logs, Settings and onboarding flows.
- Keep the warm light/dark visual identity, semantic color-coded messages, accessibility, responsive layouts and documented component/design rules.
- Keep the current Ctrl/Cmd+K command palette and page/entity navigation foundation.
- Ensure every current mutating action has clear target/effect/approval/verification feedback and every page has truthful loading/empty/error/offline states.
- Add/retain browser-level tests for the real workflows, both themes and busy-data conditions.
- Do not block v0.1.0 on the new orchestration canvas, full Operations model, capability marketplace, policy simulator or full Operator.

### Small foundations that are worth including if they do not destabilize the release

- A capability inventory view derived from current read tools and skills (even if internally still backed by the existing types).
- Stable capability IDs/categories as metadata for later migration.
- Clear distinction between **unknown**, **unavailable**, **failed** and **not applicable** in host profiles where current UI can support it.
- Collect capability/provider version and host suitability metadata during agent registration.

### Explicitly not required for v0.1.0

- Autonomous mutation or security containment.
- A universal workflow/playbook engine.
- A full knowledge graph or dedicated graph database.
- Vector search, embeddings or a separate vector database.
- Dynamic LLM-created capabilities, self-installing skills or generated code execution.
- Kubernetes, Windows/macOS, cloud orchestration or full router management.
- Multi-user role-based access control beyond the current intended deployment model.

**Why:** v0.1.0 should prove that monitoring, triage, approval and a constrained execution primitive are dependable before increasing the intelligence or authority of the system.

## 3. v0.2–v0.9 — build the foundations for v1

These are not all separate releases; order by dependency and deliver in usable slices.

### 3.1 Capability platform

- Define capability descriptors: stable ID, version, provider, description, input/output schemas, target scope, effects/risk, prerequisites, supported platforms, timeout, rate limit, idempotency, verification, rollback/expiry and policy hooks.
- Wrap existing native read tools and skills as the first capability providers.
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
- Add typed entities, relationships and claims/facts with provenance, confidence, scope, freshness, validity and supersession.
- Start entities with hosts, services, containers, storage, networks, routers, volumes, integrations, capabilities and users/owners.
- Add relationship types such as runs_on, depends_on, exposes, stores_data_on, reachable_through, protected_by, backed_up_to and managed_by.
- Add a structured experience stream for phase transitions, evidence requests, hypotheses, plans, policy outcomes, action attempts, verification, rollbacks and user decisions.
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
- Rules at global, host, capability category and capability/operation scope; explicit deny wins.
- Model target constraints, source conditions, time windows, operation limits, TTL, rate limits, blast radius and required recovery path.
- Separate human approval from autonomous policy authorization.
- Provide shadow mode to show what would have happened without executing.
- Add policy test fixtures, simulation and an autonomy UI showing inherited versus explicit decisions.
- Make policy versioned, revocable and locally verifiable.

### 3.6 First safe automation

- Choose reversible low-impact operations with reliable postconditions (for example, restart one known service/container, reload a specific config, renew a certificate through a supported provider, unload an unused inference model).
- Add execution permits, idempotency keys, target revalidation, concurrency locks, operation budgets and postcondition checks.
- Add rollback or automatic expiry wherever possible.
- Use per-host opt-in; default to human approval; explicit deny for destructive operations.
- Run drills before granting autonomy to real hosts.

### 3.7 UI and interaction foundations

- Introduce the target UI vocabulary and canonical object routes for Incident, Operation, Host, Capability, Knowledge, Policy and Integration.
- Build the shared operation activity/event presentation model so graphs, timelines and chat can render the same durable state.
- Add the global Operator side panel and full workspace over the same conversation and operation state; do not create a chat-specific executor.
- Build a first Operations list/detail view with phase-level progress before the complete graph editor/visualizer.
- Add the capability catalog/installed inventory and a safe agent installation flow through the UI.
- Add structured Knowledge browser/detail and initial provenance editing.
- Add granular policy editor and read-only policy simulator, including historical scenario reconstruction.
- Add the first detailed/standard/essential presentation preferences and customizable Control Room widgets.
- Add a page design-spec and interaction/motion quality gate to implementation planning.

### 3.8 First provider expansion

- Improve Linux host and systemd provider coverage.
- Docker and Docker Compose lifecycle, logs, stats, inspect and controlled stack operations.
- Network diagnostics: DNS, gateway, TCP/HTTP, TLS expiry, routes and listeners.
- Backup systems and storage visibility (read/verify first; no broad delete operations).
- Optional read-first providers for reverse proxies, VPNs, monitoring systems and router/firewall APIs.

**Why this stage:** it establishes the common contracts, durable memory and policy machinery that make new capabilities additive instead of one-off LLM prompts and scripts.

## 4. v1.0.0 — target adaptive operator

v1.0.0 should ship when the end-to-end target loop is complete and production-safe for the supported platform set.

### v1.0 product requirements

1. **Discoverable capabilities:** agents and integrations advertise typed, versioned capabilities with availability, prerequisites, effects and safety metadata.
2. **Dynamic investigation:** the reasoner can ask for evidence beyond the initial context; the engine validates, budgets, executes and returns it.
3. **Durable orchestration:** phase/cycle state, requested evidence, plan steps, deadlines and outcomes survive restart and disconnection.
4. **Adaptive plans:** the model composes plans from available capabilities and can choose monitor, ask, escalate, human approval or autonomous execution.
5. **Deterministic authorization:** every mutation gets a policy decision independent of LLM reasoning; human and autonomous authority are distinct.
6. **Granular autonomy:** per host/category/capability/operation, with deny-wins, constraints, rate limits, expiry and transparent explanation.
7. **L2 operation set:** at least a practical set of low-risk, reversible operations can execute automatically on explicitly opted-in hosts.
8. **L3 security response:** a first narrowly scoped containment operation may be included only if trusted management paths, TTL, allowlists, enforcement-point verification and recovery drills all pass. Otherwise ship the policy/capability framework and keep L3 disabled until a later point release.
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
- Policy management supports granular host/capability/operation permissions and historical/current/proposed simulation using the live evaluator without side effects.
- Knowledge UI supports entities, facts, relationships, provenance, freshness, notes and suggestions; permission/authority remains separate.
- Detailed is the design baseline; Standard, Essential and Custom presentation modes do not remove audit/authority data or change behavior.
- Major pages have approved page specifications covering P0–P3 priority, space class, content containment, interactions, live states, motion, responsive behavior and acceptance criteria.
- Cross-page workflows are browser-tested: incident → operation → capability → policy result; Operator → operation; capability install → enable → authorize; historical scenario → policy simulation → apply draft.

### v1.0 acceptance gates

- A novel incident can be investigated without a prewritten playbook, using only discoverable capabilities.
- Missing evidence is represented honestly; failed/unavailable sources are not interpreted as healthy.
- A plan cannot execute an operation that is absent, stale, out of scope or not authorized.
- A policy deny cannot be overridden by the LLM, a playbook, a learned preference or a historical success.
- A failed verification starts a new reasoning cycle with the failed attempt in context; it does not repeat the same action blindly.
- A hub restart or agent reconnect cannot duplicate a non-idempotent action.
- Offline operation is limited to explicitly pre-authorized and unexpired operations.
- Every operation has a traceable proposal/authorization/dispatch/result/verification sequence.
- Security containment cannot block configured management paths; if the trusted path is unknown, automatic containment is refused or narrowed to a safe alternative.
- Policy changes, knowledge changes and experience records are visibly distinct.
- Backup/restore and data migration tests pass for all persistent stores.
- A defined suite of realistic positive and negative incident drills passes.

## 5. Post-v1 capability expansion

The following are high-value directions, not v1.0 blockers. Prioritize using real incidents and user demand.

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
- **Protective/security:** temporary block, isolate, restrict exposure. L3 only with dedicated conditions, trusted-path protection, TTL, rate limits and recovery.
- **High-impact write:** upgrade, deploy, reconfigure network, rotate credentials, change storage topology. Human approval by default; may require staged plans and rollback.
- **Destructive:** delete data, format, prune, wipe, destroy volume, remove backup. Deny by default; never included in generic autonomy.

Risk is contextual. Restarting a disposable container is not the same as restarting a database during a migration. Capability metadata and policy must consider target, dependencies, environment state and recovery—not only a static label.

## 8. Delivery rules

- Ship capabilities as end-to-end slices: discover → explain → authorize → execute → verify → audit.
- Implement read-only observation before mutation for every new provider.
- Add capability-specific tests and at least one incident drill before enabling it in autonomy.
- Do not expose a capability for autonomy merely because a script exists or because the model has used it successfully with approval.
- Track operation idempotency, concurrency, timeout, cancellation, partial failure, uncertain result and compensation.
- Prefer an integration with a stable API over shelling out to fragile CLI parsing; keep CLI providers typed and bounded when no API exists.
- Do not add a database, broker, workflow engine or embedding service without a measured requirement.
