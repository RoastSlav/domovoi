# Policy

A **policy** is the set of rules that says what Domovoi may do on its own and what needs you. Every step of an operation that runs a capability is checked against it. Rules are enforced for two capabilities only: restarting a service with `unit.restart` and blocking a network source with `firewall.block_source`. For those, a rule that says Allow can let the step run without you, inside hard limits. For everything else a person approves every step that changes a host, and nothing a rule says can run it by itself.

## What a rule is

A rule matches a capability, a host or a target, and gives one of three answers:

- **Deny:** the step does not run. Nothing asks you.
- **RequireApproval:** the step waits for a person. This is what happens for every step that changes a host, and for every step whose rules do not say otherwise.
- **Allow:** the step may run without asking, but only for the two capabilities above. For any other capability, Allow is a recorded answer, not a permission.

When several rules match one step, the most specific rule wins. A rule that names one host beats a rule that names every host, and a rule that names one capability beats a rule for a whole group of them.

A **locked** rule is one that cannot be weakened. A locked Deny stays a Deny in every version, and an edit that would remove it or turn it into an Allow needs the admin password.

## Shadow mode

Shadow mode is the way to see what a policy would do before it does anything. When it is on (`[policy] shadow = true` in `hub.toml`), the hub writes one decision row for every Execution step. Each row holds two answers:

- the **enforced** answer: what the published rules say, which is the answer that counts;
- the **shadow** answer: what the draft rules marked as shadow would say.

Nothing is enforced in shadow mode. The step runs or waits exactly as it would without it, and a sweep writes the rows a few seconds after the step is created. A failed sweep never blocks an operation; the rows just arrive later.

A row is **degraded** when some input the decision needs could not be read. The hub then uses the value that denies or narrows, and lists the gap in the row. A degraded row can therefore be stricter than the real answer would be, so read it as "at least this strict", not as a verdict. The inputs that are not available yet are listed under [What is not available](#what-is-not-available).

## Versions and publication

There is one **draft**, which you edit freely. Publishing turns the draft into a numbered **version**. A version never changes after it is published: its rules, its number and the acknowledgements given with it are fixed. A decision row is written once and never rewritten, and it names the version that was current when it was written, so a later publication does not change a decision you already saw.

Before publishing, you ask for a **preview**. The preview shows what the draft would change and returns a `preview_hash`. Publishing then needs that hash echoed back, so the version you publish is the version you previewed. If the draft changed after the preview, the hash no longer matches and you preview again.

A step planned before any version was published is judged against an empty rule set. That is shown as version 0.

## Acknowledgements and the password

Some changes need more than a click:

- An edit to the draft that touches a **locked** rule needs the admin password.
- Publishing always needs the admin password and the echoed `preview_hash`.
- Publishing also stores the **acknowledgements** given with it. They are kept with the version and cannot be changed afterwards.

## The simulator

The simulator takes a description of a step (a capability, a host and its arguments) and shows what the draft and the published version would decide for it. It records nothing and enforces nothing. Use it to check a rule before you publish it, and to understand a row from shadow mode.

## Enforcement

A rule is acted on in one place: when a step asks to restart a service or block a network source, the hub evaluates the published rules and, only if the answer is **Allow automatically** and every guard below holds, issues a **permit**. The permit names the decision, the rules that applied, the policy version and the exact plan, and it expires after `[policy] permit_ttl` (never later than the step's own window). The permit goes to the agent, which checks it against its own `[unattended]` ceiling before it asks the helper to restart anything. A permit cannot be made by any API call or by a chat message: only the hub's issuer makes one, and the web UI has no control that does.

The guards run in this order, and the first one that fails withholds the permit. Each is recorded in the audit log with its reason:

1. The **pause switch** is off. Pausing needs only your session; resuming needs your password.
2. The capability can be allowed at all: only `unit.restart` and `firewall.block_source` can. Skills and `container.restart` never get a permit.
3. The rule is not **suspended** by its circuit breaker. A rule is suspended when it has `[policy] circuit_failures` failed restarts inside `circuit_window`. Resetting a rule needs your password.
4. No earlier restart of the same step has an **unknown outcome**. Resolve it yourself first, with your password.
5. The hub and the agent agree on what will be restarted: the helper's identity of the unit still matches the plan.
6. The permit rate limits hold: `[policy] max_permits_per_hour` for the whole hub and the agent's `max_per_hour` for the unit.

Anything else is the ordinary path: you approve in the web UI. A withheld permit never blocks that path.

What stays true when a permit exists: the agent's `[unattended]` section and the helper's `restart_units` and `protected_units` are local files that the hub cannot change. A compromised hub could label a request as allowed by policy, and the agent would still refuse it unless it is inside what you set on that host. That is the accepted limit of this design.

## What is not available

One kind of input is not produced, so every decision uses the safe value for it:

- **Safety facts.** The hub does not produce any facts, so every fact is unknown and is treated as not met. A rule cannot depend on one.

The unattended ceiling is reported by each agent, and the pause switch and the circuit breaker are read from the hub's own records, so none of those shows as a gap. Other inputs are modelled conservatively: the root account of the agent is assumed to be root unless the hub knows otherwise, and contracts that need the containment helper are marked incomplete. Each of these shows as a gap in the decision rows.

## The Policy pages

The **Policy** page shows what is in force, what is being drafted and what was recorded. It has three tabs: **Rules** (the active version), **Versions** (every published version, with the conflicts acknowledged when it was published) and **Decisions** (the recorded decisions, see [Reading a decision](#reading-a-decision)).

### Overview and recording status

The top of the page shows the active version with its rule counts, the draft if one exists, and the recording card. Recorded decisions are for comparison and are not applied themselves. Nothing on this page blocks or allows a step; the answers are there so you can compare them before you rely on a rule.

- When recording is on, the card shows how many decisions were recorded in the last 24 hours, how many are degraded, how many differ between the active rules and the shadow rules, how many could not be evaluated, and when the last recording pass ran.
- When recording is off, the page says so. To turn it on, set `shadow = true` under `[policy]` in `hub.toml` and restart the hub (see [Configuration](../reference/configuration.md)).
- A failed recording pass is shown in red with its error. It never stops an operation.

### Writing a draft

Choose **Start a draft** to copy the active version's rules into a draft. Only the draft is edited. Saving the draft stores your edits, but nothing runs with them until you publish. **Edit draft** returns to it later, and **Discard draft** under **More** throws the draft away after asking.

Each rule matches a capability, a host or a target, and gives one outcome: **Deny**, **Ask for approval** or **Allow** (see [What a rule is](#what-a-rule-is)). Two marks can sit on a rule:

- **Shadow** rules are recorded for comparison and never acted on. Use one to see what a rule would have done on recorded decisions before you make it real.
- **Locked Deny** rules carry a lock icon and the words "Locked Deny". No narrower rule can override them. Only a Deny can be locked.

Changing a locked rule needs your admin password, because a locked rule is the one part of a policy that must not be weakened quietly. Saving a draft that edits, unlocks or removes a locked rule opens **Change a locked rule**. It lists each locked change and what it would do to recorded decisions, then asks for the password. Nothing is stored until you confirm. Removing a locked rule from the draft asks first, for the same reason.

### Check rules

**Check rules** compares the draft with the active version and changes nothing. It reports:

- the problems that stop a save or publish, each linked to its rule;
- the conflicts between rules, and what the safer outcome is when neither rule is more specific;
- the locked rules that would change;
- how many recorded decisions would change.

If the rules have no conflicts, the check says so. Checking, saving and publishing are three separate steps.

### Publishing

**Review and publish** is available once the draft is saved. While there are unsaved edits it stays disabled, with the reason "Save the draft first.".

The publish page shows what will become version N, with its rule, locked and shadow counts. It lists:

- the **conflicts** that must be acknowledged. Tick each one before publishing. The acknowledgement is stored with the version and cannot be changed later;
- the **locked rules** that are added, changed, unlocked or removed;
- the **simulation** over recent recorded decisions, including how many would become Allow automatically.

Problems must be fixed first, and every conflict acknowledged, before **Publish version N** is enabled. Publishing then asks for your admin password. The version you publish is the one you reviewed: if the draft changed after the review, the hub refuses and the page asks you to review it again. A published version never changes, and a decision keeps the version that was current when it was made.

### Reading a decision

Open a row in **Decisions** to see one recorded decision. It shows:

- **The outcome**: Allow automatically, Ask for approval, Deny, Cannot decide, or Could not evaluate. A Deny is never offered for approval; to allow it, change the policy. Cannot decide means nothing runs and nothing is offered.
- **Why it won**: the reason in plain words (for example, "Most specific rule" or "A locked rule applies"), the rules that won, and any broader rule they overrode. A locked Deny is marked as locked, and the page says what the rules alone would have decided.
- **Hard limits**: the limits that applied after the rules, in their own card. They are not rules and cannot be edited on this page. When the answer with the limits differs from the answer with the rules alone, both are shown.
- **Missing inputs**: any input the decision could not read is listed as a gap. The result then uses the safe reading of that input, so it can be stricter than the real answer. Read it as "at least this strict".
- **Part of this explanation could not be read**: if the hub recorded something this page does not know, a warning says so and **Unread details** lists what was skipped. Do not treat that explanation as complete.
- **Not enforced**: the answer on this page is a record. It is never applied itself. A rule that allows a service restart or a block is acted on only when a new permit is issued for a live request, as described under Enforcement above.

**Simulate this step** opens the simulator for the same step.

### The simulator

The simulator re-runs decisions against rules you pick. It records nothing and enforces nothing.

1. Choose the rules: the draft, or a numbered version.
2. Choose which answers to compare: **Active policy**, **Shadow rules** or **Both**.
3. Choose the inputs: **Recorded** or **Current**.
4. Narrow the decisions by host, capability, status or date, and set how many to show (up to 500).

Nothing is sent until you choose **Run simulation**. With **Recorded** inputs, a decision is re-run with the host state, facts and ceiling that were recorded when it was made, wherever that information was recorded, and any gap is shown. Some decisions cannot be replayed: records that were skipped or could not be evaluated, and records whose inputs are missing. A degraded record keeps the limits it had when it was made, so a replay of it is only as complete as the original. With **Current** inputs, each step is re-run with today's clock and host state. That is a what-if, not a record of what happened, so use recorded inputs when you want to explain a past decision.

Each result is marked **Same**, **Changed** or **Cannot replay**.

### Time zones

The hub evaluates time windows in UTC. Every time window on a rule, such as "Start" and "End" for a time of day, shows its time zone beside it, for example "Start (UTC)". The pages read the zone from the policy overview and label it, so they follow the hub's zone. A window that ends before it starts runs overnight. If the hub does not report a zone, the time fields are read-only and say so.
