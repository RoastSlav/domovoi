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
2. Agent handshake reports which contracts and versions it implements, with an implementation hash and availability, not just script filenames. Availability is `Known`, `Unknown` (no session yet since hub start) or `Absent`; an unknown host is waited for, never rejected.
3. Hub builds a scoped catalogue for the incident: only capabilities that exist for the target and whose basic prerequisites are met.
4. Reasoner may ask to inspect a capability's schema or request an operation using typed arguments. The model sees a closed set of meta-tools (list and describe capabilities, request evidence, search experience, propose a plan, ask the user, finish); none of them approves or executes anything.
5. Engine checks the schema, scope, freshness, budget and current policy before dispatch.
6. Provider and agent validate again at the enforcement point.
7. Result returns as structured outcome plus bounded, redacted evidence.

Capability discovery must not expose secrets or imply permission. **Available does not mean authorized.** A capability may be visible to the model while policy still denies its execution.

## Capability families

Initial broad groups (examples, not a closed list):

- **Host observation:** processes, memory, disk, filesystems, temperatures, journal, listening sockets, service state.
- **Application/container operations:** inspect, logs, restart, health checks, queue state, deployment metadata.
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

- A capability package is signed. The agent installs it only if the host opted in (`allow_install`, off by default), the signature verifies against a key in the agent's **locally** trusted keys, and the package is compatible with the agent. The hub only relays the bytes and cannot add trusted keys, so a compromised hub cannot make an agent accept an unsigned package.
- Installing never enables or authorizes anything. Install, enable (hub) and unattended use (policy plus the host ceiling) remain three separate steps.
- **Open:** third-party or community packages. Recommendation for v1: first-party signed packages plus packages signed with the owner's own key; no arbitrary third-party registries.
- Agent self-update stays out of scope (see the catalogue flags in `11`).

## Missing capabilities

If the needed operation does not exist on the relevant host or through an integration, the reasoner should say so, explain which part of the response is blocked, and propose a typed capability/provider extension. It must not pretend that an unavailable action exists or fall back to arbitrary shell execution. The proposed extension follows the review, test, install and explicit-policy path above.

## Playbooks

Playbooks are advisory assets that can describe known diagnostic patterns, useful evidence requests, safe sequencing and verification ideas. The orchestrator and policy engine remain in control. The reasoner may depart from a playbook when evidence requires it, but it may not bypass capability validation or policy.

## Open design items

The ownership split above is decided; the exact schema and version negotiation are specified in `docs/rework` (RW-07, RW-17). Still open: discovery refresh details, provider lifecycle for hub-side integrations, third-party package trust, and how generated capability drafts are reviewed. See [`09-open-questions.md`](09-open-questions.md).
