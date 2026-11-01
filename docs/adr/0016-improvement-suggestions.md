# 0016. Suggest improvements from history, never apply them

- Status: Proposed
- Date: 2026-11-01

## Context

After a few weeks Domovoi holds enough history to see patterns the user may miss:
- the same incident comes back and is fixed by hand the same way each time;
- triage keeps asking for logs it cannot read;
- the user acknowledges the same thing every week;
- a service is installed but nothing checks it;
- a check flaps.

Each pattern points at a concrete improvement, such as a new skill, an ack rule, a new check or a different threshold. Today these patterns go unnoticed.

Any feature that turns observations into changes must stay inside ADR 0007: only an authenticated human changes what Domovoi can do.

## Decision

- **Model.** Add `AdvisoryKind::Suggestion` with a typed payload `Suggestion { rule, evidence, proposal }`. Suggestions reuse the advisory machinery: they appear in the digest and the web UI and never send a notification on their own.
- **Detection is deterministic.** A set of pure rules in `domovoi-engine::suggest`, each `(history window, now) -> Vec<Suggestion>`, run by the scheduler once a day before the digest. Initial rules:

  | Rule | Fires when | Proposal |
  | --- | --- | --- |
  | `RepeatManualFix` | the same fingerprint was resolved by the same manual action at least 3 times in 30 days | a skill for that action |
  | `MissingLogAccess` | triage asked for a log path no read skill covers at least twice in 14 days | a read skill for that path |
  | `RepeatAck` | the same fingerprint was acknowledged at least 3 times in 30 days | an ack rule |
  | `UncheckedService` | an agent reports a known service (for example `snapraid`) with no matching check | the matching check |
  | `FlappingCheck` | a check opened and resolved at least 5 times in 7 days | a higher threshold or a confirmation run |

  Thresholds live in config with these defaults.
- **The LLM drafts, it does not decide.** For proposals that need text, such as a `skill.toml` draft or a one-line explanation, the hub may ask the configured LLM provider. That call has the normal budget and redaction rules. When no provider is set or the budget is spent, the suggestion is still raised without a draft.
- **Nothing is applied automatically.**
  - A drafted skill is a file shown in the web UI for the user to copy to the agent host. The hub never writes to `skills/` and never sends skill files to agents.
  - Proposed ack rules and thresholds are applied only from the web UI by an authenticated user, through the same endpoints as manual edits, and are written to the audit log.
  - Drafts with `risk = "destructive"` are labelled as such and never prefilled as enabled.
- **No nagging.**
  - Each suggestion has a stable key (rule plus target).
  - Dismissing a suggestion silences its key for good; snoozing silences it for a chosen period.
  - A rule that keeps firing updates the existing suggestion instead of adding a new one.
  - The digest shows at most 3 suggestions, newest evidence first.

## Consequences

- New `suggestions` table: key, rule, payload, state (`open`, `snoozed`, `dismissed`, `applied`), timestamps. This needs a new migration.
- Rules are pure and fully tested with the fake clock and testkit fixtures. Each rule gets tests for when it fires and when it does not.
- A gate test asserts that no suggestion code path constructs a `HumanApproval`, writes to an agent, or changes config without an authenticated request.
- Value grows with history, so this ships after the incident, action and triage data from v0.1 has been in use.

## Alternatives considered

- Let the LLM scan history and propose anything: unbounded cost, hard to test, and it may invent problems. Deterministic rules with an LLM for wording keep both cost and behaviour predictable.
- Auto-apply low-risk suggestions such as ack rules: even an ack rule hides alerts, so a wrong one is a silent failure. A human applies every change.
- Send suggestions as their own notifications: they are not urgent, and the digest is the right place for them.
