# 0026. The hub's use-case layer and its module-edge rules

- Status: Accepted
- Date: 2026-11-19

## Context

A person reaches the hub through two surfaces: HTTP handlers and chat buttons. If one surface imports another's read models or holds the whole application state, a change to an HTTP type breaks chat, and a command written for one surface is copied for the next. Each command must have one implementation, and the services that run it must never mint an approval themselves ([0007](0007-human-only-approvals.md), [0021](0021-plan-hash-and-authorization.md)).

## Decision

- **A use-case layer.** `crate::services` holds plain Rust services such as `IncidentService`
  (acknowledge, mute, unmute, ask), `OperationCommands` (approve and deny a step; cancel, pause,
  resume and answer an operation), `HelperTrust`, `PolicyService`, `HostService` and
  `BriefingService` (what a host and the digest look like). They are built once by the runtime
  into `Services`.
- **Three lines for a transport.** A transport authenticates and parses (turning a cookie or a
  button press into a typed request and a `Caller`), calls one service method, and renders the
  answer in its own format. Everything between ("is this allowed, what exactly is written, which
  audit row") belongs to the service. A service returns domain or engine types and its own small
  error enums, never a DTO or an HTTP error, and never spawns a task.
- **Commands go through a service; reads may stay.** A plain repository read may stay in the
  transport that renders it; a read shared by two transports lives in a service. HTTP-only
  handlers stay in `api` until a second transport needs them.
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
  `selftest`, `capability`, `helper_trust`, `packages`, `policy`, `csp` and `version` are support code a
  service may use, so they are unzoned.
- **Shared types live outside the transports.** `AgentAdmin` is in `agent_server`, the version rule
  in `crate::version`, and the digest and host-status builders in services, so a service never
  needs a transport's helper.
- **No step-up for operation controls.** Cancelling, pausing, resuming and answering an operation
  need an owner session and the CSRF header, like approving an offered step. Step-up (the
  account password again) is for policy publication, locked rules, capability enable,
  installation, the LLM CLI and the helper pin.
- **A pause a person asks for has its own reason**, `PauseReason::Manual`, which resumes without
  a budget condition, and an answer to a question is audited as `operation.answered` without the
  text.
- **New commands plug in here.** A new command is a service method that takes a `&Caller`, plus a
  route in `api` or a chat interaction that calls it.

## Consequences

- A command has one implementation per behaviour. Adding a surface means parsing its input and
  calling the same methods.
- A transport cannot reach another transport's helpers or a command handle, and a reviewer reads
  the rule instead of remembering the convention.
- Changing the zones, the forbidden names or the `Caller` cards takes a new ADR, like every other
  edit of `rules.rs`.
