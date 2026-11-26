# 0025. The operation aggregate: steps, edges and one writer

- Status: Accepted
- Date: 2026-11-19

## Context

An investigation gathers evidence, forms an assessment, proposes a plan, waits for a person, runs one or more steps, verifies the result and may start again. Modelling that as a chain of loosely linked actions loses the reasons between the steps and makes pauses, budgets and restarts hard to state. One piece of work needs one durable record with one place to read its state from.

## Decision

- **One aggregate per operation.** An `Operation` holds the goal, kind, cycle, budgets, pinned subjects, incident links and a status. One writer task owns its state. Every change is one `UnitOfWork` commit that writes the operation row, its steps, edges, executions, journal rows, audit rows and effects together, or none of them. A stale version is reloaded and decided once more; a second conflict fails the command.
- **Steps are immutable.** A step's identity, kind, phase, parent, cycle and intent are fixed at creation, and the store refuses updates to those columns with a trigger. Only the progress columns (outcome, timestamps, summary, reason, output) change, and only from the status they were read with. The step outcome is the only execution state.
- **A projection, not a second status.** The operation's status is derived from its open steps and its pause state by a pure function and stored only as a cache of that derivation. An `executions` row joins one Execution step one-to-one and holds the plan, the approval and the dispatch facts.
- **The ordinal rule keeps the graph acyclic.** Each step has a dense ordinal inside its operation. An edge must point from a later step to an earlier one of the same operation, enforced in the writer and by a trigger on `step_edges`, so the trace can be read in one pass.
- **The execution id is the step id.** A result or a reconcile names the step directly.
- **One dispatch site.** A step reaches an agent only through the operation service: the cards of [0022](0022-single-execution-boundary.md) name every site.
- **Claims are scoped to the aggregate.** An effect claim names the operation it belongs to; a claim by kind alone is not allowed, because it could let one operation's worker take an effect of another. Reverting claims at start and cancellation are scoped the same way, and an empty scope claims nothing. `EffectCancel` cancels the pending effects of one cancelled operation only. A dispatched execution is not recalled: its late result is recorded against its step.
- **Only registered contracts execute.** An Execution step runs a contract resolved through the capability registry ([0024](0024-capability-registry.md)) and pinned to a host: a `skill.<name>` contract, or one of the capabilities of [0037](0037-permit-issuer-and-unit-restart.md) and [0038](0038-containment-boundary.md). A model's proposal for any other contract is stored as a refused step.
- **Every execution needs an authorization.** A step runs after an approval by an authenticated human ([0007](0007-human-only-approvals.md)), or, for a contract in `POLICY_DISPATCHABLE`, after a permit from the hub's permit issuer (0037). Nothing else writes an authorization.
- **A queued operation loses no work.** A seed may carry an evidence request or a question before the operation has a slot. Its effects are created at once; while the operation is queued their handlers put them back for another look instead of completing them, so the read runs and the timeout fires only after admission.
- **A model turn is charged when it is accepted.** A batch of turns is checked against the running count so it cannot pass a limit together, and the tokens and cost a turn reports are charged when it finishes.

## Consequences

- The trace shows who asked for what, what came back, what was approved and what was verified, in order.
- Pauses, budgets and restarts are read from the operation row, the steps and the executions, with no second status to disagree.
- Adding a step kind, an outcome or an edge kind is additive to the trace schema. Renaming or changing the meaning of one bumps the trace schema version.
- The store and the engine enforce the step, edge and execution rules themselves, because a caller that writes raw SQL is refused only by the triggers. The contract suites run on the in-memory store and on SQLite.

## Alternatives considered

- Chaining actions: loses the reasoning between steps and cannot express a plan with steps that wait on each other.
- Mutable steps with a status history table: two places would hold the state and could disagree.
- A separate execution status column on steps: duplicates the step outcome.
- Kind-only effect claims: simpler queries, but one operation could act on another's effects.
