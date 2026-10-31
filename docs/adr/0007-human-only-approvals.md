# 0007. Only humans approve actions

- Status: Accepted
- Date: 2026-10-31

## Context

The triage agent reads untrusted text. A prompt-injected model must not be able to authorise anything. Prior art (Homelab Guardian) shows the safe pattern: the model may propose; approval lives in a channel the model cannot reach.

## Decision

- Approval requires a `HumanApproval` value. It is constructed only in `crates/domovoi-hub/src/auth/` after a verified web session with CSRF, or after the chat interaction authorizer has matched the platform user against the allowlist. `xtask arch-check` restricts the call sites.
- The LLM tool catalogue is the closed `ReadTool` enum; it has no approve, write or execute variant.
- Approvals bind to a plan hash and are re-validated at dispatch.
- Destructive actions need a typed confirmation in the web UI and are never approvable from a chat button. Future autonomy (standing policies) may only act on reversible write skills.

## Consequences

- Gate tests (`tests/gates.rs`) assert the tool catalogue contents, the call-site restriction, plan-hash re-validation and destructive gating. They may never be deleted, only extended.
- Any future surface (MCP, operator, policies) must state in its ADR how it preserves this invariant.
- Loading a persisted approval is the one other way to obtain a `HumanApproval`; [ADR 0014](0014-rehydrating-stored-approvals.md) restricts it to the store.

## Alternatives considered

- Letting the model approve low-risk actions: risk classification itself becomes an injection target.
