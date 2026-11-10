# 26 · Security: one admission gate for triage, follow-ups and chat

Decided by the maintainer: follow-up questions (web and Discord) and the general chat
count against the same hourly cap as triage.

## Background (verified on `main`)

- Model calls start from three places in `crates/domovoi-engine/src/triage/`:
  - `run.rs` `outcome()` → `over_cap()` (`:177`): checks the monthly budget
    (`budget_spent`, `:155`) and the rolling hour (`Limits.starts`,
    `TriageSettings.per_hour_runs`, default 6), then records the start.
  - `followup.rs` `answer()` (`:130`): **checks neither**. Reached from
    `POST /api/v1/incidents/{id}/ask` and from Discord thread messages
    (`crates/domovoi-hub/src/auth/interactions/mod.rs:134`).
  - `chat.rs` `answer()` (`:134`): checks the monthly budget only.
- Each answer may make up to `max_tool_calls` (15) tool calls and walk the whole provider
  chain (`Shared::chain()`), so one question can be many model calls.
- `crates/domovoi-hub/src/api/chat.rs:132` spawns every chat request as its own task, so
  there is no bound on chat calls running at once. Triage itself runs on
  `concurrency` workers (`triage/queue.rs`).
- Config: `TriageSection` in `crates/domovoi-config/src/hub/llm.rs:103`, mapped to
  `TriageSettings` in `crates/domovoi-hub/src/runtime/settings.rs:40` and shown in
  `crates/domovoi-hub/src/api/settings.rs:60,274`.
- `ApiError::TooMany(i64)` (seconds to wait) already exists
  (`crates/domovoi-hub/src/api/error.rs:70`) and answers 429.
- With a shared cap of 6, a few questions could make the next incident's triage skip.
  So part of the hour is kept for triage.

## Steps

1. **Config.** Add to `TriageSection`:
   `reserved_for_incidents: u32` (default 2; doc: "Runs per hour that questions and chat
   may not use, so new incidents can still be triaged"). Validate it is below
   `per_hour_runs` in the existing validation (`crates/domovoi-config/src/validate*` or
   wherever `per_hour_runs` is checked; add an issue with the TOML path). Map it into
   `TriageSettings`. Run `cargo xtask docs-config`.
2. **Gate.** New `crates/domovoi-engine/src/triage/admission.rs`:
   - `pub(crate) enum Purpose { Triage, Question }`.
   - `pub(crate) enum Refusal { BudgetSpent, Hourly { retry_at: Option<Timestamp> } }`.
   - `pub(crate) async fn admit(shared: &Shared, purpose: Purpose) -> Result<(), Refusal>`:
     monthly budget first; then the rolling hour from `Limits.starts`. `Triage` may use
     every slot; `Question` only while free slots are more than
     `reserved_for_incidents`. On success it records the start in the same deque, so
     questions and triage share one count.
   - Move `over_cap`, `budget_spent`, `free_slots`, `next_slot` and their helpers from
     `run.rs` into this module; `run.rs` calls `admit(.., Purpose::Triage)` and maps the
     refusal to its existing `Cap` handling (deferred retry unchanged).
3. **Questions.** `followup::answer` and `chat::answer` call
   `admit(.., Purpose::Question)` before anything else. Add
   `TriageError::Busy { retry_at: Option<Timestamp> }` ("the hourly model limit is
   reached; try again after HH:MM UTC"). One call to `answer` counts as one start,
   whatever the number of tool calls.
4. **Concurrency.** Add `questions: tokio::sync::Semaphore` to `Shared`, sized
   `settings.concurrency`. `followup::answer` and `chat::answer` hold a permit for the
   whole answer; `try_acquire` failing returns `TriageError::Busy { retry_at: None }`
   ("another question is being answered; try again in a moment").
5. **Hub.** Map `TriageError::Busy` to `ApiError::TooMany(seconds until retry_at, or 30)`
   in `api/chat.rs`, `api/incidents.rs` (ask) and `api/followups.rs`. The Discord path
   already prints the error text; keep it.
6. **Settings page.** Show the new field next to "runs per hour" (it comes from the
   settings schema; check `web/src/features/settings/sections.ts` lists it under
   Triage).
7. **Book.** `docs/book/src/guide/llm-providers.md` (or the triage budget section):
   one paragraph saying questions and chat share the hourly limit and that
   `reserved_for_incidents` keeps room for triage.

## Tests (`crates/domovoi-engine/tests/services/limits.rs`)

- `questions_count_against_the_hourly_limit`: with `per_hour_runs = 3`,
  `reserved_for_incidents = 1`, two questions succeed and the third is `Busy`.
- `questions_cannot_use_the_reserved_runs`: after those two questions, a triage still
  runs; a fourth question is refused.
- `follow_up_is_refused_when_the_monthly_budget_is_spent`.
- `a_second_question_at_once_is_busy`: hold a permit with a blocking fake provider,
  ask again, get `Busy`.
- Hub: `chat_answers_429_when_the_hourly_limit_is_reached` in
  `crates/domovoi-hub/tests/hub/api_chat.rs`.

## Done when

- No model call starts without passing `admit`.
- Questions can never leave fewer than `reserved_for_incidents` runs for triage in an
  hour.
