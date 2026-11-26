# 0022. One execution boundary and one read boundary

- Status: Accepted
- Date: 2026-11-17

## Context

A skill must reach an agent from exactly one place, after a human decision, and a host's tool output must be read from exactly one place. Nothing in the code or CI said where those calls may live, so a new transport, a scheduler or an admin endpoint could add a second path to the raw agent link and no check would notice.

## Decision

- A skill is dispatched only by the operation service's `execute_step(AuthorizedStep)` in `crates/domovoi-engine/src/operations/execute.rs`. `AuthorizedStep` has one constructor, `of`, called at one site (`operations/effects/dispatch.rs`); it carries the step and its execution id, is taken by value and is never stored. The writer records the dispatch before the request is sent, and refuses a step with no stored human approval, a policy authorization the contract does not allow, a different plan hash and an expired window.
- `collect_evidence(EvidenceRequest)` in `crates/domovoi-engine/src/evidence/` is the only caller of the raw read operation. Its scope is built from the incident or host the caller was given. It checks the host's availability first and answers `NotConnected` or `Incompatible` before any request is built, then applies the enablement of the contract, and returns the agent's output unchanged. Redaction and truncation stay with the callers.
- Host metadata is read through `AgentAdmin`, and verification re-runs a declared check with `run_check`; both sit outside the read boundary.
- `AgentServer::link()` is private to the hub's `agent_server` module and reachable only inside the hub crate; the runtime composition root is its one caller outside that module. The raw link reaches a test only through the hub's non-default `test-support` feature, which the hub's and the binary's dev-dependencies enable. A release build has no accessor.
- Four rules, each default-deny, are checked by one syntax-aware checker ([0023](0023-capability-objects-one-checker-bounded-evidence.md)): `EXEC-1` (the raw read only in the evidence boundary), `EXEC-2` (`run_skill`, `start_skill`, `skill_result`, `run_capability` and the relay operations only at their cards' exact sites and counts), `EXEC-3` (`run_check` only in verification) and `EXEC-4` (a transport may not name `AgentLink`, `WsAgentLink` or take the link from the server). Every exception names one file exactly, so a new file beside an allowed one inherits nothing.
- The operation path's cards: `hand_off` and `await_handoff_result` once each in `operations/execute.rs`; `OperationCommand::Dispatch` built once in `operations/execute.rs` and matched once in `operations/writer/mod.rs`; `AuthorizedStep::of` and `execute::execute_step` once each in `operations/effects/dispatch.rs`.
- Gate tests assert the same shape from inside the engine and the hub by calling the checker `cargo xtask arch-check` runs: `single_execution_boundary`, `the_boundary_gate_counts_each_operation_exactly_once`, `dispatch_construction_and_match_are_separate_sites` and `transports_do_not_name_the_agent_link`.

## Consequences

- There is one path from a human decision to a running skill and one from a caller to a host's tool output. Adding a second fails CI instead of passing review.
- The enforcement is syntax-aware, not a compiler seal. Its limits are stated in [0023](0023-capability-objects-one-checker-bounded-evidence.md). The value is that the ordinary way to add a second path, or for a transport to take the link, fails `arch-check` or a gate test.
- `run_check` stays outside the read boundary: verification re-runs a declared check after a run, which is not evidence and not read-only.
- `test-support` is a test configuration: it adds one accessor to builds that enable it, and no behaviour to a shipped binary.

## Alternatives considered

- A newtype around the link with `start_skill` and `run_tool` only on the newtype: the compiler would enforce it, but a transport could still name the type, and the hub needs a route to the raw link for reconciliation. Capability objects ([0023](0023-capability-objects-one-checker-bounded-evidence.md)) cover the engine side and the scan covers naming.
- Keeping `AgentServer::link()` public and relying on review: a transport could hold a link and add a second path with no failing check.
- A `#[doc(hidden)]` accessor in every build: callable by any consumer, so the boundary would rest on convention.
- Moving verification's `run_check` into `collect_evidence`: it would make a non-read-only call look like evidence.
