# 0026 The hub's use-case layer and its module-edge rules

- Status: accepted
- Date: 2026-11-19
- Extends [0007](0007-human-only-approvals.md) and [0021](0021-plan-hash-v2-and-authorization.md): the
  services below never mint an approval, they ask `auth/approval.rs` for one.

## Context

The hub grew two ways in for a person: HTTP handlers and Discord buttons. The chat code reached
into the HTTP module for its read models (`host_dto`, `summary_of_host`, `system::build`,
`latest_triage`) and held the whole `ApiState`, so a change to an HTTP type could break chat and
a command written for one surface was copied for the other. A third surface, the Operator, is
coming, and it must not copy a third time.

## Decision

- **A use-case layer.** `crate::services` holds plain Rust services: `IncidentService`
  (acknowledge, mute, unmute, ask, triage again), `ApprovalService` (approve and deny an action),
  `OperationCommands` (approve and deny a step; cancel, pause, resume and answer an operation),
  `HostService` and `BriefingService` (what a host and the digest look like). They are built once
  by the runtime into `Services`.
- **Three lines for a transport.** A transport authenticates and parses (turning a cookie or a
  button press into a typed request and a `Caller`), calls one service method, and renders the
  answer in its own format. Everything between ("is this allowed, what exactly is written, which
  audit row") belongs to the service. A service returns domain or engine types and its own small
  error enums, never a DTO or an HTTP error, and never spawns a task.
- **Commands go through a service; reads may stay.** A plain repository read may stay in the
  transport that renders it; a read shared by two transports lives in a service. HTTP-only
  handlers stay where they are until a second transport needs them: incident list, detail and
  dismiss, rules, notes, settings, enrollment, revocation, skills and logs of a host, profile
  get and refresh, follow-up history, the Operator chat, overview, tool usage, updates,
  authentication, readiness, and every read endpoint of operations, events and snapshots.
- **`Caller` is the proof of authentication.** It names the person and the surface (a web session
  or an allowlisted chat user). Only a transport builds one, only after its own authentication,
  and only two call sites in the hub may: the session extractor and the chat authorizer
  (`CALLER-1`, two boundary cards). `authorization_for` and `approval_for` in `auth/approval.rs`
  are the only place a `Caller` becomes a `HumanApproval`, so `HumanApproval::new` still lives
  only under `crates/domovoi-hub/src/auth/`.
- **Module-edge rules in `cargo xtask arch-check`**, from the zones in `rules.rs`:
  - `MOD-1`: a transport (`api`, `auth::interactions`, `ingest`) does not import another
    transport.
  - `MOD-2`: `services` imports no transport and not `runtime` or `router`.
  - `MOD-3`: a new top-level hub module is a zone or is listed as unzoned, so the rules cannot be
    escaped by adding a directory.
  - `MOD-4`: a transport names neither `ActionHandle` nor `OperationHandle`.
  Paths are resolved after `use` renames and `super::` chains, in expressions, types and macro
  bodies; a file that does not parse fails. Test-only code is skipped.
- **Unconstrained, and why.** `runtime`, `router`, `spa` and `middleware` compose or serve;
  `auth` (outside `interactions`), `agent_server`, `profile`, `digest`, `settings`, `admin`,
  `selftest`, `capability`, `csp` and `version` are support code a service may use. `capability`
  is the hub-owned registry, a support module like `profile`, so it is unzoned.
- **Shared types left the transport.** `AgentAdmin` moved to `agent_server`, the version rule to
  `crate::version`, and the digest and host-status builders to services, so a service never
  needs a transport's helper.
- **No step-up for operation controls.** Cancelling, pausing, resuming and answering an operation
  need an owner session and the CSRF header, like approving an offered step. Step-up stays for
  policy publication, locked rules, capability enable, installation and LLM CLI. `autonomy.resume`
  is the future global pause switch and is unrelated to resuming one operation.
- **A pause a person asks for has its own reason**, `PauseReason::Manual`, which resumes without
  a budget condition, and an answer to a question is audited as `operation.answered` without the
  text.
- **Later work plugs in here.** Chat approval of investigation steps already runs through
  `OperationCommands`; the Operator, knowledge edits, policy publication and package installs add
  services in `crate::services` and routes in `api`.

## Consequences

- A command has one implementation per behaviour. Adding a surface means parsing its input and
  calling the same methods.
- A transport cannot reach another transport's helpers or a command handle, and a reviewer reads
  the rule instead of remembering the convention.
- Changing the zones, the forbidden names or the `Caller` cards takes a new ADR, like every other
  edit of `rules.rs`.
- The hub's public `ApiDeps`, `ChatDeps` and `InteractionAuthorizer::new` change; their only
  consumers are the binary, the dev hub example and the tests.
