# 34 · Refactor: one conversation builder for the incident chat and the general chat

No behaviour change for the operator except consistent limits. Do it after 26 (both
`answer` functions gain the admission check there).

## Background (verified on `main`)

- Incident chat: `crates/domovoi-engine/src/triage/followup.rs`. History comes from the
  incident timeline (`triage/history.rs`, `since_clear`, `count_clear`); its
  `remembered()` (`:67`) keeps 10 turns within 16 Ki **characters**, does **not** redact
  replayed turns, and keeps one oversized turn whole.
- General chat: `crates/domovoi-engine/src/triage/chat.rs`. History comes from the
  browser (`sessionStorage`, `web/src/features/chat/ChatPage.tsx:24-50`) in the request;
  its `remembered()` (`:59`) keeps 20 turns within 16 Ki **bytes**, redacts each turn,
  and cuts each to 2 KiB.
- Limits are declared twice: the hub caps a question at 1,000 chars and turns at 4,000
  (`crates/domovoi-hub/src/api/chat.rs:21-27`); the engine at 4 KiB / 2 KiB
  (`triage/chat.rs:28-31`).
- The general chat's host overview (hosts, connection state, active incidents, digest
  facts) is built in the HTTP handler, `api/chat.rs:147-244` (`overview`,
  `incident_line`, `fact_lines`). Prompt text belongs in the engine next to
  `triage/context.rs`.
- Both loop over `shared.chain()` with nearly the same code, and both build the
  "first message, earlier turns, question" list by hand.

## Steps

1. **History.** New `crates/domovoi-engine/src/triage/conversation.rs`:
   - `pub(crate) struct Exchange { role: Role, text: String }`.
   - `pub(crate) fn replay(redactor: &dyn Redactor, turns: &[Exchange], limits: Limits) -> Vec<LlmMessage>`
     with `Limits { max_turns, turn_bytes, total_bytes }`: newest first, every turn
     redacted and cut to `turn_bytes`, oldest dropped past `total_bytes`, then reversed.
     This is today's `chat.rs` behaviour; the incident chat adopts it, so replayed
     incident turns are now redacted too (INV-5).
   - One `pub(crate) const QUESTION_BYTES: usize = 4 * 1024;` used by both.
   - Move the tests of both `remembered` functions here and adapt them.
2. **Provider loop.** In the same module,
   `pub(crate) async fn ask_chain(shared, scope: Option<Scope>, skills, system: &str, messages: Vec<LlmMessage>, used: &mut Vec<ToolUse>) -> Result<String, TriageError>`
   with today's "try each provider, collect reasons, redact and clip the joined reasons"
   logic (the redaction from prompt 28). Both `answer` functions call it.
3. **Overview in the engine.** Move `overview`, `incident_line`, `fact_lines` from
   `api/chat.rs` to `crates/domovoi-engine/src/triage/overview.rs`. It needs
   `HostRepo` and `FactRepo` (already ports in `domovoi-core`) and connection state
   from `AgentLink::presence`: add `hosts` and `facts` to `TriageDeps` and wire them in
   `crates/domovoi-hub/src/runtime/mod.rs`. `TriageHandle::chat` takes
   `host: Option<HostId>` and builds the overview itself; drop its `overview: &str`
   parameter. `api/chat.rs` keeps validation only.
4. **One set of limits.** The hub validates against the engine's constants: re-export
   `MAX_QUESTION_CHARS`, `MAX_HISTORY_TURNS`, `MAX_TURN_CHARS` from
   `domovoi_engine::triage` and use them in `api/chat.rs` and `api/incidents.rs`
   (ask). Pick one unit (characters) for API validation and bytes inside the engine,
   and say so in the doc comments.
5. Keep `followup.rs`, `chat.rs` and the new modules under 500 lines each.

## Tests

- `replayed_incident_turns_are_redacted` (new behaviour).
- The moved `remembered` tests pass against `replay`.
- `crates/domovoi-hub/tests/hub/api_chat.rs` passes unchanged except where it built the
  overview itself.

## Done when

- One history function, one provider loop, one set of limits, and no prompt text in
  `crates/domovoi-hub/src/api/`.

## Not in this prompt

- Saving the general chat on the hub (it stays in `sessionStorage`). That is a later
  feature.
