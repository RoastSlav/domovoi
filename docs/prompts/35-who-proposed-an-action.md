# 35 · Bug: the audit log says who proposed an action and from where

## Background (verified on `main`)

- `crates/domovoi-engine/src/triage/proposals.rs`: `PROPOSER = "triage"` (`:18`), and
  `store()` always proposes with `Principal::System(PROPOSER.into())` (`:113`).
- `store()` is called by triage (`proposals::create`, from `run.rs:95`) **and** by the
  incident chat (`followup.rs:222-223`), so an action the chat suggested after an
  operator's question is audited as `system:triage`, and the asker is lost.
- `actions/audit.rs:94-101` `proposed()` turns the principal into the actor string and
  puts `args` and `expires_at` in the detail.
- The asker is known in `followup::answer` (`asked_by: UserId`); for Discord it is the
  hub admin account the chat acts as (`auth/interactions/mod.rs:143`).
- INV-1 is not affected: proposing is not approving, and the principal stays `System`
  (the model proposed, the human only asked).

## Steps

1. In `proposals.rs`, add
   `pub(crate) struct Proposer { pub component: &'static str, pub asked_by: Option<UserId> }`
   with `TRIAGE` (`"triage"`, `None`) as a const, and a `"chat"` component. `store()`
   takes a `&Proposer`; the principal is `Principal::System(proposer.component.into())`.
2. Pass the asker into the audit entry: give `audit::proposed` an
   `asked_by: Option<UserId>` parameter and add `"asked_by": "user:<id>"` to the detail
   when present.
3. `followup::answer` passes `Proposer { component: "chat", asked_by: Some(asked_by) }`;
   triage passes `TRIAGE`. Keep `PROPOSER` exported (it is public) as an alias of the
   triage component, with its doc saying so.
4. Web: the Approvals page shows "Suggested by triage" or "Suggested in chat" from
   `proposed_by` (check `ActionDto` in `crates/domovoi-hub/src/api/dto.rs` exposes it;
   if not, add `proposed_by: String` and regenerate OpenAPI).

## Tests

- Engine: `a_chat_proposal_is_audited_as_chat_with_the_asker` and
  `a_triage_proposal_is_still_audited_as_triage` in
  `crates/domovoi-engine/tests/services/chat.rs` / `report.rs`.
- Web: the approvals test shows the source line.

## Done when

- Every `action.proposed` audit entry tells triage from chat, and a chat one names who
  asked.
