# 58 — Remove the triage queue and make investigations the default

| | |
|---|---|
| Branch | `remove-legacy-triage-path` |
| Milestone | M3 |
| Depends on (must be merged) | 27, 28, 59 (and 26, which 27 depends on). 59 (`chat-step-approvals`) gives operation steps chat approve and deny buttons, so the default flip here loses no chat approval. Prompts 26, 27 and 28 are written but may not be merged when this brief is read: **every name taken from them is "introduced by prompt NN — verify in code before use"** |
| Can run in parallel with | 30 (order-flexible, never concurrent; 30 builds screens only, this prompt touches no `web/` source) |
| Produces (shared contracts) | none new. It removes: `TriageHandle::{triage, retriage, queued}`, the triage `Queue`, in-memory deferred retries, `TriageSettings::{queue, run_queue}`, legacy proposal creation (`proposals::{create, store}`), `TriageDeps::{actions, actions_service}`. It changes one default: `[orchestration] enabled = true` |
| Consumes | the investigation pipeline, `OperationHandle::{investigate, investigate_and_wait}` and the parity suite of prompt 27; the journal SSE of prompt 28 (so default-on investigations are visible live) |
| Launch gate | **the parity suite of prompt 27 (`crates/domovoi-engine/tests/services/parity.rs`, including `scripted_drills_pass_under_both_modes`) is green on the base branch, run by you in step 1 and recorded in the PR**, and prompt 59 merged |
| Suggested model | medium: mostly deletion and test porting, with two judgement calls already decided here |

## Ownership notes

Two items an earlier draft of this brief left open are now owned by other prompts. Neither is done here.

1. **Chat approve and deny of steps** is prompt 59 (`chat-step-approvals`), which must be merged before
   this prompt. Chat cards for new proposals therefore keep their buttons after the default flip. Confirm
   in step 1 that the Discord card of an Execution step has approve and deny buttons; if it does not,
   stop and report.
2. **Removing the legacy action executor and expiring the remaining legacy rows** is prompt 60
   (`remove-legacy-executor`), which runs after this prompt. Section 5.1 lists what this prompt leaves in
   place for it.

## 1. Objective

After this change:

- a new incident is investigated by an Investigate operation **by default** (`[orchestration] enabled` defaults to `true`), and the in-memory triage queue, its workers, its deferred retries and its start-up reload are **gone** (retries were already durable effects in prompt 27);
- triage no longer creates legacy `Proposed` actions or chat tokens; the only way a model proposal becomes pending work is an Execution step, approved on the web or with the chat buttons of prompt 59;
- `TriageHandle` keeps what is still needed: follow-up questions, chat and their history, which share the admission gate with investigations;
- setting `[orchestration] enabled = false` is the owner's kill switch: no operation service, no automatic investigation (a card still goes out untriaged, INV-11), follow-up questions and chat still work, the legacy approve endpoints and the legacy executor still finish old actions;
- the suites that ran the triage drills and scenarios under both modes now run them once, on the operation path, against **frozen expectations** recorded from the legacy path before it is deleted, so no check gets weaker.

It does **not** remove the legacy action executor, `actions/`, `recheck.rs` or the second dispatch site (section 5.1), does not add chat approval for steps, and adds no screen, migration or wire change.

## 2. Architectural context

**RW-04 (Operations replace the triage queue).** Triage state was stringly-typed timeline rows plus an in-memory queue and in-memory deferred retries (lost on restart). An Investigate operation holds the same work durably: one reasoning turn, its report, its proposals as steps, its retries as `effects`, its notification as a step. Prompt 27 built and proved that path behind `orchestration.enabled`; this prompt makes it the only automatic path.

**05 §7 (parity checkpoint).** The queue may be removed only after the parity suite passes: same report fields, budgets, fallback, host scoping, and the scripted drills unchanged under both flag values. That suite exists (27). This prompt's launch gate is that it is green.

**RW-32 / RW-36 (legacy actions).** `actions` rows are retained and shown read-only, labelled **Legacy**, never converted. While the legacy executor exists, pending legacy actions follow the legacy path under their `ActionId`. After this prompt nothing creates a new row in `actions`; the table only drains.

**RW-11 (approval stages, M3).** New work is approved on steps; the action endpoint serves legacy actions only. `HumanApproval::new` stays only in `crates/domovoi-hub/src/auth/`.

**What the flag means now.** `orchestration.enabled = true` (default): operations run, `TriageRequested` creates an investigation when triage is enabled. `false`: no operation service; nothing consumes `TriageRequested`; incidents are not triaged automatically; cards work without triage (INV-11). Follow-up questions, chat and the legacy action path do not depend on the flag.

## 3. Repository context

Verify each item first; prompt 27's edits may have shifted lines. Marks: **(existing, path:line)** at the time of writing, **(introduced by prompt 27)** where noted.

### 3.1 What to remove

- `crates/domovoi-engine/src/triage/queue.rs` (351 lines): `Queue`, `serve`, `retry_deferred`, `take_due`, `earliest`, `retry`, `reload_at_start`, `reload`, `skip_dropped`, `worker` **(existing, `queue.rs:16-351`)**. Its `serve` is spawned in `TriageService::spawn` **(`mod.rs:281`)**, or skipped when `run_queue` is false (introduced by prompt 27).
- In `triage/mod.rs`: `DEFAULT_QUEUE` (`:58`), `TriageSettings::queue` (`:77-79`) and (27) `run_queue`, `RetryReason` (`:202-208`), `Shared::{queue, deferred, deferred_ready}` and `Shared::defer` (`:210-236`), `TriageHandle::{triage, retriage, queued}` (`:286-296, 345-349`), `PROPOSAL_TTL` (`:60-61`, used only by legacy proposals), the export `PROPOSER` (`:49`). The `mint_token` and `token_hash` exports are **not** removed: prompt 59 moved those helpers to `crates/domovoi-engine/src/chat_token.rs`, which stays; remove only re-exports that still point into `proposals.rs`.
- In `triage/run.rs`: `triage()` and `outcome()` (the legacy composition) and the `RetryReason`/`shared.defer` calls **(existing, `run.rs:40-157`)**; 27 split out `prepare`, `generate`, `refusal_text`, `failed`, `succeeded`: **keep those**.
- In `triage/proposals.rs`: `create`, `store`, `same_plan`, `session_current`, `PROPOSER` and any copy of `mint_token`/`token_hash` left there (the live helpers are in `chat_token.rs`, prompt 59) **(existing, `proposals.rs:20-304`)**; **keep** `check` and `append_dropped` (27 made them `pub(crate)`; the investigation uses them), so the file shrinks to about 80 lines.
- `TriageDeps::{actions, actions_service}` **(existing, `mod.rs:148-152`)**: they existed only for proposals. Callers: `crates/domovoi-hub/src/runtime/triage_build.rs:93-117` (and the `actions` field of `TriageBuild`), every test world that builds `TriageDeps` (`tests/drills/world.rs:312-340`, `tests/services/harness.rs:279-303, 364`, others: `grep -rn "actions_service" crates/domovoi-engine/tests`).
- `HubRuntime::assemble` order: triage no longer needs the action service, so `triage_build::build` no longer takes `actions` **(existing, `runtime/mod.rs:260-275`)**.

### 3.2 What stays

`TriageHandle::{follow_up, chat, history}`, `Shared::{deps, settings, limits, questions}` (the `questions` semaphore is sized by `settings.concurrency`, so **`[triage] concurrency` stays**, now meaning "questions and chat at once"); `admission`, `cache`, `context`, `report`, `agent`, `complete`, `prompt`, `tools`, `history`, `followup`, `chat`; `FAILED_RETRY`, `FAILED_RETRY_LIMIT`, `DEGRADED_AFTER` (used by the investigation handlers); `EventKind::TriageRequested` and `TriageCompleted { tokens }` (the field stays, always empty for new outcomes, because cards of legacy actions still read it); `ActionHandle::propose` and the whole `actions/` module (legacy executor).

### 3.3 Verified in code: `recheck.rs`

Prompt 15 already replaced the in-memory verification sleep with durable `wake` effects: `actions/recheck.rs` now holds only the delay policy (`verify_delay`, `verify_after`) and `RECHECK_DEADLINE`, used by `actions/execute.rs:26` and `actions/verify_wake.rs:17` **(existing)**. There is no remaining sleep or in-memory timer to remove. What is left is the legacy executor's own verification policy and goes with the executor (5.1). **Do not touch it.** Confirm with `grep -rn "recheck" crates/domovoi-engine/src` in step 1 and say so in the report.

### 3.4 Config, hub, tests

- `OrchestrationSection` and its `Default` / `#[serde(default)]` functions in `crates/domovoi-config/src/hub/orchestration.rs` (introduced by prompt 26); test `orchestration_is_disabled_by_default` (26), `flag_off_starts_no_operation_service` and `flag_on_starts_the_operation_service_and_the_composite_reconciler` (26).
- Hub: `retry_triage` **(existing, `api/incidents.rs:470-491`, changed by 27 to use `investigate_and_wait` when `deps.operations` is `Some`)**; `ApiError::{TriageOff, Unavailable}` **(`api/error.rs:78-106`)**; the other users of `deps.triage`: `api/chat.rs:117`, `api/followups.rs:59`, `api/incidents.rs:435` (ask), which stay.
- Gates: `crates/domovoi-engine/tests/gates.rs` and `crates/domovoi-hub/tests/gates.rs` contain checks that touch triage and proposals (the INV-9 "injection creates no actions" family, the "only read tools" family, destructive-web-only). Find them with `grep -n "triage\|proposals\|Proposed" crates/*/tests/gates.rs`.
- Tests that call the legacy entry points: `grep -rn "\.triage\.triage(\|\.retriage(\|\.queued()\|actions_service" crates/domovoi-engine/tests crates/domovoi-hub/tests` (about 35 call sites in `tests/services/{actions,limits,attach,harness,capability,chat}.rs` and `tests/drills/world.rs`).
- The parity suite and its `Mode` (prompt 27): `tests/services/parity.rs`, `tests/services/harness.rs`, `tests/drills/world.rs`.

## 4. Exact scope

| Area | Change |
|---|---|
| `domovoi-engine` `src/triage/` | delete `queue.rs`; remove the items of 3.1; `TriageService::spawn(deps, settings)` keeps its signature and return type: it builds `Shared` and returns `(TriageHandle, JoinHandle<()>)` where the task simply waits for `deps.shutdown`; `TriageSettings` loses `queue` and `run_queue`; update module docs (`triage/mod.rs` header) to say triage now serves questions and chat and the investigation pipeline |
| `domovoi-engine` `src/operations/investigate/` | only what the removal forces (for example a removed `Shared` field it used); no behaviour change |
| `domovoi-engine` `src/triage/followup.rs` | the `PROPOSALS:` protocol removed; follow-up answers are plain text (section 4.1) |
| `domovoi-hub` | `triage_build` and `runtime/mod.rs` as in 3.1; `settings::triage` drops `queue`/`run_queue`; `retry_triage` always uses the operation path, with 503 `"investigations are not enabled; set [orchestration] enabled = true"` when `deps.operations` is `None`; a `tracing::warn!` at start when `triage.enabled && !orchestration.enabled` ("triage is on but orchestration is off: incidents are not investigated automatically") |
| `domovoi-config` | `OrchestrationSection` default `enabled = true`; doc comments of `[orchestration] enabled` and `[triage] enabled` updated (6.1); regenerate the configuration reference |
| Tests (engine, hub) | port, rename and re-freeze as in section 12; add the new tests |
| docs | `ARCHITECTURE.md`, book pages, configuration reference (section 15) |

### 4.1 Follow-up answers stop proposing

Today `triage/followup.rs` turns a `PROPOSALS:` line at the end of a follow-up answer into legacy
`Proposed` actions through `proposals::store` (about `:270-332`, verify) and publishes `ActionChanged`.
That path is deleted with `proposals::{create, store}`. A follow-up answer becomes plain text: a
conversation proposes work only by starting or attaching to an operation (RW-20), and the operator gets
new proposals by investigating again (`retry_triage`).

- In `followup.rs` delete `PROPOSALS_MARK`, `MAX_CHAT_PROPOSALS`, `PROPOSAL_LINE_MAX_CHARS` (if unused),
  `split_proposals`, `parse_proposals`, the proposal handling and the `Proposed:`/`Not proposed:` line
  building in `answer`, the `ActionChanged` publish and the use of `proposals::store`. The answer is the
  model's text, redacted and clipped to `MAX_ANSWER_CHARS`, recorded as before. Delete the unit tests of
  the removed functions; keep `clip`, `remembered` and their tests.
- `FOLLOW_UP_PROMPT` loses the sentences about ending with a `PROPOSALS:` line and says: "You cannot run,
  approve or propose anything. If an action would help, describe it in words; the operator decides what to
  do."
- A follow-up answer that still contains a `PROPOSALS:` line is shown as ordinary text and creates
  nothing.
- Tool offering is not changed here; prompt 31 replaces the follow-up tool list with its read-only
  meta-tool set and finds this part already done.

Tests: `follow_up_answer_never_creates_an_action` (engine services: a scripted answer ending in a
`PROPOSALS: [...]` line returns that text, creates no `actions` row and publishes no `ActionChanged`), and
rewrite the seven `a_follow_up_answer_with_proposals_*`/`a_chat_*` proposal tests in
`tests/services/conversation.rs` to assert that. `no_code_path_creates_a_legacy_action` covers
`followup.rs`.

Not changed: `crates/domovoi-engine/src/actions/**`, `xtask/**` (no rule change; no ADR), `domovoi-store`, `domovoi-proto`, `domovoi-agent`, `web/` sources, migrations.

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| Removing the legacy action executor, the action writer's dispatch path, `actions/recheck.rs`, `actions/verify_wake.rs`, `ActionHandle::propose`, the legacy approve/deny endpoints and their tests | prompt 60 (`remove-legacy-executor`, 5.1) |
| Collapsing the second dispatch site and the EXEC-2 arch cards back to one site | same prompt: the cards collapse only when `actions/execute.rs` goes |
| Expiring legacy `Proposed`/`Approved` rows with an audited reason | same prompt (RW-32 ties it to the executor's removal); see 5.1 for why it is not needed earlier |
| Chat approve and deny of steps | prompt 59 (merged before this prompt) |
| Cancel, pause, resume and answer routes | prompt 29 |
| Screens, SSE, use-case layer, meta-tools, layered context | 30, 28, 29, 31, 32 |
| Any migration, wire change or new config key | none needed |

### 5.1 Why the legacy executor stays, and the hand-over list

Decided from `05 §1`, `§2`, `§7`, `RW-32`, `RW-36` and the code:

1. The second parity checkpoint of `05 §7` ("all ADR 0014/0019 tests and action gates pass on the new path") is **not** this prompt's launch gate, and nothing proves it yet. Removing the executor without it would drop tests that protect dispatch safety.
2. The legacy path must still finish rows that are already `Approved`, `Dispatched` or verifying at the upgrade (RW-36: they finish under their `ActionId`). The reconcile and verification handlers for those rows live in `actions/` (`reconcile/`, `verify_wake.rs`, `recheck.rs`). Deleting them would orphan in-flight runs.
3. After this prompt nothing creates a legacy action (test `no_code_path_creates_a_legacy_action`), and every pre-existing `Proposed` row already leaves by itself: `PROPOSAL_TTL` was one hour and `actions/expire.rs` settles an undecided `Proposed` row as `Expired` and an `Approved` row whose window closed as `NotRun`. The audited reason "legacy action expired at upgrade" (RW-32) is therefore only needed by the change that deletes the sweep.

**Hand-over list for prompt 60 (`remove-legacy-executor`, which depends on this prompt):** (a) prove the ADR 0014/0019 behaviours and the action gates on the operation path (the second checkpoint), as named tests; (b) retire `crates/domovoi-engine/src/actions/**` including `recheck.rs` and the second dispatch site; collapse the EXEC-2 cards in `xtask/src/arch/rules.rs` to the single site in `operations/execute.rs` (with an ADR, because the rules change), and make `AuthorizedStep` the only authorized value; (c) in the same release expire remaining `Proposed`/`Approved` rows with the audited reason "legacy action expired at upgrade" and keep a terminal path for `Dispatched`/verifying rows (finish them as `OutcomeUnknown` with an audited reason, or keep their reconcile); (d) remove the legacy approve/deny endpoints and the `legacy` label once `actions` is empty or archived; (e) retire `ActionHandle::propose` and only the legacy-action chat-token pieces (`actions.chat_token_hash` lookups, `ActionHandle::rotate_chat_token`); everything prompt 59 uses for steps stays (60 §5.1 is authoritative).

## 6. Required interfaces and contracts

### 6.1 Configuration **fixed**

`HubConfig::default().orchestration.enabled == true`; a `hub.toml` without `[orchestration]` enables it; `enabled = false` disables it. Doc comment of `[orchestration] enabled`: "Investigate new incidents as operations and approve proposed actions per step on the Operations page. Default true. Set to false to switch off automatic investigation (alert cards still go out) and the operation service; follow-up questions, chat and any old pending actions keep working. Read once at start; changing it needs a restart." `[triage] enabled` (default still `false`) gains: "Automatic investigation also needs `[orchestration] enabled`." Run `cargo xtask docs-config`.

### 6.2 Engine **fixed**

```rust
// crates/domovoi-engine/src/triage/mod.rs
pub struct TriageSettings {            // `queue` and `run_queue` are removed; every other field unchanged
    pub concurrency: u8, pub per_hour_runs: u32, pub reserved_for_incidents: u32,
    pub monthly_budget_micros: Option<u64>, pub cache_days: u32, pub max_prompt_bytes: usize, pub budget: TriageBudget,
}
pub struct TriageDeps { /* every field of today's struct except `actions` and `actions_service` */ }
impl TriageService { pub fn spawn(deps: TriageDeps, settings: TriageSettings) -> (TriageHandle, JoinHandle<()>); }   // unchanged signature
impl TriageHandle { /* follow_up, chat, history only */ }                                                         // + pub(crate) shared() from prompt 27
```

### 6.3 Hub **fixed**

`retry_triage` (`POST /api/v1/incidents/{id}/triage`): unchanged request and response shapes; with `deps.operations == None` it answers 503 with the text of section 4 (an `ApiError::Unavailable`); the OpenAPI response list keeps `503 "Triage is not enabled"` and gains the new meaning in its description. If the annotation text changes, run `cargo xtask openapi && cd web && npm run gen:api`.

## 7. Ordered implementation steps

1. **Inspect and verify; report discrepancies before editing.**
   - Open every file and symbol of section 3. Confirm 26, 27 and 28 are merged and name their real symbols: `OperationHandle::investigate_and_wait`, `investigate::register`, `TriageSettings::run_queue`, `prepare`/`generate`, `Mode` in the test harnesses.
   - **Run the parity suite on the unchanged base and record the result:** `cargo nextest run -p domovoi-engine --test services parity` and `cargo nextest run -p domovoi-engine --test drills`. If anything is red, stop and report (this is the launch gate).
   - `grep -rn "recheck" crates/domovoi-engine/src` and `grep -rn "mint_token\|token_hash\|PROPOSER\|DEFAULT_QUEUE\|\.queued()" crates` and list every user of each item to remove.
   - Confirm prompt 59 is merged: an Execution step's chat card has approve and deny buttons. If not, stop.
   - Run `cargo xtask arch-check` and the engine and hub suites for the baseline.
2. Create branch `remove-legacy-triage-path` from the latest `overhaul-investigation`.
3. **Freeze the expectations before deleting anything.** Add `crates/domovoi-engine/tests/services/frozen.rs`: for each of the six drills, run the **legacy** mode once more and write with `insta` the fields `headline`, `summary`, `cause`, `evidence`, `verdict`, `next_step`, `manual_steps`, the proposals (skill and args), the score total and the number of model requests, to `tests/services/snapshots/frozen__<drill>.snap`; do the same for the parity scenarios that compare numbers or texts (skipped texts, the joined failure reason, the number of granted tokens and requests at the tool-call and token budgets, the slot time of a capped run). Then assert the **operation** mode equals each snapshot. **Read every snapshot** before accepting it. Commit `test(engine): freeze the triage results the operation path must keep`.
4. **Remove** the triage queue and its helpers, the legacy composition and the legacy proposals (3.1). After each removal let the compiler list the breakage, then fix the tests as in step 5. Do not touch `actions/`.
5. **Port the tests** (section 12.2): delete `Mode` and the legacy arm from both harnesses; keep the parity file's assertions on the operation path under behaviour names (drop "across modes"); classify every test that used `w.triage.triage/retriage` as either (a) a triage behaviour test → run it through `World::run()` (operation path), or (b) an action-pipeline test that only used triage to create a `Proposed` action → seed the action directly with the existing helper of `tests/services/actions_harness.rs` (`propose`) and keep its expectations. **No test is deleted without its behaviour being asserted somewhere**; list each moved, ported or deleted test in the PR description.
6. **Hub**: `triage_build`, `runtime/mod.rs`, `settings::triage`, `retry_triage`, the start-up warning; update `examples/dev_hub.rs` and `tests/hub/api_harness.rs` if they build `TriageDeps` or `ApiDeps`.
7. **Config**: flip the default, doc comments, validation unchanged; `cargo xtask docs-config`. Update the 26 tests named in 3.4 (`orchestration_is_enabled_by_default`; runtime tests that exercised the legacy path now set `orchestration.enabled = false` explicitly and keep their expectations).
8. **New tests** (12.1) and gates (12.3).
9. **Docs** (section 15).
10. **Checks and the completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-engine -p domovoi-hub -p domovoi-config --all-targets -- -D warnings`; `cargo nextest run -p domovoi-engine -p domovoi-hub -p domovoi-config`; `cargo nextest run -E 'rdeps(domovoi-engine)'` (public items were removed); `cargo xtask arch-check`; `cargo xtask docs-config --check`; `cargo xtask docs-links`. If `web/src/api` was regenerated: `cd web && npm run lint && npm run typecheck`.

Commit in small green steps (one-line Conventional Commits, one scope each): after steps 3, 4 and 5 together (the tree must compile and pass), 6 and 7, 9.

## 8. Database and persistence

No migration and no schema change (`ls crates/domovoi-store/migrations/` identical before and after). In-memory state removed: the queue and the deferred list; the durable equivalents (operations, effects) came with prompt 27. Existing `actions` rows, `effects` rows and operations stay as they are. **N-1 upgrade test: not applicable**; the existing upgrade tests must still pass.

## 9. Protocol and compatibility

No wire change. One HTTP behaviour change: `POST /api/v1/incidents/{id}/triage` answers 503 when orchestration is off (before, it ran a direct triage). No DTO change. `TriageCompleted.tokens` stays a field and is empty for new outcomes.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Hub restarts with investigation effects pending | the operation runner continues them (prompt 27); nothing else starts triage |
| Upgrade from a hub whose queue held waiting incidents | those in-memory requests are lost, as on any restart; the investigator's start-up catch-up (27) creates operations for open incidents that have no triage entry |
| `orchestration.enabled = false` and `triage.enabled = true` | start-up warning; `TriageRequested` ignored; the card goes out untriaged; questions and chat answer; `retry_triage` answers 503 |
| `orchestration.enabled = true`, `triage.enabled = false` | operation service runs (for later use), no investigation starts, as before for triage off |
| A legacy `Proposed` action exists after the upgrade | still approvable on the legacy endpoints for its remaining window; expires through the existing sweep; shown with `legacy: true` |
| A legacy `Approved` or `Dispatched` action at the upgrade | finishes on the legacy executor under its `ActionId` (unchanged code) |
| Someone expects chat buttons on a new proposal | they exist (prompt 59); nothing changes here |

## 11. Security requirements

- Removing code must not remove a protection: every gate that the deleted tests carried is either still asserted on the operation path (12.2) or moved to `gates.rs` (12.3). **Never weaken a gate.** If a gate's subject disappears (for example "triage creates only `Proposed` actions"), replace it with the equivalent statement on steps: "no step is authorized by anything but the approve route", "no proposal is dispatched without a human".
- INV-1: no `HumanApproval::new` outside `crates/domovoi-hub/src/auth/`; the step approve route of prompt 27 stays the only authorizer of steps; the legacy approve route stays the only authorizer of legacy actions.
- INV-9: report text never executes: the `nothing_executed`, `nothing_approved` and `only_read_tools` probes keep running on every drill.
- The flag's default flip must not enable unattended execution: no policy permit exists; every execution still needs a human (prompt 26).
- Secrets: unchanged; the frozen snapshots contain only redacted drill output (the drills' secret must not appear in any `.snap`: a test greps the snapshot directory).

## 12. Testing requirements

### 12.1 New tests

| Test | Where | Setup, action, expected |
|---|---|---|
| `orchestration_is_enabled_by_default` | config tests | `HubConfig::default().orchestration.enabled`, an empty `hub.toml`, and `[orchestration]` with `enabled = false` |
| `incident_is_investigated_by_default` | hub runtime tests | default config with triage enabled and scripted primary: a confirmed incident gets one operation and a `triage.report` timeline entry |
| `orchestration_off_runs_no_automatic_triage` | hub runtime tests | `enabled = false`: `TriageRequested` creates no operation, the card is the untriaged card, `follow_up` and `chat` still answer, `retry_triage` is 503 with the text of section 4 |
| `triage_service_without_a_queue_answers_questions_and_chat` | engine services | the existing follow-up and chat scenarios pass through `TriageService` built without any queue |
| `follow_up_answer_never_creates_an_action` | engine services | a follow-up answer ending in a `PROPOSALS: [...]` line is returned as text; no `actions` row, no step, no `ActionChanged` |
| `no_code_path_creates_a_legacy_action` | `crates/domovoi-engine/tests/gates.rs` | a source scan of `crates/domovoi-engine/src` and `crates/domovoi-hub/src` finds no caller of `ActionHandle::propose`, no `ActionRequest::propose` and no `actions_service` outside `crates/domovoi-engine/src/actions/` (the definition) |
| `triage_queue_and_in_memory_retries_are_gone` | engine gates | no `queue.rs` in `src/triage/`; no `RetryReason`, `retry_deferred`, `deferred_ready`, `DEFAULT_QUEUE` in `src/`; `TriageHandle` has no `triage`, `retriage` or `queued` (scan the `impl` block) |
| `legacy_proposed_action_expires_through_the_existing_sweep` | engine services | seed a legacy `Proposed` action with a one-hour window, advance `FakeClock` past it, run the sweep: status `Expired`, audited; proves 5.1 point 3 |
| `legacy_approved_action_still_dispatches_with_orchestration_on` | engine services or hub | an `Approved` legacy action under the default config dispatches once under its `ActionId` and never appears in `executions` |
| `frozen_results_match_the_operation_path` | `tests/services/frozen.rs` | per 7 step 3, one test per drill and per scenario |
| `frozen_snapshots_contain_no_secret` | same | grep the snapshot directory for each drill's secret: none |

### 12.2 Ported and renamed tests

- `tests/drills/drills.rs`: `drills_pass_with_scripted_models` runs the six drills on the operation path only; thresholds (80) and cases unchanged; `drills_live` unchanged.
- `tests/services/parity.rs`: each test becomes a plain behaviour test of the operation path (`tool_call_budget_is_enforced`, `fallback_chain_behaves…` and so on, names without "across modes" or "same"), its comparisons replaced by assertions against the frozen snapshots of step 3. The file may be renamed `investigation_limits.rs`.
- `tests/services/{limits,attach,capability,chat,actions,...}.rs`: per step 5; the expectations of each test (reasons, counts, advisories, timeline kinds) are unchanged.
- Gates: in `tests/gates.rs` of the engine and hub, every probe that built a card from `actions` keeps its assertion on a seeded legacy action; every probe about triage proposals is restated on steps (an Execution step is `Pending` with no approval after an investigation; no skill request reached `FakeAgentLink`).

### 12.3 Gates

Add the two engine gates of 12.1. Extend, never weaken, the existing gates. `cargo nextest run -p domovoi-engine -p domovoi-hub` must pass without `#[ignore]` added to anything.

## 13. Acceptance criteria

1. `HubConfig::default().orchestration.enabled` is `true`; `enabled = false` disables the operation service and automatic investigation; the configuration reference is regenerated.
2. `crates/domovoi-engine/src/triage/queue.rs` is deleted; the queue, deferred retries, `RetryReason`, `TriageHandle::{triage, retriage, queued}`, legacy proposal creation, `TriageDeps::{actions, actions_service}` and `TriageSettings::{queue, run_queue}` no longer exist; `grep` finds no remaining use.
3. `TriageHandle::{follow_up, chat, history}` behave exactly as before (existing tests pass).
4. With the default configuration and triage enabled, a confirmed incident is investigated by an operation; every proposal is an Execution step; no `actions` row is created by anything but the legacy tests' seeding.
5. `retry_triage` runs an investigation (or answers 409/503 as specified by prompt 27 and section 4).
6. The six drills pass on the operation path with the same scores as the frozen legacy snapshots, and every frozen snapshot equality holds.
7. Every test that exercised the removed entry points is ported or its behaviour is asserted elsewhere; the PR lists each moved, ported or deleted test; no gate is weakened and no test is `#[ignore]`d.
8. `actions/recheck.rs`, the rest of `actions/`, the EXEC-2 cards and the legacy endpoints are untouched (diff proves it); the PR states that the executor removal and the audited legacy expiry are handed to a later change and quotes the list of 5.1.
9. The step 1 parity run result is recorded in the PR description.
10. No migration, wire, proto or agent change; `cargo xtask arch-check`, clippy, the engine/hub/config tests and `cargo xtask docs-config --check` pass.

## 14. Migration and compatibility

- **Existing installs.** After upgrading, a hub with no `[orchestration]` section starts the operation service and investigates new incidents as operations (when triage is enabled). To keep automatic triage off, set `[orchestration] enabled = false`. There is no longer a queue-based automatic triage to fall back to.
- **Legacy actions.** Retained, listed read-only with the **Legacy** label, finishing under their original ids; nothing is converted or expired by this change.
- **Chat approvals.** Cards of new proposals carry approve and deny buttons from prompt 59; legacy actions' buttons keep working while they last.
- **Agents, wire, database.** Unchanged.
- **Rollback.** Run the previous binary: the schema is unchanged and the previous binary's queue resumes; operations created meanwhile stay in the store and are ignored by it.

## 15. Documentation requirements

- `ARCHITECTURE.md` §7.1: investigations are the only automatic triage path; the queue is gone; the meaning of `orchestration.enabled = false`; §12: the legacy executor and the second dispatch site remain until a later change (do not edit the arch cards).
- Book: `docs/book/src/concepts/operations.md` (what is on by default, how to switch it off, that old pending actions finish as before and are labelled Legacy), `docs/book/src/concepts/incidents.md` (triage runs as an investigation), `docs/book/src/concepts/actions.md` (legacy actions only drain; new proposals are approved per step on the web, not from chat), the troubleshooting page (an untriaged card with orchestration off). Run `cargo xtask docs-links`. Add an upgrade note where the book lists release changes, if such a page exists.
- Config reference: `cargo xtask docs-config`. OpenAPI: regenerate only if an annotation changed.
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
