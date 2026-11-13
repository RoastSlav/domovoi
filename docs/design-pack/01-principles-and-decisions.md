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
| D-18 | Ceiling scope: exact capability id, explicit targets, maximum rate, declared constrainable arguments, optional, stricter-only agent values for containment maximum TTL and maximum active (the helper's `[safety.containment]` values are the hard maxima, enforced for every caller), mandatory management-path exclusions (held in the root-owned helper `[safety]` section and enforced by the helper). No entry, no policy-authorized execution. | 02, 05 | RW-12, RW-37 |
| D-19 | Incomparable conflicting rules resolve Deny > RequireApproval > Allow; publication shows the conflicts, equal-scope ones as well, and needs acknowledgement. | 05, 09, 14 | RW-10 |
| D-20 | Exact protocol major.minor match, checked only after the agent has proven its identity: older and newer minors are rejected, same-protocol patch releases are compatible, and application release numbers are not a compatibility dimension. No legacy adapter and no support window. Coordinated upgrade: hub first, then agents through the install procedure. The `Incompatible` host state is persisted, survives a hub restart, is visible and keeps history; an incompatible agent idles instead of reconnecting. One protocol bump before v1 (M1). No agent self-update. | 02, 10, 13, 14, 15, 17, 18 | RW-17, RW-42 |
| D-21 | Retention defaults, all configurable. Deleted data leaves the live database, search and model context, and stays in backups until they rotate. | 07, 14 | RW-28 |
| D-22 | Every containment expires independently of the main agent; connectivity-affecting containment also needs an independent watchdog; nothing is enabled unattended until expiry, recovery, watchdog, outcome-unknown handling and drills exist. | 05, 10 | RW-13 |
| D-23 | A policy Deny is not offered for approval; the owner must change the policy. No bypass. | 05, 14 | RW-10 |
| D-24 | No general Docker socket write access for the agent; a narrow first-party helper with its own ADR; Docker write capabilities are unavailable for unattended use until then (the helper is part of D-29's design). The v1 `docker.write` group is restart by allowlisted container id only; there is no pause or unpause. | 03, 05, 10, 11 | RW-39 |
| D-25 | Historical graph playback ships last in U2; the timeline and history are fully useful without it. | 10, 14, 15, 17 | RW-21 |
| D-26 | Locked Deny (default off): an owner-declared prohibition that narrower rules cannot override; visible in the editor, simulator, explanation and trace; step-up to change. | 05, 14, 17 | RW-10 |
| D-27 | Agent-side mutation rate limits apply to all mutating executions, human- and policy-labelled, separately from the ceiling. | 02, 05, 14 | RW-38 |
| D-28 | Unattended containment on a host requires owner-confirmed, current management paths; unknown, stale, conflicting, uncovered or unverifiable means refused. The mandatory exclusions have one authoritative source, the root-owned `[safety]` section of `helper.toml`; the helper enforces them and establishes the dynamic elements itself; the hub cannot remove or widen them. The helper computes and signs its effective safety report, the agent relays it unchanged, and the hub verifies it against the exact helper public key it pinned only after the owner compares the fingerprint the hub computed from the received key with the one printed on the host (the 16-hex key id is a label, never trust). A path can be confirmed only when a verified helper attestation shows it is covered, and confirmations, permits and human-approved containment bind the full attested safety identity, including `boot_id`, so a permit does not survive a reboot; without verification only a human may approve, on the unverified human path (step-up, typed confirmation, "not protected" list; RW-37; the safety part waits on research R-10). The UI shows paths, coverage, provenance, freshness and what is blocked; monitoring and other human-approved work are unaffected. | 05, 06, 09, 14 | RW-37 |
| D-29 | Three identities: the control agent runs as the unprivileged `domovoi-agent`; skills and packages run through a socket-activated skill runner as `domovoi-skill`, which cannot reach the helper socket, the skill-runner socket, agent state or the agent key, or any of `/etc/domovoi` other than the read-only `skills/`; privileged host operations go only through one narrow, root-owned helper installed and updated by the owner-controlled OS path, never by capability packages or skills. The helper allowlist bounds which privileged operations exist; it is not execution authorization. Root installs are labelled and get no unattended execution. Skills whose `run_as` is not `domovoi-skill` are an open owner question (O-30; recommended `Unavailable(run_as_unsupported)`). | 02, 03, 05, 10, 13, 14 | RW-39, RW-12, RW-26 |

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
