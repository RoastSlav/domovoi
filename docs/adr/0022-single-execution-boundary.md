# 0022. One execution boundary and one read boundary

- Status: Accepted
- Date: 2026-11-17

## Context

The hub runs a skill from exactly one place today: an approved action reaches the executor's run task, which re-checks the incident, waits for the host, re-validates the skill, recomputes the plan hash, commits `Approved → Dispatched` through the one writer, and only then hands the request to the agent. Host reads are almost as concentrated: the model's tools and the profile collector both call `AgentLink::run_tool`, and the triage context calls it in a third place.

Two things were still missing. Nothing in the code or CI said *where* those calls may live, so a new transport, a scheduler or an admin endpoint could add a second path to `start_skill` or `run_tool` and no check would notice. And the raw link was reachable from any module in the hub through `AgentServer::link()`, so a transport could hold one and skip the use case entirely.

`04 §8.1` and `03 RW-08` asked for a single execution boundary and a single read boundary with a gate; the mechanism the repository already uses for rules like this is a line-based source scan in `xtask arch-check`, backed by a gate test.

## Decision

- `execute(AuthorizedExecution)` in `crates/domovoi-engine/src/actions/execute.rs` is the only function that dispatches and the only caller of `AgentLink::start_skill`. `AuthorizedExecution` has one crate-private constructor, `of`, called at exactly one site (the executor's per-action run task, immediately before `execute`); it carries the action and its `ExecutionId`, is taken by value, and is never serialised, stored or logged in full. `execute` refuses an action with no stored human approval, refuses a policy authorization, and refuses an expired window without writing an outcome of its own; expiry inside the run task stays the writer's `NotRun`.
- `execute` keeps two separately named hashes: the approval-family hash the `Dispatch` command carries (v2 for new actions, recomputed from the host's current enrolled key) and the always-v1 `runskill_plan_hash` on the wire, which the agent recomputes.
- `collect_evidence(EvidenceRequest)` in `crates/domovoi-engine/src/evidence/` is the only caller of `AgentLink::run_tool`. Its `EvidenceScope` is built from the incident or host the caller was given, so a caller cannot pass a host beside an incident. It applies the checks that exist at M1 - the closed `ReadToolCall` set, the read-only skill rule, and availability - and returns the agent's `ToolOutput` unchanged; redaction, truncation and the cut notes stay with the callers that own them. It is not the only way to learn about a host: host metadata is read through `AgentAdmin`, and verification re-runs a check with `run_check`.
- `HostCatalog` is the narrow, non-tool view of an agent (session snapshot, announced skills, presence) that triage and profile collection need beside their reads. It replaces `Arc<dyn AgentLink>` in `TriageDeps` and `ProfileDeps`.
- `AgentServer::link()` is crate-private. The composition root builds `Evidence`, `HostCatalog` and `PolicyLink` from it once; the API reads presence, skills and logs through `AgentAdmin`, which delegates to inherent methods on `AgentServer`. A `#[doc(hidden)] link_for_tests` accessor exists because this crate's and the binary's test suites drive an agent's wire directly; the scan refuses a call to it outside `agent_server/` and test paths.
- Four rules, each a constant allowlist and a line-based scan: `EXEC-1` (`run_tool(` only in the evidence boundary), `EXEC-2` (`run_skill(`, `start_skill(` and `run_capability(` only in `execute.rs`), `EXEC-3` (`run_check(` only in verification), `EXEC-4` (a hub transport may not name `AgentLink`, `WsAgentLink` or `PolicyLink`). The trait's declarations, its two hub implementations, the engine's two boundaries and testkit are allowed through `LINK_DEFINITION_PREFIXES`. The scans cover `domovoi-core`, `domovoi-engine`, `domovoi-hub` and `domovoi-store` sources; the agent, runner, protocol crate and binary have their own request handlers and are out of scope.
- Gate tests assert the same shape from inside the engine and the hub (`single_execution_boundary`, `transports_do_not_name_the_agent_link`), including that `WriteCommand::Dispatch {` is constructed only in `execute.rs` outside the writer that defines and matches it.

## Consequences

- There is one path from a human decision to a running skill, and one from a caller to a host's tool output; adding a second one now fails CI instead of passing review.
- `AgentLink` gains no method and the wire does not change: this is a consolidation, not a capability.
- `run_check` stays outside the read boundary: verification re-runs a declared check after a run, it is not evidence and not read-only. A later operation layer may move it with the verification effect.
- The enforcement is textual. A renamed re-export, a macro, a fully qualified call split across lines or a generated call is not caught, and nothing here is a compiler seal. The value is that the ordinary way to add a second path fails `arch-check`.
- A new `AgentLink` implementation must be added to `LINK_DEFINITION_PREFIXES`, which is a deliberate edit a reviewer sees.
- No table, column, endpoint, message or DTO changes; `AuthorizedExecution` never leaves memory and the evidence boundary stores nothing.

## Alternatives considered

- A trait or newtype around the link handed to the engine, with `start_skill`/`run_tool` only on the newtype: the compiler would enforce it, but the engine already receives the link through one port and the hub would still need a route to the raw link for reconciliation and journal queries. The scan was chosen because it also catches a transport that names the type without calling it.
- Keeping `AgentServer::link()` public and relying on review: a transport could hold a link and add a second path with no failing check. Rejected.
- Moving verification's `run_check` into `collect_evidence`: it would make a non-read-only call look like evidence, and the verification effect (a later milestone) is where it belongs. Rejected.
- A database column recording the execution id: `ExecutionId` is derived from the action at M1 and nothing needs to be stored. Rejected.
