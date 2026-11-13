# 11 — Atomic incident writes: signal, incident and timeline in one commit

| | |
|---|---|
| Branch | `atomic-incident-writes` |
| Milestone | F1 |
| Depends on (must be merged) | 09 (which depends on 08) |
| Can run in parallel with | 10 (different engine modules; both add fields to hub wiring and test harnesses, so expect trivial rebase conflicts in `crates/domovoi-hub/src/runtime/mod.rs`) |
| Produces (shared contracts) | the incident manager's version cache rule ("always take an incident from `VersionedRead`") that prompt 34 and later writers of incidents must follow |
| Consumes | prompt 08: `UnitOfWork`, `Transaction`, `StateWrite::{Signal, Incident, IncidentEvent, DiscardPendingIncident}`, `Expected`, `VersionedRead`, `CommitReceipt.versions`, `CommitError`; the interim dual-write of timeline entries into `incident_events` and `journal` |
| Launch gate | none |
| Suggested model | medium |

## 1. Objective

Each command the incident manager handles produces a list of writes (signals, incident inserts and
updates, timeline entries, a discard of a pending incident). Today they are applied one at a time with
no transaction, so a crash or error part-way leaves, for example, an incident without its timeline
entry. After this change a command's writes are **one** `UnitOfWork` commit with an expected version for
every incident write. A conflict is retried once after reloading; a second conflict fails the command.
Nothing is published to the event bus unless the commit succeeded (existing rule).

## 2. Architectural context

- **Problem (RW-03, defect D06).** `IncidentManager::handle` loops over `decision.writes` and applies each
  through a different repository call; on failure it reloads the cache and returns an error, but the
  earlier writes stay (`crates/domovoi-engine/src/manager/actor.rs:142-150`). Only a signal and its
  incident link are atomic today.
- **Decision.** One commit per command. The pure `decide` function is unchanged: it still returns
  `Decision { writes: Vec<Write>, events, flaps }`. A new builder translates `Write` values into a
  `Transaction`.
- **Optimistic versions (RW-03).** `incidents` has a `version` column (prompt 08). The manager keeps a
  `version` for every incident in its cache and for any other incident it loads to write. A write names
  the version it read; the store increments it. Inside this single-writer aggregate, a conflict means a
  bug or an outside writer. Policy (04 section 5): reload the aggregate, record it, re-decide **once**; a
  second conflict fails the command with a store error.
- **One rule keeps versions honest:** never pair a body read through `IncidentRepo` with a version read
  through something else. Every incident the manager might write is read through `VersionedRead`, which
  returns body and version together.
- **Timeline: decided here (explicitly).** New timeline entries are written with
  `StateWrite::IncidentEvent`. Until prompt 28, the store **dual-writes**: each such write inserts the
  `incident_events` row (which every reader still uses: triage, delivery, the incident API) **and** a
  mirrored `journal` row (kind `incident.timeline`, carrying `legacy_event_seq`). The manager builds no
  journal draft for timeline entries; the store does the mirroring. Prompt 28 later copies old
  `incident_events` rows into `journal` skipping those already mirrored, then retires the old table.
  `incident_events` therefore keeps being written, by the unit of work, until that prompt.
- **No audit rows.** The incident manager writes no audit entries today and none are added (the audit
  log records security-relevant decisions; 04 section 3.4 lists none for incident signals).
- **Ownership.** The incident manager is the single owner of `Incident`, `Signal` and timeline writes
  (ADR 0011). Nothing else may build those `StateWrite` variants. (Arch-check scans for this come in a
  later prompt; do not add them here.)
- **Events after commit.** `decision.events` and metrics stay after the commit, as now.

## 3. Repository context

Verify each item in step 1. Paths relative to `C:\Programming\domovoi`.

- `crates/domovoi-engine/src/manager/actor.rs` (309 lines):
  - `ManagerDeps { incidents: Arc<dyn IncidentRepo>, signals: Arc<dyn SignalRepo>, acks: Arc<dyn AckRepo>,
    clock, redactor, policy, bus, shutdown }` (line ~29).
  - `IncidentManager { deps, active: Vec<Incident>, flaps }`.
  - `reload()` (line ~120): pages through `incidents.list` with `active: Some(true)`.
  - `handle(&mut self, command)` (line ~149): `state_for`, `decide`, then
    ```rust
    for write in &decision.writes {
        if let Err(err) = self.write(write).await { /* reload; return Err(store(err)) */ }
    }
    for write in &decision.writes { self.remember(write); }
    ```
    then flaps/metrics, then `for event in decision.events { let _ = self.deps.bus.send(event); }`.
  - `state_for` (line ~196): for `Ingest`, a cache miss calls `find_active_by_fingerprint` (and pushes the
    result into `self.active`) and `find_recent_resolved_by_fingerprint` (into `resolved`); for
    `RecordFollowUp` it reads `incidents.timeline`; for `AttachTriage`/`SetVerdict` on an incident not in
    the cache it reads `incidents.get` into `triaged`.
  - `write()` (line ~253): `InsertSignal` → `signals.insert`, `InsertIncident` → `incidents.insert`,
    `UpdateIncident` → `incidents.update`, `Timeline` → `incidents.append_event`,
    `DiscardPending` → `incidents.discard_pending`.
  - `remember()` (line ~268): updates the cache after a successful write (resolved incidents leave it).
- `crates/domovoi-engine/src/manager/decide.rs` (500 lines, **at the limit: do not edit**):
  `pub enum Write { InsertSignal(Signal, Option<IncidentId>), InsertIncident(Incident),
  UpdateIncident(Incident), Timeline(IncidentTimelineEntry), DiscardPending(IncidentId) }` (line ~42);
  `State { active, resolved: BTreeMap<Fingerprint, Incident>, acks, windows, triaged: Option<Incident>,
  timeline }`; `Decision { writes, events, flaps }`.
- Other `manager/` files: `annotate.rs` (triage and verdict timeline entries), `dismiss.rs`, `command.rs`
  (`Command`, `CommandError { UnknownIncident, NotAllowed, Store(String), Unavailable }`), `handle.rs`.
- Where `ManagerDeps {` is constructed (9 sites): `crates/domovoi-hub/src/runtime/mod.rs:192`,
  `crates/domovoi-hub/examples/dev_hub.rs:438`, `crates/domovoi-engine/tests/drills/world.rs:235`,
  `tests/engine/actor.rs:119`, `tests/engine/properties.rs:42`, `tests/services/actions_harness.rs:102`,
  `tests/services/harness.rs:141`, `tests/services/recovery.rs:121`, `tests/services/writer.rs:97`.
- `crates/domovoi-engine/tests/engine/actor.rs`: wrapper `Watched` implements `IncidentRepo`, fails writes
  and records whether anything was published before a write (tests `events_are_published_only_after_writes`
  at line ~170, `store_error_publishes_nothing` at ~190).
- Every reader of the timeline uses `IncidentRepo::timeline` (`incident_events`):
  `triage/queue.rs:183,245`, `triage/followup.rs:182`, `triage/incident_context.rs:127`,
  `triage/mod.rs:315`, `hub/src/api/incidents.rs:169`, `hub/src/runtime/delivery.rs:298`,
  `hub/src/auth/interactions/buttons.rs:101`. They must keep working unchanged.
- (08) `UnitOfWork::commit`, `Transaction::{new, write}`, `StateWrite`, `Expected::{New, Version}`,
  `CommitReceipt { versions: Vec<(AggregateRef, u64)> }`, `AggregateRef::incident(id)`, `VersionedRead::{
  incident, active_incidents}`, `Versioned<T> { value, version }`, `CommitError`. Verify exact names.
- Testkit: `InMemoryStore` implements all of those; `FakeClock`; `testkit::build`.
- Metrics: `FLAPS_METRIC` const in `actor.rs`; `crate::telemetry` (`engine/src/telemetry.rs`).

## 4. Exact scope

1. `ManagerDeps`: add `uow: Arc<dyn UnitOfWork>` and `versions: Arc<dyn VersionedRead>`; remove `signals`
   (it only served the signal write and becomes unused). Keep `incidents` (reads: `find_*`, `timeline`,
   `list`), `acks`, the rest.
2. New file `crates/domovoi-engine/src/manager/commit.rs` holding: the version cache type
   (`Versions`), `build_transaction`, the conflict counter metric name constant, and the commit-and-retry
   helper (section 6). `actor.rs` calls into it and must stay at most 500 lines.
3. `actor.rs`:
   - `reload()` uses `versions.active_incidents()` and fills both `active` and the version cache.
   - `state_for()` takes incidents it may write from `versions.incident(id)` (body and version together),
     never from `find_*` bodies: after `find_active_by_fingerprint` / `find_recent_resolved_by_fingerprint`
     returns an incident, call `versions.incident(found.id())` and use that value and version (drop the
     `find_*` body). `triaged` the same way. The versions of incidents placed into `State.resolved` and
     `State.triaged` go into a per-command map passed to the commit helper.
   - `ManagerHandle` (in `handle.rs`) gains the `write_conflicts` counter next to `flaps`.
   - `handle()` builds one `Transaction`, commits it, and on success updates the cache and the version
     cache from the receipt; then flaps, metrics and events exactly as before.
4. Wiring: update the nine `ManagerDeps` sites (drop `signals`, add `uow` and `versions`, both
   `store.clone()` of the same store the other fields use; in `actor.rs` tests, the wrapper).
5. Rewrite the test wrapper in `tests/engine/actor.rs` (section 12) and add the new tests.
6. Docs (section 15).

## 5. Explicit non-goals

- Do not edit `decide.rs`, `annotate.rs`, `dismiss.rs` or `command.rs` (decision logic and commands are
  unchanged); `handle.rs` changes only by the new counter, nor the `Command`/`CommandError` public API.
- No audit entries from the manager; no journal drafts for timeline entries (the store mirrors them).
- No change to the action writer, triage, delivery or any reader of the timeline (prompt 10 owns actions).
- No copy of old `incident_events` rows into `journal`, no retirement of `incident_events`, no SSE (prompt 28).
- No retention change for `journal` (prompt 57); say in the PR description that mirrored journal rows
  accumulate until retention for the journal exists.
- No advisory for write conflicts: advisory kinds are a closed set with a `CHECK` constraint. Conflicts are
  logged and counted by metric.
- No changes to `domovoi-core` types, the store, the testkit's `InMemoryStore` (except nothing), or the
  protocol.
- Do not change how `Store::purge` treats `incident_events` or signals.

## 6. Required interfaces and contracts

### 6.1 Version cache and transaction builder (**convention** for names, **fixed** behaviour)

```rust
// crates/domovoi-engine/src/manager/commit.rs

/// Versions of the incidents the manager has read, keyed by incident id.
#[derive(Debug, Default, Clone)]
pub(crate) struct Versions(BTreeMap<IncidentId, u64>);

impl Versions {
    pub(crate) fn get(&self, id: IncidentId) -> Option<u64>;
    pub(crate) fn set(&mut self, id: IncidentId, version: u64);
    pub(crate) fn remove(&mut self, id: IncidentId);
    pub(crate) fn clear(&mut self);
}

/// Translates `writes` into one transaction. `cached` is the manager's cache versions;
/// `loaded` holds versions of incidents read for this command only (reopened, triaged).
/// Within the transaction an incident written twice expects `version + 1` the second time;
/// an incident inserted earlier in the same transaction expects `Version(1)` when updated later.
pub(crate) fn build_transaction(
    writes: &[Write],
    cached: &Versions,
    loaded: &BTreeMap<IncidentId, u64>,
) -> Result<Transaction, CommandError>;
```

Mapping (**fixed**):

| `Write` | `StateWrite` |
|---|---|
| `InsertSignal(signal, incident)` | `Signal { signal, incident }` |
| `InsertIncident(incident)` | `Incident { incident, expected: Expected::New }` |
| `UpdateIncident(incident)` | `Incident { incident, expected: Expected::Version(v) }`, `v` = version tracked for that id (see above) |
| `Timeline(entry)` | `IncidentEvent(entry)` |
| `DiscardPending(id)` | `DiscardPendingIncident { id, expected: v }` |

`build_transaction` returns `CommandError::Store("no known version for incident <id>")` when an update
or discard names an incident with no known version (this signals a bug; the caller treats it like a
conflict: reload, re-decide once). The `Transaction` carries **no** journal drafts, audit drafts or
effects.

After a successful commit the cache update is:

- for each `(AggregateRef{kind: Incident, id}, new_version)` in `receipt.versions`: if the incident is
  now active (not `Resolved`) in the cache after `remember(write)`, `versions.set(id, new_version)`; if it
  left the cache (resolved) or was discarded, `versions.remove(id)`;
- an incident inserted and discarded in the same command ends with no entry.

### 6.2 Commit and retry (**fixed**)

```
loop attempt in 1..=2:
    state    = state_for(command)        // reads cache + store (VersionedRead for anything writable)
    decision = decide(state, command, policy, now)?     // pure
    tx       = build_transaction(decision.writes, versions, loaded)?  (error => treat as conflict)
    match uow.commit(tx):
        Ok(receipt)  => update cache and versions; flaps; metrics; publish events; return Ok
        Err(Conflict) if attempt == 1 =>
             warn!, metric DOMOVOI_INCIDENT_WRITE_CONFLICTS (counter), reload() then continue
        Err(Conflict) => reload(); return Err(CommandError::Store("the incident changed twice while it was being updated"))
        Err(other)    => reload(); return Err(CommandError::Store(other.to_string()))
```
`now` is read once before the loop (so both attempts decide at the same instant).
Metric: `pub const WRITE_CONFLICTS_METRIC: &str = "domovoi_incident_write_conflicts_total";` incremented
with `metrics::counter!` on every conflict, and mirrored in an `Arc<AtomicU64>` shared with the handle
exactly like `flaps` (`crates/domovoi-engine/src/manager/handle.rs:180-184`), exposed as
`pub fn write_conflicts(&self) -> u64` on `ManagerHandle` (doc comment: "Version conflicts the manager
retried since it started"). Nothing is published on any failed attempt.

Error texts: `CommandError::Store(String)` (existing variant). Callers see them as store failures that
can be retried (existing meaning).

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open everything in section 3.
   Confirm prompts 08 and 09 are merged; note the real names of `UnitOfWork`, `StateWrite`, `Expected`,
   `VersionedRead`, `CommitReceipt.versions`. Run `rg "ManagerDeps \{" crates` and `rg "signals" crates/domovoi-engine/src/manager`
   and confirm the nine construction sites and that `deps.signals` is used only in `write()`.
2. Write the new tests that fail on the old code first (section 12, **red first**).
3. Add `commit.rs` with `Versions`, `build_transaction` and unit tests for the mapping and the version
   bookkeeping (including double update in one command and insert-then-update).
4. Change `ManagerDeps` and `actor.rs`: `reload`, `state_for`, `handle` (section 6.2), `remember` plus
   version updates.
5. Wire the nine construction sites; `cargo build --workspace --all-targets`.
6. Rewrite the `Watched` wrapper in `tests/engine/actor.rs` for the unit of work; keep the two existing
   tests' assertions.
7. Run the broader engine and hub suites; fix harness fallout only.
8. Docs (section 15).
9. **Run the checks and write the completion report.** `cargo fmt --all`;
   `cargo clippy -p domovoi-engine -p domovoi-hub --all-targets -- -D warnings`;
   `cargo nextest run -p domovoi-engine -p domovoi-hub`; `cargo nextest run -E 'rdeps(domovoi-engine)'`;
   `cargo xtask arch-check`. In the report show `rg "incidents\.(insert|update|append_event|discard_pending)|signals\.insert" crates/domovoi-engine/src`
   with only test-support or no matches.

## 8. Database and persistence

No migration and no schema change (tables and columns come from prompt 08). Transaction boundary: one
`UnitOfWork::commit` per manager command, containing all of that command's signals, incident writes and
timeline entries. A commit writes each timeline entry to `incident_events` and mirrors it into
`journal` inside the same transaction (store behaviour from 08).

N-1 upgrade test: not applicable (no schema change); prompt 08's fixture test stays green.

## 9. Protocol and compatibility

No wire or HTTP change. Timeline readers keep reading `incident_events`, which is still written for
every new entry. The `journal` table gains one mirrored row per timeline entry from the first command
after upgrade. Rows already in `incident_events` are not mirrored until the copy in a later release.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Commit succeeds | cache and versions updated from the receipt; metrics; events published in order |
| Store returns `Backend` or `Constraint` | nothing published, no partial writes (the transaction rolled back), cache and versions reloaded from the store, `Err(CommandError::Store(message))`; the next command works |
| Version conflict on the first attempt | log `warn` with the incident ids, count the metric, reload, re-decide once, commit again |
| Version conflict on the second attempt | reload; `Err(CommandError::Store("the incident changed twice while it was being updated"))`; nothing published |
| `build_transaction` finds no version for an incident it must update | treated as a conflict (reload and one retry); a second failure returns the same store error |
| `Constraint` for a second active incident with one fingerprint (a stale cache missed one) | same as `Backend`: reload and error; the next command then sees the existing incident through the cache-miss path |
| Reload itself fails | logged at `error`; the manager keeps running with the old cache and retries reload on the next command (existing behaviour, `run()` comment) |
| Hub crash mid-command | the transaction either committed fully or not at all; on restart `reload()` reads incident versions from the store |
| Shutdown with queued commands | unchanged (queued commands are processed before exit) |

## 11. Security requirements

- No change to authorization: incident commands arrive on the manager's channel exactly as before; no
  new entry point.
- **Fail closed:** any commit error publishes nothing and leaves stored incident state exactly as it was.
- **Redaction (INV-5):** `redact_command` still runs before `handle` (unchanged); the transaction carries
  already-redacted data. A test checks that a secret in a signal summary is absent from the stored signal,
  incident title and timeline entry and from the mirrored journal payload (existing redaction tests in
  `tests/services/redact.rs` and `tests/engine` must still pass; add the journal assertion, test 8).
- **No secrets in logs:** the conflict warning logs incident ids only, never titles or evidence.
- The incident manager remains the only writer of incident state; no new writer is introduced.

## 12. Testing requirements

All in `crates/domovoi-engine/tests/engine/` unless stated (test 1 is in the hub crate). Fakes: `InMemoryStore` (implements
`UnitOfWork`, `VersionedRead`, `JournalRead`), `FakeClock`, the existing `Sig` helper in `harness.rs`.

Rewrite the wrapper `Watched` in `actor.rs`: it now implements `UnitOfWork` and `VersionedRead` by
forwarding to the inner `InMemoryStore`, checks on every `commit` that nothing was published yet, and
can be told to fail commits (`fail_writes`) or to return `CommitError::Conflict` for the next `n`
commits (`conflicts_remaining: AtomicU32`) **without applying anything**. `ManagerDeps.incidents` becomes
`store.clone()` (reads), `uow` and `versions` are the wrapper. Keep the two existing tests
(`events_are_published_only_after_writes`, `store_error_publishes_nothing`) with unchanged assertions.

Tests (name: behaviour; **red first** = fails on the old code):

1. `incident_write_is_atomic_on_sqlite` (in the **hub** crate, which depends on `domovoi-store`:
   new file `crates/domovoi-hub/tests/hub/atomic_incidents.rs`, registered in `tests/hub/main.rs`): open a real
   `Store` in a temp directory (`Store::open` with `StoreOptions::beside`), install a failing trigger through
   `store.pool_for_tests()`:
   `CREATE TRIGGER fail_events BEFORE INSERT ON incident_events BEGIN SELECT RAISE(ABORT, 'refused'); END;`,
   spawn `IncidentManager::spawn(ManagerDeps { .. })` over that store and a `FakeClock`, and ingest an
   immediate firing signal. Expect `Err(CommandError::Store(_))`. Then assert with raw SQL that `signals`,
   `incidents`, `incident_signals`, `incident_events` and `journal` have **0 rows** (the signal and incident
   inserts that ran before the failing event insert were rolled back), and that nothing was published.
   Drop the trigger, ingest the same signal again: it succeeds and creates one incident. This proves real
   atomicity; the in-memory double cannot.
2. `one_command_is_one_commit`: with a recording wrapper, ingest a firing signal that opens an incident
   (pending → open with timeline entries); exactly one `Transaction` was committed and it contains, in
   order, `Signal`, `Incident{New}`, one or more `IncidentEvent`; and a later `Ack` command is one commit
   with `Incident{Version(1)}` then `IncidentEvent`.
3. `timeline_entries_are_written_to_incident_events_and_mirrored_into_the_journal`: after the ingest in
   test 2, `IncidentRepo::timeline(id)` is non-empty and `JournalRead::entries_for(Incident(id))` has the
   same number of `incident.timeline` rows with matching `kind/summary/detail` payloads and the entries'
   original `at` as `occurred_at`.
4. `versions_follow_every_write`: ingest (version 1), ack (2), unack/mute (3); read
   `VersionedRead::incident` after each and compare with the manager's behaviour by asserting that
   the following command still commits (no conflict) and that the stored version equals the number of
   commits touching the incident.
5. `conflict_is_retried_once_after_reload`: wrapper returns `Conflict` for the first commit only; the
   command succeeds, exactly two commits were attempted (one failed, one applied), the stored state is
   that of a single application (no duplicate timeline entries), and `ManagerHandle::write_conflicts()`
   returns 1 (the same pattern as the existing `flaps()` counter test at `tests/engine/actor.rs:252`).
6. `second_conflict_fails_the_command_and_publishes_nothing`: wrapper conflicts twice; expect
   `Err(CommandError::Store(_))`, an empty event bus, unchanged store; the next command (no conflicts left)
   succeeds.
7. `outside_writer_makes_the_cache_stale_and_the_manager_recovers`: between two commands, bump the stored
   incident through `IncidentRepo::update` on the store (this advances the version, per prompt 08); the next
   `Ack` hits a conflict on the first attempt, reloads, and succeeds on the second with the outside
   writer's change preserved except for the acked status (assert the title/severity change survived).
8. `redacted_secret_never_reaches_incident_timeline_or_journal`: ingest a signal whose summary contains the
   configured secret (use the existing `PatternRedactor` setup from the redact tests); assert absence in
   the stored signal, incident title, timeline entries and the journal payloads.
9. `reopening_a_resolved_incident_updates_it_with_the_loaded_version`: resolve an incident, then ingest the
   same fingerprint inside the reopen window; the commit uses the version read through `VersionedRead` for
   the resolved incident (assert the stored version advanced and no conflict occurred).
10. `attaching_triage_to_a_resolved_incident_uses_its_stored_version`: incident resolves, then `AttachTriage`
    arrives for it (not in the cache); the update commits and the stored version advanced.
11. `discarding_a_pending_incident_is_one_commit`: pending incident resolves before confirmation; one commit
    with `DiscardPendingIncident`; incident gone, journal rows from earlier commits kept.
12. Unit tests in `commit.rs`: `maps_each_write_to_its_state_write`, `second_update_in_one_command_expects_the_next_version`,
    `update_after_insert_expects_version_one`, `missing_version_is_an_error`.
13. Existing suites: `tests/engine/*`, `tests/services/*`, `tests/drills/*`, the hub suites, must pass
    with only harness changes (nine construction sites).

Property tests in `tests/engine/properties.rs` keep running against `InMemoryStore` with the new deps.

## 13. Acceptance criteria

1. Each manager command performs exactly one `UnitOfWork::commit` containing all its writes in the
   order `decide` produced them; no manager code calls `IncidentRepo::{insert, update, append_event,
   discard_pending}` or `SignalRepo::insert` (verified by `rg` in the report).
2. A commit failure leaves signals, incidents, timeline and journal exactly as before and publishes nothing.
3. Every incident write carries its expected version; a version conflict triggers one reload and re-decide;
   a second conflict fails the command.
4. New timeline entries appear in both `incident_events` (still read by every reader) and `journal`
   (mirrored), in the same transaction; this is stated in `ARCHITECTURE.md` and the book.
5. All pre-existing engine and hub tests pass; the two original `actor.rs` tests keep their assertions.
6. The conflict metric and the `warn` log exist; no secrets or titles in that log line.
7. `cargo xtask arch-check` passes; no non-test file over 500 lines (`actor.rs` and `commit.rs` included);
   `decide.rs` is unchanged (`git diff --stat` shows no change).
8. `ManagerDeps` no longer has `signals`; all nine construction sites updated.

## 14. Migration and compatibility

- Existing installs: no data migration. Existing `incident_events` rows stay readable; they are not in the
  journal until the later copy. Incidents loaded at start get their stored `version` (1 for every existing
  incident, from prompt 08's migration).
- If a database is upgraded and the new binary starts, `reload()` fills the version cache from the store.
- Rollback to the previous binary: it ignores `version` and `journal` and keeps writing `incident_events`
  by itself; incidents it rewrites do not bump `version`, so after a roll-forward the manager may see a
  stale cache only if it was running concurrently (it is not); the first command after start reloads.
- Agents and protocol unaffected.

## 15. Documentation requirements

- `ARCHITECTURE.md`: section 7.1 (Hub runtime), incident manager paragraph: each command is one atomic
  commit through the unit of work; timeline entries go to `incident_events` and are mirrored into the
  `journal` until the old table is retired; versions and the retry rule. Section 9: note that
  `incident_events` is dual-written.
- `docs/adr/0011-single-writer-incident-manager.md`: append a short "Update" paragraph (do not rewrite
  the decision) describing the single commit per command, the version check and the one retry.
- `docs/book/src/reference/data-model.md`: under `incident_events`, one sentence that entries are also
  recorded in `journal`; under `incidents`, the `version` column in one sentence.
- No config change (`docs-config` not needed), no API change (no OpenAPI regeneration).
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
