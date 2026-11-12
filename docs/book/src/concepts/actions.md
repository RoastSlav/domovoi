# Actions and approvals

Triage may suggest up to three actions for an incident, such as restarting a container. An action is a skill installed on the agent's host, together with the arguments to run it with. Nothing runs until a person approves it, and each step afterwards is checked again and written to the audit log.

## Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Proposed
    Proposed --> Approved: a person approves
    Proposed --> Denied: a person denies
    Proposed --> Expired: the approval window closes
    Approved --> Dispatched: re-checked, still the same plan
    Approved --> Rejected: re-check failed or loop guard
    Dispatched --> Succeeded: exit code 0
    Dispatched --> Failed: non-zero exit, agent offline or error
    Succeeded --> Verified: the original check passes again
    Succeeded --> VerifyFailed: the original check still fails
```

- **Proposed:** triage suggested it. The card shows Approve and Deny buttons. Proposals stay open for one hour.
- **Approved:** a person said yes, and the action is run straight away.
- **Rejected:** something changed between approval and running, or the loop guard stopped it. The thread says why.
- **Dispatched:** it was sent to the agent.
- **Succeeded or Failed:** what the skill's run reported. Failures are posted in the incident's thread.
- **Verified or VerifyFailed:** whether the check that raised the incident passes again afterwards. A verified action marks the incident as fixed. A failed verification is posted in the thread.

Restarting the hub loses nothing that a person already decided, and invents nothing: when Domovoi starts it reads the store and runs what is still marked **Approved**, so an approval made a moment before the restart still runs. An action that was **Dispatched** when the hub stopped is marked **Failed** with the reason that the hub restarted while it ran and the host must be checked before running it again. Its answer went to the session that is gone, so nothing knows whether the skill ran, and starting it again could be the dangerous thing. The loop guard keeps working across the restart: before the new session starts anything it reads back the dispatches the previous session wrote in the audit log, so runs that happened in the hour it is still in count against it, and a fourth approval of the same skill on the same host is stopped the way it would have been stopped without the restart.

## In the web UI

An action looks the same on the incident's page and on the **Approvals** page: the skill, its risk (the info button beside it says what that risk level allows), its state, the host, the arguments it would run with, who suggested it, and, while it waits, the time left to decide. The countdown turns to a warning in the last ten minutes. On the incident's page the card also shows why triage proposed it.

**Approve** always asks first, naming the skill, its arguments and the host; a destructive skill also needs its name typed. **Deny** is one click: nothing runs, and triage can propose the action again. After you approve, the card shows the action being sent and running, and a short message tells you how it ended, wherever on the page you are. An action that failed shows the reason; one that expired says that nobody decided in time and that nothing ran.

The **Approvals** page lists what is **Waiting for you**, the soonest to expire first, and, in a section that starts closed, what was **Recently decided** in the last 24 hours. It follows the host chosen in the top bar.

## Safety rules

- **Only people approve.** An approval comes from a signed-in web session or an allowed chat user. The model, alert text, log lines and other automation can never approve an action. The model's read-only tools include nothing that can change an action.
- **One decision per action.** Approving, denying and letting the window close are applied one at a time, in the order they arrive. If you press Approve and Deny together, only the first is applied and the other is refused with a message that the action already has an outcome; the card then shows the decision that was made. Expiry never overwrites a decision a person already made.
- **Destructive actions need a typed confirmation on the web.** Skills marked `risk = "destructive"` can never be approved from a chat button.
- **Chat buttons work once.** Each proposal gets its own random token, and only its hash is stored. The token stops working after it is used, and once the action is denied or expired.
- **The plan is checked again before it runs.** The approval covers one plan: the skill, the exact manifest, the host and the arguments. Just before running, Domovoi fetches the skill from the agent again. If the skill is gone, its risk changed, its manifest changed or the arguments no longer fit its parameters, the action is rejected instead of run.
- **Nothing retries on its own.** A rejected or failed action stays that way. To try again, triage or a person proposes a new action, and someone approves it.
- **The loop guard limits repeat runs.** The same skill runs at most three times per host in any hour. A fourth approval is blocked, and the thread says so.
- **An offline agent fails the action.** If the agent is not connected, Domovoi waits up to five minutes before marking the action failed.
- **Runs are bounded.** The agent runs the skill directly, with no shell, as the skill's user and within the skill's timeout.
- **Results are verified.** A skill with a `verify` section is checked again after its delay. A write skill that applies to the incident's check is checked after 30 seconds.

## Audit trail

Every step adds an entry to the append-only audit log:

| Entry | Records |
| --- | --- |
| `action.proposed` | who proposed it, the arguments, when the approval window closes |
| `action.approved`, `action.denied` | the person and how they approved: web or chat user |
| `action.expired` | the approval window closed |
| `action.rejected`, `action.blocked` | why the action did not run |
| `action.dispatched` | the plan hash that matched |
| `action.succeeded`, `action.failed` | exit code, argv hash, output hash, duration, and the first 4 KiB of the output, with secrets removed |
| `action.verified`, `action.verify_failed` | what the check reported afterwards |

Every entry also names the incident, host, skill, risk and plan hash.
