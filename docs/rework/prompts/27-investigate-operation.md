# 27 — Triage as an Investigate operation, operation endpoints and step approvals

| | |
|---|---|
| Branch | `investigate-operation` |
| Milestone | M3 |
| Depends on (must be merged) | 26 and 28 (28 runs first; and everything 26 depends on: 22, 23, 24, 25). Prompts 24, 25, 26 and 28 are written but may not be merged when this brief is read: **every name taken from them below is "introduced by prompt NN — verify in code before use"** |
| Can run in parallel with | 29 (order-flexible, never concurrent). 28 must be merged first because both edit `crates/domovoi-hub/src/api/mod.rs`, `ApiDeps` and `web/src/api/*`; 27 builds on 28's `ApiDeps.journal` and `snapshot_seq` |
| Produces (shared contracts) | HTTP: `GET /api/v1/operations`, `GET /api/v1/operations/{id}`, `GET /api/v1/operations/{id}/trace`, `GET /api/v1/operations/{id}/steps/{step}`, `POST /api/v1/operations/{id}/steps/{step}/approve`, `POST /api/v1/operations/{id}/steps/{step}/deny`; DTOs `OperationPage`, `OperationSummaryDto`, `OperationDetailDto`, `OperationTraceDto`, `StepDetailDto`, `ExecutionDto`, `ApproveStepRequest`; `ActionDto.legacy`; `IncidentDetail.operations`. Engine: `OperationHandle::investigate`, `OperationHandle::investigate_and_wait`, the four effect handlers `BuildContext`, `CallModel`, `DraftSummary`, `Notify`, `OperationDeps::investigate`, `TriageSettings::run_queue`. Test: the parity suite `crates/domovoi-engine/tests/services/parity.rs` (the launch gate of prompt 58) |
| Consumes | prompt 28's `snapshot_seq(&ApiState)` (`crates/domovoi-hub/src/api/snapshot.rs`), `ApiDeps.journal: Arc<dyn JournalFeed>` (28 removes `ApiDeps.bus` and adds `journal`), its SSE topics `operations` and `operation:<id>` and its web query keys `keys.operations` / `keys.operation(id)`; the whole operation aggregate of 26 (`OperationHandle`, `OperationRepo`, `decide`, the effect runner, the trace types, `ExecutionRecord`); the existing triage service and its admission gate (17), the structured approvals and `plan_binding` of 18 and 22, `web_authorization` in hub auth |
| Launch gate | none beyond the dependencies |
| Suggested model | large: it moves the primary model-backed path of the product onto the new aggregate behind a flag and adds the only approval route for steps |

## 1. Objective

With `[orchestration] enabled = true` (and `[triage] enabled = true`), a new incident is investigated by an
**Investigate operation** instead of the in-memory triage queue:

- the same report (`TriageReport`), the same limits (hourly runs, monthly budget, per-run tool calls,
  seconds and output tokens), the same provider fallback chain, the same host scoping of tools and the
  same card on the incident, produced by the **existing triage code**, wrapped in four effect handlers
  (`BuildContext`, `CallModel`, `DraftSummary`, `Notify`);
- every model proposal becomes an **Execution step** (an approval-gated, plan-hash-v2 bound proposal),
  never a legacy `actions` row;
- the work survives a restart: a deferred or interrupted investigation continues from the store, not from
  memory;
- a person approves or denies a step through one new human-only endpoint, bound to the stored plan hash
  v2;
- the browser can read operations, their trace (26's frozen schema, now published through OpenAPI), one
  step, and the read-only **Legacy** projection of old actions.

With `enabled = false` (still the default in this prompt) **nothing changes**: the triage queue and the
legacy action path behave exactly as before, and no new endpoint returns data. The **parity suite**
proves, scenario by scenario, that both modes give the same results; the six scripted triage drills pass
unchanged under both modes. This prompt does **not** remove the triage queue or flip the default (58),
does not add meta-tools or a plan validator (31), layered context (32), SSE (28), the use-case layer (29)
or any screen (30).

## 2. Architectural context

Everything needed is here; the cited documents are background.

**RW-04 / RW-05 (operation, steps).** An operation is a durable aggregate of immutable steps joined by
typed edges, written only by the one operation writer (prompt 26). A step's intent never changes; its
progress is a derived projection. An Execution is a step of kind `Execution` plus one `executions` row
whose id is the step id (RW-36). This prompt only **uses** the writer through `OperationHandle`; it adds no
write path of its own and no migration.

**RW-06 (model output).** Model calls are read-only but not idempotent: a crash before the result is
committed re-issues the call as a new attempt; only a *committed* result is acted on. Each `ReasoningTurn`
persists its usage; a failed call stores no fabricated tokens or cost.

**RW-11 (approval stages, M3).** Approval moves to `POST /operations/{id}/steps/{step}/approve` with the
v2 plan hash shown to the person. The action endpoint stays only for legacy actions. `HumanApproval::new`
is called only in `crates/domovoi-hub/src/auth/`; the endpoint builds its `Authorization` with the existing
`web_authorization` helper and hands it to `OperationHandle::authorize`, which refuses a policy
authorization, a plan hash that is not the stored one, an expired window and a second approval (prompt 26).
No chat command, model tool or background task approves anything.

**RW-32 (legacy actions).** Existing `actions` rows are never converted. They stay in `actions`, finish on
the legacy executor under their `ActionId`, and appear through a read-only projection labelled **Legacy**.
Nothing is synthesized: no steps, events or approvals are invented for them.

**RW-21 (trace).** The timeline and graph render from the trace frozen in prompt 26 (golden fixtures in
`web/src/test/fixtures/operation-trace/`). This prompt is the first to publish it over HTTP; the JSON that
the endpoint returns must equal the fixture shape field for field.

**05 §7 (parity checkpoint).** Before the queue can be removed: same report fields, budgets, fallback and
host scoping; the existing scripted triage drills pass unchanged under both flag values. The parity suite of
section 12 is that checkpoint, and it is the launch gate of prompt 58.

**Design of the mapping (decided here; do not revisit).** One cycle of an Investigate operation is a
pipeline of four durable effects, each one a thin wrapper over code that already exists in
`crates/domovoi-engine/src/triage/`:

| Effect | Wraps (existing) | Result committed |
|---|---|---|
| `BuildContext` | the repeat cache (`run.rs:68`), the admission gate (`admission.rs:101`), the context bundle (`context.rs:41`) | a `CallModel` effect carrying the redacted bundle, **or** a finished turn (cached, skipped) |
| `CallModel` | the provider chain, tool loop and report parse (`run.rs:193-345`, `agent.rs`) | the turn step finished with the report (or the failure) and a `DraftSummary` effect |
| `DraftSummary` | the proposal checks (`proposals.rs:40`) and the proposal formatting (`proposals.rs:276-304`) | `Assessment`, `Plan`, `Execution` and `Notification` steps and a `Notify` effect |
| `Notify` | `ManagerHandle::attach_triage` and `record_tool_use` (`run.rs:42-50`, `run.rs:148-155`), the report cache write (`run.rs:126`) | the `Notification` step succeeded; the investigation closed when nothing is left |

The whole tool loop of a triage run (up to 15 tool calls) stays **inside one `CallModel` effect and one
`ReasoningTurn` step**: closed `ReadTool` calls are not `EvidenceRequest` steps in this prompt (meta-tools
and per-call steps are prompt 31). The budgets of the run (`RunBudget`: tool calls, seconds, output tokens,
the cumulative allowance) are enforced inside that call exactly as today.

## 3. Repository context

Verify each item first. Marks: **(existing, path:line)** or **(new)**.

### 3.1 The triage service today

- `TriageService::spawn(deps, settings) -> (TriageHandle, JoinHandle<()>)` builds `Shared` and spawns
  `queue::serve` **(existing, `crates/domovoi-engine/src/triage/mod.rs:266-284`)**. `Shared { deps, settings, limits, questions, queue, deferred, deferred_ready }` is `pub(crate)` **(`mod.rs:210-221`)**; `TriageHandle { shared }` has a private field **(`mod.rs:248-252`)**. `TriageHandle::{triage, retriage, follow_up, chat, history, queued}` **(`mod.rs:286-349`)**.
- `queue::serve` feeds `TriageRequested` events (`event.kind == EventKind::TriageRequested && event.notify`) into a bounded in-memory `Queue`, runs `settings.concurrency` workers and `retry_deferred`, and calls `reload_at_start` **(existing, `queue.rs:69-100, 336-351`)**. Deferred retries live in `Shared::deferred` (memory) **(`mod.rs:219-236`, `queue.rs:107-193`)**.
- One run: `run::triage(shared, incident, fresh)` → `outcome(..)` → `manager.attach_triage(incident, outcome, tokens)` **(existing, `run.rs:40-157`)**. Order inside `outcome`: (1) repeat cache unless `fresh` (`:66-79`); (2) `admit(shared, Purpose::Triage)` with the refusal texts and `shared.defer(.., RetryReason::Cap)` (`:80-111`); (3) `catalog.snapshot(host)` and `context::build` (`:112-117`); (4) `RunBudget::for_run` and `providers(..)` (`:118-121`); (5) on success `redact_report`, `proposals::create`, `cache.put`, `succeeded(shared)` (`:122-135`); on failure `failed(shared, ..)` and `defer(.., RetryReason::Failed)` unless degraded (`:136-146`); (6) `record_tool_use` (`:148-155`).
- Admission: `admit`, `free_slots`, `next_slot`, `slot_after`, `ADMISSION_RETRY`, `Refusal` are `pub(crate)` inside the **private** module `admission` **(existing, `admission.rs:16-177`)**. `Purpose::{Triage, Question}`.
- Proposals: `proposals::create(shared, incident, snapshot, skills, &mut report)` stores `Proposed` legacy actions through `shared.deps.actions_service.propose` and returns chat tokens **(existing, `proposals.rs:276-304`)**; `check(proposal, incident, skills)` validates skill, `applies_to` and args **(`:40-56`)**; the "Proposals not offered:" formatting is in `create` **(`:292-302`)**.
- Retry limits: `FAILED_RETRY = 1 h`, `FAILED_RETRY_LIMIT = 3`, `DEGRADED_AFTER = 3`, `PROPOSAL_TTL = 1 h` **(existing, `mod.rs:60-71`)**; `failed()` and `succeeded()` keep the failure streak and the `TriageDegraded` advisory in `Shared::limits` **(`run.rs:347-384`)**.
- Usage: each model call records a usage row in `UsageRepo` inside `complete` **(existing, `complete.rs:19-144`)**; the per-call `Usage { input_tokens, output_tokens, cost_micros, source }` is available there but not accumulated per run.
- The hub spawns triage after the action service, in `runtime/triage_build.rs:93-117`; `settings::triage(config)` builds `TriageSettings` **(existing, `crates/domovoi-hub/src/runtime/settings.rs:37-50`)**. `config.triage.enabled` defaults to `false`; when false the hub holds no `TriageHandle` **(`triage_build.rs:55-57`)**.
- The incident manager stores a triage outcome as a timeline entry and publishes `TriageCompleted`, which the notifier turns into the card **(existing, `manager/annotate.rs:65-103`)**. The card shows approve buttons only for legacy `ActionRequest`s with a chat token **(`cards.rs:123-156`)**.

### 3.2 What prompt 26 provides (introduced by prompt 26 — verify every name in code)

`OperationHandle` methods `create(OperationSeed)`, `propose(op, steps, edges)`, `propose_execution(op, ExecutionRequest)`,
`authorize(op, step, Authorization, PlanBinding, ApprovalProvenance)`, `deny(op, step, HumanApproval)`, `step_finished`, `finish`, `cancel`;
crate-private handler commands `StartStep`, `FinishStep`, `Defer`, `FinishEffect`, `Settle`; `OperationService::spawn(OperationDeps, OperationSettings)`
which builds the operation `EffectRunner` (scoped to `AggregateKind::Operation`) and registers the handlers for `CollectEvidence`, `Dispatch`, `Wake`,
`Verify`, `Reconcile`, **and leaves `BuildContext`, `CallModel`, `DraftSummary`, `Notify` without a handler**; `OperationRepo::{get, load, list, for_incident, execution, with_status, ..}`;
`OperationSnapshot`, `StepRecord`, `StepKind`, `StepOutcome`, `StepIntent`, `ExecutionIntent`, `OperationSeed`, `NewStep`, `NewEdge`, `OperationTrace` and `build_trace`; the
`[orchestration]` config section; `Actor::System(SYSTEM_OPERATIONS)`; journal and audit kinds. A `Verified{passed:false}` creates a `BuildContext` effect
`{"v":1,"operation":..,"cycle":N}` (`dedup_key build:<operation>:<N>`). `OperationDeps` is built in `crates/domovoi-hub/src/runtime/orchestration.rs` **only when `config.orchestration.enabled`**.

Check in step 1 whether `Defer` can create a successor of **any** effect kind with a given `not_before` and `dedup_key`. If it only creates `Wake`/`Dispatch`/`Reconcile` successors,
generalise it (an `EffectSpec` parameter) as the one permitted edit of 26's writer in this prompt, and say so in the report.

### 3.3 Hub and web today

- `ApiDeps` **(existing, `crates/domovoi-hub/src/api/mod.rs:135-196`)**; routes are registered in `routes()` **(`:305-365`)** with `utoipa` annotations; the tag list is in `ApiDoc` **(`:281-292`)**. Constructors of `ApiDeps`: `runtime/mod.rs:306`, `examples/dev_hub.rs:501`, `tests/hub/api_harness.rs:413`.
- The legacy approve/deny handlers: `api/actions.rs:118-188`; the error mapping `approval_error` **(`:19-46`)**; `web_authorization(user, confirm_text, skill, session_hash, now) -> (Authorization, ApprovalProvenance)` and `web_approval(..) -> HumanApproval` **(existing, `crates/domovoi-hub/src/auth/approval.rs:13-62`)**. A destructive skill is approved only with `confirm_text` equal to its name (`approval.rs:13-22`, INV-6).
- `retry_triage` runs `triage.retriage/triage(&incident)` in a spawned task and returns `TriageStateDto` **(existing, `api/incidents.rs:470-491`)**; `IncidentDetail { incident, host, report, triage, card, timeline, actions, tools_used }` **(`api/dto.rs:247-264`)**; `ActionDto` **(`dto.rs:285-319`)** and `impl From<&ActionRequest>` **(`:321-356`)**.
- `as_of_seq`: **introduced by prompt 28 — verify in code**: `pub(crate) async fn snapshot_seq(state: &ApiState) -> Result<u64, ApiError>` in `crates/domovoi-hub/src/api/snapshot.rs` returns the journal head and must be called **before** the data it describes is loaded; `ApiDeps.journal: Arc<dyn JournalFeed>` exists and `ApiDeps.bus` is gone. Operation journal rows reach the browser on SSE topics `operations` and `operation:<id>`; that mapping (`topics_of`) belongs to 28 and is not redefined here. Web query keys `keys.operations` and `keys.operation(id)` are 28's.
- Web: `web/src/api/operationTrace.ts` and the five fixtures in `web/src/test/fixtures/operation-trace/` (introduced by prompt 26); `web/src/api/openapi.json` and `schema.d.ts` are generated (`cargo xtask openapi && cd web && npm run gen:api`), never hand-edited.

### 3.3a Tests today

- Drills: `crates/domovoi-engine/tests/drills/{drills,world,score,probes,format}.rs` (declared `[[test]] name = "drills"`, `Cargo.toml:46-48`); `world::run(drill, model) -> Outcome` spawns the manager, the action service and `TriageService`, then calls `triage.triage(&incident)` and reads `ActionRepo::list` for scoring **(existing, `world.rs:340-364`)**; `score::score(key, outcome, actions: &[ActionRequest])`; `probes::all(&Outcome, secret)`; six cases in `tests/drills/cases/*.yaml` (`ac_unplugged_check_by_hand`, `container_down_with_disk_decoy`, `disk_full_only_destructive_fits`, `log_injection`, `pcie_correctable_expected`, `unit_failed_no_skill`). Test `drills_pass_with_scripted_models` **(`drills.rs`)**.
- Services: `tests/services/harness.rs` builds a `World` (store, `FakeClock`, `FakeAgentLink`, scripted primary/secondary/third providers, manager, `TriageService`) in `build(setup)`; `World::run()` calls `self.triage.triage(&self.incident)` **(`harness.rs:379`)**; scenarios live in `limits.rs` (hourly and monthly caps, cache, deferral), `turns.rs`, `report.rs`, `reliability.rs`, `evidence.rs`, `capability.rs`, `effects_triage.rs`.

## 4. Exact scope

### 4.1 `domovoi-engine`: triage visibility and two seams (narrowest possible edits)

| File | Change |
|---|---|
| `src/triage/mod.rs` | modules `admission`, `cache`, `context`, `proposals`, `report`, `run` become `pub(crate) mod`; add `pub(crate) fn shared(&self) -> Arc<Shared>` on `TriageHandle`; `TriageSettings` gains `pub run_queue: bool` (default `true`; documented: false means the queue and its deferred retries are not started because investigations do the work); `TriageService::spawn` starts `queue::serve` only when `run_queue` is true, otherwise it spawns a task that waits for `deps.shutdown` (the returned `JoinHandle` contract is unchanged) |
| `src/triage/run.rs` | split `outcome` into three `pub(crate)` pieces used by both paths, without changing behaviour: `prepare(shared, incident, fresh) -> Prepared` (cache + admission, `Prepared::{Cached{report, from}, Refused(Refusal), Admitted}`), `generate(shared, incident, skills, bundle) -> Generated { result: Result<TriageReport, String>, tool_use: Vec<ToolUse>, usage: RunUsage }` (steps 4 and 5-redaction of the old flow), and the existing `failed`/`succeeded` made `pub(crate)`. `outcome()` is rewritten as `prepare` + `context::build` + `generate` + `proposals::create` + the old tail, so the legacy path runs the same code. `refusal_text(shared, &Refusal) -> (String, Option<Timestamp>)` is extracted from `:80-111` (text and the optional retry time) |
| `src/triage/agent.rs`, `complete.rs` | `RunBudget` gains an accumulator `usage: RunUsage { input_tokens: u64, output_tokens: u64, cost_micros: Option<u64>, any_unpriced: bool }` updated from each successful `complete` response; `RunBudget::usage() -> RunUsage`. No change to what is granted or refused |
| `src/triage/proposals.rs` | `check` becomes `pub(crate)`; extract `pub(crate) fn append_dropped(report: &mut TriageReport, dropped: Vec<String>)` from `create` (`:292-302`) and call it from `create` |
| `src/triage/queue.rs` | unchanged in this prompt (removed by 58) |

### 4.2 `domovoi-engine`: the investigation module (all new)

`src/operations/investigate/` (each file at most 500 lines):

| File | Contents |
|---|---|
| `mod.rs` | module doc; `InvestigateDeps { triage: TriageHandle }`; `InvestigateSettings`; `register(runner, deps)` that registers the four handlers; re-exports |
| `start.rs` | `impl OperationHandle { investigate, investigate_and_wait }`; `seed_for(incident, fresh, now)`; the `Investigator` task that turns `TriageRequested` events into operations, with its start-up catch-up |
| `turn_output.rs` | `TurnOutput` (the JSON stored in the turn step's `output`), `ContextRefs` constants, `report_view(snapshot, turn) -> TriageReport` |
| `build_context.rs` | the `BuildContext` handler |
| `call_model.rs` | the `CallModel` handler |
| `draft_summary.rs` | the `DraftSummary` handler (assessment, plan, execution proposals, notification step, retry scheduling) |
| `notify.rs` | the `Notify` handler (attach, tool-use record, cache write, close attempt) |
| `close.rs` | `close_decision(&OperationSnapshot) -> Option<(FinishOutcome, String)>` (pure) and the closer poll loop |
| `waiters.rs` | `Waiters`: in-memory `OperationId -> oneshot::Sender<TriageOutcome>` used only by `investigate_and_wait` |

Edits to prompt 26's files (minimal): `operations/mod.rs` (`OperationDeps` gains `pub investigate: Option<InvestigateDeps>`; `spawn` calls `investigate::register` and spawns the `Investigator` and closer tasks when it is `Some`, joined by the same `JoinHandle`), `operations/effects/mod.rs` only if the registration needs a hook, and `Defer` (see 3.2). `src/lib.rs` re-exports `InvestigateDeps`.

### 4.3 `domovoi-hub`

| File | Change |
|---|---|
| `src/api/mod.rs` | `ApiDeps` gains `operations: Option<OperationHandle>`, `operation_repo: Arc<dyn OperationRepo>` (28 already added `journal`; add no second journal field); tag `operations`; routes of 4.3a; `pub(crate) mod operations;` |
| `src/api/operations.rs` **(new)** | handlers: `list`, `detail`, `trace`, `step`, `approve_step`, `deny_step`; error mapping |
| `src/api/dto_operations.rs` **(new)** | the DTOs of section 6 and their `From` impls |
| `src/api/dto.rs` | `ActionDto.legacy: bool`; `IncidentDetail.operations: Vec<OperationSummaryDto>` |
| `src/api/incidents.rs` | `detail` fills `operations` and `legacy`; `retry_triage` uses `investigate_and_wait` when `deps.operations` is `Some` (6.6) |
| `src/api/actions.rs`, `src/api/dto.rs` | `legacy` is set on every `ActionDto` built while orchestration is enabled |
| `src/runtime/orchestration.rs`, `src/runtime/mod.rs`, `src/runtime/triage_build.rs`, `src/runtime/settings.rs` | order and wiring of 4.3b |
| `examples/dev_hub.rs`, `tests/hub/api_harness.rs` | the new `ApiDeps` fields |
| `tests/hub/operations_api.rs` **(new)** + `tests/hub/main.rs` | endpoint tests of section 12.4 |

4.3a routes (all behind `Authed`; the two POSTs also pass the existing CSRF guard):

```
GET  /api/v1/operations                              operation_id list_operations
GET  /api/v1/operations/{id}                         get_operation
GET  /api/v1/operations/{id}/trace                   get_operation_trace
GET  /api/v1/operations/{id}/steps/{step}            get_operation_step
POST /api/v1/operations/{id}/steps/{step}/approve    approve_operation_step
POST /api/v1/operations/{id}/steps/{step}/deny       deny_operation_step
```

4.3b wiring. Today `orchestration::spawn` (prompt 26) runs right after `ActionService::spawn`, before the triage service exists. Triage is built later
(`runtime/mod.rs:260-275`). This prompt moves the call, **and the composite reconciler registration (`server.set_reconciler`)**, to just after
`triage_build::build(..)`, so that `OperationDeps.investigate` can be `Some(InvestigateDeps { triage })` when triage is enabled. If triage is disabled (`None`)
the operation service starts without `investigate` and `TriageRequested` is ignored exactly as today. `settings::triage(config)` sets
`run_queue = !config.orchestration.enabled`. `ApiDeps.operations` is `Some` only when the flag is on; `operation_repo` is always the store.

### 4.4 `domovoi-config`, xtask, docs, web

- config: **no new key**. (The keys of `[orchestration]` come from 26; `enabled` stays `false` by default.) Run `cargo xtask docs-config` and confirm it reports no change; update the doc comment of `[orchestration] enabled` and `[triage] concurrency` to say what each now controls (6.8).
- xtask: no rule change (no ADR). Do **not** touch `xtask/src/arch/rules.rs`.
- web: `web/src/api/openapi.json` and `schema.d.ts` regenerated; `web/src/api/operationTrace.ts` becomes type aliases of the generated trace schema (6.5). No page, no component.
- docs: section 15.

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| Removing the triage queue, deferred retries, `TriageHandle::triage/retriage/queued`, the `[triage]` queue plumbing; flipping `orchestration.enabled` to true; legacy expiry | 58 |
| Removing the legacy action executor, `recheck.rs`, `actions/` | a later removal prompt (see 58); legacy actions keep working unchanged |
| Meta-tools, plan validator, `EvidenceRequest` steps per tool call, target resolution of model strings, removing the `PROPOSALS:` text | 31 |
| Layered context, `ContextSource`, evidence dedupe | 32 |
| SSE, `as_of_seq` rule and `snapshot_seq`, topics (`operations`, `operation:<id>`), the `incident_events` copy | 28 (merged first; this prompt only calls `snapshot_seq`) |
| Use-case services, module-edge rule | 29 |
| Screens, approval card component, banners | 30 |
| Cancel, pause, resume and answer-a-question HTTP routes | **unowned** at the time of writing; `OperationHandle` has the methods (26), no route is added here; reported to the orchestrator |
| Chat (Discord) approve and deny buttons for steps | **unowned**; the card shows no step buttons in operation mode (6.7); reported to the orchestrator |
| Policy evaluation, permits, unattended execution | 39, 40, 45 |
| Converting, migrating or expiring legacy `actions` rows | never converted (RW-32); expiry is 58's concern |
| A migration, a wire change, `PROTOCOL_VERSION`, any `domovoi-agent`/`domovoi-proto` file | none needed; forbidden (RW-42) |
| `HumanApproval::new` anywhere new, a model tool, an approve path other than the endpoint of 6.4 | INV-1 |

## 6. Required interfaces and contracts

Marks: **fixed** = names and semantics must match; **convention** = follow repository style. Every public item gets a doc comment that says what it is for.
Never copy a prompt number, "the build plan" or an RW/D id into code, doc comments, commit messages, the book or the PR description.

### 6.1 Engine entry points **fixed**

```rust
// crates/domovoi-engine/src/operations/investigate/mod.rs
pub struct InvestigateDeps { pub triage: TriageHandle }
pub struct InvestigateSettings {
    pub close_interval: SignedDuration,        // 30 s: how often the closer scans open investigations
    pub wait_limit: SignedDuration,            // investigate_and_wait gives up after this: 20 min
    pub catch_up_limit: u16,                   // incidents examined by the start-up catch-up: 200
}

// crates/domovoi-engine/src/operations/investigate/start.rs
impl OperationHandle {
    /// Starts an Investigate operation for `incident`, or returns the id of the one already open for it.
    pub async fn investigate(&self, incident: &Incident, fresh: bool) -> Result<OperationId, OperationError>;
    /// `investigate`, then waits for the first outcome the investigation attaches to the incident.
    /// An already-open investigation returns `InvestigateError::AlreadyRunning`; waiting longer than
    /// `InvestigateSettings::wait_limit` returns `InvestigateError::TimedOut`.
    pub async fn investigate_and_wait(&self, incident: &Incident, fresh: bool) -> Result<TriageOutcome, InvestigateError>;
}
pub enum InvestigateError { AlreadyRunning(OperationId), TimedOut(OperationId), Operation(OperationError) }   // thiserror, short plain text
```

### 6.2 The operation seed **fixed**

`seed_for(incident, fresh, now) -> OperationSeed`:

| Field | Value |
|---|---|
| `kind` | `OperationKind::Investigate` |
| `goal` | `redact("Investigate: " + incident.title())`, cut to 500 chars |
| `pinned` | exactly one `PinnedSubject { host: incident.host(), check: incident.check().clone(), target: incident.target().cloned() }` |
| `incidents` | `[incident.id()]` |
| `parent` | `None` |
| `budgets` | `Some(BudgetLimits { tokens: None, cost_micros: None, ..<26's defaults from OperationSettings> })` — the token and cost limits are **off** for investigations, because the run's own `RunBudget`, `per_hour_runs` and the monthly budget already bound spending; a whole tool loop is one turn and would otherwise trip a 200 000-token operation limit and pause the operation before its proposals are stored |
| `first_steps` | one `NewStep { kind: ReasoningTurn, phase: "investigate", parent: None, intent: ReasoningTurn { context_refs: ["incident","host_profile","signals","changes","similar","notes","skills"] } }` (the seven parts of `context::build`, in order) |
| `effects` | `[(BuildContext, {"v":1,"operation":<id>,"cycle":0,"fresh":<bool>,"retry":false})]` (the operation id is the one `create` assigns: build the payload with the id the handle returns for the seed, or let `create` fill `"operation"`; verify how 26 does it) |
| `actor` | `Actor::System(SYSTEM_OPERATIONS)` |

`BuildContext` payload fields beyond 26's `{v, operation, cycle}` are `#[serde(default)]`: `fresh: bool`, `retry: bool`.

### 6.3 Handlers **fixed** (kinds, payloads, results); layout **convention**

All payloads carry `"v": 1`. An unparseable payload is `Dead{"unreadable payload"}`. Handlers read steps and incident **from the store**, never from the payload (except `CallModel`'s context text, below).
A stale claim (`CommitError::StaleEffect`) is `Handled`. Each handler commits through one writer command that completes its held effect in the same transaction.

**`TurnOutput`** (stored as the turn step's `output`; JSON, bounded to 16 KiB by cutting `report.details`, then `report.summary`, never other fields):

```json
{"v":1,"kind":"report|cached|skipped|failed","from":"<incident id, cached only>","report":{...TriageReport...},"reason":"<skipped|failed>","retry_at":"<RFC 3339, skipped with a retry only>"}
```

**`BuildContext`** payload `{v, operation, cycle, fresh, retry}`:

1. Load the snapshot. Operation terminal or the cycle's turn already final → `FinishEffect`. Status `Queued` → `Defer` the same effect kind, due `now + retry_interval` (`dedup_key build:<op>:<cycle>:wait`).
2. Find the cycle's open turn: the highest-ordinal `ReasoningTurn` step with `cycle == payload.cycle` whose outcome is `Running`. None (a later cycle, or after a retry was dropped) → `propose` one (`phase "investigate"`, intent as 6.2, plus a `caused_by` edge to the latest `Verification` step when `cycle > 0`).
3. Load the incident (first of `snapshot.incidents`). Missing → finish the turn `Failed{reason "the incident no longer exists"}`, no retry. For `retry == true`: an incident that is `Resolved`, `Muted` or `Pending` ends the investigation quietly: finish the turn `Skipped`, `Finish{Succeeded, "no investigation needed"}` (this mirrors `queue.rs:198-212`); the newest `triage.*` timeline entry not being `skipped`/`failed` also ends it (mirrors `queue.rs:220-231`).
4. `retry == true` and `free_slots(now) == 0` (or unreadable): no outcome is attached; `Defer` a `BuildContext{retry:true}` due at `next_slot` (or `now + ADMISSION_RETRY` when unreadable). The turn stays `Running`.
5. `prepare(shared, incident, fresh)` (cache only on `cycle == 0`, never on `retry`): `Cached{report, from}` → finish the turn `Succeeded` with `TurnOutput{kind: cached}` and create a `DraftSummary` effect. `Refused(r)` → `(text, retry_at) = refusal_text(..)`; finish the turn `NotRun{reason: text}` with `TurnOutput{kind: skipped, retry_at}` and create `DraftSummary`. `Admitted` → continue.
6. `context::build(shared, incident, &skills)` with `skills` from `deps.catalog.snapshot(host)`; the bundle is already redacted. Create a `CallModel` effect `{v, operation, step: <turn>, cycle, context: "<bundle>"}` (`max_attempts 3`, `dedup_key call:<turn>`) and complete this effect, one commit. The bundle travels in the effect payload (redacted, at most about 32 KiB) so a re-issued model call sees the same context.

**`CallModel`** payload `{v, operation, step, cycle, context}`:

1. The turn must still be `Running` and the operation open, else `FinishEffect`.
2. `generate(shared, incident, skills, bundle)`. The run is bounded by `RunBudget::for_run` (the old budgets, unchanged), fallback and capability handling are the existing `providers(..)` code.
3. `Ok(report)`: `succeeded(shared)`; finish the turn `Succeeded`, `summary = headline`, `output = TurnOutput{kind: report}`, `StepUsage { input_tokens, output_tokens, cost_micros }` from `RunUsage` (`cost_micros: None` when `any_unpriced`; never fabricated). `Err(reason)`: `failed(shared, ..)`; finish the turn `Failed{reason: redacted}`, `output = TurnOutput{kind: failed, reason}`, **no usage** when no response was received. Either way create `DraftSummary` `{v, operation, step: <turn>}` (`dedup_key draft:<turn>`).
4. The tool uses of the run (`Vec<ToolUse>`, already the redacted shape `record_tool_use` stores) are not part of `TurnOutput`: put them in the `DraftSummary` payload field `tools`, for `Notify` to hand to `record_tool_use`.
5. Store failure → `Retry`. A provider failure is a **result**, never `Retry` (a retry would spend another admission slot).

**`DraftSummary`** payload `{v, operation, step, tools}`:

1. Parse the turn's `TurnOutput`. For `report` and `cached`:
   a. If no `Assessment` step has a `caused_by` edge to the turn, `propose` `Assessment{text: cut(summary + "\n" + cause, 2000)}` (phase `assess`) and, when `next_step` or `manual_steps` is non-empty, `Plan{text: cut(next_step + manual steps joined by newlines, 2000)}` (phase `plan`), each with a `caused_by` edge (Plan → Assessment, Assessment → turn).
   b. For each proposal, in order: `proposals::check` against the host's current skills (a failure is a dropped line `"<skill>: <why>"`, redacted, and **no step**); skip a proposal for which this operation already has an `Execution` step with a `caused_by` edge to the turn and the same `(contract, args)` (it is kept, not duplicated); otherwise `propose_execution(op, ExecutionRequest { intent: ExecutionIntent { contract: ContractId::skill(skill), contract_version: 1, host, check, target, args, retry_of: None }, parent: Some(turn), caused_by: Some(turn) })`. A returned step whose outcome is `Refused` adds the dropped line `"<skill>: <reason>"`. `HostUnavailable` adds `"<skill>: no authenticated agent session to bind the action to"`.
2. Create one `Notification` step (`channel "incident"`, `subject = headline or the outcome's one-line reason`, phase `notify`, `caused_by` the turn) born `Pending`, unless one with a `caused_by` edge to the turn exists.
3. If the turn is `failed` and the failure streak is not degraded and the number of `Failed` turns of this operation is **below** `FAILED_RETRY_LIMIT`, or the turn is `skipped` with `retry_at: Some(at)`: `propose` a new `ReasoningTurn` (phase `investigate`, same cycle, `caused_by` the failed turn) and create `BuildContext{v, operation, cycle, fresh: false, retry: true}` due at `now + FAILED_RETRY` (failed) or `at` (skipped), `dedup_key build:<op>:<cycle>:retry:<turn>`. Skip this when a turn with a higher ordinal already exists (idempotency). This replaces `shared.defer` (`run.rs:86, 103, 143`) with a durable effect.
4. Create the `Notify` effect `{v, operation, step: <turn>, tools, dropped}` (`dedup_key notify:<turn>`, `max_attempts 5`) and complete this one, in the last commit. Every earlier sub-step is idempotent by the checks above, so a redelivered `DraftSummary` completes without duplicating anything.

**`Notify`** payload `{v, operation, step, tools, dropped}` (`dropped` is the list of check-failure lines `DraftSummary` computed, each already redacted; it travels in the `Notify` payload because a check failure has no step):

1. `outcome = report_view(snapshot, turn, &dropped)` rebuilds the `TriageOutcome` from the turn's `TurnOutput` and the Execution steps caused by the turn: `proposals` keeps those whose step is not `Refused`; the dropped lines are `dropped` followed by one `"<skill>: <reason>"` per `Refused` step (reason from the step's progress), in proposal order; `proposals::append_dropped` formats them; when at least one Execution step of the turn is `Pending`, append the constant `APPROVAL_POINTER = "Proposed actions are waiting for your approval on the Operations page of the web UI."` to `details`.
2. `cache.put(fingerprint, digest, report, now)` for `report` outcomes (the digest from `evidence_digest(incident, latest signal)`, as `run.rs:65, 126`).
3. `manager.attach_triage(incident, outcome, BTreeMap::new())` (no chat tokens: **no step has a chat token**) and, when `tools` is non-empty, `manager.record_tool_use(incident, "triage", tools)` — the same two calls `run::triage` makes (`run.rs:42-50, 148-155`).
4. Finish the `Notification` step `Succeeded` (summary = headline), resolve the `Waiters` entry of the operation with the outcome, complete the effect. **At-least-once**: a crash between step 3 and step 4 re-attaches the outcome once more on redelivery (a second timeline entry and card edit), exactly like every notification in this system; say so in the code comment.
5. `try_close(snapshot)` (6.3a).

### 6.3a Closing an investigation **fixed**

```rust
/// Pure. `Some((outcome, summary))` when the investigation has nothing left to do.
pub(crate) fn close_decision(snapshot: &OperationSnapshot) -> Option<(FinishOutcome, String)>;
```

Closes only an operation of kind `Investigate` whose status is `Running` or one of the four `Waiting*` (never `Queued`, `Paused` or terminal) and only when **all** hold:
no step is `Pending` or `Running`; the highest-ordinal `ReasoningTurn` is final; a `Notification` step with a `caused_by` edge to that turn is `Succeeded`.
Outcome: that turn `Succeeded` → `Succeeded` ("report delivered"); `Failed` → `Failed` (its reason); `NotRun` (skipped, no retry left) → `Failed` ("skipped: <reason>"). Operations that proposed executions stay open until every Execution step is final
(approved and verified: `decide` already finishes them on `Verified{passed:true}`; denied, expired, refused or failed ones leave no open step and the closer finishes them). The **closer task** runs `close_decision` over
`OperationRepo::with_status(["running","waiting_approval","waiting_user","waiting_agent","waiting_timer"], 200)` every `close_interval` (using `Clock::sleep_until`, never a real sleep) and calls `OperationHandle::finish` with `Actor::System`; `Notify` calls it once itself.
A `Finish` refused with `ExecutionInFlight` is retried on the next tick.

### 6.4 The investigator task **fixed**

`Investigator` subscribes to `triage.shared().deps.bus` and, for `EventKind::TriageRequested` with `event.notify == true` (the same filter as `queue.rs:83`), calls `handle.investigate(&event.incident, false)`. `investigate` creates nothing when `OperationRepo::for_incident(incident.id())` holds a non-terminal `Investigate` operation (returns its id). On `RecvError::Lagged` it runs the catch-up below. **Start-up catch-up** (replaces `reload_at_start`, `queue.rs:260-320`, for incidents that had no operation when the flag was turned on or while the hub was down): list active incidents (page size `Page::MAX_LIMIT`, at most `catch_up_limit`), skip `Pending`, skip those with any `triage.report|cached|skipped|failed` timeline entry, skip those with an open investigation, and for each remaining `Open` incident call `investigate`. A lagged bus also includes `Acked`/`Muted` ones as the old reload did.

### 6.5 Hub DTOs **fixed** (names and JSON); mirrors the frozen trace

Add `utoipa::ToSchema` structs in `dto_operations.rs`. Enumerations are `String` fields (consumers show unknown values generically). Timestamps are RFC 3339 UTC with a trailing `Z` (as the fixtures). Optional fields serialise as `null`, never absent, except where stated.

```rust
pub struct OperationSummaryDto {              // list rows and the incident page
    pub id: String, pub kind: String, pub goal: String,
    pub status: String,                       // OperationStatus::name
    pub pause_reason: Option<PauseReasonDto>,
    pub waiting_for: Vec<String>,             // "approval" | "user" | "agent" | "timer"
    pub cycle: u32, pub incidents: Vec<String>, pub hosts: Vec<String>,   // host ids of the pinned subjects
    pub created_at: String, pub started_at: Option<String>, pub ended_at: Option<String>, pub updated_at: String,
}
pub struct OperationPage { pub items: Vec<OperationSummaryDto>, pub next_cursor: Option<String>, pub as_of_seq: u64 }   // as_of_seq: journal head read before the page (28)
pub struct OperationDetailDto {
    pub as_of_seq: u64,                       // journal head read before loading (28's snapshot_seq); equals trace.as_of_seq
    pub operation: OperationSummaryDto,
    pub trace: OperationTraceDto,
    pub executions: Vec<ExecutionDto>,
}
pub struct ExecutionDto {
    pub step: String,                         // the Execution step id == the execution id
    pub approval_step: Option<String>,
    pub contract: String,                     // e.g. "skill.restart_container"
    pub contract_version: u32,
    pub host_id: String, pub check: String, pub target: Option<String>,
    pub args: BTreeMap<String, String>,
    pub state: String,                        // ExecutionState name, derived (26)
    pub plan_hash: String,                    // 64 hex: the v2 hash a person approves
    pub plan_binding: String,                 // always "v2"
    pub risk: Option<String>,                 // "read" | "write" | "destructive" from the host's current skill list; null when the skill is not on the host now
    pub confirm_required: bool,               // true when risk is "destructive" or null (6.6)
    pub expires_at: String,
    pub approved_by: Option<String>,          // "user:<id>"
    pub approved_at: Option<String>,
    pub reason: Option<String>,               // the step's reason (why refused / not run / unavailable / failed)
}
pub struct StepDetailDto {
    pub id: String, pub operation_id: String, pub ordinal: u32, pub kind: String, pub phase: String,
    pub parent: Option<String>, pub cycle: u32, pub outcome: String,
    pub started_at: Option<String>, pub ended_at: Option<String>,
    pub summary: Option<String>, pub reason: Option<String>,
    pub output: Option<String>,               // redacted, at most 16 KiB (never a secret: redacted by the writer)
    pub intent: serde_json::Value,            // the immutable intent JSON, as stored
    pub created_at: String,
}
pub struct ApproveStepRequest {               // POST .../approve
    pub plan_hash: Option<String>,            // required: 64 hex, same schema attributes as ApproveRequest
    pub confirm_text: Option<String>,         // required when ExecutionDto.confirm_required is true: the skill's name, exactly
}
// OperationTraceDto, TraceOperationDto, TraceStepDto, TraceEdgeDto, TraceRefDto, BudgetsDto, BudgetLimitsDto,
// BudgetUsedDto, PinnedSubjectDto, PauseReasonDto mirror 26's Rust trace types field for field
// (PauseReasonDto { type: String, exhausted?: String, step?: String, message?: String }, the three optionals skipped when None).
```

`impl From<&OperationTrace> for OperationTraceDto` must be lossless. **Contract test**: deserialize each of the five fixtures into `OperationTrace`, convert to the DTO, serialise: the JSON equals the fixture file. `GET .../trace` returns the DTO with `as_of_seq`: the handler calls `snapshot_seq(&state)` **first**, then `OperationRepo::load`, then `build_trace(&snapshot, as_of_seq)` (26's signature); a client applies only SSE events with `seq > as_of_seq`. `list` and `detail` do the same (`OperationPage.as_of_seq`, `OperationDetailDto.as_of_seq` and the nested trace carry the one value read once).

`web/src/api/operationTrace.ts` becomes `export type OperationTrace = components["schemas"]["OperationTraceDto"]` (and the four others) from the generated schema; keep the fixture test passing; if a union type no longer type-checks against the fixtures, fix the DTO, not the fixtures.

### 6.6 Endpoint behaviour **fixed**

| Endpoint | Behaviour |
|---|---|
| `GET /operations` | calls `snapshot_seq` first and returns it as `as_of_seq`; query `OperationQuery { status, incident, host (name), kind, cursor, limit }` (`IntoParams`); `status` must be in `OperationStatus::NAMES` else 400; `host` via `host_named` (404); `incident` must parse else 400; newest first; `OperationRepo::list`. `limit` clamps 1..=200, default 50 |
| `GET /operations/{id}` | `snapshot_seq` first (`as_of_seq` and `trace.as_of_seq` equal); 404 for an unparsable or unknown id; `OperationDetailDto`. `ExecutionDto.risk` comes from `deps.agents.skills(host)`; `confirm_required = risk != Some("write"|"read")` |
| `GET /operations/{id}/trace` | `OperationTraceDto`; 404 as above |
| `GET /operations/{id}/steps/{step}` | `StepDetailDto`; 404 when the step is not in that operation |
| `POST .../steps/{step}/approve` | `{step}` is the **Execution step id**. 400 without a valid `plan_hash`. 403 when `confirm_required` and `confirm_text` is not exactly the skill name (the same text as the legacy endpoint: "type the skill's name to approve a destructive action"; checked **before** any write). Builds `(authorization, provenance) = web_authorization(current.user.id, confirm_text, skill_name, &current.session_hash, now)` and calls `OperationHandle::authorize(op, step, authorization, PlanBinding::Full(plan_hash), provenance)`. Returns the refreshed `ExecutionDto` |
| `POST .../steps/{step}/deny` | `web_approval(..)` then `OperationHandle::deny(op, step, approval)`; returns `ExecutionDto` |

Error mapping (one function `operation_error(OperationError) -> ApiError`): `NotFound` → 404; `Execution(PlanChanged)` → 409 "the plan changed since you opened it; reload the page and review it again"; `Execution(Expired)` → 409 "the approval window has closed"; `Execution(AlreadyAuthorized)` → 409 "this step was already approved"; `Execution(NotYetSupported)` → 409 "policy authorizations are not supported yet; a person must approve"; `Decide(e)` → 409 with `e`'s plain text; `Conflict` and `StaleEffect` → 409 "another change landed first; reload and try again"; `HostUnavailable` → 409 "the host's agent is not connected"; `Store` → 500 via `ApiError::internal`; `Unavailable` → 503 "the operation service is restarting; try again". With `deps.operations == None` every endpoint except the two lists answers 503 "orchestration is not enabled"; the lists answer 200 with empty pages.

`retry_triage` with `deps.operations == Some`: calls `investigate_and_wait(&incident, fresh)` inside the same spawned task; `AlreadyRunning` → 409 "an investigation is already running for this incident"; `TimedOut` → 200 with `TriageStateDto { kind: "skipped", reason: Some("the investigation is still running; its result appears on the incident") }`; otherwise `triage_state(&outcome)`. With `None` it is unchanged.

### 6.7 Legacy projection **fixed**

- `ActionDto` gains `legacy: bool`: **true for every row** returned while `config.orchestration.enabled` is true (every `actions` row was created by the old path), false otherwise. `GET /api/v1/actions`, `IncidentDetail.actions` and the legacy approve/deny responses use it. The rows are the unchanged `ActionRequest`s: nothing is converted, no step or event is created, no field is invented. A unit test proves `list_actions` over seeded actions creates zero rows in `operations`/`steps`.
- `IncidentDetail.operations: Vec<OperationSummaryDto>` from `OperationRepo::for_incident`, newest first (empty when there are none).
- The Operations list screen shows both: it calls `GET /api/v1/operations` and `GET /api/v1/actions` and labels rows of the second **Legacy** (consumer note in section 14).
- The legacy approve and deny endpoints and the legacy executor stay as they are.
- In operation mode a card shows **no approve or deny button for a step** (`build_card` only knows legacy actions; no chat token is minted for a step). The report's `details` carries `APPROVAL_POINTER` while a step awaits approval (6.3, Notify).

### 6.8 Configuration documentation **convention**

Update doc comments only: `[orchestration] enabled` — "Run investigations as operations: new incidents are investigated by an Investigate operation and its proposed actions are approved per step on the Operations page. Needs `[triage] enabled`. Read once at start; flipping it needs a restart."; `[triage] concurrency` — "Triage runs at the same time, and questions and chat at the same time. With `[orchestration] enabled` investigations are limited by `[orchestration] max_running` instead." Then `cargo xtask docs-config`.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Open every file and symbol of section 3. Record differences (moved lines, renamed types, changed signatures).
   - Confirm prompts 22 to 26 are merged: `OperationHandle`, `OperationService::spawn`, `OperationDeps`, `OperationRepo`, `decide`, the operation runner and the `[orchestration]` section exist. **If 26 is not merged, stop and report.** Use its merged names, not the names in this brief, and list every difference.
   - Check whether `Defer` creates successors of any effect kind (3.2); check how `seed.effects` payloads learn the operation id; check whether a `Queued` operation's effects can be claimed before admission (the handler of 6.3 defers while `Queued` either way).
   - `grep -rn "ApiDeps {" crates` and `grep -rn "TriageSettings {" crates` and keep both lists for step 7 and step 3.
   - Run `cargo xtask arch-check` and the engine tests `drills`, `services` once to know the baseline is green. Run `ls crates/domovoi-store/migrations/` only to confirm that this prompt adds none.
2. Create branch `investigate-operation` from the latest `overhaul-investigation`.
3. **Triage seams (4.1)**: visibility, `run_queue`, the `prepare`/`generate`/`refusal_text` split, `RunUsage`, `append_dropped`, `check`. After this step **the whole existing engine suite must still pass unchanged** (`cargo nextest run -p domovoi-engine`); this is the proof that the legacy path runs the same code. Commit `refactor(engine): split a triage run into prepare, generate and report steps`.
4. **Investigation module (4.2)**: `turn_output.rs` and `close.rs` first (pure, with unit tests), then `start.rs`, `waiters.rs`, the four handlers, registration in `OperationService::spawn`. Unit tests for `TurnOutput` round trip and cut, `close_decision` (table of 12.2), `report_view`.
5. **Parity harness (12.3)**: extend `tests/drills/world.rs` and `tests/services/harness.rs` with a `Mode`, adapt `score`/`probes` to a neutral proposal view, write `tests/services/parity.rs`. Make every scenario pass in `Mode::Legacy` first (it must, unchanged), then in `Mode::Operations`. Commit `test(engine): run the triage drills and limit scenarios under both modes`.
6. **Engine operation tests (12.2)**: restart, retry, cap, closing, no duplicate on redelivery.
7. **Hub**: DTOs, handlers, routes, `ApiDeps` fields (and every constructor), the `legacy` flag, `IncidentDetail.operations`, `retry_triage`, runtime wiring order, `run_queue`. Hub tests (12.4).
8. `cargo xtask openapi && cd web && npm run gen:api`; replace `operationTrace.ts` by aliases; `npm run lint && npm run typecheck && npx vitest run src/test/fixtures/operation-trace`.
9. **Docs** (section 15) and `cargo xtask docs-config`, `cargo xtask docs-links`.
10. **Checks and the completion report.** `cargo fmt --all`; `cargo clippy -p domovoi-engine -p domovoi-hub -p domovoi-core --all-targets -- -D warnings`; `cargo nextest run -p domovoi-engine -p domovoi-hub`; `cargo nextest run -E 'rdeps(domovoi-engine)'` (public items changed); `cargo xtask arch-check`; `cargo xtask docs-config --check`.

Commit in small green steps (one-line Conventional Commits with one scope each, for example `feat(engine): investigate operation handlers`, `feat(hub): operation endpoints and step approvals`): after steps 3, 5, 6, 7 and 9.

## 8. Database and persistence

**No migration, no schema change.** The investigation reuses the tables of prompt 26 (`operations`, `steps`, `step_edges`, `executions`, `operation_incidents`) and the `effects`
table; `ls crates/domovoi-store/migrations/` must be the same before and after. The effect payloads (a redacted bundle of about 32 KiB in `CallModel`, `tools` and `dropped` lists) are ordinary
`effects.payload` text. Transaction boundaries are those of 26: every handler result is one `UnitOfWork` commit that also completes the held effect. **N-1 upgrade test: not applicable** (no schema change); the existing upgrade tests must still pass.

## 9. Protocol and compatibility

No wire change (`PROTOCOL_VERSION` stays 1.2). A step's id travels as the existing `action_id`/`execution_id` UUID (prompt 26); nothing here sends anything to an agent. HTTP: six new routes and four additive DTO fields (`ActionDto.legacy`, `IncidentDetail.operations`); no existing field is removed or renamed, so the generated `schema.d.ts` changes only by addition. Old clients ignore the new fields.

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| Hub restarts with a pending or claimed `BuildContext`, `CallModel`, `DraftSummary` or `Notify` effect | the operation runner reclaims it (claimed non-dispatch effects revert to `pending`, prompt 26); the handler is idempotent by the checks in 6.3; a lost model call is re-issued as a new attempt (it takes another admission slot, as a restart-resumed triage does today) |
| `CallModel` redelivered after its result committed | the turn is no longer `Running`: `FinishEffect`, no second model call |
| Provider chain fails | the turn is `Failed` with the redacted reason, `failed(shared, ..)` raises `TriageDegraded` after 3 in a row, the failure card goes out, a retry is scheduled after `FAILED_RETRY` unless degraded or already `FAILED_RETRY_LIMIT` failed turns exist |
| Hourly cap reached with a known free time | the turn is `NotRun` (skipped), the skipped card goes out, a `BuildContext{retry:true}` is created for that time; on firing it re-checks `free_slots` first and silently reschedules when the hour is still full |
| Hourly cap that can never admit, monthly budget spent, unpriced provider | the turn is `NotRun` (skipped), the card goes out, **no retry**, the investigation closes `Failed` ("skipped: ...") |
| Admission limits unreadable (`Unverifiable`) | skipped as today, retried after `ADMISSION_RETRY` |
| The incident disappears or is `Resolved`/`Muted`/`Pending` when a **retry** fires | the investigation ends `Succeeded` ("no investigation needed") without a model call |
| The model proposes a skill that does not apply, has bad arguments, or is not on the host | no step for it; one "Proposals not offered" line in the report |
| `propose_execution` stores a step `Refused` (target not pinned, no blind retry, budget spent) | the step stays as the record; the line is added to the report |
| `propose_execution` fails with `HostUnavailable` | dropped line; no step |
| `DraftSummary` redelivered mid-way | steps already created are found by their `caused_by` edges and reused; nothing is duplicated |
| `Notify` crashes between `attach_triage` and finishing the Notification step | redelivery re-attaches once more (a duplicate timeline line and card edit); documented at-least-once |
| `attach_triage` fails (manager closed) | `Retry` while `attempt < max_attempts`, then the effect dies and an advisory is raised by the runner (prompt 26 / effect lifecycle); the Notification step stays `Pending`, so the closer never closes the investigation silently |
| An execution is denied, expires, is refused or fails | no open step remains; the closer finishes the investigation within `close_interval` |
| `Verified{passed:false}` | prompt 26 creates cycle `cycle + 1` and a `BuildContext`; the handler proposes the new cycle's turn with a `caused_by` edge to the verification and runs a full investigation again; the cycle limit escalates |
| `retry_triage` while an investigation is open for the incident | 409 "an investigation is already running for this incident" |
| `investigate_and_wait` times out | 200 `skipped` with the "still running" reason; the investigation continues and attaches its outcome later |
| Triage disabled, orchestration enabled | operations exist but no investigation starts; `TriageRequested` is ignored, as today with triage off |
| Orchestration disabled | `run_queue` stays true, the queue runs, no operation service, no investigator |
| Step endpoint with the service down | 503 as in 6.6 |

## 11. Security requirements

- **Human-only approval (INV-1, ADR 0007).** The approve and deny routes are the only way to authorize or refuse a step. `HumanApproval::new` is called only inside `crates/domovoi-hub/src/auth/` (the routes call the existing `web_authorization`/`web_approval`); this prompt adds no constructor, no chat command, no model tool, no background approver. The `ReadTool` catalogue stays closed and no tool schema names `authorize`, `approve`, `propose_execution` or `deny`.
- **Plan binding.** A step is authorized only with `PlanBinding::Full(<the stored v2 hash>)`; there is no prefix binding for steps (a prefix exists only for chat buttons, which cannot approve steps). The endpoint never reads the hash from the stored row to fill a missing request value: a missing or malformed `plan_hash` is a 400.
- **Typed confirmation (INV-6).** A destructive skill, **and a skill whose risk is unknown at approval time**, needs `confirm_text` equal to the skill's name; checked in the route before `authorize`; recorded in the `Web { typed_confirmation }` channel. Fail closed: unknown means destructive.
- **Pinned targets, no blind retry, fail-closed dispatch.** All enforced by the writer of prompt 26; the investigation adds no way around them: every proposal carries the incident's own `(host, check, target)`, which is the operation's pinned subject.
- **Model output stays data (INV-9).** The report text is stored in a step output and shown, never executed; only a skill name that exists on the host and passes `check` and the writer's validation becomes a step; a step is never dispatched without a human approval; `nothing_approved` drills probe it in both modes.
- **Secrets.** The bundle is redacted before it enters the effect payload; the report is redacted in `generate` before it is stored; step `output` is redacted by the writer; the journal, audit rows and the trace never contain outputs; the new endpoints return only redacted, bounded text. Provider errors pass through the redactor before they become a reason.
- **Authentication.** Every new route needs `Authed`; the POSTs pass the CSRF guard like all unsafe routes. `ExecutionDto` exposes the plan hash only to an authenticated session.
- **Audit.** Approvals and denials are audited by the writer in the same transaction (prompt 26). The routes add no audit rows of their own.

## 12. Testing requirements

Names describe behaviour. Fakes that exist: `InMemoryStore`, `FakeClock`, `FakeAgentLink`, `ScriptedLlm`, `ListRedactor`, `action_caps`, `evidence_caps`, `observer_of`
(`crates/domovoi-testkit/src/`); prompt 26's harness (`crates/domovoi-engine/tests/operations/harness.rs`) and its fake registry and resolver. Tests never sleep: they advance `FakeClock`.

### 12.1 Triage seams (engine unit tests next to the code, and the whole existing suite)

`prepare_reports_cached_refused_and_admitted` (table over a cache hit, a spent monthly budget, a full hour, an unreadable window, an open hour); `refusal_text_matches_the_old_skip_texts` (the exact strings of `run.rs:80-111`);
`run_usage_sums_every_call_and_marks_an_unpriced_one`; `append_dropped_formats_the_not_offered_lines` (equal to what `create` produced before: assert with the existing `proposals` tests);
`run_queue_false_starts_no_workers` (spawn with `run_queue = false`, send `TriageRequested`: nothing is triaged and the `JoinHandle` ends on shutdown). **Gate for step 3:** all of `cargo nextest run -p domovoi-engine` passes with no test edited.

### 12.2 Investigation module (`crates/domovoi-engine/tests/operations/investigate.rs`, registered in `tests/operations/main.rs`; harness = prompt 26's plus `ScriptedLlm`, the manager and `TriageService` with `run_queue = false`)

| Test | Setup, action, expected |
|---|---|
| `turn_output_round_trips_and_stays_under_16_kib` | a report with a 40 KiB `details`: the stored JSON is at most 16 KiB, `headline`, `verdict` and `proposals` intact |
| `seed_pins_exactly_the_incident_subject` | `seed_for` has one pinned subject equal to the incident's host, check and target, tokens and cost limits off, one running `ReasoningTurn` |
| `incident_event_starts_one_investigation` | publish `TriageRequested` twice for one incident: one operation, linked to the incident |
| `retriage_while_open_is_refused` | `investigate_and_wait` with an open operation: `AlreadyRunning` |
| `investigation_delivers_the_report_and_closes` | scripted model returns a valid report with no proposals: turn `succeeded`, one `Assessment`, one `Notification` `succeeded`, the incident timeline has `triage.report`, the operation is `succeeded` |
| `usage_is_recorded_on_the_turn_and_never_fabricated` | reported usage appears in `budgets.used`; a failed run with no response records no tokens |
| `proposal_becomes_an_execution_step_with_a_plan_hash` | model proposes `restart_container`: one `Execution` step (with an `executions` row, id equal to the step id), an `Approval` step, `depends_on` and `caused_by` edges, the operation `waiting_approval`, **no row in `actions`** |
| `proposal_for_an_unknown_skill_is_dropped_with_a_line` | the card's report `details` ends with "Proposals not offered:" and the skill, no step |
| `refused_execution_is_listed_as_not_offered` | an unpinned target (force a mismatch) stores a `Refused` step and a dropped line |
| `report_tells_the_reader_where_to_approve` | with a pending step the attached report `details` contains `APPROVAL_POINTER`; with none it does not |
| `no_chat_token_is_minted_for_a_step` | the `TriageCompleted` event has an empty `tokens` map |
| `cached_report_goes_through_the_same_pipeline` | resolve an incident, open a repeat: the turn is `succeeded` with `kind: cached`, proposals re-validated and proposed as steps, timeline `triage.cached` |
| `hourly_cap_skips_then_retries_when_the_hour_frees` | `per_hour_runs = 1`, run twice: the second turn is `not_run`, `triage.skipped` is attached, a retry effect is due at the slot time; advance the clock: the retry runs and attaches `triage.report`; the first-pass card is not repeated while the hour is full |
| `monthly_budget_spent_skips_without_a_retry` | no retry effect, the investigation closes `failed` |
| `failed_run_retries_twice_then_stops` | three failing scripted runs: three `triage.failed` entries, exactly two retry effects were created, the third failure closes the operation `failed`; the degraded advisory is raised after three failures across incidents |
| `retry_after_resolution_ends_quietly` | skip, resolve the incident, fire the retry: no model call, operation `succeeded` |
| `restart_resumes_an_unfinished_investigation` | stop the service after `BuildContext` committed `CallModel` but before it ran; start a new service on the same store: the report is attached once |
| `redelivered_call_model_makes_no_second_model_call` | run the handler twice for one effect record: one provider request |
| `redelivered_draft_summary_creates_no_duplicate_steps` | run it twice: one Assessment, one Notification, one Execution per proposal |
| `redelivered_notify_attaches_again_but_finishes_once` | documents the at-least-once behaviour: after a simulated crash before step 4 the second delivery attaches a second outcome and the Notification step is `succeeded` once |
| `catch_up_investigates_open_incidents_without_a_triage_entry` | seed an open incident with no timeline entry and one with `triage.report`: only the first gets an operation; a `Pending` one is skipped |
| `closer_finishes_after_denial_expiry_and_failure` | deny, expire and a failed run each leave no open step; within `close_interval` the operation is `succeeded`/`failed` as `close_decision` says |
| `closer_never_closes_a_paused_queued_or_waiting_investigation` | pending approval, `Paused{Budget}`, `Queued`: untouched |
| `verification_failure_runs_a_second_investigation_cycle` | approve and run, verify check still firing: cycle 1, a new turn caused by the verification, a second report attached |
| `investigate_and_wait_times_out_with_a_skipped_outcome` | block the scripted model past `wait_limit` (advance the clock): `TimedOut`; the investigation later finishes |
| `queued_operation_defers_build_context` | `max_running = 1` with a running operation: the second waits `Queued`, its `BuildContext` defers, then runs after the first closes |

`close_decision` table (unit): no turn; turn running; turn final without a notification; notification pending; notification succeeded with a pending execution; with all final and turn `succeeded`/`failed`/`not_run`; paused; terminal; non-Investigate kind.

### 12.3 The parity suite (this is the checkpoint of prompt 58)

Harness: add `pub enum Mode { Legacy, Operations }` to `crates/domovoi-engine/tests/services/harness.rs` and to `tests/drills/world.rs`.
`World::run()` and `world::run(drill, model)` take a `Mode`; in `Operations` they spawn the manager, `TriageService` with `run_queue = false`, an `OperationService` with `InvestigateDeps`, create the operation with `investigate_and_wait` and drain the effect runner by advancing `FakeClock` (no sleeping). The drill scorer and probes get a neutral input
`ProposalView { skill: String, args: BTreeMap<String,String>, risk: Risk, awaiting_human: bool, approved: bool }` built from `ActionRequest`s in `Legacy` and from Execution steps and their rows in `Operations`; the thresholds, the six YAML cases and the score weights are **not** edited.

New file `crates/domovoi-engine/tests/services/parity.rs` (registered in `main.rs`), each test running one scenario twice and asserting equal results:

| Test | Compared |
|---|---|
| `scripted_drills_pass_under_both_modes` | all six drills (`tests/drills`), scores at least 80 and every gate probe holds in `Legacy` and in `Operations`; the two modes give the **same total** per drill |
| `report_fields_are_identical_across_modes` | for each drill: `headline`, `summary`, `cause`, `evidence`, `verdict`, `next_step`, `manual_steps` equal; `proposals` equal (skill and args); `details` equal except for the trailing `APPROVAL_POINTER` paragraph |
| `tool_call_budget_is_enforced_the_same` | a script that asks for 20 calls: the run stops at 15 with `NO_CALLS_LEFT`/`BUDGET_EXHAUSTED`, same request count in both modes |
| `output_token_budget_and_deadline_are_enforced_the_same` | same granted tokens and the same failure text when the allowance or `per_run.seconds` is spent |
| `fallback_chain_behaves_the_same` | primary fails, secondary answers: same report; both fail: same joined failure reason, same `TriageDegraded` advisory after three |
| `capability_rejection_rebuilds_the_run_the_same` | provider refuses `tools`: same rebuilt request shape and report |
| `hourly_and_monthly_caps_behave_the_same` | same skipped reasons, same slot time, no model call when refused |
| `repeat_cache_is_used_the_same` | same `cached` outcome and `from` |
| `host_scoping_is_the_same` | a model call for another host's tool is refused identically; read skills limited to the incident's host |
| `secret_never_reaches_the_model_in_either_mode` | the drill secret is absent from every request |
| `nothing_runs_without_a_human_in_either_mode` | no skill request reaches the link; in `Operations` no step is `Authorized` |
| `legacy_mode_creates_no_operation_and_operations_mode_creates_no_action` | row counts |

### 12.4 Hub (`crates/domovoi-hub/tests/hub/operations_api.rs`, harness `api_harness.rs`)

| Test | Expected |
|---|---|
| `operations_list_filters_and_pages` | by status, incident, host name, kind; unknown status 400; cursor pages; newest first |
| `operations_endpoints_need_a_session` | 401 without a cookie; the two POSTs also need the CSRF header |
| `operation_trace_json_equals_the_golden_fixtures` | each of the five fixtures through `OperationTrace` → `OperationTraceDto` → JSON equals the file |
| `operation_detail_lists_executions_with_the_v2_plan_hash` | `plan_hash` is 64 hex and equals the stored hash; `plan_binding == "v2"`; `risk` from the live skill list; `confirm_required` true for destructive and for a skill missing from the host |
| `operation_reads_carry_as_of_seq_read_before_loading` | list, detail and trace each return `as_of_seq` equal to the journal head before the call; a journal row committed between `snapshot_seq` and the load (store that appends on read) never makes the value higher than the head the snapshot saw; detail's `as_of_seq` equals its `trace.as_of_seq` |
| `operation_journal_rows_reach_the_operations_topics` | smoke only: with 28's stream, a step approval produces a frame on topics `operations` and `operation:<id>` (the mapping is 28's; this only proves the endpoints and the stream agree on `as_of_seq`) |
| `step_detail_returns_intent_and_bounded_output` | `intent` equals the stored JSON; an output over 16 KiB cannot exist |
| `approve_step_with_the_shown_hash_authorizes_and_dispatches` | approve with the hash from the detail: execution `authorized`, a `dispatch` effect exists, audit `execution.approved` names the user |
| `approve_step_without_a_hash_is_a_bad_request` | 400, nothing changes |
| `approve_step_with_another_hash_is_a_conflict` | 409 plan-changed text, nothing changes |
| `approve_destructive_step_needs_the_skill_name` | 403 without `confirm_text`; 200 with it; the stored channel has `typed_confirmation = true` |
| `approve_step_for_a_skill_gone_from_the_host_needs_the_skill_name` | fail closed |
| `approve_expired_and_double_approve_are_conflicts` | 409 each |
| `deny_step_refuses_the_execution` | approval `refused`, execution `refused`, audit `execution.denied` |
| `step_approval_is_the_only_route_to_authorized` | a source scan of `crates/domovoi-hub/src` finds `OperationHandle::authorize` called only in `api/operations.rs`; `HumanApproval::new` still only under `auth/` |
| `legacy_action_endpoints_still_work_and_are_labelled` | with orchestration on, `GET /actions` rows have `legacy: true`, approve on a legacy action still dispatches under its `ActionId`; with it off `legacy` is false |
| `listing_legacy_actions_creates_no_operations_rows` | counts of `operations`, `steps`, `executions` stay zero |
| `incident_detail_lists_its_operations` | `operations` filled, newest first; empty array when none |
| `retry_triage_runs_an_investigation_when_enabled` | 200 with the outcome kind; a second concurrent call 409 |
| `endpoints_answer_503_when_orchestration_is_off` | the four reads and two POSTs per 6.6; lists are empty 200 |
| `flag_off_hub_serves_the_queue` | with the flag off the runtime has no `OperationHandle` and `TriageSettings.run_queue` is true; with it on, false |

### 12.5 Gates

Extend `crates/domovoi-engine/tests/gates.rs` (never weaken): `investigate_handlers_never_name_the_raw_link` (no `AgentLink`, `run_skill`, `run_tool`, `run_check`, `start_skill` under `operations/investigate/`); `investigate_proposals_only_go_through_propose_execution` (no `ActionRequest::propose`, `actions_service` or `OperationCommand::Dispatch` under `operations/investigate/`); `investigate_never_constructs_an_authorization` (no `Authorization::`, `HumanApproval`, `PolicyPermit` there); `model_cannot_reach_the_operation_writer` from prompt 26 still passes (`ReadTool::ALL` unchanged).

### 12.6 Web

`npx vitest run src/test/fixtures/operation-trace` passes with `operationTrace.ts` re-pointed at the generated schema; `npm run typecheck` proves each fixture JSON is assignable to the generated `OperationTraceDto` (an assignment in `trace.test.ts`).

## 13. Acceptance criteria

1. With `[orchestration] enabled = false`: no operation service, no investigator, `run_queue = true`; the engine suite (`drills`, `services`, `engine`, `crash`) passes **unchanged**, the six drills pass, and `cargo xtask arch-check` is green.
2. With the flag on and triage enabled, a `TriageRequested` event creates exactly one Investigate operation (pinned to the incident's subject, linked to the incident) and no legacy `actions` row is ever created for a proposal.
3. The report attached to the incident, the cards, the caps, the budgets, the fallback chain and the host scoping match the legacy mode; the parity suite of 12.3 passes, in particular `scripted_drills_pass_under_both_modes` and `report_fields_are_identical_across_modes`.
4. Each model proposal that passes the checks becomes an `Approval` + `Execution` step pair with a v2 plan hash; every proposal that does not is listed under "Proposals not offered" in the report.
5. Deferred and failed investigations resume from durable effects; a restart at any point of the pipeline finishes the investigation once, with no second model call for a committed turn, and without duplicate steps (12.2 restart and redelivery tests).
6. Hourly-cap and failure retries behave as before (same texts, same limits of 3 failed runs and unlimited cap retries) and are visible as steps and effects.
7. An investigation closes itself when no step is open and its notification succeeded; it never closes while a step awaits approval, a retry is pending, or it is paused.
8. `POST /operations/{id}/steps/{step}/approve` authorizes only with the stored v2 hash, a typed confirmation for destructive or unknown-risk skills, a human session and CSRF; no other code path authorizes a step; deny refuses it.
9. `GET /operations`, `/operations/{id}`, `/operations/{id}/trace` and `/operations/{id}/steps/{step}` return the DTOs of 6.5; the trace JSON equals the five golden fixtures.
10. `ActionDto.legacy` and `IncidentDetail.operations` are present; legacy actions are listed unchanged and nothing is synthesized for them.
11. No migration, no wire change, no new config key; `cargo xtask openapi && npm run gen:api` is current; `cargo xtask docs-config --check` is clean.
12. `cargo clippy -p domovoi-engine -p domovoi-hub -p domovoi-core --all-targets -- -D warnings`, `cargo nextest run -p domovoi-engine -p domovoi-hub` and the web checks pass.

## 14. Migration and compatibility

- **Existing installs.** Nothing changes until the owner sets `[orchestration] enabled = true` and restarts. The tables of prompt 26 already exist and are empty until then.
- **Turning the flag on with work in flight.** Legacy `Proposed`/`Approved` actions keep working through the legacy endpoints and executor and finish under their `ActionId`. The queue stops, so triage runs that were queued in memory are lost exactly as on any restart today; the start-up catch-up covers open incidents with no triage entry. Triage reports already attached stay readable on the incident page.
- **Turning it back off.** Restart with the flag off: the queue runs again; operations already created stay in the store and are shown read-only by the endpoints (they return 503 for the POSTs and empty lists while the service is off). An open investigation is not resumed until the flag is on again.
- **Operation approval window.** Operation proposals use `[orchestration] approval_ttl` (24 h by default) instead of the 1 h `PROPOSAL_TTL`, which applies only to the legacy path.
- **Concurrency.** `max_running` bounds investigations; excess incidents wait `Queued` durably, instead of the in-memory queue that dropped the oldest beyond 64.
- **Consumers (notes for the operations screens).** The UI must (a) list `GET /operations` and `GET /actions` together and label the second set **Legacy** (`ActionDto.legacy`); (b) approve with `{step}` = `ExecutionDto.step` and the body `{ plan_hash: ExecutionDto.plan_hash, confirm_text? }`, showing a typed-confirmation field when `ExecutionDto.confirm_required`; (c) render the timeline and graph from `OperationTraceDto` (`GET /operations/{id}/trace`, with `as_of_seq`; refetch on SSE frames for topics `operations` and `operation:<id>` through 28's `keys.operations` and `keys.operation(id)`, applying only events with `seq > as_of_seq`), reading step inspector data from `GET /operations/{id}/steps/{step}`; (d) treat every enumeration string as open (unknown values shown generically); (e) not expect cancel, pause, resume or answer routes yet.
- **Agents, wire, retention.** No change.

## 15. Documentation requirements

- `ARCHITECTURE.md`: §7.1 the investigation pipeline behind `orchestration.enabled` and the `run_queue` switch; §10 (HTTP) the six routes and the legacy label; §15 the approval invariant for steps (endpoint, binding, typed confirmation); keep §4 untouched (no new crate edge). No ADR (no arch rule changes); if `cargo xtask arch-check` reports a needed rule change, stop and report.
- Book: extend `docs/book/src/concepts/operations.md` (created by the previous prompt): an Investigate operation, its steps, how proposals are approved per step, that chat cannot approve steps yet, and where legacy actions appear; `docs/book/src/concepts/actions.md`: legacy actions and the **Legacy** label; `docs/book/src/concepts/incidents.md`: the operations list on the incident page; the page that documents `[triage]` and `[orchestration]` keys is regenerated by `cargo xtask docs-config`. Run `cargo xtask docs-links`.
- OpenAPI: `cargo xtask openapi && cd web && npm run gen:api`; commit the regenerated `openapi.json` and `schema.d.ts`; never hand-edit them.
- Config reference: `cargo xtask docs-config` (doc-comment changes only).
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
