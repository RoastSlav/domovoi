# Operations

An operation is one durable piece of work on your hosts: find out why something is failing and, with a person's approval, fix it. It is made of steps. Each step records one thing that happened or was decided, such as a reading taken from a host, an assessment, a plan, a question to a person, an approval, a run of a skill, or a check afterwards.

- **Step:** one record with a kind, a phase, a cycle and an outcome. A step is never edited after it is written. Its outcome says how far it got: `succeeded`, `failed`, `refused`, `not_run`, `unavailable`, `outcome_unknown`, `skipped`, `cancelled`, or still `pending` or `running`.
- **Edge:** a link from a later step to an earlier one, such as "this check verifies that run" or "this retry replaces that failed run". Links only point backwards, so the history of an operation always reads in order.
- **Execution:** a step that runs a skill on a host. It needs a person's approval, and the approval is recorded with it.
- **Status:** the operation as a whole is queued, running, waiting for an approval, a user or an agent, waiting for a timer, paused, or finished (succeeded, failed, escalated or cancelled). The status is worked out from its steps, never set by hand.

## On by default

Operations are on unless you switch them off under `[orchestration]`. With `enabled = false`, Domovoi does not start the operation service and does not investigate incidents automatically: alert cards still go out, without a report. Follow-up questions and chat keep working, and the Operations endpoints answer 503. Approval is per step: you approve or deny each Execution step on the web or from its card in chat.

With `[orchestration] enabled` and `[triage] enabled` both on, an incident is investigated by an operation; see [Investigations](#investigations). A new round of reasoning after a failed check starts another cycle of that investigation. Investigate is the only kind of operation.

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

An operation can propose work, but it cannot run an execution on its own. Each execution waits for a person to approve it, and the approval expires after `approval_ttl` if nobody answers. Nothing in an operation, including the model, can approve an execution. The one exception is a policy permit for restarting a service or blocking a network source, which the hub issues only inside hard limits; see [Policy](policy.md#enforcement).

The **Approvals** page points at the operations that wait for you. A decision is made on the step itself, in the operation's detail page, in the Control room's approval card or from the card in chat.

### What an approval covers

Each proposed execution has a **plan hash**: a code shown on the step as `Plan a1b2c3d4`. It binds the capability and its version, the manifest and the SHA-256 of the entry script the agent will run, the host and its enrolled agent key, the incident's check and target, the exact arguments, and the execution id. It does not bind transitive dependencies, interpreters, sourced scripts, other files in the skill folder or the runtime environment, and it cannot prove the hub itself is honest: a compromised hub can compute any hash it likes, so the agent's own checks and the audit chain remain the runtime limits.

The approval request carries the hash the step showed. If it no longer matches what is stored, the approval is refused and nothing changes, so a stale or substituted card cannot approve a different plan. At dispatch the hub recomputes the hash from one authenticated agent session, re-reads the host's enrolled key and the incident subject, and refuses the run on any difference: a replacement session, re-enrolment, a replaced entry script, a changed target or a changed argument all invalidate the approval.

A Discord button has no room for the whole code, so it carries a short prefix. The button's single-use token is what authenticates the press; the prefix only catches a card that has gone stale. If a step's arguments are too long to show in chat, the card has no Approve or Deny button, only a link, because you must be able to read every argument before approving.

### Safety rules

- **Only people approve.** An approval comes from a signed-in web session or an allowed chat user. The model, alert text, log lines and other automation can never approve a step. The model's tools include nothing that can change one.
- **One decision per step.** Approving, denying and letting the window close are applied one at a time, in the order they arrive. If you press Approve and Deny together, only the first applies and the other is refused with a message that the step already has an outcome. Expiry never overwrites a decision a person made.
- **Destructive steps need a typed confirmation on the web.** A skill marked `risk = "destructive"`, or a step whose risk is unknown, can never be approved from a chat button.
- **Chat buttons work once.** Each step gets its own random token, and only its hash is stored. The token stops working after it is used and once the step is denied or expired.
- **The plan is checked again before it runs.** Just before running, Domovoi fetches the skill from the agent again. If the skill is gone, its risk or manifest changed, or the arguments no longer fit its parameters, the step is refused instead of run.
- **Nothing retries on its own.** A refused or failed step stays that way. To try again, the investigation proposes a new step, and someone approves it.
- **The loop guard limits repeat runs.** The same skill runs at most `loop_guard_per_hour` times (default 3) per host in any hour. A further approval is blocked, and the operation says so.
- **An offline agent holds the step.** If the host's agent is not reachable, Domovoi waits for it up to `agent_wait_limit` before refusing the step; an agent this hub rejected as incompatible is refused at once.
- **Runs are bounded.** The agent runs the skill directly, with no shell, as the skill's user and within the skill's timeout.
- **Results are verified.** A skill with a `verify` section is checked again after its delay. A write skill that applies to the incident's check is checked after 30 seconds. The re-check is scheduled in the same commit that records the run, so a hub restart between the run and the re-check does not skip it. A re-check that cannot reach the host is retried with growing delays (30 seconds doubling up to one hour, five attempts in all) and is then recorded as verification failed, with the reason that it could not run.

### After a restart

Restarting the hub loses nothing a person already decided and invents nothing. When it starts it reads the store and carries on from the effects still pending, so an approval made a moment before the restart still runs. A step that was dispatched when the hub stopped stays dispatched; when that host's agent authenticates again, Domovoi asks the agent's journal what became of it. A recorded result is committed, a run still going stays running, a run the agent can prove never reached it becomes **not run**, and one the agent cannot account for becomes **outcome unknown**. Uncertainty is shown as uncertainty and is never turned into a success or a failure: check the host before running the work again. A result recovered from the journal is shown as succeeded or failed, but the check that raised the incident is not re-run for it.

### Audit trail

Every step of an execution adds an entry to the append-only [audit log](../guide/audit.md). Each names the operation and the step, and none contains a secret or a typed confirmation.

| Entry | Records |
| --- | --- |
| `execution.proposed` | the execution waits for a person |
| `execution.approved`, `execution.denied` | the person and how they decided: web or chat user |
| `policy.permit_issued` | a policy permit authorized the execution |
| `execution.expired` | the approval window closed |
| `execution.refused`, `execution.unavailable`, `execution.blocked` | why the execution did not run: refused by a check, host or capability not available, stopped by the loop guard |
| `execution.dispatched` | the plan hash that matched |
| `execution.succeeded`, `execution.failed` | the outcome |
| `execution.outcome_unknown`, `execution.not_run` | the run may have happened, or never did |
| `execution.token_rotated` | the step's chat token was replaced; never the token |

## Investigations

With `[orchestration] enabled = true` (the default) and `[triage] enabled = true` a new incident is investigated by an Investigate operation. It is limited by the runs per hour, the monthly budget, and the tool calls, seconds and output tokens per run set under `[triage]`. It falls back between models, uses the tools for the incident's host and posts its report as the card on the incident. Its properties:

- The work survives a restart. A deferred or interrupted investigation continues from the store.
- The model works with six tools and no others: it lists and describes the capabilities of the host, requests evidence (a read of the host), stages a plan of at most three steps, asks you a question, and finishes with its report. It cannot approve, run or change anything.
- Every read the model makes is a step of the operation, so the trace shows what was looked at. A read that could not be made, because the host was offline or the read failed, is recorded as such, and the model is told the answer is unknown rather than empty. Reads that were refused, for instance because the budget was spent, are shown but not counted against it.
- A step the model stages is checked before it is accepted: the capability must exist on the host, its arguments must pass, the host must be one the investigation covers and be reachable, and a step that repeats a failed run must say what changed. A refused step comes back to the model with the reason so it can fix or drop it; steps it never corrected are listed in the report under "Proposals not offered". They are checked once more when they are stored.
- The result is an assessment, a plan and a notification step, and the card.
- When the model needs something only you know, it can ask you a question. The investigation then waits for your answer, shown on the Operations page, and continues with it; if nobody answers within `question_timeout`, it continues and says that no answer arrived. It asks at most twice, and never while it has a plan staged.
- Each action the model proposes, and that passes the checks, is an **Execution** step with a plan hash that a person approves on the operation. A proposal that does not pass is listed in the report under "Proposals not offered".
- An investigation closes itself when nothing is left to do. It stays open while a proposed step waits for a person, while a retry is pending, and while it is paused.
- When the hourly limit is reached the run is skipped with a note and tried again once an hour slot frees; a failed run is tried again after an hour, up to three failed runs.

A person approves or denies a step with the hash shown on it. A destructive skill, or one the host no longer offers, needs its name typed to approve. In chat, a step that proposes a read or write skill gets Approve and Deny buttons on the incident card, named after the skill and the host. A press approves with the same human authorization as the web, a single-use token and the first six characters of the plan hash. The token is stored only as a hash on the step, replaced when the card is sent again, and spent or cleared as soon as the step is decided. The plan prefix is a staleness check, not a boundary against a compromised hub: the hub still recomputes the full plan hash before dispatch and refuses any difference. A destructive step, a step whose risk is unknown, a step with arguments too long for the card, and any step beyond the card's third get a link to the Operations page instead, and the report then says where to approve. The card is rebuilt when a step is approved, denied or expires, so the buttons disappear. A later card refresh that carries no new token also shows the link until the card is sent again.

An investigation of an incident that already has one open is refused; re-running triage on the incident page starts an investigation and waits for its first outcome, and says so if it is still running. With orchestration off, re-running triage answers that investigations are not enabled.

## The Operations page

The Operations page lists your operations. Each row shows the operation's status.

Open an operation to see its detail page. It has a header with the status, then one banner at a time, the most urgent first:

- **Waiting for you** lists what the operation needs from a person: a step to approve, or a question to answer.
- A banner says when the operation is paused, when a limit has run out, or when it has finished.

The controls are:

- **Pause** stops the operation from adding steps. Nothing is undone, and you can resume at any time.
- **Cancel** stops what has not started. You can give a reason, which is kept with the operation. A run already handed to an agent is not recalled.
- **Resume** starts a paused operation again. If a limit ran out, raise that limit first.
- **Raise a spent limit** lets the operation go on after one of its budgets ran out.
- **Answer a question** sends your reply to the operation. The reply is shown to the model as information, never as an instruction.

A step that needs approval shows its plan code, for example `Plan a1b2c3d4`. Check what the step will do, then approve or deny it. Approval is the only way a step runs, apart from a policy permit. A destructive step, or one whose risk is unknown, also asks you to type the skill's name exactly as shown before the Approve button works.

When an operation ends, Domovoi writes a summary of it. See [Experience](experience.md).

Click any step in the list to open the step inspector beside the page, or as a full-width sheet on a narrow window. Press Escape to close it.

## The graph view

On a screen wide enough for it, the Steps section of an operation opens as a graph: one box per step, drawn left to right in the order the work depends on itself, with the same steps as a list right under it. On a narrow screen the timeline opens instead. The **View** switch next to the heading lets you pick Timeline, Tree or Graph, and your choice stays until you leave the page.

**What it shows.** Each box is a step. It has the kind of step, its outcome in words with an icon, and a short summary. Its colour is the outcome's tone, but the words and icon always say the same thing. A step you selected has a thick green border. A step that is running shows a still icon, not a spinner. Steps that are not decisions on their own, such as the model's turns and evidence requests, are folded in Standard and Essential: a dashed box such as "3 technical steps" stands for them, and Enter or a click opens it. A step that needs you, or went wrong, is never folded. Use **Fold technical steps** in the toolbar to close the groups you opened.

**What the lines mean.** An arrow runs from a step to the step that has to come before it. The pattern of the line tells you why, so it never depends on colour alone:

| Line                      | Meaning                                       |
| ------------------------- | --------------------------------------------- |
| Solid                     | Depends on: it could not start without it     |
| Dashed                    | Caused by: it was proposed because of that    |
| Dotted                    | Verifies: it checks the result of that step   |
| Dash and dot, alternating | Retry of: it is another attempt at that step  |
| Long dashes               | Answers: it replies to that question          |

The edge key under the graph lists the patterns that are present.

**Using the keyboard.** The graph is one stop in the tab order. Moving around it with the keys only moves focus: it does not select anything or change the page address. A line under the graph, read aloud by screen readers, describes the focused step.

| Key                     | What it does                                                                         |
| ----------------------- | ------------------------------------------------------------------------------------ |
| Tab                     | Enters the graph, at the selected step if there is one                               |
| Right arrow, Left arrow | Moves to a step that depends on this one, or to one this step depends on             |
| Down arrow, Up arrow    | Moves to the next or previous step in the same column                                |
| Page Down, Page Up      | Moves to the next or previous step in time order                                     |
| Home, End               | Moves to the first or the last step                                                  |
| `a`                     | Moves to the step that is active: running, else waiting for you, else not started    |
| Enter                   | Opens the step in the step inspector, or opens a folded group                        |
| Escape                  | Clears the selection and closes the inspector                                        |
| `+`, `-`, `0`           | Zooms in, zooms out, fits the whole graph in view                                    |

With a pointer, click a step to select it, double-click to open it, drag the background to move the view, and hold Ctrl or Cmd while you scroll to zoom. In the inspector, Escape closes it and puts focus back on the graph.

**The list under the graph.** The same steps appear below it as a list. Selecting a step in the list selects it in the graph, and the other way round. The list is the way to read the steps with a screen reader, and it stays on the page if the graph cannot be drawn. In that case the graph is replaced by a message with **Try again**, and nothing else on the page changes.

The graph is only for looking. Approving, denying, pausing and answering happen where they always do: on the banner and the cards above it, and in the step inspector.

## Replaying how an operation unfolded

Above the graph there is a **Replay** button. It opens a bar that lets you watch the graph as it was at an earlier moment: which steps existed, and what state each was in. A *moment* is one whole second in which something was recorded. The last stop, **Live**, is the operation as it is now. Nothing is loaded until you press Replay.

**Use the bar.** Drag the "Replay position" slider, or press **Previous moment** and **Next moment**. **Play** steps through the moments one a second, and the speed control makes it 2 or 4 times faster. **Back to live** (or **Close replay**) returns to the present. While you are looking at an earlier moment, a note above the graph says "Replay: showing" and the time, and the status line says which moment of how many you are on and what changed.

**What it can show, and what it cannot.** The replay is rebuilt from two things: the operation's final record and the journal of changes. It does not keep a picture of every second. So it is exact to the second for when a step was created and for the outcome it ended with. A step shows as `running` only where the journal recorded that it started or was dispatched, and otherwise stays pending until its final outcome. The text of a step is always the final text: the replay never invents words. Costs, budgets and the header of the page are not replayed.

**Everything else stays live.** Only the graph goes back in time. The step list, the banner, **Waiting for you** and its Approve button, and the step inspector all show the present, and the inspector says "Showing the live record" while you replay. You can approve or deny while replaying, and Replay never does it for you.

**When the history is incomplete.** If the oldest records of the operation were removed, a note says the replay starts later than the operation. If the operation has more records than a replay can hold, a note says the replay stops before the latest changes; **Back to live** shows the current state.

**Keys.** With the graph focused and the replay bar open: `,` goes to the previous moment, `.` to the next, Space plays or pauses, and `l` returns to live. The slider also answers the arrow keys, Home, End, Page Up and Page Down. Escape is not used by the replay.

**Reduced motion.** If your system asks for reduced motion, there is no automatic play: **Play** is disabled and says why. Previous moment and Next moment still work, and the graph changes at once.
