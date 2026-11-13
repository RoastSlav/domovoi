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
- **Application/container operations:** inspect, logs, restart, health checks, queue state, deployment metadata. Docker write operations go only through the narrow, root-owned first-party helper (typed `docker.read` and `docker.write` operation groups against a root-owned allowlist, where `docker.write` is restart by allowlisted container id only and there is no pause or unpause; owner decisions D-24, D-29, RW-39); the agent runs unprivileged and gets no general Docker socket write access, and Docker write capabilities stay unavailable for unattended use until the helper and its ADR exist. Container targets are bound by full id and systemd unit targets by canonical id plus a hash of the fragment and drop-ins; generated and transient units are not valid mutation targets, and files a unit references are not bound (RW-34).
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
- **Privileges (owner decision D-29, RW-26, RW-39).** Packages install into `packages_dir` (not `skills_dir`), written by the agent and read-only to the skill identity, and run only through the skill runner as `domovoi-skill`: never root and never `domovoi-agent`. They can never install or modify the root-owned helper, the skill-runner units or its allowlist; that is a separate step through the owner-controlled OS install path. Signing grants execution as `domovoi-skill` only. Where no skill runner exists (R-9) skills are unavailable rather than run as the control agent. A root install is labelled and gets no unattended execution.
- **DEFERRED:** third-party or community packages and registries. Anyone publishing a package could run code as `domovoi-skill` after one click, so v1 has none (`docs/rework/06` "Deferred and after-v1").
- **DEFERRED, not planned:** agent self-update. It violates the rule that the hub cannot push code to a host; upgrades follow the install procedure (owner decision D-20; see the catalogue flags in `11`).

## Missing capabilities

If the needed operation does not exist on the relevant host or through an integration, the reasoner should say so, explain which part of the response is blocked, and propose a typed capability/provider extension. It must not pretend that an unavailable action exists or fall back to arbitrary shell execution. The proposed extension follows the review, test, install and explicit-policy path above.

## Playbooks

Playbooks are advisory assets that can describe known diagnostic patterns, useful evidence requests, safe sequencing and verification ideas. The orchestrator and policy engine remain in control. The reasoner may depart from a playbook when evidence requires it, but it may not bypass capability validation or policy.

## Open design items

The ownership split above is decided; the exact schema and version negotiation are specified in `docs/rework` (RW-07, RW-17). Package trust is decided (owner decision D-16) and protocol compatibility is exact-match (owner decision D-20). Still open: discovery refresh details, the provider lifecycle for hub-side integrations (DEFERRED, `docs/rework/06`), and how generated capability drafts are reviewed. See [`09-open-questions.md`](09-open-questions.md).
