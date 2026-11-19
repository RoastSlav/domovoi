# 0027 Record policy shadow decisions beside execution, never in the writer

- Status: Proposed
- Date: 2026-11-19
- Extends [0021](0021-plan-hash-v2-and-authorization.md) and [0025](0025-operation-aggregate.md):
  nothing here issues an authorization. A shadow decision is a record, not a permit.

## Context

Policy rules decide, for an Execution step, whether it may run unattended, needs a human, or is
denied. Before a policy can enforce anything, the hub must show what the rules would have decided
for real steps: which ones would change the outcome, and which inputs were missing when the
decision was made. Operations are the only path that runs skills, so the record has to be made for
every Execution step, and it must not change how any operation behaves.

Three forces shape the design:

- The operation writer is the single writer of operation state (ADR 0025). Adding a policy
  evaluation to its commit would put a rule engine and its inputs on the critical path of every
  approval and every step.
- Some inputs do not exist yet. The unattended ceiling is only reported by part of the fleet, no
  safety facts are produced, the containment helper is not built, and the autonomy switches are not
  modelled. A decision made on those inputs must say so, not pretend they are safe.
- Rules are data that outlive the binary that wrote them. A rule written today must read back the
  same way after a later release, and a published version must never change.

## Decision

- **Shadow decisions are written by a sweep task, not by the operation writer.** A recorder runs on
  a timer (every 5 seconds, at most 10 batches of 50 steps per tick). It reads operation state and
  the policy repository, and commits only `PolicyWrite::RecordShadow`. The operation writer does not
  wait for it, and no operation code path imports it. A failed or slow sweep leaves a backlog that
  drains on a later tick; it never blocks or fails an operation.
- **Idempotent, crash-safe inserts.** Each decision row has the step's execution id as its key.
  Inserting the same step twice is a no-op, so a sweep that crashes after its commit and runs again
  writes nothing new. The sweep state row records the last successful batch and the last error.
- **One row per Execution step, holding both decisions.** The row stores the enforced-rules decision
  and the shadow-rules decision side by side, with the outcome words of each (`allow`,
  `require_approval`, `unavailable`, `deny`, `error`). A row therefore answers "would these differ"
  without a join.
- **No published version means version 0.** A step planned before any version was published is
  evaluated against the empty rule set. Its row records `policy_version = 0` and the hash of the
  empty set, so it stays distinguishable from a step evaluated under a real version.
- **Coverage.** Every Execution step gets a row. Contracts under `agent.package.*` are never
  evaluated: their row has status `skipped`, a reason, and no decision on either side, so the
  coverage count stays complete.
- **Draft and publication are gated by the person.** An edit to a draft that touches a locked rule
  needs the admin password. Publishing always needs the password and an echo of the `preview_hash`
  returned by the preview, so the published version is the one that was shown.
- **Rules are stored as JSON envelopes.** Each rule is stored as `{"v":1,"rule":...}`. Golden tests
  pin the envelope bytes and the decision document, so a change to the serialised form fails a test
  and cannot silently re-hash stored versions. A later format is a new `v`, read beside `v:1`.
- **Publication index.** The set of contracts a publication is checked against is the compiled
  contracts plus the skills the agents announced. A skill name announced on several hosts is
  represented once, with the strictest effective class.
- **Missing inputs fail closed and are recorded as gaps.** The recorder substitutes the value that
  denies or narrows, never a value that allows, and lists each substitution in the row's `gaps`.
  The substitutions in this release are:
  - ceiling: none, so the ceiling does not cover the contract;
  - safety facts: none, so every fact is unknown;
  - root agent: assumed true when no live hello is available;
  - helper-dependent contracts: marked incomplete, because no containment helper exists yet;
  - autonomy: not modelled, so neither the pause nor the circuit breaker is in effect.
  A row with at least one gap has `degraded` set. Gap reasons are `not_produced_yet`,
  `host_not_connected` and `unresolvable`.
- **Configuration is shared, keys are scoped.** `[policy]` is one table name in two files:
  `hub.toml` reads `shadow` (`policy.shadow`) and `agent.toml` reads `allow_risk`
  (`policy.allow_risk`). Environment overrides are matched key by key, so an override for one role
  does not apply to the other, and a key the role does not read is skipped.
- **No retention yet.** Shadow rows are kept indefinitely in this release. A retention rule is a
  later decision.

## Consequences

- Operations gain no new failure mode. A bug in the recorder can leave rows missing or late; it
  cannot change an approval, a dispatch or an operation status. The recorder's module comment
  states the boundary; the engine's gate tests must be extended to assert that no operation module
  imports `policy_shadow` (rule R1 scans cover the crate edges, not the module edges).
- Shadow rows are a measurement, not a decision. Nothing reads them to allow or deny a step, and the
  `degraded` flag and the gap list are the way to see how far a measurement can be trusted.
- Rows grow with every Execution step and are not pruned in this release. Operators with long
  histories should expect the table to grow until retention lands.
- Golden snapshots for the rule envelope and the decision document must be reviewed on every change
  (rule R14, wire formats).
- Until a permit issuer exists, a policy authorization cannot be issued, so an enforced outcome of
  `allow` is still advisory. Every executed step keeps its human approval (ADR 0025).

## Alternatives considered

- **Evaluate inside the operation writer's commit.** Gives a consistent row for free, but puts rule
  evaluation and its inputs on the approval path, so a slow or failing input could block an operation.
- **Evaluate on read, when someone opens the page.** No stored rows and no growth, but the answer
  would depend on the rules and hosts of the moment, not of the step, and nothing would be recorded
  for steps nobody looked at.
- **Substitute safe values for missing inputs.** Makes most rows look clean, and hides exactly the
  gaps the shadow mode exists to show. Rejected in favour of fail-closed values with a visible gap
  list.
- **Store rules as plain JSON without an envelope.** Shorter, but a later format change would have to
  guess which shape a stored rule has. The `v` field makes the format explicit at no real cost.
