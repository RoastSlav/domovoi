# 0014. Rehydrating stored approvals

- Status: Accepted
- Date: 2026-10-31

## Context

ADR 0007 makes `HumanApproval` the only proof that a human approved an action. It has no `Deserialize`, and `HumanApproval::new` may be called only in `crates/domovoi-hub/src/auth/`, after authentication. `ActionRequest` holds the approval and changes status only through its transition methods.

The store must persist approved actions and load them back unchanged: the hub dispatches an approved action after a restart, and the audit trail shows who approved what. With no way to rebuild a `HumanApproval` or an `ActionRequest` from stored fields, the action repository cannot be implemented. Deriving `Deserialize` would let any code turn a JSON object into an approval, which is exactly what ADR 0007 rules out.

## Decision

- `HumanApproval::restore(user, channel, at)` rebuilds an approval that `HumanApproval::new` created earlier and the store persisted. It records no new decision.
- `ActionRequest::from_stored(StoredAction)` loads a request with every stored field. The approval can only be supplied as a `HumanApproval`, so loading cannot approve anything that was not approved when it was stored.
- `xtask arch-check` restricts `HumanApproval::restore(` to `crates/domovoi-store/src/` (and testkit), the same way it restricts `HumanApproval::new(` (INV-1). This adds `HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES` to `xtask/src/arch/rules.rs`.
- `HumanApproval`, `ActionRequest` and `Principal` still have no `Deserialize`; a gate test in `domovoi-core/tests/gates.rs` asserts it.
- `Principal::System` holds a `Cow<'static, str>` instead of `&'static str`, so a stored component name can be read back.

## Consequences

- Two audited call sites can produce a `HumanApproval`: authentication creates one, the store reloads one. A model, a chat message or an HTTP body still cannot.
- Approvals are re-validated at dispatch as before (plan hash, manifest hash, arguments), so a tampered database row cannot widen what runs: the agent recomputes the plan hash from its own manifest (INV-3, INV-4).
- `Principal` is no longer `Copy`; `ActionRequest::proposed_by` returns a reference.

## Alternatives considered

- Deriving `Deserialize` on `HumanApproval` and storing actions as JSON: any code could forge an approval from JSON.
- A separate "stored approval" type accepted by `from_stored`: anything could construct it, so it would need the same call-site restriction while weakening the type that ADR 0007 relies on.
- Leaving the action repository unimplemented: blocks dispatch after a restart.
