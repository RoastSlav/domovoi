# 0007. Only humans approve actions

- Status: Accepted
- Date: 2026-10-31

## Context

The triage model reads untrusted text. A prompt-injected model must not be able to authorise anything. The safe pattern is that the model proposes and approval lives in a channel the model cannot reach.

## Decision

- Approval requires a `HumanApproval` value. It is constructed only in `crates/domovoi-hub/src/auth/`, after a verified web session with CSRF or after the chat interaction authorizer has matched the platform user against the allowlist. `xtask arch-check` restricts the call sites (`INV-1`). Loading a stored approval is the one other way to obtain one, and [0014](0014-rehydrating-stored-approvals.md) restricts it to the store.
- The model's tool catalogue is the closed `MetaTool` enum. Reads go through `request_evidence`, whose argument is the closed `ReadTool` enum. Neither has an approve, authorize, write or execute variant.
- An approval binds to a plan hash and is re-validated at dispatch ([0021](0021-plan-hash-and-authorization.md)).
- A destructive action needs a typed confirmation in the web UI and is never approvable from a chat button.
- A policy may authorize only the contracts listed in `POLICY_DISPATCHABLE`, through the one permit issuer ([0037](0037-permit-issuer-and-unit-restart.md)). A permit is not an approval.

## Consequences

- Gate tests (`tests/gates.rs`) assert the tool catalogue, the call-site restriction, plan-hash re-validation and destructive gating. They are never deleted, only extended.
- Any new surface (MCP, an operator, a policy) must state in its ADR how it preserves this invariant.

## Alternatives considered

- Letting the model approve low-risk actions: risk classification itself becomes an injection target.
