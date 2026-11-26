# 0027. Record policy shadow decisions beside execution, never in the writer

- Status: Accepted
- Date: 2026-11-19

## Context

Policy rules decide, for an Execution step, whether it may run unattended, needs a human or is denied. Before a rule can enforce anything, the owner needs to see what the rules would have decided for real steps: which would change the outcome, and which inputs were missing when the decision was made. A shadow decision is a record, not a permit; it authorizes nothing.

Three forces shape the design:

- The operation writer is the single writer of operation state ([0025](0025-operation-aggregate.md)). Putting a rule engine and its inputs into its commit would put them on the critical path of every approval and every step.
- Some inputs are not always available: the agent's unattended ceiling, safety facts, whether the agent runs as root, the pinned targets and the containment state. A decision made without one must say so, not pretend the input was safe.
- Rules are data that outlive the binary that wrote them. A rule must read back the same way after a later release, and a published version must never change.

## Decision

- **A sweep task writes shadow decisions, not the operation writer.** A recorder runs on a timer (every 5 seconds, at most 10 batches of 50 steps per tick). It reads operation state and the policy repository and commits only `PolicyWrite::RecordShadow`. The operation writer does not wait for it and no operation code path imports `policy_shadow`. A failed or slow sweep leaves a backlog that drains on a later tick; it never blocks or fails an operation.
- **Idempotent, crash-safe inserts.** Each decision row is keyed by the step's execution id, so inserting the same step twice is a no-op. The sweep state row records the last successful batch and the last error.
- **One row per Execution step, holding both decisions.** The row stores the enforced-rules decision and the shadow-rules decision side by side, with the outcome word of each (`allow`, `require_approval`, `unavailable`, `deny`, `error`), so "would these differ" needs no join.
- **No published version means version 0.** A step planned before any version was published is evaluated against the empty rule set and records `policy_version = 0` and the hash of the empty set.
- **Coverage is complete.** Contracts under `agent.package.*` are never evaluated: their row has status `skipped`, a reason and no decision, so the coverage count stays whole.
- **Draft and publication are gated by the person.** An edit to a draft that touches a locked rule needs the admin password. Publishing needs the password and an echo of the `preview_hash` the preview returned, so the published version is the one that was shown.
- **Rules are stored as JSON envelopes** `{"v":1,"rule":...}`. Golden tests pin the envelope bytes and the decision document, so a change to the serialised form fails a test and cannot silently re-hash stored versions. A later format is a new `v`, read beside `v:1`.
- **The publication index** is the compiled contracts plus the skills the agents announced. A skill name announced on several hosts is represented once, with the strictest effective class.
- **Missing inputs fail closed and are recorded as gaps.** The recorder substitutes the value that denies or narrows, never one that allows, and lists each substitution in the row's `gaps` with a reason (`not_produced_yet`, `host_not_connected` or `unresolvable`). A row with at least one gap has `degraded` set.
- **`[policy]` is one table name in two files.** `hub.toml` reads `shadow` and `agent.toml` reads `allow_risk`. Environment overrides match key by key, so an override for one role does not apply to the other.
- **No retention.** Shadow rows are kept indefinitely.

## Consequences

- Operations gain no failure mode. A bug in the recorder can leave rows missing or late; it cannot change an approval, a dispatch or an operation status. The engine's gate tests assert that no operation module imports `policy_shadow`.
- Shadow rows are a measurement. Nothing reads them to allow or deny a step, and `degraded` and the gap list show how far a measurement can be trusted.
- Rows grow with every Execution step and are not pruned.
- Golden snapshots for the rule envelope and the decision document are reviewed on every change (R14).
- Enforcement is a separate path: the one permit issuer of [0037](0037-permit-issuer-and-unit-restart.md).

## Alternatives considered

- Evaluating inside the operation writer's commit: a consistent row for free, but a slow or failing input could block an operation.
- Evaluating on read: no stored rows, but the answer would depend on the rules and hosts of the moment, and nothing would be recorded for steps nobody looked at.
- Substituting safe values for missing inputs: makes most rows look clean and hides the gaps the shadow mode exists to show.
- Storing rules as plain JSON without an envelope: a later format change would have to guess which shape a stored rule has.
