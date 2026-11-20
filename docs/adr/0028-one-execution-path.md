# 0028 One execution path

- Status: accepted
- Date: 2026-11-20
- Supersedes in part [0019](0019-single-writer-for-actions.md) and [0022](0022-single-execution-boundary.md): the action writer and the action executor they describe are gone; the operation writer ([0025](0025-operation-aggregate.md)) and the operation dispatch site replace them.

## Context

Two execution paths existed side by side: the first-generation action executor (`actions/`) and the operation service's own dispatch of Execution steps. The architecture rules (EXEC-2) named both sites and said they would stay "until a later change removes them together". Nothing creates a legacy action any more, the triage queue is gone, and approval of a step works from the web and from chat. What remained was a second place that decides to hand a skill to an agent, a second writer, a second verification policy and the rules that pinned them.

## Decision

- **One dispatch site.** A skill reaches an agent only through `operations/execute.rs` (`AuthorizedStep`, `execute_step`). EXEC-2 allows exactly this: `hand_off` and `await_handoff_result` once each in `operations/execute.rs`; `OperationCommand::Dispatch` built once in `operations/execute.rs` and matched once in `operations/writer/mod.rs`; `AuthorizedStep::of` and `execute::execute_step` once each in `operations/effects/dispatch.rs`. `AuthorizedExecution::of`, `execute::execute` and `WriteCommand::Dispatch` are retired names and are violations anywhere. The gate tests and the xtask tests assert that.
- **`actions/` is deleted** (writer, executor, dispatch, recovery, expiry, reconcile, verification, `ActionService`, `ActionHandle`, `ApprovalError`). The reconcile sentences and reason constants moved to `operations/reconcile_text.rs`. The legacy verification policy (a skill's `verify` spec, otherwise the default delay for a write skill that applies to the check, and a 60 second re-check deadline) is carried by the operation path and tested there.
- **Legacy rows are retired once, at the first start.** `drain_legacy_actions` runs before any service starts: `Proposed` becomes `Expired`, `Approved` becomes `NotRun` (both "legacy action expired at upgrade"), and a still-`Dispatched` run becomes `OutcomeUnknown` ("legacy executor removed before this run was reconciled"). A pending verification wake is cancelled and audited as `action.verification_dropped`; the action stays `Succeeded`. Each move is one transaction with its audit and journal rows, under the status guard, by `system:legacy`. The drain never refuses to start: a refusal would let a host that never comes back block an upgrade for good, and `OutcomeUnknown` is the state the legacy path already gave a run it could not reconcile. The agent's journal keeps the real record under the same id.
- **Legacy rows stay readable.** `GET /api/v1/actions` and `GET /api/v1/actions/{id}` remain, read-only, every row labelled `legacy`. The approve and deny routes and the legacy chat button branch are removed; an old legacy button answers that the card is out of date. The table, its repository and the one-time token machinery that steps use are kept.

## Consequences

- One place decides to dispatch, with a value that exists for one run, and the cards say so.
- The `domovoi_action_*` metrics are no longer emitted for new runs; the operation path records its own state in the journal and audit log.
- A downgrade cannot reopen retired rows; the pre-migration backup is the way back.
- No schema change and no wire change.
