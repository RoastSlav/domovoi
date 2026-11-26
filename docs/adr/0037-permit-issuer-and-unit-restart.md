# 0037. Permit issuer, plan family and unit restart

- Status: Accepted
- Date: 2026-11-25

## Context

Policy is recorded in shadow ([0027](0027-policy-shadow-recording.md)) and enforced for exactly one contract, `unit.restart`, with bounds on every side: the evaluator's hard limits, hub guards (pause, circuit breaker, rates, unresolved unknown outcomes), the agent's `[unattended]` ceiling and the helper's own allowlist, protected set and restart budget.

## Decision

1. **One issuer.** `HubPermitIssuer` in `crates/domovoi-hub/src/policy/` is the only code that calls `PolicyPermit::issue` and `Authorization::policy`. Guards run in a fixed order and each returns early, so a permit is built only on the `Allow` branch after every guard passed. The engine's permit effect calls the `PermitIssuer` port and records the authorization it is handed; it never builds one. `PolicyPermit::restore` rebuilds a stored permit and may be called only in the store and test support.

2. **Closed policy-dispatchable list.** `POLICY_DISPATCHABLE` holds `unit.restart` and nothing else. Skills stay human-only: `RunSkill` carries no authorization kind, so the agent cannot check a ceiling for one. `container.restart` exists so rules can name it; no path dispatches it, and the registry answers `DockerWriteUnsupported` for either authorization kind.

3. **A new plan family.** `plan:capability-v1` binds the contract and version, the host and its enrolled key, the target identity, the identity hash, the canonical arguments and the execution id. Plan v2 ([0021](0021-plan-hash-and-authorization.md)) needs a skill manifest and a check key, and the identity hash must be bound. The literal is written only in `crates/domovoi-core/src/action/capability_plan.rs`.

4. **A closed unit call and a capability object.** `UnitCall` is `Identity { name }` or `Restart { id, identity_hash }`; it carries no free argument. The hub reaches the agent through `UnitRelay` (capability `UnitCaps`), over the existing `run_capability` frame. It adds no wire message.

5. **The identity algorithm.** A pure, ordered algorithm in `domovoi_core::unit` binds the loaded properties plus the bytes of every file of the unit, and uses a disk enumeration of the whole search path as the stale check. `NeedDaemonReload` and `systemctl cat` are never the stale check. The helper never runs `daemon-reload`.

6. **Post-restart check.** The helper checks the new process (`/proc/<pid>/environ`, `cmdline`, owner) and never re-reads the loaded properties after a restart. Checking the process rather than the loaded properties avoids false alarms the properties produced. A wrong start is an alert and a `Failed` step; the helper takes no corrective action.

7. **Template instances are refused.** The drop-in directory of a template cannot be enumerated without parsing the instance name, which rule `ID-3` forbids in the helper. Instances are refused as `target_unsupported` with reason `instance` rather than half-checked. 

8. **Compare reports `identity`.** The helper receives only the approved hash, so a mismatch at dispatch is `target_changed` with the field `identity`, or `id` when the manager's id for the name differs. The first differing field is known only where the bound identity is held, which is not the helper.

9. **Rules added.** `policy` joins `UNZONED_MODULES` (the permit issuer's directory, not a transport or a service). `PolicyPermit::restore` and the `plan:capability-v1` literal are confined. `start_unit_call` and `unit_call_result` are counted raw operations with one site each in `runtime/caps.rs` (two calls each: the read and the restart). `UnitRelay::dispatch_restart` and `restart_result` are called only in `operations/execute_unit.rs`, `read_identity` only by the target resolver, and `execute_unit::execute_unit_restart` once from `effects/dispatch.rs`.

## What the checker cannot see

The agent and helper re-check what they can, but a compromised hub can label a request `human` and skip the agent's ceiling. The helper's allowlist, protected set, identity check and restart budget still hold. This is an accepted limitation. A residual race remains between the helper's last read and the manager acting. The approval card says so, and the post-restart check detects a wrong start.

## Consequences

- Nothing runs unattended until a host has an `[unattended]` ceiling, a helper with `systemd.write` on and the owner's opt-in on its platform tier, **and** the owner publishes a non-shadow `Allow` rule for `unit.restart`.
- Otherwise every `unit.restart` step is an approval offer a person can approve.
- A restart of an instance of a template is not supported.
