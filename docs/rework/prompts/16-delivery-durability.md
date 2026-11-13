# 16 — Delivery durability: spool before state, notify state after confirmed delivery

| | |
|---|---|
| Branch | `delivery-durability` |
| Milestone | F2 |
| Depends on (must be merged) | 08 (index requirement; this prompt does not use 08's code, see §2 "Why not effects") |
| Can run in parallel with | 14, 15 |
| Produces (shared contracts) | none |
| Consumes | existing agent spool and check store; existing hub delivery path and `NotifyStateRepo` |
| Launch gate | none. Step 3 (R-4 investigation) only affects documentation wording; it does not gate the code. |
| Suggested model | medium |

## 1. Objective

Fix four durability defects (defect 01 D37) so that a failed write or failed delivery never leaves state
saying "done":

1. **Agent.** A check run's state (its check state, its firing memory) is saved only **after** the run's
   signals are safely in the spool. If the spool push fails, nothing from that run is remembered, so the next
   run produces the same signals again instead of silently losing a `firing` or `resolved` signal.
2. **Agent spool.** A failed `Spool::push` leaves the spool exactly as it was (no half-added batch in memory,
   no skipped sequence number).
3. **Hub, notification state.** The per-incident notification policy state (first alert time, reminder time,
   fold counters, held queue) is saved only **after** the cards it describes were delivered with a confirmed
   success. A failed delivery leaves the previous policy state in place, so the next event or tick plans the
   same notification again. Chat message references (card id, thread id) are still saved the moment a post
   is confirmed, so a posted card is never posted twice.
4. **Hub, reads.** A notification-state **read error** is an error: the event is not delivered and no default
   state is invented (a default state has no card reference, so it would post a duplicate card).
5. **Hub, quiet-hours release and reminders.** The "released" and "reminded" states are saved only after the
   release summary, card or reminder was delivered.

Plus one investigation step (R-4, Discord message `nonce`): read-only; the result decides one sentence of
documentation.

## 2. Architectural context

- **RW-03 / RW-15 / RW-35 and 01 D37.** The rule for any side effect: record "it happened" only after the
  system confirmed it happened, and never turn a failed read into an empty state. The target design makes
  notifications `Notify` effects (a durable outbox with leases, retries and dead-lettering, written by the
  incident writer in its own transaction). Those effects cannot exist yet: the incident writer moves to the
  unit of work in a later prompt, and the effect runner is prompt 15. This prompt therefore fixes the
  **ordering and error handling** inside today's in-process delivery, and states the delivery guarantee the
  target design also states: **at least once**.
- **At-least-once, in plain terms.** If the hub crashes or a call times out after a chat platform accepted a
  message but before the hub recorded it, the notification is sent again later, and a duplicate message is
  possible. Remote idempotency is used where the platform provides it (Discord `nonce` + `enforce_nonce`,
  R-4); that is investigated here but **not implemented**.
- **Why not effects (decision, do not reopen).** Prompt 08 only gives the table and the transaction port;
  15 gives the lifecycle. Notification plans are computed from bus events, which no owning writer commits yet.
  Doing it now would build a second, throw-away outbox.
- **Retry behaviour that remains.** There is **no periodic retry** of a failed card. A failed card is planned
  again by the next event that changes the card (triage completed, action changed, acknowledged, resolved,
  escalated), by the minute tick for reminders and released held alerts, and by the lag recovery. That is the
  behaviour today; this change makes it correct (today the state is advanced, so some of those later plans
  were wrongly suppressed). A real retry loop arrives with `Notify` effects. State this in the docs.
- **Policy state vs references.** `NotifyState` (`crates/domovoi-core/src/notify.rs:34-53`) holds two kinds of
  data: **chat references** (`messages`: card and thread ids per platform), which must be saved the moment a
  platform confirms a post, and **policy fields** (`first_alert_at`, `last_reminder_at`, `last_fold_reply_at`,
  `folds_since_reply`, `held`), which must advance only when the delivery they stand for is confirmed.

## 3. Repository context

Paths relative to `C:\Programming\domovoi`. Verify each before use.

**Agent** (`crates/domovoi-agent`)
- `src/scheduler.rs:62-83` `CheckEnv::run(check, store)`: runs the check on `store.state(key)` and **immediately**
  `store.put_state(&key, state)`:
  ```rust
  let report = run_bounded(check, &mut ctx).await;
  store.put_state(&key, state);
  report
  ```
  `CheckEnv::run` is also used by on-demand `run_check` (`src/requests/mod.rs:205`).
- `scheduler.rs:231-250` `run_one`; `:296-346` `run_due`. The part that is wrong (`:322-346`):
  ```rust
  let previous = self.store.firing(&key);
  let (signals, firing) = signals_for(self.env.host, check, &report, &previous, now);
  self.store.set_firing(&key, firing);          // remembered BEFORE the push
  batch.extend(signals);
  ...
  if !batch.is_empty() && let Err(err) = self.spool.push(batch).await {
      tracing::error!(%err, "could not spool signals");   // state is kept regardless
  }
  if let Err(err) = self.store.save() { tracing::error!(%err, "could not save check state"); }
  ```
  A failed run (`Err` report) still has its state stored by `CheckEnv::run`; the `Err` arms `continue`.
- `src/store.rs:44-96` `CheckStore` (`state`, `put_state`, `firing`, `set_firing`, `save` writing `checks.json`
  atomically, mode 0600).
- `src/spool.rs:116-176` `Spool::push`: increments `meta.next_seq`, pushes the batch to `batches` and adds to
  `bytes` **before** the file write, then `shrink`, `rewrite` or append, then `save_meta`; any error after the
  first mutation returns `Err` with memory changed. `SharedSpool` wraps it with a mutex and `pushed()` notify.
- `src/link/mod.rs` flush path reads `spool.pending_after(..)`; a batch that is in memory is sent even if the
  push reported an error.
- Tests: `crates/domovoi-agent/tests/agent/scheduler.rs` (helpers `start(checks)` building `Running { spool,
  store, outbox, shutdown, clock, _dir }`; `check_state_persists_after_each_run` `:268`;
  `ScriptedCheck`, `report`, `firing`, `key`, `settle` in `tests/agent/harness.rs`), `tests/agent/spool.rs`.

**Hub** (`crates/domovoi-hub/src/runtime/`)
- `delivery.rs` (430 lines; the file is at risk of exceeding 500: split if needed). Relevant code:
  - `event(&self, event)` `:55-71`: `let state = self.state(id).await;` then
    `let plan = notify::plan(...)`, then **`NotifyStateRepo::put(&*self.store, id, &plan.state, now)` before any
    delivery** (`:58-63`), then `self.actions(event.incident, plan.actions, plan.state, tokens)`.
  - `tick_actions(&self, incident, actions)` `:73-80`: loads `self.state(id)` again.
  - `actions(...)` `:82-110`: builds the card, wraps `state` in `Arc<tokio::sync::Mutex<NotifyState>>`, runs one
    job per gateway; each job runs every action with `perform`.
  - `perform(...)` `:112-140`: `Ok(()) => self.succeeded(&platform)`, `Err => self.failed(&platform)`; the result
    is not returned.
  - `post/update/reply` `:142-240` call `self.save(incident.id(), &current)` mid-delivery with the **whole**
    current state (policy fields included): e.g. after `post_card` succeeded, before `open_thread` was tried.
  - `save` `:~262` logs `could not save chat message references` on error.
  - `state(&self, incident) -> NotifyState` `:~270`:
    ```rust
    NotifyStateRepo::get(&*self.store, incident).await.ok().flatten().unwrap_or_default()
    ```
    Any read error becomes the empty default state.
  - `recover(&self, incident)` `:~330`: reads state with `match ... Err => warn; return` (already correct) and
    `Ok(None)` => a state with `first_alert_at = opened_at`; then `card_changed` and `actions`.
  - `send_all` (one-way notifiers: digests, release summary) `:~360`: counts confirmed sends.
  - `retry` `:~380`: backoff `[1,2,4,8,16]` s on the `Clock`; `BACKOFF` at `:22`.
  - `failed/succeeded`: the `DeliveryFailed` advisory.
- `dispatcher.rs`: `tick` `:~203-285`, the wrong part:
  ```rust
  for (incident, state) in &plan.states {
      if let Err(err) = NotifyStateRepo::put(&*self.store, *incident, state, self.clock.now()).await { ... }
  }
  for action in plan.actions { ... self.in_order(id, self.delivery.tick_actions(incident, vec![action])).await; }
  let Some(text) = plan.release else { return; };
  let summary_sent = summary_due && self.delivery.send_all(OutboundMessage{ title: "While you were away", .. }).await > 0;
  ... for id in released { ... PostCard via tick_actions ... }
  ```
  so `held = None` (released) and `last_reminder_at = now` are saved before anything is sent.
  `notification_items` `:~290-318` loads active incidents with `if let Ok(Some(state)) = NotifyStateRepo::get(..)`
  (errors silently skipped) and held states via `list_held`.
  `run`/`recover` `:~105-190`: the event loop and lag recovery (`submit(..., delivery.recover(incident))`).
- Pure policy code (do not change): `crates/domovoi-engine/src/notify/{plan,tick}.rs`: `plan(event, &state,
  settings, now) -> Plan { actions, state }`, `tick(items, settings, now) -> TickPlan { actions, states, release }`,
  `card_changed(incident, &state)`. `NotifyAction::{PostCard, UpdateCard, ThreadReply, Hold}`; `Hold` delivers
  nothing (the held state **is** the result).
- `crates/domovoi-store/src/repos/notify.rs:13-62` `NotifyStateRepo for Store`: `get` returns `Err` on a corrupt
  `state` JSON (`from_json(.., "notify state")`); table `notify_state(incident_id, held, state, updated_at)`
  (`migrations/0002_notify_state.sql`).
- `crates/domovoi-hub/src/runtime/schedulers.rs:131-181` digest: persists `digest_last_sent_at` only after a
  notifier accepted the digest. **Already correct; do not change it.**
- Tests: `crates/domovoi-hub/src/runtime/tests.rs` (in-crate; helpers `at`, `firing`, `resolving`, `post_count`,
  `wait_for_incidents`; `Harness` in `tests/harness.rs` with `with_adapters`, `start_dispatcher`, `webhook`,
  `incidents`, `advisories`, `put_notify_state`, `restart`, `runtime.as_ref().unwrap().store` and
  `.schedulers.minute()`), notable existing tests `delivery_retries_then_records_failure_without_blocking_others`
  `:99`, `message_references_survive_restart` `:156`, `quiet_hours_release_happens_at_end_time` `:215`,
  `quiet_hours_release_without_a_notifier_posts_held_cards` `:452`, `failed_quiet_hours_summary_falls_back_to_held_cards`
  `:618`. Fakes: `RecordingGateway` (`fail_next(ChatError)`, `calls()`), `RecordingNotifier` in
  `crates/domovoi-testkit/src/chat.rs`.
- Discord adapter for the R-4 step: `crates/domovoi-chat/src/discord/gateway.rs:129-146` `post_card` (no nonce
  today).

## 4. Exact scope

**A. Agent**
1. `CheckEnv::run_staged(check, store) -> (Result<CheckReport, CheckError>, CheckState)`: runs the check on a
   copy of its stored state and returns the state it leaves **without storing it**. `CheckEnv::run` becomes
   `run_staged` + `store.put_state` (behaviour unchanged for on-demand runs).
2. `Scheduler::run_due` stages every run's result and commits (state, firing memory, facts, `store.save()`)
   only after the spool push succeeded, or when there was nothing to push.
3. `Spool::push` restores memory on failure.

**B. Hub**
4. `Delivery`: working state with a base; refs-only saves; `Delivered` outcome; `event` saves policy state only
   on full success; state read returns `Result`.
5. `Dispatcher::tick`: deliver first, then save, per incident; release handling per §6.4.
6. `notification_items`: a state read error skips that incident and counts it.

**C. R-4 investigation** (read-only, no code change).

**D. Tests and docs.**

Files expected to change: `crates/domovoi-agent/src/{scheduler,spool}.rs`;
`crates/domovoi-hub/src/runtime/{delivery,dispatcher}.rs` (split `delivery.rs` into `delivery.rs` +
`delivery/state.rs` or similar if it would pass 500 lines); tests in `crates/domovoi-agent/tests/agent/
{scheduler,spool}.rs`, `crates/domovoi-hub/src/runtime/tests.rs` plus a new
`crates/domovoi-hub/src/runtime/tests/delivery_state.rs` **(new)** declared with `mod delivery_state;` next to
`mod harness;`; book pages in §15.

## 5. Explicit non-goals

- No `Notify` effects, no outbox table, no use of `UnitOfWork`, no lifecycle (prompts 08, 15; the incident
  writer in a later prompt). No periodic retry loop for failed cards.
- No change to the pure notification policy (`domovoi-engine/src/notify/`), to card rendering, to the
  `NotifyStateRepo` trait or its table, or to `NotifyState`'s fields.
- No Discord nonce implementation (R-4 is investigated only). R-1…R-10 are not resolved by this prompt.
- No change to the digest persistence (already correct), to `DeliveryFailed` advisory semantics, to
  `retry`/`BACKOFF`, or to metrics names (one new counter is allowed, §6.5).
- Agent `checks.json` that fails to parse is still replaced by an empty store (`store.rs:43-46`); changing that
  is out of scope (report it as a remaining risk).
- No start-up replay of cards for incidents that never got a state row.
- No change to `domovoi-store`, `domovoi-core` or the wire.

## 6. Required interfaces and contracts

### 6.1 Agent (**fixed**)

```rust
impl CheckEnv {
    /// Runs `check` on a copy of its stored state and returns the report with the state it left,
    /// without storing it. The scheduler stores it only once the run's signals are spooled.
    pub async fn run_staged(&self, check: &dyn Check, store: &CheckStore)
        -> (Result<CheckReport, CheckError>, CheckState);

    /// Runs `check` once on its stored state and stores the state it leaves (on-demand runs).
    pub async fn run(&self, check: &dyn Check, store: &CheckStore)
        -> Result<CheckReport, CheckError>;   // = run_staged, then store.put_state, same as today
}
```
`run_due` (**fixed behaviour**):
```text
reports = run checks concurrently with run_staged             (unchanged concurrency, MAX_CONCURRENT)
staged  = []   // per check: key, new_state, Option<firing set>, facts
batch   = []
for (check, (result, new_state)) in reports:
    Err(unavailable) -> debug log; staged.push(key, new_state, None, [])       // state only; no signals
    Err(other)       -> warn log;        staged.push(key, new_state, None, [])  // as today: a failing check's state is kept
    Ok(report)       -> previous = store.firing(key)
                        (signals, firing) = signals_for(host, check, &report, &previous, now)
                        batch.extend(signals); staged.push(key, new_state, Some(firing), report.facts)
if batch is non-empty:
    match spool.push(batch):
        Err(e) -> tracing::error!("could not spool signals; this run's state is discarded and the run repeats"),
                  DISCARD staged (do not touch store or self.facts), return
        Ok     -> continue
commit staged: for each: store.put_state(key, new_state); if Some(firing): store.set_firing(key, firing);
               for facts: self.facts.insert(..)
store.save()   // error logged, as today
```
A failed run's discarded state means the next run starts from the old state; the next due time is still
computed from the run time (the loop in `run` already does this), so the retry happens one cadence later.

`Spool::push` (**fixed behaviour**): on any error after the in-memory batch was added, remove that batch from
`batches` (if it is still the last one), subtract its size from `bytes`, and restore `meta.next_seq` to its value
before the call; then return the error. If `shrink` already dropped older batches in memory, leave that as is
(they are still on disk and will simply be resent after a restart: at-least-once). Do not change `ack`,
`pending_after`, `open`, or the file format.

### 6.2 Hub: working state (**fixed**)

Inside `delivery.rs` (or `delivery/state.rs`):
```rust
/// What a delivery pass works on: the state before the event (`base`, what is stored now) and the state
/// the plan wants after delivery (`current`, which also collects chat references as posts are confirmed).
struct Working { base: NotifyState, current: NotifyState }

/// What a pass delivered.
pub(super) struct Delivered {
    /// Every action on every gateway was confirmed (or there was nothing to deliver).
    pub ok: bool,
    /// `current` after the pass: plan state plus the chat references that were confirmed.
    pub state: NotifyState,
}
```
- The `Arc<tokio::sync::Mutex<NotifyState>>` parameter of `perform/post/update/reply/message` becomes
  `Arc<tokio::sync::Mutex<Working>>`.
- `save(incident, &Working)` writes **refs only on top of the base**: 
  `NotifyState { messages: working.current.messages.clone(), ..working.base.clone() }`, skipping the write when
  that equals `working.base`. A failed write is logged at `error` and counted (§6.5), not swallowed silently.
  Use it at every place that called `self.save(.., &current)` mid-delivery (after `post_card`, after
  `open_thread`).
- `perform` returns `bool` (`true` for `Ok`, `Hold`; `false` for `Err`). `actions(...)` returns `Delivered`:
  - no actions, or no gateways: `Delivered { ok: true, state: current }` (nothing to confirm; behaviour for
    "no chat configured" is unchanged: the plan state is saved as before);
  - `card()` returned `None` (host missing, store error): `ok: false`, state unchanged, logged at warn as today;
  - otherwise `ok` = every gateway job returned true for every action; `state` = `current` after the jobs.
- Persisting the **policy** state:
  ```rust
  fn confirmed(base: &NotifyState, delivered: Delivered) -> NotifyState {
      if delivered.ok { delivered.state }
      else { NotifyState { messages: delivered.state.messages, ..base.clone() } }
  }
  ```
  The caller `put`s `confirmed(..)` only when it differs from `base` (no row is created for an event that
  changed nothing and delivered nothing).

### 6.3 Hub: `Delivery::event`, `state`, `tick_actions` (**fixed behaviour**)

```rust
async fn state(&self, incident: IncidentId) -> Result<NotifyState, StoreError> {
    Ok(NotifyStateRepo::get(&*self.store, incident).await?.unwrap_or_default())
}   // Ok(None) means "nothing was said yet": a legitimate default. Err is an error.
```
```text
event(event):
  base = match self.state(id): Ok(s) -> s
         Err(e) -> tracing::error!(%e, %id, "could not read notification state; this event is not delivered");
                   counter notify_state_errors += 1; return        // NO default state, NO delivery
  plan = notify::plan(&event, &base, &settings, now)
  delivered = self.actions(event.incident, plan.actions, working(base.clone(), plan.state), tokens)
  to_save = confirmed(&base, delivered)
  if to_save != base: put(to_save)  -- on error: error log + counter; (the next event re-plans from the old row)
```
`tick_actions` changes to
`pub(super) async fn tick_actions(&self, incident: Incident, before: NotifyState, after: NotifyState,
actions: Vec<NotifyAction>) -> bool`: runs the actions with `Working { base: before, current: after }`, saves
`confirmed(&before, delivered)` (when it differs from `before`) and returns `delivered.ok`. It no longer reads
the state itself.

`recover` is unchanged except that it passes its state as both `base` and `current` through the new
`actions` signature and saves refs the same way (it never advances policy fields).

### 6.4 Hub: `Dispatcher::tick` (**fixed algorithm**)

```text
items  = notification_items()                      // Vec<(Incident, NotifyState)>; states are the stored ("before") ones
plan   = notify::tick(&items, &settings, now)
before = map id -> NotifyState from items
after  = map id -> NotifyState from plan.states
released = ids where before[id].is_held() and !after[id].is_held()          (as today)

1. Reminders (incidents in `after` that are not in `released`):
     acts = plan.actions whose incident id is this one
     in_order(id, delivery.tick_actions(incident, before[id], after[id], acts))
        // saves after[id] only if every action was confirmed; otherwise refs only
     (an incident in `after` with no actions: save after[id] directly)
2. Release (only when `plan.release` is Some):
     summary_due  = delivery.can_replace_held_cards()
     summary_sent = summary_due && send_all("While you were away", text) > 0
     summary_failed = summary_due && !summary_sent
     for id in released:
        needs_card = !(incident.resolved_at().is_some() && !summary_failed)       // as today
        if needs_card:
            in_order(id, delivery.tick_actions(incident, before[id], after[id], [PostCard{id}]))
        else:
            // resolved while held and its summary went out (or no summary mechanism is configured)
            save after[id]                                        // nothing else to confirm
     If `plan.release` is None, any id in `released` is saved as `after[id]` (nothing to send).
```
Consequences (write them as comments): if a released card or summary is not delivered, the stored state stays
**held**, so the next tick releases it again; a reminder that fails is planned again at the next tick because
`last_reminder_at` did not advance. Held states that are still held are never in `plan.states`.

`notification_items`: a `NotifyStateRepo::get` error for one incident is logged at `error`, counted (§6.5) and
that incident is skipped for this tick (as today it was silently skipped); it is never replaced by a default.

### 6.5 Metric (**fixed**)

One new counter `domovoi_notify_state_errors_total` with label `op` = `read` or `write`, incremented at each
state read error (event, tick) and each failed state write (including the refs-only save). Add it to the metrics
table in `docs/book/src/reference/cli.md` next to `domovoi_delivery_failures_total`.

### 6.6 R-4 investigation (**procedure**)

1. Find the `twilight-http` version in `Cargo.lock`; open its source under `%USERPROFILE%\.cargo\registry\src`.
2. Check whether `CreateMessage` has `nonce` and `enforce_nonce` (or equivalent) methods in that version.
3. Check, in `crates/domovoi-chat/src/discord/gateway.rs`, that `post_card`, `reply` and `open_thread` do not
   set one (they do not).
4. Write the finding (versions, method names, yes or no) in the completion report under "Implementation
   decisions". **Do not add a nonce.** The documentation sentence in §15 is chosen by the result:
   - nonce supported by the pinned client: "Discord deduplicates a retried message only if the hub sends a
     nonce; it does not yet, so a duplicate card is possible after a crash";
   - not supported: "The Discord client the hub uses cannot deduplicate a retried message, so a duplicate is
     possible after a crash".

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file and line range in §3.
   Confirm the three wrong behaviours exist as quoted (state saved before delivery in `event` and `tick`;
   `state()` swallows errors; `set_firing`/`put_state` before `push`; `push` mutates before writing). Run
   `cargo nextest run -p domovoi-agent -p domovoi-hub` for a baseline and note existing failures.
2. **Write the failing tests first** (§12) for the four spec names and run them; record that they fail on the
   baseline: `check_state_not_saved_when_spool_push_fails`, `notification_state_written_only_after_success`,
   `notification_state_read_error_is_not_default_state`, `quiet_hours_release_not_saved_before_delivery`.
3. **R-4 step** (§6.6). Record the result.
4. **Agent**: `Spool::push` rollback; `run_staged`; `run_due`.
5. **Hub**: `Working`/`Delivered`; `state()`; `event`; `tick_actions`; `recover`; `Dispatcher::tick`;
   `notification_items`; the counter. Keep every file ≤ 500 lines (split `delivery.rs`).
6. **Update existing tests** whose expectations assumed state advanced before delivery (§12 "Changed").
7. **Docs** (§15).
8. **Checks and completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-agent -p domovoi-hub
   --all-targets -- -D warnings`; `cargo nextest run -p domovoi-agent -p domovoi-hub`;
   `cargo xtask arch-check` (only if modules were added or moved); `cargo xtask docs-links`. Confirm the step-2
   tests now pass.

## 8. Database and persistence

Not applicable: no migration, no schema change. `notify_state` rows keep their format; the agent's
`checks.json` and `spool.jsonl` keep theirs. Transaction boundaries: the hub's single `NotifyStateRepo::put` is
one statement; the agent's commit is "spool push durable, then `checks.json` rewritten atomically"; a crash
between the two means the run repeats after restart (at-least-once; the hub folds repeated firing signals).
No N-1 upgrade test is needed (no persisted shape changes).

## 9. Protocol and compatibility

Not applicable: no wire change. Behavioural notes for mixed versions: none (the agent change is local to the
agent; the hub change is local to the hub).

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Spool push fails (disk full, permission) | log `error`; staged state, firing memory and facts of that run are discarded; spool memory unchanged; the next run (one cadence later) recomputes the same signals |
| Spool push fails mid-way after memory change | the added batch is removed from memory; `next_seq` restored; error returned |
| Check run errors (`Err`) | state staged and committed with the rest (as today); no signals; no change to firing memory |
| `store.save()` fails after a good push | log `error` (as today); in-memory state is current; the file catches up at the next save |
| Agent crashes between push and save | the run repeats after restart; the hub folds duplicate firing signals; a `resolved` signal repeats harmlessly |
| Notification state read error on an event | log `error`, count `op="read"`, **do not deliver**, do not write; the next event retries |
| Notification state read error on a tick | that incident is skipped this tick |
| `Ok(None)` state | legitimate default (nothing said yet) |
| `post_card` confirmed, `open_thread` fails | the card reference is saved immediately (refs only on the old base); policy fields stay old; the next plan finds the card and does not repost |
| Every gateway confirms | policy state advances and is saved once |
| One gateway fails, another succeeds | policy state does **not** advance; the good gateway keeps its saved references (no second card); a repeated thread reply or reminder to it is possible (documented at-least-once) |
| Card cannot be built (host missing, store error) | `ok = false`; nothing saved; warn log as today |
| Quiet-hours release: summary or card not delivered | state stays held; next tick releases again |
| State write fails after confirmed delivery | log `error`, count `op="write"`; the next event plans from the older state (a duplicate reminder or card update is possible) |
| No gateways configured | plan state saved as before (nothing to confirm) |
| `Hold` action | delivers nothing; held state saved |

## 11. Security requirements

- No new secrets; card text and logs unchanged. Error logs carry incident ids and store error text only (never
  card contents or tokens). Action approval tokens (`tokens` map in `event`) are used exactly as today and are
  not persisted by this change.
- Fail closed on reads: an unreadable state suppresses delivery instead of inventing a state (no duplicate
  cards that could carry fresh approval links).
- Nothing here approves, dispatches or modifies an action. `HumanApproval::new/restore` untouched.
- The agent still writes `checks.json` and the spool with mode 0600 through the existing helpers.

## 12. Testing requirements

Fakes: `FakeClock` (agent harness `clock()`, hub `Harness.clock`), `RecordingGateway` (`fail_next`),
`RecordingNotifier`, real `Store` in the hub harness, `ScriptedCheck`. Never sleep.

**Agent** (`crates/domovoi-agent/tests/agent/scheduler.rs`)
- Helper (new, in the test file): `block_spool(dir)` creates a **directory** named `spool.jsonl` inside the
  state directory after `Spool::open` (so `push` fails when it opens the file for append or renames over it) and
  `unblock_spool(dir)` removes it. If the platform lets the push succeed anyway, stop and report instead of
  adjusting the assertion.
- `check_state_not_saved_when_spool_push_fails` (spec name, **fails on baseline**): `ScriptedCheck` that
  increments `runs` in its state and fires target `a`; block the spool; first run happens at start. Assert:
  the spool holds no batch; `store.state(key).get("runs")` is `None`; `store.firing(key)` is empty;
  `CheckStore::open(dir)` has neither. Then `unblock_spool`, advance the clock by the cadence, `settle()`:
  `runs == "1"` (the failed run's state was discarded), the firing signal for `a` is in the spool, `firing`
  contains `a`, and `checks.json` has them.
- `firing_signal_is_not_lost_when_spool_push_fails`: as above, asserting exactly one batch with one firing signal
  after recovery (not zero, not two).
- `resolved_signal_is_not_lost_when_spool_push_fails`: run 1 fires `a` and commits; block; run 2 reports `a`
  fine (would send `resolved`) and fails to spool; `firing` still contains `a`; unblock; run 3 spools the
  `resolved` signal.
- `check_with_no_signals_still_saves_state`: a check that reports nothing firing, no previous firing: no batch;
  state saved (`runs` persisted).
- `failing_check_keeps_its_state_as_before`: a check returning `Err` still stores its mutated state (unchanged
  behaviour).
- `on_demand_run_check_still_stores_state` (`tests/agent/requests.rs`, or add there): `run_check` stores state
  immediately as before.
- Existing `check_state_persists_after_each_run` must pass unchanged.

**Spool** (`crates/domovoi-agent/tests/agent/spool.rs`)
- `push_failure_leaves_the_spool_unchanged`: `Spool::open`, push one good batch (seq `s`), block, push again
  fails; `len() == 1`; `pending_after(None)` has only the first batch; after unblocking, the next push gets
  seq `s + 1` (no gap).

**Hub** (new `runtime/tests/delivery_state.rs`, plus changes in `tests.rs`)
- Helper `stored_state(&harness, incident) -> Option<NotifyState>` via `NotifyStateRepo::get(&*store, id)`.
- `notification_state_written_only_after_success` (spec name, **fails on baseline**): one gateway `bad` with
  enough `fail_next(ChatError::Transport(..))` entries to exhaust a whole retry chain (12, as in
  `delivery_retries_then_records_failure_without_blocking_others`); fake clock at a fixed time; post a firing
  webhook; advance the clock 31 s in 1 s steps with `yield_now` (copy that test's loop) until a
  `DeliveryFailed` advisory exists. Assert the stored state has `first_alert_at == None`, no `messages`, and is
  not held. Then let the gateway recover (no more failures) and trigger a card-changing event for the same
  incident (post a second event that plans `card_changed`: easiest is the lag path `harness` offers in
  `bus_lag_triggers_reload_not_crash` at `tests.rs:362`, or `TriageCompleted` via an injected event; read those
  tests and reuse the mechanism). Assert exactly one `PostCard` call succeeded overall and the stored state now
  has `first_alert_at` and a `messages["<platform>"]` entry.
- `notification_state_written_after_a_confirmed_post`: the happy path: after a webhook, wait for `post_count == 1`;
  stored state has `first_alert_at` and the card reference.
- `card_reference_saved_when_thread_fails_and_not_reposted`: the gateway's `post_card` succeeds and `open_thread`
  fails every time (script `fail_next` so that the second call of the chain fails; read `RecordingGateway` to see
  which call consumes which failure): after exhaustion the stored state has the card reference **and** the old
  policy fields (`first_alert_at == None`); a later event does not post a second card (`post_count == 1`).
- `notification_state_read_error_is_not_default_state` (**fails on baseline**): store a corrupt row for the
  incident (`sqlx::query("INSERT INTO notify_state (incident_id, held, state, updated_at) VALUES (?, 0, 'not json',
  0)")` through `runtime.store.pool_for_tests()`, or `put_notify_state` then an `UPDATE`), then post a firing
  webhook for that incident (the incident id must exist first: create the incident with one webhook, stop the
  dispatcher, corrupt the row, restart the dispatcher, send a second webhook). Assert `post_count` does not
  increase (no duplicate card), the row is untouched, and `domovoi_notify_state_errors_total{op="read"}`
  incremented (reuse the `LagRecorder` metrics recorder pattern in `tests.rs:323-360`).
- `quiet_hours_release_not_saved_before_delivery` (spec group, **fails on baseline**): incident held in quiet
  hours (as `quiet_hours_release_happens_at_end_time`), no notifier (`Harness::without_notifier`) and a gateway
  that fails the card post for the whole retry chain; set the clock after quiet hours and run
  `schedulers.minute()`; advance 31 s with yields; assert `NotifyStateRepo::list_held` **still lists** the
  incident (state still held). Then let the gateway succeed, run `minute()` again: the card is posted and the
  incident is no longer held.
- `reminder_state_not_advanced_when_reminder_fails`: a posted, open critical incident past its reminder time,
  the reply fails for the whole chain: `last_reminder_at` unchanged in the store; next tick after recovery sends
  the reminder and advances it.
- `summary_failure_keeps_held_incident_held_until_a_card_is_delivered`: with a notifier that replaces held cards
  failing once and the gateway failing: still held; both succeed later: released, one card.
- `digest_delivery_date_is_saved_only_after_a_notifier_accepts_it`: assert the existing behaviour with a failing
  notifier (no `digest_last_sent_at`) so a future change cannot regress it (add only if no test covers it:
  `grep -n "digest_last_sent_at" crates/domovoi-hub`).
- Existing tests to keep passing unchanged: `message_references_survive_restart`,
  `delivery_retries_then_records_failure_without_blocking_others`, `quiet_hours_release_*`,
  `failed_quiet_hours_summary_falls_back_to_held_cards`, `summary_sent_after_a_retry_needs_no_fallback_cards`.
  If one fails because it relied on state advancing before delivery, fix the test's expectation only when the
  new behaviour is the one this brief specifies, and say which test and why in the report.

## 13. Acceptance criteria

1. `check_state_not_saved_when_spool_push_fails`, `notification_state_written_only_after_success`,
   `notification_state_read_error_is_not_default_state` and `quiet_hours_release_not_saved_before_delivery` fail
   on the baseline and pass now (record each).
2. After a failed spool push, `checks.json` and the in-memory store show nothing from that run, and the next
   successful run emits the signals exactly once.
3. `Spool::push` returning an error leaves `len()`, `pending_after` and the next sequence number as before the
   call.
4. A notification state read error never produces a default state and never delivers a card; it is logged and
   counted.
5. Policy fields in `notify_state` change only after every action of the pass was confirmed; chat references are
   saved as soon as each post is confirmed and never advance policy fields.
6. A failed quiet-hours release leaves the incident held; a failed reminder leaves `last_reminder_at` unchanged.
7. `rg "unwrap_or_default" crates/domovoi-hub/src/runtime/delivery.rs` shows no use on a state read result
   (other than after `Ok(None)` inside `state()`).
8. The digest path is unchanged and still has a test showing the date is saved only after acceptance.
9. The R-4 finding is in the completion report and the matching sentence is in the book.
10. No change in `domovoi-core`, `domovoi-store`, `domovoi-proto`, `domovoi-engine`; no `#[allow]`; every file
    ≤ 500 lines; `cargo xtask arch-check` passes if run.

## 14. Migration and compatibility

- No data, schema, config or wire change. Existing `notify_state` rows and agent state files are read as before.
- Behaviour change visible to operators: a failed delivery no longer marks the alert as sent, so the same alert
  may be tried again at the next event or tick, and a duplicate message after a crash is documented. Agents that
  could not write their spool no longer lose a `firing` or `resolved` signal silently.
- Rollback: pure code rollback; nothing persisted differs.

## 15. Documentation requirements

- `docs/book/src/reference/agent.md` (spool section, `:46-53`): add that check state and the "was firing" memory
  are saved only after the run's signals are on disk in the spool; if the spool cannot be written, the run is
  repeated at the next interval and an error is logged.
- `docs/book/src/concepts/hub-and-agent.md` (the spool bullet at `:15`): one clause to match.
- A hub-side page that describes chat delivery (look in `docs/book/src/concepts/cards.md` and
  `docs/book/src/guide/discord.md`): a short "Delivery guarantees" paragraph: delivery is at least once; the hub
  records that a card was sent only after the chat platform confirmed it; a card that could not be delivered is
  tried again at the next change to the incident (or at the next minute for reminders and the end of quiet hours);
  a duplicate message is possible after a crash; use the R-4 sentence chosen in §6.6.
- `docs/book/src/reference/cli.md` metrics table: add `domovoi_notify_state_errors_total` (label `op`).
- `docs/book/src/troubleshooting.md`: an entry "Cards stopped appearing for one incident" pointing to the log line
  "could not read notification state" and the `op="read"` counter, and saying that a corrupt `notify_state` row
  for that incident has to be removed by restoring from a backup or deleting the row with the hub stopped.
  (State exactly that; do not invent a CLI command.)
- No `ARCHITECTURE.md` change unless a sentence there says state is saved before delivery.
- No OpenAPI or config reference change.
- Do not mention prompts or the build plan in committed files.

## 16. Execution constraints

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

## 17. Repository rules

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
- **Data and time.** Never edit a merged migration; add a new one. Time comes from the `Clock` port;
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

## 18. Completion report

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
