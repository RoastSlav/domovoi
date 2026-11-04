# Implementation prompts

Ready-to-paste prompts for a local coding model, one per reported problem or requested
improvement. Each prompt was written after reading the code it touches, so it already
names the files, functions, root causes and file-size limits. The model should not need
to search the repository.

## How to use them

1. Start a fresh session for every prompt (or sub-prompt such as `15a`).
2. Paste [`00-rules.md`](00-rules.md) first, then the prompt. For a prompt with parts,
   paste the shared "Background" section plus one part.
3. Let the model work, then run the checks at the end of `00-rules.md`. If something
   fails, paste the exact error back with: "Fix only this. Do not change anything else."
4. Commit after each prompt. Later prompts assume earlier ones are merged.

## The prompts

| # | Prompt | Kind | Size | Needs |
| --- | --- | --- | --- | --- |
| 01 | [Discord test fails in the setup wizard](01-fix-setup-discord-test.md) | bug, web + hub | S | |
| 02 | [Hub's own host shows "Never connected" without a local agent](02-fix-hub-host-without-local-agent.md) | bug, web | S | |
| 03 | [Resolved incidents still show "Action needed"](03-fix-resolved-incident-badge.md) | bug, web | S | |
| 04 | [Triage skipped by the hourly limit never runs again](04-fix-triage-retry-after-hourly-cap.md) | bug, engine | M | |
| 05 | [Redo triage (fresh run, no cache)](05-redo-triage.md) | engine + API + web | S | |
| 06 | [Group the incident page by state](06-incident-groups.md) | web | M | 03 |
| 07 | [Ack rules ("known conditions") API and page](07-ack-rules-api-and-ui.md) | API + web | M | |
| 08 | [Dismiss false positives](08-dismiss-false-positives.md) | core + engine + API + web | M | 07 |
| 09 | [Expected SSH logins](09-expected-ssh-logins.md) | core + API + web | M | 07 (08 optional) |
| 10 | [Skills and tools used per incident](10-skills-used-per-incident.md) | engine + API + web | M | |
| 11 | [Suggestions for hardware incidents ("AC unplugged")](11-hardware-incident-proposals.md) | 11a skills, 11b report | S + M | |
| 12 | [Explain things in the UI (what notes are, etc.)](12-ui-explanations.md) | web | S | |
| 13 | [More skill details in the UI](13-skill-details.md) | core + runner + proto + API + web | M | |
| 14 | [Enable and disable skills from the UI](14-skill-enable-disable.md) | hub + API + web | M | |
| 15 | [Add and delete skills from the UI](15-skill-add-delete-from-webui.md) | 15a agent reload, 15b **security change**, 15c UI | L | 14 |
| 16 | [Agent and hub logs in the UI](16-agent-logs.md) | 16a hub logs, 16b agent logs | M + M | |
| 17 | [Minimum version checks both ways](17-min-version-checks.md) | proto + hub + agent + web | M | |
| 18 | [New release notice and update help](18-release-check-and-updates.md) | 18a notice, 18b `domovoi upgrade` (optional) | M + M | 17 helps |
| 19 | [Fallback AI provider](19-fallback-provider.md) | engine + hub + web | M | |
| 20 | [Fallback model from the same provider](20-fallback-model.md) | config + engine + hub | M | 19 |
| 21 | [Incident follow-ups as a real chat](21-incident-chat.md) | 21a engine/API, 21b UI, 21c proposals (optional) | M + M + M | |
| 22 | [General chat, not tied to an incident](22-general-chat.md) | 22a engine, 22b API + UI | M + M | 21a, 21b |
| 23 | [Denser, modern, faster UI with a host selector](23-ui-refresh.md) | 23a shell, 23b layouts, 23c performance | M + M + S | |

## Suggested order

1. Bugs: **03 → 02 → 01 → 04**.
2. Shell first, so new pages fit it: **23a**.
3. Small wins: **05, 06, 12**.
4. Known conditions: **07 → 08 → 09**.
5. Triage insight: **10, 11a, 11b**.
6. Skills: **13 → 14**.
7. Versions and logs: **17 → 16a → 16b**.
8. Models: **19 → 20**.
9. Chat: **21a → 21b → (21c) → 22a → 22b**.
10. Updates: **18a → (18b)**.
11. Skill management: **15a**, then decide on **15b** (see below), then **15c**.
12. Polish: **23b → 23c**.

## Decisions that are yours, not the model's

- **15b changes invariant INV-3** ("the hub cannot push scripts"). The prompt keeps it
  opt-in per agent, limited to a separate folder and to read skills by default, and
  requires an ADR, but it is still a security trade-off. Without 15b, 15c falls back to
  generating copy-paste install commands, and 14 covers turning skills off.
- **18b** writes over the installed binary when run with `sudo`. A self-updating hub is
  deliberately not proposed: the hub runs unprivileged and Domovoi never spawns
  processes outside the runner.
- **18a** makes the hub call the GitHub API twice a day by default (it can be turned
  off). Flip the default in the prompt if you prefer opt-in.

## Notes from the investigation

- **01**: the wizard sends only the current step's fields to the test, the hub then
  rejects an enabled Discord section without approver IDs, and the wizard hides the
  validation issues. If you have the exact error text from the failed test, add it to
  the prompt; the fix covers this cause and shows any other one.
- **03**: the hub keeps the triage verdict after an incident resolves (by design), and the
  list badge preferred the verdict over the status. The detail page looked right because
  it adds "Resolved" next to the badge.
- **02**: the hub always registers its own machine as a `local` host, because it files
  hub-level problems (such as losing its network) against it.
- **19**: the fallback provider already exists in config and the engine; it was missing
  from the UI and only kicked in for network, rate-limit and outage errors.
- Several Rust files are close to the 500-line limit (`engine/src/manager/decide.rs` is
  at 492). The prompts that touch them say where to put new code instead.
