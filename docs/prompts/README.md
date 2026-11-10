# Implementation prompts

Ready-to-paste prompts for a coding model, one per problem or improvement. Each was
written after reading the code on `main` (at `a50505a`, after `domovoi upgrade`), so it
names the files, functions, root causes and file-size limits it needs. The model should
not need to search the repository.

The prompts finished earlier (01–14, 16–22, 23a and 24) were removed: they are merged.
Their history is in this branch's earlier commits.

## How to use them

1. Start a fresh session for every prompt (or part, such as `25a`).
2. Paste [`00-rules.md`](00-rules.md) first, then the prompt. For a prompt with parts,
   paste the shared "Background" section plus one part.
3. Let the model work, then run the checks at the end of `00-rules.md`. If something
   fails, paste the exact error back with: "Fix only this. Do not change anything else."
4. Merge before starting a prompt that depends on it. Line numbers drift: if a cited
   line does not match, the model should find the named function instead.

## The prompts

| # | Prompt | Kind | Size | Needs |
| --- | --- | --- | --- | --- |
| 15 | [Add and delete skills from the UI](15-skill-add-delete-from-webui.md) | 15a agent reload, 15b **INV-3 change**, 15c UI | M + M + M | 32; 15b needs a decision |
| 23 | [Denser layouts and a targeted performance pass](23-ui-refresh.md) | 23b layouts, 23c performance | M + S | 42 |
| 25 | [One writer for action state, recovery after a restart](25-single-writer-for-actions.md) | bug, engine + store, ADR | M + M | |
| 26 | [One admission gate for triage, follow-ups and chat](26-one-admission-gate-for-model-calls.md) | security, engine + config + hub | M | |
| 27 | [A reused triage report proposes its actions again](27-cached-reports-create-actions.md) | bug, engine | S | |
| 28 | [Redact model failure reasons](28-redact-provider-failures.md) | security, engine | S | |
| 29 | [Authenticate webhooks before rate limiting](29-webhook-auth-before-rate-limit.md) | security, hub | S | |
| 30 | [Cap unauthenticated agent connections](30-cap-agent-connections.md) | security, hub | S | |
| 31 | [Sign releases with minisign](31-signed-releases.md) | security, binary + CI + installer, ADR | M | a key pair from you |
| 32 | [Per-host settings in their own table](32-per-host-settings-table.md) | refactor, store + core + hub | M | |
| 33 | [Maintenance windows from the UI](33-maintenance-windows.md) | feature, core + store + hub + web | M | 39 helps |
| 34 | [One conversation builder for both chats](34-unify-the-two-chats.md) | refactor, engine + hub | M | 26, 28 |
| 35 | [The audit log says who proposed an action](35-who-proposed-an-action.md) | bug, engine + web | S | |
| 36 | [Loop guard, triage cap and retries survive a restart](36-limits-survive-a-restart.md) | bug, engine + store | M | 25, 26 |
| 37 | [Markdown never loads remote images](37-markdown-without-remote-images.md) | security, web | S | |
| 38 | [Docs match the code](38-docs-match-the-code.md) | docs | M | run after the code prompts |
| 39 | [Split the files at the size limit](39-split-files-near-the-limit.md) | refactor | S | |
| 40 | [An audit log page](40-audit-log-page.md) | feature, hub + store + web | S | 35 |
| 41 | [Change the password, sign out other sessions](41-change-password-and-sign-out-everywhere.md) | feature, hub + CLI + web | S | |
| 42 | [Host selector everywhere, real error states](42-web-host-selector-and-error-states.md) | web | M | |
| 43 | [Restore from a backup, see the last backup](43-backup-restore.md) | feature, CLI + store + hub | S | |

## Suggested order

1. **Action and cost correctness** (real defects, do first):
   **25a → 25b**, **27**, **26**, **28**.
2. **Small security fixes** (independent, any order): **29**, **30**, **37**, then
   **31** once you have generated the minisign key.
3. **Make room before features**: **39**.
4. **Data model**: **32 → 36**.
5. **Chat and audit**: **35 → 34**, then **40**.
6. **Operator features**: **33**, **41**, **43**.
7. **Skill management**: **15a**, decide on 15b, then **(15b) → 15c**.
8. **Web polish**: **42 → 23b → 23c**.
9. **Docs**: **38** after the batch you merge (it describes the final state).

Parallel lanes, if you run more than one session (no shared files):

- lane A, engine: 25a → 25b → 27 → 26 → 28 → 36 → 35 → 34 (27, 26, 28 and 34 all touch
  `engine/src/triage/`, so they stay in one lane);
- lane B, hub and release: 29, 30, 31, 39 → 32 → 41 → 43;
- lane C, web: 37 → 42 → 23b → 23c.

## Decisions taken (from the audit questions)

- Action state uses **one writer** (25), not just a compare-and-set in the store.
- Follow-up questions and the general chat **count against the hourly triage cap** (26),
  with `reserved_for_incidents` keeping room for triage.
- Releases are signed with a **minisign key compiled into the binary** (31).
- `/metrics` and `/healthz` **stay unauthenticated** for Prometheus and Netdata; 38
  documents it.
- A generic webhook, ntfy, Slack and Telegram notifiers are **planned for later**, not
  written as prompts yet; 38 moves the generic webhook out of the v0.1 claims.

## Decisions still yours

- **15b** changes INV-3 ("the hub cannot push scripts"). The prompt lists three options;
  the audit recommends skipping it (15a and 15c already make adding a skill a
  copy-paste plus at most a minute's wait).
- **26**: is `reserved_for_incidents = 2` the right default with `per_hour_runs = 6`?
- **31**: you generate the key pair and add two repository secrets before running it.

## Later, not written yet

Ideas from the audit that build on existing code, for when the list above is done:
more notifiers (generic webhook, ntfy, email) and gateways (Slack, Telegram, Matrix);
a read-only second user; agent self-update from the Hosts page; more checks
(per-host certificate expiry, ZFS/mdraid/btrfs health, backup-job heartbeats, pending
package updates); a weekly cost line in the digest; config and notes export; saving the
general chat on the hub.
