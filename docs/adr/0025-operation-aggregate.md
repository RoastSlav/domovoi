# 0025 The operation aggregate: steps, edges and one writer

- Status: accepted
- Date: 2026-11-19
- Extends [0019](0019-single-writer-for-actions.md) and [0022](0022-single-execution-boundary.md)
  for the operation service. Both stand for actions; the second execution site below is the
  only exception they gain.

## Context

Domovoi's unit of work so far is an action: one proposed skill run with one approval. An
investigation needs more than that. It gathers evidence, forms an assessment, proposes a plan,
waits for a person, runs one or more skills, verifies the result and may start again. Keeping
that as a chain of loosely linked actions loses the reasons between the steps and makes pauses,
budgets and restarts hard to state.

The operation aggregate models one piece of work as a durable record. It is off by default
(`[orchestration] enabled = false`), and with the flag off nothing about actions changes.

## Decision

- **One aggregate per operation.** An `Operation` holds the goal, kind, cycle, budgets, pinned
  subjects, incident links and a coarse status. Its state is written by one writer task. Every
  change is one `UnitOfWork` commit that writes the operation row, its steps, edges, executions,
  journal rows, audit rows and effects together, or none of them. A stale version is reloaded
  and decided once more; a second conflict fails the command.
- **Steps are immutable.** A step's identity, kind, phase, parent, cycle and intent are fixed at
  creation; the store refuses updates to those columns with a trigger. Only the progress columns
  (status, timestamps, summary, reason, output) change, and only from the status they were read
  with. The step is the unit of execution state.
- **Projection, not a second status.** The operation's status is derived from its open steps and
  its pause state by a pure function. It is stored only as a cache of that derivation. There is
  no execution status outside the step's outcome: an `executions` row joins one Execution step
  one-to-one and holds the plan, the approval and the dispatch facts, and its state is read from
  the row and the step together.
- **The ordinal rule keeps the graph acyclic.** Each step has a dense ordinal inside its
  operation, assigned at creation. An edge must point from a later step to an earlier one of the
  same operation, enforced in the writer and by a trigger on `step_edges`. A cycle cannot be
  written, so the trace can be ordered and read in one pass.
- **Execution id is the step id.** An Execution step's id is the `execution_id` sent to the
  agent, so a result or a reconcile names the step directly. An execution id may never equal an
  action id; the store refuses it.
- **A second dispatch site, temporarily.** While actions still run on the legacy executor, the
  operation service has its own dispatch path. It is the second place that calls
  `HandoffRunner::hand_off` and `await_handoff_result`, and the second place that builds
  `OperationCommand::Dispatch`. `xtask arch-check` allows exactly these, under rule `EXEC-2`:
  - `hand_off` and `await_handoff_result`: once in `actions/execute.rs` and once in
    `operations/execute.rs`.
  - `OperationCommand::Dispatch`: constructed once, in `operations/execute.rs`.
  - `AuthorizedStep::of`: once, in `operations/effects/dispatch.rs`, the operation service's one
    authorizing constructor.
  - `execute::execute_step`: called once, from `operations/effects/dispatch.rs`.

  The legacy site goes when the legacy executor does. Removing `actions/execute.rs` removes its
  cards in the same change, so the operation path becomes the one execution boundary again.
- **Claims are scoped to the aggregate.** An effect claim names the operation it belongs to, and
  a claim by kind alone is not allowed. A kind-only claim could let one operation's worker take
  or revert an effect that belongs to another operation of the same kind, and that is unsafe
  once two operations run at once. `revert_claimed_at_start` and cancellation are scoped the same
  way, and an empty scope claims nothing.
- **`EffectCancel`** is the effect that cancels an operation's pending effects when the operation
  is cancelled. It touches only effects of that aggregate. A dispatched execution is not recalled:
  its late result is recorded against its step.
- **Only `skill.<name>` contracts execute.** An operation step may run only a skill contract,
  resolved through the capability registry and pinned to a host. Any other contract proposed by
  a model is stored as a refused step at proposal time.
- **No policy evaluation yet, so every execution needs a human.** The `policy_decision` step kind
  exists in the trace schema, but nothing writes an authorization of kind `policy`. Every
  Execution step waits for an approval by an authenticated human, under the same rule as
  [0007](0007-human-only-approvals.md). INV-1 is unchanged for steps.
- **Some effects are created and never claimed yet.** The operation service has no handler for
  `build_context`, `call_model`, `draft_summary`, `notify`, `evaluate_policy` or `issue_permit`.
  A failed verification creates a `build_context` effect, and it stays pending until the
  reasoning driver that handles it exists. This is a deliberate boundary, not an omission to be
  worked around.
- **A queued operation loses no work.** A seed may carry an evidence request or a question
  before the operation has a slot. Its effects are created at once; while the operation is
  queued their handlers put them back for another look (the same way they do for a paused
  operation) instead of completing them, so the read runs and the timeout fires only after
  admission.
- **A model turn is charged when it is accepted.** A batch of turns is checked against the
  running count, so it cannot pass a limit together; the tokens and cost a turn reports are
  charged when it finishes.

## Consequences

- Investigations keep their reasons: the trace shows who asked for what, what came back, what
  was approved and what was verified, in order.
- Pauses, budgets and restarts have one place to be read from: the operation row, the steps and
  the executions, with no second execution status to disagree with them.
- Adding a step kind, an outcome or an edge kind is additive to the trace schema. Renaming or
  changing the meaning of one bumps the trace schema version.
- The store and the engine must enforce the step, edge and execution rules themselves, because a
  caller that writes raw SQL is refused only by the triggers. The contract suites run on the
  in-memory store and on SQLite.
- Until the legacy executor is removed, there are two dispatch sites. The `EXEC-2` cards name
  both, so a third site or a new caller fails `arch-check`.
- No policy layer means no skill runs without a person. That is slower by design and stays so
  until a policy decision is recorded as its own authorization.

## Alternatives considered

- **Keep chaining actions.** Loses the reasoning between steps and cannot express a plan with
  several steps that wait on each other.
- **Mutable steps with a status history table.** Two places would hold the state, and a status
  and its history could disagree; an immutable step with a derived projection has one source.
- **A separate execution status column on steps.** Duplicates the step outcome; rejected for the
  same reason as the second status.
- **Kind-only effect claims.** Simpler queries, but one operation could act on another's effects;
  rejected as unsafe with concurrent operations.
- **Allow `policy` authorizations now.** No policy engine exists to produce them; accepting the
  kind now would let a stored row claim a decision nothing made.
- **Share the legacy dispatch site.** Would make the legacy executor a dependency of the operation
  service, which is the coupling the single boundary was meant to avoid; the temporary second site
  is named and dies with the legacy path.
