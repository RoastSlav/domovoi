# 0024. The capability registry

- Status: Accepted
- Date: 2026-11-19

## Context

Which contracts a host can run is a hub-owned answer, not the agent's. The compiled contracts, the agent's catalogue and `hello.skills`, the host's own status words, the owner's per-host enablement, the raise-only skill floors and the agent's ceiling summary all bear on it. A wrapper around the agent link that hid disabled skills would answer only part of that and would put policy in a transport-layer object.

## Decision

- The hub has one capability registry. It combines those inputs into one availability answer per `(host, contract)`. The engine asks it through the `CapabilityRegistry` port. It evaluates no policy, approves nothing and dispatches nothing.
- Enablement is enforced at the two boundaries the engine owns, `collect_evidence` and the execution path ([0022](0022-single-execution-boundary.md)), which refuse a disabled skill with the same sentence taken from one constant. The disabled-skill setting is stored as `host.<name>.skills` with `{"disabled": [..]}`.
- The registry sees the agent only through `LinkCapabilities`, its view of the raw link (availability, announced catalogue hash, catalogue pages, the status words, announced skills), built once in `crates/domovoi-hub/src/runtime/caps.rs`. The registry and its module never name the link (`EXEC-4`).
- `AgentServer::link` is the one accessor for the raw link, reachable only inside the hub crate. The raw operations `run_tool`, `start_skill`, `skill_result` and `run_check` are named once each, in the hub link's own `AgentLink` implementation and in `runtime/caps.rs`.
- A hub module that wants agent facts asks the capability layer, not the link.

## Consequences

- The engine has one more port in its dependency structs, and the testkit has a `FakeCapabilityRegistry` beside `FakeAgentLink`.
- An `available` row is a report about missing pieces, never a promise that a run will proceed or an authorization.

## Alternatives considered

- A wrapper around the link that narrows `skills()`: it answers only part of the question and hides the reason a contract is unavailable.
- Letting each caller combine the inputs: the answers would drift.
