# 09 — Audit hash chain, Actor identity and audit verification

| | |
|---|---|
| Branch | `audit-hash-chain` |
| Milestone | F1 |
| Depends on (must be merged) | 08 |
| Can run in parallel with | any prompt that adds no migration; **not** with a prompt that adds one (08 added `0006` and `0007`; see section 8 and index §7) |
| Produces (shared contracts) | `domovoi_core::Actor`; `domovoi_core::audit_chain::{AuditAnchor, ChainRow, ChainVerifier, canonical_json, entry_hash, LegacyDigest, VerifyReport, ...}`; table `audit_anchor`; columns `audit_log.prev_hash`, `audit_log.hash`; `Store::verify_audit`, `Store::audit_head`; CLI `domovoi audit verify` and `domovoi audit head`; `AuditDraft` and `JournalDraft` now carry an `Actor` |
| Consumes | prompt 08: `UnitOfWork`, `Transaction`, `AuditDraft`, `JournalDraft`, `insert_audit` (the single place that inserts into `audit_log`), `Store::open` sequence |
| Launch gate | none |
| Suggested model | medium: security-sensitive, but the encoding is fully specified below |

## 1. Objective

Make the audit log tamper-evident and give every audit and journal entry a typed actor.

1. Add an `Actor` enum (`User`, `System`, `Agent`, `Policy`) with a stable string form.
2. Add an anchor over the existing audit history and hash-chain every audit row written from now on.
   Existing rows are never rewritten.
3. Every audit row, whether written through the unit of work or through `AuditLog::append`, joins the
   chain inside the same SQLite transaction that inserts it.
4. Verify the chain: `domovoi audit verify`, `domovoi audit head`, and a bounded check at hub start.
   Legacy (unchained, digest-checked) and chained history are reported separately.

## 2. Architectural context

- **Why (D-14, RW-27).** Audit rows are plain rows protected only by triggers that anyone with file
  access can drop. A hash chain detects accidental corruption, edits by tools that do not recompute the
  chain, and mid-chain deletion. It does **not** stop an attacker who rewrites the whole database and
  recomputes every hash, and it does not detect deletion of the newest rows unless the head hash was
  recorded elsewhere (that is what `domovoi audit head` is for). Say so in the docs; do not overclaim.
- **One scheme for new and migrated installs.** A migration cannot compute SHA-256 over history (SQLite
  has no SHA-256). So the migration only creates the empty table `audit_anchor` and the two new columns;
  `Store::open` then writes the single anchor row in Rust, inside one `BEGIN IMMEDIATE` transaction,
  if none exists. The anchor commits to: an install id, the highest legacy row id, the number of legacy
  rows, and a SHA-256 digest over every legacy row. A fresh install writes the same anchor with count 0
  and the digest of the empty string. The first chained row's `prev_hash` is the anchor hash.
- **Who computes hashes.** The store, inside the commit transaction (RW-03). Hash functions, canonical
  encoding and the pure verifier live in `domovoi-core` (no I/O, `sha2` is already allowed there) so the
  encoding is defined once and unit-tested with golden vectors.
- **Actor (D-13, RW-29).** Single owner through v1, no RBAC, but every stored identity is already an
  `Actor`: `User(UserId) | System(component) | Agent(HostId) | Policy(rule id)`. Today audit rows carry
  strings such as `user:<uuid>`, `system:actions`, and for chat approvals `chat:discord:<id>`. The new
  enum's string form matches the first two exactly, so existing writers keep working. Chat approvals
  become `user:<UserId>` with the channel already recorded in the row's detail (prompt 10 changes that
  writer; this prompt does not).
- **Pruning and checkpoints.** Audit rows are retained forever by default (D-21). If an owner later
  configures pruning, the pruner writes a chained `audit.checkpoint` row before deleting. This prompt
  defines the checkpoint row and makes the verifier understand it; it does **not** implement pruning
  (the table's delete trigger stays).
- **Restore.** A restored database verifies as it was at backup time. The "restore is the first new
  chained row" rule belongs to the restore command (prompt 19), not here.
- **Failure policy at start (decided here).** A broken chain at start does not stop the hub (an owner
  could not fix it otherwise, and the chain is evidence, not a gate). The hub logs an error and writes
  one chained audit row `audit.chain_broken` (skipped if the newest such row already names the same
  first broken sequence), so the break is visible on the audit page later.

## 3. Repository context

Verify each item in step 1. Paths are relative to `C:\Programming\domovoi`. Items marked (08) are
introduced by prompt 08; verify the real names in the merged code.

- `crates/domovoi-core/src/ports/repos/audit.rs`: `AuditEvent { at: Timestamp, actor: String, kind,
  subject, detail: Value }`, `AuditFilter`, `AuditLog::{append, list}`.
- `crates/domovoi-store/src/repos/audit.rs`: `AuditLog` impl; (08) the shared function `insert_audit`.
  `audit_log` columns: `seq INTEGER PRIMARY KEY AUTOINCREMENT, at INTEGER (unix ms), actor TEXT,
  kind TEXT, subject TEXT, detail TEXT (JSON, default 'null')`; triggers `audit_log_no_update`,
  `audit_log_no_delete`, `audit_log_no_replace` (`migrations/0001_init.sql`).
- (08) `crates/domovoi-store/src/uow/` commit path: calls `insert_audit` for each `AuditDraft`.
  (08) `AuditDraft { event: AuditEvent }`, `JournalDraft { actor: String, .. }`,
  `timeline_journal` uses actor `"system:incidents"`.
- (08) `Store::open` sequence in `crates/domovoi-store/src/db.rs`: ... migrations, `compat::record`,
  `quick_check`, read pool.
- `crates/domovoi-engine/src/actions/audit.rs`: builds audit events; actor strings `user:<id>`,
  `system:actions`, `system:<component>`, and `chat:<platform>:<user>` (`fn actor(approval)`, line ~58).
  Other writers call `AuditLog::append` directly: `crates/domovoi-hub/src/api/rules.rs:300,418`,
  `api/settings_write.rs:477`, `auth/interactions/mod.rs:157`, `settings/service.rs:414`,
  `crates/domovoi-engine/src/triage/proposals.rs:142`.
- CLI: `crates/domovoi/src/cmd/mod.rs` (`Command` enum, `run`, `block_on`, `fail`),
  `cmd/hub.rs` (pattern for commands that open the store with the hub stopped, via
  `domovoi_hub::admin::OfflineAdmin::open(config, clock)`), `cmd/digest.rs` (smallest example),
  `HubConfigArg` in `cmd/mod.rs`. `crates/domovoi-hub/src/admin/mod.rs`: `OfflineAdmin` holds the
  `HubLock` and a `Store`; methods `open`, `agents`, `revoke`, `send_digest`, `close`.
- Hub start: `crates/domovoi-hub/src/runtime/mod.rs:159`
  (`let store = Arc::new(adapters::open_store(&config, &*deps.clock).await?);`), `runtime/adapters.rs:16`.
- Tests: `crates/domovoi-store/tests/sqlite/audit.rs`, `tests/sqlite/common.rs` (`Fresh`),
  fixture (08) `crates/domovoi-store/tests/fixtures/n1-schema5.db` and
  `tests/sqlite/upgrade.rs`; `crates/domovoi-testkit/src/repos/audit.rs` (in-memory `AuditLog`).
- Core allowlist for external crates includes `sha2`, `hex`, `serde_json`, `jiff`, `uuid`, `thiserror`
  (`xtask/src/arch/rules.rs`). Core has `src/sha256.rs` (a macro for hex newtypes), not needed here.
- No install id exists anywhere yet.

## 4. Exact scope

**Core (`crates/domovoi-core`)**
- `src/actor.rs` (new): `Actor` and its parsing and display. Export from `lib.rs`.
- `src/audit_chain.rs` (new, may split into `audit_chain/{mod,encode,verify}.rs`, each at most 500 lines):
  anchor, canonical JSON, entry hash, legacy digest, `ChainVerifier`, `VerifyReport` and friends.
- `ports/uow/journal.rs` (08): `JournalDraft.actor` becomes `Actor`; `AuditDraft` becomes
  `{ actor: Actor, at, kind, subject, detail }` with `AuditDraft::new(actor, at, kind, subject, detail)`
  and `fn into_event(self) -> AuditEvent` (actor via `Display`). `timeline_journal` uses
  `Actor::System("incidents".to_owned())`.
- `JournalEntry.actor` and `AuditEvent.actor` stay `String` (they must represent legacy values like
  `chat:discord:42`).

**Store (`crates/domovoi-store`)**
- `migrations/NNNN_audit_chain.sql` (section 8).
- `src/audit_chain.rs` (new): `insert_audit` rewritten to chain; `ensure_anchor`; reads for verify.
- `src/audit_verify.rs` (new): `Store::verify_audit`, `Store::audit_head`, `Store::audit_anchor`.
- `src/db.rs`: call `audit_chain::ensure_anchor` in the open sequence.
- `src/repos/audit.rs`: `append` runs in its own `BEGIN IMMEDIATE` transaction around `insert_audit`.
- `src/uow/*`: audit drafts use `into_event` then `insert_audit` (signature change is internal).

**Testkit (`crates/domovoi-testkit`)**: adjust for the new `AuditDraft`/`JournalDraft` types; contract
functions from 08 that build drafts are updated. `InMemoryStore` does **not** chain; document that in
its rustdoc.

**Hub (`crates/domovoi-hub`)**
- `src/admin/mod.rs`: `OfflineAdmin::verify_audit(scope)` and `OfflineAdmin::audit_head()`.
- `src/runtime/adapters.rs` (or `runtime/mod.rs`): `verify_audit_on_start(&Store, &dyn Clock)` called
  right after the store is opened at hub start (the call at `runtime/mod.rs:159` only; not in the
  admin or settings-apply paths).
- Constant `pub const STARTUP_AUDIT_VERIFY_ROWS: u64 = 10_000;` in the hub crate.

**Binary (`crates/domovoi`)**
- `src/cmd/audit.rs` (new) and registration in `src/cmd/mod.rs`: `domovoi audit verify` and
  `domovoi audit head`.

**Docs** (section 15).

## 5. Explicit non-goals

- No change to who writes audit rows and no change to what they contain: the action writer, triage,
  the hub APIs keep building today's events. (Prompt 10 migrates the action writer and the chat-actor
  mapping.)
- No pruning, no `audit.checkpoint` writer, no `audit.restored` row, no restore command.
- No `authorize(actor, action)` function (RW-29): it has no caller yet and arrives with the use-case
  layer. Do not add dead code for it.
- Head hash in the daily digest (RW-27 "cheap anchor") is **not** done here; only `domovoi audit head`.
  Say this in the PR description.
- No config key for the startup verification window; the constant above is the default. No new HTTP
  endpoint or UI (the audit page is a later prompt), so no OpenAPI regeneration.
- No change to the append-only triggers on `audit_log`; no new trigger on `audit_log`.
- No `advisories` change. None is needed here; after prompt 08 a new advisory kind would be a Rust-only change.
- Do not edit merged migrations; do not rebuild `audit_log`.
- Do not touch `crates/domovoi-engine/src/actions/` or `manager/` (prompts 10 and 11).

## 6. Required interfaces and contracts

### 6.1 `Actor` (**fixed**)

```rust
// crates/domovoi-core/src/actor.rs
/// Who did something. Stored in audit and journal rows in its string form.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Actor {
    /// A human user.
    User(UserId),
    /// A Domovoi component, named in kebab or snake case (for example `actions`, `incidents`).
    System(String),
    /// An agent, by the host it runs on.
    Agent(HostId),
    /// A policy rule, by its id (rules arrive in a later release).
    Policy(String),
}
// Display: "user:<uuid>", "system:<component>", "agent:<host uuid>", "policy:<rule id>".
// FromStr: the inverse. `System` and `Policy` names must be 1..=64 chars of [a-z0-9._-];
// anything else (including the legacy "chat:<platform>:<user>") is Err(ActorParseError).
// serde: serializes as the Display string, deserializes through FromStr.
impl Actor {
    /// A system actor; the component name is validated like `FromStr` does.
    pub fn system(component: &str) -> Result<Self, ActorParseError>;
}
```
`ActorParseError` is a `thiserror` enum. Define constants for the components used by this release in
`actor.rs`: `pub const SYSTEM_INCIDENTS: &str = "incidents";`, `SYSTEM_ACTIONS: &str = "actions"`,
`SYSTEM_AUDIT: &str = "audit"`. (Library code cannot `unwrap`; where a constant component is needed,
construct `Actor::System(SYSTEM_INCIDENTS.to_owned())` directly.)

### 6.2 Encoding (**fixed, byte-exact**)

All hashes are SHA-256. Integers are big-endian. `‖` is plain byte concatenation.

**Canonical JSON of an audit event** (`canonical_json(&AuditEvent) -> String`): the compact JSON
object with exactly these five keys in this (alphabetical) order:

```
{"actor":<string>,"at":<integer unix milliseconds>,"detail":<value>,"kind":<string>,"subject":<string>}
```

`at` is `event.at.as_millisecond()` (the store keeps milliseconds). `detail` is the event's `Value`
with **every object's keys recursively sorted** lexicographically by UTF-8 bytes, serialized with
`serde_json::to_string` (compact, no spaces; non-ASCII kept as UTF-8). Implement the key sort yourself
(do not rely on serde_json's map type; the `preserve_order` feature may be on elsewhere). `Value::Null`
serializes as `null`.

**Entry hash** (`entry_hash(seq: u64, prev: &[u8; 32], event) -> [u8; 32]`):

```
SHA-256( "domovoi-audit-v1" ‖ u64be(seq) ‖ prev(32 bytes) ‖ canonical_json(event) as UTF-8 )
```

**Legacy digest** (`LegacyDigest`, a streaming hasher): for each legacy row in ascending `seq`,
update with `u64be(seq) ‖ u32be(len(c)) ‖ c`, where `c` = `canonical_json(event)` bytes (the stored
`detail` text is parsed to a `Value` first, so key order in the stored text does not matter).
`finish()` returns `(count, digest)`. With no rows the digest is SHA-256 of the empty string
(`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`).

**Anchor hash** (`AuditAnchor::hash()`):

```
SHA-256( "domovoi-audit-anchor-v1" ‖ u32be(len(install_id)) ‖ install_id (UTF-8)
         ‖ u64be(legacy_last_id) ‖ u64be(legacy_count) ‖ legacy_digest(32 bytes) )
```

**Golden vectors** (computed independently; your unit tests must reproduce them exactly):

| Case | Input | Output (hex) |
|---|---|---|
| anchor, fresh install | install_id `install-1`, last id 0, count 0, digest = SHA-256 of empty | `9f0e006afbfa18e042a21bfb571b46792ee6a222f79fb448aeb0d026f93fd549` |
| canonical JSON, row 1 | actor `user:0190a0a0-0000-7000-8000-000000000001`, at `1700000000123`, kind `action.approved`, subject `action:0190a0a0-0000-7000-8000-000000000002`, detail `{"b":1,"a":[true,null]}` | `{"actor":"user:0190a0a0-0000-7000-8000-000000000001","at":1700000000123,"detail":{"a":[true,null],"b":1},"kind":"action.approved","subject":"action:0190a0a0-0000-7000-8000-000000000002"}` |
| hash, row 1 | seq 1, prev = the fresh anchor hash above, the event above | `a4f75c26c56d0ac324b1d31418ca4281388129b0eca508e3b0fca480ebfe0864` |
| canonical JSON, row 2 | actor `system:actions`, at `1700000001000`, kind `action.dispatched`, subject `action:0190a0a0-0000-7000-8000-000000000002`, detail `null` | `{"actor":"system:actions","at":1700000001000,"detail":null,"kind":"action.dispatched","subject":"action:0190a0a0-0000-7000-8000-000000000002"}` |
| hash, row 2 | seq 2, prev = hash of row 1, event row 2 | `fd2a3eb162101f8b87ea3904222b963f9d9a30120703b0a292dc0291da612436` |
| legacy digest | rows (1: actor `user:u1`, at `1700000000000`, kind `login`, subject `user:u1`, detail null), (2: actor `system:actions`, at `1700000000500`, kind `action.expired`, subject `action:a1`, detail `{"x":1}`) | `9acfd54233b8d05352496b07960b45dcf81083510c76d9740e291df079d76cbe` |
| anchor over those two rows | install_id `install-2`, last id 2, count 2, that digest | `41a8cf272a78ac1655662964bdd8858f6eebfaf5c0e14dc38bcf37208c79f6ad` |

(Timestamps in the tests are built from the millisecond values with `Timestamp::from_millisecond`.)

### 6.3 Core verifier API (**fixed names**, bodies pure)

```rust
// crates/domovoi-core/src/audit_chain.rs
pub type Hash = [u8; 32];

pub struct AuditAnchor { pub install_id: String, pub legacy_last_id: u64, pub legacy_count: u64, pub legacy_digest: Hash }
impl AuditAnchor { pub fn hash(&self) -> Hash; }

/// One stored audit row with its chain fields (None on legacy rows).
pub struct ChainRow { pub seq: u64, pub prev_hash: Option<Hash>, pub hash: Option<Hash>, pub event: AuditEvent }

pub const CHECKPOINT_KIND: &str = "audit.checkpoint";
/// Detail of a checkpoint row: {"first_retained_seq": u64, "first_retained_prev_hash": "<64 hex>",
/// "pruned_count": u64, "legacy_pruned": bool}.

pub enum ChainStart {
    /// The first row must have `prev_hash == anchor_hash` and `seq == first_seq`.
    Anchor { anchor_hash: Hash, first_seq: u64 },
    /// Verify from a row in the middle: the first row's own `prev_hash` is accepted as given.
    Trusted,
}
pub enum BreakReason {
    MissingChainFields,   // a row after the anchor has no prev_hash or hash
    HashMismatch,         // recomputed hash differs from the stored hash
    LinkMismatch,         // prev_hash differs from the previous row's hash (or the anchor hash)
    SequenceGap,          // seq is not previous + 1
    CheckpointMismatch,   // retained start does not match the newest checkpoint
}
pub struct ChainBreak { pub seq: u64, pub reason: BreakReason }

pub struct ChainVerifier { /* private */ }
impl ChainVerifier {
    pub fn new(start: ChainStart) -> Self;
    /// Feed rows in ascending `seq`. Stops being useful after the first `Err`. Checks, in this
    /// order: chain fields present (`MissingChainFields`), `seq` is previous + 1 once a previous row
    /// or an anchor `first_seq` is known (`SequenceGap`), `prev_hash` equals the previous hash or
    /// the anchor hash (`LinkMismatch`; skipped for the first row of a `Trusted` start), and
    /// `entry_hash(seq, prev_hash, event) == hash` (`HashMismatch`).
    pub fn push(&mut self, row: &ChainRow) -> Result<(), ChainBreak>;
    pub fn checked(&self) -> u64;
    pub fn head(&self) -> Option<(u64, Hash)>;
}

pub enum LegacyStatus {
    Verified { count: u64 },
    Mismatch { expected_count: u64, actual_count: u64, digest_matches: bool },
    CountOnly { count: u64 },      // startup check: count only, digest not computed
    NotVerifiable,                 // legacy rows were pruned (checkpoint says so)
}
pub enum ChainStatus {
    Empty,
    Intact { checked: u64, head_seq: u64, head_hash: Hash },
    Broken { checked: u64, first_broken_seq: u64, reason: BreakReason },
}
pub struct VerifyReport { pub legacy: LegacyStatus, pub chained: ChainStatus, pub anchor: AuditAnchor }
pub enum VerifyScope { Full, Last(u64) }
```
All public items get doc comments. `BreakReason` has `Display` giving a short plain sentence each
(used by the CLI), for example "a row was edited" for `HashMismatch`, "a row was removed or
inserted" for `LinkMismatch`, "rows are missing" for `SequenceGap`.

### 6.4 Store API (**fixed**)

```rust
impl Store {
    /// Verifies the audit history. `Full` checks the legacy digest and every retained chained row;
    /// `Last(n)` checks the legacy row count only and the newest n chained rows.
    pub async fn verify_audit(&self, scope: VerifyScope) -> Result<VerifyReport, StoreError>;
    /// The newest chained row's sequence number and hash, or `None` before the first chained row.
    pub async fn audit_head(&self) -> Result<Option<(u64, Hash)>, StoreError>;
    /// The anchor, `None` only if the store was not opened through `Store::open`.
    pub async fn audit_anchor(&self) -> Result<Option<AuditAnchor>, StoreError>;
}
```

`insert_audit` (rewritten, **fixed behaviour**), inside an already open `BEGIN IMMEDIATE` transaction on
the single writer connection (`&mut SqliteConnection`):

1. Load `prev`: the `hash` of the row with the highest `seq` that has a non-NULL `hash`; if none, the
   anchor row's `anchor_hash`. If there is no anchor row return `StoreError::Backend("audit anchor is missing")`.
2. `seq` = `max(sqlite_sequence.seq for 'audit_log' (0 if absent), MAX(seq) of audit_log (0 if empty)) + 1`.
3. `hash = entry_hash(seq, &prev, &event)` where `event` is the event **as it will be stored** (timestamp
   truncated to milliseconds, `detail` as the value that round-trips from the stored JSON text).
4. `INSERT INTO audit_log (seq, at, actor, kind, subject, detail, prev_hash, hash) VALUES (...)` with the
   explicit `seq`. Return it.

`AuditLog::append` wraps `insert_audit` in `begin_with("BEGIN IMMEDIATE")` ... `commit`. Both entry points use the same function.

`ensure_anchor` (**fixed behaviour**), called in the open sequence after migrations (and after
`compat::record`), before `quick_check`:

1. `BEGIN IMMEDIATE`. If `audit_anchor` has its row, commit and return.
2. Otherwise stream every `audit_log` row in `seq` order into a `LegacyDigest` (parse `detail` to a
   `Value`), take `legacy_last_id` = highest `seq` (0 if none), `legacy_count` = number of rows,
   `install_id` = a fresh UUID v4 string, `anchor_hash = AuditAnchor::hash()`.
3. Insert the anchor row (including `anchor_hash` and `created_at = options.now` in ms), commit.

On an existing database that already contains chained rows but no anchor (cannot happen through
supported paths) return `StoreOpenError::Database("audit chain exists without an anchor")`.

### 6.5 `Store::verify_audit` algorithm (**fixed**)

```
anchor = audit_anchor()?  (missing -> Err(Backend))
Full:
  legacy: if a checkpoint row's detail says legacy_pruned -> NotVerifiable
          else read rows with seq <= anchor.legacy_last_id in ascending order in batches of 1000,
          feed LegacyDigest; compare count and digest with the anchor
          -> Verified{count} or Mismatch{expected_count, actual_count, digest_matches}
  chained: the chained range is every row with seq > anchor.legacy_last_id (rows without chain
           fields included, so an intruder row is seen)
           first = lowest-seq row of the chained range (LIMIT 1); none -> Empty
           if first.seq == anchor.legacy_last_id + 1 -> ChainStart::Anchor{anchor.hash(), first.seq}
           else: cp = newest row with kind 'audit.checkpoint' whose detail.first_retained_seq == first.seq
                 none, or first has no prev_hash, or cp.detail.first_retained_prev_hash != hex(first.prev_hash)
                   -> Broken{first_broken_seq: first.seq, reason: CheckpointMismatch}
                 else ChainStart::Trusted
           stream the chained range in batches of 1000 into ChainVerifier
Last(n):
  legacy: count rows with seq <= legacy_last_id; equal to anchor.legacy_count -> CountOnly{count}
          else Mismatch{.., digest_matches: false}
  chained: the n newest rows of the chained range (n clamped to >= 1) in ascending order,
           ChainVerifier(Trusted); if the window's first row has seq == anchor.legacy_last_id + 1 use
           ChainStart::Anchor instead
Report Intact{checked, head} or Broken{..}.
```
A legacy row is any row with `seq <= anchor.legacy_last_id`. Rows after it with a NULL `hash` are
`MissingChainFields` breaks (a tool inserted a row without computing the chain). `audit_head` and the
`prev` lookup in `insert_audit` use the newest row whose `hash` is not NULL.

### 6.6 CLI (**fixed text shape**, **convention** for wording)

```
$ domovoi audit verify [--config <file>] [--last <n>]
Audit log of <data_dir>/hub.db
Legacy history:  verified, 1204 rows (digest matches the anchor)
Chained history: intact, 311 rows checked, head seq 1515 hash 9f0e00...
$ echo $?   # 0
```
Broken chain: print `Chained history: BROKEN at seq <n>: <BreakReason display>` and exit code 1
(`ExitCode::FAILURE`). Legacy mismatch: `Legacy history:  CHANGED: expected <e> rows, found <a>
(digest <matches|differs>)` and exit code 1. `NotVerifiable` prints `not verifiable (older rows were pruned)`
and does not fail. Without `--last`, scope is `Full`; with it, `Last(n)`. `domovoi audit head` prints
`seq <n> hash <64 hex>` or `no chained rows yet`. Both commands need the hub stopped (same lock as
`digest now`); add them to the cli.md list of such commands.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open everything in section 3.
   Confirm prompt 08 is merged and read its real names: `insert_audit`, `AuditDraft`, `JournalDraft`,
   `Store::open` sequence, the highest migration number. List differences before editing.
2. **Core: `Actor`.** Implement, with unit tests (round trip of all four variants, rejects
   `chat:discord:1`, empty and over-long names, uppercase, `user:not-a-uuid`; serde round trip).
3. **Core: `audit_chain`.** Implement encoding, `LegacyDigest`, `AuditAnchor::hash`, `ChainVerifier`,
   report types. Unit tests for every golden vector in 6.2, plus the verifier tests in section 12.
4. **Core: update `AuditDraft` and `JournalDraft`** to carry `Actor`; fix the contract functions in
   the testkit that build them.
5. **Migration** (section 8). Run the existing store tests.
6. **Store: `insert_audit` chaining, `append` transaction, `ensure_anchor`, open sequence.** Run the
   existing audit contract (`sqlite_audit_log_satisfies_contract`): it must still pass unchanged.
7. **Store: `verify_audit`, `audit_head`, `audit_anchor`.**
8. **Hub: `OfflineAdmin` methods and `verify_audit_on_start`.**
9. **Binary: `audit verify` and `audit head`.**
10. **Tests** (section 12), including the extension of the N-1 upgrade test.
11. **Docs** (section 15).
12. **Run the checks and write the completion report.** `cargo fmt --all`;
    `cargo clippy -p domovoi-core -p domovoi-store -p domovoi-testkit -p domovoi-hub -p domovoi --all-targets -- -D warnings`;
    `cargo nextest run -p domovoi-core -p domovoi-store -p domovoi-testkit -p domovoi-hub -p domovoi`;
    `cargo nextest run -E 'rdeps(domovoi-core)'`; `cargo xtask arch-check`; `cargo xtask docs-links`.

## 8. Database and persistence

Migration `crates/domovoi-store/migrations/NNNN_audit_chain.sql`. Choose `NNNN` by listing the directory
on your branch and taking the next unused number; do not take a number from this document or from a
plan. Prompt 08 added `0006_unit_of_work.sql` and `0007_advisories_kind_unchecked.sql`, so a branch that
contains it (such as `overhaul-investigation`, via `v2-overhaul`) ends at `0007` and the next free number
there is `0008`, or `0009` if prompt 13 merged first. `main` still ends at `0005`, where the next free
number is `0006`. Verify the directory and rely on none of these numbers. If the branch you land on
gained a migration first, renumber your own unmerged file; never edit or renumber a merged migration.
Tests and the completion report refer to the file by its contents (`_audit_chain`), not its number.
Additive only; **do not rebuild `audit_log`** (triggers). Adding nullable columns with `ALTER TABLE`
does not fire the triggers.

```sql
-- Hash chain fields. NULL on rows written before the chain existed (the "legacy" rows).
ALTER TABLE audit_log ADD COLUMN prev_hash BLOB CHECK (prev_hash IS NULL OR length(prev_hash) = 32);
ALTER TABLE audit_log ADD COLUMN hash      BLOB CHECK (hash      IS NULL OR length(hash)      = 32);

-- One row, written by Store::open (the digest needs SHA-256, which SQLite lacks).
CREATE TABLE audit_anchor (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    install_id     TEXT    NOT NULL,
    legacy_last_id INTEGER NOT NULL,
    legacy_count   INTEGER NOT NULL,
    legacy_digest  BLOB    NOT NULL CHECK (length(legacy_digest) = 32),
    anchor_hash    BLOB    NOT NULL CHECK (length(anchor_hash) = 32),
    created_at     INTEGER NOT NULL
) STRICT;
CREATE TRIGGER audit_anchor_no_update BEFORE UPDATE ON audit_anchor BEGIN
    SELECT RAISE(ABORT, 'audit_anchor is append-only');
END;
CREATE TRIGGER audit_anchor_no_delete BEFORE DELETE ON audit_anchor BEGIN
    SELECT RAISE(ABORT, 'audit_anchor is append-only');
END;
```

If SQLite rejects `ADD COLUMN ... CHECK` on your bundled version, drop the `CHECK` clauses and validate
lengths in Rust (`hash32` in `codec.rs`); say so in the report.

Transaction boundaries: `append` is one `BEGIN IMMEDIATE` transaction (read previous hash, compute,
insert). `insert_audit` inside `Store::commit` runs in the commit's transaction; if any later step
fails, the audit row and its chain position roll back with everything else, so the chain never has a
hole. `ensure_anchor` is one transaction.

**N-1 upgrade test (extend 08's).** `upgrade_from_previous_schema_keeps_every_row` (08) opens the
fixture `n1-schema5.db`. After this change it must additionally assert (add a new test
`upgrade_writes_an_anchor_over_legacy_audit_rows`, do not weaken the old one): `audit_anchor` has one
row; `legacy_count` = the fixture's 5 audit rows; `legacy_last_id` = 5; recomputing the digest in the test
with `LegacyDigest` over the rows read through `AuditLog::list` gives `legacy_digest`; `verify_audit(Full)`
reports `Verified{count: 5}` and `Empty`; one `AuditLog::append` afterwards is chained
(`prev_hash` = anchor hash, `seq` = 6) and `verify_audit(Full)` reports `Intact{checked: 1, ..}`.

## 9. Protocol and compatibility

No wire change; protocol stays 1.1. No new HTTP endpoint.

Compatibility: the audit string format for `user:` and `system:` actors is unchanged, so existing
readers (hub audit API, web) keep working. Chat approvals still write `chat:<platform>:<user>` until
prompt 10 changes that writer. A binary from before this change reading the upgraded database ignores
the new columns and table (additive), but if it appends audit rows it writes them without chain fields;
the next verification reports `MissingChainFields` at the first such row. That is the intended behaviour
(it detects tools that do not compute the chain) and is why rolling back across this release is
documented as "verify will report the rows written by the old binary".

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `AuditLog::append` or an audit draft in a commit fails (I/O, constraint) | the transaction rolls back; for `append` the caller gets `StoreError::Backend`; for a commit the whole transaction fails (`CommitError::Backend` or `Constraint`) |
| Anchor row missing when appending | `StoreError::Backend("audit anchor is missing")`; nothing written. `Store::open` always creates it, so this only happens if the file was edited |
| Open finds chained rows but no anchor | `Store::open` fails with `StoreOpenError::Database("audit chain exists without an anchor")` |
| Row edited (content or hash changed) | `verify_audit` returns `Broken{first_broken_seq: that row, reason: HashMismatch}` |
| Row deleted mid-chain | `Broken{..., reason: SequenceGap}` at the row after the gap (checked before link) |
| Row inserted by a tool without chain fields (after the anchor) | `Broken{..., reason: MissingChainFields}` at that row |
| Row inserted with fields that do not link | `Broken{..., LinkMismatch}` |
| Oldest rows deleted without a matching checkpoint | `Broken{first_broken_seq: first retained, reason: CheckpointMismatch}` |
| Newest rows deleted | not detectable by the chain; `audit head` compared with a head recorded elsewhere shows it. Documented, not an error |
| Legacy row edited or deleted | `Mismatch{expected_count, actual_count, digest_matches: false}`; the report does not say which row |
| Chain broken at hub start | log `error`, append `audit.chain_broken` (actor `system:audit`, subject `audit`, detail `{"first_broken_seq", "reason", "checked"}`) unless the newest such row has the same `first_broken_seq`; hub keeps starting |
| `verify_audit` read error | `StoreError::Backend`; at hub start log `warn` and continue |
| `--last 0` | treated as 1 |
| CLI while a hub runs | refuses with the existing "a hub is already running" error |

## 11. Security requirements

- **Process identity / authorization:** no new authority. `audit verify` and `audit head` are read-only
  CLI commands that open the database under the hub lock as the current OS user.
- **Append-only is preserved.** No code path updates or deletes `audit_log` or `audit_anchor` rows;
  the existing triggers and the new `audit_anchor` triggers stay. Tests assert an `UPDATE`/`DELETE` on
  `audit_anchor` is refused.
- **One insert path.** `insert_audit` is the only code that inserts into `audit_log`. Add a test or a
  `rg` check in the completion report: `INSERT INTO audit_log` appears in exactly one source file
  (`crates/domovoi-store/src/audit_chain.rs`). Migrations and tests excluded.
- **Fail closed in commits**: a failed audit insert fails the whole transaction (prompt 10 depends on it).
- **No secrets** in anything hashed or logged: the CLI prints sequence numbers and hashes only; the
  `audit.chain_broken` detail contains no row contents.
- **HumanApproval** untouched.
- **Hash comparison** uses ordinary equality (the values are not secrets).
- Honest limits are written in the book (section 15): whole-database rewrite and tail truncation are
  out of scope of this mechanism.

## 12. Testing requirements

**Core unit tests** (`crates/domovoi-core`, in `audit_chain` and `actor` modules):
- `canonical_json_sorts_keys_recursively_and_is_compact`, `entry_hash_matches_golden_vectors`,
  `anchor_hash_matches_golden_vectors`, `legacy_digest_matches_golden_vector`,
  `legacy_digest_of_no_rows_is_the_empty_digest`.
- `verifier_accepts_a_valid_chain_from_the_anchor`, `verifier_detects_an_edited_row` (change `detail`,
  keep stored hash: `HashMismatch` at that seq), `verifier_detects_a_removed_row` (`SequenceGap`),
  `verifier_detects_a_forged_link` (row with correct own hash but wrong `prev_hash`: `LinkMismatch`),
  `verifier_rejects_a_row_without_chain_fields`, `trusted_start_accepts_a_window_in_the_middle`,
  `anchor_start_rejects_a_first_row_that_does_not_link_to_the_anchor`.
- `Actor` tests listed in step 2.

**Store tests** (`crates/domovoi-store/tests/sqlite/audit_chain.rs`, new, registered in `main.rs`; use `Fresh`):
- `fresh_install_gets_an_empty_anchor` (count 0, last id 0, digest = empty digest, anchor hash = `AuditAnchor::hash`).
- `first_chained_row_links_to_the_anchor` (append; `prev_hash` = anchor hash; `hash` = `entry_hash(1, ..)`).
- `rows_written_through_a_commit_and_through_append_share_one_chain` (alternate `AuditLog::append` and
  `UnitOfWork::commit` with audit drafts; `verify_audit(Full)` is `Intact{checked: n}`, seqs consecutive).
- `audit_failure_rolls_back_the_chain_position` (a commit whose audit draft follows a failing write:
  nothing appended; the next append has the seq and `prev_hash` it would have had).
- `audit_chain_detects_edit` (drop trigger `audit_log_no_update` with raw SQL on `pool_for_tests`, edit
  a chained row's `detail`, recreate the trigger; `verify_audit(Full)` is `Broken{HashMismatch, seq}`).
- `audit_chain_detects_mid_chain_deletion` (drop `audit_log_no_delete`, delete a middle row;
  `SequenceGap`).
- `audit_chain_detects_a_row_inserted_without_the_chain` (raw `INSERT` with NULL hashes: `MissingChainFields`).
- `legacy_audit_digest_reported_separately` (use a database with legacy rows: create it by opening the
  N-1 fixture; edit one legacy row's `detail` with the trigger dropped; `Full` reports
  `legacy = Mismatch{digest_matches: false, ..}` while `chained` stays `Intact`/`Empty` independent of it).
- `last_n_verification_checks_only_the_newest_rows` (append 30 rows; corrupt row 5 of the chain;
  `Last(10)` is `Intact`, `Full` is `Broken` at that row; `legacy` is `CountOnly`).
- `verification_starts_at_the_retained_start_after_a_checkpoint`: append 10 rows; append a row with kind
  `audit.checkpoint` and detail `{"first_retained_seq": S, "first_retained_prev_hash": <hex of row S's
  prev_hash>, "pruned_count": S-1-legacy, "legacy_pruned": false}`; simulate pruning (drop trigger,
  delete rows below S, recreate trigger); `Full` is `Intact`. Variant `checkpoint_with_a_wrong_link_is_broken`
  (wrong hex): `Broken{CheckpointMismatch}`. A third variant: delete the same rows with no checkpoint:
  `Broken{CheckpointMismatch}`.
- `audit_anchor_cannot_be_changed` (UPDATE and DELETE are refused).
- `audit_head_reports_the_newest_chained_row` and `audit_head_is_none_before_the_first_chained_row`.
- `upgrade_writes_an_anchor_over_legacy_audit_rows` (section 8).
- `timestamps_are_hashed_at_millisecond_precision` (append an event whose `at` has nanoseconds; read back
  through `list`; recompute `entry_hash` from the listed event: equals stored `hash`).
- `insert_into_audit_log_exists_in_one_source_file` as a test under `tests/`: read `crates/domovoi-store/src`
  recursively, count files containing the text `INSERT INTO audit_log`, expect exactly one, and that it is
  `audit_chain.rs`. (Source scans of this kind belong under `tests/`, per repository convention.)

**Hub tests** (`crates/domovoi-hub`, near existing admin tests in `src/admin/tests.rs`):
- `startup_verification_records_a_broken_chain_once`: build a store with a broken chain (as above),
  call `verify_audit_on_start` twice; exactly one `audit.chain_broken` row exists, and the hub function
  returns normally.
- `startup_verification_is_silent_on_an_intact_chain`.
- `admin_verify_reports_both_parts`.

**Binary tests**: `cargo test -p domovoi` must still pass; add a small test of the report renderer if
you put it in a function (keep rendering in a pure `fn render_report(&VerifyReport) -> (String, bool)`
so it is unit-testable: lines and "ok" flag). Tests: `render_intact`, `render_broken_sets_failure`,
`render_not_verifiable_does_not_fail`.

Testkit fakes: none new. `InMemoryStore` does not chain; existing in-memory audit behaviour is
unchanged.

## 13. Acceptance criteria

1. `domovoi_core::Actor` exists with the four variants, the string forms in 6.1, and the stated parse rules.
2. The golden vectors in 6.2 are reproduced by unit tests exactly.
3. After opening any database through `Store::open`, `audit_anchor` has exactly one row; on the N-1
   fixture it commits to the 5 legacy rows with a digest that tests recompute independently.
4. Every row written by `AuditLog::append` or by `UnitOfWork::commit` after the anchor has `prev_hash`
   and `hash`, links to the previous row (or the anchor), and verifies.
5. `INSERT INTO audit_log` occurs in exactly one source file of the store.
6. An audit failure inside a commit rolls the whole transaction back and leaves no hole in the chain.
7. `verify_audit(Full)` reports legacy and chained history separately and detects: an edited chained
   row, a deleted middle row, a row inserted without chain fields, a legacy row change (digest), a bad
   checkpoint link.
8. `verify_audit(Last(n))` checks only the newest n chained rows and the legacy count.
9. `domovoi audit verify` and `domovoi audit head` work as in 6.6 and exit non-zero on a broken chain or
   changed legacy history.
10. Hub start verifies the newest 10 000 chained rows, never refuses to start because of a broken
    chain, and records a broken chain once as `audit.chain_broken`.
11. `AuditDraft` and `JournalDraft` carry an `Actor`; `AuditEvent.actor` and `JournalEntry.actor` are still strings.
12. The existing audit contract suite and all existing tests of the touched crates pass unchanged;
    `cargo xtask arch-check` passes; no non-test file exceeds 500 lines.
13. The book and `ARCHITECTURE.md` are updated as in section 15, including the honest limits.

## 14. Migration and compatibility

- Existing installs: first start after upgrade takes the pre-migrate backup, adds the columns and
  table, then computes the anchor over every existing audit row (one pass; a few thousand rows take
  milliseconds, a million rows seconds; state this in the PR). Existing rows are untouched.
- Rolling back to the previous binary works (additive), with the caveat in section 9.
- Agents and protocol: unaffected.
- Config: no new keys; `cargo xtask docs-config` is not needed.
- INV-7 is strengthened, not changed: the append-only triggers remain.

## 15. Documentation requirements

- `ARCHITECTURE.md`: section 9 (Persistence): replace the audit bullet with the chain description
  (anchor, chained rows, `domovoi audit verify`, what it does and does not detect); section 15
  (Security invariants), INV-7 row: "SQLite trigger + hash chain + store tests". Add `AuditChainRead`-style
  wording only if you introduced a port (you did not); do not edit section 6.
- `docs/book/src/reference/data-model.md`: describe `audit_anchor` and the two audit columns, and the
  legacy versus chained distinction.
- `docs/book/src/reference/cli.md`: sections for `domovoi audit verify` and `domovoi audit head`
  (options, output, exit codes); add both to "Commands that need the hub stopped".
- `docs/book/src/security.md` (item 7 about the audit log): one short paragraph saying the log is
  hash-chained from this release, what that detects, and the limits (a full rewrite of the database;
  deletion of the newest rows unless you keep the head hash elsewhere: run `domovoi audit head` and store the output
  off the host).
- `docs/book/src/reference/backup.md`: one sentence that a restored database verifies as it was at backup time.
- Run `cargo xtask docs-links`. No config keys, no OpenAPI change.
- No mention of prompts, the build plan, or coding agents in any committed file.

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
