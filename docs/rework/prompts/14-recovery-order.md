# 14 — Host availability, startup order and dispatch freshness

| | |
|---|---|
| Branch | `recovery-order` |
| Milestone | F2 |
| Depends on (must be merged) | 10 (`atomic-action-writes`: the action writer goes through the unit of work; this prompt edits the writer's `dispatch`, so it must see that code). Prompt 13 is **not** required; both cases are specified. |
| Can run in parallel with | 15, 16 |
| Produces (shared contracts) | `domovoi_core::ports::HostAvailability`, `AgentLink::availability`, `FakeAgentLink::set_availability` |
| Consumes | prompt 13's `HostRepo::agent_report` / `agent_server::protocol_state` when present (case A below) |
| Launch gate | none |
| Suggested model | medium |

## 1. Objective

Make a hub restart safe for approvals that were granted but not yet sent to an agent. Today, at start,
the executor immediately tries every `Approved` action; the agent has not reconnected, the hub's skill
list for that host is empty, and `revalidate` rejects a perfectly valid approval with "the skill is no
longer on the host" (defect 01 D36). After this change:

1. The hub reports one of four **host availabilities** for every host: `Known`, `Unknown`, `Absent`,
   `Incompatible`.
2. An approved action **waits** while its host is `Unknown` or `Absent`, and is dispatched only when the
   host is `Known`. It is never rejected merely because the host has not connected yet.
3. Right before dispatch the action is checked for **freshness**: the approval window has not closed
   (`expires_at`), the incident still exists and is not resolved, and the plan hash still matches the
   skill the agent has now.
4. A host the hub has rejected as `Incompatible` makes its waiting actions fail at once, without
   dispatch.
5. Startup runs in a fixed order (RW-30) on today's action path.

## 2. Architectural context

- **RW-30, startup recovery order** (decision):
  1. open the store, migrate, quick-check, verify the audit;
  2. load the writers (incidents, operations, conversations; here: the action writer);
  3. start the agent server: **every host is `Unknown`**;
  4. resume non-dispatch effects (timers, notifications; prompt 15, not here);
  5. `Dispatched` executions are reconciled per host when that host's `hello` arrives (RW-09; later prompt);
  6. `Approved` but undispatched actions are re-checked for freshness and dispatched only when the host
     becomes `Known`.
  Nothing is rejected because a host is still `Unknown`. A host reported `Incompatible` turns its waiting
  steps `Unavailable` with "agent upgrade required"; its history is kept.
- **Host availability** (RW-09, `04` §15.1):
  - `Known`: an agent session is live; its skill catalogue is current.
  - `Unknown`: no session since this hub started. Work waits.
  - `Absent`: there was a session since this hub started and it ended. Work waits up to the wait limit,
    then fails.
  - `Incompatible`: the hub itself rejected the agent's protocol after it authenticated (RW-17). Work
    fails at once with reason "agent upgrade required". (A host the hub only *predicts* to be incompatible
    after its own upgrade, "expected incompatible", is **not** `Incompatible`: it may reconnect fine, so it
    behaves like `Unknown`/`Absent`.)
- **Where the statuses come from on today's action path.** Operations and steps do not exist yet.
  `ActionStatus` is `Proposed, Approved, Denied, Expired, Dispatched, Rejected{reason}, Succeeded,
  Failed{reason}, Verified, VerifyFailed`. The target vocabulary (`NotRun`, `Refused`, `Unavailable`) maps to
  `Rejected { reason }` here, with fixed reason strings (§6.4). Never invent a new `ActionStatus`.
  `Dispatched → Failed` stays reserved for "was sent".
- **INV-4 (re-validate at dispatch)** is unchanged and still enforced by the writer
  (`ActionRequest::dispatch(plan_hash)`): the plan hash is recomputed from the skill the agent announces
  *now*. That only works when the host is `Known`, which is the whole point of waiting first.
- **The wait limit.** 04 names it `[orchestration] agent_wait_limit` (default 30 min); that config key does
  not exist yet. Here the existing `ActionSettings::offline_wait` plays that role and its default becomes
  30 minutes. It is not user-configurable in this prompt.

## 3. Repository context

Paths relative to `C:\Programming\domovoi`. Verify each before use.

**Engine, action service** (`crates/domovoi-engine/src/actions/`)
- `mod.rs:26-56` `ActionSettings { loop_guard_per_hour, offline_wait (5 min), presence_poll (5 s),
  verify_default_delay, dispatch_grace }`; `:66-83` `ActionDeps { actions, incidents, audit, link, redactor,
  clock, manager, bus, shutdown }`; `:104-117` `Shared { deps, settings, dispatches, running, writer }`;
  `:176-216` `ActionService::spawn`: reloads the loop guard (`recover::dispatch_history`) then
  `tokio::join!(writer::run(..), executor::serve(..))`.
- `writer.rs:95-103` `run` first calls `recover::fail_abandoned` (fails `Dispatched` leftovers with
  `RESTART_REASON`), then loops over commands. `:285-300` `dispatch(shared, id, plan)`:
  ```rust
  let mut action = load(shared, id).await?;
  let now = shared.deps.clock.now();
  let ActionChange { from, .. } = action.dispatch(plan, now).map_err(ApprovalError::Refused)?;
  let entry = match action.status().clone() {
      ActionStatus::Rejected { reason } => audit::rejected(&action, &reason, now),
      _ => audit::dispatched(&action, now),
  };
  shared.record_from(&action, from.name(), entry).await.map_err(|err| stale(err, id))?;
  ```
  `:303-324` `reject(shared, id, reason, blocked)`. (Prompt 10 changes how these write; read the merged code
  and keep the same structure.)
- `executor.rs:43-81` `serve` calls `catch_up` once at start and again after a lagged event bus; `:95-113`
  `catch_up` starts every `approved(shared)` action with at most `MAX_CATCH_UP_RUNS = 8` (`:90`) runs at
  once; `:48-ish` `start`. `:225-285` `run(shared, id)`: claim → load (must be `Approved`) → `revalidate` →
  `reserve_dispatch` (loop guard) → `PlanHash::compute` → `ask_dispatch` → **`wait_for_agent` (after the
  action is already `Dispatched`)** → `link.run_skill`. `:326-345` `revalidate` reads
  `shared.deps.link.skills(host)`. `:343-372` `wait_for_agent` polls `link.presence(host)` every
  `presence_poll` until `offline_wait`; on timeout the run completes the action as
  `Failed("agent offline")`.
- `recover.rs:164-175` `approved(shared)` lists `status = approved`; `:204-240` `fail_abandoned`.
- Tests: `crates/domovoi-engine/tests/services/recovery.rs` (`restarted(states)` builds a store holding
  actions left by the previous session, a `FakeAgentLink` whose host is already `Connected`, then spawns the
  service), `tests/services/execute.rs:243-280` `offline_agent_waits_then_fails` (asserts
  `Failed("agent offline")` after ≥ 5 min and `a.runs() == 0`), `tests/services/actions_harness.rs`
  (`actions(settings)`, `propose`, `until`, `settle`, `trail`, `runs`), `tests/gates.rs:539`.

**Core** (`crates/domovoi-core/src/`)
- `ports/agent_link.rs:68-84` `Presence { Connected{since, version}, Disconnected{last_seen} }`;
  `:102-135` `trait AgentLink` (`run_tool`, `run_skill`, `run_check`, `presence`, `skills`); `ports/mod.rs:17`
  re-exports.
- `action/request.rs:15-35` `ActionStatus`; `:282` `ActionRequest::expires_at()`; `action/transition.rs:67-90`
  `approve` (needs `now < expires_at`), `dispatch`, `reject` (`Approved → Rejected`).
- `incident/state.rs:17-31` `IncidentStatus { Pending, Open, Acked, Muted{until}, Resolved }`;
  `IncidentRepo::get(id) -> Result<Option<Incident>, _>` (used in `actions/recheck.rs`).

**Hub** (`crates/domovoi-hub/src/agent_server/`)
- `registry.rs:77-80` `Registry { hosts: Mutex<BTreeMap<HostId, Entry>>, .. }`, `Entry { live, last_seen,
  remote_ip, raised }` (`:60-66`), `track` (`:100`, called at start for every host, sets `last_seen` even with
  no session: **do not use `last_seen` to mean "had a session"**), `register` (`:107`), `end` (`:124`, called
  by `session.rs:67`), `close` (`:134`, called by `handshake.rs:114` and `mod.rs:235`), `presence` (`:210`).
- `link.rs:24-48` `WsAgentLink { registry, clock }` and `impl AgentLink for WsAgentLink` (`:110`);
  `skill_policy.rs:151-200` `PolicyLink` wraps another `AgentLink` and forwards `presence`/`skills`.
- `runtime/mod.rs:230` `ActionService::spawn(ActionDeps { link: policy_link.clone(), .. })`.

**Testkit** `crates/domovoi-testkit/src/link.rs:71-260` `FakeAgentLink` (`set_presence`, `set_skills`,
`on_skill`, `on_check`, `requests()`); default presence is `Disconnected { last_seen: None }`.

**Hub tests** `crates/domovoi-hub/tests/hub/` (`harness.rs` `Hub::start`, `agent.rs` `FakeAgent`, `online`,
`enroll`, `sessions.rs`), registered in `tests/hub/main.rs`.

**Book** `docs/book/src/concepts/actions.md:16,28,47`.

## 4. Exact scope

1. **Core:** `HostAvailability` enum and `AgentLink::availability` with a default implementation.
2. **Hub:** `Registry` remembers when a session ended; `WsAgentLink::availability`; `PolicyLink` forwards it.
3. **Testkit:** `FakeAgentLink::set_availability` override.
4. **Engine:**
   - new `actions/freshness.rs`;
   - executor `run` reordered: freshness → wait for host `Known` → revalidate → loop guard → dispatch;
   - writer `dispatch` also checks freshness;
   - startup order: the executor's catch-up waits until the writer finished its recovery pass;
   - catch-up concurrency cap moves from "runs started" to "runs past the host wait".
5. **Docs:** book `concepts/actions.md`.

Files expected to change (new **(new)**):
`crates/domovoi-core/src/ports/agent_link.rs`, `ports/mod.rs`; `crates/domovoi-hub/src/agent_server/{registry,
link,skill_policy,session,handshake,mod}.rs`; `crates/domovoi-testkit/src/link.rs`;
`crates/domovoi-engine/src/actions/{mod,executor,writer,recover}.rs`, `freshness.rs` **(new)**; tests in
`crates/domovoi-engine/tests/services/{recovery,execute,actions_harness}.rs`, a new
`crates/domovoi-engine/tests/services/freshness.rs` **(new)** registered in `tests/services/main.rs`,
`crates/domovoi-hub/tests/hub/availability.rs` **(new)** registered in `tests/hub/main.rs`;
`docs/book/src/concepts/actions.md`.

## 5. Explicit non-goals

- No `Dispatched` reconciliation on `hello`, no `OutcomeUnknown`, no `action_status` message (RW-09, prompts
  20/21). `recover::fail_abandoned` and `RESTART_REASON` stay exactly as they are. (The target design keeps
  a `Dispatched` action on an `Incompatible` host as "awaiting compatible agent"; that arrives with
  reconciliation. Do not implement it here.)
- No new `ActionStatus` variant, no new audit kind other than reusing `action.rejected`. No new wire
  message, no protocol change.
- No `Wake`/effects, no durable timers (prompt 15). `wait_for_host` is an in-task poll on the `Clock`, as
  `wait_for_agent` is today.
- No config key for the wait limit. No UI change.
- Do not change the loop guard rules, the chat/web approval paths, `expire_due`, or verification
  (`recheck.rs`) other than what the new `run` order needs.
- Do not touch `domovoi-proto` or agent crates.

## 6. Required interfaces and contracts

### 6.1 Core (**fixed**)

In `crates/domovoi-core/src/ports/agent_link.rs`, re-exported from `ports/mod.rs`:

```rust
/// How a host's agent stands for the purpose of dispatching work to it (RW-09, RW-30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAvailability {
    /// A session is live; `skills()` is the current catalogue.
    Known { since: Timestamp },
    /// No session since this hub started. Work waits.
    Unknown,
    /// A session existed since this hub started and has ended at `since`. Work waits, up to the limit.
    Absent { since: Timestamp },
    /// The hub rejected the agent's protocol after it authenticated. `reported` is the protocol
    /// the agent speaks, as `major.minor`. Work fails at once.
    Incompatible { reported: String },
}
```
Add to `trait AgentLink`:
```rust
/// What the hub knows about `host`'s agent right now. The default derives it from
/// [`presence`](Self::presence): connected is `Known`; disconnected after being seen is `Absent`;
/// never seen is `Unknown`. Implementations that can tell more override it.
async fn availability(&self, host: HostId) -> HostAvailability {
    match self.presence(host) {
        Presence::Connected { since, .. } => HostAvailability::Known { since },
        Presence::Disconnected { last_seen: Some(since) } => HostAvailability::Absent { since },
        Presence::Disconnected { last_seen: None } => HostAvailability::Unknown,
    }
}
```
Unit tests for the default in `domovoi-core` (`availability_default_follows_presence`).

### 6.2 Hub registry and link (**fixed** behaviour, **convention** names)

- `Entry` gains `absent_since: Option<Timestamp>`.
- `Registry::register` sets it to `None`.
- `Registry::end(host, session, at: Timestamp)` and `Registry::close(host, at: Timestamp)` set
  `absent_since = Some(at)` **only when they actually removed a live session**. Update the three callers
  (`session.rs:67`, `handshake.rs:114`, `mod.rs:235`) to pass `inner.deps.clock.now()`.
- ```rust
  /// What the registry knows about the host's session, for availability.
  pub(crate) enum SessionState { Live { since: Timestamp }, Lost { at: Timestamp }, Never }
  pub fn session_state(&self, host: HostId) -> SessionState
  ```
  (`Live` when `live` is set; `Lost` when `absent_since` is set; otherwise `Never`. A host the registry has
  never seen is `Never`. `last_seen` is not consulted.)
- `WsAgentLink::availability(host)` (override):
  1. `Live{since}` → `Known{since}`;
  2. else **case A (prompt 13 merged)**: if `AgentServer::protocol_state(host)` (the function that derives
     `ProtocolState` from the stored agent report; verify the name in code) is `Incompatible { reported, .. }`
     → `Incompatible { reported: reported.to_string() }`. `ExpectedIncompatible` is **not** mapped to
     `Incompatible`. The link needs the hosts repo to read it; prompt 13 already gives `WsAgentLink` a
     `hosts: Arc<dyn HostRepo>` for `LinkError::Incompatible`; reuse that helper. A store read error is
     logged at `warn` and treated as "not incompatible";
     **case B (13 not merged)**: skip this step. The `Incompatible` variant still exists and the engine
     handles it, but the hub never produces it yet. Say so in the completion report.
  3. else `Lost{at}` → `Absent{since: at}`; `Never` → `Unknown`.
- `PolicyLink` forwards `availability` to its inner link (`AgentLink::availability(&*self.inner, host).await`).
  Do not forget this: the engine is given the `PolicyLink`.

### 6.3 Testkit (**fixed**)

`FakeAgentLink::set_availability(&self, host: HostId, availability: HostAvailability)` stores an override map;
`availability()` returns the override when present, otherwise the trait default. `set_presence` keeps
working unchanged. Clearing: `clear_availability(host)`.

### 6.4 Engine freshness (**fixed**)

New file `crates/domovoi-engine/src/actions/freshness.rs`:
```rust
/// Why an approved action may no longer be dispatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stale { Expired, IncidentGone, IncidentResolved }

impl Stale {
    /// The reason stored in `ActionStatus::Rejected`.
    pub(crate) const fn reason(self) -> &'static str { match self {
        Self::Expired => "the approval expired before the action was dispatched",
        Self::IncidentGone => "the incident no longer exists",
        Self::IncidentResolved => "the incident was resolved before the action was dispatched",
    } }
}

/// The first freshness check an approved action fails, if any. `Err` when the store cannot
/// say: the caller leaves the action `Approved` and does not dispatch.
pub(crate) async fn check(shared: &Shared, action: &ActionRequest, now: Timestamp)
    -> Result<Option<Stale>, StoreError>;
```
Rules, in this order:
1. `now >= action.expires_at()` → `Expired`.
2. `deps.incidents.get(action.incident())`: `Ok(None)` → `IncidentGone`; `Ok(Some(i))` with
   `i.status() == IncidentStatus::Resolved` → `IncidentResolved`; any other status (`Pending`, `Open`,
   `Acked`, `Muted`) is open for this purpose; `Err(e)` → `Err(e)`.
3. Otherwise `Ok(None)`. The plan hash is checked by the writer's `dispatch` as today.

Executor reason strings (**fixed**), used verbatim in `Rejected { reason }` and tests:
- host wait limit passed: `"agent offline"` (unchanged text);
- host `Incompatible`: `"agent upgrade required: the agent speaks another protocol version"`.

### 6.5 Executor flow (**fixed**)

```text
run(shared, id, pool: Option<Arc<Semaphore>>):
  claim id (existing); load; if not Approved -> return (existing)
  1. now = clock.now(); match freshness::check(shared, &action, now)
        Err(e)        -> log error; return   (stays Approved; next start or lag-reload retries)
        Some(stale)   -> ask_reject(shared, id, stale.reason(), blocked=false); return
  2. match wait_for_host(shared, &action, started)     // started = clock.now() at step 1
        Known         -> continue
        Incompatible  -> ask_reject(... INCOMPATIBLE_REASON ...); return
        TimedOut      -> ask_reject(... "agent offline" ...); return
  3. let _permit = pool.acquire() (only for catch-up runs)
  4. freshness::check again (the wait may have been long); same handling as step 1
  5. revalidate (existing: skill present, risk unchanged, args valid)  -> ask_reject on Err (existing)
  6. reserve_dispatch (loop guard)                  (existing)
  7. plan = PlanHash::compute(..); ask_dispatch      (existing; the writer also runs freshness::check)
  8. run_skill, complete, verify                     (existing)  -- the old wait_for_agent is DELETED
```
`wait_for_host`:
```rust
enum HostWait { Known, Incompatible(String), TimedOut }
async fn wait_for_host(shared: &Shared, action: &ActionRequest, started: Timestamp) -> HostWait {
    let give_up = started + shared.settings.offline_wait;      // checked_add, fall back to started
    loop {
        match shared.deps.link.availability(action.host()).await {
            HostAvailability::Known { .. } => return HostWait::Known,
            HostAvailability::Incompatible { reported } => return HostWait::Incompatible(reported),
            HostAvailability::Unknown | HostAvailability::Absent { .. } => {}
        }
        let now = clock.now();
        if now >= give_up { return HostWait::TimedOut; }
        sleep_until(min(now + presence_poll, give_up))
    }
}
```
`started` for a catch-up run is the instant the run began in this hub session (step 1's `now`), not the
action's `approved` time: a hub that was down for an hour does not time its approvals out at start.

`ActionSettings::default().offline_wait` changes from 5 minutes to **30 minutes**; the doc comment says "the
longest an approved action waits for its host's agent before it is rejected".

Catch-up concurrency: `catch_up` no longer joins runs to stay under 8; it starts **every** approved action
(a waiting run is one sleeping task). It creates one `Arc<Semaphore>` of `MAX_CATCH_UP_RUNS` permits (store it
in `Shared` as `catch_up_slots: Arc<Semaphore>` so a second `catch_up` after a lag shares it) and passes
`Some(slots)` to `start`. Runs started from `ActionApproved` events pass `None`. The permit is taken at
step 3 (after the host is `Known`) and held until `run` returns. This keeps the original purpose (do not
flood one agent) and stops runs waiting on a missing host from starving runs for hosts that are up.

### 6.6 Writer freshness (**fixed**)

In `writer.rs::dispatch`, after `load` and before `action.dispatch(plan, now)`:
```rust
match freshness::check(shared, &action, now).await {
    Err(err) => return Err(ApprovalError::Store(err.to_string())),   // stays Approved
    Ok(Some(stale)) => { /* Approved -> Rejected{reason: stale.reason()}, audited with audit::rejected,
                            same record_from path as the plan-mismatch rejection; return the action */ }
    Ok(None) => {}
}
```
Use `action.reject(stale.reason().to_owned(), now)` (existing transition) and `audit::rejected`. The executor
sees `Rejected` in the answer and stops (existing code at `run`: `if let ActionStatus::Rejected {..}`).

### 6.7 Startup order inside `ActionService` (**fixed**)

`Shared` gains `recovered: tokio::sync::watch::Sender<bool>` (initially `false`). `writer::run` sends `true`
**after** `recover::fail_abandoned` finished (also when it failed: it logged the error), before it handles the
first command. `executor::serve` awaits
```rust
let mut ready = shared.recovered.subscribe();
let _ = ready.wait_for(|done| *done).await;   // returns early if the sender is dropped
```
before its first `catch_up`, selecting on `shutdown`. Order in the spawned task, written as a comment above
the `tokio::join!` in `ActionService::spawn`:
(1) loop-guard history reload, (2) `fail_abandoned` in the writer, (3) `catch_up` of `Approved` actions, each
waiting for its host. (Steps "open the store" and "start the agent server, every host `Unknown`" happen in
`runtime`, before `ActionService::spawn`; verify the order in `crates/domovoi-hub/src/runtime/mod.rs`:
`AgentServer::new` precedes `ActionService::spawn`, and the registry starts empty, so every host is
`Unknown` until its `hello`. Do not reorder it. If it is not so, stop and report.)

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file in §3. Check
   whether prompt 13 is merged (`grep -rn "fn protocol_state" crates/domovoi-hub/src`) and whether prompt 10
   changed the writer. Run `cargo nextest run -p domovoi-engine` once for the baseline.
2. **Write the failing tests first** (§12) that demonstrate D36 and the stale-approval hole on the baseline:
   `catch_up_waits_for_unknown_host` and `stale_approval_not_dispatched_after_restart`. Run them; record
   that they fail.
3. Core: `HostAvailability`, trait default, unit test.
4. Testkit: `set_availability`, `clear_availability`.
5. Hub: registry `absent_since`, `end`/`close` signature, `session_state`, `WsAgentLink::availability`,
   `PolicyLink` forwarding, callers.
6. Engine: `freshness.rs`; `Shared` fields (`recovered`, `catch_up_slots`); reorder `run`; `wait_for_host`;
   delete `wait_for_agent`; writer freshness; `serve`/`catch_up` changes; default `offline_wait`.
7. Update existing tests that encoded the old behaviour (§12 "Changed").
8. Hub availability tests; docs.
9. **Checks and completion report.** Run `cargo fmt --all`; for each of `domovoi-core`, `domovoi-testkit`,
   `domovoi-engine`, `domovoi-hub`: `cargo clippy -p <crate> --all-targets -- -D warnings` and
   `cargo nextest run -p <crate>`; `cargo nextest run -E 'rdeps(domovoi-core)'` (public port changed);
   `cargo xtask arch-check`; `cargo xtask docs-links`. Confirm the two step-2 tests now pass.

## 8. Database and persistence

Not applicable: no migration, no schema change. Availability is derived and in memory (the registry), plus,
in case A, the agent report row that prompt 13 owns. Action status transitions use the existing writer and
its existing audit rows (`action.rejected`).

## 9. Protocol and compatibility

Not applicable: no wire change. `AgentLink` gains one method with a default, so out-of-tree implementors
keep compiling.

Behavioural compatibility:
- An approved action for a host that is offline is now **`Rejected("agent offline")` and never
  `Dispatched`** after the wait limit (previously it was `Dispatched` then `Failed("agent offline")`). The
  audit shows `action.rejected`, no `action.dispatched`. The loop-guard slot is not consumed because the
  guard is reserved after the wait (step 6).
- Existing stored `Failed("agent offline")` actions are unchanged.
- The wait limit is 30 minutes instead of 5.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Hub restarts with `Approved` actions, host not reconnected | Run waits; action stays `Approved`; nothing rejected |
| Host connects within the limit | Next poll sees `Known`; freshness re-checked; skill list read; dispatched |
| Limit passes with host `Unknown` or `Absent` | `Rejected("agent offline")`, audited `action.rejected`, no dispatch |
| Host `Incompatible` | `Rejected("agent upgrade required: the agent speaks another protocol version")` immediately |
| Host `ExpectedIncompatible` (case A) | Not `Incompatible`; treated as `Unknown`/`Absent` (waits) |
| `expires_at` passed before dispatch (at start or after waiting) | `Rejected(Stale::Expired.reason())`, no dispatch |
| Incident resolved or deleted before dispatch | `Rejected(IncidentResolved / IncidentGone reason)`, no dispatch |
| Incident store read fails | action stays `Approved`; error logged; run ends; picked up at next start or lag reload |
| Skill changed while host was away (plan hash differs) | writer's `dispatch` rejects: "the plan changed after approval" (existing) |
| Skill removed while host was away | `revalidate` rejects: "the skill is no longer on the host" (existing; now based on a real catalogue) |
| Agent drops between `Dispatched` and `run_skill` | `run_skill` error → `Failed(err)` (existing) |
| Hub shuts down while waiting | task cancelled by `shutdown` (existing `select!` in `start`); action stays `Approved` |
| Writer queue closed while rejecting | `ask` logs at debug and returns (existing) |
| `Dispatched` leftovers at start | failed with `RESTART_REASON` before any catch-up (§6.7); unchanged text |

## 11. Security requirements

- INV-4 stays: the plan hash is recomputed from the catalogue the agent announces at dispatch; the writer
  remains the only place that moves `Approved → Dispatched`.
- Freshness checks fail **closed**: a store error never dispatches.
- Nothing in this change can approve, or re-approve, an action. `HumanApproval::new`/`restore` are not
  touched. A rejected-for-staleness action needs a fresh proposal and a fresh human approval.
- No secret is logged; reasons are fixed strings.
- A host's reported protocol (case A) is shown only in the `Incompatible { reported }` variant, never used to
  grant anything.

## 12. Testing requirements

Engine tests use `FakeClock` (advance, never sleep), `FakeAgentLink`, `InMemoryStore`, the existing harness.

**Harness additions (tests only).** In `tests/services/recovery.rs`, give `restarted` a sibling
`restarted_with(states, link_setup: impl FnOnce(&FakeAgentLink))` (the existing function calls it with the
current "connected, skills present" setup) so a test can start with the host `Unknown`.

**New tests** (`tests/services/recovery.rs` and new `tests/services/freshness.rs`)
- `catch_up_waits_for_unknown_host` (**fails on baseline**, proves D36): one `Approved` action; link has no
  presence and no skills (`Unknown`). After `settle()` the action is still `Approved`, `runs() == 0`, its audit
  trail has no `action.rejected`. Advance the clock 10 s, still `Approved`. Then
  `link.set_skills(host, [verified_restart()])` and `set_presence(Connected)`; advance `presence_poll`;
  `until Succeeded`; trail contains `action.dispatched`.
- `stale_approval_not_dispatched_after_restart` (**fails on baseline**): the action was proposed with a 1 h
  window at `T0` and approved at `T0`; the fake clock starts at `T0 + 2h`; host `Known`. Expect
  `Rejected("the approval expired before the action was dispatched")`, `runs() == 0`, trail ends
  `action.rejected`, no `action.dispatched`.
- `approval_for_resolved_incident_is_not_dispatched`: the incident is resolved in the store before the service
  starts → `Rejected` with the resolved reason.
- `approval_for_deleted_incident_is_not_dispatched`: no incident row → `Rejected(IncidentGone)`.
- `approval_expiring_while_waiting_for_host_is_not_dispatched`: settings `offline_wait = 3h`; host `Unknown`;
  approval window 1 h; advance 61 min in 5-minute steps (`presence_poll`), then connect the host → `Rejected`
  with the expired reason (the second freshness check, step 4).
- `plan_changed_while_host_unknown_is_rejected`: host connects with the same skill name but a different
  manifest hash (`set_skills` with a modified `SkillSummary`) → `Rejected("the plan changed after
  approval")`, `runs() == 0`.
- `skill_missing_after_host_connects_is_rejected_with_a_real_catalogue`: host connects with an empty skill
  list → `Rejected("the skill is no longer on the host")` (only after the host is `Known`).
- `unknown_host_past_wait_limit_is_rejected_without_dispatch`: default settings; advance 30 min + 1 poll →
  `Rejected("agent offline")`; `runs() == 0`; trail has no `action.dispatched`; loop guard unconsumed (a later
  approval of the same skill on a `Known` host is not blocked).
- `absent_host_that_returns_in_time_runs_the_action`: `Absent{since}` via `set_availability`; after 10 min
  `clear_availability` and connect → `Succeeded`.
- `incompatible_host_is_rejected_immediately`: `set_availability(host, Incompatible { reported: "1.0" })`;
  `Rejected("agent upgrade required: ...")` with **no** clock advance; `runs() == 0`.
- `waiting_runs_do_not_starve_other_hosts`: nine `Approved` actions on host A (`Unknown`) and one on host B
  (`Known`, own incident); host B's action reaches `Succeeded` while A's are still `Approved`. (Create a second
  host id with `HostId::new()` and its own `open_incident`.)
- `catch_up_dispatches_at_most_eight_at_once_once_hosts_are_known`: twelve `Approved` actions on a `Known`
  host with a skill handler that blocks until released (use a `tokio::sync::Notify` in `on_skill`): exactly 8
  `Skill` requests are in `link.requests()` until some finish.
- `startup_recovery_runs_before_catch_up`: store holds one `Dispatched` and one `Approved` action;
  assert (by audit sequence from `trail`, or `AuditLog::list` newest-first reversed) that the `Dispatched`
  action's failure row precedes the `Approved` action's `action.dispatched` row.
- `approved_action_is_still_started_by_the_event_path_without_the_pool`: an approval made after start on a
  `Known` host runs immediately even when the catch-up pool is exhausted.

**Changed tests**
- `tests/services/execute.rs::offline_agent_waits_then_fails` → `offline_agent_waits_then_is_rejected`:
  expects `Rejected { reason: "agent offline" }`, elapsed ≥ 30 min (`SignedDuration::from_mins(30)`), `runs() ==
  0`. Keep its second half (an agent that comes back in time runs the action).
- `tests/gates.rs:539` uses `SignedDuration::from_mins(5)`; read it. If it depends on `offline_wait`, update it
  to the new default; if it is unrelated, leave it. Do not weaken any gate.
- Any other test that left a host `Disconnected` and expected `Dispatched → Failed("agent offline")`: update to
  `Rejected`.

**Core unit test** `availability_default_follows_presence` (three cases).

**Hub tests** (`tests/hub/availability.rs`)
- `availability_is_unknown_until_first_session_then_known_then_absent`: enrolled agent, no session →
  `link().availability(host) == Unknown`; `online(..)` → `Known`; drop the `FakeAgent` and wait for the hub to
  notice (`closed()` + `pump` as in `tests/hub/sessions.rs`) → `Absent { since }` with `since` equal to the fake
  clock's now; reconnect → `Known`.
- `availability_is_unknown_after_hub_restart`: build a second `AgentServer` over the same store (copy the
  setup in `harness.rs`) → `Unknown`.
- `track_does_not_make_a_host_absent`: a host registered via `track` only (never a session) is `Unknown`.
- Case A only: `rejected_protocol_makes_host_incompatible` (a `1.0` agent is rejected after authenticating →
  `Incompatible { reported: "1.0" }`) and `expected_incompatible_host_is_not_incompatible` (row with protocol
  1.0, `incompatible_since: None`, hub 1.1 → `Unknown`).
- `policy_link_forwards_availability`: wrap a `FakeAgentLink` with `set_availability(Absent)` in a `PolicyLink`
  and assert it is returned.

## 13. Acceptance criteria

1. `catch_up_waits_for_unknown_host` and `stale_approval_not_dispatched_after_restart` fail on the baseline
   commit and pass after this change (record both results).
2. With a host `Unknown` or `Absent`, an `Approved` action is neither rejected nor dispatched until the host is
   `Known` or the 30-minute limit passes.
3. After the limit the action is `Rejected("agent offline")` with no `action.dispatched` audit row.
4. An `Approved` action whose `expires_at` has passed, or whose incident is resolved or gone, is `Rejected` with
   the fixed reason and never reaches the agent, whether it was found at start or became stale while waiting.
5. The writer's `dispatch` performs the same freshness check (a direct `WriteCommand::Dispatch` on an expired
   action is rejected).
6. `HostAvailability::Incompatible` makes the action `Rejected` at once; the variant exists and the engine
   handles it in both cases A and B.
7. `AgentLink::availability` exists with a default; `WsAgentLink`, `PolicyLink` and `FakeAgentLink` implement or
   forward it; `Unknown`/`Known`/`Absent` follow session history, not `last_seen`.
8. `fail_abandoned` runs before the first catch-up (`startup_recovery_runs_before_catch_up`).
9. Waiting runs do not block runs for other hosts, and a catch-up never has more than 8 skills in flight.
10. `wait_for_agent` no longer exists; no new `ActionStatus`; no wire, schema or config change.
11. `cargo xtask arch-check` passes; no new `#[allow]`; no source file over 500 lines (`executor.rs` is 417
    lines now: if it would exceed 500, move `wait_for_host` and `HostWait` into `freshness.rs` or a new
    `actions/host_wait.rs`).

## 14. Migration and compatibility

- No data migration. Existing `Approved` actions in a store are handled by the new rules at the next start.
- Existing installs: after upgrading, an action approved shortly before the restart now waits for its agent
  instead of being rejected; one whose approval window passed during the downtime is rejected with the
  expired reason (previously it would have run late).
- No config change. The wait limit becomes 30 min for everyone.
- Agents and the wire are untouched; mixed fleets are unaffected.

## 15. Documentation requirements

- `docs/book/src/concepts/actions.md`:
  - `:16` state diagram: add `Approved --> Rejected: the agent never came back, the approval expired, or the
    incident is closed` and keep `Dispatched --> Failed: non-zero exit or error`.
  - `:28` restart paragraph: say an approved action waits for its agent to reconnect (up to 30 minutes) and is
    checked again before it is sent: not expired, incident still open, the skill unchanged.
  - `:47` "An offline agent fails the action": replace with "An agent that is not connected delays the action,
    up to 30 minutes, then the action is rejected without being sent. A host whose agent speaks an
    incompatible protocol version rejects it at once."
- `ARCHITECTURE.md` §5.6 / §12: edit only a sentence that states the old five-minute rule or the old order,
  if one exists (`grep -n "offline" ARCHITECTURE.md`); say so in the report.
- No OpenAPI, no config reference change (run `cargo xtask docs-config` and confirm no diff).
- No mention of prompts or the build plan in any committed file.

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
