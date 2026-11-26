# 0040. The model is offered a closed set of tools

- Status: Accepted
- Date: 2026-11-26

## Context

The triage model reads untrusted text, and its output is data, never a command ([0007](0007-human-only-approvals.md)). Whatever the model can call is therefore the whole of what a prompt injection can reach. A tool list assembled from registries or configuration would grow unnoticed.

## Decision

- The model is offered the closed enum `MetaTool` in `domovoi-core`: `list_capabilities`, `describe_capability`, `request_evidence`, `search_experience`, `propose_plan`, `ask_user` and `finish`, in that order. No variant approves, authorizes, runs a skill, dispatches or writes.
- `request_evidence` takes a closed `ReadTool` argument (the read-only tools: Docker reads, host overview, journal, listening ports, SMART report, incident history, notes search and read-only skills). `ReadTool` has no variant that changes state.
- `ToolSet` picks the subset for a conversation: an investigation is offered all seven (six without `ask_user` where an answer cannot be given), and follow-up questions and chat are offered the four read meta-tools.
- A name outside the offered set is answered `unknown tool` and changes nothing.
- `propose_plan` takes at most three steps, each naming a capability, string arguments, an optional host and target by name, a reason and an optional retry reference. A plan validator checks every step against the pinned scope, the hub-owned contract, availability, budget and history, and the engine checks it again when it stores the step. Nothing the model writes is an identity, a fingerprint or a plan hash.
- `search_experience` takes only a query and searches past operations of the conversation's own host; its results are labelled "historical evidence, not authorization".

## Consequences

- Gate tests assert the catalogue contents and that no variant can approve or run anything. They are extended, never weakened.
- Adding a tool is an edit to a closed enum and its gate test, which a reviewer sees, and a new surface that offers tools to a model needs its own ADR.
- A model cannot widen its own reach: a capability it names is resolved by the registry ([0024](0024-capability-registry.md)) and every execution still needs an authorization ([0025](0025-operation-aggregate.md)).

## Alternatives considered

- Building the tool list from the capability registry: the registry grows with every skill an agent announces, so the model's reach would follow the host.
- Exposing a generic "run skill" tool: removes the plan validator and the approval from the model's path.
