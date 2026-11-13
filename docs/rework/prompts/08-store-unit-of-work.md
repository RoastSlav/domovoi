# 08 — Unit of work: one atomic commit for state, journal, audit and effects

| | |
|---|---|
| Branch | `store-unit-of-work` |
| Milestone | F1 |
| Depends on (must be merged) | none |
| Can run in parallel with | 13 (it also adds a migration: see "Database and persistence", migration numbering) |
| Produces (shared contracts) | `domovoi_core::ports::{UnitOfWork, Transaction, StateWrite, JournalDraft, AuditDraft, EffectDraft, EffectCompletion, CommitError, CommitReceipt, VersionedRead, JournalRead, EffectRead}`; tables `journal`, `effects`, `schema_compat`; `version` columns on `incidents` and `notes`; the `UowSubject` contract suite in `domovoi-testkit` |
| Consumes | the existing repository ports, `Store`, `InMemoryStore` and the contract-suite pattern |
| Launch gate | none |
| Suggested model | large: it defines a contract that five later prompts build on and touches core, store and testkit |

## 1. Objective

Give the hub one way to change state atomically. A caller builds a `Transaction` holding typed state
writes (each with an expected version), journal entries, audit entries, new effects and effect
completions. The store applies all of it in one SQLite transaction or none of it, and returns either
a receipt or a typed `CommitError`.

Also in this change, because they ship together (RW-16, RW-31):

- The new tables `journal`, `effects` and `schema_compat`, and `version` columns on `incidents` and
  `notes`.
- `synchronous=FULL` on every connection, and a `quick_check` at startup.
- A start-up refusal when the database says it needs a newer binary.
- A shared contract suite that runs against both `Store` (SQLite) and `InMemoryStore`.

**No caller is migrated here.** The incident manager, the action writer, triage and the hub keep
writing through the repository ports. Prompts 09 to 12 move them over, one at a time.

## 2. Architectural context

Plain-language summary of the decisions this prompt implements (you do not need to open the planning
documents).

- **Why.** Today the action writer updates an action and then appends the audit row as a separate
  statement whose failure is only logged. The incident manager writes signal, incident and timeline
  entries one at a time. A crash between statements loses audit rows or leaves a timeline without its
  incident (RW-03, defect D06). The fix is one transaction per business event.
- **The port lives in `domovoi-core`; the SQLite implementation in `domovoi-store`.** Core does no
  I/O, so core holds only types and the trait. `InMemoryStore` in `domovoi-testkit` is the second
  implementation and must behave identically (proved by one shared contract suite).
- **One writer connection.** The store has a write pool of exactly 1 connection and a read pool of 4
  (existing, `db.rs`). Every write goes through that one connection, so `BEGIN IMMEDIATE` on it gives
  mutual exclusion, and the journal's `AUTOINCREMENT` sequence equals commit order. If that pool size
  ever changes, sequence assignment must change (RW-15); leave a doc comment saying so.
- **Optimistic versions.** `incidents` and `notes` get a `version` column. A write names the version
  it read. If the stored version differs, the whole transaction rolls back with
  `CommitError::Conflict`. Inside a single-writer aggregate a conflict means a bug or an outside
  writer; the caller decides (prompts 10 and 11). Actions already have an optimistic guard: the
  stored status name; keep it (ADR 0019) instead of adding a version column to `actions`.
- **Ownership is not enforced by the port.** The port does not decide who may write which aggregate.
  Each `StateWrite` variant is meant to be built only by the aggregate's owning writer. Document that
  in rustdoc. Do not change `xtask` arch rules (changing `xtask/src/arch/rules.rs` needs an ADR);
  a later prompt adds scans.
- **Journal vs audit vs effects (RW-15).** `journal` is the operational experience record and the
  future change stream: typed kinds, global `seq`, written by the owning writer in its transaction.
  `audit_log` stays the separate security record (append-only triggers, existing). `effects` is the
  work outbox (pending side effects), not an event store. This prompt only creates the tables and the
  write paths; prompt 15 implements the effect lifecycle (claim, lease, retry, dead-letter) and
  prompt 28 turns the journal into SSE.
- **Interim dual-write of incident timeline entries (decided here, used by prompt 11).** Readers of
  the timeline (triage, delivery, the incident API) still read `incident_events`. Prompt 28 later
  copies `incident_events` into `journal` 1:1 and retires the table. Until then,
  `StateWrite::IncidentEvent` makes the store write **both** the `incident_events` row and a mirrored
  `journal` row in the same transaction. The journal row stores the `incident_events.seq` in a
  nullable `legacy_event_seq` column, so prompt 28's copy can skip rows already mirrored. The caller
  never builds a journal draft for timeline entries.
- **Durability (RW-16).** `synchronous=FULL` ships now so approvals and audit survive power loss from
  F1 on. Cost: one extra fsync per commit. R-6 asks for a measurement; you add an ignored benchmark
  (below), not a gate.
- **Rollback safety (RW-31).** A `schema_compat` row records the lowest binary version that can read
  the schema. A binary older than that refuses to start; a binary that knows fewer migrations than
  the database ignores the unknown ones instead of failing.
- **Actor.** Journal and audit entries will carry an `Actor` enum (RW-29), introduced by prompt 09.
  Until then actors are strings in today's audit format (`user:<id>`, `system:<component>`).

## 3. Repository context

Verify each item in step 1. Paths are relative to `C:\Programming\domovoi`.

**Core ports.**
- `crates/domovoi-core/src/ports/mod.rs` declares the port modules and `pub use repos::*;`. All
  ports are object-safe `async_trait` traits, `Send + Sync`.
- `crates/domovoi-core/src/ports/repos/mod.rs:23-48` defines
  `StoreError { NotFound, Conflict(String), Backend(String) }`, `Page`, `Paged`.
- `repos/incidents.rs`: `SignalRepo::insert(&Signal, Option<IncidentId>)`, `IncidentRepo`
  (`insert`, `update`, `get`, `discard_pending`, `append_event`, `timeline`, ...),
  `IncidentTimelineEntry { incident, at, kind: String, summary, detail: Value }`.
- `repos/actions.rs:41-52`: `ActionRepo::insert`, `update_from(&ActionRequest, expected: &str)`
  (guard is `json_extract(status,'$.type') = ?`, store `repos/actions.rs:171-195`).
- `repos/knowledge.rs:27-51`: `NoteRepo::upsert/get/delete/list/search`.
- `repos/audit.rs`: `AuditEvent { at, actor: String, kind, subject, detail: Value }`,
  `AuditLog::append(&AuditEvent) -> u64`.
- `crates/domovoi-core/src/id.rs`: ids are made by `define_id!` (UUIDv7 newtypes). There is no
  `EffectId` yet.
- `crates/domovoi-core/Cargo.toml` already has `async-trait`, `serde_json`, `jiff`, `uuid`, `thiserror`.

**SQLite store (`crates/domovoi-store`).**
- `src/db.rs`: `Store { read: SqlitePool, write: SqlitePool, options }`;
  `connect_options` sets WAL and `SqliteSynchronous::Normal` (line ~93); `Store::open` (line ~113)
  creates the directory, takes the `pre-migrate-*` backup when migrations are pending, builds the
  write pool with `max_connections(1)`, probes FTS5, runs `MIGRATOR.run(&write)`, builds the read pool
  with `READ_CONNECTIONS` (4). `pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");`
  `pool_for_tests()` is `#[doc(hidden)]`.
- `src/error.rs`: `StoreOpenError { Io, Database, NoFts5, Migrate }`; helpers `db`, `db_parent`,
  `corrupt`.
- `src/codec.rs`: `ms`, `from_ms`, `json`, `from_json`, `name`, `from_name`, `parse`, `hash32`, `int`,
  `uint`.
- `src/repos/incidents.rs`: `SignalRepo::insert` (own `begin()`), `IncidentRepo::insert` (line ~120),
  `update` (~145, binds `body = json(incident)`), `discard_pending` (~271), `append_event` (~304).
  Incident rows keep the full state as JSON in `body`.
- `src/repos/actions.rs`: `bound()`, `bind_columns!`, `insert` (~160), `update_from` (~172).
- `src/repos/notes.rs`: `upsert` (~72) is `INSERT ... ON CONFLICT (id) DO UPDATE`.
- `src/repos/audit.rs`: `append` is one `INSERT ... RETURNING seq` on `self.write`.
- `src/backup.rs`: `vacuum_into`, `pre_migrate_name`.
- `migrations/0001`..`0005` (latest: `0005_ack_rule_creator.sql`). Conventions in `0001_init.sql`:
  ids `TEXT`, timestamps `INTEGER` unix ms, JSON `TEXT`, every table `STRICT`.
  `audit_log` has triggers `audit_log_no_update`, `audit_log_no_delete`, `audit_log_no_replace`.
- Workspace dependency: `sqlx = "0.9.0"` (`runtime-tokio`, `sqlite-bundled`, `migrate`, `derive`,
  `macros`). In sqlx 0.9: `Pool::begin_with(statement)` exists, and
  `Migrator::set_ignore_missing(&mut self, bool)` exists (verify in
  `%USERPROFILE%\.cargo\registry\src\*\sqlx-core-0.9.0\src\migrate\migrator.rs`).
- Tests: `crates/domovoi-store/tests/sqlite/` (`main.rs` lists modules; `common.rs` has `Fresh`;
  `contracts.rs` runs every testkit suite on SQLite through the `contract_tests!` macro;
  `migrations.rs`; `concurrency.rs`).

**Testkit (`crates/domovoi-testkit`).**
- `src/repos/mod.rs`: `InMemoryStore { state: Mutex<State>, failures }`, `begin()` (pops an injected
  `fail_next` failure or locks the state), `State` holds `incidents`, `timeline`, `signals`, `actions`,
  `audit: Vec<AuditEvent>`, `notes`, ... No versions, journal or effects.
- `src/repos/incidents.rs`, `actions.rs`, `audit.rs`, `knowledge.rs`: the in-memory repo
  implementations (for example `second_active(state, incident)`, `is_active`).
- `src/contracts/mod.rs`: `ContractError`, `ContractResult`, one suite per repo port.
- `tests/contracts.rs`: `contract_tests!` macro runs each suite on `InMemoryStore::new()`.
- `src/build.rs`: `T0`, `at(secs)`, `host`, `signal`, `incident`, `open_incident`,
  `resolved_incident`, `action(incident, host, Risk)`, `manifest`. `src/approvals.rs`: `human(UserId)`.
- Testkit must not depend on `domovoi-store` (arch rule: testkit depends only on core and proto).

**Architecture rules** (`xtask/src/arch/rules.rs`): core's external allowlist is
`serde, serde_json, thiserror, jiff, uuid, async-trait, sha2, hex, secrecy, regex`. Store and testkit
have no external allowlist. Non-test source files are at most 500 lines (R8).

## 4. Exact scope

Build:

**A. Core types and ports** in a new directory module `crates/domovoi-core/src/ports/uow/`
(split by responsibility; each file at most 500 lines):

- `mod.rs`: re-exports and the `UnitOfWork` and `VersionedRead` traits.
- `transaction.rs`: `Transaction`, `StateWrite`, `Expected`, `ActionGuard`, `AggregateKind`,
  `AggregateRef`, `Expectation`, `Observed`, `CommitReceipt`, `CommitError`, `Versioned<T>`.
- `journal.rs`: `JournalKind`, `JournalDraft`, `JournalEntry`, `JournalRead`, `AuditDraft`, and the
  pure function `timeline_journal`.
- `effects.rs`: `EffectKind`, `EffectStatus`, `ClaimToken`, `EffectDraft`, `EffectCompletion`,
  `EffectRecord`, `EffectRead`.
- `crates/domovoi-core/src/id.rs`: add `EffectId` with `define_id!`; re-export it from
  `crates/domovoi-core/src/lib.rs`'s `pub use id::{...}` list.
- `crates/domovoi-core/src/ports/mod.rs`: `pub mod uow;` and `pub use uow::*;`.

**B. SQLite implementation** in `crates/domovoi-store`:

- `migrations/NNNN_unit_of_work.sql` (see section 8 for the number and DDL).
- `src/uow/mod.rs` (the `UnitOfWork` impl and transaction driver), `src/uow/writes.rs` (state writes),
  `src/uow/journal.rs`, `src/uow/effects.rs`, `src/uow/reads.rs` (`VersionedRead`, `JournalRead`,
  `EffectRead`).
- Refactor, without changing behaviour, the SQL in `repos/incidents.rs`, `repos/actions.rs`,
  `repos/notes.rs` and `repos/audit.rs` into `pub(crate)` free functions that take
  `impl sqlx::Executor<'_, Database = sqlx::Sqlite>` (a pool, or `&mut *tx`), so the repository
  methods and the unit of work run **the same SQL**. The repository methods then call them with
  `&self.write`. Functions needed: `insert_incident`, `update_incident`, `insert_signal_and_link`
  (the link insert inside), `discard_pending_incident`, `insert_incident_event` (returns the new
  `seq`), `insert_action`, `update_action_from`, `upsert_note`, `insert_audit` (returns `seq`).
  Prompt 09 changes `insert_audit` to compute the hash chain; keep it the **only** place that
  inserts into `audit_log`.
- `src/compat.rs`: `schema_compat` read and write, binary-version comparison.
- `src/integrity.rs`: `quick_check`.
- `src/db.rs`: `SqliteSynchronous::Full`; the new open sequence; a migrator that ignores unknown
  applied migrations.
- `src/error.rs`: new `StoreOpenError::SchemaTooNew { min_reader, binary }` and
  `StoreOpenError::Integrity(String)`.
- `src/lib.rs`: re-export `MIN_READER`.
- `incidents.body` gets a `"schema_version": 1` key written by the store (see section 6,
  `INCIDENT_BODY_SCHEMA_VERSION`). Readers ignore it today.
- Repository writes keep versions in step: `IncidentRepo::insert` creates `version = 1`,
  `IncidentRepo::update` and `NoteRepo::upsert` (when the note exists) do `version = version + 1`.

**C. In-memory implementation** in `crates/domovoi-testkit`:

- `src/repos/uow.rs` and `src/repos/uow_apply.rs` (the `UnitOfWork`, `VersionedRead`, `JournalRead`,
  `EffectRead` impls for `InMemoryStore`); `State` gains `versions`, `journal`, `effects`,
  `event_seq`.
- Repository methods in `src/repos/incidents.rs`, `actions.rs`, `audit.rs`, `knowledge.rs` update
  `versions` the same way as SQLite, and share apply logic with the unit of work where the logic is
  more than one line (`second_active`, token uniqueness, guarded update).
- `src/contracts/uow/` (several files, each at most 500 lines): the `UowSubject` trait and the
  contract functions listed in section 12.
- `tests/contracts.rs`: run the new contract functions on `InMemoryStore`.

**D. Store tests** in `crates/domovoi-store/tests/sqlite/`: `uow.rs` (contract run with a
`SqliteSubject`), `durability.rs`, `compat.rs`, `upgrade.rs` (N-1), `commit_cost.rs` (ignored
benchmark), and the fixture file (section 8).

**E. Docs** (section 15).

## 5. Explicit non-goals

- No caller changes: do not touch `crates/domovoi-engine`, `crates/domovoi-hub`,
  `crates/domovoi-agent`, `crates/domovoi-proto`, `crates/domovoi` or `web/`.
  Compile errors there from your changes mean you changed a public signature you should not have.
- No `Actor` enum and no audit hash chain (prompt 09). `AuditDraft` wraps today's `AuditEvent`.
- No effect lifecycle (prompt 15): no claim function, no lease expiry sweep, no retry, no
  dead-letter, no runner. You only create effects, complete them (fenced), and read them.
- No journal kinds beyond the two listed. No SSE, no `seq` broadcast, no retention for `journal` or
  `effects` (prompts 28 and 57 own those).
- No copy of old `incident_events` rows into `journal` (prompt 28).
- No `audit_anchor` table, no audit columns (prompt 09).
- No `domovoi restore` command, no backup verification, no pre-migrate pruning (prompt 19).
- No changes to `xtask`, arch rules or the CI workflow.
- Do not edit migrations `0001`..`0005`. Do not add a `version` column to `actions`.
- Do not change `StoreError` or any existing repository trait signature.
- No new external dependencies. `sha2` is not needed here.

## 6. Required interfaces and contracts

### 6.1 Core types (**fixed** field and variant names; doc comments are required on every item)

```rust
// crates/domovoi-core/src/ports/uow/transaction.rs

/// Which kind of aggregate a write, journal entry or effect belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AggregateKind { Incident, Action, Note }
impl AggregateKind {
    /// Stable lowercase name stored in the database: "incident", "action", "note".
    pub const fn as_str(self) -> &'static str;
    /// Reads `as_str` back; `None` for anything else.
    pub fn parse(text: &str) -> Option<Self>;
}

/// An aggregate instance: kind plus id text (a UUID for every kind here).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AggregateRef { pub kind: AggregateKind, pub id: String }
impl AggregateRef {
    pub fn incident(id: IncidentId) -> Self;   // also ::action(ActionId), ::note(NoteId)
}
// Display as "<kind>:<id>".

/// What a versioned write expects to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected { New, Version(u64) }

/// What an action write expects to find (actions are guarded by their status name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionGuard { New, Status(String) }   // Status holds ActionStatus::name(), e.g. "proposed"

/// The expectation recorded in a conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expectation { New, Version(u64), Status(String) }   // From<Expected>, From<ActionGuard>

/// What was actually stored when a conflict happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed { Missing, Version(u64), Status(String) }

/// A value and the stored version it was read at.
#[derive(Debug, Clone, PartialEq)]
pub struct Versioned<T> { pub value: T, pub version: u64 }

/// One typed state change. Each variant is built only by the writer that owns the aggregate.
#[derive(Debug, Clone)]
pub enum StateWrite {
    /// Stores a signal and, if given, links it to an incident.
    Signal { signal: Signal, incident: Option<IncidentId> },
    /// Inserts (`Expected::New`) or replaces (`Expected::Version(n)`) an incident.
    Incident { incident: Incident, expected: Expected },
    /// Appends a timeline entry. Interim dual-write: also writes a mirrored journal row.
    IncidentEvent(IncidentTimelineEntry),
    /// Deletes a pending incident with its timeline and signal links.
    DiscardPendingIncident { id: IncidentId, expected: u64 },
    /// Inserts (`ActionGuard::New`) or replaces an action whose stored status is as guarded.
    Action { action: ActionRequest, expected: ActionGuard },
    /// Inserts (`Expected::New`) or replaces (`Expected::Version(n)`) a note.
    Note { note: Note, expected: Expected },
}

/// Everything one business event changes. Applied in this order: `writes`, then the mirrored and
/// explicit journal rows, then `audit`, then `new_effects`, then `completed_effects`.
#[derive(Debug, Clone, Default)]
pub struct Transaction {
    pub writes: Vec<StateWrite>,
    pub journal: Vec<JournalDraft>,
    pub audit: Vec<AuditDraft>,
    pub new_effects: Vec<EffectDraft>,
    pub completed_effects: Vec<EffectCompletion>,
}
impl Transaction {
    pub fn new() -> Self;
    pub fn is_empty(&self) -> bool;
    // Builder helpers, each returns Self: write(StateWrite), journal(JournalDraft),
    // audit(AuditDraft), effect(EffectDraft), complete(EffectCompletion).
}

/// What a successful commit assigned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitReceipt {
    /// Journal sequence numbers: mirrored timeline rows in write order, then explicit drafts in draft order.
    pub journal_seqs: Vec<u64>,
    /// Audit sequence numbers in draft order.
    pub audit_seqs: Vec<u64>,
    /// New version of every versioned aggregate written (incidents, notes), in write order.
    pub versions: Vec<(AggregateRef, u64)>,
}

/// Why a commit changed nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommitError {
    /// A guarded write found something other than what the caller read.
    #[error("{aggregate} changed meanwhile: expected {expected:?}, found {found:?}")]
    Conflict { aggregate: AggregateRef, expected: Expectation, found: Observed },
    /// A uniqueness, foreign-key or state rule refused a write; the text names it.
    #[error("constraint violated: {0}")]
    Constraint(String),
    /// A completion named an effect that is not claimed under that token (or not pending, for an
    /// unclaimed completion): the caller is a stale lease holder.
    #[error("effect {effect} is not held by this caller")]
    StaleEffect { effect: EffectId },
    /// The store failed. Nothing was written.
    #[error("store error: {0}")]
    Backend(String),
}
```

```rust
// crates/domovoi-core/src/ports/uow/mod.rs

/// Commits a [`Transaction`] atomically.
#[async_trait]
pub trait UnitOfWork: Send + Sync {
    /// Applies the whole transaction or none of it. An empty transaction succeeds without
    /// touching the store and returns an empty receipt.
    async fn commit(&self, tx: Transaction) -> Result<CommitReceipt, CommitError>;
}

/// Reads aggregates together with the version a later write must name.
#[async_trait]
pub trait VersionedRead: Send + Sync {
    /// The incident and its version. Body and version come from one statement or one snapshot.
    async fn incident(&self, id: IncidentId) -> Result<Option<Versioned<Incident>>, StoreError>;
    /// Every active (not resolved) incident with its version, newest `opened_at` first.
    async fn active_incidents(&self) -> Result<Vec<Versioned<Incident>>, StoreError>;
    /// The note and its version.
    async fn note(&self, id: NoteId) -> Result<Option<Versioned<Note>>, StoreError>;
}
```

```rust
// crates/domovoi-core/src/ports/uow/journal.rs

/// Typed journal kinds. Closed on purpose; later prompts add variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalKind { IncidentTimeline, ActionTransition }
// as_str: "incident.timeline", "action.transition"; parse(&str) -> Option<Self>

/// A journal row to append. The store assigns `seq`.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalDraft {
    pub aggregate: AggregateRef,
    pub kind: JournalKind,
    /// Schema version of `payload` for this kind. Start at 1.
    pub payload_v: u32,
    pub payload: serde_json::Value,
    /// Today's actor string format (`user:<id>`, `system:<component>`); prompt 09 replaces it with `Actor`.
    pub actor: String,
    pub occurred_at: jiff::Timestamp,
}

/// A stored journal row.
#[derive(Debug, Clone, PartialEq)]
pub struct JournalEntry {
    pub seq: u64,
    pub aggregate: AggregateRef,
    /// The stable kind string as stored. Readers must tolerate kinds they do not know.
    pub kind: String,
    pub payload_v: u32,
    pub payload: serde_json::Value,
    pub actor: String,
    pub occurred_at: jiff::Timestamp,
}

/// An audit entry to append in the same transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditDraft { pub event: AuditEvent }
impl AuditDraft { pub fn new(event: AuditEvent) -> Self; }

/// The journal row that mirrors a timeline entry (aggregate = the incident, kind
/// `IncidentTimeline`, `payload_v` 1, `actor` "system:incidents", `occurred_at` = `entry.at`).
/// Payload is exactly {"kind": entry.kind, "summary": entry.summary, "detail": entry.detail}.
pub fn timeline_journal(entry: &IncidentTimelineEntry) -> JournalDraft;

#[async_trait]
pub trait JournalRead: Send + Sync {
    /// Up to `limit` entries with `seq > after_seq`, oldest first. `limit` is clamped to 1..=500.
    async fn entries_after(&self, after_seq: u64, limit: u16) -> Result<Vec<JournalEntry>, StoreError>;
    /// Up to `limit` entries of one aggregate, oldest first.
    async fn entries_for(&self, aggregate: &AggregateRef, limit: u16) -> Result<Vec<JournalEntry>, StoreError>;
    /// The highest assigned `seq`, 0 when the journal is empty.
    async fn head_seq(&self) -> Result<u64, StoreError>;
}
```

```rust
// crates/domovoi-core/src/ports/uow/effects.rs

/// The kinds of pending work (04 section 4). Stored as snake_case strings, validated in Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    BuildContext, CallModel, CollectEvidence, EvaluatePolicy, IssuePermit,
    Dispatch, Reconcile, Verify, Notify, Wake, DraftSummary,
}
// as_str: build_context call_model collect_evidence evaluate_policy issue_permit dispatch
//         reconcile verify notify wake draft_summary; parse(&str) -> Option<Self>

/// Effect lifecycle states. Only `Pending` and `Done` are produced by this prompt; prompt 15 owns the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectStatus { Pending, Claimed, Done, Dead, Cancelled }
// as_str: pending claimed done dead cancelled; parse(&str) -> Option<Self>

/// Identifies one claim of an effect. A fresh value per claim; fences stale lease holders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ClaimToken(uuid::Uuid);   // new() (v4), from_uuid, as_uuid, Display, FromStr

/// A new pending effect, created in its owner's transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectDraft {
    pub id: EffectId,
    pub kind: EffectKind,
    pub aggregate: AggregateRef,
    pub payload: serde_json::Value,
    pub not_before: jiff::Timestamp,
    /// Attempts allowed before dead-lettering; stored, enforced by prompt 15. Must be >= 1.
    pub max_attempts: u32,
    /// While an effect with this key is pending or claimed, a second one is refused.
    pub dedup_key: Option<String>,
    pub created_at: jiff::Timestamp,
}

/// Completes an effect inside the owner's transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectCompletion {
    pub id: EffectId,
    /// `Some(token)`: the effect must be `claimed` under exactly this token (fenced).
    /// `None`: the effect must still be `pending` (completed without ever being claimed).
    pub claim_token: Option<ClaimToken>,
    pub at: jiff::Timestamp,
}

/// A stored effect row. Fields mirror the columns in section 8.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectRecord {
    pub id: EffectId, pub kind: String, pub aggregate: AggregateRef, pub payload: serde_json::Value,
    pub status: String, pub not_before: jiff::Timestamp, pub attempt: u32, pub max_attempts: u32,
    pub dedup_key: Option<String>, pub claim_token: Option<ClaimToken>, pub claim_generation: u64,
    pub lease_until: Option<jiff::Timestamp>, pub runner_id: Option<String>, pub last_error: Option<String>,
    pub created_at: jiff::Timestamp, pub completed_at: Option<jiff::Timestamp>,
}

#[async_trait]
pub trait EffectRead: Send + Sync {
    async fn effect(&self, id: EffectId) -> Result<Option<EffectRecord>, StoreError>;
    /// Up to `limit` effects in `status`, in creation order (oldest first); clamp as in `JournalRead`.
    async fn effects_with_status(&self, status: EffectStatus, limit: u16) -> Result<Vec<EffectRecord>, StoreError>;
}
```

`Store` and `InMemoryStore` implement `UnitOfWork`, `VersionedRead`, `JournalRead` and `EffectRead`.
Keep `kind` and `status` strings in the read models (not enums) so readers survive future values.

### 6.2 Store constants and errors (**fixed**)

```rust
// crates/domovoi-store/src/compat.rs
/// Lowest binary version that can read a database written by this code. Raise it in the same
/// change as any migration an older binary cannot read (a drop, a rename, a changed meaning).
pub const MIN_READER: &str = "0.0.0";

// crates/domovoi-store/src/repos/incidents.rs (or a small shared module)
/// Written as "schema_version" inside every stored incident body.
pub(crate) const INCIDENT_BODY_SCHEMA_VERSION: u64 = 1;
```

```rust
// StoreOpenError additions (thiserror)
#[error("this database needs Domovoi {min_reader} or newer, but this is {binary}; upgrade Domovoi. \
         Restoring an older backup is the only way back")]
SchemaTooNew { min_reader: String, binary: String },
#[error("the database failed its integrity check ({0}); restore a backup")]
Integrity(String),
```

### 6.3 Commit semantics (**fixed**)

SQLite, in `Store::commit`:

1. If `tx.is_empty()`, return `CommitReceipt::default()`.
2. `let mut dbtx = self.write.begin_with("BEGIN IMMEDIATE").await.map_err(backend)?;`
   (If the type does not accept a `&'static str`, use `sqlx::AssertSqlSafe` or the equivalent that
   sqlx 0.9 requires; verify against the sqlx source.)
3. For each `StateWrite` in order, apply (table below). Any error returns immediately; dropping
   `dbtx` rolls back. Never `unwrap` or ignore a result.
4. Insert journal rows: first the mirrored rows already created in step 3 (they were inserted at
   that point, in write order), then `tx.journal` in order. `INSERT ... RETURNING seq`.
5. Insert audit rows with `insert_audit`, in order.
6. Insert new effects.
7. Complete effects (fenced; zero affected rows is `StaleEffect`).
8. `dbtx.commit()`; a commit failure is `Backend`.

| `StateWrite` | Behaviour | Errors |
|---|---|---|
| `Signal` | insert into `signals`, then the `incident_signals` link if given | duplicate signal id: `Constraint("signal id exists")` |
| `Incident` + `New` | `SELECT version` first; if a row exists: `Conflict{New, Version(v)}`. Else insert with `version = 1` | active incident with the same fingerprint (unique index): `Constraint("an active incident with this fingerprint exists")` |
| `Incident` + `Version(n)` | `UPDATE ... SET ..., version = version + 1 WHERE id = ? AND version = ?`. Zero rows: select the stored version; none: `Conflict{Version(n), Missing}`; some `v`: `Conflict{Version(n), Version(v)}` | same unique-index constraint |
| `IncidentEvent` | insert into `incident_events` (get `seq`), then insert the mirrored journal row (`timeline_journal(entry)` with `legacy_event_seq = seq`) | unknown incident (foreign key): `Constraint("unknown incident")` |
| `DiscardPendingIncident` | version check as above; status must be `pending` else `Constraint("only pending incidents are discarded")`; delete links, then the incident (timeline rows cascade). Journal rows are kept | missing: `Conflict{Version(n), Missing}` |
| `Action` + `New` | if the id exists: `Conflict{New, Status(current)}`; else insert | chat token hash already used: `Constraint("chat token already used")` |
| `Action` + `Status(s)` | `UPDATE ... WHERE id = ? AND json_extract(status,'$.type') = ?`. Zero rows: stored status found: `Conflict{Status(s), Status(found)}`; none: `Conflict{Status(s), Missing}` | chat token clash: `Constraint` |
| `Note` + `New` / `Version(n)` | like `Incident` (`notes` has no uniqueness beyond id) | |

`CommitReceipt.versions` lists `(AggregateRef, new_version)` for each `Incident` and `Note` write
(1 for `New`, `n + 1` for `Version(n)`), and for `DiscardPendingIncident` nothing.

`InMemoryStore::commit` must give identical observable results. Its mechanism: call `self.begin()`
(so `fail_next` makes a commit fail with `CommitError::Backend(<message>)` for any injected
`StoreError`), clone the affected collections, apply the writes to the clones, and replace the
originals only if every step succeeded.

### 6.4 Sqlite error mapping (**convention**)

`fn backend(err: sqlx::Error) -> CommitError`: unique and foreign-key violations map to
`Constraint(message)`; everything else (including a busy timeout) maps to `Backend(message)`.
Where the table above names a specific constraint message, check for the condition explicitly (or
inspect which index failed) instead of passing sqlite's text through.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file and symbol
   named in section 3. Confirm: migrations stop at `0005_ack_rule_creator.sql`; `Store::open` order as
   described; `sqlx` 0.9 has `Pool::begin_with` and `Migrator::set_ignore_missing`; `Migration` is
   `Clone`; no `version` column exists. Run `ls crates/domovoi-store/migrations` and note the highest
   number (prompt 13 may have merged a migration). Write down any difference before editing.
2. **Create the N-1 fixture before touching any store code.** On the unmodified tree, add a
   temporary `#[ignore]` test `generate_n1_fixture` in `crates/domovoi-store/tests/sqlite/upgrade.rs`
   that uses `Store::open` (still migrations 0001..0005) and the repository ports with testkit
   builders to write the content listed in section 8 into a database, then `VACUUM INTO`
   `crates/domovoi-store/tests/fixtures/n1-schema5.db` (use the pool from `pool_for_tests`).
   Use fixed ids built with `from_uuid`/fixed timestamps (`T0`, `at(n)`) so the content is known.
   Run it once. Then **delete the generator**, keep the `.db` file, and write a module comment in
   `upgrade.rs` saying the file was produced from the code at the commit before this change, with
   migrations 0001 to 0005 only, and cannot be regenerated. Commit the fixture alone
   (`test(store): add a database written by the previous schema`).
3. **Core.** Add `EffectId`; add the `uow` module with all types and traits from section 6.1;
   unit tests in `uow/` for `AggregateKind::parse`, `JournalKind`/`EffectKind`/`EffectStatus`
   round trips, `timeline_journal` payload, `Transaction::is_empty`. `cargo clippy -p domovoi-core`
   and `cargo nextest run -p domovoi-core`.
4. **Migration.** Add the migration (section 8). Run the store tests for repositories to see nothing
   regressed (`cargo nextest run -p domovoi-store`).
5. **Refactor SQL into shared executor functions** (section 4.B) with no behaviour change. Add
   `version` handling to `IncidentRepo::insert/update` and `NoteRepo::upsert`, and the
   `schema_version` key in incident bodies. Existing store tests must pass unchanged.
6. **`Store::commit` and the readers** (`src/uow/`). Implement the table in 6.3.
7. **Open sequence** (section 8: compat, synchronous, `quick_check`, `ignore_missing`).
8. **Testkit.** In-memory implementation, `UowSubject`, contract functions. Make the repo
   implementations bump versions.
9. **Tests.** Wire the contract functions into `testkit/tests/contracts.rs` and
   `store/tests/sqlite/uow.rs`; add the SQLite-only tests (section 12).
10. **R-6 benchmark** (`commit_cost.rs`, `#[ignore]`): see section 12. Run it once and put the
    numbers in the completion report.
11. **Docs** (section 15).
12. **Run the checks and write the completion report.** `cargo fmt --all`;
    `cargo clippy -p domovoi-core -p domovoi-store -p domovoi-testkit --all-targets -- -D warnings`;
    `cargo nextest run -p domovoi-core -p domovoi-store -p domovoi-testkit`;
    `cargo nextest run -E 'rdeps(domovoi-testkit)'` (testkit changed; this proves engine and hub tests
    still pass against the extended `InMemoryStore`); `cargo xtask arch-check`.

## 8. Database and persistence

### Migration

File: `crates/domovoi-store/migrations/NNNN_unit_of_work.sql` where `NNNN` is the next free number
(expected `0006`). Prompt 13 runs in parallel and may add its own migration. Take the next number
after the highest file on your branch at the time you start, and **before opening the PR rebase
and renumber yours if `main` gained one** (renaming your own unmerged file is allowed; sqlx rejects
duplicate versions). Never edit a merged migration. Additive only; no CHECK constraints on new enum
columns (enums are validated in Rust); no rebuild of tables protected by triggers or carrying FTS
rowids.

```sql
-- Optimistic versions for the aggregates the unit of work writes with a version check.
ALTER TABLE incidents ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE notes     ADD COLUMN version INTEGER NOT NULL DEFAULT 1;

-- The operational journal. Written only through the unit of work. Not trigger-protected:
-- retention deletes old rows. Rows outlive the aggregate they describe.
CREATE TABLE journal (
    seq              INTEGER PRIMARY KEY AUTOINCREMENT,
    aggregate_type   TEXT    NOT NULL,
    aggregate_id     TEXT    NOT NULL,
    kind             TEXT    NOT NULL,
    payload_v        INTEGER NOT NULL,
    payload          TEXT    NOT NULL,
    actor            TEXT    NOT NULL,
    occurred_at      INTEGER NOT NULL,
    -- incident_events.seq this row mirrors during the dual-write period; NULL otherwise.
    legacy_event_seq INTEGER
) STRICT;
CREATE INDEX journal_aggregate ON journal (aggregate_type, aggregate_id, seq);
CREATE INDEX journal_occurred  ON journal (occurred_at);
CREATE UNIQUE INDEX journal_legacy_event ON journal (legacy_event_seq)
    WHERE legacy_event_seq IS NOT NULL;

-- The work outbox. Lifecycle columns exist now; the lifecycle itself is implemented later.
CREATE TABLE effects (
    seq              INTEGER PRIMARY KEY AUTOINCREMENT,
    id               TEXT    NOT NULL UNIQUE,
    kind             TEXT    NOT NULL,
    aggregate_type   TEXT    NOT NULL,
    aggregate_id     TEXT    NOT NULL,
    payload          TEXT    NOT NULL,
    status           TEXT    NOT NULL DEFAULT 'pending',
    not_before       INTEGER NOT NULL,
    attempt          INTEGER NOT NULL DEFAULT 0,
    max_attempts     INTEGER NOT NULL,
    dedup_key        TEXT,
    claim_token      TEXT,
    claim_generation INTEGER NOT NULL DEFAULT 0,
    lease_until      INTEGER,
    runner_id        TEXT,
    last_error       TEXT,
    created_at       INTEGER NOT NULL,
    completed_at     INTEGER
) STRICT;
CREATE INDEX effects_due ON effects (status, not_before, seq);
CREATE INDEX effects_aggregate ON effects (aggregate_type, aggregate_id);
CREATE UNIQUE INDEX effects_dedup ON effects (dedup_key)
    WHERE dedup_key IS NOT NULL AND status IN ('pending', 'claimed');

-- The lowest binary version that can read this schema. One row, id = 1, written by Store::open.
CREATE TABLE schema_compat (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    min_reader  TEXT    NOT NULL,
    written_by  TEXT    NOT NULL,
    written_at  INTEGER NOT NULL
) STRICT;
```

`SELECT version` for an incident never sees a legacy row without a version: the column default
gives every existing row `version = 1` (**the upgrade test asserts this**).

### Drop the `advisories.kind` CHECK once (separate migration file, same PR)

Today `advisories.kind` has a `CHECK (kind IN (...))` list
(`crates/domovoi-store/migrations/0004_settings_advisories.sql`). Every new advisory kind has forced
a full table rebuild (0003, 0004). Several later changes need new kinds: `effect_dead`,
`backup_failed`, an audit-chain advisory and an admission advisory. To stop the repeated rebuilds,
this PR rebuilds `advisories` **once**, without the kind CHECK:

- File: `NNNN_advisories_kind_unchecked.sql`, numbered after your unit-of-work migration.
- Copy the current table definition from `0004` exactly: every column, index, default and
  constraint, **except** the `CHECK (kind IN (...))` on `kind`. Copy the rows across with an
  `INSERT INTO ... SELECT`, drop the old table, rename the new one, and recreate its indexes. Follow
  the same pattern `0004` used.
- Kinds stay validated in Rust: `AdvisoryKind` in `crates/domovoi-core/src/advisory.rs`. Reading a
  row with an unknown kind must return a store error and must not panic. Check how
  `domovoi-store` maps the column today, and keep that mapping.
- Tests:
  - `advisories_rows_survive_kind_check_removal`: the N-1 fixture rows are unchanged after the
    migration.
  - `advisory_with_unknown_kind_is_a_store_error`: insert a raw row with kind `bogus`, then read it.

From then on, a new advisory kind is a Rust-only change (`AdvisoryKind` variant plus its string
mapping), with no migration.

### Open sequence (new `Store::open`)

1. Create the directory (existing).
2. If the database file exists: `compat::check(path)`, on a fresh connection, before any backup or
   migration. If table `schema_compat` and its row exist and the binary version
   (`env!("CARGO_PKG_VERSION")`) is lower than `min_reader`, return `SchemaTooNew`. Compare
   `major.minor.patch` numerically; ignore anything after `-` or `+`; an unparsable `min_reader`
   returns `StoreOpenError::Database("schema_compat.min_reader is unreadable")`. A missing table or
   row passes.
3. Pre-migrate backup when migrations are pending (existing).
4. Build the write pool, FTS5 probe (existing). `connect_options` now uses
   `SqliteSynchronous::Full`; it applies to every connection of every pool and to the helper
   connections in `applied_versions` and the backup.
5. Run migrations with a migrator built from the embedded one that ignores unknown applied
   migrations: `let mut m = Migrator::with_migrations(MIGRATOR.iter().cloned().collect());
   m.set_ignore_missing(true);` (verify the exact API). Keep `pub static MIGRATOR` as it is: tests use it.
6. `compat::record`: when migrations were pending (the same `needs_backup` result, or the database was
   new) or the `schema_compat` row is absent, upsert row `id = 1` with `min_reader` = the larger of the
   stored value and `MIN_READER`, `written_by` = binary version, `written_at` = `options.now` in ms.
   Otherwise leave it alone (an older binary opening a newer database must not overwrite
   `written_by`).
7. `integrity::quick_check`: `PRAGMA quick_check` on the write connection. The result is one row
   `ok`, or up to many rows of problems; on anything but a single `ok` return
   `StoreOpenError::Integrity` with the first 5 lines joined by `; `. Also expose
   `pub async fn quick_check(&self) -> Result<(), StoreOpenError>` on `Store` (prompt 19 verifies
   backups with it).
8. Build the read pool (existing).

Time for the compat timestamp: use `options.now` (the existing `StoreOptions.now`), not the system
clock.

### Transaction boundaries

One `Store::commit` call is one `BEGIN IMMEDIATE ... COMMIT` on the single writer connection. The
repository ports keep their own statements (each on the same writer connection), so a repository
write and a commit never interleave: the pool hands the one connection to one user at a time.

### N-1 upgrade test and fixture

Fixture: `crates/domovoi-store/tests/fixtures/n1-schema5.db`, created in step 2 from the previous
code. Contents to create (all with fixed ids and timestamps): 2 hosts; 1 agent identity; 3 incidents
(one open, one resolved, one pending), each through `IncidentRepo::insert`; 3 signals linked to the
open incident; 4 `incident_events` through `append_event`; 2 actions (one `proposed` with a chat token
hash, one `approved`, using `testkit::approvals::human`); 5 audit rows; 2 notes (one global, one with
a host); 1 setting. Name the incident titles and note titles so FTS assertions can find them.

Test `upgrade_from_previous_schema_keeps_every_row` in `tests/sqlite/upgrade.rs`:
copy the fixture into a temp dir, `Store::open` it with `StoreOptions::beside(&path, at(1000))`, then
assert:

1. A `pre-migrate-<time>.db` exists in the backup directory, opens, and has none of the new tables
   (`journal`, `effects`, `schema_compat`).
2. Row counts of `hosts`, `incidents`, `signals`, `incident_events`, `actions`, `audit_log`, `notes`,
   `settings` equal the fixture's.
3. Every incident and note has `version = 1`; every incident round-trips through
   `IncidentRepo::get` equal to what was inserted (rows written before the change have bodies
   without `schema_version`; they still load).
4. `journal`, `effects` have 0 rows; `schema_compat` has exactly one row with `id = 1`,
   `min_reader = MIN_READER`.
5. The three `audit_log_no_*` triggers still exist, and an `UPDATE audit_log` is still refused.
6. `IncidentRepo::search` and `NoteRepo::search` still find the titled rows (FTS intact).
7. `_sqlx_migrations` lists every embedded migration as successful.

## 9. Protocol and compatibility

No wire protocol change. The hub and agent protocol stays 1.1. No new HTTP endpoints, so no OpenAPI
regeneration.

Schema compatibility: the new migration is additive. A previous-release binary (which has no
`schema_compat` check) can still open the upgraded database because it ignores new tables and
columns, but it will refuse to run migrations it does not know. The sqlx `ignore_missing` behaviour
and the `min_reader` check apply from this release onward.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Empty `Transaction` | `Ok(CommitReceipt::default())`; no `BEGIN` is issued |
| Stored version differs from `Expected::Version(n)` | `CommitError::Conflict`; the transaction is rolled back; nothing is visible |
| `Expected::New` but the id exists | `Conflict{expected: New, found: Version(v)}` (incident, note) or `Status(s)` (action) |
| Guarded write target missing | `Conflict{..., found: Missing}` |
| Second active incident for a fingerprint | `Constraint("an active incident with this fingerprint exists")`, whole transaction rolled back |
| A later write fails after earlier writes, journal, audit or effects were applied in the same transaction | everything rolls back; journal `head_seq`, audit and effects are exactly as before |
| Completion with the wrong or no claim token, or on an effect that is not claimed (token given) or not pending (no token) | `StaleEffect{effect}`; the whole transaction (including state writes) rolls back |
| `EffectDraft.max_attempts == 0` | `Constraint("max_attempts must be at least 1")` before any write |
| `dedup_key` already pending or claimed | `Constraint("duplicate effect")` |
| SQLite busy past the 5 s busy timeout, I/O error, disk full | `Backend(message)`; nothing written |
| `InMemoryStore::fail_next(err)` armed | next `commit` returns `Backend(err.to_string())`, state unchanged |
| Database needs a newer binary | `Store::open` returns `SchemaTooNew` before any backup, migration or write; the file is byte-identical afterwards |
| `quick_check` reports problems | `Store::open` returns `Integrity`; the hub does not start |
| Journal read finds an unknown kind string | returned as-is in `JournalEntry.kind`; never an error |
| A stored journal payload is not valid JSON | `StoreError::Backend("stored journal payload is unreadable: ...")` (use `corrupt`) |

## 11. Security requirements

- **No approval path changes.** `HumanApproval::new` and `restore` call sites are untouched. The unit
  of work must not construct or accept anything that bypasses the action state machine: `StateWrite::Action`
  carries an `ActionRequest` produced by the domain transition methods, exactly as `ActionRepo` does today.
  `HumanApproval::restore` still happens only inside `crates/domovoi-store/src/repos/actions.rs`;
  if your refactor moves the row-to-action conversion, keep it under `crates/domovoi-store/src/`.
- **Audit is append-only.** Audit rows are inserted only by `insert_audit`; you add no update or delete
  of `audit_log`, and the triggers stay. The upgrade test asserts they survive.
- **Fail closed.** Any error during a commit leaves no partial state (tests prove it). A database
  that needs a newer binary or fails `quick_check` makes `open` fail; there is no "continue anyway" flag.
- **No secrets in journal or effect payloads by construction**: the port takes `serde_json::Value` from
  callers who already redact; do not log payloads. Log only ids, kinds and counts at `debug`.
- **Chat token hashes** stay hashes; nothing in this change prints them.
- `unwrap`/`expect`/`panic!` are forbidden in library code (tests excepted).

## 12. Testing requirements

All names are behaviours. Contract functions live in `crates/domovoi-testkit/src/contracts/uow/`,
are `pub async fn name(subject: &dyn UowSubject) -> ContractResult`, and are exported from
`contracts/mod.rs`. Each runs on a **fresh** store.

`UowSubject` (new, in `contracts/uow/mod.rs`), implemented by `InMemoryStore` and, in the store's test
crate, by a local `SqliteSubject(Store)`:

```rust
#[async_trait]
pub trait UowSubject: Send + Sync {
    fn uow(&self) -> &dyn UnitOfWork;
    fn versions(&self) -> &dyn VersionedRead;
    fn journal(&self) -> &dyn JournalRead;
    fn effects(&self) -> &dyn EffectRead;
    fn incidents(&self) -> &dyn IncidentRepo;
    fn signals(&self) -> &dyn SignalRepo;
    fn actions(&self) -> &dyn ActionRepo;
    fn notes(&self) -> &dyn NoteRepo;
    fn audit(&self) -> &dyn AuditLog;
    /// Test seam: puts a pending effect into `claimed` under `token`, as the effect runner will.
    async fn force_claim(&self, id: EffectId, token: ClaimToken, lease_until: Timestamp)
        -> Result<(), ContractError>;
}
```

SQLite `force_claim` runs `UPDATE effects SET status='claimed', claim_token=?, claim_generation=claim_generation+1,
lease_until=?, attempt=attempt+1 WHERE id=?` through `store.pool_for_tests()`.

Contract functions (setup / action / expected). Use `testkit::build` helpers
(`host`, `open_incident`, `incident`, `signal`, `action`, `at`, `T0`).

1. `commit_applies_state_journal_audit_and_effects_together`: one transaction with an `Incident New`,
   a `Signal` linked to it, an `IncidentEvent`, an explicit `JournalDraft(ActionTransition)`, an
   `AuditDraft`, and an `EffectDraft(Wake)`. Expect: receipt has 2 journal seqs (mirror first, then
   explicit), 1 audit seq, `versions == [(incident ref, 1)]`; the incident, signal link and timeline
   entry are readable; `journal.entries_after(0, 10)` returns both rows in seq order; the audit row
   is listed; the effect is `pending` with `attempt = 0`, `claim_generation = 0`.
2. `empty_transaction_commits_nothing`: receipt equals default; `head_seq` is 0.
3. `stale_incident_version_conflicts_and_changes_nothing`: insert at version 1; commit an update with
   `Version(1)` (receipt version 2); commit another with `Version(1)` plus an audit draft and a journal
   draft. Expect `Conflict{Version(1), Version(2)}`; the stored incident, audit list and `head_seq`
   are unchanged from before the failed commit.
4. `new_incident_over_existing_id_conflicts`: `Expected::New` on an existing id gives
   `Conflict{New, Version(1)}`.
5. `second_active_incident_for_a_fingerprint_is_refused_and_rolls_back`: transaction = `Incident New`
   (A, open) followed by `Incident New` (B with the same fingerprint, active). Expect
   `Constraint(...)` and that A is **not** stored either.
6. `later_failure_rolls_back_earlier_writes_journal_audit_and_effects`: transaction with a valid
   `Note New`, a journal draft, an audit draft, an effect, then a failing `Incident` write
   (`Version(7)` on a missing id). Expect `Conflict{Version(7), Missing}` and nothing visible from the
   earlier items (note absent, `head_seq` unchanged, audit unchanged, effect absent).
7. `repository_writes_advance_versions_like_commits`: `IncidentRepo::insert` then `VersionedRead::incident`
   gives version 1; `IncidentRepo::update` gives 2; a commit with `Version(2)` succeeds and gives 3.
   Same for `NoteRepo::upsert` twice (1 then 2), and a `Note` commit with `Version(2)`.
8. `action_guard_refuses_a_changed_status`: insert an action through `StateWrite::Action New`; approve it
   in memory (`ActionRequest::approve` with `testkit::approvals::human`); commit `Status("proposed")`
   (ok); commit the same again: `Conflict{Status("proposed"), Status("approved")}` and nothing written.
   `Status("proposed")` on a missing id gives `found: Missing`.
9. `chat_token_hash_must_be_unique_across_actions`: two `Action New` with the same chat token hash:
   `Constraint`, first one not stored when both are in one transaction.
10. `incident_event_is_mirrored_into_the_journal_with_its_original_time`: commit an `IncidentEvent`
    with `at = at(500)`. Expect `incidents.timeline(id)` has the entry, and the journal row has
    kind `incident.timeline`, `occurred_at = at(500)`, payload `{kind, summary, detail}` equal to the
    entry, actor `system:incidents`. For SQLite only (separate test in `store/tests/sqlite/uow.rs`),
    `journal.legacy_event_seq` equals `incident_events.seq`.
11. `journal_sequence_follows_commit_order`: three commits; `entries_after(first_seq, 10)` returns only
    later rows; `head_seq` equals the last receipt seq; `entries_for(incident_ref)` returns only that
    incident's rows.
12. `discarding_a_pending_incident_removes_its_timeline_and_unlinks_signals`: pending incident with
    signal and event; commit `DiscardPendingIncident{expected: 1}`. Expect incident gone, timeline
    empty, signal still stored but no longer listed for the incident, journal rows kept. A non-pending
    incident gives `Constraint("only pending incidents are discarded")`.
13. `audit_draft_is_appended_with_the_next_sequence_number`: `AuditLog::append` returns n, a commit's
    audit seq is n + 1.
14. `duplicate_dedup_key_is_refused_while_pending`: two effects with one key in one transaction give
    `Constraint("duplicate effect")`; after completing the first (`claim_token: None`), a new one with
    that key is accepted.
15. `effect_completion_needs_the_current_claim_token`: create effect; `force_claim(token_a)`; commit a
    completion with `Some(token_b)` plus an `Incident New` in the same transaction: `StaleEffect`, and
    the incident is **not** stored; commit with `Some(token_a)`: effect `done`, `completed_at` set.
    A completion with `None` on a claimed effect gives `StaleEffect`.
16. `completing_an_unclaimed_effect_without_a_token_marks_it_done`.
17. `zero_max_attempts_is_refused`.

Additional store tests (`crates/domovoi-store/tests/sqlite/`):

- `uow.rs`: one `#[tokio::test]` per contract function through a macro like `contract_tests!`
  (names `sqlite_<behaviour>`), plus the `legacy_event_seq` assertion above, plus
  `incident_body_carries_a_schema_version` (insert via `IncidentRepo::insert` and via a commit; read the
  raw `body` with `pool_for_tests` and assert `"schema_version":1`; an old-format body without the
  key still loads).
- `durability.rs`: `connections_flush_every_commit` (`PRAGMA synchronous` on `pool_for_tests` returns
  `2`, FULL); `startup_runs_an_integrity_check` (open a store; then corrupt a copy of the file by
  overwriting a middle page region with zeros and assert `Store::open` returns `Integrity` — if
  reliable corruption of a tiny database proves flaky on the platform, instead unit-test the result
  parser of `integrity.rs` with canned `quick_check` outputs `["ok"]`, `["ok","x"]`, `["*** in database main ***"]`
  and keep a smoke test that a healthy database passes; say which you did).
- `compat.rs`: `database_requiring_a_newer_binary_is_refused_untouched` (open; close; set
  `schema_compat.min_reader = '999.0.0'` with a raw connection; hash the file bytes; `Store::open`
  returns `SchemaTooNew{min_reader: "999.0.0", ..}`; no backup directory created; file bytes
  identical); `older_binary_ignores_migrations_it_does_not_know` (open; insert a fake row into
  `_sqlx_migrations` with version 999999 and `success = 1`; close; reopen succeeds);
  `written_by_is_kept_when_nothing_migrated` (set `written_by = '9.9.9'`; reopen; still `9.9.9`);
  version-compare unit tests (`0.1.9 < 0.1.10`, `1.0.0-rc.1` equals `1.0.0`, garbage is an error).
- `upgrade.rs`: `upgrade_from_previous_schema_keeps_every_row` (section 8).
- `commit_cost.rs`, `#[ignore = "benchmark: run with --ignored --nocapture"]`:
  `commit_latency_with_full_synchronous`: on a temp-dir database, time 300 single-incident commits
  (each an `Incident New` plus one `IncidentEvent`), print count, median, p95 and max in
  milliseconds using `eprintln!`, and the same for 300 repository `insert` calls for comparison. It must not
  assert on timings. Run it once with `cargo test -p domovoi-store --test sqlite commit_latency -- --ignored --nocapture`
  and report the numbers (R-6). Tests never sleep; this one uses the real clock only to measure.

Testkit tests (`crates/domovoi-testkit/tests/contracts.rs`): the 17 contract functions on
`InMemoryStore::new()` (names `in_memory_<behaviour>`), plus
`failed_commit_injected_by_fail_next_changes_nothing` (arm `fail_next(StoreError::Backend("disk full".to_owned()))`;
the commit returns `CommitError::Backend(s)` where `s` is the injected error's `to_string()`, which
contains `disk full`; state is unchanged; the next commit succeeds).

Fakes: `InMemoryStore` (existing, extended), `FakeClock` is not needed (the port takes timestamps in
its drafts). New: `UowSubject` **(new)**.

## 13. Acceptance criteria

1. `domovoi_core::ports` exports `UnitOfWork`, `Transaction`, `StateWrite`, `JournalDraft`,
   `AuditDraft`, `EffectDraft`, `EffectCompletion`, `CommitError`, `CommitReceipt`, `VersionedRead`,
   `JournalRead`, `EffectRead` with the shapes in section 6.1, and `domovoi_core::EffectId` exists.
2. `Store` and `InMemoryStore` implement `UnitOfWork`, `VersionedRead`, `JournalRead`, `EffectRead`, and
   all 17 contract functions pass on both.
3. A failed commit (conflict, constraint, stale effect) leaves state, journal, audit and effects
   exactly as before, on both stores (contract tests 3, 5, 6, 9, 15).
4. Incident timeline writes through the unit of work create both the `incident_events` row and a
   journal row with `legacy_event_seq` set (SQLite test).
5. Migration `NNNN_unit_of_work.sql` creates `journal`, `effects`, `schema_compat` and the two
   `version` columns exactly as in section 8 and is additive.
6. Every SQLite connection runs with `synchronous = FULL` (`durability.rs`).
7. `Store::open` runs `quick_check` and fails with `StoreOpenError::Integrity` when it reports a problem
   (or the parser test proves it, if you used the fallback).
8. `Store::open` refuses a database whose `schema_compat.min_reader` exceeds the binary version
   before touching the file, and ignores applied migrations it does not know.
9. `upgrade_from_previous_schema_keeps_every_row` passes against the committed N-1 fixture.
10. All existing repository contract suites and existing tests in `domovoi-store`, `domovoi-testkit`
    and (via `rdeps`) `domovoi-engine`, `domovoi-hub` still pass unchanged.
11. `cargo xtask arch-check` passes; no non-test source file exceeds 500 lines; no
    `#[allow]`, `unwrap`, `expect`, `panic!` in library code.
12. The R-6 benchmark exists, is `#[ignore]`d, and its measured numbers appear in the completion report.
13. `ARCHITECTURE.md` and the book data-model page are updated as in section 15.

## 14. Migration and compatibility

- Existing installs: opening an existing database takes the usual `pre-migrate-*` backup (new pending
  migration), adds the tables and columns, gives every existing incident and note `version = 1`,
  writes the `schema_compat` row, runs `quick_check`. The first start after upgrade is slower on a
  very large database because of `quick_check` (O(database size)); say so in the PR description.
- `synchronous=FULL` adds an fsync per commit. Report the measured cost; the book states it.
- Existing incident rows have no `schema_version` in `body`; they gain it on their next write.
- Existing code keeps working because nothing calls the unit of work yet.
- Agents and the wire protocol are unaffected.
- Rollback: the previous binary opens the upgraded database (it ignores the new tables and columns),
  except that its `Migrator` refuses unknown applied migrations; therefore the supported rollback
  from the *next* release forward uses `ignore_missing`, and a rollback to a binary older than this
  one is a forward-fix or restore situation (RW-31). Mention it in the book page.

## 15. Documentation requirements

- `ARCHITECTURE.md` section 6 (Ports): add rows for `UnitOfWork` (and `VersionedRead`, `JournalRead`,
  `EffectRead`): purpose "commit state writes, journal, audit and effects atomically", implemented by
  store and testkit, used by engine (prompts 10 and 11 wire it in; say "being adopted"). Section 9
  (Persistence): add tables `journal`, `effects`, `schema_compat`; the `version` columns; WAL with
  `synchronous=FULL`; start-up `quick_check`; the compatibility rule.
- `docs/book/src/reference/data-model.md`: add rows to the Tables table for `journal`, `effects`,
  `schema_compat`; a note under Conventions that new enum-like columns are validated by the program,
  not by `CHECK`; a "Durability" paragraph (every commit is flushed to disk; an integrity check runs
  at start-up; what a failed check means: restore a backup); a "Compatibility" paragraph describing
  `schema_compat` and the refusal message.
- No config keys are added, so `cargo xtask docs-config` is not needed. No API change, so no OpenAPI
  regeneration. Run `cargo xtask docs-links` if you add links.
- Do not mention prompts, prompt numbers, the build plan, or coding agents in any committed file or
  commit message. Write "coming in a later release" if you need to refer to future work.

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
