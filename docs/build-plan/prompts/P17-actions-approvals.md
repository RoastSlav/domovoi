# P17 — Actions and approvals

| | |
| --- | --- |
| Phase | 4, Intelligence |
| Depends on | P16 |
| Crates | `domovoi-engine` |
| Size | L |
| Invariants touched | INV-1, INV-4, INV-6, INV-7 |

## Goal

Carry proposed actions to completion safely: human approval (and only human approval), expiry, re-validation at dispatch, a loop guard, dispatch to the agent, verification by re-running the originating check, and a complete audit trail. Every step emits events so cards and the UI stay current.

## Read first

- `ARCHITECTURE.md` §5.6, §12, §15
- ADR 0007
- P02 `ActionRequest` transitions, P03 `AgentLink` and `AuditLog`, P16 proposals

## In scope

### Approval service (`actions/approvals.rs`)

- `ApprovalService::approve(action_id, approval: HumanApproval, chat_token: Option<&str>) -> Result<ActionRequest, ApprovalError>`:
  - Load the action; must be `Proposed` and not expired.
  - If `approval.channel` is `Chat`, a token is required and its SHA-256 must equal the stored `chat_token_hash`; the hash is cleared on use (single use).
  - Call `ActionRequest::approve` (which enforces typed confirmation for destructive actions, INV-6).
  - Persist, audit (`action.approved`, actor = approval user and channel), emit `ActionApproved`.
- `deny(action_id, approval)` similarly. `expire_due(now)` called by the scheduler: expires `Proposed` actions past `expires_at` and audits them.
- There is no other way to reach `Approved`. The service API takes `HumanApproval` by value; nothing in engine constructs one (arch-check enforces).

### Executor (`actions/executor.rs`)

- Subscribes to `ActionApproved`. For each:
  1. **Loop guard:** at most 3 dispatches per (host, skill) per rolling hour (configurable). Over → status `Rejected { reason: "loop guard" }`, audit, emit `ActionBlocked`; this always reaches a human (card update + thread reply).
  2. **Re-validate (INV-4):** fetch the host's current `SkillSummary` from `AgentLink::skills`; skill must still exist with the same risk; validate the stored args; recompute `PlanHash`; call `dispatch(plan_hash)`. Mismatch → `Rejected`, audit, event.
  3. Host not connected → wait up to 5 min for presence, else `Failed { reason: "agent offline" }`.
  4. `AgentLink::run_skill` with deadline = manifest timeout + 30 s. Map the result: success exit code 0 → `Succeeded`; else `Failed`. Store a redacted output excerpt (≤ 4 KiB) and the `argv_hash`.
  5. **Verify:** if the manifest's `verify.recheck` is set (or by default for `Write` skills tied to a check), wait `verify.delay` using the `Clock`, then `AgentLink::run_check(check, target)`; target OK → `Verified`, else `VerifyFailed` (reaches a human).
  6. Audit each transition with plan hash, argv hash, exit code, output SHA-256, duration and the verification result.
- Send an `ActionChanged { action, incident }` event on every transition. Successful verification also sets the incident verdict to `Fixed` through the incident manager (`ManagerCommand::SetVerdict`).

### Audit (`actions/audit.rs`)

- Helpers that build `AuditEvent`s for every transition with consistent `kind` names: `action.proposed`, `action.approved`, `action.denied`, `action.expired`, `action.rejected`, `action.dispatched`, `action.succeeded`, `action.failed`, `action.verified`, `action.verify_failed`, `action.blocked`.

## Out of scope

- Where `HumanApproval` comes from (hub auth, P18 and P21). Standing policies and autonomy (v0.5). Revert and commit-confirm (v0.3).

## Tests required

Fakes from testkit; `HumanApproval` values from `domovoi_testkit::approvals` (allowed in tests):

1. `approved_write_action_dispatches_succeeds_and_verifies`.
2. `failed_verification_reaches_a_human`.
3. `chat_approval_requires_matching_single_use_token`.
4. `expired_action_cannot_be_approved`; `expire_due_expires_and_audits`.
5. `manifest_change_between_approval_and_dispatch_rejects` (INV-4).
6. `args_no_longer_valid_reject`.
7. `loop_guard_blocks_fourth_dispatch_in_an_hour`.
8. `offline_agent_waits_then_fails`.
9. `audit_trail_is_complete_and_ordered` for the happy path and each failure path.
10. `output_excerpt_is_redacted_and_capped`.
11. Gates in `crates/domovoi-engine/tests/gates.rs`:
    - `destructive_action_cannot_be_approved_from_chat` (INV-6).
    - `destructive_action_needs_typed_confirmation_on_web` (INV-6).
    - `only_approve_reaches_approved_state` (exhaustively drive every other service method and event; status never becomes `Approved`) (INV-1).
    - `rejected_or_failed_actions_never_retry_silently`.

## Architecture gates

- `HumanApproval::new` appears nowhere in engine sources (arch-check).
- Engine remains I/O-free.

## Acceptance criteria

- `cargo xtask ci` passes.
- `docs/book/src/concepts/actions.md` explains the action lifecycle with the state diagram and the safety rules in plain language.

## Deliverables

- `crates/domovoi-engine/src/actions/*`
- Tests and gates; `docs/book/src/concepts/actions.md`

## PR

Title: `feat(engine): human-only approvals, re-validated dispatch, verification and audit`
