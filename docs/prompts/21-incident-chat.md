# 21 · Feature: incident follow-ups as a real chat

Three prompts: 21a (history, clear, full context), 21b (chat UI), 21c (optional: the
chat may propose actions). Run in order.

## Background (verified)

- Today a follow-up is one question, one answer:
  `crates/domovoi-engine/src/triage/followup.rs`, `answer()`. It builds the context bundle
  (`triage/context.rs`, `build()`), adds only the latest triage *headline*, sends **one**
  user message, runs `agent::converse()` with the read tools (so it can already call
  `docker_logs`, `journal`, `run_read_skill`, ...), cuts the answer to
  `MAX_ANSWER_CHARS` (1500) and records it through
  `ManagerHandle::record_follow_up` as a timeline entry of kind `"triage.followup"` with
  detail `{ "asked_by", "question", "answer" }` (`manager/annotate.rs`, `follow_up()`).
- Earlier questions are never sent back to the model, there is no way to clear them, and
  the model does not see the full report, the actions or the timeline.
- Discord thread messages from allowed users use the same path
  (`crates/domovoi-hub/src/auth/interactions/mod.rs`, `follow_up`), so web and Discord
  share one history.
- API: `POST /api/v1/incidents/{id}/ask` (`crates/domovoi-hub/src/api/incidents.rs`).
  UI: `web/src/features/incidents/AskBox.tsx` (shows only the latest answer).
- `latest_triage()` in `api/incidents.rs` looks at timeline kinds starting with
  `"triage."` except `"triage.followup"`; `reload()` in `triage/queue.rs` matches four
  exact kinds. Do not start new timeline kinds with `"triage."`.
- **`manager/decide.rs` is 492 lines: add at most 6 lines.**

---

## 21a · History, clear, and the full incident as context

### Steps

1. **Clear command.** Add `Command::ClearFollowUps { incident: IncidentId, by: UserId }`,
   `ManagerHandle::clear_follow_ups`, a free function in `annotate.rs` writing a timeline
   entry kind `"followup.cleared"` (summary `cleared by user:<id>`, publishes nothing), one
   arm in `decide()`, and add it to the "loads nothing" arm in `actor.rs`.
2. **History.** New file `crates/domovoi-engine/src/triage/history.rs`:
   - `pub struct Turn { pub question: String, pub answer: String, pub asked_by: String, pub at: Timestamp }`
     (serde, documented).
   - `pub fn since_clear(timeline: &[IncidentTimelineEntry]) -> Vec<Turn>`: the
     `"triage.followup"` entries after the last `"followup.cleared"` entry, oldest first,
     read from each entry's `detail`.
   - Re-export it from `triage/mod.rs`, and add
     `TriageHandle::history(incident) -> Result<Vec<Turn>, TriageError>`.
3. **Full context.** New `crates/domovoi-engine/src/triage/incident_context.rs` with
   `pub(crate) async fn about(shared, incident) -> String`: the latest report from the
   timeline (`"triage.report"` or `"triage.cached"` detail parsed as `TriageOutcome`),
   written out in full (headline, summary, cause, evidence lines, verdict, next step,
   proposals, details); the incident's actions
   (`deps.actions.list(&ActionFilter { incident: Some(id), ..Default::default() }, &Page::first(20))`:
   skill, status, args, reason); and the last 20 timeline lines (`kind: summary`). Cap
   the result with `cut(.., 8 * 1024)` and redact it.
4. **Conversation.** In `followup::answer()`:
   - First user message: `bundle` + `about()` + "Answer the operator's questions about
     this incident.". (`bundle` comes from `context::build`, so once prompt 24c is done it
     already includes the host profile.)
   - Then, for the last 10 turns of `since_clear(timeline)`: a `Role::User` message with
     the question and a `Role::Assistant` message with the answer (as `Part::Text`).
   - Then the new question as the last `Role::User` message. Keep the total of the
     history under 16 KiB by dropping the oldest turns first.
   - Raise `MAX_ANSWER_CHARS` to 4000 (Discord already splits long messages; update the
     test in `crates/domovoi-engine/tests/services/actions.rs` that asserts the cap).
   - Update `FOLLOW_UP_PROMPT`: mention that earlier turns of the conversation are
     included and that it may call the read tools and read skills it is offered.
5. **API.**
   - `GET /api/v1/incidents/{id}/followups` → `FollowUpsDto { turns: Vec<TurnDto { question, answer, asked_by, at }> }`.
   - `POST /api/v1/incidents/{id}/followups/clear` → 204 (calls the clear command; works
     on resolved incidents too, like `record_follow_up`).
   Add the POST to the `writes` list in `crates/domovoi-hub/tests/gates.rs`.
   Regenerate OpenAPI.
6. **Discord.** No change needed: thread follow-ups now get the history automatically.

### Tests

- `history::since_clear`: entries before a clear are dropped; order is oldest first.
- Engine (`tests/services/turns.rs` style, scripted LLM): ask twice; the second request
  to the model contains the first question and answer as earlier messages; after
  `clear_follow_ups`, a third question contains neither.
- Engine: the first message contains the report's cause and an action's skill name.
- Hub: `GET /followups` returns turns; clear empties it.

---

## 21b · The chat UI

### Steps

1. Hooks in `web/src/api/hooks/incidents.ts`: `useFollowUps(id)` (key
   `["incidents", "followups", id]`, add it to `queryClient.ts`), `useClearFollowUps(id)`;
   `useAsk(id)` invalidates the follow-ups query on success.
2. Replace `AskBox.tsx` with `web/src/features/incidents/IncidentChat.tsx`:
   - The turns as a message list: the question right-aligned in an accent bubble, the
     answer left-aligned rendered with `Markdown`, each with `RelativeTime`.
   - While asking, show the pending question immediately and a "Thinking…" bubble
     (optimistic: keep it in local state until the query refetches).
   - Input at the bottom: a textarea (max 1000 chars, counter), **Enter** sends,
     **Shift+Enter** adds a new line, Send button.
   - **Clear conversation** button (with `ConfirmDialog`: "The model forgets this
     conversation. The questions stay in the timeline.").
   - Scroll to the newest message when turns change (`ref.scrollIntoView`; guard for
     jsdom where it is missing).
   - Hint under the input: "Uses read-only tools on {host}. It cannot change anything."
3. Use `IncidentChat` in `IncidentDetailPage.tsx` in place of `AskBox`, under the
   heading "Chat".
4. The timeline keeps showing `triage.followup` entries; hide their long text there (the
   chat shows it).

### Tests

`incidents.test.tsx`: renders existing turns; sending a question shows it at once and
then the answer; Clear calls the endpoint after confirming; Enter submits and
Shift+Enter does not.

---

## 21c · Optional: the chat may propose actions

Proposals stay data until a human approves them (INV-1, INV-9); this only lets the chat
suggest them like triage does.

### Steps

1. Refactor `crates/domovoi-engine/src/triage/proposals.rs`: extract the loop of
   `create()` into
   `pub(crate) async fn store(shared, incident, skills, proposals: Vec<ActionProposal>) -> (BTreeMap<ActionId, String>, Vec<String> /* dropped reasons */)`
   and make `create()` call it.
2. In `FOLLOW_UP_PROMPT` explain: to suggest actions, end the answer with a line
   `PROPOSALS:` followed by one JSON array of `{ "skill", "args", "reason" }` using only
   skills from "Skills you may propose", at most 3.
3. In `followup::answer()`, split the answer at the last `PROPOSALS:` line, parse the
   array with `serde_json` into `Vec<ActionProposal>` (ignore it when it does not
   parse), call `proposals::store`, and replace the block with a line per stored
   proposal: "Proposed: {skill} ({reason}). Approve it on the Approvals page." and per
   dropped one: "Not proposed: {reason}".
4. For each stored action publish
   `DomainEvent::new(EventKind::ActionChanged { action, status: ActionStatus::Proposed }, &incident, true)`
   on `shared.deps.bus` (`crates/domovoi-engine/src/events.rs`); the hub turns it into an
   SSE `action` event (`crates/domovoi-hub/src/api/events.rs`), so the web Approvals page
   and the incident page refresh. Do **not** reuse `TriageCompleted`. Chat-proposed
   actions get no Discord approval buttons (their single-use tokens are only minted with
   a triage report); say "Approve it on the Approvals page" in the answer.

### Tests

- A scripted answer with a valid `PROPOSALS:` block stores one `Proposed` action and the
  returned answer no longer contains JSON; an invalid skill is reported as not proposed;
  a malformed block is ignored.

### Done when (all of 21)

- The incident chat remembers the conversation (web and Discord), can be cleared, sees
  the full report, actions and timeline, uses read tools and read skills, and (21c) can
  propose actions for approval.
