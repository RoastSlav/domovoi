# 25 · Bug: one writer for action state, and recovery after a restart

Two prompts, in order. 25a needs an ADR. Decided by the maintainer: actions get a single
writer, like incidents (ADR 0011), rather than only a compare-and-set in the store.

## Background (verified on `main`)

- `ActionRequest` (`crates/domovoi-core/src/action/request.rs`) changes status only
  through its methods in `action/transition.rs`: `approve`, `deny`, `expire`, `dispatch`,
  `reject`, `complete`, `verified`.
- Every change is load → mutate → `ActionRepo::update` → audit → publish:
  - `crates/domovoi-engine/src/actions/approvals.rs`: `approve` (`:29`), `deny` (`:61`),
    `expire_due` (`:77`).
  - `crates/domovoi-engine/src/actions/executor.rs`: `run` (`:130`), `block` (`:240`),
    `revalidate` (`:265`), plus `finish` and `verify` further down.
  - All of them call `Shared::record` (`actions/mod.rs:111`), which does
    `actions.update` with no status check.
- `ActionRepo::update` (`crates/domovoi-store/src/repos/actions.rs:170`) is
  `UPDATE actions SET ... WHERE id = ?`. Status is stored as JSON; its name is
  `json_extract(status, '$.type')` (see `list`, `:204-208`).
- The race: approve, deny and the minute expiry sweep (`runtime/schedulers.rs:125` →
  `expire_due`) each load, change and overwrite. A deny or expiry can overwrite
  `Approved` after `ActionApproved` was published, so the skill runs while the record
  says denied or expired, and the executor's later writes overwrite again.
- `expire_due` lists **every** action (`ActionFilter::default()`) and filters in memory.
  Actions are never purged, so this grows forever.
- The executor starts runs only from `ActionApproved` events or after a `Lagged` error
  (`executor.rs:42-73`). Nothing runs `approved()` (`:76`) at start, so an action
  approved just before a restart is never dispatched. A run cancelled by shutdown
  (`:33-38`) leaves the action `Dispatched` forever. A settings save restarts the hub
  runtime, so this happens in normal use. ADR 0014 says approved actions are dispatched
  after a restart; today they are not.
- The incident manager is the pattern to copy: `crates/domovoi-engine/src/manager/actor.rs`
  (an `mpsc` command channel of `COMMAND_QUEUE`, one task, replies over `oneshot`).

---

## 25a · An action writer for approve, deny and expire

### Steps

1. **ADR** `docs/adr/0019-single-writer-for-actions.md` (copy `0000-template.md`):
   every change to a stored action goes through one task, the action writer; nothing
   else calls `ActionRepo::update`. The store also refuses an update whose expected
   status no longer matches (defence in depth). Link ADR 0011 and 0014.
2. **Store guard.** Add to `ActionRepo` (`crates/domovoi-core/src/ports/repos/actions.rs`):
   `async fn update_from(&self, action: &ActionRequest, expected: &str) -> Result<(), StoreError>`
   that appends `AND json_extract(status, '$.type') = ?` to the update and returns
   `StoreError::Conflict("the action changed meanwhile")` when no row matched but the id
   exists (`NotFound` when it does not). Implement it in `domovoi-store` and in the
   testkit fake. Remove `update` from the port once nothing uses it (end of 25b).
3. **Status name.** If `ActionStatus` has no method returning its name for one value,
   add `pub fn name(&self) -> &'static str` next to `NAMES`, using the same strings.
4. **Due expiries in SQL.** Add `expires_before: Option<Timestamp>` to `ActionFilter`
   (`#[derive(Default)]` keeps it `None`) and `AND (?7 IS NULL OR expires_at < ?7)` to
   `list`. `expire_due` lists with `status: Some("proposed")` and
   `expires_before: Some(now + 1 ms)`.
5. **Writer.** New `crates/domovoi-engine/src/actions/writer.rs`:
   - `enum WriteCommand { Approve { id, approval: HumanApproval, chat_token: Option<String>, reply }, Deny { id, approval, reply }, ExpireDue { reply } }`,
     replies over `tokio::sync::oneshot`.
   - `fn spawn(shared: Arc<Shared>) -> (WriterHandle, JoinHandle<()>)`; one task reads a
     bounded `mpsc::channel(64)` until `deps.shutdown`.
   - Move the bodies of `approvals::approve`, `deny` and `expire_due` into the writer,
     replacing `Shared::record`'s `update` with `update_from(action, <status before>)`.
   - `ActionHandle::approve`, `deny` and `expire_due` keep their signatures and send to
     the writer. A closed channel becomes `ApprovalError::Store("the action service is stopping")`.
   - Keep `approvals.rs` for the token compare (`same`) and the pure helpers, or delete
     it if empty. Every file stays under 500 lines (`executor.rs` is at 454).
6. Map `StoreError::Conflict` from the writer to `ApprovalError::Refused(..)` with a
   plain message, so the API answers 409 "This action changed meanwhile; reload."

### Tests (`crates/domovoi-engine/tests/services/approve.rs` or a new `writer.rs`)

- `approve_and_deny_at_once_leave_exactly_one_outcome`: send both without awaiting the
  first; exactly one succeeds, the stored status matches the one that succeeded, and
  only that one was audited.
- `expiry_never_overwrites_an_approval`: approve, then run `expire_due` with the clock
  past `expires_at`; the action stays `Approved`.
- `expire_due_reads_only_due_proposals`: a store with 500 non-proposed actions and one
  due proposal expires one, and the fake store saw a filtered list call.
- Store: `update_from_refuses_a_stale_status` in `crates/domovoi-store/tests/sqlite/`.

### Done when

- Approve, deny and expiry go through one task; the store refuses a stale overwrite.
- Existing approval gate tests (`crates/domovoi-engine/tests/gates.rs`,
  `crates/domovoi-hub/tests/gates.rs`) pass unchanged.

---

## 25b · The executor writes through the writer, and actions survive a restart

### Steps

1. Add writer commands for every executor transition:
   `Dispatch { id, plan }`, `Reject { id, reason, blocked: bool }`,
   `Complete { id, result: Result<(), String>, record: Option<RunRecord> }`,
   `Verified { id, passed }`. Each loads the action, applies the transition, writes
   with `update_from`, audits with the existing helper in `actions/audit.rs`, and
   publishes the same event the executor publishes today. Each replies with the updated
   `ActionRequest` or an error.
2. Change `executor.rs` so it never calls `Shared::record`; it sends commands and uses the
   returned action. Delete `Shared::record` and `ActionRepo::update` (and their fake and
   store implementations) once unused. The `running` claim and the loop guard stay in the
   executor.
3. **Recovery at start** (in `writer::spawn`, before reading commands, or a `recover()`
   the executor awaits first):
   - Every `Dispatched` action becomes `Failed` with reason
     `"the hub restarted while this ran; check the host before running it again"`
     (its result can no longer arrive: the agent's reply went to the old session). If
     `complete` refuses from `Dispatched`, add the needed transition in
     `action/transition.rs` and say why in its doc comment.
   - Every `Approved` action is started by the executor (call `approved()` once at
     start, then `start()` for each).
4. Book: `docs/book/src/concepts/actions.md`: one paragraph on what happens to approved
   and running actions when the hub restarts.

### Tests

- `an_action_approved_before_a_restart_runs_after_it`: store an `Approved` action, start
  the service, assert a dispatch reached the fake link.
- `a_dispatched_action_fails_after_a_restart`: store a `Dispatched` action, start the
  service, assert `Failed` with the restart reason, audited once.
- Existing executor tests (`tests/services/execute.rs`, `actions.rs`) pass.

### Done when

- `grep -rn "actions.update\|\.update(&" crates/domovoi-engine/src` finds nothing outside
  `writer.rs`.
- No action is left `Approved` or `Dispatched` after a restart without being run or
  failed.
