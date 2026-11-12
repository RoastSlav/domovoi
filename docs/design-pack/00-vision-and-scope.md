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
