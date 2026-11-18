# 36 — Operator conversations: durable conversation aggregate, streamed answers, panel and workspace

| | |
|---|---|
| Branch | `operator-conversations` |
| Milestone | M6 |
| Depends on (must be merged) | 29 (use-case layer; see "Hub use-case layer" below), 28 (journal-backed SSE: `ApiDeps.journal`, `snapshot_seq`, the `Topic::Conversation` name, `useJournalStream`) and 26 (`AggregateKind::Operation`, `OperationRepo`; this prompt adds its `AggregateKind::Conversation` beside it). Both are written but may not be merged when this brief is read: every name taken from 26 or 28 below is "introduced by prompt NN, verify names in code before use". 08, 09, 17 and 57 are merged and are used as they are. Prompt 27 (operation read endpoints) is **not** required; if it has merged, use its operation read dependency on `ApiDeps` (verify the field name) for the attach step, otherwise use `OperationRepo` directly (section 7, step 1). Prompt 30 (shell and right slot) **must be merged first**: register the Operator panel with the right-slot manager 30 introduces, and build no second dock (section 3.6; if no slot manager exists, the order is wrong: stop and report). |
| Can run in parallel with | 29, 30, 31, 32, 34, 41 (order-flexible, never concurrent; take the next free migration number when you cut the branch) |
| Produces (shared contracts) | The `live.*` ephemeral SSE event names and payloads (`live.start`, `live.text`, `live.tool`, `live.end`); the `conversations:<uuid>` arm of `topics_of`; `AggregateKind::Conversation`, `StateWrite::Conversation`, the `conversation.*` journal kinds; tables `conversations` and `messages`; the `ConversationRepo` port and the conversation writer (`ConversationService`, `ConversationHandle`); the `AnswerSink` and `LivePublisher` ports; HTTP `/api/v1/conversations...` (replaces `POST /api/v1/chat`); web `OperatorThread` kit component (the panel chrome is prompt 30's `SidePanel`) and the Operator panel and workspace. |
| Consumes | 28 (SSE contract, `useJournalStream`, `snapshot_seq`); 26 (`AggregateKind` pattern, `OperationRepo::get`); 57 (`RetentionPolicy.conversations`, `Store::purge`, `PURGED_TABLES`); 17 (`admit`, `Purpose::Question`, reached through `TriageHandle`); the existing general chat code (`triage::chat`, `api/chat.rs`, `features/chat`). |
| Launch gate | none beyond the dependencies |
| Suggested model | medium. Four layers (core, store, engine and hub, web) but each follows an existing pattern; the streaming sink and the web live state need care. |


## Hub use-case layer (read first)

Prompt 29 (`use-case-layer`) runs before this prompt in the recommended order and changes how hub code is
organised. Check `crates/domovoi-hub/src/services/` on your branch. If it exists:

- Every command (anything that writes) is a method on a service in `crate::services`, taking `&Caller`.
  The HTTP handler in `api/` and any chat handler in `auth/interactions/` only parse the request, call that
  method and map the result. Plain reads may stay in `api/` handlers.
- Do not add an engine handle (`ActionHandle`, `OperationHandle` or a new service handle of yours) to
  `ApiDeps` or `ChatDeps`; give it to `Services` instead. The arch rules `MOD-1`…`MOD-4` reject the
  other form.
- A new top-level hub module must be listed in `ZONES` or `UNZONED_MODULES` in `xtask/src/arch/rules.rs`.
- Where this brief says "add `X` to `ApiDeps`" for a handle used by a command, read it as "add `X` to
  `Services`". Read-only dependencies may still go on `ApiDeps`.

If `services/` does not exist, prompt 29 is not merged: stop and report, because the order is wrong.

## 1. Objective

Replace the browser-only, one-request-per-answer general chat with **durable Operator conversations**.

After this change:

1. A **conversation** is stored on the hub: its explicit scope (all hosts, or one host), an optional attached
   operation, and its **messages** with their status. Chat history no longer lives in `sessionStorage`. A reload,
   another browser tab or a hub restart shows the same thread.
2. Asking a question **commits the question and a pending answer in one transaction**, answers `202` at once, and
   runs the model in a background task that the browser can leave and come back to. The finished answer is committed
   as a journal event (`conversation.message`) and stays in the thread.
3. While the model works, the answer **streams** to the browser as ephemeral SSE events (`live.start`, `live.text`,
   `live.tool`, `live.end`) that carry **no `id`** and are never replayed. A secret is never streamed.
4. A **stale reply is discarded** by turn (request) id on both sides: a late model result for a stopped or superseded
   turn is never committed and never shown.
5. The **composer draft is kept** in the browser until the send is committed.
6. The web UI gets the **Operator**: a side panel available on every page and a workspace page (`/operator`) with a
   conversation list. The old Chat page and `POST /api/v1/chat` are removed.
7. The Operator stays **read-only** (INV-1, INV-9): it uses the same closed `ReadTool` catalogue as today, it cannot
   propose or approve anything, and a gate test fails if the conversation code can reach an executor.
8. Operator conversations are purged after `retention.conversations_days` (default 180): the messages are deleted,
   and the link to an operation is kept.

Parity checkpoint (`05` section 7): "chat to Operator: host-scoped read-only Q&A unchanged, now durable". The model
input, tools, limits, admission and redaction are the existing ones; section 3 lists them so you can prove it.

Not in this prompt: meta-tools, proposing or starting operations, layered context, experience search, an attention
centre, voice, editing or deleting single messages, and any change to the incident follow-up thread (section 5).

## 2. Architectural context

Plain-language summary of the decisions you implement. You do not need to open the planning documents.

- **RW-20, durable conversations.** `Conversation` is an aggregate with its own writer. Messages are stored.
  Streaming tokens are ephemeral SSE events without ids. The committed final message is a journal event. A
  conversation can attach to an operation. Host or global scope is explicit per conversation. A stale response for a
  superseded request is discarded by request id. The composer draft is kept locally until the send is committed.
- **One journal (RW-15, D-02).** Committed state changes are rows of the single `journal` table, written in the same
  transaction as the state (`seq` = commit order). The SSE stream (prompt 28) replays journal rows by topic.
  `conversations:<uuid>` is a topic prompt 28 reserved and this prompt fills (`topics_of` arm). Ephemeral events
  (model tokens) are **not** journal rows: they have no id, are never replayed, and their final state is the
  committed message. The SSE event-name prefix `live.` is reserved by prompt 28 for exactly this; this prompt owns the
  four names.
- **Writers (04 section 2).** Each aggregate has exactly one writer: a tokio task with a command channel that loads
  fresh state, decides, and commits one `Transaction` through the `UnitOfWork`. Conversations get their own writer
  (`ConversationService`). Nothing else writes `conversations` or `messages` rows (the purge job deletes old rows, see
  section 8).
- **The Operator is read-only (INV-1, INV-9).** The model's tools are the closed `ReadTool` set. The
  Operator has **no** approve path: `HumanApproval::new` stays in `crates/domovoi-hub/src/auth/`; nothing in this
  prompt constructs an `Authorization`, an `ActionRequest`, a plan or an operation step. Attaching an operation only
  lets the model read a short summary of it. Meta-tools (propose, start an operation) are prompt 31.
- **Model admission is unchanged (prompt 17).** A model answer still passes `admit(Purpose::Question)`
  (hourly window persisted, monthly budget, the reserved triage runs) and the one-answer-per-worker semaphore inside
  the existing chat code. A refusal now becomes a stored **failed answer** with a reason code instead of an HTTP
  error, because the question was already committed.
- **Redaction (INV-5).** Every model input is redacted by the existing code, and the answer is redacted before it is
  stored. This prompt adds the same rule to **live text**: only text that is already safe (redacted, and not the
  trailing half-finished word) is ever published. Stored messages are redacted. Journal payloads and audit rows of
  conversations never contain message text.
- **Retention (D-21, RW-28).** Conversations are kept 180 days (configurable as `retention.conversations_days`,
  prompt 57). Pruning deletes the messages; a conversation row that is attached to an operation stays as the link.
  Deleted data stays in backups until they rotate; the book already says so.
- **Host scope (lessons INV-H1, INV-H2).** The host selector in the top bar is a view filter. A conversation's scope
  is chosen explicitly when it is created and never follows the selector afterwards. "All hosts" is never an implicit
  default: creating a conversation without a scope is a 400.
- **Actors (RW-29).** The person's questions are journaled with `Actor::User`. Everything the service decides (an
  answer, a failure, a restart cleanup) is `Actor::System("operator")`.
- **Time (rule 11).** The live-text throttle and every timestamp take the `Clock` port. Tests never sleep.
- **Wire protocol is frozen at 1.2 (RW-42).** This prompt changes only the browser-facing HTTP API and the in-process
  LLM port. No agent protocol message changes.

## 3. Repository context

Verify every item in step 1 of section 7 and report discrepancies before coding. Marks: **(existing, path:line)** or
**(new)**. Line numbers are from when this brief was written.

### 3.1 The chat today (to be replaced)

- **Hub endpoint (existing).** `crates/domovoi-hub/src/api/chat.rs`: `POST /api/v1/chat`, `operation_id = "chat"`,
  `tag = "hosts"` (`:63-75`). `ChatRequest { host: Option<String>, history: Vec<ChatTurnDto>, question }` (`:42-51`),
  `ChatResponse { answer }`. Limits: `MAX_QUESTION_CHARS = 1_000` (`:21`), `MAX_HISTORY_TURNS = 20` (`:24`),
  `MAX_TURN_CHARS = 4_000` (`:27`). The client sends the **history**, including `assistant` turns it can forge.
  The handler resolves the host (`host_named`), downgrades an offline host to "no tools" (`:124-127`: `tool_host` is
  `Some(id)` only when `host_dto(..).connected`), builds the text overview (`overview`, `:152-204`: one line per host,
  the newest active incidents, and for a chosen host the last day's digest facts via `FACT_WINDOW`), then runs
  `triage.chat(tool_host, &overview, &history, &question)` inside `tokio::spawn` so a closed tab does not cancel the
  call (`:130-146`). Errors map: `TriageError::Provider` -> `ApiError::Unavailable`, `BudgetSpent` ->
  `ApiError::BudgetSpent`, `Busy{retry_at}` -> 429 through `busy(..)`, `AdmissionUnavailable` -> `Unavailable`,
  `Store` -> internal. Route registered at `crates/domovoi-hub/src/api/mod.rs:335` (`routes!(chat::chat)`).
- **Engine (existing).** `crates/domovoi-engine/src/triage/chat.rs`: `CHAT_PROMPT` (`:15`), `ChatRole`, `ChatTurn`
  (`:40-54`), `remembered(redactor, history)` (`:61`, newest `MAX_TURNS = 20`, each cut to `TURN_BYTES`, total
  `HISTORY_BYTES`), `answer(shared, host, overview, history, question) -> Result<String, TriageError>` (`:139`). It
  takes the `shared.questions` semaphore (`Busy{retry_at: None}` when none free), calls `admit(shared,
  Purpose::Question)`, builds the opening message, then per provider in `shared.chain()` runs either
  `agent::converse` (chosen host and a provider with tools) or `agent::ask` (no tools), handles
  `LlmError::CapabilityRejected` once per capability, and returns `clip(redact(answer), MAX_ANSWER_CHARS)`
  (`MAX_ANSWER_CHARS = 4000`, `followup.rs:45`). The tool calls the model made are collected in a local
  `tool_use: Vec<agent::ToolUse>` and **dropped** ("a general chat has no incident whose timeline they could go on",
  `:184-186`). `TriageHandle::chat` (`crates/domovoi-engine/src/triage/mod.rs:314`) is a thin wrapper. Many engine
  tests call `.chat(..)` (`tests/services/chat.rs`, `capability.rs:445`, `redact.rs`, `reliability.rs`); **keep that
  method and its behaviour**.
- **One model request (existing).** `agent::ask` (`triage/agent.rs:261`) loops over `complete(shared, provider, req,
  deadline)` (`triage/complete.rs:17`) which calls `provider.complete(req)` (`complete.rs:46`) under the run deadline,
  records usage, redacts errors, and fails closed on an unpriced turn. A turn that ends at `MaxTokens` is **discarded
  and asked again** with a larger allowance (`agent.rs:280-294`); `agent::converse` (`:329`) runs the read-tool loop
  and returns only the **last** response's text (`text_of(&response.parts)`). `RunBudget` (`:96`) carries per-turn
  tokens, the remaining total and the deadline through every request of one run.
- **The LLM port (existing).** `crates/domovoi-core/src/ports/llm.rs`: `trait LlmProvider` (`:228`) has `id`, `model`,
  `supports_tools`, `prices_usage` and **one** call, `complete(&self, req: LlmRequest) -> Result<LlmResponse,
  LlmError>` (`:252`). There is **no** way to receive text while a turn runs. The adapters already stream on the wire
  and assemble: `Anthropic::complete` (`crates/domovoi-llm/src/anthropic.rs:74`, sets `stream = true` at `:76`, calls
  `anthropic_stream::assemble(&mut events, secret)` at `:87`), `OpenAiCompatible::complete` (`openai.rs:168`,
  `openai_stream::assemble` at `:83`), the CLI provider (`cli.rs:205`, no streaming), and the hard-deadline wrapper
  `Deadline` (`deadline.rs:37`, `complete` races the inner call against `Clock::sleep_until`). The testkit fake is
  `ScriptedLlm` (`crates/domovoi-testkit/src/llm.rs:137`, `push_text` `:78`, `push_ok` `:73`, `hold` `:107`).
  `anthropic_stream::assemble` accumulates `BlockDelta::TextDelta` into `Block::Text` (`anthropic_stream.rs:~155`);
  `openai_stream::assemble` accumulates `choice.delta.content` into `content` (`openai_stream.rs:~109`).
- **Web (existing).** `web/src/features/chat/ChatPage.tsx` (132 lines): keeps turns in `sessionStorage` through
  `conversation.ts` (`chat:<host>` keys), calls `useChat()` (`web/src/api/hooks/chat.ts`, `api.POST("/api/v1/chat")`),
  and renders the kit's `ChatThread` (`web/src/components/ChatThread.tsx`, **247 lines, at the 250 cap**; it is also
  the incident follow-up thread, do not change it) over `ChatComposer` (`ChatComposer.tsx`, controlled `value` and
  `onChange`; `MAX_QUESTION_CHARS = 1000`) and `ChatToolTags`. `useChatOff` (`features/chat/useChatOff.ts`) explains
  "Triage is off". Wiring: `web/src/app/nav.ts:59` (`{ to: "/chat", label: "Chat", icon: "chat", preload }`),
  `web/src/app/pages.ts:29` (`chat: page(...)`), `web/src/app/routes.tsx:70` (`<Route path="/chat" ...>`). Tests:
  `web/src/features/chat/chat.test.tsx`.
- **Hub tests (existing).** `crates/domovoi-hub/tests/hub/api_chat.rs` (the chat limits, overview, refusals; harness
  `api_with_triage()` / `api_with_triage_settings(..)` in `tests/hub/api_harness.rs:246-252`, `Api.post/get/admin`).
  `crates/domovoi-hub/tests/gates.rs:614` lists `(Method::POST, "/api/v1/chat")` in the "writes need an admin session
  and CSRF" test; CSRF is enforced for every unsafe method by middleware.

### 3.2 The unit of work and the journal (as of 08, extended by 26 and 28)

- `AggregateKind { Incident, Action, Note }` with `as_str`/`parse` and `AggregateRef` constructors
  (`crates/domovoi-core/src/ports/uow/transaction.rs:15-83`); prompt 26 adds `Operation`; prompt 34 adds `Fact`,
  `Relation`, `Entity` if merged. **Add `Conversation` beside them.**
- `StateWrite` (`transaction.rs:164`), `Transaction { writes, journal, audit, new_effects, .. }` (`:209`),
  `Expected { New, Version(n) }`, `Expectation::{New, Version, Status(String)}`, `Observed::{Missing, Version,
  Status(String)}`, `CommitError::{Conflict, Constraint, StaleEffect, Backend}`, `CommitReceipt.versions`.
- `JournalKind` closed enum with `as_str`/`parse` (`ports/uow/journal.rs:15-45`), `JournalDraft`, `JournalEntry`,
  `AuditDraft::new(actor, at, kind, subject, detail)`.
- SQLite `commit` (`crates/domovoi-store/src/uow/mod.rs:38-87`): one `BEGIN IMMEDIATE`, then `writes::apply` per write
  (`uow/writes.rs:106-140`, an exhaustive `match`), journal rows, audit rows, effects. In-memory twin:
  `crates/domovoi-testkit/src/repos/uow_apply.rs`; crash-harness write summary: `crates/domovoi-testkit/src/crash/mod.rs`.
  Prompt 26 lists the same three files as the places to update for a new `StateWrite` variant.
- `Actor` (`crates/domovoi-core/src/actor.rs`): `Actor::User(UserId)`, `Actor::System(String)`; constants
  `SYSTEM_INCIDENTS`, `SYSTEM_ACTIONS`, `SYSTEM_AUDIT` (`:9-13`); prompt 26 adds `SYSTEM_OPERATIONS`. A system name is
  1 to 64 chars of `[a-z0-9._-]`.
- IDs: `define_id!` in `crates/domovoi-core/src/id.rs` (UUIDv7, `Display`, `FromStr`, `from_uuid`, `as_uuid`). `RequestId`
  already exists and means a hub-to-agent request (`id.rs:~113`): **do not reuse it**; this prompt adds `TurnId`.
- Pagination types: `Page { cursor, limit }`, `Paged<T> { items, next_cursor }`
  (`crates/domovoi-core/src/ports/repos/mod.rs:50-84`).
- Migrations directory `crates/domovoi-store/migrations/` ends at `0011_plan_binding.sql` plus whatever 26, 28, 34 and
  42 added. **List it and take the next free number. Never hard-code one.**

### 3.3 What prompt 28 gives you (introduced by 28, verify names in code)

- `GET /api/v1/events?topics=&after=`; frames `ready`, `journal` (SSE `id` = journal `seq`), `reset`; a `: keepalive`
  comment every 15 s from the `Clock`. Reserved: "events whose name starts with `live.` are **ephemeral**: no `id`,
  never replayed, never emitted by [28]. Prompt 36 will define `live.*` names. A client ignores `live.*` names it does
  not know." That sentence is the contract this prompt fulfils.
- `Topic::Conversation(String)` is accepted by the parser (`conversations:<uuid>`) and **delivers nothing** until this
  prompt adds the arm. `topics_of(entry: &JournalEntry) -> Vec<Topic>` is in
  `crates/domovoi-core/src/ports/uow/topic.rs` (**28, new**); its `match entry.aggregate.kind` ends with a deliberate
  `_ => vec![]` arm ("add an arm here when a consumer needs it"). 28's unit test `reserved_topics_match_nothing`
  asserts that a set of `hosts` and `conversations:<uuid>` matches none of the sample entries: **this prompt changes it
  deliberately** (section 12.1).
- The stream is a pull-based state machine in `crates/domovoi-hub/src/api/events/stream.rs` (**28, new**) built with
  `futures::stream::unfold` over a private state struct (cursor, pending batch, `last_send`, the `JournalFeed`, the
  `Clock`, the `TopicSet`, the redactor, a `StreamGuard`); it waits with `tokio::select!` over `feed.wait_after(cursor)`
  and `clock.sleep_until(last_send + KEEP_ALIVE)`. Frame builders live in `events/wire.rs` (one function per frame
  returning `axum::response::sse::Event`). `MAX_EVENT_STREAMS = 32`. "No `broadcast` channel sits between the journal
  and a client" is a rule for **journal** frames; the live branch added here is a separate, lossy path (section 6.5).
- `snapshot_seq(&ApiState) -> Result<u64, ApiError>` (`crates/domovoi-hub/src/api/snapshot.rs`, **28, new**): read it
  **before** loading a snapshot and return it as `as_of_seq`.
- Web: `useJournalStream({ topics, enabled?, onFrame, onReset? }) -> Live` in `web/src/api/journalStream.ts`,
  `parseJournalFrame`, `applyJournal`, `refetchLive` in `journalFrames.ts`, query keys `keys.operations` and
  `keys.operation(id)` in `queryClient.ts`. One `EventSource` per hook call, reconnecting with `?after=<lastId>`.
- `ApiDeps.bus` is gone and `ApiDeps.journal: Arc<dyn JournalFeed>` exists.

### 3.4 What prompt 26 gives you (introduced by 26, verify names in code)

- `OperationId`, `AggregateRef::operation(id)`, `OperationRepo::get(OperationId) -> Result<Option<Versioned<Operation>>,
  StoreError>`, `OperationRepo::load(id) -> OperationSnapshot`, and `Operation { kind, goal, pinned, status, .. }`.
  Use `get` only. The Operator never calls `OperationHandle`.

### 3.5 Retention and purge (merged, prompt 57)

- `crates/domovoi-store/src/purge.rs` (194 lines): `PURGED_TABLES` (`:14`), `FTS_CLEANUP` (`:28`, empty today),
  `RetentionPolicy { signals, timeline, conversations, notifications, triage, resolved_advisories }` (`:32`; its
  `conversations` field is "Follow-up questions, answers and their clear markers"), `PurgeReport` (`:67`), the `DELETE_*`
  constants (`:77-91`), `keep_for` (`:98`) and `Store::purge(now, &policy)` (`:114`) running every statement in **one**
  transaction on `self.write`. Config key `retention.conversations_days` (default 180, `applies: Now`) is in
  `crates/domovoi-config/src/hub/retention.rs` (doc comment at `:18`: "Days a follow-up question and answer on an incident
  are kept"), mapped in `crates/domovoi-hub/src/runtime/settings.rs:119`. The generated reference line is
  `docs/book/src/reference/configuration.md:212`. A guard test asserts that a purged table with a full-text index is listed
  in `FTS_CLEANUP` (`crates/domovoi-store/tests/sqlite/purge.rs`).

### 3.6 The web shell (existing) and the right slot

- `web/src/app/Layout.tsx`: `HostProvider`, `PageTitleProvider`, `Sidebar`, `TopBar`, `<main>`, `useLiveEvents()` (the
  shell stream, prompt 28 turns it into the journal stream over `incidents,approvals,operations`). `TopBar.tsx` has
  `TopBarActions` (a portal into the bar) and the host selector. `web/src/app/useMediaQuery.ts` has `WIDE_QUERY =
  "(min-width: 768px)"`. `HostContext.tsx`: `useSelectedHost()`.
- `web/DESIGN.md` ("Planned" section) describes **one right-hand slot shared by the Operator panel and the inspector**.
  That slot manager is prompt 30's (U1) and **must be merged before this prompt**: `web/src/app/rightSlot/`
  (`RightSlotProvider`, `RightSlotHost`, `RightSlotSurface`, `useRightSlot`, `useSlotModal`) and the kit chrome
  `SidePanel` (verify names in code). Register the Operator as `<RightSlotSurface id="operator" …>` and use 30's
  `SidePanel` chrome. Build no dock, no `OperatorProvider` layout column, no `--operator-panel-width` token and no second
  `SidePanel`. If `web/src/app/rightSlot/` does not exist, stop and report: the order is wrong.
- Kit and rules (read `web/DESIGN.md` before touching `web/`): pages import only from `web/src/components/index.ts`;
  statuses only through `web/src/design/tone.ts` (vocabularies in `design/*Vocab.ts`, added to `VOCABULARIES`); error
  words only through `web/src/design/messages.ts` (`describeError`); icons only through `Icon`; no inline `style`, no
  native `title`, no raw `<button>` in `src/features` and `src/app`; every mutation through `useActionFeedback`; source
  files at most 250 lines; `/dev/design` demos in `web/src/dev/` for every new kit component; every page test runs axe.

## 4. Exact scope

Build, in dependency order.

### 4.1 `domovoi-core` (no I/O)

1. IDs `ConversationId`, `MessageId`, `TurnId`; actor constant `SYSTEM_OPERATOR = "operator"`.
2. Module `conversation/`: `Conversation`, `ConversationScope`, `Message`, `MessageRole`, `MessageStatus`,
   `MessageFailure`, `MessageTool`, size constants.
3. Ports: `ConversationRepo` (reads), `ConversationWrite` + `MessageGuard` + `StateWrite::Conversation`,
   `AggregateKind::Conversation` + `AggregateRef::conversation`, four `JournalKind` variants, the `topics_of` arm and
   `TopicSet::contains_conversation`, `AnswerSink` and `LlmProvider::complete_streaming`, `LivePublisher` and `LiveEvent`.

### 4.2 `domovoi-llm`

4. Forward text deltas: `anthropic_stream::assemble` and `openai_stream::assemble` take an optional sink; `Anthropic`,
   `OpenAiCompatible` and `Deadline` implement `complete_streaming`. The CLI provider keeps the default.

### 4.3 `domovoi-store`

5. Migration `NNNN_conversations.sql`; `ConversationRepo for Store`; `uow/conversations.rs` (apply
   `ConversationWrite`); one new arm in `uow/writes.rs`; purge of messages and empty conversations.

### 4.4 `domovoi-testkit`

6. In-memory `ConversationRepo` and apply; `contracts/conversations.rs`; `RecordingLive`; `ScriptedLlm::push_streamed`;
   crash-harness summary strings.

### 4.5 `domovoi-engine`

7. `triage`: `AnswerSink` plumbing through `RunBudget`, `complete`, `converse`; `chat::answer_with` returning the text
   and the tools used; `TriageHandle::chat_streaming`; `ChatAnswer`.
8. New module `conversations/`: the writer, the turn runner, the live sink, `ChatModel`, `ConversationHandle`, recovery.

### 4.6 `domovoi-hub`

9. `api/operator/` endpoints (create, list, detail, send, cancel, retry, attach, delete) and the context builder moved
   out of `api/chat.rs`; delete `api/chat.rs`; the `LiveBus` (`LivePublisher` impl) and its branch in the event stream;
   `ApiDeps` fields; runtime wiring; OpenAPI regeneration.

### 4.7 `domovoi-config`, docs

10. Doc-comment update of `retention.conversations_days`; regenerate the configuration reference.

### 4.8 `web/`

11. API hooks, live reducer, the `OperatorThread` kit component, the Operator panel and workspace, the
    `turn status` vocabulary, `describeTurnFailure`, nav and routes, removal of the old chat feature, tests, `/dev/design`
    demos, `DESIGN.md` entries.

Files expected to change or be added (**(new)** marks new files; line caps: Rust non-test 500, web 250):

| File | Change |
|---|---|
| `crates/domovoi-core/src/id.rs` | `ConversationId`, `MessageId`, `TurnId` via `define_id!` |
| `crates/domovoi-core/src/actor.rs` | `pub const SYSTEM_OPERATOR: &str = "operator";` |
| `crates/domovoi-core/src/conversation/{mod,model,message}.rs` **(new)** | types and constants of 6.1 |
| `crates/domovoi-core/src/ports/uow/conversations.rs` **(new)** | `ConversationWrite`, `MessageGuard` |
| `crates/domovoi-core/src/ports/uow/transaction.rs`, `journal.rs`, `topic.rs`, `mod.rs` | variants, kinds, topic arm, `contains_conversation`, re-exports |
| `crates/domovoi-core/src/ports/repos/conversations.rs` **(new)**, `repos/mod.rs` | `ConversationRepo`, `ConversationFilter` |
| `crates/domovoi-core/src/ports/llm.rs` | `AnswerSink`, `complete_streaming` default |
| `crates/domovoi-core/src/ports/live.rs` **(new)**, `ports/mod.rs`, `lib.rs` | `LivePublisher`, `LiveEvent`, `LiveEnd`, `ToolPhase` |
| `crates/domovoi-llm/src/{anthropic,anthropic_stream,openai,openai_stream,deadline}.rs` | sink plumbing |
| `crates/domovoi-store/migrations/NNNN_conversations.sql` **(new)** | section 8 |
| `crates/domovoi-store/src/repos/conversations.rs`, `conversations_rows.rs` **(new)**, `repos/mod.rs` | `ConversationRepo for Store` |
| `crates/domovoi-store/src/uow/conversations.rs` **(new)**, `uow/writes.rs`, `uow/mod.rs` | apply the write |
| `crates/domovoi-store/src/purge.rs` (and `purge_operator.rs` **(new)** if it would pass 500 lines) | messages and empty conversations |
| `crates/domovoi-testkit/src/repos/{conversations,uow_conversations}.rs` **(new)**, `repos/{mod,uow,uow_apply}.rs`, `crash/mod.rs` | in-memory twin |
| `crates/domovoi-testkit/src/contracts/conversations.rs` **(new)**, `contracts/mod.rs` | contract suite |
| `crates/domovoi-testkit/src/{live.rs (new),llm.rs}`, `lib.rs` | `RecordingLive`, `push_streamed` |
| `crates/domovoi-engine/src/triage/{agent,complete,chat,mod}.rs` | sink plumbing, `chat_streaming` |
| `crates/domovoi-engine/src/conversations/{mod,deps,model,writer,commit,turn,sink,recover,journal}.rs` **(new)** | section 6.4 |
| `crates/domovoi-engine/src/lib.rs`, `Cargo.toml` | `pub mod conversations;`, `[[test]] name = "conversations"` |
| `crates/domovoi-engine/tests/conversations/{main,harness,writer,turns,live,restart,gates}.rs` **(new)** | section 12.4 |
| `crates/domovoi-hub/src/api/operator/{mod,dto,create,messages,context,live}.rs` **(new)**, `api/mod.rs` | endpoints |
| `crates/domovoi-hub/src/api/chat.rs` | **delete** |
| `crates/domovoi-hub/src/api/events/{stream,wire}.rs` (28) | live branch and `live_event` frame |
| `crates/domovoi-hub/src/runtime/{mod,settings}.rs`, `examples/dev_hub.rs`, `tests/hub/api_harness.rs` | wire the service, `ApiDeps` constructors |
| `crates/domovoi-hub/tests/hub/{api_chat.rs (delete), api_operator.rs, api_operator_live.rs}` **(new)**, `tests/hub/main.rs`, `tests/gates.rs` | tests |
| `crates/domovoi-config/src/hub/retention.rs` | doc comment of `conversations_days` |
| `web/src/api/hooks/{chat.ts (delete), conversations.ts (new)}`, `web/src/api/{operatorLive.ts, journalStream.ts, queryClient.ts}` | hooks, live reducer, `onLive`, keys |
| `web/src/components/{OperatorThread,OperatorTurns}.tsx` **(new)**, CSS modules, `index.ts` | kit (`SidePanel` comes from prompt 30; do not add another) |
| `web/src/features/operator/*` **(new)**, `web/src/features/chat/*` **(delete)** | the Operator |
| `web/src/app/{Layout,TopBar,nav,pages,routes,commandPalette}.ts(x)`, `Layout.module.css` | top-bar button, routes, palette; the surface registers with 30's right slot |
| `web/src/design/{status,tone,messages}.ts`, `web/src/design/operatorVocab.ts` **(new)** | turn status vocabulary, `describeTurnFailure` |
| `web/src/dev/*Demos.tsx`, `web/DESIGN.md`, `web/src/styles/tokens.css` | demos, docs, width token |
| `web/src/api/openapi.json`, `web/src/api/schema.d.ts` | **regenerated, never hand-edited** |
| `docs/book/src/concepts/{how-it-works,operator (new)}.md`, `SUMMARY.md`, `reference/{api,data-model,configuration}.md`, `guide/retention.md`, `ARCHITECTURE.md` | docs |

## 5. Explicit non-goals

- **No meta-tools, no proposing, starting or cancelling operations, no step approval.** Prompt 31 owns meta-tools; 27
  owns step approval. The Operator reads only. Attaching an operation shows the model a read-only summary.
- **No layered context, no experience search, no knowledge reads** (prompts 32, 35, 34). The model input stays what
  `triage::chat` builds today.
- **No change to the incident follow-up thread** (`POST /api/v1/incidents/{id}/ask`, `triage.followup` timeline
  entries, `ChatThread` and its tests). It keeps working unchanged.
- **No change to `TriageHandle::chat`** and the existing engine chat tests; `chat_streaming` shares the same function.
- **No wire protocol change** (RW-42), no new agent message, no new config key, no change to the effect outbox (model
  turns are **not** effects: section 10 says why).
- **No journal pruning and no change to prompt 28's topic grammar.** Only the `Conversation` arm of `topics_of` and
  `TopicSet::contains_conversation` are added to 28's files.
- **No rename, edit or per-message delete of conversations**, no sharing, no export, no full-text search of messages,
  no per-conversation model choice, no attachments, no voice.
- **No attention centre, no autonomy indicator, no `useVisibility`** (prompts 51, 37). No profile categories yet.
- **Do not hand-edit** `web/src/api/openapi.json` or `schema.d.ts`; do not change `ChatThread.tsx`.
- **Do not touch** `crates/domovoi-runner`, `crates/domovoi-agent`, `crates/domovoi-proto`, `xtask/src/arch/rules.rs`
  (the no-executor rule is a gate test, section 12.4, so no ADR is needed; if you conclude `rules.rs` must change, stop and report).

## 6. Required interfaces and contracts

Each item is marked **fixed** (must match) or **convention** (follow repository style).

### 6.1 Core types **fixed** (names, fields, wire strings)

```rust
// crates/domovoi-core/src/id.rs
define_id!(/** Identifies an Operator conversation. */ ConversationId);
define_id!(/** Identifies one message of a conversation. */ MessageId);
define_id!(/** The client-chosen id of one question and its answer: the request id that makes a send idempotent and lets
              a stale reply be recognised. */ TurnId);

// crates/domovoi-core/src/conversation/*.rs
pub const MAX_QUESTION_CHARS: usize = 1_000;      // same as the old chat limit
pub const MAX_MESSAGES: usize = 200;              // messages kept in one conversation (100 exchanges)
pub const MAX_TITLE_CHARS: usize = 80;
pub const MAX_FAILURE_DETAIL_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConversationScope { Global, Host(HostId) }     // no `Default`: scope is always chosen

#[derive(Debug, Clone, PartialEq)]
pub struct Conversation {
    pub id: ConversationId,
    pub title: String,                 // "" until the first question; then its first line, redacted, <= MAX_TITLE_CHARS chars
    pub scope: ConversationScope,
    pub operation: Option<OperationId>,   // OperationId: introduced by prompt 26
    pub last_ordinal: u32,             // highest ordinal ever assigned; never decreases (purge deletes rows, not the counter)
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum MessageRole { User, Assistant }       // "user" | "assistant"
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum MessageStatus { Pending, Complete, Failed, Cancelled }
//   as_str/parse: "pending" | "complete" | "failed" | "cancelled". A user message is always Complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageFailure { Busy, BudgetSpent, ModelUnavailable, TriageOff, Interrupted, Cancelled, Internal }
//   as_str/parse: "busy" | "budget_spent" | "model_unavailable" | "triage_off" | "interrupted" | "cancelled" | "internal"

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTool { pub tool: String, pub failed: bool }   // a read tool name from the closed set, or "unknown tool"

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: MessageId,
    pub conversation: ConversationId,
    pub ordinal: u32,                  // 1-based, unique per conversation, increasing
    pub role: MessageRole,
    pub status: MessageStatus,
    pub turn: TurnId,                  // a user message and the assistant message answering it share the turn at creation
    pub text: String,                  // redacted; "" while pending, failed or cancelled
    pub failure: Option<MessageFailure>,       // Some only when status is Failed or Cancelled
    pub failure_detail: Option<String>,        // redacted, <= MAX_FAILURE_DETAIL_CHARS chars
    pub tools: Vec<MessageTool>,       // assistant messages only
    pub created_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}
```

Every enum has `as_str`/`parse` and a round-trip unit test. Validation of unknown stored strings happens in Rust when a row
is decoded (a corrupt value is a `StoreError::Backend("corrupt ...")`), never in SQL.

### 6.2 Ports **fixed**

```rust
// crates/domovoi-core/src/ports/uow/conversations.rs
/// A write to the conversation aggregate. Built only by the conversation writer.
pub enum ConversationWrite {
    /// Inserts (`Expected::New`) or replaces (`Expected::Version(n)`, the store bumps `version`) a conversation row.
    Conversation { conversation: Conversation, expected: Expected },
    /// Inserts (`MessageGuard::New`) or replaces a message whose stored status is as guarded.
    Message { message: Message, guard: MessageGuard },
    /// Deletes a conversation and, by cascade, its messages. `expected` is the version the writer read.
    Delete { id: ConversationId, expected: u64 },
}
pub enum MessageGuard { New, Status(MessageStatus) }   // Status(s): replace only if the stored status is s
```
`StateWrite::Conversation(ConversationWrite)` is the single new `StateWrite` variant. `AggregateKind::Conversation`
(`as_str` `"conversation"`) and `AggregateRef::conversation(ConversationId)` are added beside the others. A failed
message guard is `CommitError::Conflict { aggregate: AggregateRef::conversation(id), expected: Expectation::Status(..),
found: Observed::Status(stored) | Observed::Missing }`. `CommitReceipt::versions` reports `(AggregateRef::conversation(id),
new_version)` for every conversation write.

```rust
// crates/domovoi-core/src/ports/repos/conversations.rs
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversationFilter { pub host: Option<HostId>, pub operation: Option<OperationId> }

#[async_trait]
pub trait ConversationRepo: Send + Sync {
    async fn get(&self, id: ConversationId) -> Result<Option<Versioned<Conversation>>, StoreError>;
    /// Newest `updated_at` first (ties by id); `Paged::items` carry `message_count` through `ConversationRow`.
    async fn list(&self, filter: &ConversationFilter, page: &Page) -> Result<Paged<ConversationRow>, StoreError>;
    /// All messages of one conversation, ascending `ordinal`.
    async fn messages(&self, id: ConversationId) -> Result<Vec<Message>, StoreError>;
    async fn message(&self, id: MessageId) -> Result<Option<Message>, StoreError>;
    /// The message of `role` created for `turn` in `conversation`, if any.
    async fn by_turn(&self, conversation: ConversationId, turn: TurnId, role: MessageRole)
        -> Result<Option<Message>, StoreError>;
    /// Up to `limit` Complete messages with `ordinal < before`, oldest first (the model's history).
    async fn recent_complete(&self, conversation: ConversationId, before: u32, limit: u16)
        -> Result<Vec<Message>, StoreError>;
    /// Every Pending assistant message (startup recovery).
    async fn pending(&self) -> Result<Vec<Message>, StoreError>;
}
pub struct ConversationRow { pub conversation: Conversation, pub version: u64, pub message_count: u32 }
```
The port has **no write method**: every write is a `ConversationWrite` in a `Transaction`.

```rust
// crates/domovoi-core/src/ports/uow/journal.rs : four new JournalKind variants (as_str / parse / round-trip test extended)
ConversationCreated  => "conversation.created"
ConversationMessage  => "conversation.message"
ConversationUpdated  => "conversation.updated"
ConversationDeleted  => "conversation.deleted"
```

```rust
// crates/domovoi-core/src/ports/llm.rs  (additions)
/// Receives what a model turn produces while it runs. Used by the Operator to stream an answer; everything else passes none.
pub trait AnswerSink: Send + Sync {
    /// A new model request starts: anything pushed before belonged to an attempt that was discarded.
    fn attempt_started(&self);
    /// A fragment of the text the model is writing in this attempt. Not redacted; the sink does that.
    fn text(&self, fragment: &str);
    /// The engine starts or finishes one read tool; `tool` is a name from the closed set or `unknown tool`.
    fn tool(&self, tool: &str, phase: ToolPhase, failed: bool);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ToolPhase { Started, Finished }

// on trait LlmProvider (default body, so no existing implementation changes):
async fn complete_streaming(&self, req: LlmRequest, sink: &dyn AnswerSink) -> Result<LlmResponse, LlmError> {
    let response = self.complete(req).await?;
    for part in &response.parts { if let Part::Text(text) = part { sink.text(text); } }
    Ok(response)
}
```
The default emits the whole text once, after the turn: a provider that cannot stream (the CLI provider, every test fake
that does not override it) still works, it just "streams" one fragment.

```rust
// crates/domovoi-core/src/ports/live.rs
/// Publishes ephemeral live events to whoever watches a conversation. Never blocks, never fails, never stores.
pub trait LivePublisher: Send + Sync { fn publish(&self, conversation: ConversationId, event: LiveEvent); }
#[derive(Debug, Clone, PartialEq)]
pub enum LiveEvent {
    Start { turn: TurnId, message: MessageId },
    Text  { turn: TurnId, attempt: u32, text: String },              // the whole safe text of the attempt so far (a snapshot)
    Tool  { turn: TurnId, tool: String, phase: ToolPhase, failed: bool },
    End   { turn: TurnId, outcome: LiveEnd },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum LiveEnd { Complete, Failed, Cancelled, Discarded }
```

### 6.3 Topic mapping **fixed**

In `topics_of` (prompt 28's file) add before the wildcard arm:

| Aggregate | Kind | Topics |
|---|---|---|
| `Conversation` | any (`conversation.created`, `.message`, `.updated`, `.deleted`) | `Topic::Conversation(<aggregate id>)` (when the id is not a UUID: `[]`) |

and `TopicSet::contains_conversation(&self, id: &str) -> bool` (true when the set holds `Topic::Conversation(id)`). The
`conversations:<uuid>` topic stops being "reserved": update 28's doc comment and its test (section 12.1). `hosts` stays
reserved.

### 6.4 Journal payloads and audit **fixed**

All `payload_v` 1. **No payload and no audit detail contains message text, a title or a failure detail.**

| Kind | Aggregate | Actor | Payload |
|---|---|---|---|
| `conversation.created` | conversation | `User` | `{"conversation": "<id>", "scope": "global" \| "host", "host": "<host id>"?, "operation": "<id>"?}` |
| `conversation.message` | conversation | `User` for the user message; `System("operator")` for every assistant message transition | `{"conversation": "<id>", "message": "<id>", "ordinal": <n>, "role": "user" \| "assistant", "status": "pending" \| "complete" \| "failed" \| "cancelled", "turn": "<id>", "failure"?: "<code>"}` |
| `conversation.updated` | conversation | `User` | `{"conversation": "<id>", "change": "operation_attached" \| "operation_detached", "operation"?: "<id>"}` |
| `conversation.deleted` | conversation | `User` | `{"conversation": "<id>"}` |

Audit (same transaction): only `conversation.deleted` (subject `conversation:<id>`, actor the person, detail
`{"messages": <count before deleting>}`), because it destroys data. Questions and answers are not security events and are
not audited (the journal is their record).

### 6.5 Live events on the SSE stream **fixed**

These four event names are the contract prompt 28 reserved. Each is an SSE frame with **an `event:` name and a `data:` JSON
line and no `id:` line**. They are never replayed, never stored, and a client that connects late gets only what follows.
A connection receives live events for a conversation only if its topic set contains `conversations:<that uuid>`.

```
event: live.start
data: {"conversation":"<uuid>","turn":"<uuid>","message":"<assistant message uuid>"}

event: live.text
data: {"conversation":"<uuid>","turn":"<uuid>","attempt":2,"text":"The journal shows two restarts of nginx at "}

event: live.tool
data: {"conversation":"<uuid>","turn":"<uuid>","tool":"journal","state":"started","failed":false}

event: live.end
data: {"conversation":"<uuid>","turn":"<uuid>","status":"complete"}
```

- `live.start`: sent when the model call begins (after admission passed; a refused turn sends only `live.end` with status
  `failed`).
- `live.text` is a **snapshot**, not a delta: `text` is the whole safe text of the current attempt so far. Snapshots tolerate a
  dropped frame (the next one repairs it). `attempt` starts at 1 and **increases** whenever the model starts a new request (a
  tool round, a retry after `MaxTokens`, a provider fallback, a capability retry) or when redaction rewrote text that was already
  sent. A client replaces its live text when it sees a higher `attempt` and ignores a lower one. A new attempt is announced at
  once with `text: ""`. Frames are throttled to at most one per `LIVE_MIN_INTERVAL = 100 ms` of `Clock` time, except the
  attempt-start frame, which is never throttled. `text` is cut to `MAX_ANSWER_CHARS` (4000).
- `live.tool`: `state` is `started` or `finished`; `tool` is a name from the closed `ReadTool` set or `unknown tool`, never
  model-supplied text.
- `live.end`: `status` is `complete`, `failed`, `cancelled` or `discarded` (a result that was not committed because the turn was
  no longer pending). Sent **after** the commit that settled the turn (so the committed `journal` frame normally arrives first).
  Clients treat both the journal frame and `live.end` as "refetch the thread" and are idempotent.
- Delivery is lossy by design: the hub-side `LiveBus` is a `tokio::sync::broadcast` channel of capacity 256 per hub; a client
  that cannot keep up misses frames (counted in `domovoi_sse_live_dropped_total`) and is repaired by the next snapshot or by the
  committed message. The publisher never blocks.

### 6.6 HTTP API **fixed** (paths, methods, status codes, JSON shapes)

All routes need a session (`Authed`); unsafe methods are CSRF-protected by the existing middleware. Tag `operator`
(add it to the `tags(...)` list of `ApiDoc`). `POST /api/v1/chat` is **removed** together with `ChatRequest`, `ChatResponse`,
`ChatTurnDto`.

| Method and path | `operation_id` | Request | Success | Errors |
|---|---|---|---|---|
| `GET /api/v1/conversations` | `list_conversations` | query `cursor`, `limit`, `host` (a host name), `operation` (a uuid) | 200 `ConversationPage` | 400 bad filter; 404 unknown host name |
| `POST /api/v1/conversations` | `create_conversation` | `CreateConversationRequest` | 201 `ConversationDetailDto` | 400 scope missing or malformed; 404 unknown host or operation |
| `GET /api/v1/conversations/{id}` | `get_conversation` | - | 200 `ConversationDetailDto` | 404 |
| `POST /api/v1/conversations/{id}/messages` | `send_message` | `SendMessageRequest` | 202 `SendResponse` (new turn) or 200 `SendResponse` (same `turn` sent again) | 400 empty or too long, bad `turn`; 404; 409 a turn is pending, or the conversation has `MAX_MESSAGES`; 503 `triage_off` |
| `POST /api/v1/conversations/{id}/turns/{turn}/cancel` | `cancel_turn` | - | 200 `CancelResponse` | 404 |
| `POST /api/v1/conversations/{id}/messages/{message}/retry` | `retry_message` | - | 202 `SendResponse` | 404; 409 not the last assistant message, or its status is not `failed` or `cancelled`, or a turn is pending; 503 `triage_off` |
| `PUT /api/v1/conversations/{id}/operation` | `set_conversation_operation` | `{"operation_id": "<uuid>" \| null}` | 200 `ConversationSummaryDto` | 400; 404 unknown conversation or operation |
| `DELETE /api/v1/conversations/{id}` | `delete_conversation` | - | 204 | 404; 409 a turn is pending ("stop it first") |

```jsonc
// CreateConversationRequest. `scope` is required: omitting it is a 400 (no implicit "all hosts").
{ "scope": { "kind": "global" }  |  { "kind": "host", "host": "nas" },  "operation_id": "<uuid>"? }

// SendMessageRequest. `turn` is a client-generated uuid (the request id); `text` is trimmed, 1..=1000 characters.
{ "turn": "0190a0a0-0000-7000-8000-0000000000c1", "text": "Why is the NAS slow?" }

// MessageDto
{ "id": "<uuid>", "ordinal": 4, "role": "assistant", "status": "complete", "turn": "<uuid>", "text": "...",
  "failure": null, "failure_detail": null, "tools": [ { "tool": "journal", "failed": false } ],
  "created_at": "2026-11-18T10:00:00Z", "completed_at": "2026-11-18T10:00:07Z" }

// ConversationSummaryDto
{ "id": "<uuid>", "title": "Why is the NAS slow?",
  "scope": { "kind": "host", "host_id": "<uuid>", "host_name": "nas" },     // host_name null when the host is gone
  "operation_id": "<uuid>" | null, "message_count": 4, "created_at": "...", "updated_at": "...", "version": 7 }

// ConversationDetailDto = ConversationSummaryDto fields plus:
{ "messages": [ MessageDto, ... ],                       // ascending ordinal, all kept messages
  "turn_in_progress": { "turn": "<uuid>", "message": "<uuid>" } | null,
  "as_of_seq": 4821 }

// ConversationPage
{ "items": [ ConversationSummaryDto ], "next_cursor": "..." | null, "as_of_seq": 4821 }

// SendResponse: the committed pair. `user` is complete; `assistant` is pending (a replayed send returns its current state).
{ "user": MessageDto, "assistant": MessageDto, "as_of_seq": 4821 }

// CancelResponse: the assistant message's status after the call; `cancelled` only when this call stopped it.
{ "status": "cancelled" | "complete" | "failed" }
```

Rules (**fixed**):

- `as_of_seq` is `snapshot_seq(&state)` read **before** the data (prompt 28 rule) on `list_conversations` and
  `get_conversation`; `send_message` and `retry_message` return the journal head after their commit (a fresh `snapshot_seq`).
- `send_message` and `retry_message` answer **503 `triage_off`** (existing `ApiError::TriageOff`) when `ApiDeps.triage` is
  `None`; reading, creating, attaching and deleting work with triage off. A spent budget or a busy model is **not** an HTTP
  error here: it is a stored failed answer (6.7). The existing 429 path (`busy(..)`) is no longer used by conversations.
- The server builds the model's history from stored messages. **A client can no longer send history.**
- `host` on create and list is a host **name**, resolved with `host_named` (404 when unknown); stored as the `HostId`.
- An unknown or malformed `turn`, `message` or `{id}` uuid is a 400 (`turn`, `message` in a body or path) or a 404 (`{id}` of
  a conversation), as `parse` fails: follow the neighbouring handlers.
- Errors keep the existing codes (`bad_request`, `not_found`, `conflict`, `triage_off`, `unavailable`, `internal`); **add no
  new `ApiError` variant** (so the web `HUB_CODES` list is unchanged). Conflict messages are the specific text the web shows.

### 6.7 Turn semantics **fixed**

State of the assistant message (the user message is `Complete` from birth):

| From | Event | To | `failure` | Journal row (`conversation.message`, assistant, actor `System("operator")`) |
|---|---|---|---|---|
| (new) | question committed | `Pending` | - | yes, `status: "pending"` (the user message gets its own row in the same commit) |
| `Pending` | model answer arrives and the guard holds | `Complete` | - | yes |
| `Pending` | model error: budget spent | `Failed` | `budget_spent` | yes |
| `Pending` | model error: hourly limit or another answer running | `Failed` | `busy` | yes |
| `Pending` | model error: every provider failed, or the admission check failed | `Failed` | `model_unavailable` | yes |
| `Pending` | triage became unavailable | `Failed` | `triage_off` | yes |
| `Pending` | store or task failure | `Failed` | `internal` | yes |
| `Pending` | `cancel` | `Cancelled` | `cancelled` | yes |
| `Pending` | hub restarted while it ran | `Failed` | `interrupted` | yes |
| `Failed` or `Cancelled` (the last assistant message) | `retry` | `Pending` with a **new** `turn` | cleared | yes |
| any not `Pending` | a late model result arrives | unchanged (**discarded**) | - | none; `live.end` with `discarded` |

`failure_detail` is the redacted `TriageError` text cut to `MAX_FAILURE_DETAIL_CHARS` (for `busy` it is the existing sentence
with the retry time). Cancelled and interrupted messages keep `text = ""`.

**Idempotent send.** A send whose `turn` equals the `turn` of an existing user message of that conversation changes nothing
and returns the stored user message and its current assistant message with `200`. A send while the conversation has a pending
assistant message (and a different `turn`) is `ConversationError::TurnInProgress` (409). A conversation with `MAX_MESSAGES`
existing messages refuses a send with `ConversationError::Full` (409, "this conversation is full; start a new one").

**Stale discard.** The writer commits a turn's result with `MessageGuard::Status(Pending)` after re-reading the assistant message
and checking that its `turn` still equals the finishing task's `turn`. Anything else means the result is stale: nothing is
written, `live.end{discarded}` is published, and the model's spend (already recorded by the usage code) stands.

### 6.8 The conversation writer (engine) **fixed** (names), body **convention**

```rust
// crates/domovoi-engine/src/conversations/mod.rs
pub struct ConversationDeps {
    pub conversations: Arc<dyn ConversationRepo>,
    pub uow: Arc<dyn UnitOfWork>,
    pub model: Arc<dyn ChatModel>,
    pub live: Arc<dyn LivePublisher>,
    pub redactor: Arc<dyn Redactor>,
    pub clock: Arc<dyn Clock>,
    pub shutdown: CancellationToken,
}
pub struct ConversationService;
impl ConversationService {
    /// Runs startup recovery (6.7: every Pending assistant message becomes Failed/interrupted), then serves commands.
    pub async fn spawn(deps: ConversationDeps) -> Result<(ConversationHandle, JoinHandle<()>), ConversationError>;
}
#[derive(Clone)] pub struct ConversationHandle { /* command channel, turn registry */ }

pub struct NewConversation { pub scope: ConversationScope, pub operation: Option<OperationId>, pub actor: Actor }
pub struct ChatContext { pub overview: String, pub tool_host: Option<HostId> }   // built by the hub per send, see 6.9
pub struct SendMessage { pub conversation: ConversationId, pub turn: TurnId, pub text: String, pub context: ChatContext, pub actor: Actor }
pub struct RetryTurn { pub conversation: ConversationId, pub message: MessageId, pub context: ChatContext, pub actor: Actor }
pub struct TurnStarted { pub user: Message, pub assistant: Message, pub replayed: bool }

impl ConversationHandle {
    pub async fn create(&self, new: NewConversation) -> Result<Conversation, ConversationError>;
    pub async fn send(&self, cmd: SendMessage) -> Result<TurnStarted, ConversationError>;
    pub async fn retry(&self, cmd: RetryTurn) -> Result<TurnStarted, ConversationError>;
    pub async fn cancel(&self, conversation: ConversationId, turn: TurnId, actor: Actor) -> Result<MessageStatus, ConversationError>;
    pub async fn set_operation(&self, conversation: ConversationId, operation: Option<OperationId>, actor: Actor)
        -> Result<Conversation, ConversationError>;
    pub async fn delete(&self, conversation: ConversationId, actor: Actor) -> Result<(), ConversationError>;
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConversationError {
    #[error("no such conversation")] NotFound,
    #[error("an answer is already being written in this conversation")] TurnInProgress,
    #[error("this conversation is full; start a new one")] Full,
    #[error("only the last answer can be retried, and only after it failed or was stopped")] NotRetryable,
    #[error("the question must have 1 to {MAX_QUESTION_CHARS} characters")] BadQuestion,
    #[error("store error: {0}")] Store(String),
    #[error("the conversation service is stopping")] Unavailable,
}
```

```rust
// crates/domovoi-engine/src/conversations/model.rs
pub struct ChatQuestion { pub host: Option<HostId>, pub overview: String, pub history: Vec<ChatTurn>, pub question: String }
pub struct ChatAnswer { pub text: String, pub tools: Vec<ToolUse> }     // ToolUse: existing engine type (triage::ToolUse)
#[async_trait]
pub trait ChatModel: Send + Sync {
    async fn answer(&self, question: ChatQuestion, sink: Arc<dyn AnswerSink>) -> Result<ChatAnswer, TriageError>;
}
impl ChatModel for TriageHandle { /* calls TriageHandle::chat_streaming */ }
pub struct DisabledModel;          // answers TriageError::Provider("triage is off"); used when triage is not running
```

**The writer.** One tokio task with a bounded command channel (64), like the operation writer of prompt 26 and
`actions/writer.rs`. Each command (1) loads the conversation (and what it needs) fresh from `ConversationRepo`, (2) decides in
a pure function (`commit.rs` builds the `Transaction`: conversation row with `Expected::Version`, messages with their
guards, journal drafts per 6.4, audit for delete), (3) commits through `UnitOfWork`, (4) answers the caller. On
`CommitError::Conflict` it reloads and retries **once**; a second conflict is `ConversationError::Store`. A `Constraint`
failure from the unique pending index (two sends racing) is `TurnInProgress`. After a successful `send` or `retry` the writer
**spawns the turn task** (below) and registers `(TurnId -> CancellationToken)` in a map shared with `cancel`.

**The turn task** (`turn.rs`), one per pending assistant message:

```
publish Start{turn, message}
history  = repo.recent_complete(conversation, before = user.ordinal, limit = MAX_HISTORY_TURNS(20))  -> Vec<ChatTurn>  (role, text)
sink     = TurnSink::new(live, conversation, turn, redactor, clock)            // 6.10
result   = select! { _ = token.cancelled() => Cancelled,
                     r = model.answer(ChatQuestion{host: ctx.tool_host, overview: ctx.overview, history, question: user.text}, sink) => r }
command  = Settle { conversation, turn, outcome }                              // sent to the writer, never committed here
writer.settle(): re-read assistant by turn; if missing or status != Pending or turn differs -> publish End{Discarded}, return
                 else commit Message(replace, guard Status(Pending)) + journal + conversation.updated_at/version, then publish End{outcome}
```

`Settle` outcomes: `Answered(ChatAnswer)` (text = the engine's already redacted and clipped answer, tools mapped to
`MessageTool{tool, failed: failed || refused}`), `Failed(MessageFailure, detail)`, `Cancelled`. The mapping from `TriageError`:
`BudgetSpent` -> `BudgetSpent`; `Busy{..}` -> `Busy`; `Provider(_)` and `AdmissionUnavailable` -> `ModelUnavailable`; `Store(_)` ->
`Internal`; `UnknownIncident(_)` -> `Internal`. A panic or join error of the task -> `Internal`.

The turn task is spawned with `tokio::spawn` and is **independent of every HTTP request and every SSE connection**: closing the
panel, the tab or the stream never cancels it (test `closing_panel_does_not_stop_operation`).

### 6.9 Hub **fixed** (names), body **convention**

- `crates/domovoi-hub/src/api/operator/context.rs`: the overview builder moved from `api/chat.rs`:
  `pub(crate) async fn chat_context(state: &ApiState, conversation: &Conversation) -> Result<ChatContext, ApiError>`. It keeps
  the existing behaviour exactly: `ConversationScope::Host(id)` -> look the host up (`HostRepo::get_host` by id; a vanished host
  is treated as `Global` for the overview and the response notes nothing), `tool_host` is `Some(id)` **only** when
  `host_dto(state, &host).connected`, the text is the former `overview(state, Some(&host))` (hosts, active incidents, last-day
  digest facts of the chosen host), `Global` -> `overview(state, None)` and `tool_host: None`. New: when
  `conversation.operation` is `Some`, append a section `Attached operation (read-only summary)` with the operation's goal (cut to
  300 chars), its status name and its pinned subjects as text, read with the operation read dependency (`OperationRepo::get`);
  a missing operation appends nothing. The text is redacted by the engine as today (it redacts the overview). Move
  `fact_lines`, `incident_line` and the overview with their tests from `api/chat.rs`; delete `api/chat.rs`.
- `crates/domovoi-hub/src/api/operator/live.rs`: `pub struct LiveBus { tx: broadcast::Sender<LiveFrame> }` with
  `LiveBus::new() -> Arc<Self>` (capacity `LIVE_CAPACITY = 256`), `subscribe()`, and `impl LivePublisher for LiveBus`
  (serialises a `LiveEvent` to a `LiveFrame { conversation: String, name: &'static str, data: Arc<str> }` with the JSON of 6.5;
  `send` errors (no receivers) are ignored). `ApiDeps` gains `conversations: Arc<dyn ConversationRepo>`,
  `conversation_service: ConversationHandle`, `live: Arc<LiveBus>` (names **convention**, document them in the PR).
- The event stream (prompt 28, `api/events/stream.rs` and `wire.rs`): add `wire::live_event(&LiveFrame) -> Event` (=
  `Event::default().event(name).data(data)`, **no `.id(..)`**) and a live branch in the state machine:
  - at connection time, only when the `TopicSet` contains any `Topic::Conversation(_)`, take `let mut live = state.deps.live.subscribe()`;
  - add `frame = live.recv()` to the `tokio::select!` that already waits on `feed.wait_after(cursor)` and the keepalive timer. On
    `Ok(frame)` with `topics.contains_conversation(&frame.conversation)`: yield `wire::live_event(&frame)` and set `last_send = clock.now()`;
    on `Ok(other)`: ignore; on `Err(RecvError::Lagged(n))`: add `n` to counter `domovoi_sse_live_dropped_total`, continue; on
    `Err(RecvError::Closed)`: stop selecting on it for this connection;
  - the cursor, `ready` and `reset` logic and the journal batch rule do not change; live frames never move the cursor.
- Handlers in `operator/{create,messages,mod}.rs`, DTOs in `operator/dto.rs` (`ToSchema`). `host_name` is looked up by `HostRepo`
  per list page (one query), not per row. Convert `ConversationError` -> `ApiError`: `NotFound` -> `NotFound`, `TurnInProgress` ->
  `Conflict("an answer is already being written; stop it or wait")`, `Full` -> `Conflict(..)`, `NotRetryable` -> `Conflict(..)`,
  `BadQuestion` -> `BadRequest(..)`, `Store(e)` -> `ApiError::internal`, `Unavailable` -> `Unavailable("the hub is stopping")`.
- Runtime (`runtime/mod.rs`, next to `triage_build::build`): create `let live = LiveBus::new();`, spawn
  `ConversationService::spawn(ConversationDeps { conversations: store.clone(), uow: store.clone(), model: triage.0.clone().map_or(Arc::new(DisabledModel) as _, |t| Arc::new(t) as _), live: live.clone(), redactor: redactor.clone(), clock: clock.clone(), shutdown: worker_shutdown.clone() })`, keep its `JoinHandle` like `triage_task` (joined in the existing shutdown sequence), and put the three new fields into `ApiDeps`. Update the other `ApiDeps` constructors 28 lists (`examples/dev_hub.rs`, `tests/hub/api_harness.rs`).

### 6.10 The live sink and redaction **fixed** (rules), body **convention**

`TurnSink` (engine, `conversations/sink.rs`) implements `AnswerSink` and is the only thing that publishes `Text` and `Tool`
events. State (behind a `std::sync::Mutex`): `attempt: u32` (0 before the first request), `raw: String` (all text of this attempt),
`sent: String` (the safe text last published for this attempt), `last_emit: Option<Timestamp>`.

```
attempt_started(): attempt += 1; raw.clear(); sent.clear(); last_emit = None; publish Text{turn, attempt, text: ""}
text(fragment):    raw.push_str(fragment)
                   released = releasable(&raw, redactor)            // pure, below
                   if !released.starts_with(&sent):                  // redaction rewrote text that was already sent
                       attempt += 1; sent.clear(); publish Text{turn, attempt, text: ""}   // the client starts over at once
                   if released.len() == sent.len(): return          // nothing new is safe yet
                   now = clock.now(); if last_emit is Some(t) and now - t < LIVE_MIN_INTERVAL: return   // throttle, no timer
                   sent = released; last_emit = now; publish Text{turn, attempt, text: clip(sent, MAX_ANSWER_CHARS)}
tool(name, phase, failed): publish Tool{turn, tool: name_or_unknown(name), phase, failed}   // name checked against ReadTool::ALL
```

`releasable(raw, redactor) -> String` (**fixed** rules, pure, unit-tested):

1. If `raw` contains `-----BEGIN` with no later `-----END`, drop everything from that last `-----BEGIN` on (an unfinished key block is
   never published).
2. `r = redactor.redact(result of 1)`.
3. Cut `r` after its last whitespace character (so the trailing, possibly half-written word is held back). When `r` has no whitespace
   the result is the empty string.

The final committed text does not come from the sink: it is the engine's `clip(redact(answer))`. The sink never touches storage.

### 6.11 Web contracts **fixed** (names), layout **convention**

```ts
// web/src/api/queryClient.ts : keys
conversations: ["conversations"] as const,
conversationList: (filter: object) => ["conversations", "list", filter] as const,
conversation: (id: string) => ["conversations", "detail", id] as const,

// web/src/api/hooks/conversations.ts  (every mutation is wrapped by useActionFeedback where it is used)
useConversations(filter: { host?: string; operation?: string }): UseQueryResult<ConversationPage>
useConversation(id: string | undefined): UseQueryResult<ConversationDetail>            // disabled while id is undefined
useCreateConversation(), useSendMessage(id), useCancelTurn(id), useRetryMessage(id),
useSetConversationOperation(id), useDeleteConversation()
// A successful send/retry writes the returned pair into keys.conversation(id) with setQueryData (so the question and the
// pending answer show at once) and invalidates keys.conversations. A successful create writes the detail into its key.
```

```ts
// web/src/api/operatorLive.ts  (pure reducer + one hook; at most 250 lines)
export interface LiveTool { tool: string; state: "started" | "finished"; failed: boolean }
export interface LiveState { turn: string | null; attempt: number; text: string; tools: LiveTool[] }
export const NO_LIVE: LiveState;                                   // { turn: null, attempt: 0, text: "", tools: [] }
export interface LiveFrame { name: string; data: unknown }
export function liveReducer(state: LiveState, frame: LiveFrame, activeTurn: string | undefined): LiveState;   // pure
export function useConversationLive(id: string | undefined, activeTurn: string | undefined):
  { live: LiveState; status: Live };                               // Live: the existing type of prompt 28
```

`liveReducer` rules (**fixed**): `live.start` always adopts: `{ turn, attempt: 0, text: "", tools: [] }`. `live.text` for turn `T`:
if `state.turn !== T`, adopt only when `T === activeTurn` (a page opened mid-turn), else **ignore** (stale turn); then apply only when
`attempt >= state.attempt`, replacing `text` and `attempt`. `live.tool` for turn `T`: same adoption rule; update the tool entry (`started`
adds, `finished` marks it). `live.end` clears to `NO_LIVE` only when its turn equals `state.turn`. Malformed data, unknown names and
unknown fields are ignored. The component shows `live` only while `live.turn === detail.turn_in_progress?.turn`. `useConversationLive`
opens `useJournalStream({ topics: [`conversations:${id}`], enabled: id !== undefined, onFrame, onLive, onReset })` where `onFrame` and
`onReset` invalidate `keys.conversation(id)` and `keys.conversations` (skip a query whose cached `as_of_seq >= frame.seq`, the predicate
of prompt 28), and `onLive` feeds the reducer. **Prompt 28's `useJournalStream` gains one option** (**fixed**): `onLive?: (name: string,
data: unknown) => void`, called for exactly `live.start`, `live.text`, `live.tool` and `live.end` with the parsed JSON (malformed JSON is
dropped); live frames never change the hook's `lastId` and are never deduplicated.

```ts
// Kit (web/src/components/, exported from index.ts, demos in web/src/dev/, listed in web/DESIGN.md)
// SidePanel: prompt 30's kit chrome, used as is (verify its props in code); not defined here
OperatorThread(props: OperatorThreadProps)                   // 6.12; built on ChatComposer and ChatToolTags, not on ChatThread
export interface OperatorTurn {
  id: string; at: string; role: "user" | "assistant";
  status: "complete" | "pending" | "failed" | "cancelled";
  text: string; tools?: ChatTool[];
  problem?: { title: string; detail?: string };              // words from describeTurnFailure, set for failed or cancelled
  canRetry?: boolean;                                        // only the last assistant message, when failed or cancelled
}
// design vocabulary (web/src/design): TurnStatus = "pending" | "complete" | "failed" | "cancelled"; TURN_STATUSES table
// (label, tone, hint, icon each), turnStatusMeta(status) in tone.ts, an entry in VOCABULARIES. Suggested words and existing icons
// (verify the names in design/icons.tsx): pending "Answering" (progress, `clock`), complete "Answered" (success, `check-circle`),
// failed "Not answered" (warning, `alert`), cancelled "Stopped" (neutral, `x-circle`). Put the table in a new design/operatorVocab.ts.
// web/src/design/messages.ts: describeTurnFailure(failure: string | null | undefined, detail?: string | null): { title: string; detail?: string }
// covers the seven codes of 6.1 and any unknown string (a newer hub), never shows the raw code.
```

Draft retention (**fixed**): `web/src/features/operator/draft.ts` exports `readDraft(id)`, `writeDraft(id, text)`, `clearDraft(id)` over
`localStorage` key `operator:draft:<conversation id>`, every access in try/catch (blocked storage degrades to memory only). The composer
writes on every change. A send keeps the stored draft until `useSendMessage` resolves, then clears it; on rejection the text goes back into
the composer and the stored draft is untouched. Per-viewer conveniences only (the active conversation id `operator:active` and the panel's
open state are stored the same way); nothing that must persist is stored in the browser.

### 6.12 Operator surfaces **convention** (behaviour **fixed** where stated)

- **`OperatorConversation`** (`features/operator/OperatorConversation.tsx`) is the connected thread for one conversation id, used by both the
  panel and the workspace. It reads `useConversation(id)`, runs `useConversationLive`, maps messages to `OperatorTurn[]` (`turns.ts`), shows the
  scope as a `Tag` ("About all hosts" or "About nas", plus an "Operation" link when attached), and wires Send (`useSendMessage` with a fresh
  `crypto.randomUUID()` turn per question), Stop (`useCancelTurn`), Retry (`useRetryMessage`) and the draft. The composer hint says what the
  Operator may do: scope host: "Read-only. It can look at the containers, logs, journal and read skills of nas."; global: "Read-only. It
  answers from a summary of every host; start a conversation about one host to let it look closer."; both end "It never changes anything." A
  host that is offline gets an `InlineMessage` ("nas is offline. The Operator answers from what the hub last knew."). With no model
  (`useOperatorOff`, the old `useChatOff`) the composer is replaced by the existing "Set up a model" `EmptyState` link; a spent budget shows
  after a failed answer through `problem` and keeps the composer usable (Retry works once the budget allows).
- **Workspace** `/operator` and `/operator/:id` (`OperatorPage.tsx`): `PageHeader` (title "Operator", one-sentence intro, docs link); on wide
  screens two columns, the conversation list (about 320 px) at the left and `OperatorConversation` or `NewConversation` at the right; under
  768 px `/operator` shows the list and `/operator/:id` the thread with a back link. `ConversationList` is a plain bordered list of rows (title,
  scope tag, relative time, a pending dot) as one `Card` per row or a `DataTable`; the whole row is the click target; it has loading, empty and
  error states (`QueryBoundary`). `NewConversation` is a `Field` group: `SegmentedControl` "One host" / "All hosts" (neither preselected when no
  host is selected in the top bar; with a host selected, "One host" is preselected and shows that host as a `Select`; the choice is visible
  text, never implicit), an optional operation chip (from `?operation=<id>`), and a primary "Start conversation" button (disabled with a reason
  until a scope is chosen). Delete is a `ConfirmDialog` naming the conversation and saying its messages are removed (disabled with a reason
  while an answer is pending). Because the workspace lists every host's conversations, render `<AllHostsNote>` as the DESIGN rule requires
  (verify its props in `web/src/app/AllHostsNote.tsx`). `usePageName(conversation.title)` names the detail page.
- **Panel** (`OperatorPanel.tsx` inside `SidePanel`): header with the conversation picker (`OptionPicker` over the ten newest conversations plus a
  "New conversation" `Button`), an "Open in workspace" `LinkButton` and the close `IconButton`; body `OperatorConversation` for the active id, or
  `NewConversation` when there is none. The panel keeps working on every page. When the route is under `/operator`, the top-bar button is
  disabled with `disabledReason` "The Operator is open on this page." and the panel is closed (one stream per conversation per tab).
- **Right-slot registration**: `OperatorSurface.tsx` renders `<RightSlotSurface id="operator" title="Operator">` around
  `OperatorPanel` while the panel is open; `useRightSlot().open("operator")` and `.close("operator")` replace any local
  open state (verify the API in `web/src/app/rightSlot/types.ts`). Layout, width, sheet mode on narrow screens, focus,
  Escape and inert-page behaviour all come from 30's `RightSlotHost`; do not reimplement them. `TopBar.tsx` gets an
  `IconButton` (`icon="chat"`, `label` "Open the Operator" or "Close the Operator", `aria-pressed`) that toggles the
  surface, and the command palette (`commandPalette.ts`, `paletteSources.ts`) gets an action "Ask the Operator" that opens
  it.
- Motion uses `--motion-base` (180 ms) for now; under reduced motion the tokens are 0 ms. Colours, spacing and radii come only from tokens.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** List `crates/domovoi-store/migrations/` and note the next
   free number. Open every file, symbol and line range of section 3 and confirm it behaves as described. Confirm prompts 26 and
   28 are merged and read their real names: `AggregateKind::Operation`, `OperationId`, `OperationRepo::get`; `Topic::Conversation`,
   `topics_of`, `TopicSet`, `snapshot_seq`, `ApiDeps.journal`, the stream state machine's real structure (`events/stream.rs`,
   `wire.rs`), `useJournalStream`, `journalFrames.ts`. Check whether prompt 27 added an operation read dependency to `ApiDeps`
   (use it for the attach step and the context summary; otherwise add `ApiDeps.operations: Arc<dyn OperationRepo>`). Check whether
   34 and 42 merged and added `AggregateKind` or `StateWrite` variants (add yours beside them; do not reorder). Run `grep -rn
   "RightSlot\|right-slot" web/src` (section 3.6). Run `grep -rn "api/chat\|useChat\|features/chat\|\"/chat\"" crates web/src docs/book`
   and list every reference you must update. Run `graphify query "LlmProvider" --budget 1500` only as a map. If anything differs
   materially, write it down before coding; stop if it changes what you would build.
2. **Core.** IDs, actor constant, `conversation/` types with round-trip tests, `ConversationWrite`, `StateWrite::Conversation`,
   `AggregateKind::Conversation`, journal kinds, `ConversationRepo`, `AnswerSink` + `complete_streaming`, `LivePublisher`, the
   `topics_of` arm and `contains_conversation` (edit 28's `topic.rs` and its test deliberately). Fix every exhaustive `match` the
   compiler flags (store `writes.rs`, testkit `uow_apply.rs`, testkit `crash/mod.rs`, `AggregateKind` matches). Commit
   (`feat(core): ...`).
3. **LLM crate.** Sink plumbing in both stream assemblers and the three providers; unit tests. Commit (`feat(llm): ...`).
4. **Store.** Migration, `ConversationRepo for Store`, `uow/conversations.rs`, the `writes.rs` arm, purge. Update the guard list
   (`PURGED_TABLES`). Commit (`feat(store): ...`).
5. **Testkit.** In-memory twin, contract suite (register it for both stores in `crates/domovoi-store/tests/sqlite/contracts.rs` and
   the testkit's own contract test), `RecordingLive`, `ScriptedLlm::push_streamed`. Commit (`test(testkit): ...`).
6. **Engine, triage plumbing.** `RunBudget` carries the sink; `complete` takes it; `converse` reports tools; `chat::answer_with`;
   `TriageHandle::chat_streaming`. The existing `.chat(..)` tests must pass unchanged. Commit (`feat(engine): ...`).
7. **Engine, conversations.** `conversations/` module, tests in the new `conversations` test binary (add the `[[test]]` entry),
   the no-executor gate. Commit (`feat(engine): ...`).
8. **Hub.** `api/operator/`, delete `api/chat.rs` and `tests/hub/api_chat.rs`, `LiveBus`, the stream live branch, `ApiDeps`, runtime
   wiring and the other constructors, rewrite the gate list entry, hub tests. Regenerate OpenAPI (`cargo xtask openapi`), review and
   accept the changed insta snapshot (`hub__api_routes__openapi.snap`) only after reading it, then `cd web && npm run gen:api`.
   Commit (`feat(hub): ...`).
9. **Web.** In this order, committing after each: tokens and vocabulary (`turn status`, `describeTurnFailure`), kit (
   `OperatorThread` with `OperatorTurns`, demos, `DESIGN.md`), `api` hooks + `operatorLive.ts` + the `onLive` option of
   `useJournalStream`, `features/operator/*`, shell wiring (right-slot surface, top-bar button, nav, routes, palette), delete the old chat feature,
   tests. Check `/dev/design` in both themes and run `node scripts/screens.mjs`. Commits `feat(web): ...`.
10. **Docs and config comment**, then `cargo xtask docs-config` and `cargo xtask docs-links`. Commit (`docs: ...`).
11. **Run the checks and write the completion report** (standard block, below). Run in this order and fix each red check before the
    next: `cargo fmt --all`; `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>` for
    `domovoi-core`, `domovoi-llm`, `domovoi-store`, `domovoi-testkit`, `domovoi-engine`, `domovoi-hub`;
    `cargo nextest run -E 'rdeps(domovoi-core)'` (a public port and enum changed); `cargo xtask arch-check`; in `web/`:
    `npm run lint`, `npm run typecheck`, `npm test`, `npm run build`, `npm run format:check`.

## 8. Database and persistence

**Migration file:** `crates/domovoi-store/migrations/<next free number>_conversations.sql`. Number it by listing the directory on your
branch; never assume it; renumber only your own unmerged file if the branch gained a migration first. Additive only: two new tables, no
change to an existing table. `MIN_READER` (`crates/domovoi-store/src/compat.rs:18`) is **not** raised (the previous binary ignores the tables).
Enums are validated in Rust, so the DDL has no `CHECK` on an enum column (repository rule since `0007`); only structural checks.

```sql
-- Operator conversations: one row per conversation and one per message. Message text lives only here,
-- never in the journal, so retention can delete it (messages are removed after retention.conversations_days).
CREATE TABLE conversations (
    id           TEXT PRIMARY KEY,
    version      INTEGER NOT NULL,
    title        TEXT NOT NULL CHECK (length(title) <= 400),
    scope_kind   TEXT NOT NULL,                 -- 'global' | 'host', validated in Rust
    host_id      TEXT,                          -- set for 'host'; no foreign key: a conversation outlives its host row
    operation_id TEXT,                          -- the attached operation, if any; no foreign key (operations are prompt 26's table)
    last_ordinal INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,              -- ms since epoch, like every other table
    updated_at   INTEGER NOT NULL
) STRICT;
CREATE INDEX conversations_updated   ON conversations (updated_at DESC, id);
CREATE INDEX conversations_host      ON conversations (host_id) WHERE host_id IS NOT NULL;
CREATE INDEX conversations_operation ON conversations (operation_id) WHERE operation_id IS NOT NULL;

CREATE TABLE messages (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    ordinal         INTEGER NOT NULL,
    role            TEXT NOT NULL,              -- 'user' | 'assistant', validated in Rust
    status          TEXT NOT NULL,              -- 'pending' | 'complete' | 'failed' | 'cancelled', validated in Rust
    turn_id         TEXT NOT NULL,
    text            TEXT NOT NULL DEFAULT '',
    failure         TEXT,
    failure_detail  TEXT,
    tools           TEXT NOT NULL DEFAULT '[]', -- JSON array of {tool, failed}
    created_at      INTEGER NOT NULL,
    completed_at    INTEGER,
    UNIQUE (conversation_id, ordinal)
) STRICT;
CREATE UNIQUE INDEX messages_turn_role ON messages (conversation_id, turn_id, role);
CREATE UNIQUE INDEX messages_one_pending ON messages (conversation_id) WHERE status = 'pending';
CREATE INDEX messages_created ON messages (created_at);
```

Notes: `messages_turn_role` makes a replayed send detectable at the database as well; a retried assistant message gets a **new** `turn_id`, so
the unique key still holds; `messages_one_pending` is the database backstop for "one answer at a time" (a violation is `CommitError::Constraint`,
mapped to `TurnInProgress`). `PRAGMA foreign_keys` is on in this store (the cascade is relied on for `Delete`; verify in `db.rs` and add a test).

**Transactions.** Every writer command commits **one** `Transaction`:

| Command | Writes (in order) | Journal | Audit |
|---|---|---|---|
| `create` | `Conversation{ New }` | `conversation.created` | none |
| `send` | `Conversation{ Version }` (`last_ordinal += 2`, `updated_at`, `title` when empty), `Message{ user, New }`, `Message{ assistant pending, New }` | `conversation.message` user, then assistant pending | none |
| `settle` (answered, failed, cancelled by the service) | `Message{ assistant, Status(Pending) }`, `Conversation{ Version }` (`updated_at`) | `conversation.message` | none |
| `cancel` | same as `settle` with `Cancelled` | `conversation.message` | none |
| `retry` | `Message{ assistant, Status(Failed or Cancelled) }` (now `Pending`, new turn, cleared failure), `Conversation{ Version }` | `conversation.message` | none |
| `set_operation` | `Conversation{ Version }` | `conversation.updated` | none |
| `delete` | `Delete{ id, expected }` | `conversation.deleted` | `conversation.deleted` |
| startup recovery | per pending message: `Message{ Status(Pending) }` becomes `Failed/interrupted` | `conversation.message` | none |

The title is set by `send` when it is empty: the first line of the redacted question, cut to `MAX_TITLE_CHARS` characters (with `…`).

**Purge (retention).** Extend `crates/domovoi-store/src/purge.rs` inside the **same single transaction** as the other statements, using
the existing `policy.conversations` duration (no new policy field and no new config key):

```sql
DELETE FROM messages      WHERE created_at < ? AND status <> 'pending';
DELETE FROM conversations WHERE updated_at < ? AND operation_id IS NULL
                              AND NOT EXISTS (SELECT 1 FROM messages WHERE messages.conversation_id = conversations.id);
```
Add `"messages"` and `"conversations"` to `PURGED_TABLES`; neither has a full-text index, so `FTS_CLEANUP` stays empty and `keep_for` maps both to
`policy.conversations`. `PurgeReport` gains `operator_messages: u64` and `operator_conversations: u64` (the existing `conversations` field keeps
counting incident follow-up rows). A conversation attached to an operation keeps its row (title, scope, link) after its messages are gone: the
operation link is kept (04 section 14). Pending messages are never purged. Journal rows of conversations hold no text and are not touched here (the
journal has no pruner yet). If `purge.rs` would pass 500 lines, move the operator statements to `purge_operator.rs`.

**N-1 upgrade test.** Reuse the committed `crates/domovoi-store/tests/fixtures/n1-schema5.db` (do not regenerate it): open it with `Store::open`;
both tables exist and are empty, every pre-existing row survives (extend `tests/sqlite/upgrade.rs`).

## 9. Protocol and compatibility

- **Agent wire protocol:** unchanged (1.2 frozen, RW-42).
- **Browser API:** `POST /api/v1/chat` is removed and `/api/v1/conversations...` is added. The embedded web UI ships in the same binary, so hub and UI
  change together; a tab opened before the upgrade that calls the removed endpoint gets 404 until reloaded (accepted, like 28's old-tab note).
  The OpenAPI document gains tag `operator` and drops `ChatRequest`, `ChatResponse`; `web/src/api/{openapi.json,schema.d.ts}` are regenerated.
- **SSE:** `live.start`, `live.text`, `live.tool`, `live.end` are new event names on `GET /api/v1/events`; a client that does not know them ignores
  them (prompt 28's rule). `stream_events`'s description lists them. Journal frames of kinds `conversation.*` are new strings; readers already
  tolerate unknown kinds.
- **LLM port:** `AnswerSink` and `complete_streaming` are additive with a default body; no existing `LlmProvider` implementation must change.
- **Database:** additive migration; the previous release binary can open the upgraded database.
- **Config:** no new key. `retention.conversations_days` now also covers Operator conversations; its doc comment and the generated reference say so.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| No session | 401 `unauthorized` (`Authed`); unsafe methods without CSRF: 403 (existing middleware) |
| Create without `scope`, with an unknown `kind`, or `host` kind without `host` | 400 `bad_request`; nothing stored |
| Create with an unknown host name or operation id | 404 `not_found`; nothing stored |
| Send with empty or over-long text, a malformed `turn` | 400; nothing stored |
| Send while a turn is pending (different `turn`) | 409 `conflict`; nothing stored |
| Send with a `turn` already stored | 200 with the stored pair; no second message, no model call |
| Send to a conversation with `MAX_MESSAGES` messages | 409 `conflict`; nothing stored |
| Send or retry while `ApiDeps.triage` is `None` | 503 `triage_off`; nothing stored |
| Monthly budget spent when the model is asked | the question is already stored; the assistant message becomes `Failed/budget_spent`; `live.end{failed}` |
| Hourly limit reached, or another answer running in the worker | `Failed/busy` with the existing sentence in `failure_detail`; no model call, no usage |
| Every provider failed | `Failed/model_unavailable` with the redacted failure text |
| The user presses Stop | `cancel` commits `Cancelled`, cancels the turn token, publishes `live.end{cancelled}`; the late model result, if any, is discarded |
| Cancel of a turn that already settled | 200 with its status; nothing changes |
| A late result arrives for a message that is no longer pending or has another turn | nothing is committed; `live.end{discarded}` |
| Hub restarts while a turn runs | at start, recovery commits `Failed/interrupted` for every pending assistant message (journal row each); no model call is re-issued automatically (a person retries) |
| Hub shuts down while a turn runs | the turn token is cancelled with the worker shutdown; the pending message stays and is recovered at the next start |
| Store commit conflict in the writer | reload and retry once; a second conflict returns `ConversationError::Store`, HTTP 500 `internal` (logged) |
| Two sends race | the unique pending index makes the loser `TurnInProgress` (409) |
| Retry of anything but the last assistant message, or of a non-failed one | 409 `conflict`; nothing changes |
| Delete while a turn is pending | 409 `conflict` ("stop it first"); nothing deleted |
| Delete | one transaction removes the conversation and its messages (cascade), journal `conversation.deleted`, audit `conversation.deleted` |
| `retention` purge | old messages and empty unattached conversations are deleted in the existing purge transaction (section 8) |
| A host in the scope disappears | the conversation stays; `host_name` is `null`; the overview is built as for "all hosts" and no tools are offered |
| An attached operation disappears | the summary section is left out; the link stays |
| SSE client slow or gone | live frames are dropped (counter), the publisher never blocks, the committed answer arrives through the journal frame and the refetch |
| SSE `reset` or fresh connection | the web client refetches the conversation (`onReset`) |
| Web: POST fails (network or 5xx) | the question text goes back into the composer and the stored draft is kept; the failure shows through `useActionFeedback` with Retry; resending reuses a **new** `turn` |
| Web: a `live.*` frame for another turn | ignored (stale) |
| Web: the page opens mid-turn | `turn_in_progress` shows "Answering"; text appears with the next `live.text` snapshot |

Why model turns are **not** durable effects: a conversational answer re-run after a restart minutes later would surprise the person and spend
budget they did not ask for. The question is durable; the answer is retried only when the person presses Retry (RW-30 step 4 concerns operation
effects). Say this in the code comment of `recover.rs`.

## 11. Security requirements

- **Read-only Operator (INV-1, INV-9).** `ConversationDeps` holds no `ActionHandle`, `OperationHandle`, `AgentLink`, `Authorization`, `ProcessExecutor` or
  `Evidence` write type; the model's tools are the unchanged closed `ReadTool` set through `triage::chat`. No new tool, endpoint, chat command or code
  path lets anything but an authenticated human approve an action. Prompt 31 adds meta-tools later.
- **Gate `operator_has_no_executor`** (engine `tests/conversations/gates.rs`, hub `tests/gates.rs`): a source scan of `crates/domovoi-engine/src/conversations/`
  and `crates/domovoi-hub/src/api/operator/` fails if any non-comment line contains one of `HumanApproval`, `Authorization`, `ActionRequest`, `ActionHandle`,
  `OperationHandle`, `AuthorizedExecution`, `AuthorizedStep`, `execute_step`, `PlanHash`, `ProcessExecutor`, `AgentLink`, `HandoffRunner`, `run_skill`,
  `run_capability`, `start_skill`, `PolicyPermit`. The test also asserts that the scan found files (a scan of nothing must not pass). Never weaken it.
- **Authorization.** Every route is behind `Authed`; unsafe methods behind the CSRF guard. A conversation is the single owner's (RW-29: single owner in v1);
  there is no per-user filter. Every journal row carries the real `Actor`.
- **Client cannot forge history.** The server builds the model's history from stored `Complete` messages; the browser sends only a question and a turn id.
  `Failed`, `Cancelled` and `Pending` messages are never replayed to the model.
- **Validation.** Question: trimmed, 1 to 1000 characters; `turn` and ids: UUID parse; scope: closed enum; host: `host_named`; `operation_id`: must exist.
  Bodies are small (`DefaultBodyLimit` default is enough).
- **Secrets (INV-5).** Model input is redacted by the existing chat code; the answer is redacted before it is stored; the question is redacted before it is
  stored (so a pasted password is not kept); `failure_detail` is redacted and cut; the **live text** is released only through `releasable` (section 6.10) so a
  secret is never streamed, including one split across fragments or an unfinished private-key block; titles are derived from the redacted question; journal and
  audit rows contain no text. Never log message text or live frames.
- **Fail closed.** A refused admission, an unreadable limit, an unknown failure code or an unreadable store row never turns into a streamed or stored answer.
- **Abuse resistance.** At most one pending turn per conversation (database-enforced), `MAX_MESSAGES` per conversation, the existing per-worker answer semaphore
  and the hourly and monthly admission gate. The live bus is bounded (256) and lossy; event streams stay capped at `MAX_EVENT_STREAMS`.
- **Audit.** Only `conversation.deleted` is audited (it destroys data); an audit failure fails the commit (same transaction).
- **No new agent-facing surface.** Nothing here reaches `domovoi-agent`, `domovoi-proto` or the runner.

## 12. Testing requirements

Name tests after behaviour. Use the real fakes: `FakeClock`, `InMemoryStore`, `ScriptedLlm`, `RecordingLive` **(new)**. Tests never sleep: await a frame
only with `tokio::time::timeout(Duration::from_secs(10), ..)` as an upper bound; advance time with `FakeClock::advance`.

### 12.1 Core (`crates/domovoi-core`, unit tests next to the code, and `tests/`)

| Test | Setup, action, expected |
|---|---|
| `conversation_enums_round_trip_their_wire_strings` | `MessageRole`, `MessageStatus`, `MessageFailure`, `ConversationScope` kinds parse what `as_str` writes; an unknown string is `None` |
| `conversation_journal_kinds_round_trip` | extend `journal_kinds_round_trip` with the four new kinds and strings |
| `conversation_rows_belong_to_their_own_topic` | a `conversation.message` entry for conversation `C` maps to `[Topic::Conversation(C)]`; a non-UUID id maps to `[]` |
| `conversation_topic_matches_only_its_conversation` | a set of `conversations:<A>` matches an entry of `A` and not of `B`; `contains_conversation` agrees |
| `reserved_topics_match_nothing` (28's test, edited) | now only `hosts` is reserved; the test asserts `hosts` matches none of the sample entries and that `conversations:<uuid>` matches only conversation entries |
| `default_streaming_emits_the_whole_text_once` | a provider that implements only `complete` and returns two text parts: `complete_streaming` calls `sink.text` once per text part, in order, after the turn, and returns the same response |
| `message_defaults_are_pending_free` | `Message` of a user is `Complete` by construction helper; text and failure invariants of 6.1 hold (builder helper tests) |

### 12.2 LLM crate (`crates/domovoi-llm`)

| Test | Setup, action, expected |
|---|---|
| `anthropic_stream_forwards_text_deltas_to_the_sink` | feed `assemble` an event stream with three `text_delta` events and a tool block; the sink saw exactly the three fragments in order, none from the tool block or from thinking blocks; the returned response is unchanged |
| `openai_stream_forwards_content_deltas_to_the_sink` | same with `delta.content` chunks; reasoning content is not forwarded |
| `deadline_wrapper_forwards_streaming_and_still_times_out` | `with_deadline` over a scripted provider: `complete_streaming` returns the inner result and forwards fragments; a provider that never answers yields `LlmError::Timeout` at the clock deadline (`FakeClock`) |
| `streaming_without_a_sink_is_unchanged` | `complete` on both adapters behaves exactly as before (existing tests stay green) |

### 12.3 Store (`crates/domovoi-store/tests/sqlite/conversations.rs` **(new)**, `upgrade.rs`, `purge.rs`, `contracts.rs`)

| Test | Setup, action, expected |
|---|---|
| `sqlite_conversation_repo_satisfies_contract` and the in-memory run | the shared `conversation_repo_contract` suite (testkit): create, list newest first and by host and operation, paging, messages ordered, `by_turn`, `recent_complete` (only Complete, oldest first, `before` honoured, `limit` honoured), `pending`, guards (`Status` mismatch is a `Conflict`), version bump on conversation writes, delete cascades |
| `a_question_commits_both_messages_and_their_journal_rows_together` | commit the `send` transaction; both messages, the updated conversation and two journal rows exist; a forced failure of the second message (duplicate ordinal) rolls back **all** of it |
| `a_second_pending_message_in_one_conversation_is_refused` | insert two pending assistant messages: the second commit is `CommitError::Constraint` |
| `a_late_settle_of_a_cancelled_message_conflicts` | cancel, then commit a `Status(Pending)` replace: `Conflict` with `Observed::Status("cancelled")` |
| `deleting_a_conversation_removes_its_messages` | cascade works (foreign keys on) |
| `purge_deletes_old_messages_and_keeps_operation_links` | policy of 30 days: old messages deleted; an old conversation without an operation and without messages is deleted; an old conversation with `operation_id` stays with `message_count` 0; a recent message stays; a pending message of any age stays; report counts match |
| `purge_deletes_operator_rows_in_the_same_transaction` | make the last statement of `purge` fail (drop `sessions` on a test pool) and see no message deleted (extend the existing single-transaction test) |
| `purged_tables_list_names_the_operator_tables` | `PURGED_TABLES` contains `messages` and `conversations`; the existing FTS guard still passes |
| `conversation_tables_exist_after_upgrade_from_the_schema5_fixture` | open `n1-schema5.db`: both tables exist and are empty; every pre-existing row survives; `MIN_READER` is still `"0.0.0"` |
| `conversations_migration_is_additive` | `include_str!` the new migration: it contains none of `DROP`, `ALTER`, `DELETE` and no `CHECK (... IN (` enum list |
| `commit_publishes_conversation_journal_seqs` | the receipt's `journal_seqs` match the rows; `versions` reports the conversation's new version |

### 12.4 Engine (`crates/domovoi-engine/tests/conversations/`, new test binary `conversations`; plus `tests/services/`)

Harness (`harness.rs`): `InMemoryStore`, `FakeClock`, `RecordingLive` **(new)**, and a local `FakeChatModel` (scripted `Ok(ChatAnswer)` or `Err(TriageError)`;
`push_fragments(&[..])` makes it call `sink.attempt_started()` then `sink.text(..)` per fragment before returning; a gate (`hold`/`release`) so a test controls
when the answer arrives). A second set of tests goes through the **real** `TriageHandle` with `ScriptedLlm` in `tests/services/chat_streaming.rs` **(new)**.

| Test | Setup, action, expected |
|---|---|
| `a_question_commits_the_user_message_and_a_pending_answer_together` | `send`: `TurnStarted` has a `Complete` user message and a `Pending` assistant message with the same `turn`; the store holds both, `last_ordinal` is 2, the title is the question; journal has two `conversation.message` rows with no text in the payload; `live` saw `Start` |
| `the_answer_is_committed_with_its_tools_and_a_journal_row` | release the model with answer `"Two restarts."` and tools `[journal, docker_ps(failed)]`: assistant becomes `Complete`, `text` stored, `tools` stored as `MessageTool`, `completed_at` set; journal row status `complete`; `live` saw `End{Complete}` **after** the commit (the store already shows `Complete` when the event is observed) |
| `a_replayed_turn_returns_the_same_messages_and_calls_no_model` | send the same `turn` twice: second result has `replayed = true`, the same ids, and `FakeChatModel` was called once; message count 2 |
| `a_second_question_while_one_is_pending_is_refused` | `send` with another turn while pending: `TurnInProgress`, nothing stored |
| `a_full_conversation_refuses_another_question` | fill to `MAX_MESSAGES` (seed rows): `Full` |
| `stale_reply_discarded` | send, `cancel` (status `Cancelled`), then release the model with an answer: the message stays `Cancelled` with empty text, no new journal row, `live` saw `End{Discarded}` for the late result |
| `cancel_marks_the_turn_stopped_and_cancels_the_model_call` | `cancel` returns `Cancelled`; the model future was dropped (a drop flag on the fake); `live` saw `End{Cancelled}` |
| `cancel_of_a_settled_turn_changes_nothing` | after completion `cancel` returns `Complete`, no new rows |
| `retry_reruns_the_last_failed_turn_with_a_new_turn_id` | fail the first turn (`BudgetSpent`), then `retry`: the same assistant message becomes `Pending` with a new `turn`, failure cleared, the model is asked the **same question** with the same history; a replay of the original `turn` still returns the pair |
| `retry_is_refused_unless_the_last_answer_failed_or_was_stopped` | retry of a `Complete` message, of an earlier message, and while pending: `NotRetryable` or `TurnInProgress`, nothing changes |
| `model_errors_are_stored_as_failure_codes` | table-driven: `BudgetSpent` -> `budget_spent`, `Busy{..}` -> `busy`, `Provider(..)` -> `model_unavailable`, `AdmissionUnavailable` -> `model_unavailable`, `Store(..)` -> `internal`; each with a redacted, cut `failure_detail`; `live` saw `End{Failed}` |
| `history_sent_to_the_model_is_the_stored_complete_messages` | seed a conversation with complete, failed and cancelled turns; the next question's `ChatQuestion.history` contains only the `Complete` messages in order, at most 20, and never the pending one |
| `restart_marks_pending_turns_interrupted` | seed a pending assistant message in the store, build a new service over the same store: it is `Failed/interrupted`, a journal row exists, the model was not called |
| `the_question_is_redacted_before_it_is_stored` | question contains a registered secret: the stored user message and the title contain the redaction marker, not the secret |
| `closing_panel_does_not_stop_operation` | start a turn, drop every `RecordingLive` subscriber and every handle clone except the service's: the turn still completes and commits; (the attached operation, a seeded row, is untouched: its status and version are unchanged) |
| `global_scope_is_explicit` | `NewConversation` requires a `ConversationScope` (compile-time) and a created global conversation is stored with `scope_kind = global`; a host conversation stores the host id; scope never changes after creation (no API to change it) |
| `set_operation_attaches_and_detaches` | attach: journal `conversation.updated` (`operation_attached`), `operation` set; detach with `None`; no operation effect, no step, no audit |
| `delete_removes_the_conversation_and_audits_it` | delete: rows gone, journal `conversation.deleted`, audit `conversation.deleted` with the message count; delete while pending: refused through the hub (engine: `TurnInProgress`) |
| `the_turn_task_survives_the_command_that_started_it` | the `send` caller's future is dropped right after it returns; the turn still settles |

`live.rs` (sink and redaction; pure and through the sink):

| Test | Setup, action, expected |
|---|---|
| `live_text_holds_back_the_unfinished_word` | fragments `"The disk is at 9"`, `"4% now"`: after the first, the published text is `"The disk is at "`; after the second `"The disk is at 94% "` (the trailing word waits) |
| `a_secret_split_across_fragments_never_reaches_a_live_frame` | registered secret `hunter2hunter2` fed as `"password: hunt"`, `"er2hunter2 ok"`: no published text contains any part of the secret; the final published text contains the redaction marker |
| `an_unfinished_private_key_block_is_never_published` | fragments `"Here: -----BEGIN PRIVATE KEY-----\nMIIE"`, then `"...\n-----END PRIVATE KEY-----\ndone "`: nothing from the first `-----BEGIN` on is published until the block is complete, then only `[REDACTED]` |
| `rewritten_text_starts_a_new_attempt` | a redaction that changes text already sent: a `Text{attempt+1, ""}` is published first, then the corrected text |
| `a_new_request_starts_a_new_attempt_with_empty_text` | `attempt_started` twice: `Text{attempt 1, ""}` then (after text) `Text{attempt 2, ""}` |
| `live_text_is_throttled_by_the_clock` | two fragments 10 ms apart publish one frame; after `advance(100 ms)` the next fragment publishes; the attempt-start frame is never throttled |
| `live_tools_report_only_closed_names` | `tool("journal", Started, false)` publishes it; `tool("rm -rf /", ..)` publishes `unknown tool` |
| `live_text_is_cut_to_the_answer_limit` | 5000 safe characters: the published text has at most `MAX_ANSWER_CHARS` |
| `an_unfinished_word_is_not_published_at_all_without_whitespace` | one long fragment with no whitespace publishes nothing |

`gates.rs`:

| Test | Setup, action, expected |
|---|---|
| `operator_has_no_executor` | the source scan of 11 over `src/conversations/` finds files and none of the forbidden names |
| `the_operator_model_is_offered_only_read_tools` | through the real `TriageHandle` and `ScriptedLlm`: a streamed chat request lists exactly the read-only tool set and nothing else: `ToolSet::Read.tools()` if prompt 31 (meta-tools) has merged, otherwise `read_tools()` (extend the existing `model_sees_only_read_tools` idea to `chat_streaming`) |
| `a_model_that_names_approve_in_the_operator_changes_nothing` | the scripted model calls an `approve_action` tool: the call is `unknown tool`, no action row, no approval, no audit row |

`tests/services/chat_streaming.rs`:

| Test | Setup, action, expected |
|---|---|
| `chat_streaming_returns_the_text_and_the_tools_used` | chosen host, one scripted tool call then text: `ChatAnswer.text` is the redacted answer, `tools` has the call |
| `chat_streaming_streams_fragments_and_resets_per_request` | `ScriptedLlm::push_streamed` for a tool round then the answer: sink saw `attempt_started`, fragments, tool started and finished, `attempt_started`, fragments |
| `chat_streaming_discards_an_incomplete_turn_and_restarts_the_text` | first response ends at `MaxTokens`: the sink is told `attempt_started` again before the retry |
| `chat_and_chat_streaming_agree` | the same script through `chat` and `chat_streaming` gives the same text |

### 12.5 Hub (`crates/domovoi-hub/tests/hub/api_operator.rs` and `api_operator_live.rs` **(new)**, `tests/gates.rs`)

Use `api_with_triage()` (the scripted `Api.llm`) unless a test needs triage off (`api()`).

| Test | Setup, action, expected |
|---|---|
| `creating_a_conversation_requires_an_explicit_scope` | POST without `scope`, with `{kind:"all"}`, with `{kind:"host"}` and no host: 400 each; `{kind:"global"}` and `{kind:"host", host:"nas"}`: 201 with the scope echoed; an unknown host: 404 |
| `a_question_answers_202_and_the_answer_arrives_in_the_thread` | send, the scripted model answers; poll `GET .../{id}` until the assistant message is `complete` (bounded timeout); text equals the answer; `turn_in_progress` is `null` afterwards |
| `the_same_turn_sent_twice_is_answered_once` | two POSTs with one `turn`: 202 then 200, one model request, two messages |
| `a_second_question_while_answering_is_a_conflict` | hold the model, send, send again with another turn: 409 |
| `send_is_503_without_triage_and_reading_still_works` | `api()` (no triage): create 201, get 200, send 503 `triage_off` |
| `question_limits_are_enforced` | empty, whitespace, 1001 characters, malformed `turn`: 400, nothing stored |
| `history_comes_from_the_store_not_the_client` | two questions; the second model request contains the first exchange; a request body field `history` is ignored |
| `an_offline_host_conversation_gets_no_tools` | host scope with `Presence::Disconnected`: the model request has no tools; the overview names the host; the response is `complete` |
| `a_connected_host_conversation_offers_that_hosts_read_tools` | host scope connected: the request offers the read tools; a scripted tool call reaches that host's fake link |
| `the_overview_lists_hosts_incidents_and_the_chosen_hosts_facts` | the former `api_chat` overview assertions, moved |
| `a_spent_budget_is_a_failed_answer` | `api_with_triage_settings` with a spent monthly budget (the old test's setup): the POST is 202, the assistant message ends `failed` with `failure` `budget_spent`; no model request |
| `the_hourly_limit_is_a_failed_answer_with_the_retry_time` | exhaust the hour (old test's setup): `failed`, `busy`, `failure_detail` names the retry time |
| `retry_asks_again_and_completes` | after a failed answer, `POST .../retry` returns 202 with a pending message and a new turn; the model answers; it ends `complete` |
| `stop_cancels_the_turn_and_the_late_answer_is_dropped` | hold the model, send, `cancel`: 200 `{status:"cancelled"}`; release the model: the thread still shows `cancelled` and empty text (`stale_reply_discarded`, hub level) |
| `deleting_a_conversation_audits_and_removes_it` | delete 204; `GET` is 404; `audit_events("conversation.deleted")` has one row with the count; deleting while pending is 409 |
| `attach_operation_validates_and_feeds_the_overview` | `PUT .../operation` with an unknown id: 404; with a seeded operation: 200; the next model request's overview contains the "Attached operation" section with the goal; `null` detaches |
| `list_filters_by_host_and_operation_and_pages` | three conversations; filters and `limit`/`cursor` behave; newest first; `as_of_seq` equals the journal head read first |
| `conversation_snapshots_carry_as_of_seq` | `as_of_seq` is the head at request time; larger after a send |
| `conversation_writes_require_a_session_and_csrf` (in `tests/gates.rs`) | replace `(POST, "/api/v1/chat")` with the six write routes (create, send, cancel, retry, `PUT` operation, `DELETE`): 401 without a session, 403 without CSRF |
| `conversation_endpoints_require_a_session` | the two `GET`s answer 401 anonymously (add to `api_auth.rs`'s list) |
| `the_old_chat_endpoint_is_gone` | `POST /api/v1/chat` answers 404 |
| `operator_has_no_executor` (hub gate) | the scan of 11 over `src/api/operator/` |
| `no_message_text_in_journal_or_audit` | after a full exchange containing a distinctive sentence, no `journal.payload` and no `audit_log.detail` contains it |
| `openapi_document_lists_the_conversation_routes` | the OpenAPI JSON has the eight operation ids and not `chat`; the insta snapshot is reviewed |

SSE live (`api_operator_live.rs`, reading the stream body as 28's tests do):

| Test | Setup, action, expected |
|---|---|
| `a_conversation_stream_delivers_live_frames_without_ids` | open `?topics=conversations:<id>`, send with a streamed scripted answer: the frames `live.start`, at least one `live.text`, `live.end` appear, each with an `event:` line and **no `id:` line**; the journal frames for the same turn have ids |
| `live_frames_reach_only_the_subscribed_conversation` | two conversations, a stream on A only; a turn on B delivers no live frame and no journal frame to it |
| `live_frames_need_the_conversation_topic` | a stream with `topics=incidents` receives no `live.*` while a turn runs |
| `live_frames_are_never_replayed` | complete a turn, then connect with `Last-Event-ID` before the turn's journal rows: the journal rows replay, no `live.*` frame does |
| `live_text_never_contains_a_secret` | the scripted answer streams a registered secret in pieces: no frame body contains it |
| `a_lagging_live_subscriber_does_not_block_the_turn` | open a stream, never read it, run a turn that publishes more than 256 live frames: the turn commits; reading afterwards yields a journal frame for the final message (live frames may be missing) |
| `live_end_follows_the_committed_message_frame` | the `conversation.message` journal frame with status `complete` is delivered before `live.end` |
| `closing_the_stream_does_not_stop_the_turn` | open a stream, send, drop the body after `live.start`: the answer still completes (poll `GET`) |
| `keepalive_still_works_with_live_frames` | a live frame resets the keepalive timer (`FakeClock`): 14 s after a live frame stays silent, 15 s gives `: keepalive` |

### 12.6 Web (`web/src/**`, vitest; every page test is an axe check)

| Test | Expected |
|---|---|
| `liveReducer: a_late_frame_for_another_turn_is_ignored` | state at turn B; a `live.text` for turn A leaves it unchanged |
| `liveReducer: a_higher_attempt_replaces_the_text_and_a_lower_one_is_ignored` | attempt 2 text replaces attempt 1; then attempt 1 is ignored |
| `liveReducer: live_end_clears_only_its_own_turn` | `live.end` for the current turn clears; for another turn does nothing |
| `liveReducer: a_reload_mid_turn_adopts_the_active_turn` | with `activeTurn = T`, a `live.text` for T adopts it without a `live.start`; for another turn it does not |
| `liveReducer: malformed_or_unknown_frames_are_ignored` | no throw, state unchanged |
| `useJournalStream: onLive_receives_only_live_frames_and_never_moves_the_cursor` | after `live.text` and a journal frame `seq` 7, the reconnect URL has `after=7`; `onLive` saw only the live frame |
| `operator_thread_shows_streamed_text_and_replaces_it_with_the_committed_answer` | a pending assistant turn plus live text shows the text; after the detail refetch returns `complete`, the committed text shows and the live text is gone |
| `stale_reply_discarded` (web) | the detail shows turn B pending; a `live.text` for turn A never appears on screen |
| `sending_posts_the_turn_and_shows_the_question_at_once` | `POST .../messages` body is `{turn, text}` with a uuid turn; the pair shows without waiting for the answer; no `history` field |
| `stop_calls_cancel_with_the_turn_id` | the Stop button posts `.../turns/<turn>/cancel` |
| `failed_turns_show_words_and_retry` | a failed assistant message with `budget_spent` shows the title from `describeTurnFailure`, a Retry button for the last one only, and no raw code; Retry posts `.../retry` |
| `draft_survives_closing_the_panel` | type, close the panel, reopen: the text is back (from storage) |
| `draft_is_kept_when_the_send_fails` | the POST fails: the text is back in the composer and storage still has it; `draft_is_cleared_once_the_send_is_committed` |
| `global_scope_is_explicit` | the new-conversation form has no preselected scope without a selected host; Start is disabled with a reason until one is chosen; with a host selected, "One host" is preselected and shows that host; the created conversation's header says "About all hosts" or "About nas" |
| `the_selector_does_not_change_an_existing_conversations_scope` | open a host conversation, change the top-bar host: the scope tag and the composer hint do not change |
| `closing_panel_does_not_stop_operation` (web) | with a turn pending, closing the panel makes **no** cancel request and no `DELETE`; reopening shows the same pending turn |
| `workspace_lists_conversations_with_loading_empty_and_error_states` | skeleton, `EmptyState` with the next step, `LoadError` with Retry; the whole row is the click target |
| `workspace_new_conversation_prefills_from_the_query` | `/operator?operation=<id>&host=nas` shows the operation chip and the host |
| `deleting_asks_for_confirmation_and_is_disabled_while_answering` | `ConfirmDialog` names the conversation; the button has a `disabledReason` while a turn is pending |
| `panel_button_is_disabled_on_the_workspace_with_a_reason` | on `/operator` the top-bar button has `disabledReason` "The Operator is open on this page." |
| `panel_opens_focuses_the_composer_and_escape_closes_it` | keyboard: opening focuses the composer; Escape with focus inside closes and returns focus to the top-bar button; Escape with a menu open closes the menu first |
| `no_model_replaces_the_composer_with_the_setup_link` | triage off: the "Set up a model" `EmptyState`; no composer |
| `old_chat_route_redirects` | `/chat` renders the Operator workspace; nav shows "Operator" and no "Chat" |
| `turn_status_vocabulary_is_complete` | covered by the existing vocabulary test: `TURN_STATUSES` has label, tone, hint, icon for all four values |
| `turn_failure_codes_all_have_words` | every code `busy`, `budget_spent`, `model_unavailable`, `triage_off`, `interrupted`, `cancelled`, `internal` and an unknown string gives a sentence-case title with no raw code and no "!" (`voice.test.ts` still passes) |
| `kit: OperatorThread demos render and pass axe` | `/dev/design` demos in both states, test like `newKitDemos.test.tsx` |
| existing | `guardrails.test.ts`, `contrast.test.ts`, `voice.test.ts`, the size check (`npm run build`), the incident follow-up `ChatThread` tests all stay green; a deleted file's tests are deleted with it |

Fixtures: add conversation fixtures to `web/src/test/fixtures.ts` (a global and a host conversation, one with a failed turn, one pending) and a
frames fixture `web/src/test/fixtures/operator-live/frames.json` (`schema_version: 1`; frames `live_start`, `live_text_attempt1`, `live_text_attempt2`,
`live_tool_started`, `live_tool_finished`, `live_end_complete`, `live_end_discarded`) **generated by the Rust test** `live_frames_match_the_committed_fixture`
(hub, renders the production `wire::live_event` for fixed ids and compares; `OPERATOR_LIVE_UPDATE=1` rewrites it; read every generated file before committing).

## 13. Acceptance criteria

1. `POST /api/v1/conversations` with an explicit scope creates a durable conversation; without a scope it answers 400. `POST /api/v1/chat` is gone.
2. `POST .../messages` commits the user message and a pending assistant message (and their journal rows) in **one** transaction and answers 202 at
   once; the model runs in a task independent of the request; the same `turn` sent again answers 200 with the stored pair and makes no second model call.
3. The finished answer is stored (redacted, with the tools used) and a `conversation.message` journal row with status `complete` exists in the same
   commit; the journal and audit rows contain no message text.
4. While the model works, a connection subscribed to `conversations:<id>` receives `live.start`, `live.text`, `live.tool`, `live.end` exactly as in 6.5,
   with **no `id:` line**, never replayed, only for that conversation, lossy under backpressure without ever blocking the turn.
5. No secret is ever published in a live frame, including one split across fragments or an unfinished private-key block (`live_*` tests).
6. A late model result for a cancelled, settled or superseded turn is not committed and is shown nowhere (`stale_reply_discarded` in engine, hub and web).
7. A conversation holds at most one pending turn (database-enforced), at most `MAX_MESSAGES` messages, and the model's history is the stored `Complete`
   messages only; the client cannot send history.
8. A refused model call (budget spent, hourly limit, every provider failed) is a stored `Failed` answer with a code; Retry re-runs the last failed or
   stopped answer with a new turn id and the same question; a restart turns every pending answer into `Failed/interrupted` without calling the model.
9. The Operator is read-only: `operator_has_no_executor` passes in the engine and the hub; the model is offered exactly the read-only tool set (`ToolSet::Read` after prompt 31, `read_tools()` before); nothing in the new
   code constructs an authorization, an action, a plan or an operation step.
10. `topics_of` has the `Conversation` arm and `TopicSet::contains_conversation`; prompt 28's reserved-topic test is updated deliberately and passes.
11. Messages older than `retention.conversations_days` are deleted in the purge transaction; an empty unattached conversation is deleted with them; a
    conversation attached to an operation keeps its row; pending messages are kept.
12. `LlmProvider::complete_streaming` exists with a default body; the Anthropic and OpenAI-compatible adapters and the deadline wrapper forward text
    fragments; the existing `.chat(..)` engine tests pass unchanged.
13. The web UI has the Operator panel (every page) and the workspace (`/operator`, `/operator/:id`); `/chat` redirects; the draft survives closing the
    panel and a failed send; streamed text is replaced by the committed answer; closing the panel never cancels a turn; every state has loading, empty,
    error and offline handling; axe passes on every page test; the bundle budget holds (`npm run build`).
14. `cargo xtask openapi` and `npm run gen:api` were run, not hand-edited; `cargo xtask docs-config` and `cargo xtask docs-links` pass; the migration is
    additive and the schema-5 fixture upgrades with every pre-existing row intact.

## 14. Migration and compatibility

- **Existing installs.** The migration adds two empty tables; nothing is backfilled. Chat history that lived in browser `sessionStorage` is not migrated
  (it was per tab and cleared with the tab); say so in the release note.
- **Existing incident follow-ups** are untouched and keep their own storage (`triage.followup` timeline entries) and retention rule.
- **Agents** are unaffected (no protocol change). **Config files** need no change.
- **Rollback.** The previous release binary opens the upgraded database and ignores the tables; the conversations are then invisible, not lost.
- **Tabs open during the upgrade** must be reloaded (section 9).
- **Prompts that follow.** 31 (meta-tools) will extend `ChatModel` and the `ReadTool`-based catalogue behind the same `ConversationService`; 30 may add an
  "Ask the Operator about this operation" button that links to `/operator?operation=<id>`; 51 may count pending turns. None of that is built here.

## 15. Documentation requirements

- `docs/book/src/concepts/how-it-works.md`: replace the "Chat" section by an "Operator" section (durable conversations, explicit scope, Stop, Retry,
  streaming, read-only, retention, what a restart does to a running answer).
- `docs/book/src/concepts/operator.md` **(new)** and `SUMMARY.md` (under Concepts): what the Operator is, the panel and the workspace, scope, attaching an
  operation, what it can and cannot do, the monthly budget, retention (180 days by default, messages deleted, operation link kept, backups keep deleted data
  until they rotate).
- `docs/book/src/reference/api.md`: a "Conversations" section with the eight routes and the shapes of 6.6, the removal of `POST /api/v1/chat`, and in the
  Events section the four `live.*` names, "no `id`, never replayed, lossy".
- `docs/book/src/reference/data-model.md` (tables `conversations`, `messages`; the retention row names Operator conversations) and `guide/retention.md`.
- `crates/domovoi-config/src/hub/retention.rs`: the doc comment of `conversations_days` becomes "Days follow-up questions on an incident and Operator
  conversation messages are kept. Default 180." Then regenerate with `cargo xtask docs-config` (changes `docs/book/src/reference/configuration.md:212`).
- `ARCHITECTURE.md`: add the `conversations` writer to the writers list and the ports `LivePublisher` and `ChatModel` (and the `AnswerSink` hook of
  `LlmProvider`) to the ports table (verify the table near §10 and the writers text near §12; keep the edit small). No invariant changes; INV-1 and INV-9 are
  unchanged and the new gate is named there as an additional proof for them.
- `web/DESIGN.md`: add `OperatorThread` to the Components list and to "New kit components", note that the Operator is a surface of the
  right slot (section 3.6), and add the `turn status` vocabulary; no
  other section changes.
- OpenAPI regeneration: `cargo xtask openapi && cd web && npm run gen:api`; book link check: `cargo xtask docs-links`.
- Product docs and committed files must not mention prompts, the build plan or coding agents.

## Execution constraints

You are implementing **one** change in the Domovoi repository (a Rust workspace with an embedded React UI).
This document is your complete brief. The planning documents it cites are background: what you need is
written here.

1. **Inspect before relying on anything.** Before you edit, open every file, type and function this
   brief names.
   - Confirm that each one exists and behaves as described.
   - If something differs materially, write down the difference **before** changing code. Examples: a
     different signature, a missing module, behaviour already implemented, or a prerequisite prompt not
     merged.
   - If the difference changes what you would build, stop and report it instead of guessing.
2. **Follow the contracts here exactly.** Do not change public interfaces, wire formats, database
   schemas or security behaviour beyond what this brief specifies.
3. **Stay in scope.** No unrelated refactoring, renaming or reformatting. No new dependencies unless
   this brief names them. Add any new dependency to `[workspace.dependencies]` in the root
   `Cargo.toml` and reference it from crates with `{ workspace = true }`.
4. **Never weaken a security check, a gate test or an arch rule to make something pass.**
5. **Stop and report rather than invent.** Do this when the brief contradicts the code, an
   architectural decision is missing, or a prerequisite does not exist.
6. **Be truthful.**
   - Never claim a test passed unless you ran it and saw it pass.
   - Never present stubbed or partial work as complete.
   - Mark anything unfinished as unfinished.

This brief names no new dependency. If the web or Rust code seems to need one, stop and report.

## Repository rules

These come from `AGENTS.md`, which CI enforces.

- **Crate boundaries.** `domovoi-core`, `domovoi-proto` and `domovoi-engine` do no I/O. Only
  `domovoi-runner` spawns processes, always with argv vectors and never `sh -c`. Dependency edges follow
  `ARCHITECTURE.md` §4. `cargo xtask arch-check` enforces all of this.
- **Approvals.** `HumanApproval::new` is called only in `crates/domovoi-hub/src/auth/`, and
  `HumanApproval::restore` only in `crates/domovoi-store/src/`. The LLM tool catalogue stays closed.
  Nothing but an authenticated human approves an action.
- **Code rules.**
  - No `unwrap`, `expect`, `panic!`, `todo!`, `unimplemented!` or `dbg!` in library code.
  - No new `#[allow(...)]`.
  - `thiserror` in libraries; `anyhow` only in `domovoi` and `xtask`.
  - Every public item has a doc comment saying what it is for.
  - Non-test source files stay at or under 500 lines; web source files at or under 250 lines.
- **Data and time.** Never edit a merged migration; add a new one. Choose a migration's number by listing
  `crates/domovoi-store/migrations/` on your branch and taking the next unused number; never assume it
  from a plan or a predecessor. Renumber only your own unmerged file if the branch gained a migration
  first, and describe the migration by its contents, not its number. Time comes from the `Clock` port;
  tests advance the fake clock from `domovoi-testkit` and never sleep. Secrets are
  `secrecy::SecretString` and are never logged, put into prompts or returned from the API.
- **Web.** Read `web/DESIGN.md` before touching `web/`. Build only from the kit in
  `web/src/components/`. Use `useActionFeedback` for every mutation and `describeError` for every error
  message, and tokens only for styling.
- **Tests.** Name tests after the behaviour, not the function. Never delete or weaken `tests/gates.rs`.
  Review every `insta` snapshot you accept.
- **Branch and commits.**
  - Work on a branch named as the brief says.
  - Commit messages are one line in Conventional Commits form, with exactly one scope, for example
    `feat(store): ...`. No body and no trailers.
  - Committed files, commit messages and the PR description must not mention prompts, prompt numbers,
    the build plan, coding agents or AI assistance.
- **Checks to run.** Run them for what you changed only. CI runs the full suite.
  - `cargo fmt --all`
  - `cargo clippy -p <crate> --all-targets -- -D warnings` for each crate you edited
  - `cargo nextest run -p <crate>` for each crate you edited, or `cargo test -p <crate>` if nextest
    is not installed
  - `cargo nextest run -E 'rdeps(<crate>)'` when you changed a public item that other crates use
  - `cargo xtask arch-check` when you added or moved modules or touched a `Cargo.toml`
  - for `web/`: `npm run lint`, `npm run typecheck`, and the vitest files for the changed area
  - On Windows, the hub and agent run only under WSL 2; unit and integration tests run natively.

Notes specific to this change:

- PR title has at most one scope (CI rejects more): for example `feat(hub): durable Operator conversations`; the individual commits use the scopes
  of section 7 (`core`, `llm`, `store`, `testkit`, `engine`, `hub`, `web`).
- The existing hub tests that edit `crates/domovoi-hub/tests/gates.rs` and `tests/hub/main.rs` extend them; do not delete other gate tests.
- `cargo nextest run -E 'rdeps(domovoi-core)'` is required because `AggregateKind`, `StateWrite`, `JournalKind` and `LlmProvider` are public items other
  crates match on or implement.

## Completion report

End with a report in exactly this structure:

1. **Implemented behaviour**: what now works, in observable terms.
2. **Files**: added, changed and removed, each with one line on why.
3. **Implementation decisions**: any choice the brief left to repository convention, and what you chose.
4. **Interfaces**: types, functions, endpoints and messages introduced or changed, with signatures.
5. **Database and protocol changes**: migrations (file names), schema changes, wire changes.
6. **Security**: how each security requirement in the brief is met, and where it is tested.
7. **Tests added**: the test names, and which acceptance criterion each covers.
8. **Commands run**: each exact command and its actual result (pass, fail, counts).
9. **Not run**: tests or checks you did not run, and why.
10. **Deviations**: every place you departed from the brief, and why.
11. **Known limitations and remaining work**.

Also state in the report: the right-slot API names you used (section 3.6); the exact names you found for
prompt 26's and 28's items (section 7, step 1); and the list of seven failure codes kept in step between Rust (`MessageFailure`) and the web
(`describeTurnFailure`).
