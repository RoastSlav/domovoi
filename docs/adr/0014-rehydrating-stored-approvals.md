# 0014. Only the store may rebuild a stored approval

- Status: Accepted
- Date: 2026-10-31

## Context

[0007](0007-human-only-approvals.md) makes `HumanApproval` the only proof that a human approved something. It has no `Deserialize`, and `HumanApproval::new` may be called only in `crates/domovoi-hub/src/auth/`, after authentication.

The store must persist approved executions and install targets and load them back unchanged: the hub dispatches an approved step after a restart, and the audit trail shows who approved what. Deriving `Deserialize` would let any code turn a JSON object into an approval, which is exactly what 0007 rules out.

## Decision

- `HumanApproval::restore(user, channel, at)` rebuilds an approval that `HumanApproval::new` created earlier and the store persisted. It records no new decision.
- `xtask arch-check` restricts `HumanApproval::restore(` to `crates/domovoi-store/src/` and the testkit, the same way it restricts `HumanApproval::new(` (`INV-1`).
- `HumanApproval` and `Principal` have no `Deserialize`; a gate test in `domovoi-core/tests/gates.rs` asserts it. `Principal::System` holds a `Cow<'static, str>` so a stored component name can be read back.

## Consequences

- Two audited call sites can produce a `HumanApproval`: authentication creates one and the store reloads one. A model, a chat message or an HTTP body cannot.
- Approvals are re-validated at dispatch (plan hash, manifest hash, arguments), so a tampered database row cannot widen what runs: the agent recomputes the hash from its own manifest.

## Alternatives considered

- Deriving `Deserialize` on `HumanApproval`: any code could forge an approval from JSON.
- A separate "stored approval" type accepted by loaders: anything could construct it, so it would need the same call-site restriction while weakening the type 0007 relies on.
