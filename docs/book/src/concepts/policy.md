# Policy

A **policy** is the set of rules that says what Domovoi may do on its own and what needs you. Every step of an operation that runs a capability is checked against it. Today the rules are only recorded, not enforced: a person still approves every step that changes a host, and nothing a rule says can run a step by itself.

## What a rule is

A rule matches a capability, a host or a target, and gives one of three answers:

- **Deny:** the step does not run. Nothing asks you.
- **RequireApproval:** the step waits for a person. This is what happens today for every step that changes a host.
- **Allow:** the step may run without asking. Allow is only meaningful once enforcement exists; until then it is a recorded answer, not a permission.

When several rules match one step, the most specific rule wins. A rule that names one host beats a rule that names every host, and a rule that names one capability beats a rule for a whole group of them.

A **locked** rule is one that cannot be weakened. A locked Deny stays a Deny in every version, and an edit that would remove it or turn it into an Allow needs the admin password.

## Shadow mode

Shadow mode is the way to see what a policy would do before it does anything. When it is on (`[policy] shadow = true` in `hub.toml`), the hub writes one decision row for every Execution step. Each row holds two answers:

- the **enforced** answer: what the published rules say, which is the answer that counts;
- the **shadow** answer: what the draft rules marked as shadow would say.

Nothing is enforced in shadow mode. The step runs or waits exactly as it would without it, and a sweep writes the rows a few seconds after the step is created. A failed sweep never blocks an operation; the rows just arrive later.

A row is **degraded** when some input the decision needs could not be read. The hub then uses the value that denies or narrows, and lists the gap in the row. A degraded row can therefore be stricter than the real answer would be, so read it as "at least this strict", not as a verdict. The inputs that are not available yet are listed under [What is not yet available](#what-is-not-yet-available).

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

## What is not yet available

Two kinds of input are not produced yet, so every decision made today uses the safe value for them:

- **The unattended ceiling.** The limit a host sets on what may run without you is not reported to the hub yet, so a rule that depends on it is evaluated as if the ceiling does not cover the step.
- **Safety facts.** The hub does not produce any facts yet, so every fact is unknown and is treated as not met.

Other inputs are also modelled conservatively for now: the root account of the agent is assumed to be root unless the hub knows otherwise, contracts that need the containment helper are marked incomplete, and the pause and the circuit breaker are not modelled, so neither is in effect. Each of these shows as a gap in the decision rows.

Until the ceiling and the facts arrive, a policy cannot tell you that a step is safe to run unattended. Shadow rows show what would change once they do.
