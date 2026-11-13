# 12 — Crash harness: crash sweep over every action and incident write, and the upgrade flow

| | |
|---|---|
| Branch | `crash-harness` |
| Milestone | F1 |
| Depends on (must be merged) | 10, 11 (and through them 08, 09) |
| Can run in parallel with | 13 (and, later, 14 to 19 once their own prerequisites are merged; none of them is needed here) |
| Produces (shared contracts) | `domovoi_testkit::crash::{CrashingStore, CrashPlan, CrashPoint, SweepStore, Reopen, InMemoryReopen, Snapshot, TxSummary, check_invariants}`; the sweep scenarios in `crates/domovoi-engine/tests/crash/`, reused by the SQLite run in the hub crate and by later milestones that add writes |
| Consumes | prompt 08: `UnitOfWork`, `Transaction`, `CommitError`, `CommitReceipt`, `JournalRead`, `VersionedRead`, `EffectRead`, the fixture `crates/domovoi-store/tests/fixtures/n1-schema5.db`; prompt 09: audit chain, `Store::verify_audit`; prompt 10: `ActionHandle::{propose, rotate_chat_token}`, the `action.transition` journal payload; prompt 11: the manager's single commit per command, mirrored timeline journal rows |
| Launch gate | none |
| Suggested model | medium: test infrastructure with clear invariants |

## 1. Objective

Prove the F1 guarantee with a machine: **a crash at any commit of any action or incident write never
leaves state without its audit and journal rows, and the system recovers.** Concretely:

1. A `CrashingStore` wrapper (in `domovoi-testkit`) that makes a chosen commit fail in one of two ways
   (the process dies before the commit, or after it but before the caller hears back) and then plays
   dead until restarted.
2. A restart helper and an id-agnostic state `Snapshot` and invariant checker.
3. A **sweep**: for each scenario (an incident lifecycle and an action lifecycle covering every kind of
   write), run it once to learn how many commits it makes, then re-run it once per commit and per crash
   point, restart, and check that the durable state is exactly a prefix of the committed sequence, that
   the invariants hold, and that the services work again. Run on the in-memory store (engine tests)
   and on real SQLite (hub tests).
4. The end-to-end **upgrade test**: the committed N-1 database (written by the code before this work)
   is opened by the current code, the services run against it, new writes are chained and journaled,
   audit verification passes, and the invariants hold after a restart.

## 2. Architectural context

- **What "crash" means with a unit of work.** Every business event is one `commit`. A process can die
  (a) before the commit starts or while it is not yet durable: nothing is applied; or (b) after the
  commit is durable but before the caller learns the outcome: everything is applied and the caller never
  sees the receipt. There is no third state; torn commits are impossible by construction (prompt 08,
  proved by its rollback tests). The sweep therefore tests the *callers*: after either outcome, restart
  and recovery must leave a consistent system, and the writers must not have relied on "my commit
  succeeded" for something else (events, metrics, caches, tokens).
- **Prefix property.** Let the scenario's committed sequence be `T1..Tn`. A crash at `Tk` before commit
  leaves the state of `T1..T(k-1)`; a crash after commit leaves `T1..Tk`. The sweep checks this exactly
  by comparing the post-crash snapshot with snapshots recorded in a fault-free dry run.
- **Snapshots are id-agnostic.** Ids and tokens are random; snapshots hold statuses, kinds and counts.
- **Where the code lives.** `domovoi-testkit` depends only on `domovoi-core` and `domovoi-proto`, so it
  cannot start engine services. The wrapper, snapshot and invariants live in testkit; the scenarios and
  the sweep live in `crates/domovoi-engine/tests/crash/` (in-memory store). The hub crate (which depends
  on both engine and store) includes the same scenario file with `#[path]` (precedent:
  `crates/domovoi-hub/tests/hub/main.rs` already includes `../gates.rs`) and runs the sweep on SQLite,
  reopening the database file as the restart.
- **Out of scope as a mechanism:** killing a real OS process mid-fsync, fault injection inside SQLite,
  agent-side crash windows (a later milestone), effect-runner crash windows (prompt 15 adds scenarios here
  for its writes when it merges).
- **The N-1 fixture is owned by an earlier prompt**: `crates/domovoi-store/tests/fixtures/n1-schema5.db`
  is a database written by the code before the rework (migrations 0001 to 0005), created and committed by
  prompt 08, which also tests the schema upgrade. This prompt reuses it for the end-to-end flow. Do not
  regenerate or replace it.

## 3. Repository context

Verify each item in step 1. Paths relative to `C:\Programming\domovoi`. Items marked (08..11) come from
those prompts: read the merged code for the real names.

- `crates/domovoi-testkit/src/lib.rs`: module list, `lock()` helper, re-exports; `src/repos/mod.rs`:
  `InMemoryStore` (`fail_next`); `src/clock.rs` `FakeClock`; `src/link.rs` `FakeAgentLink`
  (`set_presence`, `set_skills`, `on_skill`, `on_check`, `RecordingSignalSink`); `src/build.rs`
  (`T0`, `at`, `host`, `incident`, `open_incident`, `signal`, `action`, `manifest`);
  `src/approvals.rs` (`human`, `chat`, `web_typed`). Testkit `Cargo.toml` has `domovoi-core`,
  `domovoi-proto`, `async-trait`, `jiff`, `serde_json`, `thiserror`, `tokio` (`sync`, `time`, `macros`, `rt`),
  `uuid`; no engine, no store.
- `crates/domovoi-engine/tests/services/actions_harness.rs`: builds an `Actions` world (store, clock,
  `FakeAgentLink`, `ActionHandle`, event receiver, incident) around `ActionService::spawn`. Helpers:
  `verified_restart()`, `wipe()` (skill summaries), `ok_output`, `check`, `propose_for`, `until`, `settle`,
  `trail`. Copy the construction pattern; do not import across test binaries.
- `crates/domovoi-engine/tests/engine/harness.rs`: `Sig` (signal builder: `Sig::new(host, check, target)`,
  `.immediate()`, `.firing(secs)`, resolved variants); `tests/engine/actor.rs` for the manager world.
- `crates/domovoi-engine/src/manager/` (after prompt 11): `ManagerDeps { incidents, acks, clock,
  redactor, policy, bus, shutdown, uow, versions }`, `IncidentManager::spawn(deps) -> (ManagerHandle,
  JoinHandle)`, `Command` variants (`Ingest`, `Ack`, `Mute`, `Unmute`, `Dismiss`, `Tick`, `AttachTriage`,
  `SetVerdict`, `RecordToolUse`, `RecordFollowUp`, `ClearFollowUps`) in `command.rs`;
  `ManagerHandle` methods (`ingest`, `ack`, ...; read `handle.rs`).
- `crates/domovoi-engine/src/actions/` (after prompt 10): `ActionDeps { ..., uow }`,
  `ActionService::spawn(deps, settings) -> (ActionHandle, JoinHandle)`, `ActionHandle::{approve, deny,
  expire_due, propose, rotate_chat_token}`, `ActionSettings` (`offline_wait`, `loop_guard_per_hour`, ...),
  `writer::fail_abandoned` runs at writer start.
- (08) `UnitOfWork`, `Transaction { writes, journal, audit, new_effects, completed_effects }`,
  `StateWrite` variants, `JournalRead::{entries_after, entries_for, head_seq}`, `VersionedRead`,
  `EffectRead`; `JournalEntry { seq, aggregate, kind, payload_v, payload, actor, occurred_at }`.
- (09) `Store::verify_audit(VerifyScope)`, `VerifyReport`, `ChainStatus`, `LegacyStatus`.
- Hub tests: `crates/domovoi-hub/tests/hub/main.rs` (module list, `#[path = "../gates.rs"]`), `common` style
  in `tests/sqlite/common.rs` of the store (`Fresh`). The hub crate depends on `domovoi-store`,
  `domovoi-engine`, dev-depends on `domovoi-testkit` (verify in its `Cargo.toml`).
- Engine tests are one binary per directory with a `main.rs` listing modules
  (`tests/engine/main.rs`, `tests/services/main.rs`, `tests/drills/main.rs`); a new directory
  `tests/crash/` follows the same pattern. Check whether `Cargo.toml` needs `[[test]]` entries (it does not
  if the other directories need none).
- Store fixture (08): `crates/domovoi-store/tests/fixtures/n1-schema5.db`; its contents list is in
  `crates/domovoi-store/tests/sqlite/upgrade.rs` module comment and test (2 hosts, 1 agent, 3 incidents
  open/resolved/pending, 3 signals linked to the open incident, 4 incident events, 2 actions
  (proposed with chat token hash, approved), 5 audit rows, 2 notes, 1 setting).

## 4. Exact scope

**Testkit (`crates/domovoi-testkit/src/crash/`)** (new module, files each at most 500 lines; export from
`lib.rs` as `pub mod crash;`):
- `mod.rs`: `CrashPoint`, `CrashPlan`, `CrashingStore`, `SweepStore`, `Reopen`, `InMemoryReopen`,
  `TxSummary`.
- `snapshot.rs`: `Snapshot` and `Snapshot::capture`.
- `invariants.rs`: `check_invariants`.

**Engine tests (`crates/domovoi-engine/tests/crash/`)**: `main.rs`, `world.rs` (builds the services over a
`CrashingStore`, restarts them), `scenarios.rs` (the two scenarios; also included by the hub crate),
`sweep.rs` (the sweep driver, generic over `SweepStore`), `coverage.rs` (coverage and gate tests),
`memory.rs` (the in-memory run).

**Hub tests (`crates/domovoi-hub/tests/hub/`)**: `crash_sqlite.rs` (SQLite run: includes the engine's
`scenarios.rs`, `sweep.rs`, `world.rs` through `#[path]`, supplies a `SqliteReopen`), and `upgrade_flow.rs`
(the N-1 upgrade-then-operate test). Register both in `tests/hub/main.rs`.

**Docs**: `TESTING.md` section on the crash sweep; `ARCHITECTURE.md` section 17.

No product code changes. If you find a defect that makes a sweep case fail, do **not** fix it in this
change unless the fix is inside a test double; stop and report it, with the failing
case (scenario, commit index, crash point). Do not ignore or delete a failing case.

## 5. Explicit non-goals

- No product behaviour change in `crates/*/src` (the only non-test file additions are in
  `domovoi-testkit/src/crash/` and its re-export).
- No new fault model beyond the two crash points; no random fault injection; no killing processes.
- No agent-side journal or agent crash tests (a later milestone).
- No effect runner scenarios (prompt 15 extends the sweep when it adds writes).
- No changes to the committed fixture database; no regeneration.
- No change to `xtask`, CI workflows or arch rules.
- No performance benchmarks (the F1 commit-cost measurement was prompt 08's).
- Do not weaken or remove existing tests or gates.

## 6. Required interfaces and contracts

### 6.1 Testkit API (**fixed** names; bodies follow the behaviour below)

```rust
// crates/domovoi-testkit/src/crash/mod.rs

/// Where a simulated crash happens relative to one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    /// The process dies before the commit becomes durable: nothing is applied.
    BeforeCommit,
    /// The process dies after the commit is durable, before the caller sees the receipt.
    AfterCommit,
}

/// Which commit crashes: the `at_commit`-th non-empty commit (1-based) since arming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CrashPlan { pub at_commit: u64, pub point: CrashPoint }

/// What the storage under test must provide: the unit of work, its readers and the repository
/// ports the services read through. Implemented for `InMemoryStore` and (in the hub tests) `Store`.
pub trait SweepStore:
    UnitOfWork + VersionedRead + JournalRead + EffectRead
    + IncidentRepo + SignalRepo + ActionRepo + AuditLog + AckRepo + Send + Sync + 'static {}
impl<T> SweepStore for T where T: UnitOfWork + VersionedRead + JournalRead + EffectRead
    + IncidentRepo + SignalRepo + ActionRepo + AuditLog + AckRepo + Send + Sync + 'static {}

/// A short, id-free description of one commit, for coverage checks and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxSummary { pub writes: Vec<String>, pub journal: Vec<String>, pub audit: Vec<String> }
// writes: "signal", "incident:new", "incident:version", "incident_event", "incident:discard",
//         "action:new", "action:status:<name>", "note:new", "note:version"
// journal: JournalKind::as_str(); audit: AuditDraft kind string.

/// Wraps a store; the services' *commits* pass through it. Reads should go to the inner store
/// directly (hand the services `inner()` for repository ports and the wrapper for `uow`).
pub struct CrashingStore<S: SweepStore> { /* inner: Arc<S>, state behind a Mutex */ }
impl<S: SweepStore> CrashingStore<S> {
    pub fn new(inner: Arc<S>) -> Self;
    pub fn inner(&self) -> Arc<S>;
    /// Arms one crash. Replaces any earlier plan and resets the commit counter and the crashed flag.
    pub fn arm(&self, plan: CrashPlan);
    /// Disarms and resets the counter (a fault-free run that only records).
    pub fn record_only(&self);
    /// Number of non-empty commits attempted since arming or `record_only` (a crashed or refused commit counts).
    pub fn commits(&self) -> u64;
    /// Whether the planned crash has happened (the process is "down").
    pub fn crashed(&self) -> bool;
    /// One summary per attempted commit, in order.
    pub fn summaries(&self) -> Vec<TxSummary>;
    /// Marks the process as restarted: clears the crashed flag and the plan, keeps the recorded summaries.
    pub fn restart(&self);
    /// Starts recording a `Snapshot` of the inner store: one now (index 0, the state before any
    /// commit) and one after every commit the inner store applied successfully.
    pub async fn record_snapshots(&self) -> Result<(), StoreError>;
    /// The recorded snapshots, index i = state after the i-th commit (0 = before the first).
    pub fn snapshots(&self) -> Vec<Snapshot>;
}
#[async_trait] impl<S: SweepStore> UnitOfWork for CrashingStore<S> { /* behaviour below */ }
// also forwards VersionedRead, JournalRead, EffectRead to the inner store.

/// Reopens durable storage after a simulated crash.
#[async_trait]
pub trait Reopen<S>: Send + Sync {
    /// Closes `old` if the backend needs it and returns the store as a freshly started process
    /// would see it. In memory: the same instance. SQLite: close the pools and open the file again.
    async fn reopen(&self, old: Arc<S>) -> Arc<S>;
}
pub struct InMemoryReopen;   // impl Reopen<InMemoryStore>: returns `old`
```

`UnitOfWork::commit` on `CrashingStore` (**fixed**):

1. If the transaction is empty: forward to the inner store untouched (not counted).
2. If `crashed` is already true: return `Err(CommitError::Backend("process is down".into()))`, count it,
   record its summary, apply nothing.
3. Otherwise count it, record the summary. If the counter equals the plan's `at_commit`:
   - `BeforeCommit`: set `crashed`; return `Err(CommitError::Backend("simulated crash before commit".into()))`
     without calling the inner store.
   - `AfterCommit`: call `inner.commit(tx)`; whatever it returns, set `crashed` and return
     `Err(CommitError::Backend("simulated crash after commit".into()))`.
4. Otherwise forward to the inner store and return its result. If snapshot recording is on and the inner
   commit succeeded (including the `AfterCommit` case in step 3), capture and store a snapshot; a capture
   error is returned as `CommitError::Backend`.

### 6.2 Snapshot and invariants (**fixed** behaviour)

```rust
// snapshot.rs
/// Id-free picture of the durable state: sorted so equal states compare equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// (check key, status name) of every incident, sorted. Versions are checked by invariants, not snapshots.
    pub incidents: Vec<(String, String)>,
    /// (skill name, status name, has_chat_token) of every action.
    pub actions: Vec<(String, String, bool)>,
    /// Number of signals linked to an incident.
    pub signals: usize,
    /// (kind) of every timeline entry, sorted.
    pub timeline_kinds: Vec<String>,
    /// (kind) of every audit row, sorted.
    pub audit_kinds: Vec<String>,
    /// (aggregate kind, journal kind) of every journal row, sorted.
    pub journal_kinds: Vec<(String, String)>,
}
impl Snapshot {
    pub async fn capture<S: SweepStore>(store: &S) -> Result<Self, StoreError>;
}
```
`capture` pages through `IncidentRepo::list`, `ActionRepo::list`, `AuditLog::list`, `IncidentRepo::timeline`
for each incident, `JournalRead::entries_after`, and counts signals through
`SignalRepo::list_for_incident(id, MAX)` per incident (signals with no incident are not counted; the
scenarios always link them). It uses `Page::MAX_LIMIT` pages and follows cursors.

```rust
// invariants.rs
/// Checks the cross-table invariants of F1. Returns every violation found, not just the first.
pub async fn check_invariants<S: SweepStore>(store: &S) -> Result<(), Vec<String>>;
```
Invariants (**fixed**; each violation string starts with `I1:`, `I2:` or `I3:`):

- **I1 action state matches its journal.** For every action, the journal rows of aggregate `action:<id>`
  with kind `action.transition`, excluding rows whose payload has `change == "chat_token_rotated"`, are
  non-empty, and the **last** one has `payload.to` equal to the action's stored status name. Actions that
  were inserted outside the writer (fixture legacy rows) are exempt when they have **no** journal rows at
  all; the exemption set is the actions present in `exempt` (see `check_invariants_except`).
- **I2 audit matches journal.** For every action with journal transitions, the number of audit rows with
  subject `action:<id>` whose kind is one of `action.proposed, action.approved, action.denied,
  action.expired, action.rejected, action.blocked, action.dispatched, action.succeeded, action.failed,
  action.verified, action.verify_failed` equals the number of its transition rows (excluding rotations),
  and the number of `action.token_rotated` audit rows equals the number of rotation journal rows.
- **I3 timeline matches the mirrored journal.** For every incident that still exists, the number of
  `incident_events` entries (`IncidentRepo::timeline`) that were written through the unit of work
  equals its number of journal rows of kind `incident.timeline`. Entries older than the journal (fixture
  legacy rows) are exempt: pass the count of legacy timeline entries per incident in `exempt`.
  Also: no timeline entry, signal link or action refers to an incident that does not exist **except**
  actions (they may outlive discarded incidents; an action's incident always exists in the scenarios).

Provide `check_invariants_except<S: SweepStore>(store: &S, exempt: &Exempt) -> Result<(), Vec<String>>`
with `pub struct Exempt { pub actions: BTreeSet<ActionId>, pub legacy_timeline: BTreeMap<IncidentId, usize> }`
(`Exempt::none()`), and `check_invariants(store)` = `check_invariants_except(store, &Exempt::none())`.

### 6.3 The sweep (**fixed** algorithm; engine tests)

```rust
// crates/domovoi-engine/tests/crash/sweep.rs
pub struct Scenario<S> {
    pub name: &'static str,
    /// Builds a fresh world over a new store, runs every step, returns when quiescent.
    pub run: fn(&mut World<S>) -> BoxFuture<'_, ()>,
    /// Minimum number of commits the scenario must make (guards against a scenario that silently does nothing).
    pub min_commits: u64,
}

pub async fn sweep<S: SweepStore>(scenario: &Scenario<S>, make: &dyn Fn() -> BoxFuture<'static, Arc<S>>, reopen: &dyn Reopen<S>);
```
Algorithm:

```
// dry run
world = World::new(make().await); world.crashing.record_only(); world.crashing.record_snapshots().await
scenario.run(world)
snapshots = world.crashing.snapshots()
n = world.crashing.commits(); assert n >= scenario.min_commits
// snapshots[i] = state after commit i (i = 0..=n): obtained by re-running the scenario n+1 times is
// unnecessary: the dry run calls `crashing.record_snapshots()` first, so `crashing.snapshots()` holds
// snapshots[0..=n]; snapshots[0] is the empty state.
for k in 1..=n:
  for point in [BeforeCommit, AfterCommit]:
      world = World::new(make().await); world.crashing.arm(CrashPlan { at_commit: k, point })
      scenario.run(world)                      // steps may fail; the scenario ignores errors after the crash
      assert world.crashing.crashed()
      let after_crash = Snapshot::capture(inner)
      let expected = if point == BeforeCommit { snapshots[k-1] } else { snapshots[k] }
      assert_eq!(after_crash, expected, "scenario {name}, commit {k}, {point:?}")
      world.shutdown().await                   // cancel tokens and join service tasks
      store2 = reopen.reopen(inner).await ; world2 = World::restarted(store2)   // new services over the surviving state
      check_invariants(&*store2)               // I1..I3 after the crash, before recovery acts
      world2.recover().await                   // lets writer start-up recovery and manager reload run: see below
      check_invariants(&*store2)               // and after recovery
      world2.assert_services_work().await      // see below
```
The scenarios are deterministic given the `FakeClock` and scripted agent link, so the commit sequence of
re-runs equals the dry run's up to commit `k` (assert `summaries()[..k]` equals the dry run's summaries[..k]:
a scenario that is not deterministic is a test bug).

`World<S>` (in `world.rs`): holds `crashing: Arc<CrashingStore<S>>`, the `FakeClock`, `FakeAgentLink`,
the manager handle and action handle with their `JoinHandle`s and `CancellationToken`s, event receivers.
It builds `ManagerDeps` and `ActionDeps` with repository ports from `crashing.inner()` and `uow`/`versions`
from `crashing`. Methods: `new(store)`, `restarted(store)` (same construction over an existing store and a
fresh `CrashingStore` in record-only mode), `shutdown()`, `recover()` (advance the clock by 1 second and
send a `Tick`; await the action service's startup recovery by calling `expire_due()` once, which
only returns after the writer loop has started and finished `fail_abandoned`), and
`assert_services_work()`:
- ingest one new immediate signal for a **new** host; expect `Ok` and one more incident;
- `propose` a new action for an existing open incident (when one exists) and approve it with
  `testkit::approvals::human`; expect `Approved`;
- `check_invariants` again.

After a crash the services may still hold queued commands; `shutdown()` cancels the tokens and joins the
tasks with a bounded wait using the fake clock only (no real sleeps: poll `JoinHandle::is_finished` in a
loop with `tokio::task::yield_now()`, at most 10 000 iterations, then fail the test).

### 6.4 Scenarios (**fixed** coverage; step order is a convention)

`incident_lifecycle` (manager only, `min_commits` 14): using `Sig` and the `ManagerHandle`:
1. ingest an immediate firing signal for check A on host H (opens; commit: `signal`, `incident:new`,
   `incident_event`...);
2. ingest the same firing signal again (occurrence update; `incident:version`);
3. `ack`; `mute`; `unmute`;
4. `AttachTriage` with a triage report outcome (`TriageOutcome`; read `command.rs`/existing tests for a value);
5. `RecordFollowUp`, `SetVerdict(Fixed)`;
6. ingest a resolved signal for A (resolves);
7. ingest a firing signal for A again inside the reopen window (reopens a resolved incident);
8. `dismiss` it;
9. ingest a non-immediate firing signal for check B (pending), then a resolved signal for B before it
   confirms (`incident:discard`);
10. `Tick` after advancing the clock past a mute and the resolve TTL so `Tick` writes.
Every command is awaited; after a crash, remaining commands return `Err` and the scenario ignores them
(use a helper that treats `CommandError::Store`/`Unavailable` as "process down").

`action_lifecycle` (actions and the manager, `min_commits` 24): world with one open incident and a host
whose agent is `Connected` with skills `verified_restart` and `wipe` (reuse the definitions in
`actions_harness.rs`, copied), `ActionSettings` with `offline_wait` 0 and a small `verify_default_delay`:
1. `propose` action P1 (verified_restart) and P2 (verified_restart, other args or same skill);
2. `rotate_chat_token` on P1; approve P1 through chat with the rotated token, then let the executor
   run it to `Verified` (script the link: skill ok, check resolved);
3. `deny` P2;
4. `propose` P3 and let it expire (advance the clock beyond its window, `expire_due`);
5. `propose` P4 and approve it, script the link so the skill fails (`Failed`);
6. `propose` P5, approve, script the check so it still fails after the run (`VerifyFailed`);
7. `propose` P6 with a skill whose plan the link changes before dispatch (plan hash mismatch → `Rejected`);
8. `propose` P7, P8, P9 of one skill on one host with `loop_guard_per_hour: 2`: the third is `Blocked`
   (rejected by the guard);
9. leave P10 approved but with the agent offline and `offline_wait` 0 (executor fails it at dispatch).
Scripting the link/executor must follow what `tests/services/execute.rs` and `actions.rs` already do
(read them for the exact `FakeAgentLink` calls); `until(id, predicate)` helpers wait on events, never on wall time.

### 6.5 Coverage and gates (**fixed**)

In `coverage.rs`:
- `sweep_covers_every_kind_of_write`: union of `TxSummary.writes` over both dry runs contains `signal`,
  `incident:new`, `incident:version`, `incident_event`, `incident:discard`, `action:new`, and
  `action:status:<s>` for every `s` in `proposed, approved, denied, expired, rejected, dispatched,
  succeeded, failed, verified, verify_failed`, and the journal summaries contain `incident.timeline` and
  `action.transition`, and audit summaries contain `action.token_rotated`.
- `commit_sites_are_the_ones_the_sweep_exercises`: scan `crates/domovoi-engine/src` (excluding comments)
  for `.commit(`; the files that contain it must be exactly the set listed in a constant in the test
  (`actions/mod.rs`, `manager/commit.rs`, `manager/actor.rs`: adjust to the real files after prompts 10/11).
  A later prompt adding a commit site fails this test until it adds a scenario and extends the list: say
  that in the test's failure message.
- `snapshots_distinguish_states`: capture, change one status, capture again; not equal.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open everything in section 3.
   Confirm prompts 08 to 11 are merged; read the final names of the unit-of-work types, `ActionHandle`
   methods, `ManagerHandle` methods, `Command`, and the fixture contents. Confirm the commit sites with
   `rg "\.commit\(" crates/domovoi-engine/src`.
2. Testkit: `crash/mod.rs` (`CrashingStore`, `SweepStore`, `Reopen`, `TxSummary`) with unit tests
   (section 12, tests T1 to T6).
3. Testkit: `snapshot.rs`, `invariants.rs` with unit tests against hand-built stores (T7 to T11).
4. Engine tests: `world.rs`, `scenarios.rs` (get each scenario to run fault-free first and print its
   commit count), `sweep.rs`, `coverage.rs`, `memory.rs`.
5. Run the in-memory sweep. Investigate every failure: it is either a test bug (fix) or a product defect
   (stop and report; do not fix product code here).
6. Hub tests: `crash_sqlite.rs`, then `upgrade_flow.rs`.
7. Docs (section 15).
8. **Run the checks and write the completion report.** `cargo fmt --all`;
   `cargo clippy -p domovoi-testkit -p domovoi-engine -p domovoi-hub --all-targets -- -D warnings`;
   `cargo nextest run -p domovoi-testkit -p domovoi-engine -p domovoi-hub`;
   `cargo nextest run -E 'rdeps(domovoi-testkit)'`; `cargo xtask arch-check`. Report: the number of
   commits in each scenario, the number of sweep cases executed on each backend, and the wall time of the
   SQLite sweep. If the SQLite sweep takes more than 60 s on your machine, reduce the scenario sizes
   (not the crash points) and say so.

## 8. Database and persistence

No migration. The SQLite run opens real database files (temp directories) with `Store::open`, and the
restart closes and reopens them. The upgrade test copies the fixture into a temp directory before
opening it (the committed file must never be modified: open a copy; test
`fixture_file_is_not_modified` asserts the committed file's length and SHA-256 are unchanged by the test
run using a hash computed in the test, with the expected value read from the file at test start and
compared again at the end).

N-1 upgrade test: this prompt's `upgrade_flow.rs` **is** the F1 end-to-end upgrade test (section 12, U1 to U6).

## 9. Protocol and compatibility

None. No wire, API, config or schema change. Testkit gains a public module (dev-dependency only, per the
arch rule that testkit is never a normal dependency).

## 10. Error handling and recovery

Behaviour of the harness itself (the product behaviour it checks is the table in each earlier prompt):

| Situation | Behaviour |
|---|---|
| Planned crash commit never happens (plan beyond the commit count) | the sweep fails: `crashed()` must be true after the run |
| Scenario makes fewer commits than `min_commits` | sweep fails before any crash case |
| Re-run diverges from the dry run before the crash index | sweep fails with both summaries printed |
| Post-crash snapshot differs from the expected prefix | failure message names scenario, commit index, crash point, both snapshots |
| Invariant violation | failure lists every violation string |
| Service tasks do not stop in 10 000 yields | failure `"services did not stop"` |
| `CrashingStore` used after `restart()` | behaves as a plain pass-through counting commits again |
| Empty transaction through the wrapper | forwarded, not counted, never crashes |
| Backend reopen fails (SQLite file locked) | the test fails with the open error; the harness never swallows it |

## 11. Security requirements

- This change is test code plus a dev-only testkit module; it must not weaken any product check.
- The sweep asserts the F1 security properties: no approval, audit or journal row is lost or duplicated
  by a crash; no `Approved` action exists without its `action.approved` audit row (I2); a chat token
  rotation never leaves the stored hash and the audit/journal rows disagreeing.
- `HumanApproval` values in tests come only from `testkit::approvals`; `HumanApproval::new` is not called
  anywhere else (arch-check passes).
- No secrets in snapshots or failure messages (snapshots contain statuses and kinds only).
- The upgrade test asserts that audit history written before the rework still verifies (legacy digest)
  and that rows written afterwards are chained.

## 12. Testing requirements

**Testkit unit tests** (`crates/domovoi-testkit/src/crash/` `#[cfg(test)]` or `tests/crash.rs`):
- T1 `before_commit_crash_applies_nothing_and_plays_dead`: arm `(1, BeforeCommit)` on a `CrashingStore<InMemoryStore>`;
  commit a transaction with an `Incident New`: `Err(Backend("simulated crash before commit"))`, incident absent;
  a second commit fails with `process is down`; `crashed()` is true.
- T2 `after_commit_crash_applies_everything_and_hides_the_receipt`: arm `(1, AfterCommit)`: error returned, incident present.
- T3 `restart_makes_the_store_usable_again`: after T2, `restart()`; commit works; summaries keep earlier entries.
- T4 `only_the_planned_commit_crashes`: arm `(3, BeforeCommit)`: commits 1 and 2 succeed, the third crashes.
- T5 `empty_transactions_are_not_counted`.
- T6 `summaries_describe_each_commit_without_ids`: a transaction with signal, incident, event, action and a
  journal and audit draft yields the strings listed in 6.1.
- T7 `snapshot_is_independent_of_ids_and_order`: two stores with the same logical content built with different
  ids and insertion order produce equal snapshots.
- T8 `snapshots_distinguish_states` (also in coverage tests).
- T9 `invariants_accept_consistent_state` (build state through `UnitOfWork` commits shaped like the writers'),
  T10 `invariants_report_an_action_without_its_audit_row` (I2), T11 `invariants_report_a_timeline_entry_without_its_journal_row`
  (I3: write an `incident_events` row through `IncidentRepo::append_event` bypassing the unit of work) and
  `invariants_report_an_action_whose_journal_disagrees_with_its_status` (I1: update the action through the
  repository so its status moves without a journal row).

**Engine tests** (`crates/domovoi-engine/tests/crash/`):
- `incident_lifecycle_survives_a_crash_at_every_commit_in_memory` and
  `action_lifecycle_survives_a_crash_at_every_commit_in_memory`: run `sweep` with `InMemoryStore` and `InMemoryReopen`.
- `sweep_covers_every_kind_of_write`, `commit_sites_are_the_ones_the_sweep_exercises`, `snapshots_distinguish_states` (6.5).
- `scenarios_are_deterministic`: run each scenario twice fault-free; the summaries are equal.
- `audit_failure_rolls_back_transition` and `incident_write_is_atomic` (named in the F1 acceptance list) are
  **prompts 10 and 11's tests**; this prompt does not duplicate them but the report cross-references them.

**Hub tests** (`crates/domovoi-hub/tests/hub/`):
- `incident_lifecycle_survives_a_crash_at_every_commit_on_sqlite` and
  `action_lifecycle_survives_a_crash_at_every_commit_on_sqlite`: same sweep with `Store` and `SqliteReopen`
  (closes the pools via `Store::close` and calls `Store::open` on the same path with fresh `StoreOptions::beside`).
  After each case also run `Store::verify_audit(VerifyScope::Full)` on the reopened store: the chained history
  is `Intact` or `Empty` and legacy is `Verified` (a crash never breaks the chain).
- `upgrade_flow.rs`, the end-to-end upgrade test, with the fixture copied to a temp dir:
  - U1 `upgraded_database_opens_and_keeps_its_content`: `Store::open` at HEAD; row counts equal the
    fixture's; incident versions are 1; `quick_check` passed (open succeeded).
  - U2 `manager_updates_an_upgraded_incident_with_its_stored_version`: spawn the manager over the
    upgraded store; `ack` the fixture's open incident; success; stored version 2; a timeline entry exists in
    `incident_events` and as a mirrored `journal` row.
  - U3 `new_incident_after_upgrade_is_stored_and_journaled`: ingest a signal for a new fingerprint.
  - U4 `legacy_action_can_be_approved_after_upgrade`: approve the fixture's `proposed` action with
    `testkit::approvals::human`; expect `Approved`; one `action.approved` audit row exists that is **chained**
    (`prev_hash` equals the anchor hash; `seq` = 6) and one journal `action.transition` row with `from = "proposed"`.
    The fixture's `approved` action is picked up by the executor: script the link as offline with
    `offline_wait` 0 and assert it reaches a terminal status (`failed` or `rejected`) with a matching audit
    row (read the executor to confirm which; assert whichever it is and say so in the report).
  - U5 `audit_verifies_after_upgrade`: `verify_audit(Full)`: legacy `Verified{count: 5}`, chained `Intact`.
  - U6 `invariants_hold_after_upgrade_and_a_restart`: `check_invariants_except` with `Exempt` listing the
    fixture's two legacy actions (no journal rows) and each fixture incident's 4 or fewer legacy timeline entries
    (the per-incident counts read from the fixture before opening); then reopen (restart) and check again.
  - `fixture_file_is_not_modified` (section 8).

All tests are deterministic: `FakeClock`, `FakeAgentLink`, no sleeps, no network.

## 13. Acceptance criteria

1. `CrashingStore`, `SweepStore`, `Reopen`, `InMemoryReopen`, `Snapshot`, `TxSummary`, `check_invariants`
   (and `check_invariants_except`, `Exempt`) exist in `domovoi_testkit::crash` with the behaviour in 6.1 and 6.2,
   and tests T1 to T11 pass.
2. The sweep runs both scenarios over every commit and both crash points on the in-memory store and on
   SQLite, and every case satisfies: post-crash snapshot equals the dry-run prefix snapshot; invariants I1 to I3
   hold before and after recovery; the services work after restart.
3. `sweep_covers_every_kind_of_write` passes: every write kind and every action status written by the
   writers is exercised by at least one scenario commit.
4. `commit_sites_are_the_ones_the_sweep_exercises` passes and fails if a new commit site is added without
   updating the list.
5. The upgrade flow tests U1 to U6 pass against the committed N-1 fixture, which is byte-for-byte unchanged.
6. On SQLite, the audit chain is `Intact` (or `Empty`) after every crash case.
7. No product source file under `crates/*/src` changed except `crates/domovoi-testkit/src/lib.rs` (re-export)
   and the new `crates/domovoi-testkit/src/crash/`; `cargo xtask arch-check` passes; no non-test file over
   500 lines.
8. `TESTING.md` and `ARCHITECTURE.md` section 17 describe the sweep and how to extend it.
9. The completion report lists, for each scenario, the number of commits and crash cases, and cross-references
   the prompt 10 and 11 atomicity tests.

## 14. Migration and compatibility

No data, config, wire or API impact. The sweep becomes the pattern later milestones extend: every new
writer adds a scenario here and adds its commit site to the list checked by
`commit_sites_are_the_ones_the_sweep_exercises`. The N-1 fixture stays the base for later migration slices'
upgrade tests (each later migration slice adds its own assertions to an upgrade test; none may regenerate
the fixture).

## 15. Documentation requirements

- `TESTING.md`: a short section "Crash sweep" (what it proves, how to run only it:
  `cargo nextest run -p domovoi-engine -E 'binary(crash)'` and
  `cargo nextest run -p domovoi-hub -E 'test(crash)'`, how to add a scenario, how to read a failure message).
  Verify the nextest filter syntax against the repository's existing instructions in `TESTING.md`.
- `ARCHITECTURE.md` section 17 (Testing strategy): add the crash sweep and the upgrade-flow test to the list of
  test layers; mention `CrashingStore` in the testkit description.
- No config, API or book change. No OpenAPI or docs-config regeneration.
- No mention of prompts, the build plan, or coding agents in committed files.

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
