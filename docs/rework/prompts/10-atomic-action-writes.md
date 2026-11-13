# 10 — Atomic action writes: status, audit and journal in one commit

| | |
|---|---|
| Branch | `atomic-action-writes` |
| Milestone | F1 |
| Depends on (must be merged) | 09 (which depends on 08) |
| Can run in parallel with | 11 (they touch different engine modules, but both add one field to hub wiring and test harnesses: expect trivial rebase conflicts in `crates/domovoi-hub/src/runtime/mod.rs`) |
| Produces (shared contracts) | `ActionHandle::propose`, `ActionHandle::rotate_chat_token`; the journal payload shape for `action.transition` (section 6) |
| Consumes | prompt 08: `UnitOfWork`, `Transaction`, `StateWrite::Action`, `ActionGuard`, `JournalDraft`, `JournalKind::ActionTransition`, `AuditDraft`, `CommitError`; prompt 09: `Actor`, chained audit rows |
| Launch gate | none |
| Suggested model | medium: many call sites, one pattern; the safety behaviour is specified exactly |

## 1. Objective

Every change to an action's stored state becomes one `UnitOfWork` commit holding the guarded state
write, its audit entry and its journal entry. If the audit entry cannot be written, the transition
does not happen. Three writes that today bypass the action writer (ADR 0019) go through it: the
creation of a proposed action by triage, and the rotation of a waiting action's chat token.

After this change, a crash or a store error can never leave an action in a new status without its
audit row, nor an audit row for a status change that did not happen.

## 2. Architectural context

- **Problem (RW-03, defect D06).** `Shared::record_from` in the action writer calls
  `ActionRepo::update_from` and *then* appends the audit entry; an audit failure is only logged
  (`crates/domovoi-engine/src/actions/mod.rs:127-153`). Triage inserts proposed actions and rotates
  chat tokens directly through `ActionRepo`, outside the writer (`triage/proposals.rs:115,138`).
- **Decision.** The action writer builds one `Transaction` per transition:
  `StateWrite::Action{ action, expected: ActionGuard::Status(<status read from>) }`, one
  `AuditDraft` and one `JournalDraft` (kind `ActionTransition`). The store applies them atomically
  (prompt 08). The guard is the existing optimistic rule of ADR 0019: a decision that read the action
  before it changed is refused (`CommitError::Conflict`), mapped to the same refusal the callers get today
  (`ApprovalError::Refused(ActionError::ChangedMeanwhile)`, a 409 on the API; "someone already decided
  this action" in chat).
- **Single writer stays.** The writer task serializes decisions. The unit of work does not decide who
  writes; this prompt only moves the three remaining direct writers behind the writer's command queue.
- **Approvals.** Nothing about who may approve changes. `HumanApproval` still enters only through
  `ActionHandle::approve`/`deny`, built by the hub's auth layer (INV-1, ADR 0007). The new commands
  (`Propose`, `RotateChatToken`) carry no approval and cannot move an action out of `proposed`.
- **Audit actor (D-13, RW-29).** Audit and journal entries now carry an `Actor`. A chat approval's actor
  becomes `user:<UserId>` (the approving user), with the channel already in the audit detail
  (`"channel": approval.channel()`). Before this change it was `chat:<platform>:<user>`. Rows already
  written keep their old form.
- **Events and metrics after commit only.** `ActionChanged` events and `telemetry::record_action` are
  published only after the commit succeeded, as now.
- **Out of scope here:** the effect outbox (`Dispatch` effects, prompt 15), approval card changes,
  anything about `Wake`/verification timers.

## 3. Repository context

Verify each item in step 1. Paths relative to `C:\Programming\domovoi`.

- `crates/domovoi-engine/src/actions/mod.rs`:
  - `ActionDeps` (line ~61): `actions: Arc<dyn ActionRepo>`, `incidents`, `audit: Arc<dyn AuditLog>`,
    `link`, `redactor`, `clock`, `manager`, `bus`, `shutdown`. Constructed in 7 places (below).
  - `Shared::record_from(&self, action, expected: &str, entry: AuditEvent) -> Result<(), StoreError>`
    (line ~127) and `Shared::announce` (line ~140). Excerpt:

    ```rust
    self.deps.actions.update_from(action, expected).await?;
    self.announce(action, entry).await;   // audit.append failure is only logged
    ```
  - `ActionHandle::{approve, deny, expire_due}`; `WriteCommand` queue in `writer.rs`.
- `crates/domovoi-engine/src/actions/writer.rs` (435 lines): `WriteCommand` variants `Approve, Deny,
  ExpireDue, Dispatch, Reject, Complete, Verified`; one async fn per command each ending in
  `shared.record_from(&action, from.name(), audit::...)`; `stale(err: StoreError, id)` maps
  `Conflict`→`ApprovalError::Refused(ActionError::ChangedMeanwhile)`, `NotFound`→`NotFound(id)`,
  `Backend`→`Store`. `expire_due` (line ~367) also calls `record_from` in a loop and treats
  `StoreError::Backend` as fatal, other errors as "decided meanwhile".
- `crates/domovoi-engine/src/actions/recover.rs:225`: `fail_abandoned` calls `shared.record_from`
  with `audit::finished(...)` and matches `StoreError::Backend` versus other errors. `dispatch_history`
  (line ~79) reads the audit log with `shared.deps.audit.list` to rebuild the loop guard.
- `crates/domovoi-engine/src/actions/audit.rs` (190 lines): builders returning `AuditEvent`:
  `proposed, approved, denied, expired, rejected, blocked, dispatched, finished, verification`; `pub
  fn actor(&HumanApproval) -> String` (returns `user:<id>` for web, `chat:<platform>:<user>` for chat);
  `SYSTEM_ACTOR = "system:actions"`; kind constants (`PROPOSED`, `APPROVED`, ...).
- `crates/domovoi-engine/src/actions/approvals.rs`: `load(shared, id)` (reads via `ActionRepo::get`),
  `store(err) -> ApprovalError::Store`, `chat_token_ok`.
- `crates/domovoi-engine/src/triage/proposals.rs`: `store(shared, incident, skills, proposals)`; the
  rotation at line ~115 (`shared.deps.actions.update_from(&rotated, "proposed")`) and the insert plus
  audit at lines ~138-147:

  ```rust
  match shared.deps.actions.insert(&action).await {
      Ok(()) => {
          crate::telemetry::record_action(action.status());
          let entry = crate::actions::audit::proposed(&action, now);
          if let Err(err) = shared.deps.audit.append(&entry).await { /* logged only */ }
  ```
  `mint_token()`, `token_hash(&str) -> [u8; 32]`, `PROPOSER = "triage"`.
- `crates/domovoi-engine/src/triage/mod.rs:118`: `TriageDeps` (has `actions: Arc<dyn ActionRepo>`,
  `audit: Arc<dyn AuditLog>`, `manager: ManagerHandle`, ...). Constructed at
  `crates/domovoi-hub/src/runtime/mod.rs:462` (the action service is already spawned at line ~230, so
  its `ActionHandle` exists), `crates/domovoi-hub/tests/hub/api_harness.rs:308`,
  `crates/domovoi-engine/tests/services/harness.rs:167`, `crates/domovoi-engine/tests/drills/world.rs:260`.
- `ActionDeps {` constructed at: `crates/domovoi-hub/src/runtime/mod.rs:231`,
  `crates/domovoi-hub/examples/dev_hub.rs:449`, `crates/domovoi-hub/tests/hub/api_harness.rs:291`,
  `crates/domovoi-engine/tests/services/actions_harness.rs:112`,
  `crates/domovoi-engine/tests/services/recovery.rs:132`, `crates/domovoi-engine/tests/services/writer.rs:112`.
- Existing tests to keep green: `crates/domovoi-engine/tests/services/{approve,execute,writer,recovery,actions}.rs`;
  `execute.rs:349` asserts `entry.actor == "system:actions"`; `execute.rs:367` asserts the approval
  audit actor `starts_with("user:")`. Gate tests: `crates/domovoi-engine/tests/gates.rs`.
- (08, 09) `UnitOfWork`, `Transaction` (builder helpers `write/journal/audit`), `StateWrite::Action`,
  `ActionGuard::{New, Status}`, `JournalDraft { aggregate, kind, payload_v, payload, actor: Actor,
  occurred_at }`, `AuditDraft::new(actor, at, kind, subject, detail)`, `CommitError`. Verify exact names.
- (09) `Actor::{User, System, Agent, Policy}`.
- Testkit: `InMemoryStore` implements `UnitOfWork` and `fail_next`; `FakeClock`; `FakeAgentLink`;
  `testkit::approvals::{human, chat, web_typed}`; `testkit::build::{action, open_incident, host, T0, at}`.
- `ARCHITECTURE.md` section 12 line "Every transition writes an `audit_log` entry".

## 4. Exact scope

1. `ActionDeps` gains `uow: Arc<dyn UnitOfWork>`. `ActionDeps.actions` stays (reads: `get`, `list`);
   `ActionDeps.audit` stays (reads for the loop guard: `list`). Neither is used for writes any more.
2. `audit.rs`: every builder now returns an `AuditDraft` (via `AuditDraft::new(actor, at, kind,
   subject, detail)`); `pub fn actor(&HumanApproval) -> Actor` returns `Actor::User(approval.user())`
   for both channels. `proposed()` maps `Principal::Human(user)` to `Actor::User(user)` and
   `Principal::System(c)` to `Actor::System(c.to_string())`. The constants and detail contents stay as
   they are. Keep the file at most 500 lines.
3. `mod.rs`: replace `record_from` and `announce` with one commit function (section 6.2) that builds
   the `Transaction`, calls `deps.uow.commit`, and only on success records telemetry and publishes.
4. `writer.rs`: use the new function; adapt `stale()` and the `expire_due` loop to `CommitError`; add
   the commands `Propose` and `RotateChatToken` and the matching `ActionHandle` methods. If
   `writer.rs` would exceed 500 lines, move the two new commands into `writer_propose.rs`.
5. `recover.rs`: `fail_abandoned` uses the same function and handles `CommitError`.
6. `triage/proposals.rs`: no longer writes through `ActionRepo`. It calls `ActionHandle::propose` and
   `ActionHandle::rotate_chat_token`. `TriageDeps` gains `actions_service: ActionHandle` (keep
   `actions: Arc<dyn ActionRepo>` for the reads); remove `audit` from `TriageDeps` only if nothing
   else in triage uses it (grep first; otherwise keep it).
7. Wiring: add `uow: store.clone()` to every `ActionDeps` site and `actions_service: ...clone()` to
   every `TriageDeps` site listed in section 3. In the hub runtime and `dev_hub`, pass the existing
   action handle.
8. Tests (section 12) and docs (section 15).

## 5. Explicit non-goals

- No change to the incident manager, `ManagerDeps`, `manager/` or incident timeline writes (prompt 11).
- No new journal kinds, no new effect kinds, no outbox use. No `Dispatch` effect.
- No change to approval rules, chat token format, token hashing, plan hash, the state machine in
  `domovoi-core`, or the HTTP API shapes. No OpenAPI regeneration.
- No change to hub-level audit appends that are not action transitions (`chat.*`, rules, settings): they
  keep using `AuditLog::append`.
- Do not touch the web UI.
- Do not weaken `crates/domovoi-engine/tests/gates.rs`; do not edit prompts 08/09's contracts.
- Do not add a `version` column to `actions`; the status guard remains.
- Do not "fix" bugs you find outside this scope; note them in the completion report.

## 6. Required interfaces and contracts

### 6.1 Journal entry for an action write (**fixed**)

Every action commit adds exactly one `JournalDraft`:

```rust
JournalDraft {
    aggregate: AggregateRef::action(action.id()),
    kind: JournalKind::ActionTransition,
    payload_v: 1,
    payload: json!({
        "action":    "<action id>",
        "incident":  "<incident id>",
        "host":      "<host id>",
        "skill":     "<skill name>",
        "from":      "<previous status name>" or null,   // null only for creation
        "to":        "<new status name>",                 // ActionStatus::name()
        "plan_hash": "<hex>",
        // present only for a chat token rotation:
        "change":    "chat_token_rotated"
    }),
    actor,            // the same Actor as the audit draft of this commit
    occurred_at,      // the writer's clock reading `now` used for the transition
}
```
The payload never contains the chat token, its hash, arguments, or output.

### 6.2 The commit function (**fixed behaviour**, **convention** for names)

```rust
// crates/domovoi-engine/src/actions/mod.rs
impl Shared {
    /// Commits one action write together with its audit and journal entries, then records the
    /// metric and publishes the event. Nothing is recorded or published if the commit fails.
    pub(crate) async fn commit_action(
        &self,
        action: &ActionRequest,
        guard: ActionGuard,          // Status(expected) for a transition, New for creation
        from: Option<&str>,          // previous status name for the journal, None on creation
        audit: AuditDraft,
        now: Timestamp,
        extra: Option<(&str, &str)>, // ("change", "chat_token_rotated") for rotation, else None
        publish: Publish,            // what to announce after the commit
    ) -> Result<(), CommitError>;
}
pub(crate) enum Publish { ActionChanged, Nothing }
```
Behaviour: build `Transaction::new().write(StateWrite::Action{ action: action.clone(), expected: guard })
.audit(audit).journal(journal_draft)`; `self.deps.uow.commit(tx).await?`; then
`telemetry::record_action(action.status())`; if `Publish::ActionChanged`, publish `EventKind::ActionChanged`
through the existing `publish`. `Publish::Nothing` is used for `Propose` (triage's card announces it)
and for `RotateChatToken`.

The audit draft's `actor` and the journal draft's `actor` are the same value.

### 6.3 New handle methods (**fixed signatures**)

```rust
impl ActionHandle {
    /// Stores a newly proposed action with its audit and journal entries. Refused when the
    /// action is not in the `proposed` status.
    pub async fn propose(&self, action: ActionRequest) -> Result<ActionRequest, ApprovalError>;

    /// Replaces the chat approval token hash of a still-proposed action. The previous token stops
    /// working. Refused (`Refused(ChangedMeanwhile)`) when the action is no longer `proposed`,
    /// `NotFound` when it does not exist.
    pub async fn rotate_chat_token(
        &self,
        id: ActionId,
        token_hash: [u8; 32],
    ) -> Result<ActionRequest, ApprovalError>;
}
```
Writer commands (**convention**): `WriteCommand::Propose { action, reply }`,
`WriteCommand::RotateChatToken { id, token_hash, reply }`.

`Propose` in the writer: require `action.status() == &ActionStatus::Proposed` else
`ApprovalError::Refused(ActionError::NotAllowed { status: action.status().clone(), operation: "propose" })`
(`ActionError` is in `crates/domovoi-core/src/action/transition.rs:20`; `NotAllowed.operation` is a
`&'static str`); commit with `ActionGuard::New`, `from: None`, audit
`audit::proposed(&action, now)`. Id already exists: `CommitError::Conflict{found: Status(..)}` maps to
`Refused(ChangedMeanwhile)`; a chat token hash already used (`Constraint`) maps to `ApprovalError::Store`.

`RotateChatToken` in the writer: `load`; require `Proposed` else `Refused(ChangedMeanwhile)`; clone and
`with_chat_token_hash(token_hash)`; commit with `ActionGuard::Status("proposed")`, `from: Some("proposed")`,
extra `("change","chat_token_rotated")`, audit `AuditDraft` of kind `action.token_rotated` (new constant
`TOKEN_ROTATED` in `audit.rs`), actor `Actor::System("actions")`, subject `action:<id>`, detail
`{ "incident": ..., "host": ..., "skill": ... }` (no token, no hash). `updated_at` is not changed by a
rotation (the domain type's `with_chat_token_hash` does not touch it; keep it that way).

### 6.4 Error mapping (**fixed**)

```rust
fn stale(err: CommitError, id: ActionId) -> ApprovalError {
    match err {
        CommitError::Conflict { found: Observed::Missing, .. } => ApprovalError::NotFound(id),
        CommitError::Conflict { .. } => ApprovalError::Refused(ActionError::ChangedMeanwhile),
        CommitError::Constraint(m) | CommitError::Backend(m) => ApprovalError::Store(m),
        CommitError::StaleEffect { .. } => ApprovalError::Store("unexpected stale effect".into()),
    }
}
```
`expire_due`: a `Conflict` (any `found`) skips that action with the existing `warn`; `Backend` or
`Constraint` aborts the sweep with `ApprovalError::Store`. `fail_abandoned`: same split as today
(`Backend`/`Constraint` → warn and continue; `Conflict` → debug and continue).

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies.** Open every file in section 3.
   Confirm prompts 08 and 09 are merged and note the real names of `UnitOfWork`, `Transaction`,
   `StateWrite::Action`, `ActionGuard`, `JournalKind::ActionTransition`, `AuditDraft::new`, `Actor`.
   Run `rg "record_from|announce\(" crates/domovoi-engine/src` and `rg "deps\.actions\.(insert|update_from)"`
   and write down every call site; the lists in this brief must match.
2. Write the new tests first where they can fail on the old code (section 12, tests marked **red first**), then
   change the code until they pass.
3. `audit.rs`: builders return `AuditDraft`; `actor()` returns `Actor`; add `TOKEN_ROTATED`.
4. `mod.rs`: `ActionDeps.uow`, `commit_action`, remove `record_from`/`announce`.
5. `writer.rs` (and `writer_propose.rs` if needed): switch every command; add `Propose` and
   `RotateChatToken`; update `stale`; update `expire_due`.
6. `recover.rs`: `fail_abandoned`.
7. `ActionHandle::propose` and `rotate_chat_token`.
8. Triage: `TriageDeps.actions_service`; `proposals.rs` uses the handle; remove the direct inserts.
9. Wire every constructor (7 `ActionDeps`, 4 `TriageDeps`). `cargo build --workspace --all-targets`
   compiles before you run tests.
10. Update the existing assertions that depend on actor strings only where they now must (see section 14).
11. Docs (section 15).
12. **Run the checks and write the completion report.** `cargo fmt --all`;
    `cargo clippy -p domovoi-engine -p domovoi-hub --all-targets -- -D warnings`;
    `cargo nextest run -p domovoi-engine -p domovoi-hub`; `cargo nextest run -E 'rdeps(domovoi-engine)'`;
    `cargo xtask arch-check`. In the report, include `rg "update_from|\.insert\(&action|actions\.insert" crates/domovoi-engine/src`
    showing that only the store adapters and the writer's commit path remain.

## 8. Database and persistence

No migration and no schema change: `journal` and `audit_log` exist (prompts 08 and 09). Transaction
boundaries: one `UnitOfWork::commit` per action transition (state write + 1 audit row + 1 journal row);
one per proposal; one per token rotation. The action writer task remains the only caller that commits
action writes (the gates in section 12 check this by source scan).

N-1 upgrade test: not applicable to this prompt (no schema change); the upgrade fixture test from
prompt 08 still runs and must stay green.

## 9. Protocol and compatibility

No wire change (protocol 1.1) and no HTTP API change. Audit `actor` strings for chat approvals change
from `chat:<platform>:<user>` to `user:<UserId>` for new rows (the channel is in the detail). Existing
rows are untouched. The web audit view lists actors as plain strings; nothing parses them.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Commit fails with `Backend`/`Constraint` (audit refused, disk full) | the action keeps its previous status; no audit row; no journal row; no event; no metric; caller gets `ApprovalError::Store(message)`; the error is logged at `warn` once |
| Commit fails with `Conflict{found: Status}` | `ApprovalError::Refused(ChangedMeanwhile)` (API 409, chat "someone already decided"); nothing written |
| Commit fails with `Conflict{found: Missing}` | `ApprovalError::NotFound(id)` |
| Hub restarts between steps | an action is either in its old status or its new one, with matching audit and journal rows; `fail_abandoned` and the loop-guard rebuild see consistent rows |
| `propose` with a non-`proposed` action | `ApprovalError::Refused` (the domain's wrong-status error); nothing written |
| `propose` with an existing id | `Refused(ChangedMeanwhile)` |
| `rotate_chat_token` on a decided action | `Refused(ChangedMeanwhile)`; the old hash stays |
| `rotate_chat_token` for an unknown id | `NotFound(id)` |
| Triage cannot propose or rotate | existing behaviour: the proposal is dropped from the report with `"<skill>: could not be stored"` and a warning is logged; rotation failure keeps the old token and offers nothing for it |
| Writer queue closed (shutdown) | `ApprovalError::Unavailable` as today |
| Expiry sweep: one action's commit conflicts | that action is skipped; the sweep continues; count excludes it |
| Expiry sweep: store failure | sweep returns `ApprovalError::Store`; actions already expired in this sweep stay expired (each was its own atomic commit) |

## 11. Security requirements

- **INV-1 and the approval path are untouched.** No new constructor of `HumanApproval`; `propose` and
  `rotate_chat_token` take no approval and cannot produce an `Approved` action. A test asserts a
  `propose` of an `approved` action is refused.
- **Audit completeness.** The audit entry is part of the transition: an audit failure fails the
  transition (tests: `audit_failure_rolls_back_transition`).
- **No secrets in journal or audit.** Tokens and token hashes never appear in audit or journal
  payloads (test: `token_rotation_records_no_token_material`).
- **Chat tokens remain single-use hashes**; clearing the token on approve/deny/expire still happens in
  the same transaction (the cleared hash is part of the written `ActionRequest`).
- **Fail closed.** Any commit error leaves the previous state; the executor does not run a skill unless
  the writer answered `Dispatched` from a successful commit (existing behaviour, re-verified by the
  existing execute tests).
- **Single writer.** After this change only the writer task (and `fail_abandoned`, which runs inside
  the writer's startup) commits action writes. The gate test in section 12 enforces it by scan.

## 12. Testing requirements

Engine integration tests live in `crates/domovoi-engine/tests/services/` (module list in `main.rs`;
harness in `actions_harness.rs`). Fakes: `InMemoryStore` (implements `UnitOfWork`), `FakeClock`,
`FakeAgentLink`. Add two small test doubles in a new `tests/services/uow_probe.rs` (**new**, test-only,
not in testkit):

- `RecordingUow`: wraps `Arc<InMemoryStore>`, records every `Transaction` (cloned) and forwards.
- `RefusingAuditUow`: wraps `Arc<InMemoryStore>`; `commit` returns
  `Err(CommitError::Constraint("audit refused".into()))` **without applying anything** when the
  transaction contains an `AuditDraft` (so a test can assert the writer reports the error and the
  stored action is unchanged). Real atomicity of the store is proved by prompt 08's contract tests
  and the SQLite tests of prompt 09; do not duplicate it here.

The harness builds `ActionDeps` with `uow: store.clone()`; add a variant `actions_with_uow(settings,
uow)` for tests that need a probe (the harness keeps the same `InMemoryStore` for reads).

Tests (name: behaviour; **red first** = write it before the change and see it fail on the old code):

1. `audit_failure_rolls_back_transition` **(red first)**: approve with `RefusingAuditUow`. Expect
   `Err(ApprovalError::Store(_))`; `store.get(id)` is still `Proposed` with the same chat token hash;
   no audit rows; no `ActionApproved` event published.
1b. `audit_failure_rolls_back_transition_on_sqlite` (in the **hub** crate, which depends on `domovoi-store`:
   new file `crates/domovoi-hub/tests/hub/atomic_actions.rs`, registered in `tests/hub/main.rs`): open a real
   `Store` in a temp directory, build the action service over it the way `tests/hub/api_harness.rs` does
   (reuse its helpers if it exposes the store and the `ActionHandle`; otherwise copy the minimal
   construction), insert a proposed action, install through `store.pool_for_tests()`
   `CREATE TRIGGER fail_audit BEFORE INSERT ON audit_log BEGIN SELECT RAISE(ABORT, 'refused'); END;`,
   approve it. Expect `Err(ApprovalError::Store(_))`; with raw SQL the action row still has status
   `{"type":"proposed"}` and its chat token hash, `audit_log` has 0 rows and `journal` has 0 rows. Drop the
   trigger and approve again: it succeeds with one audit row and one journal row. This is the real
   atomicity proof; test 1 only proves the writer reports and publishes correctly.
2. `every_action_transition_commits_state_audit_and_journal_together`: with `RecordingUow`, drive an
   action through propose, approve, dispatch (accepted), complete (success), verified(true); expect
   five transactions, each with exactly one `StateWrite::Action`, one `AuditDraft`, one `JournalDraft`
   of kind `ActionTransition`, whose payload `from`/`to` match the statuses
   (`null→proposed, proposed→approved, approved→dispatched, dispatched→succeeded, succeeded→verified`),
   and whose audit and journal actors are equal.
3. `denied_and_expired_and_rejected_and_blocked_and_failed_transitions_are_atomic`: same shape check for
   deny, expiry sweep, plan-mismatch reject, loop-guard `blocked` reject, `complete` with an error,
   `verified(false)`, and `fail_abandoned` at start (seed a `Dispatched` action, start the service,
   expect a transaction with `dispatched→failed`).
4. `stale_decision_is_refused_and_writes_nothing`: read an action, approve it through the handle, then
   send a second approve for the same read state via the writer (use two concurrent approves); the loser
   gets `Refused(ChangedMeanwhile)`; the journal and audit have rows for the winner only
   (assert counts through `InMemoryStore` as `JournalRead` and `AuditLog`).
5. `chat_approval_audit_actor_is_the_user_with_the_channel_in_the_detail`: approve with
   `testkit::approvals::chat(user, "discord", "42")` and the action's token; the audit row's actor is
   `user:<UserId>` and `detail.channel.platform_user == "42"`.
6. `proposed_action_is_stored_with_its_audit_and_journal_rows`: `ActionHandle::propose` with a built
   proposal stores it, writes audit kind `action.proposed` and a journal row `null→proposed`; returns the
   action.
7. `propose_rejects_an_action_that_is_not_proposed`: propose an approved action; refused; nothing stored.
8. `token_rotation_goes_through_the_writer` and `token_rotation_records_no_token_material`: rotate on a
   proposed action; stored hash replaced; the old token no longer approves (`BadToken`); journal row has
   `change = "chat_token_rotated"`; serialize the audit row and journal payload to text and assert the hex
   of neither the old nor the new hash nor the plain token occurs.
9. `rotation_of_a_decided_action_is_refused`.
10. `triage_proposals_use_the_writer` (in `tests/services/report.rs` or `turns.rs` style, using the triage
    harness): run a triage that proposes two actions (one duplicate of a waiting action to force a
    rotation); with `RecordingUow` behind both services, assert the transactions are one `Propose` shape
    and one rotation shape and that `ActionRepo` was not written directly (the probe wraps the repo path too:
    simplest check is that `RecordingUow` saw both transactions).
11. `loop_guard_reloads_dispatches_that_were_committed_with_their_audit_rows`: in `recovery.rs` style,
    after two committed dispatches, restart the service on the same store; the guard counts both
    (existing test for the guard; extend or add one case).
12. Gate (**source scan under `tests/`**, in `crates/domovoi-engine/tests/gates.rs` as a new test; do not
    modify existing gates): `action_writes_go_through_the_unit_of_work`: scan `crates/domovoi-engine/src`
    for `.update_from(` and `actions.insert(`; expect zero matches outside comments.
13. Existing suites stay green: approve, execute, writer, recovery, actions, drills, hub API and
    interaction tests.

Hub: `cargo nextest run -p domovoi-hub` (the harnesses were updated to pass `uow` and
`actions_service`; no new hub test is required beyond compile and the existing suites).

## 13. Acceptance criteria

1. Every action transition (approve, deny, expire, dispatch, reject, blocked, complete, verified,
   abandoned) is one `UnitOfWork` commit with one `Action` write, one audit draft and one journal draft;
   test 2 and 3 prove it.
2. An audit (or any) commit failure leaves the action unchanged and publishes nothing; test 1 fails on the
   code before this change and passes after.
3. Triage proposals and chat-token rotations are committed by the action writer via
   `ActionHandle::propose` and `rotate_chat_token`; no code in `domovoi-engine/src` calls
   `ActionRepo::insert` or `update_from` (gate test 12).
4. A stale decision is refused with the same refusal as before and writes nothing.
5. The journal payload of every action commit has the shape in 6.1 and never contains token material.
6. Chat approvals' audit actor is `user:<UserId>` with the channel in the detail.
7. The loop guard still rebuilds from committed dispatch audit rows.
8. All pre-existing tests in `domovoi-engine`, `domovoi-hub` pass (except assertions you had to update for
   the actor change, each listed in the report).
9. `cargo xtask arch-check` passes; no non-test file over 500 lines; no new `#[allow]`; no `unwrap` in
   library code.
10. `ARCHITECTURE.md` section 12 and ADR note updated as in section 15.

## 14. Migration and compatibility

- Existing installs: no data migration. New audit and journal rows follow the new form from the first
  transition after upgrade.
- The only observable audit change: new chat approvals appear with actor `user:<id>`. Update any test
  that asserted the old string for an `action.*` row (search `rg "chat:" crates/*/tests` and update only
  assertions about `action.approved`/`action.denied` rows; the hub's `chat.*` interaction rows keep
  their `chat:<platform>:<user>` actor because that writer is out of scope).
- Triage's observable behaviour is unchanged (same drops, same tokens, same report text).
- Rollback: the previous binary reads the same tables; journal rows it did not write are ignored.

## 15. Documentation requirements

- `ARCHITECTURE.md` section 12: replace the audit bullet with: every transition is one atomic commit of
  the action row, its `audit_log` entry and its journal entry; an audit failure fails the transition.
  Section 7.1 (hub runtime), only if it names action persistence: mention the unit of work.
- `docs/adr/0019-single-writer-for-actions.md`: append a short "Update" paragraph (do not rewrite the
  decision): all action creates and updates, including triage proposals and chat token rotation, go
  through the writer and commit with their audit and journal entries atomically.
- `docs/book/src/reference/data-model.md`: one sentence under `audit_log` that action transitions are
  recorded in the same transaction as the change.
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
