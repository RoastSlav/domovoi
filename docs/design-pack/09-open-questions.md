# 09 — Open questions and recommended defaults

The high-level architecture is coherent enough to proceed. On 2026-11-13 the owner answered the questions that blocked the rework (`D-01`..`D-29` in [`docs/rework/99-open-decisions.md`](../rework/99-open-decisions.md)); they are listed first and are not open. One owner question is open: **O-30**, skills whose manifest sets `run_as` to another user (below). What remains besides it are detailed-design questions (kept open so later design work does not silently invent answers) and research and evidence items (`R-1`..`R-10`, [`99-open-decisions.md`](../rework/99-open-decisions.md) §C, summarised below). Technical detail for every answered item is in the linked `RW` decision in [`docs/rework/03-architecture-decisions.md`](../rework/03-architecture-decisions.md).

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
| **Q0.3a** Exact first containment set | **D-17, D-22** | `firewall.block_source` only, with a kernel-enforced nftables timeout and a 30 s two-step confirmation. `container.pause` and `unit.stop_temporary` are DEFERRED: an agent timer is not reliable automatic expiry; reconsider in v1.x when the checklist in `docs/rework/04 §9.3` is met, which needs a helper ADR adding a typed operation with independent expiry (the v1 helper has no pause, unpause or stop). Every containment expires independently of the main agent, and nothing is enabled unattended before the drills pass. | RW-13 |
| **Q0.5** How is management access protected during containment? | **D-28** | Resolved. Unattended containment on a host requires owner-confirmed, current management paths; unknown, stale or conflicting means refused; the mandatory exclusions have one authoritative source, the root-owned `[safety]` section of `helper.toml`, which the helper enforces; the helper computes and signs its effective set, the agent relays it unchanged and the hub verifies it against a key the owner pinned after comparing the fingerprint printed on the host, and a path can be confirmed only when that verified set covers it; the UI shows paths, coverage, provenance, freshness and what is blocked. Monitoring and other human-approved work are unaffected. | RW-37 |
| **Q0.6** Trust model for external providers and third-party packages | **D-16** | First-party release key plus owner keys installed locally on the agent; the hub cannot add or replace keys; a detached signature over a canonical message, verified before extraction; a rollback needs a currently trusted key. Third-party registries are DEFERRED; hub-side integrations are DEFERRED (`docs/rework/06`). | RW-26 |
| Ceiling scope and honesty | **D-18, D-27** | The agent ceiling (`agent.toml`, agent-enforced) covers the exact capability id, explicit targets, rate and constrainable arguments; no entry means no policy-authorized execution. Containment max TTL, max active and the mandatory management-path exclusions are held in the root-owned helper `[safety]` section and enforced by the helper (RW-12, RW-37). `authorization_kind` is a hub label, and a compromised hub can label a request `human` and bypass the ceiling (accepted v1 limitation), but the agent's catalogue, manifest, argument, `allow_risk` and mutation rate-limiter checks apply to all mutating executions. Owner-signed approvals are DEFERRED. | RW-12, RW-38 |
| **Q1.1a** Agent support window | **D-20** | There is no support window. Exact protocol major.minor match, checked only after the agent has proven its identity: older and newer minors are rejected with the existing reason, same-protocol patch releases are compatible, and application release numbers are not a compatibility dimension. No legacy adapter. `Incompatible` is persisted on the agent record, survives a hub restart, keeps history, and an incompatible agent idles instead of reconnecting. Coordinated upgrade, hub first, then agents through the install procedure. Exactly one protocol bump before v1 (1.1 → 1.2 in M1); F3 changes no wire format. No agent self-update. A revoked-build list is DEFERRED and separate. | RW-17, RW-42 |
| **Q1.6** Default retention values | **D-21** | Signals 30 d; incident timeline 90 d; operation journal 180 d; raw execution outputs 30 d; operations and summaries retained; audit retained; retracted fact content purged after 30 d; conversations 180 d; notifications 30 d; 7 nightly and the last 3 pre-migrate backups. Deleted data stays in backups until they rotate. | RW-28 |
| Docker access | **D-24** | No general Docker socket write for the agent; a narrow first-party helper with its own ADR; Docker write capabilities unavailable for unattended use until then. `docker.write` is restart only; no pause or unpause. | RW-39 |
| Agent privilege | **D-29** | Three identities: the control agent runs as `domovoi-agent`; skills and packages run through a socket-activated skill runner as `domovoi-skill`, with no access to the helper socket, agent state or any of `/etc/domovoi` other than the read-only `skills/` tree (all installed skills and packages stay visible to it read-only, a known limitation; R-9). Privileged host operations go only through one narrow, root-owned helper installed and updated by the owner-controlled OS path, never by capability packages; its allowlist bounds operations and is not execution authorization. Root installs are labelled and get no unattended execution. | RW-39, RW-12, RW-26 |
| Graph playback | **D-25** | Ships last in U2; the timeline and history are fully useful without it. | RW-21 |

## Open owner question

| ID | Question | Recommended default | Blocks |
|---|---|---|---|
| **O-30** | What happens to skills whose manifest sets `run_as` to a user other than the skill identity `domovoi-skill`, after D-29? **Still open.** Options: (a) unsupported, reported `Unavailable(run_as_unsupported)`; (b) the skill runner or helper switches users for allowlisted pairs, which needs a root-capable component executing package-defined programs and reopens the root path D-29 closes; (c) owner sudoers rules used inside skills, which gives `domovoi-skill` root-equivalent reach and bypasses the helper allowlist. | (a) unsupported (recommended, not decided) | H1 migration (skills impact) and package guidance |

## Priority 0 — resolve before implementing autonomous actions

None. The four questions that used to sit here (Q0.2a, Q0.3a, Q0.5, Q0.6) were answered by owner decisions D-19, D-17, D-28 and D-16 (listed above). What remains before autonomous actions are enabled is evidence, not decisions: the nftables and helper privilege spike (R-2), the helper attestation spike (R-10) and the drills that owner decision D-22 requires.

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
| R-9 | Skill-runner isolation on supported platforms: systemd socket activation with `Accept=yes`, `User=`, `InaccessiblePaths=`, `RuntimeMaxSec=`; container deployments and hosts without systemd (Unraid); verification that `domovoi-skill` cannot reach the helper or agent state; the effective filesystem view of a skill process (attempts to list and read `/etc/domovoi/*`, `/var/lib/domovoi-agent`, `/var/lib/domovoi-helper`, `/run` sockets and other secret-bearing paths, and to write to package and skill sources); feasibility of per-package narrowing. Until done, those platforms run without skills (never skills as the control agent) or keep the labelled root agent | H1 |
| R-10 | Helper safety attestation: canonical encoding of the effective set, key storage and generation at install, fingerprint enrollment UX, nonce lifetime, performance of recomputing the set per request, behaviour across helper restarts and reboots. Until done, unattended containment stays blocked | M9, H1 (safety part) |
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

Do not block general architecture work on Priority 2 questions. Do not implement L2 execution before the policy evaluator, host ceiling and target binding exist, and do not enable L3 containment on any host before the owner has confirmed that host's management paths and a verified helper attestation shows them covered (D-28, RW-37), and the drills required by D-22 pass. For Priority 1, specify only the parts required by the next vertical slice; keep the rest extensible.
