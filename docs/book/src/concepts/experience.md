# Experience

When an operation ends, Domovoi writes a short **resolution summary** of it. Later, the model and you can look up what happened the last time something like this went wrong on the same host.

A past approval or success is evidence only. It never lets anything run without your approval, never satisfies a policy and never skips a step.

## What a summary holds

| Field | Filled by | What it says |
| --- | --- | --- |
| `outcome` | Domovoi | `succeeded`, `failed`, `escalated` or `cancelled`. |
| `verified` | Domovoi | Whether the fix ran and the check after it passed. A report that only described the problem is not verified. |
| `subjects` | Domovoi | The host, check and target the operation was about. |
| `contracts_executed` | Domovoi | What was actually sent to an agent, and how each run ended. |
| `hosts` | Domovoi | Every host involved. |
| `problem`, `cause`, `fix` | the model, then you | Short text, at most 1000 characters each. |

The model's text is copied from the report it already wrote, so a summary costs no extra model call. Each narrative field shows its source: **model** for a draft, **user** once you have written or confirmed it.

A summary can take a few seconds to appear after an operation ends. Until then the operation shows no summary.

## Confirm or edit a narrative field

Only `problem`, `cause` and `fix` can be changed, and only after the operation has finished. Submitting the text the model drafted confirms it and records you as its source. Submitting new text adds a new revision. Earlier revisions stay in the history with who wrote them and when.

If the field changed since you opened it, the edit is refused and you are asked to reload. An empty text, or one over 1000 characters, is refused too. Every edit is recorded in the audit log without the text.

## Similar past operations and search

An operation or an incident page can list similar past operations from the same host. A past operation is similar when it concerned the same check or target, ran the same capability, or its text shares words with the current problem. The list shows why each one matched.

The search covers summaries by their words. It looks at one host by default. A search over every host happens only when you ask for it. The model's own search is always limited to the host of the incident it is working on, returns at most five operations and cannot ask for more.

Everything shown from past operations is labelled **historical evidence, not authorization**.

## Retention

Summaries are kept with the operation. They hold no raw command output. Deleting data does not remove it from backups until those rotate.
