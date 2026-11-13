# 15 — Durable timers and the effect lifecycle with lease fencing

| | |
|---|---|
| Branch | `durable-timers` |
| Milestone | F2 |
| Depends on (must be merged) | 08 (`store-unit-of-work`: `UnitOfWork`, `Transaction`, the `effects` table, `EffectDraft`, `EffectCompletion`, `EffectRead`). Prompt 10 (`atomic-action-writes`) should also be merged: this prompt changes the action writer, and 10 is what makes it commit through the unit of work. If 10 is not merged, stop and report. |
| Can run in parallel with | 14, 16 (14 also edits `actions/executor.rs`, `actions/writer.rs` and `actions/mod.rs`: expect a textual merge, and re-read those files at the start) |
| Produces (shared contracts) | the effect lifecycle: `EffectQueue` port, `claim_due`, fenced reschedule and dead-letter inside `Transaction`, `revert_claimed_at_start`, `EffectRunner`, `EffectHandler`, backoff; the `Wake` effect for verification; `AdvisoryKind::EffectDead` |
| Consumes | prompt 08's `effects` table (lease columns already exist) and `EffectCompletion` |
| Launch gate | none |
| Suggested model | large: concurrency, a transactional contract and a store migration |

## 1. Objective

Make timers survive a restart, and give every later effect (model turns, notifications, dispatch) one
tested lifecycle.

1. **Effect lifecycle with lease fencing (RW-35).** Prompt 08 created the `effects` table and lets an owner
   create and complete effects. This prompt adds the rest: **claim** a due effect with a fresh token and a
   lease, **reclaim** it when the lease expires, **reschedule** with backoff, **dead-letter** it, and
   **revert claimed effects to pending at restart** (except `Dispatch`). Every completion, reschedule and
   dead-letter is conditional on the claim token and runs inside the owner's transaction, so a stale lease
   holder can never change state.
2. **A runner and a scheduler on the `Clock` port** that claims due effects and hands each to a handler by
   kind.
3. **Verification as a `Wake` effect.** Today `actions/recheck.rs` sleeps in memory for the verification
   delay (`recheck.rs:65`) and the re-check is lost on restart. After this change the action writer creates a
   `Wake` effect in the same transaction as the `Dispatched → Succeeded` transition; the runner fires it, runs
   the re-check, and the writer records `Verified`/`VerifyFailed` and completes the effect in one transaction.
4. **Dead effects raise an advisory** and never loop forever.
5. **Triage queue reload at start** (interim, until triage becomes an operation in M3): untriaged open
   incidents are queued again when the hub starts.

## 2. Architectural context

- **Effects are a work outbox, not an event store (RW-15).** An effect is a pending side effect written by
  the *owning writer* in the same transaction as the state change that requires it. The runner performs it
  and reports the result as an event to that same writer, which commits the state change and the effect's
  completion together. So "state says done" and "effect says done" cannot disagree after a crash.
- **Lifecycle (RW-35).**
  ```text
  pending --claim--> claimed --owner commit--> done
  claimed --retryable failure--> pending   (not_before = now + backoff(attempt))
  claimed --permanent failure or attempt limit--> dead
  claimed --lease expired--> reclaimable   (next claim: new token, generation + 1)
  claimed --restart--> pending             (new generation), except `dispatch`
  pending|claimed --owner cancels--> cancelled   (not built here)
  ```
  A claim sets: `claim_token` (fresh UUID), `claim_generation += 1`, `lease_until = now + lease`,
  `runner_id`, `attempt += 1`.
- **Fencing.** Complete, reschedule and dead-letter are
  `UPDATE effects ... WHERE id = ? AND claim_token = ? AND status = 'claimed'` inside the owner's
  transaction. Zero rows means the caller is a stale lease holder (its lease expired and another runner
  reclaimed the effect): the **whole** transaction rolls back (`CommitError::StaleEffect`), the stale
  result is discarded and logged, and no local state changes. Prompt 08 already implements this for
  completion; this prompt adds the same for reschedule and dead-letter.
- **Restart.** After a restart every `claimed` effect becomes `pending` with a new generation. The previous
  runner is gone, and if some zombie still holds the old token it is fenced out because the token is
  cleared. `Dispatch` effects are never reverted: whether the skill ran is decided by reconciliation with
  the agent (RW-09), not by re-running. (No `Dispatch` effects exist yet; the rule is implemented now so the
  lifecycle is final.)
- **Remote delivery is at-least-once.** A stale holder may already have sent something. That is prompt 16's
  topic for notifications; here it only means a handler must be idempotent or tolerate duplicates.
- **The verification re-check is read-only** (`run_check`), so running it twice is harmless. That is why
  `Wake`/verify can be retried and reclaimed freely.
- **No blind loops.** Retry classification: transport-level failures (`NotConnected`, `Timeout`,
  `Transport`) are retryable; everything else is a result or permanent. After `max_attempts` a retryable
  failure is dead-lettered (generic) or turned into a result (verification, §6.8).
- **Decisions already taken** (do not reopen): lease default 5 minutes; backoff exponential with a cap;
  `max_attempts` 5 for verification; a dead effect raises an advisory; the runner polls on the `Clock`, no
  threads sleeping on the system clock; tests advance `FakeClock`.

## 3. Repository context

Paths relative to `C:\Programming\domovoi`. **Prompt 08 and prompt 10 are merged when you run: read their
code first and use their actual names.** The names below for 08 come from its brief; the names for current
code are verified.

**From prompt 08 (verify in code)**
- `crates/domovoi-core/src/ports/uow/` : `Transaction { writes, journal, audit, new_effects,
  completed_effects }`, `EffectKind` (includes `Wake`, `Verify`, `Dispatch`), `EffectStatus { Pending, Claimed,
  Done, Dead, Cancelled }`, `ClaimToken`, `EffectDraft { id, kind, aggregate, payload, not_before,
  max_attempts, dedup_key, created_at }`, `EffectCompletion { id, claim_token: Option<ClaimToken>, at }`,
  `EffectRecord`, `EffectRead`, `CommitError::{Conflict, Constraint, StaleEffect{effect}, Backend}`,
  `AggregateRef`/`AggregateKind { Incident, Action, Note }`, `UnitOfWork::commit`.
- `crates/domovoi-store/src/uow/` (`mod.rs`, `effects.rs`, ...): SQLite `Store::commit`, fenced completion.
- `crates/domovoi-store/migrations/NNNN_unit_of_work.sql`: `effects(seq, id, kind, aggregate_type,
  aggregate_id, payload, status DEFAULT 'pending', not_before, attempt, max_attempts, dedup_key, claim_token,
  claim_generation, lease_until, runner_id, last_error, created_at, completed_at)`, indexes `effects_due
  (status, not_before, seq)`, `effects_aggregate`, partial unique `effects_dedup` on `dedup_key` while
  `pending` or `claimed`. The `status` column has **no** CHECK constraint.
- `crates/domovoi-testkit/src/repos/uow*.rs`, `src/contracts/uow/` (`UowSubject` trait and contract
  functions), `crates/domovoi-store/tests/sqlite/uow.rs`.
- Pools: write pool size 1; `BEGIN IMMEDIATE` on it.

**From prompt 10 (verify in code):** the action writer in `crates/domovoi-engine/src/actions/writer.rs`
commits each transition through `UnitOfWork`. Find the helper that builds the `Transaction` for a transition
(it replaced `Shared::record_from` at `actions/mod.rs:130-139`) and the new `ActionDeps` field that holds the
`UnitOfWork`.

**Current code that this prompt changes**
- `crates/domovoi-engine/src/actions/recheck.rs` (100 lines): `verify_delay(shared, skill, check)` (`:44-52`),
  `verify(shared, action, skill)` (`:56-100`):
  ```rust
  let now = deps.clock.now();
  deps.clock.sleep_until(now.checked_add(delay).unwrap_or(now)).await;   // :65  in-memory timer
  ...
  let recheck = deps.link.run_check(action.host(), incident.check().clone(),
                                    incident.target().cloned(), deadline).await;
  ```
  followed by `ask_verified(shared, action.id(), passed, summary)` and, when passed,
  `deps.manager.set_verdict(action.incident(), Verdict::Fixed)`. `RECHECK_DEADLINE` is 60 s.
- `executor.rs`: `run` ends (`:~270-281`) with
  `let ok = outcome.is_ok(); let _ = ask_complete(shared, id, outcome, Some(record)).await; if ok { verify(shared, &action, &skill).await; }`.
  `ask_complete` (`:~188`) sends `WriteCommand::Complete { id, result, record, reply }`.
- `writer.rs`: `WriteCommand::{Complete, Verified, ...}` (`:40-72`); `complete(...)` (`:328`) and `verify(...)`
  (`:355`) as in the prompt-10 version of the file.
- `mod.rs:176-216` `ActionService::spawn`: loop-guard reload, then `tokio::join!(writer::run, executor::serve)`.
- `crates/domovoi-engine/src/triage/queue.rs`: `serve` (`:66-96`) spawns workers and `retry_deferred`, then
  loops on events; `reload(shared)` (`:~199-240`) pages through active incidents, skips `Pending`, skips
  incidents that already have a `triage.report|cached|skipped|failed` timeline entry, and pushes the rest into
  `shared.queue`. It is called today only when the event bus lags (`:90`). `Shared.deferred` (retry list) is
  in memory (`triage/mod.rs:203`).
- Advisories: `crates/domovoi-core/src/advisory.rs:9-31` `AdvisoryKind`; `advisory_id(kind, host, subject)`
  (`crates/domovoi-engine/src/advisories.rs:~72`); `AdvisoryRepo::upsert` (`ports/repos/knowledge.rs:88-97`);
  table `advisories` has a `CHECK (kind IN (...))` list (`migrations/0004_settings_advisories.sql`, which
  rebuilt the table in the style `0003` used);
  `crates/domovoi-hub/src/runtime/schedulers.rs:~236-246` keeps event-driven advisory kinds from being
  auto-resolved by the periodic evaluator.
- Tests: `crates/domovoi-engine/tests/services/{execute.rs, actions_harness.rs, recovery.rs}` (verification
  and restart tests; `restarted(states)` seeds a store and then spawns the service),
  `tests/services/harness.rs` (`world(setup)` for triage), `tests/gates.rs`.
- Hub wiring: `crates/domovoi-hub/src/runtime/mod.rs:230` `ActionService::spawn(ActionDeps {..})`.

## 4. Exact scope

**A. Core** (`crates/domovoi-core`)
1. In the prompt-08 `uow/effects.rs` (or a new `uow/lifecycle.rs`, to stay ≤ 500 lines): `ClaimedEffect`,
   `ClaimBatch`, `EffectReschedule`, `EffectDeath`, the `EffectQueue` port (§6.1).
2. Extend `Transaction` with `rescheduled_effects` and `dead_effects` plus builder helpers; `is_empty`
   accounts for them.
3. `AdvisoryKind::EffectDead` ("effect_dead").

**B. Store** (`crates/domovoi-store`): SQL for the three fenced operations in `commit`; `EffectQueue` impl for
`Store` (`claim_due`, `revert_claimed_at_start`); migration for the advisory kind.

**C. Testkit**: in-memory `EffectQueue`; extend the `UowSubject` trait with `fn queue(&self) -> &dyn
EffectQueue`; contract suite `effect_lifecycle`; engine-side fakes where needed.

**D. Engine** (`crates/domovoi-engine`): new module `effects/` (`mod.rs`, `runner.rs`, `backoff.rs`),
`actions/verify_wake.rs` (the handler), writer changes, executor change, deletion of the in-memory sleep,
`ActionService::spawn` starts the runner, triage start-up reload.

**E. Hub** (`crates/domovoi-hub`): pass the store as the `EffectQueue`, keep `EffectDead` advisories from
being auto-resolved, raise the advisory (§6.7).

**F. Docs** (§15).

Files expected to change (new **(new)**): `ports/uow/*.rs`, `advisory.rs` (core); `crates/domovoi-store/
migrations/NNNN_effect_dead_advisory.sql` **(new)**, `src/uow/{mod,effects}.rs`, `src/uow/queue.rs` **(new)**;
testkit `src/repos/uow_queue.rs` **(new)**, `src/contracts/uow/lifecycle.rs` **(new)**, `tests/contracts.rs`;
engine `src/effects/{mod,runner,backoff}.rs` **(new)**, `src/actions/{mod,executor,writer,recheck}.rs`,
`src/actions/verify_wake.rs` **(new)**, `src/triage/queue.rs`, `src/lib.rs`; hub `src/runtime/{mod,
schedulers}.rs`; tests in §12; book pages in §15.

## 5. Explicit non-goals

- No `Dispatch`, `Reconcile`, `Notify`, `CallModel` or any other effect kind is *produced* here. Only `Wake`
  (verification) is. The lifecycle supports them; their prompts add them.
- No lease renewal call (handlers here finish well inside the 5-minute lease). Do not add `renew_lease`.
- No `cancelled` transition (needs operations, M3), no effect retention or pruning (prompt 57).
- No change to prompt 08's table columns or the fenced completion semantics; no edit to a merged migration.
- No change to the triage queue other than the start-up reload (no durable deferred retries, no
  rewrite of `retry_deferred`).
- No new `ActionStatus`, no new audit kind beyond what `Verified`/`VerifyFailed` already write, no wire or
  API change, no web change.
- Do not make notifications durable (prompt 16).
- Do not touch the host-availability logic that prompt 14 adds.

## 6. Required interfaces and contracts

### 6.1 Core port (**fixed names and behaviour**)

```rust
/// An effect this runner now holds, with the token that fences it.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaimedEffect {
    pub record: EffectRecord,      // status "claimed", attempt and generation already updated
    pub token: ClaimToken,
}

/// What one claim pass did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClaimBatch {
    /// Effects this runner now holds.
    pub claimed: Vec<ClaimedEffect>,
    /// Effects found past their attempt limit and moved to `dead` by this pass (record as stored).
    pub dead: Vec<EffectRecord>,
}

/// Reschedules a claimed effect for a later attempt (owner's transaction, fenced).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectReschedule { pub id: EffectId, pub claim_token: ClaimToken,
                              pub not_before: Timestamp, pub error: String, pub at: Timestamp }

/// Dead-letters a claimed effect (owner's transaction, fenced).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDeath { pub id: EffectId, pub claim_token: ClaimToken,
                         pub error: String, pub at: Timestamp }

/// Claims and recovers effects. Completion, reschedule and dead-letter are not here: they travel
/// inside the owner's [`Transaction`] so they commit with the state change.
#[async_trait]
pub trait EffectQueue: Send + Sync {
    /// Claims up to `limit` effects whose kind is in `kinds`, in this order of priority:
    /// `status = pending AND not_before <= now`, then `status = claimed AND lease_until <= now`
    /// (an expired lease; never kind `dispatch`). Ordered by `not_before`, then `seq`.
    /// For each selected row, in one transaction:
    ///  - if `attempt >= max_attempts`: set `status = dead`, `last_error = "attempt limit reached"`
    ///    (or "lease expired too many times" for an expired claim), `completed_at = now`, clear the
    ///    claim fields, and report it in `dead`;
    ///  - otherwise set `status = claimed`, `claim_token = <new v4>`, `claim_generation + 1`,
    ///    `lease_until = now + lease`, `runner_id = runner`, `attempt + 1`, and report it in `claimed`.
    async fn claim_due(&self, runner: &str, kinds: &[EffectKind], now: Timestamp,
                       lease: SignedDuration, limit: u16) -> Result<ClaimBatch, StoreError>;

    /// At start: every `claimed` effect whose kind is not `dispatch` becomes `pending` with
    /// `claim_token = NULL`, `claim_generation + 1`, `lease_until = NULL`, `runner_id = NULL`.
    /// `attempt` is kept (a crash counts as an attempt). Returns how many were reverted.
    async fn revert_claimed_at_start(&self) -> Result<u64, StoreError>;
}
```
`Transaction` gains `pub rescheduled_effects: Vec<EffectReschedule>` and `pub dead_effects: Vec<EffectDeath>`
with builder helpers `reschedule(EffectReschedule)` and `kill(EffectDeath)`. Order of application in
`commit`: `writes`, journal, audit, `new_effects`, `completed_effects`, `rescheduled_effects`,
`dead_effects`. Each is fenced:

```sql
-- reschedule
UPDATE effects SET status='pending', not_before=?1, last_error=?2,
       claim_token=NULL, lease_until=NULL, runner_id=NULL
 WHERE id=?3 AND claim_token=?4 AND status='claimed';
-- dead-letter
UPDATE effects SET status='dead', last_error=?1, completed_at=?2,
       claim_token=NULL, lease_until=NULL, runner_id=NULL
 WHERE id=?3 AND claim_token=?4 AND status='claimed';
```
Zero rows affected → `CommitError::StaleEffect { effect }` and rollback of everything. `claim_generation` is
not changed by these (it counts claims). The `EffectRecord` returned by readers keeps showing `attempt`,
`claim_generation` and `last_error` after a reschedule.

`AdvisoryKind::EffectDead` serialises as `effect_dead`; add it to the enum with a doc comment ("A background
effect failed permanently and was dead-lettered").

### 6.2 Backoff and settings (**fixed values**)

```rust
// crates/domovoi-engine/src/effects/backoff.rs
/// Delay before attempt number `attempt + 1`, after attempt `attempt` (1-based) failed:
/// `min(BASE * 2^(attempt-1), CAP)`.
pub fn backoff(attempt: u32) -> SignedDuration;   // BASE = 30 s, CAP = 1 hour
```
Table (unit-tested): attempt 1 → 30 s, 2 → 60 s, 3 → 120 s, 4 → 240 s, 5 → 480 s, 12 → 1 h (capped), 40 →
1 h (no overflow: saturate).

```rust
pub struct EffectSettings {
    pub lease: SignedDuration,        // 5 minutes
    pub poll: SignedDuration,         // 5 seconds: the runner wakes at least this often
    pub batch: u16,                   // 16 effects per claim pass
}
```
with `Default`. `ActionSettings` gets no new field; the action service builds `EffectSettings::default()`.

### 6.3 Runner and handlers (**fixed shape**, **convention** file layout)

```rust
// crates/domovoi-engine/src/effects/mod.rs
/// What a handler tells the runner.
pub enum EffectOutcome {
    /// The handler committed the effect's completion itself through the owner's writer.
    Handled,
    /// A transport-level failure: try again after the backoff.
    Retry { error: String },
    /// A permanent failure: dead-letter it.
    Dead { error: String },
}

#[async_trait]
pub trait EffectHandler: Send + Sync {
    /// Runs one claimed effect. A handler that finishes the work must complete the effect in
    /// the same unit-of-work commit as the state change, using `claim.token`; it returns
    /// `Handled` after that commit succeeded or was refused as stale.
    async fn handle(&self, effect: &EffectRecord, claim: ClaimToken) -> EffectOutcome;
}

pub struct EffectRunner { /* queue, uow, clock, advisories, handlers: BTreeMap<EffectKind, Arc<dyn EffectHandler>>,
                              runner_id, settings, nudge: Arc<Notify>, shutdown */ }
impl EffectRunner {
    pub fn new(deps: EffectRunnerDeps, settings: EffectSettings) -> Self;
    pub fn register(&mut self, kind: EffectKind, handler: Arc<dyn EffectHandler>);
    /// Wakes the runner now (called by owners right after creating an effect).
    pub fn nudge(&self);
    pub async fn run(self);   // until shutdown
}
```
`runner_id` is `format!("hub-{}", Uuid::new_v4())`, one per process start (`Uuid` is available in the engine
through core's `uuid`; if the engine does not depend on `uuid` directly, build the id from `ClaimToken::new()`).

`run` algorithm (**fixed**):
```text
1. queue.revert_claimed_at_start()      // log the count at info when > 0; an error is logged at error
                                          and the runner continues (leases still expire)
2. loop until shutdown:
   a. batch = queue.claim_due(runner_id, kinds_of(handlers), clock.now(), settings.lease, settings.batch)
        Err(e): log warn; wait poll; continue
   b. for each effect in batch.dead: raise_advisory(effect)          // §6.7
   c. for each claimed effect: spawn a task (JoinSet):
        outcome = handlers[kind].handle(&record, token)
        match outcome:
          Handled            -> nothing
          Retry{error}       -> if record.attempt >= record.max_attempts:  dead(error + " (attempt limit)")
                                else reschedule(not_before = now + backoff(record.attempt), error)
          Dead{error}        -> dead(error)
        where reschedule/dead commit a Transaction through `uow` with rescheduled_effects/dead_effects;
        CommitError::StaleEffect -> log at warn "stale lease holder discarded", nothing else;
        other commit errors -> log at error (the lease will expire and the effect is reclaimed);
        after a successful dead commit -> raise_advisory(record, error)
   d. if the batch was full, loop again immediately; otherwise
      select { shutdown, nudge.notified(), clock.sleep_until(now + poll) }
3. on shutdown: stop claiming; await in-flight handlers (they are bounded by their own deadlines)
```
A handler panic is not caught specially: the task's join error is logged at error, and the lease expiry
makes the effect reclaimable (after `max_attempts` it dies).

### 6.4 The verification `Wake` effect (**fixed**)

Effect created by the action writer when `Dispatched → Succeeded` is committed **and** a verification is
due:
```text
EffectDraft {
  id: EffectId::new(),
  kind: EffectKind::Wake,
  aggregate: AggregateRef::action(action_id),
  payload: {"v":1,"purpose":"verify_action","action":"<action id>"},
  not_before: completed_at + delay,
  max_attempts: 5,
  dedup_key: Some(format!("verify:{action_id}")),
  created_at: completed_at,
}
```
`delay` is `verify_delay(shared, skill, incident.check())` as today (`recheck.rs:44-52`): the skill's own
`verify.delay` when `recheck`, else `settings.verify_default_delay` for a write skill that applies to the
incident's check, else no effect.

`WriteCommand::Complete` gains a field `verify_after: Option<SignedDuration>` (the executor computes it before
sending, from the skill and the incident, exactly as `verify` did). The writer's `complete(...)` adds the
`EffectDraft` to the same `Transaction` as the transition and audit rows only when the outcome is `Ok(())` and
`verify_after.is_some()`. After a successful commit the writer calls `EffectRunner::nudge` through a handle
stored in `Shared` (`Shared.effects_nudge: Arc<Notify>`).

`WriteCommand::Verified` gains `effect: Option<(EffectId, ClaimToken)>`. When `Some`, `verify(...)` adds
`EffectCompletion { id, claim_token: Some(token), at: now }` to the same `Transaction`. A
`CommitError::StaleEffect` maps to a new `ApprovalError::StaleEffect` variant (add it; `thiserror`: "the
effect was claimed by someone else"); the handler treats that as `Handled` and logs at warn.

New `WriteCommand::FinishEffect { effect: EffectId, claim: ClaimToken, reply }`: a transaction holding only the
fenced `EffectCompletion` (no state change), used when the verification is no longer needed.

### 6.5 The verification handler (`actions/verify_wake.rs`, **fixed behaviour**)

```text
handle(effect, claim):
  payload = parse(effect.payload)        -> invalid or unknown purpose: Dead{"unreadable wake payload"}
  action  = deps.actions.get(id)         -> Err: Retry{err}   ; None: Dead{"the action no longer exists"}
  if action.status != Succeeded          -> FinishEffect (nothing to verify: already verified, or never ran); Handled
  incident = deps.incidents.get(action.incident())   -> Err: Retry ; None: FinishEffect; Handled
  result = link.run_check(action.host(), incident.check().clone(), incident.target().cloned(), now + RECHECK_DEADLINE)
  match result:
    Ok(r)  -> (passed = r.status == Resolved, summary = redact(r.summary))
    Err(NotConnected | Timeout | Transport(_)) when effect.attempt < effect.max_attempts
           -> return Retry{error: err.to_string()}
    Err(e) -> (passed = false, summary = format!("the re-check could not run: {e}"))   // final result
  reply = ask Verified{ id, passed, summary, effect: Some((effect.id, claim)) }      // one commit
  if passed and the commit succeeded -> manager.set_verdict(incident, Verdict::Fixed)  (as recheck.rs did)
  Handled
```
`RECHECK_DEADLINE` stays 60 s. The handler registers under `EffectKind::Wake`; payload purposes other than
`verify_action` are `Dead` for now.

### 6.6 `ActionService::spawn` and startup order (**fixed**)

`ActionDeps` gains `effects: Arc<dyn EffectQueue>` (and, if prompt 10 did not already add it, the
`UnitOfWork`; use its name). The spawned task: (1) loop-guard reload, (2) `join!(writer::run, executor::serve,
runner.run())`. The runner's first step is `revert_claimed_at_start`. `executor.rs::run` no longer calls
`verify`; `recheck::verify` and the sleep are **deleted**; `recheck.rs` keeps `verify_delay` and
`RECHECK_DEADLINE` (move them next to the handler if the file becomes tiny; do not leave dead code).

### 6.7 Dead-letter advisory (**fixed**)

`raise_advisory(record, error)`:
```rust
Advisory {
    id: advisory_id(AdvisoryKind::EffectDead, None, &record.id.to_string()),
    kind: AdvisoryKind::EffectDead,
    host: None,
    title: format!("A background task failed permanently ({})", record.kind),
    detail: format!("{} on {}: {} (after {} attempts)", record.kind, record.aggregate, error, record.attempt),
    created_at: now,
    resolved_at: None,
}
```
via `AdvisoryRepo::upsert` (engine deps get `advisories: Arc<dyn AdvisoryRepo>` in `EffectRunnerDeps`; the hub
passes the store). Never put payload contents in `detail`. `schedulers.rs` adds `AdvisoryKind::EffectDead` to
the kinds the periodic evaluator does not auto-resolve.

No migration for the advisory. Prompt 08 removed the `advisories.kind` CHECK constraint, so a new kind is
a Rust-only change: add the `AdvisoryKind::EffectDead` variant and its string mapping `effect_dead`. Verify
08's migration `NNNN_advisories_kind_unchecked.sql` is merged. If it is not, stop and report, because
08 is a prerequisite.

### 6.8 Triage start-up reload (**fixed**)

In `triage/queue.rs::serve`, after the workers are spawned and **before** the event loop, call
`reload_at_start(&shared).await`. It is the existing `reload` with one difference: only incidents with status
`IncidentStatus::Open` are queued (an `Acked` or `Muted` incident was already handled by a person or a rule,
and the maintenance-window flag is not stored, so the start-up pass must not triage on behalf of those). Keep the
lag-triggered `reload` unchanged. Implementation: give `reload` a parameter `only_open: bool`. Log at info
how many incidents were queued. Known limitation to document: deferred retries (`Shared.deferred`) are still
in memory, and an incident that opened inside a maintenance window and is still `Open` after a restart is
triaged once.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Read prompt 08's and 10's merged
   code (names in §3). Confirm the `effects` table has the lease columns and no CHECK on `status`; that
   `Transaction` has `completed_effects`; that the action writer commits through the unit of work; and
   whether prompt 14 changed `executor.rs`. Run `cargo nextest run -p domovoi-engine -p domovoi-store` for a
   baseline. Write down differences first; if prompt 08 or 10 is missing, stop.
2. **Core**: types, `EffectQueue`, `Transaction` fields and helpers, `AdvisoryKind::EffectDead`; unit tests
   (`Transaction::is_empty` with the new fields, advisory serde round trip `"effect_dead"`).
3. **Migration + store**: advisory migration; fenced reschedule and dead-letter in `commit`; `EffectQueue` for
   `Store` in `src/uow/queue.rs`. Keep each file ≤ 500 lines.
4. **Testkit**: in-memory `EffectQueue` and the new `Transaction` fields; `UowSubject::queue`; the
   `effect_lifecycle` contract suite; run it on both stores (add to `tests/contracts.rs` and
   `store/tests/sqlite/uow.rs`).
5. **Engine, effects module**: `backoff`, `EffectRunner`, `EffectHandler`, `EffectOutcome`; unit tests with
   fake queue/handlers (§12).
6. **Engine, actions**: `ApprovalError::StaleEffect`; `WriteCommand::{Complete.verify_after, Verified.effect,
   FinishEffect}`; writer; executor (compute `verify_after`, delete the `verify` call); `verify_wake.rs`;
   `ActionService::spawn` starts the runner; delete the in-memory sleep.
7. **Triage reload**.
8. **Hub wiring**: pass `effects`/`advisories`, `schedulers.rs`.
9. **Update existing tests** that exercised in-task verification (§12 "Changed").
10. **Docs**.
11. **Checks and completion report.** `cargo fmt --all`; `cargo clippy -p <crate> --all-targets -- -D warnings`
    and `cargo nextest run -p <crate>` for `domovoi-core`, `domovoi-store`, `domovoi-testkit`, `domovoi-engine`,
    `domovoi-hub`; `cargo nextest run -E 'rdeps(domovoi-core)'`; `cargo xtask arch-check`;
    `cargo xtask openapi` (no diff expected: say so if there is one); `cargo xtask docs-links`.

## 8. Database and persistence

- **Prompt 08's `effects` table is used unchanged.** No new column.
- **No migration.** The `effect_dead` advisory kind needs no schema change, because prompt 08 removed the
  kind CHECK (§6.7).
- **Transaction boundaries.**
  - Claim: one `BEGIN IMMEDIATE` transaction per `claim_due` call (select, then updates). Because the write pool
    has one connection, two runners in one process cannot interleave.
  - Complete, reschedule, dead-letter: inside the owner's `Store::commit`, with the state change and audit.
  - Revert at start: one statement.
- **N-1 upgrade test:** store test `advisories_migration_keeps_existing_rows`: open `tests/fixtures/n1-schema5.db`
  (prompt 08's fixture) or build a database with prior advisories, migrate, and check each advisory row (kind,
  host, title, resolved_at) is intact and a row of kind `effect_dead` can be inserted.

## 9. Protocol and compatibility

Not applicable: no wire change, no API change. The advisory kind appears in the existing advisories listing as
a new string value; the web UI renders advisories generically (verified: no web vocabulary lists the kinds);
confirm with `grep -rn "stale_image" web/src` and report if a vocabulary exists.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Hub restarts with a verification pending | the effect row survives; the runner claims it when `not_before` passes (immediately if already due); the re-check runs; `Verified`/`VerifyFailed` is committed with the completion |
| Hub restarts while an effect is `claimed` | `revert_claimed_at_start`: back to `pending`, new generation, token cleared; claimed again in the next pass |
| Runner crashes holding a claim | the lease expires; another claim pass (or the next start) reclaims it with a new token; the first holder, if it ever resumes, is fenced |
| Stale holder commits | `StaleEffect`; the whole transaction rolls back; logged at warn; no state change |
| Re-check cannot reach the host (`NotConnected`, `Timeout`, `Transport`) | `Retry`: `pending` with `not_before = now + backoff(attempt)`; at `attempt == max_attempts` the verification **ends as `VerifyFailed`** with "the re-check could not run: ..." (a result, not a dead letter) |
| Re-check rejected by the agent (`Rejected`) | `VerifyFailed` with the reason, effect completed |
| Action no longer exists / payload unreadable | `Dead`: effect `dead`, `EffectDead` advisory raised |
| Action is no longer `Succeeded` when the wake fires | `FinishEffect`: effect `done`, no state change |
| Incident gone | `FinishEffect` |
| Store error while claiming | logged at warn; retried after `poll` |
| Store error while committing retry/dead | logged at error; the lease expires and the effect is reclaimed |
| Effect past its attempt limit found at claim | moved to `dead` by the claim pass, advisory raised |
| Advisory upsert fails | logged at warn; the effect stays `dead` (no loop) |
| Hub shutdown with handlers in flight | handlers finish within their deadlines; unfinished claims revert at the next start |
| Start-up triage reload cannot read incidents | logged at warn; the event path still works |

## 11. Security requirements

- **Fencing is the security property here**: a stale lease holder must never commit state. The token is a
  random v4 UUID, cleared on every release, and every terminal or retry update compares it.
- Effects carry action ids only, never secrets, args or output; `last_error` and the advisory `detail` are
  redacted text (pass the re-check summary through the `Redactor` as `recheck.rs` does; error strings from the
  link contain no secrets, but run them through the redactor too).
- The runner cannot approve, dispatch or reject anything: the only state change it can cause is
  `Succeeded → Verified/VerifyFailed` through the writer, which still checks the transition rules.
- No `HumanApproval` construction anywhere in this change.
- Audit: `Verified/VerifyFailed` keeps writing its existing `action.verified`/`action.verify_failed` row in the
  same transaction as before. Dead-lettering is visible through the advisory, not the audit log (no actor).
- Fail closed: if a transaction cannot commit, nothing is half-applied.

## 12. Testing requirements

Use `FakeClock` (advance, never sleep), `InMemoryStore`, `FakeAgentLink` and the existing harnesses. New
items are **(new)**.

**Contract suite `effect_lifecycle`** (testkit, run on `InMemoryStore` and on SQLite via `UowSubject`)
- `claim_sets_token_generation_lease_and_attempt`: a due `Wake` effect; `claim_due("r1", [Wake], now, 5 min,
  10)` returns it `claimed`, `attempt == 1`, `claim_generation == 1`, `lease_until == now + 5 min`,
  `runner_id == "r1"`, token present; a second `claim_due` right away returns nothing.
- `claim_skips_effects_not_yet_due_and_other_kinds`.
- `claimed_effect_reclaimed_after_lease_expiry` (spec name): claim, advance 5 min + 1 s, `claim_due` by "r2"
  returns it with a **different** token, `claim_generation == 2`, `attempt == 2`.
- `stale_lease_holder_cannot_complete_reclaimed_effect` (spec name): after the reclaim, `commit` with
  `completed_effects = [token_of_r1]` plus a state write (a note upsert) fails with
  `StaleEffect`, **and the note write is not visible** (whole transaction rolled back); with `token_of_r2` it
  commits.
- `stale_holder_cannot_reschedule_or_dead_letter`: same for `rescheduled_effects` and `dead_effects`.
- `reschedule_returns_effect_to_pending_with_backoff_time`: status `pending`, `not_before` as given,
  `last_error` set, claim fields cleared, `attempt` unchanged; not claimable before `not_before`.
- `attempt_limit_dead_letters_at_claim`: `max_attempts = 2`; claim twice with reschedules; the third pass
  returns it in `ClaimBatch.dead` and the stored status is `dead`.
- `expired_lease_beyond_attempt_limit_dies_instead_of_reclaim`.
- `dispatch_effects_are_never_reclaimed_after_lease_expiry` and
  `restart_reverts_claimed_effects_except_dispatch`: `revert_claimed_at_start` returns the count; reverted rows
  are `pending` with generation + 1 and no token; a `dispatch` effect stays `claimed`.
- `dedup_key_blocks_a_second_pending_effect` (08's constraint) still holds across a reschedule.

**Engine unit tests** (`crates/domovoi-engine/src/effects/`)
- `backoff_doubles_to_the_cap` (table in §6.2) and `backoff_does_not_overflow_for_huge_attempts`.
- `runner_retries_with_backoff_then_dead_letters`: a fake `EffectHandler` that returns `Retry` each time; fake
  clock; after each claim pass the effect's `not_before` follows `backoff(attempt)`; at `max_attempts` it is
  `dead` and one `EffectDead` advisory exists.
- `runner_discards_stale_handler_result`: handler blocks; advance past the lease; a second claim pass
  reclaims; release the first handler with `Retry`; its `StaleEffect` is logged and the effect row is
  unchanged by it.
- `runner_reverts_claimed_effects_at_start`.
- `dead_effect_raises_advisory` (spec name): permanent failure → advisory with kind `EffectDead`, title and
  detail as §6.7, `detail` contains no payload text.
- `runner_wakes_on_nudge_before_poll`.

**Service tests** (`crates/domovoi-engine/tests/services/`)
- `verification_resumes_after_restart` (spec name, **fails on baseline**): seed a store with a `Succeeded`
  action, its incident, and a due `Wake` verify effect (as `restarted(...)` seeds actions); spawn the service
  with a `FakeAgentLink` whose check handler returns `Resolved`; the action reaches `Verified`, the effect is
  `done`, and the incident verdict is `Fixed`. A second case seeds the effect as `claimed` by a previous
  session (token and lease in the future): after start it still completes (revert at start).
- `verification_effect_is_created_with_the_success_transition`: run an action end to end on a fake link; read
  `EffectRead`: a `Wake` effect exists, aggregate `action:<id>`, `not_before == completed_at + verify delay`,
  `dedup_key == verify:<id>`, created **in the same commit** as `Succeeded` (use the fault-injecting store
  `fail_next` on the commit: when the commit fails, neither the status change nor the effect exists).
- `verification_waits_for_its_delay_without_a_task_sleeping`: after `Succeeded` no re-check request exists;
  advance the clock by the delay and `settle()`; then the re-check ran and the action is `Verified`.
- `verification_retries_when_agent_is_offline_then_gives_a_result`: the check handler fails with
  `NotConnected`; the effect is rescheduled with backoff; after `max_attempts` the action is `VerifyFailed`
  with "the re-check could not run" and the effect is `done`.
- `verification_skipped_when_action_is_not_succeeded`: effect for an action already `Verified` → effect `done`,
  no re-check request.
- `unreadable_wake_payload_is_dead_lettered`.
- `untriaged_incident_triaged_after_restart` (spec name, **fails on baseline**): seed an `Open` incident with
  no triage timeline entries, **then** spawn the triage service (mirror `world(setup)`; build the store first);
  a triage run is recorded (`triage.report` or the fake model's outcome) without any new signal. A second
  incident `Acked` and a third with `triage.report` already in its timeline are not triaged.
- `lag_reload_still_queues_acked_incidents` (unchanged behaviour of the lag path).

**Changed tests**
- Every test in `tests/services/execute.rs` that asserted verification within the same run now advances the
  clock past the delay and calls `settle()`; the expected statuses and audit trail are unchanged. Read each
  and update only the timing, never the assertions about outcomes.
- `tests/services/actions_harness.rs`: `actions(settings)` builds the extra deps (`effects`, advisories).
- `tests/gates.rs`: touch only if a gate counts something these changes affect; never weaken one.

**Store tests** `advisories_migration_keeps_existing_rows` (§8).

## 13. Acceptance criteria

1. `verification_resumes_after_restart` and `untriaged_incident_triaged_after_restart` fail on the baseline
   commit and pass now (record both).
2. `rg "sleep_until" crates/domovoi-engine/src/actions/recheck.rs` shows nothing: no in-memory verification
   timer remains.
3. `Dispatched → Succeeded` and the `Wake` effect are created in one commit (proved with an injected commit
   failure).
4. `Verified`/`VerifyFailed` and the effect's completion are one commit; a stale holder changes nothing
   (`stale_lease_holder_cannot_complete_reclaimed_effect` on both stores).
5. `claimed_effect_reclaimed_after_lease_expiry` and `dead_effect_raises_advisory` pass.
6. After restart every `claimed` effect except `dispatch` is `pending` with a new generation.
7. Backoff follows the table in §6.2; a retryable failure never exceeds `max_attempts` and never loops
   forever.
8. A start-up pass queues untriaged `Open` incidents only.
9. `AdvisoryKind::EffectDead` exists, is stored (migration), and the periodic evaluator does not resolve it.
10. No wire, API, web or config change; `cargo xtask arch-check` passes; no new `#[allow]`; every new public
    item documented; no file over 500 lines.
11. The unit-of-work contract suites from prompt 08 still pass unchanged on both stores.

## 14. Migration and compatibility

- **Existing databases:** no migration in this change. The `effects`
  table from 08 is empty until the first successful action with a verification.
- **Verifications in flight at upgrade:** an action `Succeeded` before the upgrade has no effect row, so it is
  never verified (same as today: a restart lost it). Not backfilled; say so in the report.
- **Behaviour change:** verification now happens through the runner; timing is the same (the skill's delay),
  plus up to one poll interval (5 s) unless the writer nudges the runner (it does, so in practice none).
- **Rollback:** an older binary ignores the unknown `effects` rows, and it does not
  know the `effect_dead` advisory kind, so it could not deserialise a stored advisory of that kind. Decision: **do not raise
  `MIN_READER`** (the release number is not known here, and a dead effect is rare). Instead state the
  limitation in the PR description and in the completion report: rolling back to a binary without this
  change after a dead effect was recorded can fail to list advisories until that advisory row is deleted.

## 15. Documentation requirements

- `docs/book/src/concepts/actions.md`: the verification paragraph: say the re-check is scheduled durably, so a
  hub restart between the run and the re-check does not skip it; say a re-check that cannot reach the host is
  retried with growing delays (30 s doubling to one hour, five attempts) and then recorded as "verification
  failed" with the reason.
- `docs/book/src/concepts/incidents.md` (or the triage section where it lives): a hub restart queues incidents
  that were open but never triaged.
- `docs/book/src/reference/data-model.md`: mention the `effects` table and its states only if the page lists
  tables (08 may have added it; keep it consistent).
- `docs/book/src/troubleshooting.md`: one entry for the `effect_dead` advisory: what it means and where to look
  (logs, the advisory detail).
- `ARCHITECTURE.md`: only if a sentence there says verification is in-memory; change that sentence only.
- No OpenAPI or config reference change; run `cargo xtask openapi` and `cargo xtask docs-config` and confirm
  no diff.
- No mention of prompts or the build plan in committed files.

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
