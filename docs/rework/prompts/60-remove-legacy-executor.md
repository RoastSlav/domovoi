# 60 — Remove the legacy action executor and retire legacy actions

| | |
|---|---|
| Branch | `remove-legacy-executor` |
| Milestone | M3 (tail) |
| Depends on (must be merged) | 58 (`remove-legacy-triage-path`: nothing creates a legacy action any more, the triage queue is gone, `[orchestration] enabled` defaults to true) and, through it, 59 (chat approval of steps), 27, 28 and 26. Prompts 26, 27, 28, 58 and 59 are written but may not be merged when this brief is read: **every name taken from them is "introduced by prompt NN — verify in code before use"** |
| Can run in parallel with | none (it deletes the module that 26, 27, 58 and 59 sit beside) |
| Produces (shared contracts) | the single dispatch boundary `operations/execute.rs` as the **only** execution site; the one-shot legacy retirement `drain_legacy_actions`; `ActionRequest::expire_at_upgrade`; the audit kinds and reasons of section 6.3; an ADR |
| Consumes | the operation execution path, verification and reconcile handlers of 26; the step approval routes of 27 and 59; the arch cards of 26 |
| Launch gate | **the actions parity checkpoint (05 §7): every ADR 0014 and ADR 0019 test and every action gate that protects dispatch safety has a named, green equivalent on the operation execution path, proved in step 3 before anything is deleted and recorded in the PR.** If a legacy behaviour has no equivalent you can add within this prompt's scope, stop and report; do not delete its test |
| Suggested model | large: it deletes the first-generation execution path and rewrites the architecture cards that pin it |

## 1. Objective

After this change there is **one** way a skill reaches an agent: `crates/domovoi-engine/src/operations/execute.rs` (`AuthorizedStep`, `execute_step`). Specifically:

- `crates/domovoi-engine/src/actions/` (writer, executor, execute, dispatch, recovery, expiry, reconcile, verification, `ActionService`, `ActionHandle`, `ApprovalError`) is **deleted**; what operations still use from it is **moved** first, not rewritten;
- the legacy verification policy (`recheck.rs`: `verify_delay`, `verify_after`, `RECHECK_DEADLINE`; and `verify_wake.rs`) is retired, with the policy it encoded proven on the operation path;
- the architecture cards (`EXEC-2`) name one dispatch site, with a new ADR;
- at the first start of the new release the hub **retires every legacy action that is still open**, once and audited (RW-32): `Proposed` becomes `Expired`, `Approved` becomes `NotRun`, a still-`Dispatched` run becomes `OutcomeUnknown` (never `Failed`, never re-run), pending verification is dropped; every row stays readable;
- the legacy approve and deny endpoints, the legacy chat button branch and the legacy action service wiring are gone; `GET /api/v1/actions` and `GET /api/v1/actions/{id}` remain as the read-only **Legacy** projection.

It does **not** convert, delete or hide legacy rows, change the `actions` table, add a migration, touch the wire, or remove anything that prompt 59's step approval relies on (section 5.1 lists it).

## 2. Architectural context

**RW-08 (one execution boundary).** One function decides to dispatch, taking a value that exists only for one run. Prompt 26 built that for steps (`AuthorizedStep::of`, `execute_step`, the writer command `OperationCommand::Dispatch`) and kept the first-generation one for actions (`AuthorizedExecution::of`, `execute::execute`, `WriteCommand::Dispatch`) **by design for now**, pinned by two sets of architecture cards. This prompt removes the second site; the cards collapse to the first.

**RW-32 / 05 §2 (legacy actions).** Rows are never converted. The release that removes the legacy executor expires remaining `Proposed` and `Approved` rows with the audited reason **"legacy action expired at upgrade"**, a real event and not invented history. The rows stay in `actions` and show through the read-only projection labelled **Legacy** (prompt 27).

**RW-36 / RW-09 (in-flight runs).** A run that was dispatched finishes under its `ActionId` or is reconciled against the agent's journal; a hub restart is no evidence either way, so the honest unresolved state is `OutcomeUnknown` ("the run may have happened and its result cannot be established"). The legacy reconcile code keeps a bounded number of passes and then **leaves a row `Dispatched` forever** (`actions/reconcile/mod.rs:40-47`, "never failed"). So a `Dispatched` legacy row can exist at any later release, however long you wait, and the removal release must settle it itself. Decision (do not revisit): **the first start of this release settles every remaining `Dispatched` legacy row as `OutcomeUnknown` with an audited reason and never refuses to start.** A refusal to start was rejected: a host that never comes back would block the upgrade for good, and the settled state is the same one the legacy path already assigns to a pre-upgrade row it cannot reconcile (`pre_upgrade_dispatched_row_reconciles_as_outcome_unknown`). The agent's journal still holds the real record, keyed by the same id.

**What 05 §7 requires before removal.** "all ADR 0014/0019 tests and action gates pass on the new path". ADR 0014 (re-hydrating stored approvals: a restart loses no decision and invents no run) and ADR 0019 (one writer applies every decision, stale writes are refused, expiry never overwrites a decision) describe behaviours; this prompt proves each on the operation path first (step 3), and only then deletes the legacy tests that carried them.

**Chat tokens (RW-11, prompt 59).** The single-use chat token of the legacy path is the same mechanism steps now use. This prompt removes only what is specific to legacy **actions** and keeps every piece prompt 59 relies on (section 5.1).

## 3. Repository context

Verify every item first; 26, 27, 58 and 59 will have moved lines. Marks: **(existing, path:line)** at the time of writing, **(introduced by prompt NN)**.

### 3.1 What is deleted (the whole module)

`crates/domovoi-engine/src/actions/` **(existing, 5 397 lines in 25 files)**: `mod.rs` (`ActionService::spawn`, `ActionHandle`, `ApprovalError`, `Shared`, `Publish`), `writer.rs` (`WriteCommand`, the one writer, `approve`/`deny`/`transition`), `writer_propose.rs`, `commit.rs` (`commit_action`, `journal_for`), `executor.rs` (`AuthorizedExecution::of`, the event-driven start and catch-up), `execute.rs` (`AuthorizedExecution`, `execute`: the only legacy `hand_off` and `await_handoff_result` calls, `execute.rs:252, 273`), `dispatch.rs`, `preflight.rs`, `freshness.rs`, `host_wait.rs`, `reject.rs`, `outcome.rs`, `record.rs`, `v2_plan.rs`, `expire.rs` (the sweep), `recover.rs` (start-up scans, loop guard reload), `approvals.rs`, `audit.rs` (kinds and builders), `deps.rs` (`ActionDeps`, `ActionSettings` with hard-coded defaults), `recheck.rs` (35 lines), `verify_wake.rs` (164), `reconcile/{mod,queue,answer}.rs`, and the in-file test modules `execute_tests.rs`, `freshness_tests.rs`, `reconcile/guard_tests.rs`. `crates/domovoi-engine/src/lib.rs:38` re-exports `ActionDeps, ActionHandle, ActionService, ActionSettings, ApprovalError`.

### 3.2 What operations (26) takes from it, and must be moved **before** deletion

26 made a few `actions/` items `pub(crate)` so the operation path can reuse them: `ReconcileOutcome`, `from_answer`, `reason_text` (`actions/reconcile/answer.rs:71-92`) and the reason constants (`EXPIRED_REASON`, `NOT_RUN_REASON`, `HANDOFF_FAILED_REASON`, `reconcile/mod.rs:27`). Find every other one with `grep -rn "crate::actions::\|actions::" crates/domovoi-engine/src --include=*.rs | grep -v "^crates/domovoi-engine/src/actions/"` and `grep -rn "actions::" crates/domovoi-engine/src/operations`. Each such item moves to a new file under `operations/` (for the reconcile text: `operations/reconcile_text.rs`) by `git mv` of the code and an import fix, **no behaviour change**, in its own commit.

### 3.3 Verification policy to prove, then retire

`recheck.rs` **(existing)**: `RECHECK_DEADLINE = 60 s`; `verify_delay(shared, skill, check)`: a skill with `verify: Some(spec)` and `spec.recheck` → `Some(spec.delay)`; `verify: Some(_)` with `recheck == false` → `None`; `verify: None` → the default delay only for a `Risk::Write` skill whose `applies_to` contains the incident's check; `verify_after` loads the incident and applies it. `verify_wake.rs` runs the durable `wake{purpose:"verify_action"}` effect: re-runs the check through `CheckRerun` with `RECHECK_DEADLINE` and commits `Succeeded → Verified | VerifyFailed`. Prompt 26 implements the same idea for steps (`Settle { verify_after }`, the `Verify` handler); **26 computed `verify_after` from "the skill's `verify` delay (default `settings.verify_default_delay`)"**, which may not reproduce the three-case rule above. Step 3 proves the rule on the operation path; step 5 moves the constant if the operation handler has none.

### 3.4 Consumers outside the module (existing; each is edited or deleted)

- Hub: `crates/domovoi-hub/src/api/actions.rs` (approve/deny handlers `:118-188`, `approval_error`, `load`), `api/mod.rs` (`ApiDeps.actions_service`, routes), `api/readiness.rs:74` (`deps.actions_service.is_alive()`), `auth/interactions/buttons.rs` (`approve`/`deny`/`load` legacy branch, `approval_reason`, imports of `ApprovalError`), `runtime/mod.rs:231-266, 317, 373` (`ActionService::spawn`, `server.set_reconciler(..)` at `:253` (composite since 26), the scheduler and triage wiring), `runtime/schedulers.rs:9, 39, 67, 91, 142` (`actions.expire_due()` every tick), `runtime/triage_build.rs`, `runtime/settings.rs:108` (`actions()` settings), `examples/dev_hub.rs`, `tests/hub/api_harness.rs`.
- Engine: `triage/mod.rs:37`, `triage/proposals.rs`, `triage/followup.rs:270-310` (a follow-up answer stores `Proposed` actions through `proposals::store` and publishes `ActionChanged`). **Prompt 58 removes `proposals::{create, store}`; its text keeps `follow_up` but does not say what a follow-up does with a proposal afterwards.** Step 1 must find out what 58 merged. If anything still creates an `actions` row, stop and report: this prompt cannot run.
- Events: `EventKind::{ActionApproved, ActionChanged, ActionBlocked}` and their uses in `notify/plan.rs:230-259`, `crates/domovoi-hub/src/api/events.rs` (or 28's journal mapping). Keep the event variants (the drain and older cards still need `ActionChanged` mapping); remove only senders that disappear with the module.
- Core: `ActionRequest` and its state machine (`crates/domovoi-core/src/action/{request,transition,approval,...}.rs`): **stays** (the projection reads it, the drain moves it, tests build it).
- Store: the `actions` table, `ActionRepo` (`get`, `list`, `insert`, `update_from`, `find_by_chat_token_hash`), `StateWrite::Action`, `ensure_token_unused` (`crates/domovoi-store/src/uow/writes.rs:86-109`), the repo `crates/domovoi-store/src/repos/actions.rs`, contract tests `crates/domovoi-testkit/src/contracts/actions.rs`: **stay** (the drain writes through `StateWrite::Action`, retention prunes the table).
- Web: the legacy approve and deny hooks and controls (`web/src/api/hooks/actions.ts`, `web/src/features/approvals/ApproveControl.tsx`, `ActionCard.tsx` and their tests) call `POST /api/v1/actions/{id}/approve|deny`. Prompt 30 may already have replaced them: check first.
- xtask and gates: `xtask/src/arch/rules.rs:212-393` (`BOUNDARY_PAIRS`, `BOUNDARY_CARDS`) and the cards 26 added; `xtask/src/arch/{ast,ast_tests}.rs`; `crates/domovoi-engine/tests/gates.rs` (`single_execution_boundary`, `the_boundary_gate_counts_each_operation_exactly_once`, `dispatch_construction_and_match_are_separate_sites`, `action_writes_go_through_the_unit_of_work`, the `boundary_tree` fixtures that name `actions/executor.rs`, `actions/execute.rs`, `actions/writer.rs`); `crates/domovoi-hub/tests/gates.rs`.
- Legacy tests (inventory in 12.1): `crates/domovoi-engine/tests/services/{actions,actions_harness,action_metrics,approve,atomic,execute,expiry,freshness,plan_binding,plan_v2,reconciliation,recovery,writer}.rs`, `tests/crash/{world,scenarios}.rs`, `crates/domovoi-hub/tests/hub/{atomic_actions,interactions,plan_binding,reconciliation,skills,sqlite}.rs`.

## 4. Exact scope

| Area | Change |
|---|---|
| `domovoi-engine` | move the items of 3.2; delete `src/actions/` and its re-exports; new module `src/legacy.rs` **(new)** with `drain_legacy_actions`; remove `ActionHandle`-based code from `triage/`; keep `chat_token.rs`, `operations/**` |
| `domovoi-core` | `ActionRequest::expire_at_upgrade` **(new)** and its tests; nothing else |
| `domovoi-hub` | delete the legacy approve/deny routes and handlers, the legacy button branch, `ApiDeps.actions_service`, the action service spawn and scheduler call; call the drain at start; make the legacy projection unconditional; readiness; examples and harnesses |
| `xtask` | cards and `BOUNDARY_PAIRS` to one site; tests in `ast_tests.rs` |
| `web/` | only if prompt 30 has not removed them: the legacy approve/deny hooks and controls; regenerated OpenAPI types |
| tests | the parity additions of step 3, then the deletions the matrix allows; the new tests of section 12 |
| docs | ADR, `ARCHITECTURE.md`, book, upgrade note (section 15) |

Not changed: `domovoi-proto`, `domovoi-agent`, `domovoi-store` source and migrations (no schema change), `HumanApproval::new` call sites, the `actions` table, `ActionRepo`.

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| Converting, deleting, archiving or hiding legacy rows; a new Legacy screen | RW-32: rows stay, read-only (27, 30) |
| Dropping the `actions` table or its `chat_token_hash` column; any migration | additive-only rule (05 §2); retention (57) prunes old rows by age |
| A reconcile pass for legacy `Dispatched` rows | decided in section 2: settle as `OutcomeUnknown` |
| New execution features, loop-guard or availability changes on the operation path | 26 owns them; this prompt only proves parity |
| Cancel, pause, resume, answer routes | 29 |
| The wire, the agent, helper or policy work | none |

### 5.1 What is kept because prompt 59 relies on it (explicit)

Removing "the chat-token machinery" is **narrowed**. Keep, unchanged:

- `crates/domovoi-engine/src/chat_token.rs`: `mint_token`, `token_hash`, `chat_token_ok` (moved there by prompt 59; `actions/approvals.rs` is deleted, the module is not);
- `OperationHandle::authorize(.., chat_token)`, `OperationHandle::rotate_chat_token`, `OperationError::BadToken`, `ExecutionRecord::{risk, chat_token_hash}`, `ExecutionError::NeedsTypedConfirmation`, the `executions.chat_token_hash` and `executions.risk` columns, the `Notify` token minting, `CardLinks.{tokens, steps}`, `WaitingStep`, `step_offers_chat`, `EventKind::ExecutionChanged`;
- `CardButton::{Approve, Deny, OpenInWeb}`, `CardPlan`, the Discord custom ID and every file of `crates/domovoi-chat/`;
- in the hub: `chat_authorization`, `chat_approval` (`auth/approval.rs`), `InteractionAuthorizer`, `step_for`, the step branches of `approve`/`deny`, `step_reason`, `WEB_ONLY`, `STALE_CARD`.

Retire only what belongs to legacy **actions**: `ActionHandle::{approve, deny, propose, rotate_chat_token, expire_due}`, `ActionRequest::with_chat_token_hash` (verify no non-test caller remains; `clear_chat_token_hash` is used by the drain and stays), `PROPOSER`, `triage::PROPOSAL_TTL`, `ApprovalError` and the hub's `approval_reason`, and `ActionRepo::find_by_chat_token_hash` **only if** `grep` shows no non-test user. The `actions.chat_token_hash` column, `ActionRequest::chat_token_hash()` and `ensure_token_unused` stay (the repo round-trips the column; the drain writes `NULL`).

## 6. Required interfaces and contracts

### 6.1 Core **fixed**

```rust
// crates/domovoi-core/src/action/transition.rs
impl ActionRequest {
    /// `Proposed → Expired` at once, before the approval window closes: the executor that
    /// would have run an approval is gone. Refused (`ActionError`) from any other status.
    pub fn expire_at_upgrade(&mut self, now: Timestamp) -> Result<ActionChange, ActionError>;
}
```
`Approved → NotRun` uses the existing `never_ran(reason, now)`; `Dispatched → OutcomeUnknown` uses the existing `outcome_unknown(reason, now)`. No other transition changes.

### 6.2 Engine: the one-shot retirement **fixed**

```rust
// crates/domovoi-engine/src/legacy.rs (new)
pub struct LegacyDrainDeps {
    pub actions: Arc<dyn ActionRepo>, pub incidents: Arc<dyn IncidentRepo>,
    pub uow: Arc<dyn UnitOfWork>, pub effects: Arc<dyn EffectQueue>, pub effect_read: Arc<dyn EffectRead>,
    pub clock: Arc<dyn Clock>,
}
/// What one run of the retirement did.
pub struct LegacyDrainReport { pub expired: u32, pub not_run: u32, pub outcome_unknown: u32,
                               pub verification_dropped: u32, pub effects_cancelled: u64 }
#[derive(Debug, thiserror::Error)] pub enum LegacyDrainError { #[error("store error: {0}")] Store(String) }
/// Retires every open legacy action, once, and returns what it did. Idempotent: a second run finds nothing.
pub async fn drain_legacy_actions(deps: &LegacyDrainDeps) -> Result<LegacyDrainReport, LegacyDrainError>;
```
(`EffectRead`, `EffectQueue`, `IncidentRepo` are the existing ports; verify their names. `EffectCancel` is introduced by prompt 26.)

### 6.3 Retirement rules and audited reasons **fixed**

Run **before** any task that could write actions or claim effects (section 7, step 6). Order inside one run:

1. `effects.revert_claimed_at_start(&[AggregateKind::Action])`: a wake claimed by the previous session returns to `pending` so it can be cancelled (the retiring process is the only one alive).
2. For each status, list with `ActionRepo::list` (paged to the end; a failing page restarts the listing, three tries, as `recover::scan` did; after that return `LegacyDrainError::Store`):

| Status found | Move | Audit kind | Reason (detail `reason`, and the status's own reason where it has one) |
|---|---|---|---|
| `proposed` | `expire_at_upgrade` → `Expired` | `action.expired` | `legacy action expired at upgrade` |
| `approved` | `never_ran(reason)` → `NotRun{reason}` | `action.not_run` | `legacy action expired at upgrade` |
| `dispatched` | `outcome_unknown(reason)` → `OutcomeUnknown{reason}` | `action.outcome_unknown` | `legacy executor removed before this run was reconciled` |
| `succeeded` with a pending `wake` effect of that action | no status change; cancel the effect | `action.verification_dropped` (new constant) | `legacy verification dropped at upgrade` |

Rows in any other status (`denied`, `expired`, `rejected`, `failed`, `verified`, `verify_failed`, `outcome_unknown`, `not_run`) are not touched.

3. Each move is **one `Transaction`**: `StateWrite::Action { action, expected: ActionGuard::Status(from) }` with the token hash cleared (`clear_chat_token_hash`), the audit row, a journal row of kind `JournalKind::ActionTransition` in the shape `actions/commit.rs:15-45` writes (copy `journal_for` before deleting it; `from`/`to`, `plan_hash`, and the reason), and `EffectCancel { aggregate: AggregateRef::action(id), at: now }` so every pending effect of the action ends in the same commit. A `CommitError::Conflict` (someone else moved the row) skips that row and counts nothing; any other error aborts the run with `Store`. For the `succeeded` case the transaction holds the audit row and the cancel only; if the unit of work refuses a transaction with no state write, append the audit row through the audit port in the same function after the cancel commits and say so in the report.
4. Actor: `Actor::System("legacy")` (displays as `system:legacy`); audit subject `action:<id>`; the detail carries `incident`, `skill`, `host`, the 8-hex plan hash and `reason`, the same fields `actions/audit.rs` writes for the same kinds (copy `event()`'s shape; the kinds `action.expired`, `action.not_run`, `action.outcome_unknown` already exist and keep their meaning).
5. Log one `tracing::info!` with the report counts when any is non-zero. No event is published (cards refresh on their next change; a press on an old legacy button is answered with `STALE_CARD`).

### 6.4 Hub **fixed**

- `HubRuntime::assemble` calls `drain_legacy_actions` right after the store, unit of work and effect queue exist and **before** the operation service, the triage service, the agent server's reconciler registration or any scheduler spawns. A returned error is propagated like the other start-up store errors; it never panics and never skips the call.
- Removed routes: `POST /api/v1/actions/{id}/approve` and `POST /api/v1/actions/{id}/deny` (handlers, `ApproveRequest`/`approval_error`, utoipa annotations). Kept: `GET /api/v1/actions`, `GET /api/v1/actions/{id}`; `ActionDto.legacy` is `true` for every row unconditionally (27 made it depend on the flag).
- `buttons.rs`: `approve` and `deny` keep only the step branch of prompt 59; when `step_for` returns `None` (the id is not an execution) the answer is `Err(STALE_CARD)`. `approval_reason` and the `ApprovalError` import are deleted.
- `ApiDeps.actions_service` removed; `readiness.rs` checks only the operation service (when enabled); `schedulers.rs` no longer takes or calls an action handle; `server.set_reconciler(..)` registers the operation handle alone (the composite of 26 collapses to it).
- `settings::actions()` and its `ActionSettings` import are deleted.

### 6.5 Architecture cards **fixed** (end state; the ADR of section 15 records it)

In `xtask/src/arch/rules.rs`:

- `BOUNDARY_PAIRS` keeps `"AuthorizedStep::of"`, `"execute::execute_step"`, `"OperationCommand::Dispatch"`; **removes** `"AuthorizedExecution::of"`, `"execute::execute"`, `"WriteCommand::Dispatch"`.
- `BOUNDARY_CARDS` (EXEC-2): the cards for `AuthorizedExecution::of`, `execute::execute` and the three `WriteCommand::Dispatch` cards (construct, pattern, pair) are **deleted**; `Call("hand_off")` and `Call("await_handoff_result")` each list **only** `crates/domovoi-engine/src/operations/execute.rs` with count 1 (the `actions/execute.rs` site is removed); the cards for `AuthorizedStep::of`, `execute::execute_step` (site `operations/effects/dispatch.rs`, 1 each) and `OperationCommand::Dispatch` (construct in `operations/execute.rs` 1, pattern in `operations/writer/mod.rs` 1, pair none) stay exactly as 26 left them. The raw-call cards (`run_tool`, `start_skill`, `skill_result`, `run_check`, `run_skill`, `run_capability`) keep their sites; update only the message strings that name `execute(AuthorizedExecution)`, `executor.rs` or `verify_wake.rs` so they name the operation boundary (`operations/execute.rs`, the `Verify` handler).
- `xtask/src/arch/ast_tests.rs`: each legacy card's test becomes its operation equivalent (the exact allowed site passes, a second construction, a second call or one in another file fails with `EXEC-2`), **and** new negative tests assert that the retired names are now violations anywhere: a `hand_off(` in any `actions/` path, `AuthorizedExecution::of` and `WriteCommand::Dispatch` in any file.
- `crates/domovoi-engine/tests/gates.rs`: `boundary_tree` fixtures and the three boundary tests are rewritten onto `operations/execute.rs`, `operations/effects/dispatch.rs` and `operations/writer/mod.rs`; every assertion survives with the same strength (count exactly one; second site fails); `action_writes_go_through_the_unit_of_work` keeps its scan and now also lists `legacy.rs` as a permitted writer (it writes only through the unit of work).

## 7. Ordered implementation steps

1. **Inspect and verify; report discrepancies before editing.**
   - Open every file and symbol of section 3. Confirm 26, 27, 28, 58 and 59 are merged and write down their real names. Run `grep -rn "ActionRequest::propose\|actions_service\|proposals::store\|proposals::create" crates --include=*.rs`: apart from `crates/domovoi-engine/src/actions/` and tests there must be **no** caller; if a follow-up still stores actions, **stop and report**.
   - Run `grep -rn "crate::actions::" crates/domovoi-engine/src` and list every item `operations/` takes from `actions/`.
   - Run the baseline: `cargo xtask arch-check`; `cargo nextest run -p domovoi-engine -p domovoi-hub -p domovoi-store`; `ls crates/domovoi-store/migrations/` (this prompt adds none).
2. Create branch `remove-legacy-executor` from the latest `overhaul-investigation`.
3. **Parity checkpoint (the launch gate).** Build the matrix of 12.1 as a table in the PR description: for every legacy test of the listed files and every legacy gate, the name of the operation-path test that proves the same behaviour, or "mechanism replaced" with the replacing test and one sentence. Run all of them on the unchanged base and record the result. Add every missing operation-path test as a named test in `crates/domovoi-engine/tests/operations/legacy_parity.rs` **(new)** (registered in `tests/operations/main.rs`), using prompt 26's harness. Prove the verification rule of 3.3 on the operation path (`verification_delay_follows_the_skills_verify_spec_or_the_write_default`). Commit `test(engine): prove the action safety behaviours on the operation path`. **If any row cannot be filled, stop and report.**
4. **Move** the items of 3.2 (and the `RECHECK_DEADLINE` constant if the operation `Verify` handler lacks an equivalent) out of `actions/` with import fixes only. The whole engine suite passes unchanged. Commit `refactor(engine): move the reconcile text helpers into operations`.
5. **Core and drain**: `expire_at_upgrade` with tests; `legacy.rs` with the rules of 6.3; its tests (12.2). Commit `feat(engine): retire open legacy actions once at upgrade`.
6. **Hub wiring of the drain**, called at start (6.4) while the legacy executor still exists: here it is a no-op for a tree with no open rows, and the hub test `drain_runs_before_any_service_starts` (12.3) proves the order. Commit `feat(hub): retire open legacy actions at start`.
7. **Delete the consumers**: routes and handlers, the legacy button branch, `ApiDeps.actions_service`, scheduler call, readiness, `settings::actions()`, `triage` imports, the composite reconciler, the `ActionService::spawn` block; web hooks and controls if still present; `cargo xtask openapi && cd web && npm run gen:api`; `npm run lint && npm run typecheck && npm test` for the touched web area. The tree must compile after this step with `actions/` still present but unused.
8. **Delete `crates/domovoi-engine/src/actions/` and its re-exports**; let the compiler list the breakage; remove the legacy tests the matrix maps (never one without a mapped equivalent) and port the helpers that other tests used (`tests/services/actions_harness.rs` helpers that only seeded `Proposed`/`Approved` rows keep working through `ActionRepo::insert`). Rewrite the cards, `ast_tests.rs` and the engine gates as in 6.5.
9. **ADR, `ARCHITECTURE.md`, book, upgrade note** (section 15); `cargo xtask docs-links`.
10. **Checks and the completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-core -p domovoi-engine -p domovoi-hub -p domovoi-store -p domovoi-testkit -p xtask --all-targets -- -D warnings`; `cargo nextest run -p domovoi-core -p domovoi-engine -p domovoi-hub -p domovoi-store -p xtask`; `cargo nextest run -E 'rdeps(domovoi-engine)'` (public items were removed); `cargo xtask arch-check` and `cargo xtask arch-check --base origin/overhaul-investigation`; `cargo xtask openapi` shows no further diff.

Commit in small green steps (one-line Conventional Commits, one scope each): after steps 3, 4, 5, 6, 7, and 8 together with the card rewrite, then 9.

## 8. Database and persistence

**No migration and no schema change** (`ls crates/domovoi-store/migrations/` identical before and after). The drain writes only existing columns: `actions.status` (JSON), `updated_at`, `chat_token_hash = NULL`, plus audit, journal and cancelled `effects` rows, each in one transaction with the store's status guard. No row is deleted or converted. **N-1 upgrade test:** `drain_retires_a_committed_fixture_database`: open the committed fixture database (the mechanism of `crates/domovoi-hub/tests/hub/upgrade_flow.rs`; extend the fixture or build the rows through the store's own repo), assert each status of the table in 6.3 ends as specified, every other row byte-identical, and a second run reports zeros.

## 9. Protocol and compatibility

No wire change; the agent never sees this change. HTTP: two routes removed (a client that calls them gets 404/405); `ActionDto` unchanged; the OpenAPI document and generated web types shrink and are regenerated. Chat: cards that still show an old legacy button answer `This card is out of date; open the incident in the web UI to approve`.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| First start with a legacy `Proposed` row inside its window | `Expired`, audit `action.expired` reason `legacy action expired at upgrade`, token cleared, pending effects cancelled |
| Legacy `Approved` row | `NotRun{legacy action expired at upgrade}`, audit `action.not_run` |
| Legacy `Dispatched` row | `OutcomeUnknown{legacy executor removed before this run was reconciled}`, audit `action.outcome_unknown`; never `Failed`, never re-dispatched; the agent's journal keeps the real record |
| Legacy `Succeeded` row with a pending verify wake (even one claimed by the previous session) | wake cancelled, audit `action.verification_dropped`, status stays `Succeeded` |
| Any other status | untouched |
| Second start | nothing open: all counts zero, no audit rows |
| A row moved by someone else between the list and the commit | skipped (`Conflict`), no write |
| Store failure during the run | `LegacyDrainError::Store`; start-up fails like any other store error; nothing half-written (one transaction per row); the next start repeats the run |
| Hub starts with `orchestration.enabled = false` | the drain still runs; no execution of anything exists afterwards; old rows are read-only |
| A press on an old legacy Approve or Deny button | `STALE_CARD`; the service is not called |
| An old client posts to a removed route | 404 or 405 |
| An operation execution in flight at the upgrade | unaffected: the drain touches only `actions` rows and `aggregate = action` effects |

## 11. Security requirements

- **INV-1.** No approval is created, restored or changed: the drain moves a row toward a terminal state and never to `Approved`; `HumanApproval::new` and `restore` call sites are unchanged; the only authorizers of steps stay the two call sites of prompt 59; the legacy approve path no longer exists, so `only_approve_reaches_approved_state` is restated on steps (matrix row).
- **INV-4 and the dispatch rules.** Parity on the operation path is proved before deletion: v2 hash recomputed at dispatch, approval unexpired, fail-closed registry and session checks, loop guard, no blind retry, `OutcomeUnknown` terminal. Nothing in this prompt relaxes them.
- **One boundary.** After this change a skill reaches an agent only through `operations/execute.rs`; the cards and gates of 6.5 enforce it, with negative tests for the retired names.
- **Nothing runs by accident.** An `Approved` legacy row can no longer dispatch (no executor), and is settled `NotRun` so it can never be mistaken for pending work; a `Dispatched` row is never re-run.
- **No invented history.** Reasons are plain, audited, and say what happened to the row (RW-32). Plan hashes appear only as their 8-hex form in audit details.
- **Fail closed.** A store error stops start-up rather than leaving a half-retired state unannounced.
- **Secrets.** Unchanged; the drain reads no token (it writes `NULL` over the hash).

## 12. Testing requirements

Names describe behaviour. Fakes that exist: `InMemoryStore`, `FakeClock`, `FakeAgentLink`, `ScriptedLlm`, the operation harness of prompt 26 (`crates/domovoi-engine/tests/operations/harness.rs`), the hub harnesses `api_harness.rs` and `interactions.rs`. Tests never sleep.

### 12.1 The parity matrix (step 3; required rows, the executor adds any missing ones)

Legacy tests (existing; enumerate with `grep -A1 "#\[tokio::test\]\|#\[test\]" <file> | grep "fn "`) grouped by the behaviour they protect. For each group there must be an operation-path test, **found or added** in `tests/operations/` (names below are the required names for tests you add; if prompt 26 or 27 already has an equivalent, cite it instead and do not duplicate):

| Group (legacy tests) | Required operation-path behaviour (test name if added) |
|---|---|
| Approve once, stale writes refused (`writer.rs: approve_and_deny_at_once_leave_exactly_one_outcome`; store `update_from_refuses_a_stale_status` stays) | `approve_and_deny_at_once_leave_exactly_one_outcome_on_a_step` |
| Expiry never overwrites a decision (`writer.rs: expiry_never_overwrites_a_decision`, `expire_due_reads_only_what_is_due`; `expiry.rs` all six) | `expiry_never_overwrites_a_step_approval`, `expiry_settles_only_steps_whose_window_closed`, `approved_step_whose_window_closed_is_not_run`, `proposed_step_expiry_is_final` |
| Approval rules (`approve.rs`: token, expired, denied; gates `only_approve_reaches_approved_state`, `destructive_action_cannot_be_approved_from_chat`, `destructive_action_needs_typed_confirmation_on_web`, hub `destructive_action_cannot_be_approved_from_discord`, `discord_message_text_can_never_approve`) | covered by prompt 59 and 27 tests: cite `chat_authorize_*`, `web_authorize_of_a_destructive_step_needs_typed_confirmation`, `destructive_step_cannot_be_approved_from_discord`, `discord_message_text_can_never_approve_a_step`, `step_authorization_has_exactly_two_production_call_sites` |
| Plan binding (`plan_binding.rs`, `plan_v2.rs` 23 tests, hub `plan_binding.rs`) | cite 26/22 tests (`authorize_with_another_hash_changes_nothing`, dispatch-time recomputation); add `manifest_change_between_approval_and_dispatch_refuses_the_step` if absent |
| Dispatch safety (`execute.rs`: success and verify, failed verification reaches a human, manifest change rejects, args no longer valid, loop guard blocks the fourth dispatch, offline agent waits then is rejected, audit trail complete and ordered, output redacted and capped) | `approved_step_dispatches_succeeds_and_verifies`, `failed_verification_starts_another_cycle_or_reaches_a_human`, `args_no_longer_valid_refuse_the_step`, `loop_guard_blocks_the_fourth_dispatch_in_an_hour`, `offline_host_defers_dispatch_until_the_window_closes` (the legacy 30-minute offline wait is replaced by waiting inside the approval window: list as "mechanism replaced"), `step_audit_trail_is_complete_and_ordered`, `step_output_is_redacted_and_capped` |
| Freshness before dispatch (`freshness.rs` all 11) | one test per group: resolved or deleted incident, plan changed while host unknown, skill missing after the host connects, incompatible host, unknown host past the wait, host returning in time, waiting runs do not starve other hosts |
| Recovery (`recovery.rs`: approved before a restart runs after it, dispatched stays dispatched, loop guard remembered across a restart, stale approval not dispatched, catch-up waits for an unknown host, bounded catch-up) | `step_approved_before_a_restart_runs_after_it`, `dispatched_step_stays_dispatched_after_a_restart`, `loop_guard_survives_a_restart`, `stale_step_approval_is_not_dispatched_after_a_restart` |
| Reconciliation outcomes (`reconciliation.rs`: every outcome-classification test: crash after accepted or started, finished reply lost, hub crash before commit, epoch change, not-found with matching epoch, journal lost, pre-upgrade row, idempotent, running stays dispatched and is never re-dispatched, a sent run is never mistaken for not run, a terminal run is never sent, hand-off failures prove not-sent, timeout/lost reply leave `Dispatched`, agent refusal and timeout are confirmed results, duplicate and contradictory answers) | one operation-path test per **outcome class** (26's `Reconcile` handler tests count). The scheduling and pool tests (bounded passes, hello gating, slot waits, cancelled passes, listing retries) test the legacy in-memory pass machinery that durable `Reconcile` effects replace: list each as "mechanism replaced" and cite the effect-lifecycle tests (`claim`, `defer`, lease fencing) that cover the replacement |
| Crash harness (`tests/crash/scenarios.rs`: `action_lifecycle`, `left_dispatched`, `store_sent`, `wait_for_rescheduled`) | the operation crash scenarios of 26; add an `execution_lifecycle` scenario if 26 has none |
| Atomic writes and metrics (`atomic.rs`, `action_metrics.rs`) | cite 26's `approving_creates_a_dispatch_effect_and_an_audit_row_in_one_commit` and the operation commit tests; record that the metric families `domovoi_action_*` stop being emitted for new runs and say what replaces them (verify in `telemetry.rs`) |
| Gates (`single_execution_boundary`, `the_boundary_gate_counts_each_operation_exactly_once`, `dispatch_construction_and_match_are_separate_sites`, `action_writes_go_through_the_unit_of_work`, `rejected_or_failed_actions_never_retry_silently`) | the rewritten gates of 6.5, and `rejected_or_failed_steps_never_retry_silently` (the "no blind retry" rule of 26) |
| Verification policy (`recheck.rs`) | `verification_delay_follows_the_skills_verify_spec_or_the_write_default`: a table over (a) `verify: recheck=true, delay=D` → D, (b) `verify: recheck=false` → none, (c) no `verify`, `Risk::Write`, skill applies to the incident's check → the default delay, (d) no `verify`, write skill that does not apply to the check → none, (e) no `verify`, read skill → none; and `verification_recheck_uses_a_sixty_second_deadline` |

### 12.2 New tests (engine, `crates/domovoi-engine/tests/legacy.rs` **(new)**, registered as a test target as the other targets are; core unit tests next to the code)

| Test | Setup, action, expected |
|---|---|
| `proposed_action_inside_its_window_is_expired_at_upgrade` | seed `Proposed` (window open, token set); drain: `Expired`, token `None`, audit `action.expired` with the reason, journal row, report `expired = 1` |
| `approved_action_is_not_run_at_upgrade` | `Approved`: `NotRun{legacy action expired at upgrade}`, audit `action.not_run` |
| `dispatched_action_becomes_outcome_unknown_not_failed` | `Dispatched{agent_epoch}`: `OutcomeUnknown` with the reason of 6.3, audit `action.outcome_unknown`; never `Failed` |
| `pending_verification_is_cancelled_and_the_action_stays_succeeded` | `Succeeded` with a pending `wake{verify_action}` effect: effect `cancelled`, status unchanged, audit `action.verification_dropped` |
| `a_claimed_verification_wake_is_cancelled_too` | the same with the effect claimed by a previous session: reverted then cancelled |
| `terminal_actions_are_left_untouched` | one row in each other status: byte-identical after the run, no audit rows |
| `drain_is_idempotent` | second run: all zero, no new audit, journal or effect rows |
| `drain_skips_a_row_decided_meanwhile` | a store wrapper that moves the row after the list: skipped, no error, the other rows still retired |
| `drain_survives_a_failing_page_by_restarting_the_listing` | the fake store fails page two once: the run completes; fails three times: `Err(Store)` and no row changed |
| `drain_writes_through_the_unit_of_work_only` | the existing scan gate plus a unit assertion that each retired row has audit and journal rows from the same transaction (a fake unit of work that fails the audit leaves the row unchanged) |
| `expire_at_upgrade_is_only_valid_from_proposed` | core: `Approved`, `Dispatched`, terminal statuses refuse with `ActionError` |
| `no_open_legacy_action_remains_after_the_drain` | seed one row per status; after the drain none is `proposed`, `approved` or `dispatched` |

### 12.3 Hub (`crates/domovoi-hub/tests/hub/legacy_retirement.rs` **(new)**)

| Test | Expected |
|---|---|
| `drain_runs_before_any_service_starts` | assemble the runtime over a store holding an open legacy row of each kind: by the time the API answers, no row is open and the audit chain verifies |
| `legacy_projection_lists_retired_rows_read_only` | `GET /actions` and `GET /actions/{id}` return the rows with `legacy: true` and their new statuses; no route accepts a decision |
| `approve_and_deny_routes_are_gone` | `POST /api/v1/actions/{id}/approve` and `.../deny` answer 404 or 405 with a valid session and CSRF header |
| `old_legacy_button_press_is_refused_as_out_of_date` | a press built from a legacy action id: `STALE_CARD`, nothing changes |
| `step_buttons_still_work_after_the_removal` | the prompt-59 hub tests `chat_approve_step_with_valid_token_authorizes_once` and `chat_deny_step_refuses_the_execution` pass unchanged |
| `readiness_ignores_the_removed_action_service` | `/readyz` is ready with orchestration on and off |
| `orchestration_off_still_retires_legacy_rows_and_executes_nothing` | flag false: rows retired, no dispatch effect exists, `GET /operations` is empty |
| `drain_retires_a_committed_fixture_database` | section 8 |

### 12.4 Gates and xtask

Rewritten as 6.5; the ast tests assert the retired names fail. Add engine gates: `no_actions_module_remains` (no `src/actions/` directory under `domovoi-engine`; no `ActionService`, `ActionHandle`, `WriteCommand`, `AuthorizedExecution`, `ApprovalError` identifier in `crates/*/src`), `chat_token_pieces_prompt_59_needs_still_exist` (a source scan finds `pub fn mint_token`, `pub fn token_hash` and `fn rotate_chat_token` where section 5.1 says), `legacy_rows_are_written_only_by_the_drain` (a scan finds `StateWrite::Action` constructed only in `crates/domovoi-engine/src/legacy.rs` and the store/testkit). Never weaken a gate: each rewritten assertion keeps its strength.

## 13. Acceptance criteria

1. The parity matrix of 12.1 is complete, every row green on the base branch before any deletion, and recorded in the PR; nothing deleted lacks a mapped equivalent.
2. `crates/domovoi-engine/src/actions/` and the legacy approve and deny routes are gone; `GET /api/v1/actions` and `GET /api/v1/actions/{id}` return the legacy rows read-only with `legacy: true`.
3. `hand_off` and `await_handoff_result` have exactly one production call site each, in `operations/execute.rs`; `AuthorizedExecution::of`, `execute::execute` and `WriteCommand::Dispatch` appear nowhere; the EXEC-2 cards list only operation sites; the ast tests and the engine gates prove each, including that the retired names fail.
4. The first start retires every open legacy action as specified in 6.3: `Proposed` → `Expired`, `Approved` → `NotRun` (both "legacy action expired at upgrade"), `Dispatched` → `OutcomeUnknown`, pending verification dropped; every step is audited, journaled and committed in one transaction; a second start does nothing; no row is deleted or converted.
5. A `Dispatched` legacy row never blocks start-up and is never re-run or marked `Failed`.
6. The verification rule of `recheck.rs` is proved on the operation path before it is retired (`verification_delay_follows_the_skills_verify_spec_or_the_write_default`).
7. Everything prompt 59 relies on (section 5.1) still exists and its tests pass unchanged; `HumanApproval::new` call sites are unchanged; the only authorizers of steps are the two call sites of prompt 59.
8. No migration, no wire change; `cargo xtask openapi` and `npm run gen:api` are current; `cargo xtask arch-check` (including `--base`), clippy and the nextest runs of step 10 pass; nothing is `#[ignore]`d and no gate is weakened.
9. The ADR is committed and `ARCHITECTURE.md`, the book and the upgrade note say what section 15 requires.

## 14. Migration and compatibility

- **Existing installs.** On the first start of this release, open legacy actions are retired as in 6.3 and logged once (counts). Everything else (incidents, audit, operations, steps, notes) is unchanged. Recommended release order: ship this change at least one release after the one that stopped creating legacy actions, so that proposals created by the old path have long expired and in-flight runs have had a normal chance to reconcile; the retirement is correct either way.
- **Legacy rows.** Retained, read-only, labelled Legacy; the agent's journal still holds the real result of any run whose row is `OutcomeUnknown`.
- **Chat.** Old legacy buttons answer "out of date"; step buttons (prompt 59) work.
- **Rollback.** Not possible without the previous binary re-opening retired rows by hand: retired rows are terminal. Take the database backup the project already recommends before upgrading (say so in the upgrade note).
- **Agents, wire, config.** No change; no configuration key is added or removed (`ActionSettings` had hard-coded defaults).

## 15. Documentation requirements

- **ADR** `docs/adr/NNNN-one-execution-path.md` (next free number; required because `xtask/src/arch/rules.rs` changes). Record: the single execution site and its cards (the exact list of 6.5); the retirement of `actions/`; the decision that `Dispatched` legacy rows become `OutcomeUnknown` at the first start and never block it (with the reasons of section 2); the audited reasons; that legacy rows stay as a read-only projection. Mark ADR 0019 and 0022 "Superseded in part by NNNN" as the ADR README prescribes (status line only; do not edit their bodies), and add NNNN to the README index.
- `ARCHITECTURE.md`: §12 (skills and actions: one execution boundary; the card list; legacy rows read-only), §7.1 (no action service, no action writer task; the retirement at start), §15 (approval invariants: the step authorize call sites; the legacy path is gone), §10 (HTTP: the two routes removed). Do not edit §4.
- Book: `docs/book/src/concepts/actions.md` (legacy actions are read-only history, what the statuses `expired`/`not_run`/`outcome_unknown` with the reasons mean, how to read them), `docs/book/src/concepts/operations.md` (approval is per step, from the web or a card), an upgrade note where release changes are listed, if such a page exists: what happens to open legacy actions at upgrade and the backup advice. Run `cargo xtask docs-links`.
- OpenAPI: `cargo xtask openapi && cd web && npm run gen:api`; commit both generated files; never hand-edit them.
- Do not edit `docs/rework/`. Do not mention prompts, the build plan or coding agents in any committed file.

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
