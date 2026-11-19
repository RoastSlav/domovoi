# Operations

An operation is one durable piece of work on your hosts: find out why something is failing and, with a person's approval, fix it. It is made of steps. Each step records one thing that happened or was decided, such as a reading taken from a host, an assessment, a plan, a question to a person, an approval, a run of a skill, or a check afterwards.

- **Step:** one record with a kind, a phase, a cycle and an outcome. A step is never edited after it is written. Its outcome says how far it got: `succeeded`, `failed`, `refused`, `not_run`, `unavailable`, `outcome_unknown`, `skipped`, `cancelled`, or still `pending` or `running`.
- **Edge:** a link from a later step to an earlier one, such as "this check verifies that run" or "this retry replaces that failed run". Links only point backwards, so the history of an operation always reads in order.
- **Execution:** a step that runs a skill on a host. It needs a person's approval, and the approval is recorded with it.
- **Status:** the operation as a whole is queued, running, waiting for an approval, a user or an agent, waiting for a timer, paused, or finished (succeeded, failed, escalated or cancelled). The status is worked out from its steps, never set by hand.

## Off by default, and no screen yet

Operations are switched off unless you enable them under `[orchestration]`. With them off, Domovoi does not start the operation service, and its storage stays empty. Actions and approvals work as described in [Actions and approvals](actions.md), unchanged.

There is no page in the web UI for operations yet, so there is nothing to create or watch from the browser.

## Settings

Set them in `hub.toml` under `[orchestration]`. A change takes effect after a restart.

```toml
[orchestration]
enabled = false          # start the operation service at all
max_running = 4          # operations that may run at the same time
approval_ttl = "24h"     # how long a proposed execution waits for approval (at most 7d)
question_timeout = "24h" # how long a question to a person waits for an answer
agent_wait_limit = "30m" # how long a run waits for a host's agent to come back

[orchestration.budgets]
model_turns = 20
evidence_requests = 40
executions = 5
cycles = 3
wall_time = "72h"
tokens = 200000
# cost_micros is not set, so cost has no limit
```

- `loop_guard_per_hour` (default `3`) limits how many runs of the same skill on the same host may start in one hour.
- A budget that runs out pauses the operation. It stays paused until someone raises that limit.
- Every value must be positive, and each duration must be longer than zero.

## Approval

An operation can propose work, but it cannot run an execution on its own. Each execution waits for a person to approve it, in the same way as an action, and the approval expires after `approval_ttl` if nobody answers. Nothing in an operation, including the model, can approve an execution.
