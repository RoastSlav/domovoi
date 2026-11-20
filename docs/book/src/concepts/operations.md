# Operations

An operation is one durable piece of work on your hosts: find out why something is failing and, with a person's approval, fix it. It is made of steps. Each step records one thing that happened or was decided, such as a reading taken from a host, an assessment, a plan, a question to a person, an approval, a run of a skill, or a check afterwards.

- **Step:** one record with a kind, a phase, a cycle and an outcome. A step is never edited after it is written. Its outcome says how far it got: `succeeded`, `failed`, `refused`, `not_run`, `unavailable`, `outcome_unknown`, `skipped`, `cancelled`, or still `pending` or `running`.
- **Edge:** a link from a later step to an earlier one, such as "this check verifies that run" or "this retry replaces that failed run". Links only point backwards, so the history of an operation always reads in order.
- **Execution:** a step that runs a skill on a host. It needs a person's approval, and the approval is recorded with it.
- **Status:** the operation as a whole is queued, running, waiting for an approval, a user or an agent, waiting for a timer, paused, or finished (succeeded, failed, escalated or cancelled). The status is worked out from its steps, never set by hand.

## On by default

Operations are on unless you switch them off under `[orchestration]`. With `enabled = false`, Domovoi does not start the operation service and does not investigate incidents automatically: alert cards still go out, without a report. Follow-up questions and chat keep working, and the Operations endpoints answer 503. Actions that were still open when you upgraded are closed at the first start and kept as read-only history, labelled Legacy; see [Actions and approvals](actions.md). Approval is per step: you approve or deny each Execution step on the web or from its card in chat.

With `[orchestration] enabled` and `[triage] enabled` both on, an incident is investigated by an operation; see [Investigations](#investigations). There is no separate triage queue any more. A new round of reasoning after a failed check starts another cycle of that investigation. No other kind of operation exists yet.

To keep automatic investigation off, set `enabled = false` and restart the hub. A hub that has `[triage] enabled = true` and `enabled = false` logs a warning at start.

## Settings

Set them in `hub.toml` under `[orchestration]`. A change takes effect after a restart.

```toml
[orchestration]
enabled = true           # start the operation service at all
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
- A model turn counts against `model_turns` as soon as it is accepted, whether or not it reports its usage.
- An operation that is queued because `max_running` is reached keeps the reads and questions it started with. They wait, and run once it has a slot.
- Every value must be positive, and each duration must be longer than zero.

## Cancel, pause, resume and answer

You can cancel an operation, pause it, resume it and answer the question it asks, from the Operations page or the API (see [Operation controls](../reference/api.md#operation-controls)). Cancelling stops what has not started; a run already handed to an agent is not recalled, and its result is still recorded. Pausing stops the operation from adding steps until you resume it. Resuming an operation that paused because a limit ran out needs a higher limit for it; one you paused yourself needs nothing. Your answer to a question is kept with the operation and shown to the model as information, never as an instruction to Domovoi. None of these asks for your password again: a logged-in session is enough, as it is for approving a step.

## Approval

An operation can propose work, but it cannot run an execution on its own. Each execution waits for a person to approve it, in the same way as an action, and the approval expires after `approval_ttl` if nobody answers. Nothing in an operation, including the model, can approve an execution.

## Investigations

With `[orchestration] enabled = true` (the default) and `[triage] enabled = true` a new incident is investigated by an Investigate operation. It gives the same report as the old triage queue did: the same limits (runs per hour, the monthly budget, tool calls, seconds and output tokens per run), the same fallback between models, the same tools for the incident's host, and the same card on the incident. What changes is where the work lives:

- The work survives a restart. A deferred or interrupted investigation continues from the store.
- The model works with six tools and no others: it lists and describes the capabilities of the host, requests evidence (a read of the host), stages a plan of at most three steps, asks you a question, and finishes with its report. It cannot approve, run or change anything.
- Every read the model makes is a step of the operation, so the trace shows what was looked at. A read that could not be made, because the host was offline or the read failed, is recorded as such, and the model is told the answer is unknown rather than empty. Reads that were refused, for instance because the budget was spent, are shown but not counted against it.
- A step the model stages is checked before it is accepted: the capability must exist on the host, its arguments must pass, the host must be one the investigation covers and be reachable, and a step that repeats a failed run must say what changed. A refused step comes back to the model with the reason so it can fix or drop it; steps it never corrected are listed in the report under "Proposals not offered". They are checked once more when they are stored.
- The result is an assessment, a plan and a notification step, and the card.
- When the model needs something only you know, it can ask you a question. The investigation then waits for your answer, shown on the Operations page, and continues with it; if nobody answers within `question_timeout`, it continues and says that no answer arrived. It asks at most twice, and never while it has a plan staged.
- Each action the model proposes, and that passes the checks, is an **Execution** step with a plan hash that a person approves on the Operations page. It never becomes a row on the Approvals page. A proposal that does not pass is listed in the report under "Proposals not offered".
- An investigation closes itself when nothing is left to do. It stays open while a proposed action waits for a person, while a retry is pending, and while it is paused.
- When the hourly limit is reached the run is skipped with a note and tried again once an hour slot frees; a failed run is tried again after an hour, up to three failed runs.

A person approves or denies a step with the hash shown on it. A destructive skill, or one the host no longer offers, needs its name typed to approve. In chat, a step that proposes a read or write skill gets Approve and Deny buttons on the incident card, named after the skill and the host, exactly like an action. A press approves with the same human authorization as the web, a single-use token and the first six characters of the plan hash. The token is stored only as a hash on the step, replaced when the card is sent again, and spent or cleared as soon as the step is decided. The plan prefix is a staleness check, not a boundary against a compromised hub: the hub still recomputes the full plan hash before dispatch and refuses any difference. A destructive step, a step whose risk is unknown (proposed before risk was recorded), a step with arguments too long for the card, and any step beyond the card's third get a link to the Operations page instead, and the report then says where to approve. The card is rebuilt when a step is approved, denied or expires, so the buttons disappear. A later card refresh that carries no new token also shows the link until the card is sent again.

An investigation of an incident that already has one open is refused; re-running triage on the incident page starts an investigation and waits for its first outcome, and says so if it is still running. With orchestration off, re-running triage answers that investigations are not enabled.

## The Operations page

The Operations page lists your operations. Each row shows the operation's status. Actions from before operations existed are listed too, with a **Legacy** label. A Legacy action has no steps of its own; open its incident page to see what happened and to act on it there.

Open an operation to see its detail page. It has a header with the status, then one banner at a time, the most urgent first:

- **Waiting for you** lists what the operation needs from a person: a step to approve, or a question to answer.
- A banner says when the operation is paused, when a limit has run out, or when it has finished.

The controls are:

- **Pause** stops the operation from adding steps. Nothing is undone, and you can resume at any time.
- **Cancel** stops what has not started. You can give a reason, which is kept with the operation. A run already handed to an agent is not recalled.
- **Resume** starts a paused operation again. If a limit ran out, raise that limit first.
- **Raise a spent limit** lets the operation go on after one of its budgets ran out.
- **Answer a question** sends your reply to the operation. The reply is shown to the model as information, never as an instruction.

A step that needs approval shows its plan code, marked `v2 binding`. Check what the action will do, then approve or deny it. Approval is the only way an action runs. A destructive action, or one whose risk is unknown, also asks you to type the skill's name exactly as shown before the Approve button works.

When an operation ends, Domovoi writes a summary of it. See [Experience](experience.md).

Click any step in the list to open the step inspector beside the page, or as a full-width sheet on a narrow window. Press Escape to close it.
