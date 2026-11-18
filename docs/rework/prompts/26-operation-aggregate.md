# 26 — Operation aggregate, execution module and trace schema

| | |
|---|---|
| Branch | `operation-aggregate` |
| Milestone | M3 |
| Depends on (must be merged) | 23, 24, 25 (24 and 25 are written but not merged when this brief was drafted: **introduced by prompt 24 / prompt 25 — verify every name in code before use**). 22 and 23 are merged; 39 is not a dependency |
| Can run in parallel with | 39 (order-flexible, never concurrent; 39 adds no migration and no store code, so no file overlaps) |
| Produces (shared contracts) | `Operation`, `OperationStatus`, `Step`, `StepKind`, `StepOutcome`, `StepEdge`, `EdgeKind`, `OperationTrace` and its golden fixtures (Rust and web); tables `operations`, `steps`, `step_edges`, `executions`, `operation_incidents`; `AggregateKind::Operation`; `OperationWrite`; the operation writer, `decide` and the effect handlers |
| Consumes | `UnitOfWork`, `Transaction`, `EffectQueue`, `EffectRunner` (08, 15); `Authorization`, `HumanApproval`, `PlanHash::compute_v2`, `ExecutionId` (22); `Evidence::collect_evidence` and the dispatch boundary (23); `Availability`, `CapabilityRegistry`, `TargetResolver` (25); `ContractId` (24) |
| Launch gate | none beyond the dependencies above |
| Suggested model | large: cross-cutting (core, store, engine, hub wiring, arch rules) and safety-critical (it moves the dispatch decision for a second path) |

## 1. Objective

Create the **Operation aggregate**: a durable, typed record of one piece of work on one or more hosts, made
of immutable **steps** joined by typed **edges**, with a coarse status, budgets, one writer, a pure
`decide` function, effect handlers that run the work, a seeded simulator that proves the invariants, and a
**frozen trace schema** that the timeline, the graph and the SSE stream will all render from.

After this change, with `[orchestration] enabled = true`:

- an operation can be created, can accept steps, can wait for a human, dispatch a skill to an agent under
  `execution_id = step_id`, settle the result, verify it after a delay, start a new cycle when verification
  fails, pause on a spent budget or an unknown outcome, survive a hub restart, and be cancelled;
- every change is one `UnitOfWork` commit (state + journal + audit + effects), so nothing is half-written;
- the same data renders as the trace JSON in section 6.9, pinned by golden fixtures.

With `enabled = false` (the default) **nothing changes**: the operation service is not started, the legacy
action and triage paths run exactly as today, and the new tables are empty.

This prompt does **not** make the model investigate (27), approve from the UI (27, 30), stream to the
browser (28), build the use-case layer (29) or evaluate policy (39, 40). Section 5 lists every exclusion.

## 2. Architectural context

Everything the executor needs is here; the cited documents are background.

**RW-04 — Operation is an aggregate, an Execution is a typed Step.** `Operation` is distinct from
`Incident`. An execution is a `Step` of kind `Execution` plus exactly one `executions` row (contract,
target, args, plan hash, authorization, dispatch and result). Both belong to the Operation aggregate and are
written **only by the operation writer** (one tokio task for all operations, as one task serves all actions
today). The link between an operation and its incidents (`operation_incidents`) is also owned by that
writer; the incident manager only reads it. One dispatch attempt per Execution step: a retry is a new
Execution step that points back with a `retry_of` edge and needs its own authorization. Reconciliation is
not a new attempt; it completes the existing step.

**RW-05 — coarse status, immutable steps, append-only events.** `OperationStatus` is coarse. A step's
*intent* (kind, phase, parent, cycle, intent JSON) never changes after insert. Its *progress* is an
append-only sequence of journal entries; the `steps.status` column is a derived projection written in the
same transaction, for queries only. Execution states (`Proposed`, `AwaitingApproval`, `Authorized`,
`Dispatched`, …) are derived from the `executions` row and the step outcome: **there is no second status
column** for an execution.

**RW-40 — step graph.** `parent_step_id` is grouping only. Order and causality use typed edges
`step_edges(from_step, to_step, kind)` directed from dependent to prerequisite: `depends_on`, `caused_by`,
`verifies`, `retry_of`, `answers`. An edge may only point at a step that was already committed earlier in the
same operation, so the graph is acyclic by construction. Investigation cycles are new steps with
`cycle + 1`, never graph cycles. Steps and edges live as long as the operation.

**RW-36 — execution identity.** From this milestone a new Execution step gets `execution_id = step_id`,
generated when the step is inserted, and it never changes after dispatch. The agent journal and the wire
already key on a UUID called `execution_id` / `action_id` (wire 1.2 is frozen, RW-42): the step id travels in
that field. A legacy action keeps its `ActionId` and finishes on the legacy path; an id never appears in both
`actions` and `executions`.

**RW-32 — legacy actions are retained.** Existing `actions` rows are not converted. This prompt neither
migrates nor projects them (the read-only projection is prompt 27).

**RW-08 / RW-11 — one execution boundary, bound to an immutable plan.** A run is dispatched only from a row
that still carries the authorization a human gave it, with the plan hash recomputed (v2: contract, implementation
hash, target, args, execution id) and equal to the one approved. A policy authorization is refused until a
permit issuer exists (45). `HumanApproval::new` stays in `crates/domovoi-hub/src/auth/`;
`HumanApproval::restore` stays in `crates/domovoi-store/src/`. This prompt adds neither a constructor nor an
approval path: it adds a writer command that **accepts** an `Authorization` the hub's auth layer already built.

**RW-35 — effects.** Work is an outbox row created in the owner's transaction and completed in the owner's
transaction; completion, reschedule and dead-letter are fenced by the claim token. Kinds relevant here:
`CollectEvidence`, `Dispatch`, `Reconcile`, `Verify`, `Wake`. A claim today selects **by kind only**, which
is a problem the moment two services both own `wake` (section 3.5); this prompt fixes it by scoping claims
to aggregate kinds.

**RW-21 / D-08 — trace schema.** The timeline and the graph render from one contract frozen with golden
fixtures shared by Rust tests and `web/src/test/fixtures`. This prompt freezes it before either view exists.

**D-02 — one journal, no third semantic store.** Operation events are `journal` rows with typed kinds.
**D-21** — operations and their summaries are retained; only raw journal/output rows are pruned (not done here).

## 3. Repository context

Verify each item first. Marks: **(existing, path:line)** or **(new)**.

### 3.1 The unit of work

| What | Where |
|---|---|
| `AggregateKind { Incident, Action, Note }`, `as_str`/`parse`, `AggregateRef` constructors | **(existing, `crates/domovoi-core/src/ports/uow/transaction.rs:15-83`)** |
| `StateWrite { Signal, Incident, IncidentEvent, DiscardPendingIncident, Action, Note }` | **(existing, `transaction.rs:164-202`)** |
| `Transaction { writes, journal, audit, new_effects, completed_effects, rescheduled_effects, dead_effects }`, `is_empty` | **(existing, `transaction.rs:209-243`)** |
| `CommitError { Conflict, Constraint, StaleEffect, Backend }`, `Expected`, `Versioned<T>` | **(existing, `transaction.rs:93-156, 308-333`)** |
| `JournalKind { IncidentTimeline, ActionTransition }`, `JournalDraft`, `AuditDraft`, `JournalRead` | **(existing, `crates/domovoi-core/src/ports/uow/journal.rs:15-161`)** |
| `EffectKind` (11 kinds incl. `BuildContext, CallModel, CollectEvidence, EvaluatePolicy, IssuePermit, Dispatch, Reconcile, Verify, Notify, Wake, DraftSummary`), `EffectDraft`, `EffectCompletion` | **(existing, `crates/domovoi-core/src/ports/uow/effects.rs:19-205`)** |
| `EffectQueue::claim_due(runner, kinds, now, lease, limit)` and `revert_claimed_at_start()` — **kind filter only** | **(existing, `crates/domovoi-core/src/ports/uow/lifecycle.rs:68-85`; SQLite `crates/domovoi-store/src/uow/queue.rs:42-142`)** |
| `claim_due` never reclaims an expired `dispatch` claim; `revert_claimed_at_start` leaves `dispatch` claimed | **(existing, `queue.rs:61-64, 135-141`)** |
| One `BEGIN IMMEDIATE` commit; order: writes, mirrored journal, journal, audit, new effects, completions, reschedules, deaths | **(existing, `crates/domovoi-store/src/uow/mod.rs:38-87`)** |
| SQLite state-write dispatch (`apply`) | **(existing, `crates/domovoi-store/src/uow/writes.rs:106-140`)** |
| In-memory twin | **(existing, `crates/domovoi-testkit/src/repos/uow_apply.rs:21-60`)** |
| Crash-harness write summary (`match` over every `StateWrite`) | **(existing, `crates/domovoi-testkit/src/crash/mod.rs:101-140`)** |
| `EffectStatus::Cancelled` exists but **nothing writes it**: no transaction field cancels a pending effect | **(existing, `uow/effects.rs:99`; grep finds no writer)** |

### 3.2 The effect runner

`EffectRunner::new(deps, settings)`, `register(kind, handler)`, `run()` **(existing, `crates/domovoi-engine/src/effects/runner.rs:45-159`)**.
It claims only the kinds it has handlers for (`:86`). `EffectHandler::handle(&EffectRecord, ClaimToken) -> EffectOutcome { Handled | Retry | Dead }`
**(existing, `effects/mod.rs:24-46`)**. The action service builds its own runner and registers **only
`Wake`** (`VerifyWake`) **(existing, `crates/domovoi-engine/src/actions/mod.rs:243-257`)**. `VerifyWake` returns `Dead { "unreadable
wake payload" }` for any `wake` whose payload is not its own **(existing, `actions/verify_wake.rs:69-73`)**.

### 3.3 Execution today (legacy path, stays untouched)

- `AuthorizedExecution::of(ActionRequest)` and `execute(shared, authorized, skill, args, session, incident)` in
  `crates/domovoi-engine/src/actions/execute.rs:36-123` — the only dispatch path for actions; it builds
  `WriteCommand::Dispatch` once (`:333`), sends with `HandoffRunner::hand_off` (`:250`) and waits with
  `await_handoff_result` (`:271`). Called once from `actions/executor.rs:235-243`.
- Pre-dispatch checks in the writer: `actions/dispatch.rs:20-131` (authorization evidence, freshness, session and
  epoch, enrolled agent, v2 target).
- v2 hash recomputation: `actions/v2_plan.rs:18-75` (`PlanTarget::of_incident`, `PlanContract::skill(name, 1)`,
  `ImplHash::from_parts(manifest_hash, entry_script_digest)`, `PlanHash::compute_v2(.., ExecutionId::from_action(id))`).
- Host wait: `actions/host_wait.rs:31-52` blocks on the clock; operations must not block (section 6.7).
- Reconcile mapping: `actions/reconcile/answer.rs:36-92` (`ReconcileOutcome`, `reason_text`, `from_answer`), all
  `pub(crate)` inside **private** modules (`actions/mod.rs:22 mod reconcile;`, `reconcile/mod.rs:21 mod answer;`).
- Arch rules: `xtask/src/arch/rules.rs:216-220` (`BOUNDARY_PAIRS`), `:278-393` (`BOUNDARY_CARDS`). Cards pin
  `AuthorizedExecution::of` and `execute::execute` to `actions/executor.rs` (1 each), `WriteCommand::Dispatch`
  construction to `actions/execute.rs` and its pattern to `actions/writer.rs`. Raw `run_skill`/`run_capability`
  have no caller; `hand_off`/`await_handoff_result` are **not** counted.

### 3.4 Ports and types this prompt consumes

- `ExecutionId` (UUID newtype) with `from_action`/`as_action` **(existing, `crates/domovoi-core/src/id.rs:88-107`)**; ids are made by
  `define_id!` **(existing, `id.rs:8-58`)**.
- `Authorization` (private representation; `human_approval()`, `policy_permit()`, `kind()`), `PolicyPermit`
  **(existing, `crates/domovoi-core/src/action/authorization.rs:39-100`)**; `HumanApproval` (`restore` for the store),
  `PlanBinding`, `ApprovalChannel`, `ApprovalProvenance`, `PlanHash` **(existing, `action/approval.rs:9-168`)**;
  `PlanContract`, `PlanTarget::new(host, host_key, check, target)`, `ImplHash`, `PlanHash::compute_v2` **(existing, `action/plan.rs:22-183`)**.
- `AgentSnapshot { session, agent, journal_epoch, skills }`, `SkillDispatch { action: ActionId, skill, args, plan_hash }`
  **(existing, `crates/domovoi-core/src/ports/agent_link.rs:27-36, 154-163`)**. The wire `RunSkill.action_id` and
  `ActionStatusQuery.execution_ids` are plain UUIDs **(existing, `crates/domovoi-proto/src/messages/work.rs:122`, `actions.rs:19,181`)**.
- Capability objects: `AvailabilityProbe`, `HostObservation`, `HostRead`, `HandoffRunner`, `JournalStatus`, `CheckRerun`
  **(existing, `crates/domovoi-core/src/ports/caps.rs:32-129`)**; `HostReconciler` **(existing, `ports/reconcile.rs:11`)**, a `OnceCell` hook in
  the agent server (`crates/domovoi-hub/src/agent_server/mod.rs:211`).
- `Evidence::collect_evidence(EvidenceRequest) -> Result<ToolOutput, EvidenceError>`, `EvidenceRequest::for_host`
  **(existing, `crates/domovoi-engine/src/evidence/mod.rs:74, 177`)**; `ReadToolCall::from_json(tool, args)` **(existing, `crates/domovoi-core/src/triage/tool_call.rs:265`)**.
- `SkillSummary::validate_args`, `verify`, `timeout`, `manifest_hash`, `entry_script_digest` **(existing, `skill/manifest.rs:197-227`, `skill/validate.rs:184`)**;
  `HostRepo::live_agent` **(existing, `ports/repos/hosts.rs:51`)**.
- **Introduced by prompt 24 (verify):** `domovoi_core::capability::{ContractId, EffectClass, ContractIndex, CapabilityContract}`, `ContractId::skill(&SkillName)`,
  `skill_name()`, `is_skill()`. **Introduced by prompt 25 (verify):** `domovoi_core::capability::Availability` (variants `Available, NotInstalled,
  DisabledByHub, NotAllowedOnHost, Unavailable, HostUnknown, HostAbsent{since}, HostIncompatible{reported}`; `code()`, `reason_code()`, `is_available()`),
  port `domovoi_core::ports::CapabilityRegistry { availability(host, &ContractId) -> Availability, enabled(..), contracts() }`, port `TargetResolver::resolve(TargetQuery)
  -> Result<ResolvedTarget, TargetError>` with `ResolvedTarget { identity, name, kind, host_key: KeyFingerprint }`.

### 3.5 Why claims must be scoped (a defect this prompt fixes)

With the flag on there are two services that own effects. If the operation service's runner registered `Wake`,
the legacy runner (which also claims `wake`) would claim operation timers and `VerifyWake` would answer `Dead`; the
operation runner would likewise claim legacy verifications. Kind-only claims cannot separate them. Therefore:
`claim_due` and `revert_claimed_at_start` gain an **aggregate-kind scope** (section 6.6), the legacy runner is scoped
to `[Incident, Action, Note]` and the operation runner to `[Operation]`.

### 3.6 Config, hub wiring, web

- `HubConfig` sections, defaults via `#[serde(default)]`, `deny_unknown_fields`, one file per section under
  `crates/domovoi-config/src/hub/` **(existing, `crates/domovoi-config/src/hub/mod.rs:31-84`)**; section names are also listed in
  `crates/domovoi-config/src/env.rs:15-28`; cross-field validation in `crates/domovoi-config/src/validate.rs`. No `orchestration`
  section exists (grep `orchestration` finds only a doc comment in `engine/src/lib.rs`).
- Runtime assembly: `crates/domovoi-hub/src/runtime/mod.rs:174-330`; `ActionService::spawn` at `:231`, `server.set_reconciler` at `:253`;
  engine settings built in `crates/domovoi-hub/src/runtime/settings.rs` (`settings::actions()`).
- Retention key `operation_journal_days` exists but is "not applied yet" **(existing, `crates/domovoi-config/src/hub/retention.rs:29-32`)**; do not apply it here.
- Web: `web/src/test/fixtures.ts` is an existing **file**; `web/tsconfig.json` has `resolveJsonModule`; `web/eslint.config.js:42` caps files at 250 lines.
- Core snapshot tests: `crates/domovoi-core/tests/serde_snapshots.rs` with `tests/snapshots/`; `insta` is a dev-dependency of core and engine.
- Engine test binaries are declared in `crates/domovoi-engine/Cargo.toml` (`autotests = false`, `[[test]]` per binary).

## 4. Exact scope

### 4.1 `domovoi-core` (no I/O)

| File | Change |
|---|---|
| `src/id.rs` | **(new)** `OperationId`, `StepId` via `define_id!`; `ExecutionId::from_step(StepId)`, `ExecutionId::as_step(self) -> StepId` |
| `src/actor.rs` | **(new)** `pub const SYSTEM_OPERATIONS: &str = "operations";` re-exported |
| `src/operation/mod.rs` | **(new)** module doc and re-exports |
| `src/operation/status.rs` | **(new)** `OperationStatus`, `PauseReason`, `WaitKind`, `OperationKind`, `FinishOutcome` |
| `src/operation/operation.rs` | **(new)** `Operation`, `PinnedSubject` |
| `src/operation/step.rs` | **(new)** `Step`, `StepKind`, `StepIntent` and its per-kind structs, `StepOutcome`, `StepProgress`, `StepUsage` |
| `src/operation/edge.rs` | **(new)** `StepEdge`, `EdgeKind` |
| `src/operation/execution.rs` | **(new)** `ExecutionRecord`, `ExecutionApproval`, `ExecutionState`, `ExecutionError`; `ExecutionRecord::authorize`/`state` |
| `src/operation/budget.rs` | **(new)** `Budgets`, `BudgetLimits`, `BudgetUsed`, `BudgetKind` |
| `src/operation/snapshot.rs` | **(new)** `OperationSnapshot { operation, version, steps, edges, executions, incidents }` |
| `src/operation/trace.rs` | **(new)** `OperationTrace`, `TraceOperation`, `TraceStep`, `TraceEdge`, `TraceRef`, `build_trace` (pure) |
| `src/ports/uow/operations.rs` | **(new)** `OperationWrite`, `ExecutionGuard`, `StepGuard` |
| `src/ports/uow/transaction.rs` | `AggregateKind::Operation` (+ `AggregateRef::operation`, `as_str` `"operation"`, `parse`); `StateWrite::Operation(OperationWrite)`; `Transaction::cancelled_effects` + `cancel_effects` builder + `is_empty` |
| `src/ports/uow/lifecycle.rs` | `EffectCancel`; `EffectQueue::claim_due` and `revert_claimed_at_start` take an aggregate scope (section 6.6) |
| `src/ports/uow/journal.rs` | six new `JournalKind` variants (section 6.4) |
| `src/ports/repos/operations.rs` | **(new)** `OperationRepo`, `OperationFilter` |
| `src/ports/repos/mod.rs`, `src/ports/uow/mod.rs`, `src/ports/mod.rs`, `src/lib.rs` | register and re-export; `pub mod operation;` and the shared-contract names at the crate root |
| `tests/operation_trace.rs`, `tests/snapshots/operation_trace__*.snap` | **(new)** trace builder, snapshots and fixture comparison (section 12.1) |

### 4.2 `domovoi-store`

| File | Change |
|---|---|
| `migrations/NNNN_operations.sql` | **(new)** the tables of section 8. `NNNN` is the next free number in that directory on your branch (list it; never hard-code) |
| `src/repos/operations.rs`, `src/repos/operations_rows.rs` | **(new)** `OperationRepo` for `Store`; row decoding (`HumanApproval::restore` is allowed here) |
| `src/uow/operations.rs` | **(new)** applies `OperationWrite` |
| `src/uow/writes.rs` | one new match arm calling `operations::apply` |
| `src/uow/effects.rs`, `src/uow/queue.rs`, `src/uow/mod.rs` | `cancel`, the aggregate scope on claim/revert, apply `cancelled_effects` |
| `tests/sqlite/operations.rs`, `tests/sqlite/main.rs`, `tests/sqlite/contracts.rs` | **(new)** SQLite runs of the new contract suites, the N-1 upgrade test, trigger and constraint tests |

### 4.3 `domovoi-testkit`

`src/repos/operations.rs` and `src/repos/uow_operations.rs` **(new)** (in-memory twin of repo and writes, including the
step ordinal rule, the immutability rule and the execution/action id check); `src/repos/uow_queue.rs` and
`uow.rs` take the aggregate scope and `EffectCancel`; `src/contracts/operations.rs` **(new)** the suites of section 12.2; `tests/contracts.rs`
registers them; `src/crash/mod.rs` summarises `StateWrite::Operation` (one string per `OperationWrite`, for example `operation:new`,
`step:new`, `step:progress`, `edge`, `execution:new`, `execution:version`, `incident_link`).

### 4.4 `domovoi-engine`

New module `src/operations/` (each file ≤ 500 lines; split as listed):

| File | Contents |
|---|---|
| `mod.rs` | module doc; `OperationService::spawn`, `OperationHandle`, re-exports |
| `state.rs` | `OperationState` (a loaded `OperationSnapshot` with lookups) |
| `decide/mod.rs`, `decide/events.rs`, `decide/steps.rs`, `decide/execution.rs`, `decide/verify.rs`, `decide/finish.rs`, `decide/status.rs` | the pure function (section 6.2) |
| `settings.rs` | `OperationSettings` and defaults |
| `deps.rs` | `OperationDeps` (ports only) |
| `writer/mod.rs`, `writer/commands.rs`, `writer/commit.rs`, `writer/execution.rs`, `writer/dispatch.rs` | the one writer task and its commands (section 6.3). `writer/mod.rs` holds the `process` match, the only place `OperationCommand::Dispatch { .. }` is matched; `writer/dispatch.rs` holds the checks it calls |
| `execute.rs` | `AuthorizedStep`, `execute_step`, `ExecuteError` — the operation dispatch boundary (section 6.5) |
| `effects/mod.rs`, `effects/dispatch.rs`, `effects/evidence.rs`, `effects/wake.rs`, `effects/verify.rs`, `effects/reconcile.rs` | the handlers (section 6.7) |
| `audit.rs`, `journal.rs` | audit kind constants and journal payload builders |
| `lib.rs` | `pub mod operations;` and re-exports |
| `actions/mod.rs`, `actions/reconcile/mod.rs` | **narrowest `pub(crate)` visibility change** so `ReconcileOutcome`, `from_answer`, `reason_text` are reachable; no other legacy edit |
| `actions/mod.rs` | the legacy runner is scoped to the legacy aggregates (`runner.scoped(AggregateKind::LEGACY)`), one line |
| `effects/runner.rs`, `effects/mod.rs` | `EffectRunner::scoped(Vec<AggregateKind>)`; default scope is `AggregateKind::LEGACY` |
| `Cargo.toml` | `[[test]] name = "operations" path = "tests/operations/main.rs"` |
| `tests/operations/` | **(new)** `main.rs`, `harness.rs`, `decide.rs`, `writer.rs`, `execution.rs`, `effects.rs`, `restart.rs`, `sim.rs` (the simulator), `properties.rs` |
| `tests/gates.rs` | extend (section 12.5) |

### 4.5 `domovoi-config`, `domovoi-hub`, `xtask`, `web`, docs

- config: `hub/orchestration.rs` **(new)** `OrchestrationSection`, `OrchestrationBudgets`; `hub/mod.rs` adds the field and re-export; `env.rs` adds `"orchestration"` to the section list; `validate.rs` rules (section 6.8).
- hub: `runtime/orchestration.rs` **(new)** builds and spawns `OperationService` **only when `config.orchestration.enabled`**, from values the runtime already holds: the one `Evidence`, the capability objects (`action_caps`, `observer_of`), the registry and the target service that prompt 25's runtime builds (verify their names in `runtime/mod.rs`; build no second copy), the store as `OperationRepo`/`UnitOfWork`/`EffectQueue`/`EffectRead`, the redactor and the clock; `runtime/settings.rs` adds `operations(&config)`;
  `runtime/mod.rs` calls it after `ActionService::spawn`; a `CompositeReconciler` (new, hub) fans `reconcile_host` out to the action handle and the operation handle and is the one value given to `server.set_reconciler`.
- xtask: `arch/rules.rs` cards for the operation path; `arch/ast_tests.rs` unit tests (section 6.5, 12.5).
- web: `web/src/api/operationTrace.ts` **(new)** hand-written TypeScript types for the trace; `web/src/test/fixtures/operation-trace/*.json` **(new)** the golden fixtures; `web/src/test/fixtures/operation-trace/trace.test.ts` **(new)**.
- docs: `docs/adr/NNNN-operation-aggregate.md` **(new, required because `xtask/src/arch/rules.rs` changes)**, `ARCHITECTURE.md`, `docs/book/src/concepts/operations.md` **(new)** + `SUMMARY.md`, regenerated `docs/book/src/reference/configuration.md`.

## 5. Explicit non-goals

| Not here | Owner / reason |
|---|---|
| Triage as an Investigate operation, `BuildContext`/`CallModel`/`DraftSummary`/`Notify` handlers, model turns that propose steps, the parity suite | 27 (31 for meta-tools). 26 may **create** `BuildContext` effects (new cycle); with no handler registered they stay `pending` and are never claimed |
| `POST /operations/{id}/steps/{step}/approve`, approval cards, the legacy projection, any HTTP route or OpenAPI change | 27, 30. 26 adds **no HTTP endpoint** and does not run `cargo xtask openapi` |
| SSE, `as_of_seq`, topics, the `incident_events` copy | 28 |
| Use-case services, the hub module-edge rule | 29 |
| Policy evaluation, `PolicyDecision` steps, shadow decisions, permits, `PolicyPermit` issuance, unattended execution | 39, 40, 45. Every execution in 26 needs a human. The `PolicyDecision` kind exists and is part of the trace contract, but no code path in this prompt creates one |
| Executing any contract that is not `skill.<name>` (built-in mutations, helper operations, `run_capability`) | later. A non-skill contract is stored `Refused` at proposal (and would settle `Unavailable("contract_not_executable")` if it ever reached dispatch) |
| Removing the legacy executor, `recheck.rs`, the triage queue | 58 |
| Operation journal pruning (`operation_journal_days`) | 57's key stays "not applied yet"; note it in the PR |
| The agent-side mutation limiter, the unattended-reuse guard after `OutcomeUnknown` | the limiter is prompt 20's (RW-38); the unattended guard is 45. 26 has only the human path |
| Editing a merged migration, the wire, `PROTOCOL_VERSION`, `Hello`, any `domovoi-agent`/`domovoi-proto` file | forbidden (RW-42) |
| New dependencies | none: use `serde`, `serde_json`, `jiff`, `uuid`, `thiserror`, `async-trait`, `tokio`, `tracing`, `hex`, `sha2` already in the crates |

### 5.1 Decisions this prompt takes because the design documents are silent

The executor must not revisit these; each is specified where it is used.

| Decision | Why it is needed | Where |
|---|---|---|
| Effect claims are scoped by aggregate kind (`claim_due`, `revert_claimed_at_start` take `aggregates`) | two services both own `wake`; claims select by kind only (3.5) | 6.6 |
| A new `Transaction::cancelled_effects` / `EffectCancel` | RW-35 says `pending → cancelled`, but no code path writes `cancelled` | 6.6 |
| A second dispatch site, `operations/execute.rs`, beside the legacy one, pinned by arch cards | RW-08 wants one function, but the legacy executor must keep running unchanged until it is removed | 6.5 |
| Every execution needs a human; no `PolicyDecision` step is created | the evaluator (39) and shadow decisions (40) are not wired; permits are 45 | 5, 6.2 |
| Only `skill.<name>` contracts execute | no other executable implementation exists (`run_capability` has no caller) | 6.5 |
| `tool_calls` is not a budget counter (derive it); budgets are the seven of 6.1 | 04 lists eight, two overlap | 6.1 |
| `steps.output` column holds a bounded redacted output; the trace never shows it | the next cycle's context needs raw evidence, retention (D-21) prunes raw outputs | 8 |
| Host identity for the plan hash comes from `TargetResolver`, and `PlanTarget` keeps its merged shape (host, key, check, target) | consume 25 without changing 22's hash | 6.5 |
| The loop guard counts from the store (`dispatched_since`) | nothing to reload at start | 6.5 |
| A refused execution step has no `executions` row | there is nothing to dispatch or approve | 6.2 |
| Step born-outcomes per kind; `Verification`, `Approval`, `Execution`, `PolicyDecision` cannot be proposed generically | keeps the writer the only creator of the safety-relevant steps | 6.2 |
| No HTTP route is added | read and approve endpoints belong to 27 and 30; the engine and store expose the read API | 5 |

## 6. Required interfaces and contracts

Marks: **fixed** = names, variants, JSON and semantics must match; **convention** = follow repository style.
Every public item gets a doc comment that says what it is for. Remarks in the sketches that name another prompt ("introduced by 24") are notes to you:
never copy a prompt number, "the build plan" or an RW/D id into code, doc comments, commit messages, the ADR, the book or the PR description (they describe behaviour in plain words).

### 6.1 Core types **fixed** (names, variants, serde names); helpers **convention**

```rust
// crates/domovoi-core/src/id.rs
define_id!(/** Identifies an operation. */ OperationId);
define_id!(/** Identifies one step of an operation; for an Execution step it is also its execution id. */ StepId);
impl ExecutionId {
    pub fn from_step(step: StepId) -> Self;   // same UUID
    pub fn as_step(self) -> StepId;           // same UUID
}
```

```rust
// crates/domovoi-core/src/operation/status.rs
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationStatus {
    Queued, Running, WaitingApproval, WaitingUser, WaitingAgent, WaitingTimer,
    Paused { reason: PauseReason },
    Succeeded, Failed, Escalated, Cancelled,
}
impl OperationStatus {
    pub const NAMES: [&'static str; 11];      // "queued" "running" "waiting_approval" "waiting_user" "waiting_agent"
                                              // "waiting_timer" "paused" "succeeded" "failed" "escalated" "cancelled"
    pub const fn name(&self) -> &'static str;
    pub fn is_terminal(&self) -> bool;        // Succeeded | Failed | Escalated | Cancelled
    pub fn is_waiting(&self) -> bool;         // the four Waiting*
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PauseReason {
    Budget { exhausted: BudgetKind },
    OutcomeUnknown { step: StepId },
    GlobalPause,                               // reserved for M8; nothing in this prompt produces it
    Error { message: String },                 // an error the writer could not resolve
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum WaitKind { Approval, User, Agent, Timer }       // priority order, highest first
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum OperationKind { Investigate }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum FinishOutcome { Succeeded, Failed, Escalated }
```

The status column stores `name()`; a `Paused` row also stores the `PauseReason` JSON in `pause_reason`. Names are
validated in Rust when read, never by SQL `CHECK`.

```rust
// crates/domovoi-core/src/operation/operation.rs   (public fields; only the writer builds new states)
pub struct Operation {
    pub id: OperationId, pub kind: OperationKind,
    pub goal: String,                      // 1..=500 chars, already redacted by the caller
    pub status: OperationStatus, pub cycle: u32, pub budgets: Budgets,
    pub pinned: Vec<PinnedSubject>,        // 1..=8; the only targets an execution may name (RW-23)
    pub parent: Option<OperationId>,
    pub created_at: Timestamp, pub started_at: Option<Timestamp>, pub ended_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}
pub struct PinnedSubject { pub host: HostId, pub check: CheckKey, pub target: Option<TargetKey> }
```

```rust
// crates/domovoi-core/src/operation/step.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind { ReasoningTurn, EvidenceRequest, Assessment, Plan, PolicyDecision, Approval,
                    Execution, Verification, Notification, Question, Note }
impl StepKind { pub const ALL: [StepKind; 11]; pub const fn as_str(self) -> &'static str; pub fn parse(text: &str) -> Option<Self>; }

/// The single outcome vocabulary (04 §3.2). Stored in `steps.status` as a derived projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome { Pending, Running, Succeeded, Failed, Refused, NotRun, Unavailable,
                       OutcomeUnknown, Skipped, Cancelled }
impl StepOutcome { pub const ALL: [StepOutcome; 10]; pub const fn as_str(self) -> &'static str;
                   pub fn parse(text: &str) -> Option<Self>; pub fn is_final(self) -> bool; /* neither Pending nor Running */ }

pub struct Step {                          // immutable once inserted
    pub id: StepId, pub operation: OperationId,
    pub ordinal: u32,                      // 0-based insertion order within the operation; unique; edges point to a lower ordinal
    pub kind: StepKind, pub phase: String, // phase label, 1..=40 chars of [a-z0-9_-]
    pub parent: Option<StepId>,            // grouping only; must have a lower ordinal
    pub cycle: u32, pub intent: StepIntent, pub created_at: Timestamp,
}
pub struct StepProgress {                  // the derived projection
    pub outcome: StepOutcome, pub started_at: Option<Timestamp>, pub ended_at: Option<Timestamp>,
    pub summary: Option<String>,           // <= 280 chars, redacted
    pub reason: Option<String>,            // <= 280 chars; why a Refused / NotRun / Unavailable / Failed / OutcomeUnknown
    pub output: Option<String>,            // <= 16 KiB, redacted; never copied into the trace or a journal row
}
pub struct StepRecord { pub step: Step, pub progress: StepProgress }
pub struct StepUsage { pub input_tokens: u32, pub output_tokens: u32, pub cost_micros: Option<u64> }
```

`StepIntent` (internally tagged by `kind`, snake_case; **fixed** variant names and the fields shown; later prompts add
`#[serde(default)]` fields only):

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepIntent {
    ReasoningTurn { context_refs: Vec<String> },
    EvidenceRequest { host: HostId, tool: String, args: serde_json::Value },   // validated with ReadToolCall::from_json at insert
    Assessment { text: String },
    Plan { text: String },
    PolicyDecision { execution: StepId },                                       // never created in this prompt
    Approval { execution: StepId, plan_hash: PlanHash, expires_at: Timestamp },
    Execution(ExecutionIntent),
    Verification { execution: StepId, delay_secs: u64 },
    Notification { channel: String, subject: String },
    Question { text: String, expires_at: Timestamp },
    Note { text: String, author: Actor },
}
pub struct ExecutionIntent {
    pub contract: ContractId,              // introduced by 24; `skill.<name>` is the only executable family here
    pub contract_version: u32,
    pub host: HostId, pub check: CheckKey, pub target: Option<TargetKey>,
    pub args: BTreeMap<String, String>,
    pub retry_of: Option<RetryOf>,
}
pub struct RetryOf { pub step: StepId, pub changed: String }                   // `changed` is 1..=280 chars
```
`Assessment.text`, `Plan.text`, `Question.text` and `Note.text` are 1..=2000 chars; `EvidenceRequest.tool` is a `ReadTool` name.
If `ContractId` has no serde support in prompt 24's merged code, serialise its string form.

```rust
// crates/domovoi-core/src/operation/edge.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum EdgeKind { DependsOn, CausedBy, Verifies, RetryOf, Answers }          // + ALL, as_str, parse
pub struct StepEdge { pub operation: OperationId, pub from: StepId, pub to: StepId, pub kind: EdgeKind }
```
Edge rules (decide and the writer check all of them; the store re-checks existence, same operation and the ordinal rule): both steps
exist in the same operation; `to.ordinal < from.ordinal`; `from != to`; `verifies` goes Verification → Execution; `retry_of` goes
Execution → Execution; `answers` goes Note → Question; `depends_on` and `caused_by` go between any two steps.

```rust
// crates/domovoi-core/src/operation/budget.rs
pub struct BudgetLimits { pub model_turns: Option<u32>, pub evidence_requests: Option<u32>, pub executions: Option<u32>,
                          pub cycles: Option<u32>, pub wall_seconds: Option<u64>, pub tokens: Option<u64>,
                          pub cost_micros: Option<u64> }               // None = unlimited
pub struct BudgetUsed { pub model_turns: u32, pub evidence_requests: u32, pub executions: u32,
                        pub wall_seconds: u64, pub tokens: u64, pub cost_micros: u64 }   // cycles come from Operation::cycle
pub struct Budgets { pub limits: BudgetLimits, pub used: BudgetUsed }
#[serde(rename_all = "snake_case")]
pub enum BudgetKind { ModelTurns, EvidenceRequests, Executions, Cycles, WallTime, Tokens, Cost }
impl Budgets {
    /// Counted kinds (ModelTurns, EvidenceRequests, Executions, Cycles): true when one more item would pass the limit,
    /// that is `limit == Some(l) && used >= l` (a limit of N allows exactly N items; for Cycles `used` is `cycle`).
    pub fn is_spent(&self, kind: BudgetKind, cycle: u32) -> bool;
    /// Measured kinds (WallTime, Tokens, Cost): the first whose `used > limit`, else None.
    pub fn overrun(&self) -> Option<BudgetKind>;
    pub fn charge(&mut self, kind: BudgetKind, amount: u64);
    pub fn extend(&mut self, kind: BudgetKind, new_limit: Option<u64>);
}
```
"Tool calls" from `04 §4` is **not** a separate counter: it equals `evidence_requests + executions`, derived on read when a view
wants it. `Budgets` serialises as `{"limits": {...}, "used": {...}}` into one JSON column.

```rust
// crates/domovoi-core/src/operation/execution.rs
pub struct ExecutionRecord {               // the 1:1 detail row; `execution == ExecutionId::from_step(step)` always
    pub execution: ExecutionId, pub operation: OperationId, pub step: StepId,
    pub host: HostId, pub contract: ContractId, pub contract_version: u32,
    pub check: CheckKey, pub target: Option<TargetKey>, pub args: BTreeMap<String, String>,
    pub plan_hash: PlanHash,               // v2: contract, implementation, target, args, execution id
    pub expires_at: Timestamp,             // end of the approval window
    approval: Option<ExecutionApproval>,   // private: set only by `authorize` or `restore`
    pub waiting_since: Option<Timestamp>,  // first time a dispatch had to wait for the agent
    pub dispatched: Option<(JournalEpoch, Timestamp)>,
    pub settled_at: Option<Timestamp>,
    pub created_at: Timestamp, pub updated_at: Timestamp,
}
pub struct ExecutionApproval { approval: HumanApproval, displayed: PlanHash, provenance: ApprovalProvenance }
impl ExecutionRecord {
    pub fn propose(intent: &ExecutionIntent, step: StepId, operation: OperationId, plan_hash: PlanHash,
                   expires_at: Timestamp, now: Timestamp) -> Self;
    /// Mirrors `ActionRequest::approve`: a human authorization (a policy permit is `NotYetSupported`), a binding that
    /// names `plan_hash`, no earlier approval, and `now < expires_at`.
    pub fn authorize(&mut self, authorization: Authorization, provenance: ApprovalProvenance,
                     binding: &PlanBinding, now: Timestamp) -> Result<(), ExecutionError>;
    pub fn approval(&self) -> Option<&ExecutionApproval>;
    pub fn state(&self, step: StepOutcome, approval_step: Option<StepOutcome>) -> ExecutionState;
    pub fn restore(parts: ExecutionParts) -> Self;      // store only; records no decision
}
pub enum ExecutionState { Proposed, AwaitingApproval, Authorized, Dispatched, Refused, NotRun, Unavailable,
                          Succeeded, Failed, OutcomeUnknown, Skipped, Cancelled }
pub enum ExecutionError { AlreadyAuthorized, Expired, PlanChanged, NotYetSupported }   // thiserror, short plain text
```
`state()` is **derived**; there is no stored execution status. A final step outcome maps to the same-named state; `Running` is
`Dispatched`; `Pending` with a stored approval is `Authorized`; `Pending` without one is `AwaitingApproval` when the approval step is
`Pending`, else `Proposed`. `HumanApproval::restore` is called only in `crates/domovoi-store/src/repos/operations_rows.rs`
(the INV-1 scan already allows `crates/domovoi-store/src/`).

### 6.2 `decide`, the pure function **fixed** (signature, events, rules)

```rust
// crates/domovoi-engine/src/operations/decide/mod.rs
pub trait IdSource { fn step(&mut self) -> StepId; fn effect(&mut self) -> EffectId; }   // UUIDv7 in the writer, counters in tests
pub fn decide(state: &OperationState, event: OperationEvent, now: Timestamp, ids: &mut dyn IdSource)
    -> Result<Decision, DecideError>;
```
`OperationState` is a loaded `OperationSnapshot` plus lookups (`step(id)`, `execution_of(step)`, `open_steps()`). `decide` does no I/O,
reads no clock, uses no randomness and never mutates `state`.

```rust
pub struct Decision {
    pub operation: Operation,                      // the new state; equals the old one when nothing changed
    pub steps: Vec<StepRecord>,                    // new steps, ordinals = existing count + index
    pub edges: Vec<StepEdge>,
    pub executions: Vec<ExecutionRecord>,          // new rows only (updates are applied by the writer, 6.3)
    pub progress: Vec<ProgressChange>,             // { step, from: StepOutcome, to: StepProgress }
    pub effects: Vec<EffectSpec>,                  // { id, kind, payload, not_before, max_attempts, dedup_key }; aggregate = the operation
    pub journal: Vec<JournalNote>,                 // { kind: JournalKind, payload: Value }
    pub audit: Vec<AuditNote>,                     // { kind: &'static str, subject: String, detail: Value }
}
pub enum DecideError { OperationClosed, Paused, UnknownStep(StepId), KindNotProposable(StepKind), BadEdge(String),
                       StepNotOpen(StepId), BudgetStillSpent(BudgetKind), ExecutionInFlight(StepId),
                       Invalid(String) }                                                // thiserror
```

Events (**fixed** list; each carries only ids and plain facts, never a port):

```rust
pub enum OperationEvent {
    Admit,                                                                   // Queued -> Running
    Propose { steps: Vec<NewStep>, edges: Vec<NewEdge> },                    // non-execution steps
    ProposeExecution(ExecutionProposal),
    StepStarted { step: StepId },
    StepFinished { step: StepId, outcome: StepOutcome, summary: Option<String>, reason: Option<String>,
                   output: Option<String>, usage: Option<StepUsage> },
    ExecutionAuthorized { step: StepId },                                    // the writer already ran ExecutionRecord::authorize
    ExecutionDenied { step: StepId },
    ExecutionExpired { step: StepId },
    WaitingForAgent { step: StepId },
    ExecutionDispatched { step: StepId, epoch: JournalEpoch, reconcile_after: Timestamp },
    ExecutionSettled { step: StepId, outcome: StepOutcome, reason: Option<String>, output: Option<String>,
                       verify_after: Option<SignedDuration> },
    Verified { step: StepId, passed: bool, summary: String },
    Answer { question: StepId, note: NewStep },
    Cancel { reason: String }, Pause { reason: PauseReason }, Resume { extend: Vec<(BudgetKind, Option<u64>)> },
    Finish { outcome: FinishOutcome, summary: String },
    Tick,                                                                    // time passed: wall-time check
}
pub struct NewStep { pub id: StepId, pub kind: StepKind, pub phase: String, pub parent: Option<StepId>, pub intent: StepIntent }
pub struct NewEdge { pub from: StepId, pub to: StepId, pub kind: EdgeKind }
pub struct ExecutionProposal {
    pub intent: ExecutionIntent, pub step: StepId, pub approval_step: StepId,
    pub parent: Option<StepId>, pub caused_by: Option<StepId>,
    pub plan_hash: Option<PlanHash>,            // None only when `refused` is Some
    pub expires_at: Timestamp,
    pub refused: Option<String>,                // a reason the writer already found (bad args, unknown skill, non-skill contract)
}
```

**Operation status transitions** (a pair not listed is `DecideError::OperationClosed`, or `Paused` while paused):

| From | Event | To | Notes |
|---|---|---|---|
| `Queued` | `Admit` | `Running` | sets `started_at`; creates one `Wake{purpose: "wall_time"}` effect due at `started_at + wall limit` when a wall limit exists |
| `Queued` | `Cancel` | `Cancelled` | |
| `Running`, `Waiting*` | any event that changes the open steps | re-derived (below) | `derive_status` runs last |
| `Running`, `Waiting*` | `Pause`; a spent budget; `ExecutionSettled{OutcomeUnknown}` | `Paused{reason}` | OutcomeUnknown pauses with `PauseReason::OutcomeUnknown{step}` because the execution mutates |
| `Paused` | `Resume` | `Running` | refused with `BudgetStillSpent` while the paused-for budget is still spent after applying `extend` |
| `Running`, `Waiting*`, `Paused` | `Cancel` | `Cancelled` | see the `Cancel` rule below |
| `Running`, `Waiting*` | `Finish{Succeeded}` | `Succeeded` | refused with `ExecutionInFlight` while any Execution step is `Running` |
| same | `Finish{Failed}`, `Finish{Escalated}` | `Failed`, `Escalated` | same refusal |
| `Running`, `Waiting*` | `Verified{passed: true}` | `Succeeded` | only when no step is `Pending` or `Running` afterwards; otherwise the operation stays `Running` |
| `Running`, `Waiting*` | `Verified{passed: false}` | same status, `cycle + 1` | or `Escalated` when `Budgets::is_spent(Cycles, cycle)` (the cycle limit) |
| terminal | `ExecutionSettled`, `StepFinished`, `Verified` for a step that is still `Running` | unchanged | the truth is recorded (a dispatched run cannot be recalled); no new steps or effects are created |

`derive_status` (while the status is `Running` or a `Waiting*`): `WaitingApproval` when an Approval step is `Pending`; else
`WaitingUser` when a Question step is `Pending`; else `WaitingAgent` when an Execution step is `Pending`, has an approval and a
`waiting_since`; else `WaitingTimer` when a Verification step is `Pending`; else `Running`. The first match wins
(Approval > User > Agent > Timer). All waits stay visible as steps; `build_trace` lists them in `waiting_for`.

**Per-event rules** (every refusal is a `DecideError` and changes nothing):

| Event | Rule |
|---|---|
| `Propose` | refuses `Execution`, `Approval`, `Verification` and `PolicyDecision` steps with `KindNotProposable`. New steps get `cycle = operation.cycle` and the next ordinal. Born outcomes: `ReasoningTurn` → `Running` (started now); `EvidenceRequest`, `Question`, `Notification` → `Pending`; `Assessment`, `Plan`, `Note` → `Succeeded` (started = ended = now). If `is_spent(EvidenceRequests)`, an `EvidenceRequest` is stored `Refused{reason: "evidence budget spent"}` and the operation becomes `Paused{Budget{EvidenceRequests}}`; otherwise `evidence_requests += 1` and a `CollectEvidence` effect is created (`max_attempts 3`, `dedup_key collect:<step>`, due now). A `Question` also creates `Wake{purpose: "question_timeout"}` due at its `expires_at`. Every edge must satisfy the 6.1 edge rules, else `BadEdge` |
| `ProposeExecution` | `refused = Some(r)`: store the Execution step `Refused{reason: r}`, nothing else. Otherwise (1) `(host, check, target)` must equal a pinned subject, else `Refused{"target is not pinned to this operation"}`; (2) **no blind retry**: when an earlier Execution step of this operation has the same `(contract, host, check, target, args)` and its outcome is `Failed` or `OutcomeUnknown`, the new step is stored `Refused{"the same run failed earlier; say what changed"}` unless `retry_of` names such a step and `changed` is non-empty, in which case a `retry_of` edge is added; (3) `is_spent(Executions)` stores `Refused{"execution budget spent"}` and pauses with `Budget{Executions}`. A **refused** Execution step has no `executions` row and no Approval step. Otherwise `executions += 1` and the decision holds: an `Approval` step (`Pending`, created first so it has the lower ordinal), the `Execution` step (`Pending`), a `depends_on` edge Execution → Approval, an optional `caused_by` edge Execution → `caused_by`, the `ExecutionRecord`, a `Wake{purpose: "approval_expiry"}` effect due at `expires_at` (`dedup_key wake:expiry:<step>`) and audit `execution.proposed` |
| `StepStarted` | `Pending` → `Running` with `started_at = now`; on a step already `Running` it is a no-op (empty decision) |
| `StepFinished` | the step must be `Running` or `Pending` and the outcome final; sets `ended_at`; for a `ReasoningTurn` with `usage`: `model_turns += 1`, `tokens += input + output`, `cost_micros += cost`; then the measured-budget check (`overrun()` pauses). `Execution`, `Approval` and `Verification` steps are refused here (`Invalid`): they have their own events |
| `ExecutionAuthorized` | Approval step `Succeeded`; journal `execution`; audit `execution.approved` (actor: the human; detail: 8-hex plan hash, session ref, client); one `Dispatch` effect (`max_attempts 5`, `dedup_key dispatch:<step>`, due now) |
| `ExecutionDenied` | Approval `Refused`, Execution `Refused{"denied by a person"}`; audit `execution.denied` |
| `ExecutionExpired` | Approval `NotRun`, Execution `NotRun{"the approval expired before the action was dispatched"}`; audit `execution.expired` |
| `WaitingForAgent` | no outcome change; the writer sets `waiting_since`; `derive_status` shows `WaitingAgent` |
| `ExecutionDispatched` | Execution `Pending` → `Running`; audit `execution.dispatched`; one `Reconcile` effect (`dedup_key reconcile:<step>`, `not_before = reconcile_after`) |
| `ExecutionSettled` | allowed outcomes: from `Pending`: `Refused`, `NotRun`, `Unavailable`; from `Running`: `Succeeded`, `Failed`, `OutcomeUnknown`, `NotRun`. Any other pair is `Invalid`. `Succeeded` with `verify_after = Some(d)` creates a `Verification` step (`Pending`, phase `verify`), a `verifies` edge Verification → Execution and a `Verify` effect due at `now + d` (`dedup_key verify:<step>`). `OutcomeUnknown` pauses the operation. Audit `execution.<outcome>` |
| `Verified` | the Verification step must be `Pending` or `Running`; `passed` → `Succeeded`, else `Failed` and a new cycle: `cycle += 1` (or `Escalated` at the cycle limit), journal `status`, one `BuildContext` effect (`payload {"v":1,"operation":..,"cycle":N}`, `dedup_key build:<operation>:<N>`) |
| `Answer` | `question` is a `Pending` Question; `note` is a `Note` step; edge `answers` Note → Question; Question `Succeeded` |
| `Cancel` | every `Pending` step → `Cancelled`; every `Running` step except a dispatched Execution → `Cancelled`; a `Running` Execution stays `Running` and is settled later by reconciliation; `ended_at` set; audit `operation.cancelled`; the writer adds an `EffectCancel` |
| `Finish` | every `Pending` step and every `Running` non-execution step → `Skipped`; status per the table; the writer adds an `EffectCancel` |
| `Pause`, `Resume` | `Pause` is refused when terminal; `Resume` applies `extend` through `Budgets::extend` and re-checks; audit `operation.paused`, `operation.resumed` |
| `Tick` | sets `used.wall_seconds = now - started_at`; `overrun()` pauses with `Budget{WallTime}` |
| any event adding a step or edge | `OperationClosed` when terminal, `Paused` when paused (`Resume`, `Cancel`, `ExecutionSettled` and `StepFinished` of an already-`Running` step excepted) |

`decide` also fills `journal` with one note per change, using the kinds and payloads of 6.4, and always sets `operation.updated_at = now`.

### 6.3 `UnitOfWork` additions and the writer **fixed** (types), **convention** (task layout)

```rust
// crates/domovoi-core/src/ports/uow/operations.rs
pub enum OperationWrite {
    Operation { operation: Operation, expected: Expected },          // New | Version(n); the store bumps `version`
    Step(Step),                                                       // insert only; ordinals unique per operation
    Progress { step: StepId, from: StepOutcome, to: StepProgress },   // guarded by the stored outcome name
    Edge(StepEdge),                                                   // insert; the ordinal rule is checked by the store
    Execution { execution: ExecutionRecord, expected: ExecutionGuard },
    IncidentLink { operation: OperationId, incident: IncidentId },
}
pub enum ExecutionGuard { New, Version(u64) }
```
`StateWrite::Operation(OperationWrite)` is the single new `StateWrite` variant. `AggregateRef::operation(id)` is the aggregate of every
operation journal entry and effect (step ids travel in the payload). `CommitReceipt::versions` reports `(AggregateRef::operation(id), new_version)`
for the operation row; execution row versions are not reported (the writer re-reads).

**The writer.** One tokio task (`writer/mod.rs`) with a bounded command channel, like `actions/writer.rs:34-155`. A command
(1) loads the operation snapshot from `OperationRepo` (fresh every time; an in-memory copy is never trusted), (2) applies the pure
`ExecutionRecord` method if there is one, (3) calls `decide`, (4) builds **one** `Transaction` and commits it, (5) answers the caller. On
`CommitError::Conflict` it reloads and re-decides **once**; a second conflict fails the command with `OperationError::Conflict` and logs
`tracing::error!` (no new advisory kind). After any error the in-memory state is dropped.

`Decision` → `Transaction` (order inside `writes`): the operation (with `Expected::Version`), steps (ordinal order), `Progress` writes, edges, execution rows
(new, then updated), `IncidentLink`s. `journal` gets one `JournalDraft` per `JournalNote` (actor = the command's actor, `occurred_at = now`, `payload_v = 1`);
`audit` one `AuditDraft` per `AuditNote`; `new_effects` one `EffectDraft` per `EffectSpec`. When the command carries a held effect, its completion
is added **in the same transaction**. A terminal transition, `Finish` or `Cancel` adds `Transaction::cancel_effects(EffectCancel { aggregate, at })`.

```rust
pub struct OperationSeed { pub kind: OperationKind, pub goal: String, pub pinned: Vec<PinnedSubject>, pub incidents: Vec<IncidentId>,
                           pub parent: Option<OperationId>, pub budgets: Option<BudgetLimits>, pub first_steps: Vec<NewStep>,
                           pub effects: Vec<(EffectKind, serde_json::Value)>, pub actor: Actor }
pub struct ExecutionRequest { pub intent: ExecutionIntent, pub parent: Option<StepId>, pub caused_by: Option<StepId> }
pub enum OperationError { NotFound(OperationId), Decide(DecideError), Execution(ExecutionError), HostUnavailable(HostId),
                          Conflict, StaleEffect, Store(String), Unavailable }   // Unavailable: the writer is closing
```

Commands (**fixed** names on `OperationHandle`; every method is `async` and returns `Result<_, OperationError>`):

| Method | Does |
|---|---|
| `create(OperationSeed) -> OperationId` | inserts the operation `Queued` (budgets = seed or settings defaults), the first steps (through the `Propose` rules), the incident links and `seed.effects` (each created with this operation as aggregate); journal `operation.created`, audit `operation.created`; then runs admission |
| `propose(op, steps, edges) -> Vec<StepId>` | `OperationEvent::Propose` |
| `propose_execution(op, ExecutionRequest) -> StepId` | reads the live session (`HostObservation::observe_snapshot`; none → `HostUnavailable`, nothing stored), finds the skill, validates the args with `SkillSummary::validate_args` (an `ArgError`, an unknown skill or a non-skill contract becomes `refused: Some(reason)`), computes the v2 plan hash (6.5), sends `ProposeExecution`; returns the Execution step id (= execution id) even when the step was stored `Refused` |
| `authorize(op, step, Authorization, PlanBinding, ApprovalProvenance)` | the only way to `Authorized`: runs `ExecutionRecord::authorize`, then `ExecutionAuthorized`; a mismatching binding returns `Execution(PlanChanged)` and changes nothing |
| `deny(op, step, HumanApproval)` | `ExecutionDenied` |
| `answer(op, question, text, Actor)` | `Answer` (the writer builds the `Note` step) |
| `step_started`, `step_finished`, `cancel(op, Actor, reason)`, `pause(op, Actor)`, `resume(op, Actor, extend)`, `finish(op, outcome, summary)` | the matching events |
| `reconcile_host(host)` (`HostReconciler`) | for each `OperationRepo::dispatched_on(host)` execution, one `Reconcile` effect (`dedup_key reconcile:<step>`, due now); nudges the runner |
| `is_alive()` | whether the writer still takes commands |

Handler-facing commands (crate-private, used only by `effects/`): `StartStep`, `FinishStep`, `Defer` (completes the held effect and creates a successor in one
commit), `Dispatch` (6.5), `Settle`, `Verified`, `Expire`, `Tick`, `FinishEffect` (completes an effect that has nothing left to do). Each carries the
`(EffectId, ClaimToken)` it holds; a stale claim yields `CommitError::StaleEffect`, nothing is committed, and the handler reports `Handled`.

Admission: after `create` and after every commit that makes an operation terminal, the writer admits `Queued` operations oldest first while
`count(Running + Waiting* + Paused) < settings.max_running`. A `Paused` operation keeps its slot until resumed or cancelled.

### 6.4 Journal kinds, payloads and audit kinds **fixed**

New `JournalKind` variants (stable strings; `as_str` and `parse` updated; the `journal_kinds_round_trip` test extended):

| Variant | String | `payload_v 1` payload |
|---|---|---|
| `OperationCreated` | `operation.created` | `{operation, kind, goal, incidents: [..], parent}` |
| `OperationStatus` | `operation.status` | `{from, to, pause_reason?, cycle}` (status names, not status objects) |
| `StepCreated` | `operation.step_created` | `{step, ordinal, kind, phase, parent, cycle, intent}` |
| `StepStarted` | `operation.step_started` | `{step}` |
| `StepFinished` | `operation.step_finished` | `{step, outcome, summary?, reason?}` (never `output`) |
| `ExecutionTransition` | `operation.execution` | `{step, execution, from, to, reason?}` using `ExecutionState` names |

Audit kinds (constants in `operations/audit.rs`; subject `operation:<id>` or `execution:<id>`; the detail always holds `operation` and `step`):
`operation.created`, `operation.cancelled`, `operation.paused`, `operation.resumed`, `execution.proposed`, `execution.approved`,
`execution.denied`, `execution.expired`, `execution.blocked` (loop guard), `execution.dispatched`, `execution.succeeded`, `execution.failed`,
`execution.outcome_unknown`, `execution.not_run`, `execution.unavailable`, `execution.refused`. Actors: the human for approve, deny, cancel, pause and
resume that a person asked for; `Actor::System(SYSTEM_OPERATIONS)` for everything the service decides. An audit failure fails the commit (same transaction).
No audit row or journal payload contains `output`, a token or a secret, and a plan hash appears only as its 8-hex `short()` form.

### 6.5 The operation dispatch boundary **fixed** (names and counts), body **convention**

One function decides to dispatch an operation execution, and it takes a value that exists only for one run.

```rust
// crates/domovoi-engine/src/operations/execute.rs
pub struct AuthorizedStep { execution: ExecutionRecord }                  // no public constructor; never stored, never serialised
impl AuthorizedStep {
    pub(super) fn of(execution: ExecutionRecord) -> Result<Self, ExecuteError>;   // human approval present, else NotAuthorized; the policy case is NotYetSupported
    pub fn execution(&self) -> &ExecutionRecord;
}
pub(super) async fn execute_step(shared: &OpShared, authorized: AuthorizedStep, skill: SkillSummary,
                                 args: ValidatedArgs, session: AgentSnapshot) -> Result<(), ExecuteError>;
pub enum ExecuteError { NotAuthorized, NotYetSupported, Expired, PlanChanged, NotDispatched, Refused(String) }
```
`execute_step` mirrors `actions/execute.rs:116-316`, minus what a durable effect replaces:

1. `now >= expires_at` → `Expired` (writes nothing; the writer settles `NotRun`).
2. Loop guard: when `OperationRepo::dispatched_since(host, contract, now - 1h) >= settings.loop_guard_per_hour`, queue `Settle{Refused}` with audit `execution.blocked` and return `Refused`. The count comes from the store, so nothing has to be reloaded at start.
3. Recompute the v2 plan hash (below). Take the per-host dispatch gate (`OpShared::dispatch_gate(host)`, the same idea as `actions/mod.rs:155`) before the `Dispatch` command and release it after `hand_off` returns, so a reconciliation read cannot see a committed-but-unsent run.
4. Build `OperationCommand::Dispatch { .. }` **once**, in this file, and wait for the writer's answer. The writer's dispatch handling re-checks: the authorization evidence, freshness (operation `Running`, approval unexpired, no linked incident `Resolved` or `Dismissed` unless the intent says otherwise), the registry (`CapabilityRegistry::enabled` and `availability == Available`), the session and journal epoch, the enrolled agent (`HostRepo::live_agent`), and the recomputed plan hash against the stored `plan_hash`. It commits `ExecutionDispatched{epoch}` or a `Settle{Refused | NotRun | Unavailable}` together with the completion of the `Dispatch` effect.
5. Re-read the execution; send only if it is still `Dispatched` under the epoch the writer recorded. A missing epoch is never sent.
6. `HandoffRunner::hand_off(host, SkillDispatch { action: ExecutionId::as_action(step id), skill, args, plan_hash: <v1 wire hash> }, session, epoch, deadline)`. **The wire carries the legacy v1 hash** (`PlanHash::compute(skill name, manifest hash, host, args)`; the agent recomputes v1, see `actions/execute.rs:140-142`); the v2 hash is only the approval and dispatch-guard value. A `hand_off` error proves nothing was sent: queue `Settle{NotRun, "hub_not_handed_off"}`.
7. `await_handoff_result`: `Ok(output)` → `Settle{Succeeded | Failed("exit code N")}` with a bounded redacted `output` and `verify_after` from the skill's `verify` delay (default `settings.verify_default_delay`); `LinkError::Rejected(reason)` → `Settle{Failed, reason}`; any other error leaves the step `Dispatched`, asks for a `Reconcile` effect due now, and returns `Ok`.

The v2 hash: `PlanTarget::new(host, host_key, check, target)` where `host_key` is `ResolvedTarget::host_key` from `TargetResolver::resolve(TargetQuery::Host { host })`,
`PlanContract::skill(name, contract_version)`, `ImplHash::from_parts(skill.manifest_hash, skill.entry_script_digest)` and
`PlanHash::compute_v2(&contract, impl_hash, &target, &args, ExecutionId::from_step(step))`. It is computed at proposal (stored) and again at dispatch (compared);
a re-enrolment or a changed script in between changes it and the run is refused. **Only `skill.<name>` contracts execute.** A non-skill contract is stored
`Refused` at proposal; one that somehow reaches dispatch settles `Unavailable("contract_not_executable")`.

Arch-check cards to add in `xtask/src/arch/rules.rs` (each with a unit test in `xtask/src/arch/ast_tests.rs`, and the ADR of section 15). `BOUNDARY_PAIRS` gains
`"AuthorizedStep::of"`, `"execute::execute_step"` and `"OperationCommand::Dispatch"`; `BOUNDARY_CARDS` gains:

| Card | Operation | Allowed sites (exact count) |
|---|---|---|
| EXEC-2 | `Call("hand_off")` | `crates/domovoi-engine/src/actions/execute.rs` 1, `crates/domovoi-engine/src/operations/execute.rs` 1 |
| EXEC-2 | `Call("await_handoff_result")` | the same two files, 1 each |
| EXEC-2 | `Pair("AuthorizedStep::of")` | `crates/domovoi-engine/src/operations/effects/dispatch.rs` 1 |
| EXEC-2 | `Pair("execute::execute_step")` | `crates/domovoi-engine/src/operations/effects/dispatch.rs` 1 |
| EXEC-2 | `Construct("OperationCommand::Dispatch")` | `crates/domovoi-engine/src/operations/execute.rs` 1 |
| EXEC-2 | `Pattern("OperationCommand::Dispatch")` | `crates/domovoi-engine/src/operations/writer/mod.rs` 1 |
| EXEC-2 | `Pair("OperationCommand::Dispatch")` | none |

If `cargo xtask arch-check` reports another production `hand_off` or `await_handoff_result` caller before you add the cards, stop and report it. Two dispatch sites
exist **by design for now**: the legacy one for actions (RW-32, RW-36) and this one for steps. When a later prompt removes the legacy executor the cards collapse
to one site and `AuthorizedStep` becomes the only authorized value.

### 6.6 Claim scope and effect cancellation **fixed**

```rust
impl AggregateKind { pub const LEGACY: [AggregateKind; 3] = [Incident, Action, Note]; }   // `Operation` is the fourth variant

// EffectQueue (replaces the two signatures at lifecycle.rs:73-84)
async fn claim_due(&self, runner: &str, kinds: &[EffectKind], aggregates: &[AggregateKind],
                   now: Timestamp, lease: SignedDuration, limit: u16) -> Result<ClaimBatch, StoreError>;
async fn revert_claimed_at_start(&self, aggregates: &[AggregateKind]) -> Result<u64, StoreError>;

pub struct EffectCancel { pub aggregate: AggregateRef, pub at: Timestamp }
// Transaction::cancelled_effects: Vec<EffectCancel>; builder `cancel_effects(EffectCancel)`; counted by `is_empty`
```
An empty `aggregates` claims and reverts nothing (like an empty `kinds`). SQLite adds `AND aggregate_type IN (?, ..)` to both branches of the claim `WHERE`
(`store/src/uow/queue.rs:55-64`) and to the revert. `EffectCancel` turns every `pending` effect of that aggregate into `cancelled` (`completed_at = at`, no token) and
leaves `claimed` effects alone: their result is still recorded by the writer (RW-35). Cancellations are applied **after audit and before `new_effects`**, so one
transaction may cancel old work and create new work; update the `Transaction` doc comment and `store/src/uow/mod.rs` to say so.
`EffectRunner::new` scopes to `AggregateKind::LEGACY`; `EffectRunner::scoped(vec![AggregateKind::Operation])` is the operation runner. Update every caller of
`claim_due` and `revert_claimed_at_start` (about 36 call sites: `grep -rn "claim_due(\|revert_claimed_at_start(" crates`); existing tests pass
`&AggregateKind::LEGACY` and keep their expectations. The legacy action service keeps its single `Wake` handler and now only ever sees action effects.

### 6.7 The service, its handlers and its settings **fixed** (kinds, payloads, results), **convention** (layout)

```rust
// crates/domovoi-engine/src/operations/mod.rs
pub struct OperationService;
impl OperationService {
    /// Starts the writer task, the operation effect runner and the startup recovery; one JoinHandle joins them.
    pub fn spawn(deps: OperationDeps, settings: OperationSettings) -> (OperationHandle, JoinHandle<()>);
}
pub struct OperationDeps {          // ports only; there is no `AgentLink` and no raw link anywhere in this struct
    pub operations: Arc<dyn OperationRepo>, pub uow: Arc<dyn UnitOfWork>, pub effects: Arc<dyn EffectQueue>,
    pub effect_read: Arc<dyn EffectRead>, pub advisories: Arc<dyn AdvisoryRepo>, pub incidents: Arc<dyn IncidentRepo>,
    pub hosts: Arc<dyn HostRepo>, pub evidence: Evidence, pub registry: Arc<dyn CapabilityRegistry>,
    pub resolver: Arc<dyn TargetResolver>, pub observer: Arc<dyn HostObservation>, pub journal: Arc<dyn JournalStatus>,
    pub handoff: Arc<dyn HandoffRunner>, pub rerun: Arc<dyn CheckRerun>, pub redactor: Arc<dyn Redactor>,
    pub clock: Arc<dyn Clock>, pub shutdown: CancellationToken,
}
pub struct OperationSettings {      // `Default` gives the values in section 6.8; tests build their own
    pub max_running: u16, pub approval_ttl: SignedDuration, pub question_timeout: SignedDuration,
    pub agent_wait_limit: SignedDuration, pub loop_guard_per_hour: u32, pub budgets: BudgetLimits,
    // engine-internal, not user config:
    pub retry_interval: SignedDuration /* 5 s */, pub evidence_deadline: SignedDuration /* 30 s */,
    pub verify_default_delay: SignedDuration /* 30 s */, pub dispatch_grace: SignedDuration /* 30 s */,
    pub reconcile_delay: SignedDuration /* 60 s */, pub reconcile_interval: SignedDuration /* 30 s */,
    pub recheck_deadline: SignedDuration /* 60 s, like actions/recheck.rs:11 */,
}
```

The operation runner is `EffectRunner::new(..).scoped(vec![AggregateKind::Operation])` with these handlers (one `EffectHandler` per kind, each in its own
file under `operations/effects/`). Every payload is JSON with `"v": 1`; a payload that does not parse is `Dead{"unreadable payload"}`. Handlers read the
step and execution **from the store**, never from the payload (the payload only names them).

| Kind | Payload | Handler does | Ends with |
|---|---|---|---|
| `CollectEvidence` | `{v, operation, step}` | `StartStep`; rebuilds the call with `ReadToolCall::from_json(tool, args)`; `Evidence::collect_evidence(EvidenceRequest::for_host(host, call, now + evidence_deadline))`; redacts the output (`Redactor`), summary = first 280 chars, output cap 16 KiB | `FinishStep{Succeeded, summary, output}`. `NotConnected`, `Incompatible`, `NotEnabled` → `Unavailable` with the error text as `reason`. `Timeout` and `Transport` → `EffectOutcome::Retry` while `attempt < max_attempts`, then `Failed`. The operation is no longer open (terminal or the step is final) → `FinishEffect` |
| `Dispatch` | `{v, operation, step}` | the checks and call sequence of 6.5: registry availability first. `HostUnknown` or `HostAbsent` (or no live session): `Defer` with a `Wake{purpose: "retry_dispatch"}` successor due `now + retry_interval`, and the writer records `waiting_since` once (`WaitingForAgent`). `HostIncompatible` → `Settle{Unavailable, "agent upgrade required"}`. `NotInstalled`, `DisabledByHub`, `NotAllowedOnHost`, `Unavailable` → `Settle{Unavailable, availability.describe()}`. Otherwise `AuthorizedStep::of` then `execute_step` | `Handled` after the writer committed; stale claim → `Handled` |
| `Wake` | `{v, operation, step?, purpose}` with `purpose` one of `retry_dispatch`, `approval_expiry`, `question_timeout`, `wall_time` | `retry_dispatch`: past `waiting_since + agent_wait_limit` → `Settle{Unavailable, "the agent did not come back in time"}`; host `Known` → `Defer` with a `Dispatch` successor (`dedup_key dispatch:<step>`); else `Defer` with another `Wake`. `approval_expiry`: the execution is still awaiting approval and `now >= expires_at` → `Expire`; already decided → `FinishEffect`. `question_timeout`: Question still `Pending` → `FinishStep{NotRun, "no answer before the timeout"}`. `wall_time` → `Tick`. An unknown purpose is `Dead` | `Handled` |
| `Verify` | `{v, operation, step, execution}` | read-only: `CheckRerun::rerun_check(host, check, target, now + recheck_deadline)` using the **execution record's** `check` and `target` (the pinned subject); transport failures are `Retry` while attempts remain, then `passed = false` with the failure as the summary (as `actions/verify_wake.rs:112-136`) | `Verified{passed, summary(redacted)}` |
| `Reconcile` | `{v, operation, step}` | execution not `Dispatched` → `FinishEffect`. No live session for the host → `Defer` with a `Reconcile` successor due `now + reconcile_interval`. Live epoch differs from the recorded one → `Settle{OutcomeUnknown, EpochChanged text}` without asking. Else `JournalStatus::status_batch(host, [ExecutionId::as_action(step)], recorded epoch, deadline)` and map with the **existing** `from_answer` and `reason_text` (`actions/reconcile/answer.rs:71-92`, made `pub(crate)`): `Finished{succeeded}` → `Settle{Succeeded | Failed}`, `NotFound` → `Settle{NotRun}`, `Unknown{reason}` → `Settle{OutcomeUnknown, reason_text}`, `Running` → `Defer` again | `Handled` |

`BuildContext`, `CallModel`, `DraftSummary`, `Notify`, `EvaluatePolicy`, `IssuePermit` have **no handler** in this prompt, so the runner never claims them.

**Startup** (inside `spawn`, before the runner claims anything): (1) return every `claimed` `dispatch` effect of an operation to `pending`. `claim_due` and
`revert_claimed_at_start` deliberately never touch `dispatch` claims, and a claimed dispatch effect means its `ExecutionDispatched` commit never happened (that commit
completes the effect), so nothing was sent and re-pending is safe. Do it with `EffectRead::effects_with_status(Claimed)` filtered to kind `dispatch` and aggregate
`operation`, and one fenced `Transaction::reschedule` per row using the token stored on the row. (2) run admission once. (3) start the runner (which calls
`revert_claimed_at_start(&[Operation])` for the other kinds). Nothing else is rebuilt in memory: state lives in the store.

### 6.8 Configuration **fixed** (keys and defaults), **convention** (wording)

New hub section, in `crates/domovoi-config/src/hub/orchestration.rs`, field `orchestration: OrchestrationSection` on `HubConfig` (`#[serde(default)]`, `deny_unknown_fields`, `JsonSchema`):

```toml
[orchestration]
enabled = false              # default false: no operation service, the legacy action and triage paths run as before
max_running = 4              # operations running at once; others wait Queued
approval_ttl = "24h"         # how long a proposed execution waits for a person
question_timeout = "24h"     # how long a question waits for an answer
agent_wait_limit = "30m"     # how long a dispatch waits for a host's agent before the step is Unavailable
loop_guard_per_hour = 3      # runs of one capability on one host per rolling hour

[orchestration.budgets]      # defaults for a new operation
model_turns = 20
evidence_requests = 40
executions = 5
cycles = 3
wall_time = "72h"
tokens = 200000
# cost_micros omitted = no cost limit
```

Validation in `validate.rs`: `max_running >= 1`, `loop_guard_per_hour >= 1`, every duration `> 0`, `approval_ttl <= 7d`, every budget `>= 1` when present. The
section name `orchestration` is added to the hub section list in `env.rs`. Run `cargo xtask docs-config` and commit the regenerated
`docs/book/src/reference/configuration.md`. `enabled` is read once at start; flipping it needs a restart (say so in the field's doc comment).

### 6.9 `OperationRepo` and `OperationSnapshot` **fixed**

```rust
// crates/domovoi-core/src/ports/repos/operations.rs
#[async_trait]
pub trait OperationRepo: Send + Sync {
    async fn get(&self, id: OperationId) -> Result<Option<Versioned<Operation>>, StoreError>;
    /// Operation, steps (ordinal order), edges, executions and incident links from ONE consistent read.
    async fn load(&self, id: OperationId) -> Result<Option<OperationSnapshot>, StoreError>;
    async fn list(&self, filter: &OperationFilter, page: &Page) -> Result<Paged<Operation>, StoreError>;   // newest first
    async fn for_incident(&self, incident: IncidentId) -> Result<Vec<Operation>, StoreError>;
    async fn execution(&self, id: ExecutionId) -> Result<Option<Versioned<ExecutionRecord>>, StoreError>;
    /// Executions whose step outcome is `running` on `host`.
    async fn dispatched_on(&self, host: HostId) -> Result<Vec<ExecutionRecord>, StoreError>;
    /// How many executions of `contract` on `host` were dispatched at or after `since`, whatever their outcome.
    async fn dispatched_since(&self, host: HostId, contract: &ContractId, since: Timestamp) -> Result<u32, StoreError>;
    /// Operations whose status name is one of `names`, oldest first (admission and startup).
    async fn with_status(&self, names: &[&str], limit: u16) -> Result<Vec<Operation>, StoreError>;
}
pub struct OperationFilter { pub status: Option<String>, pub incident: Option<IncidentId>, pub host: Option<HostId>,
                             pub kind: Option<OperationKind> }
pub struct OperationSnapshot { pub operation: Operation, pub version: u64, pub steps: Vec<StepRecord>, pub edges: Vec<StepEdge>,
                               pub executions: Vec<Versioned<ExecutionRecord>>, pub incidents: Vec<IncidentId> }
```
The port has **no write method**: every write is an `OperationWrite` in a `Transaction`. `OperationRepo` is not given to transports in this prompt.

### 6.10 The trace schema **fixed** (this is the frozen contract for 28, 30, 38 and 52)

Rust (`crates/domovoi-core/src/operation/trace.rs`), all `Serialize + Deserialize`, enums as snake_case strings, timestamps RFC 3339 UTC with a trailing `Z`:

```rust
pub struct OperationTrace { pub schema_version: u32 /* 1 */, pub as_of_seq: u64, pub operation: TraceOperation,
                            pub steps: Vec<TraceStep>, pub edges: Vec<TraceEdge> }
pub struct TraceOperation { pub id: OperationId, pub kind: OperationKind, pub goal: String, pub status: String /* OperationStatus::name */,
                            pub pause_reason: Option<PauseReason>, pub waiting_for: Vec<WaitKind>, pub cycle: u32, pub budgets: Budgets,
                            pub pinned: Vec<PinnedSubject>, pub parent: Option<OperationId>, pub incidents: Vec<IncidentId>,
                            pub created_at: Timestamp, pub started_at: Option<Timestamp>, pub ended_at: Option<Timestamp> }
pub struct TraceStep { pub id: StepId, pub parent: Option<StepId>, pub kind: StepKind, pub phase: String, pub cycle: u32,
                       pub outcome: StepOutcome, pub started_at: Option<Timestamp>, pub ended_at: Option<Timestamp>,
                       pub summary: Option<String>, pub refs: Vec<TraceRef> }
pub struct TraceRef { pub kind: String /* "execution" in this prompt; more later */, pub id: String }
pub struct TraceEdge { pub from: StepId, pub to: StepId, pub kind: EdgeKind }
pub fn build_trace(snapshot: &OperationSnapshot, as_of_seq: u64) -> OperationTrace;     // pure
```
`build_trace` rules: steps in ordinal order; edges sorted by (ordinal of `from`, ordinal of `to`, kind name); `waiting_for` is the distinct wait kinds of the open
steps in priority order (Approval, User, Agent, Timer, by the `derive_status` conditions); `summary` is cut at 280 characters on a character boundary; `refs`
holds `{"kind": "execution", "id": <execution id>}` for an Execution step and is `[]` otherwise; `output`, approval details, plan hashes and `intent` are **not** in the trace
(the inspector reads them from detail endpoints later). `status` is the status **name**; the paused reason travels in `pause_reason`. `as_of_seq` is the journal head the
caller read before loading (28 defines how endpoints obtain it); `build_trace` does not read it.

Compatibility rule: adding a `StepKind`, outcome, edge kind or optional field is not a breaking change (consumers must show an unknown string generically, the
`Open<T>` principle of RW-15); renaming or removing one, or changing the meaning of a field, bumps `schema_version`.

Example (abridged; ids shortened):

```json
{
  "schema_version": 1,
  "as_of_seq": 42,
  "operation": {
    "id": "0190-op-1", "kind": "investigate", "goal": "plex keeps restarting on nas", "status": "waiting_approval",
    "pause_reason": null, "waiting_for": ["approval"], "cycle": 0,
    "budgets": {"limits": {"model_turns": 20, "evidence_requests": 40, "executions": 5, "cycles": 3,
                           "wall_seconds": 259200, "tokens": 200000, "cost_micros": null},
                "used": {"model_turns": 1, "evidence_requests": 1, "executions": 1, "wall_seconds": 0, "tokens": 900, "cost_micros": 0}},
    "pinned": [{"host": "0190-host-1", "check": "container.restart_loop", "target": "plex"}],
    "parent": null, "incidents": ["0190-inc-1"],
    "created_at": "2026-01-01T00:00:00Z", "started_at": "2026-01-01T00:00:01Z", "ended_at": null
  },
  "steps": [
    {"id": "0190-s0", "parent": null, "kind": "evidence_request", "phase": "gather", "cycle": 0, "outcome": "succeeded",
     "started_at": "2026-01-01T00:00:02Z", "ended_at": "2026-01-01T00:00:03Z", "summary": "docker_logs plex: 3 restarts", "refs": []},
    {"id": "0190-s1", "parent": null, "kind": "approval", "phase": "approve", "cycle": 0, "outcome": "pending",
     "started_at": null, "ended_at": null, "summary": null, "refs": []},
    {"id": "0190-s2", "parent": null, "kind": "execution", "phase": "act", "cycle": 0, "outcome": "pending",
     "started_at": null, "ended_at": null, "summary": "skill.restart_container plex",
     "refs": [{"kind": "execution", "id": "0190-s2"}]}
  ],
  "edges": [
    {"from": "0190-s2", "to": "0190-s1", "kind": "depends_on"},
    {"from": "0190-s2", "to": "0190-s0", "kind": "caused_by"}
  ]
}
```

**Golden fixtures** (one set, two homes). The five scenarios below are built **by hand** in `crates/domovoi-core/tests/operation_trace/scenarios.rs`
(fixed ids from `Uuid::from_u128(0x0190_…)`, fixed timestamps; hand-built, not produced by the writer, so they may include kinds the writer
does not yet create, such as `policy_decision`). The committed JSON files are in **`web/src/test/fixtures/operation-trace/`** (a directory beside the existing file
`web/src/test/fixtures.ts`; import the JSON by explicit path so module resolution is unambiguous):

| File | Scenario |
|---|---|
| `waiting-approval.json` | evidence done, a plan, an execution proposed and `waiting_approval`; includes a `policy_decision` step and a `reasoning_turn` |
| `verified-success.json` | approved, dispatched, `succeeded`, verification `succeeded`, operation `succeeded` |
| `failed-verification-new-cycle.json` | verification `failed`, `cycle: 1`, a `retry_of` edge, a `caused_by` edge, operation `running` |
| `outcome-unknown-paused.json` | execution `outcome_unknown`, operation `paused` with `pause_reason.type = "outcome_unknown"` |
| `question-and-budget-pause.json` | a question answered by a `note` (`answers` edge), an `assessment`, a `notification`, a `refused` execution, an `unavailable` evidence request, a `skipped` step, a `cancelled` step, `paused` with `budget` |

Together the five files must contain **every `StepKind`, every `StepOutcome` and every `EdgeKind`** (a test asserts it). The Rust test
`trace_fixtures_match_the_builder` builds each scenario, serialises it with `serde_json::to_value`, and compares it to the file for equality;
with the environment variable `OPERATION_TRACE_UPDATE=1` it rewrites the files (pretty-printed, two-space indent, trailing newline) instead of failing. Review the diff of
any rewrite as you would an `insta` snapshot. `insta` additionally snapshots `OperationTrace` of the first scenario and each public enum's JSON (`crates/domovoi-core/tests/snapshots/`).

Web (`web/src/api/operationTrace.ts`, hand-written until a later endpoint publishes the schema through OpenAPI, and replaced by the generated type then): `interface OperationTrace`,
`TraceOperation`, `TraceStep`, `TraceEdge`, `TraceRef` with `string` unions for kind, outcome and edge kind **plus `(string & {})`** so an unknown value type-checks and renders generically.
`web/src/test/fixtures/operation-trace/trace.test.ts` loads every JSON file and asserts the invariants of section 12.6.

## 7. Ordered implementation steps

1. **Inspect and verify the repository context; report discrepancies before editing.**
   - Open every file and symbol named in section 3 and confirm it exists as described. Record any difference (a moved line, a renamed type, a signature that changed).
   - Confirm 22 and 23 are merged (`ExecutionId`, `Authorization`, `PlanHash::compute_v2`, `execute::execute` all present) and that 24 and 25 are merged: `domovoi_core::capability::{ContractId, Availability}`, ports `CapabilityRegistry` and `TargetResolver`. **If 24 or 25 is not merged, stop and report.** Use their merged names, not the names in this brief, and note each difference.
   - Run `ls crates/domovoi-store/migrations/` and note the next free number; `ls docs/adr/` for the next ADR number.
   - Confirm `CheckKey`, `TargetKey`, `PlanHash`, `JournalEpoch` implement serde (they do at the time of writing) and check whether `ContractId` does.
   - Run `grep -rn "hand_off(\|await_handoff_result(" crates --include=*.rs` and confirm the only production callers are in `crates/domovoi-engine/src/actions/execute.rs` and the hub's `runtime/caps.rs` definitions.
   - Run `grep -rn "claim_due(\|revert_claimed_at_start(" crates` and keep the list for step 5.
   - Run `cargo xtask arch-check` and the engine and store tests once to know the baseline is green.
2. Create branch `operation-aggregate` from the latest `overhaul-investigation`.
3. **Core types** (4.1): ids, `ExecutionId::from_step`, status, operation, step, edge, budget, execution, snapshot, with their unit tests. No ports yet.
4. **Core ports and trace**: `AggregateKind::Operation`, `OperationWrite`, `StateWrite::Operation`, `Transaction::cancelled_effects`, `EffectCancel`, the aggregate scope on `EffectQueue`, the journal kinds, `OperationRepo`. Then `trace.rs` and `tests/operation_trace.rs` with the scenario builders; generate the JSON once with `OPERATION_TRACE_UPDATE=1`, **read every file**, and commit them. Update every existing `match` over `StateWrite` and `AggregateKind` the compiler flags (store `writes.rs`, testkit `uow_apply.rs`, testkit `crash/mod.rs`).
5. **Store**: the migration (section 8); `Store` implements `OperationRepo`; `uow/operations.rs`; the scope on claim and revert; `EffectCancel`; update every `claim_due`/`revert_claimed_at_start` caller from the step 1 list (tests pass `&AggregateKind::LEGACY`).
6. **Testkit**: the in-memory twin (`InMemoryStore` implements `OperationRepo` and applies `OperationWrite` with the same ordinal, immutability, one-to-one and action-id rules), the contract suites of 12.2, the crash-harness summary, `FakeRegistry` and `FakeTargetResolver` **only if prompt 25 did not already add fakes**. Run the contract suites against both stores.
7. **Config**: `OrchestrationSection`, validation, env section name, `cargo xtask docs-config`.
8. **`decide`** and its tests, including the property test. Pure; build it before the writer and make it pass first.
9. **Writer, handle and execution module**: commands, `Decision` → `Transaction`, admission, audit and journal builders. Writer tests (12.4).
10. **Boundary, handlers, service**: `execute.rs`, the five handlers, `OperationService::spawn` with startup recovery; make the narrow `pub(crate)` visibility change in `actions/`, scope the legacy runner with `.scoped(AggregateKind::LEGACY)` (a one-line change in `actions/mod.rs` next to line 243); handler tests, restart tests, the simulator.
11. **Hub wiring** behind `config.orchestration.enabled`, the composite reconciler, runtime tests (12.5).
12. **xtask**: the cards of 6.5 with their unit tests; **the ADR** (section 15). Run `cargo xtask arch-check`.
13. **Web**: `operationTrace.ts`, the fixtures directory (already written in step 4, now reviewed), `trace.test.ts`; `npm run lint`, `npm run typecheck`, `npx vitest run src/test/fixtures/operation-trace`.
14. **Docs** (section 15), then the checks and the completion report.

Commit in small green steps (one-line Conventional Commits, one scope each, for example `feat(core): operation, step and trace types`): after step 4, after 6, after 8, after 10, after 12 and after 14.
Each of those points must pass `cargo fmt --all`, the clippy and tests for the crates touched so far, and (from step 4 on) `cargo xtask arch-check`. Do not carry a red check forward.

## 8. Database and persistence

**Migration** `crates/domovoi-store/migrations/NNNN_operations.sql` — `NNNN` is the next free number in that directory on your branch (list it; never copy a number from this brief,
and describe the migration by its contents everywhere). It is additive: new tables only, nothing existing is altered or rebuilt, `schema_compat.min_reader` does not change
(an older binary ignores unknown tables). Enums are validated in Rust; the only `CHECK`s below are formats.

```sql
CREATE TABLE operations (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    goal         TEXT NOT NULL,
    status       TEXT NOT NULL,                 -- OperationStatus::name, validated in Rust
    pause_reason TEXT,                          -- PauseReason JSON; set only while status = 'paused'
    cycle        INTEGER NOT NULL DEFAULT 0,
    budgets      TEXT NOT NULL,                 -- {"limits":{..},"used":{..}}
    pinned       TEXT NOT NULL,                 -- JSON array of PinnedSubject
    parent_id    TEXT REFERENCES operations(id),
    created_at   INTEGER NOT NULL,
    started_at   INTEGER,
    ended_at     INTEGER,
    updated_at   INTEGER NOT NULL,
    version      INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX operations_created ON operations (created_at DESC, id DESC);
CREATE INDEX operations_status  ON operations (status, created_at, id);

CREATE TABLE operation_incidents (              -- written only by the operation writer; the incident manager only reads
    operation_id TEXT NOT NULL REFERENCES operations(id),
    incident_id  TEXT NOT NULL,                 -- no foreign key: incidents are pruned independently
    linked_at    INTEGER NOT NULL,
    PRIMARY KEY (operation_id, incident_id)
) STRICT;
CREATE INDEX operation_incidents_incident ON operation_incidents (incident_id, operation_id);

CREATE TABLE steps (
    id             TEXT PRIMARY KEY,
    operation_id   TEXT NOT NULL REFERENCES operations(id),
    ordinal        INTEGER NOT NULL,
    kind           TEXT NOT NULL,
    phase          TEXT NOT NULL,
    parent_step_id TEXT REFERENCES steps(id),
    cycle          INTEGER NOT NULL,
    intent         TEXT NOT NULL,               -- StepIntent JSON, immutable
    created_at     INTEGER NOT NULL,
    -- the derived projection below is the only part that is ever updated
    status         TEXT NOT NULL DEFAULT 'pending',   -- StepOutcome::as_str
    started_at     INTEGER,
    ended_at       INTEGER,
    summary        TEXT,
    reason         TEXT,
    output         TEXT,                        -- bounded, redacted; raw-output retention may null it later (D-21)
    UNIQUE (operation_id, ordinal)
) STRICT;
CREATE INDEX steps_status ON steps (status, operation_id);
CREATE TRIGGER steps_intent_immutable
BEFORE UPDATE OF id, operation_id, ordinal, kind, phase, parent_step_id, cycle, intent, created_at ON steps
BEGIN SELECT RAISE(ABORT, 'a step''s intent is immutable'); END;

CREATE TABLE step_edges (
    operation_id TEXT NOT NULL REFERENCES operations(id),
    from_step    TEXT NOT NULL REFERENCES steps(id),    -- the dependent
    to_step      TEXT NOT NULL REFERENCES steps(id),    -- the prerequisite
    kind         TEXT NOT NULL,
    PRIMARY KEY (from_step, to_step, kind),
    CHECK (from_step <> to_step)
) STRICT;
CREATE INDEX step_edges_operation ON step_edges (operation_id);
CREATE INDEX step_edges_to        ON step_edges (to_step);
CREATE TRIGGER step_edges_point_backwards
BEFORE INSERT ON step_edges
WHEN (SELECT operation_id FROM steps WHERE id = NEW.from_step) IS NOT NEW.operation_id
  OR (SELECT operation_id FROM steps WHERE id = NEW.to_step)   IS NOT NEW.operation_id
  OR (SELECT ordinal FROM steps WHERE id = NEW.to_step) >= (SELECT ordinal FROM steps WHERE id = NEW.from_step)
BEGIN SELECT RAISE(ABORT, 'an edge must point to an earlier step of the same operation'); END;

CREATE TABLE executions (                      -- 1:1 with an Execution step: the primary key IS the step id (RW-36)
    execution_id         TEXT PRIMARY KEY REFERENCES steps(id),
    operation_id         TEXT NOT NULL REFERENCES operations(id),
    host_id              TEXT NOT NULL,
    contract             TEXT NOT NULL,
    contract_version     INTEGER NOT NULL,
    check_key            TEXT NOT NULL,
    target               TEXT,
    args                 TEXT NOT NULL,         -- JSON object name -> value
    plan_hash            TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    expires_at           INTEGER NOT NULL,
    authorization_kind   TEXT,                  -- 'human' today; validated in Rust, no CHECK
    approved_by          TEXT,
    approved_channel     TEXT,                  -- ApprovalChannel JSON
    approved_at          INTEGER,
    approved_plan_hash   TEXT CHECK (approved_plan_hash IS NULL OR (length(approved_plan_hash) = 64 AND approved_plan_hash NOT GLOB '*[^0-9a-f]*')),
    approval_session_ref TEXT,
    approval_client      TEXT,
    waiting_since        INTEGER,
    agent_epoch          TEXT,
    dispatched_at        INTEGER,
    settled_at           INTEGER,
    created_at           INTEGER NOT NULL,
    updated_at           INTEGER NOT NULL,
    version              INTEGER NOT NULL DEFAULT 1,
    CHECK ((approved_by IS NULL) = (approved_at IS NULL)),
    CHECK ((agent_epoch IS NULL) = (dispatched_at IS NULL))
) STRICT;
CREATE INDEX executions_operation ON executions (operation_id);
CREATE INDEX executions_host_contract ON executions (host_id, contract, dispatched_at);
```

**Store rules** (in `uow/operations.rs`, with a typed error, never a panic):
- `Operation{New}` over an existing id → `Conflict(New, Version(n))`; `Version(n)` mismatch → `Conflict`; success bumps `version` and reports it in the receipt.
- `Step` insert: the operation must exist; `ordinal` must equal the current count of that operation's steps (so ordinals stay dense); `kind`, `phase`, `parent` and `intent` are validated (`phase` grammar, text lengths, `EvidenceRequest.tool` via `ReadToolCall::from_json`); a duplicate id or ordinal is `Constraint`.
- `Progress{step, from, to}` updates `status, started_at, ended_at, summary, reason, output` only where `status = from`; zero rows → `Conflict` with `Observed::Status`. The immutable columns are never in the `SET` list.
- `Edge`: both steps exist, same operation, `to.ordinal < from.ordinal` (the trigger repeats it); otherwise `Constraint("an edge must point to an earlier step of the same operation")`.
- `Execution{New}`: the step exists, has kind `execution`, belongs to the same operation, the id is not in `actions` (`SELECT 1 FROM actions WHERE id = ?` → `Constraint("execution id is an action id")`); `Version(n)` guarded update. `HumanApproval::restore` is called only when **reading** a row back.
- `IncidentLink` is idempotent (`INSERT OR IGNORE`).
- Unknown stored names (status, kind, outcome, edge kind) found on read are a `StoreError::Backend` with the offending value named, not a panic and not a silent default.

**Transaction boundaries.** Every writer command is exactly one `UnitOfWork::commit`, as listed in 6.3. Nothing writes an operation table outside a `Transaction` (the gate test of 12.5 scans for it). The effect claim in `queue.rs` stays its own `BEGIN IMMEDIATE`.

**N-1 upgrade test.** `migration_adds_the_operation_tables_and_keeps_existing_rows` (12.2) copies the committed fixture database (`crates/domovoi-store/tests/fixtures/n1-schema5.db`, pattern in `crates/domovoi-store/tests/sqlite/upgrade.rs`), opens it with the current binary, and asserts: every pre-existing table keeps its row count, the five new tables exist and are empty, `PRAGMA foreign_key_check` is empty, and `schema_compat.min_reader` is unchanged.

## 9. Protocol and compatibility

- **No wire change.** `PROTOCOL_VERSION` stays 1.2; no message, field or golden frame changes (RW-42). `execution_id = step_id` rides in the existing UUID fields `RunSkill.action_id` and `ActionStatusQuery.execution_ids`; the agent journal keys on it unchanged and never learns whether a UUID came from an action or a step.
- **Wire hash.** `RunSkill`'s plan hash stays the legacy v1 value (the agent recomputes v1); only the hub-side approval and dispatch-guard hash is v2 (see 6.5).
- **Journal and audit.** New `JournalKind` strings and audit kinds are additive. Readers tolerate unknown journal kinds already (`JournalEntry::kind` is a `String`).
- **Effects.** New aggregate type string `operation` in `journal` and `effects`. Older binaries never read it (they do not know the tables). The aggregate-scoped claim makes the two services' effects disjoint.
- **Config.** One new optional section; absent means disabled (`05 §5`, §6).

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `decide` refuses an event | the writer returns `OperationError::Decide(..)`, commits nothing, logs at `debug`; the operation is unchanged |
| `commit` returns `Conflict` | reload the snapshot, re-decide once; a second `Conflict` returns `OperationError::Conflict` and logs `error` |
| `commit` returns `StaleEffect` | nothing is committed; the handler returns `Handled`; the stale result is discarded and logged at `warn` |
| `commit` returns `Backend` | `OperationError::Store`; the handler returns `Retry` (the effect is rescheduled with backoff); no in-memory state survives |
| Audit insert fails | the whole commit fails (same transaction): no state change without its audit row |
| Hub restarts with `pending` effects | the runner claims them again; nothing is lost |
| Hub restarts with `claimed` non-dispatch effects of operations | `revert_claimed_at_start(&[Operation])` returns them to `pending` with a new generation; a stale holder is fenced |
| Hub restarts with a `claimed` `dispatch` effect | startup recovery re-pends it with a fenced reschedule (6.7); safe because that effect is completed by the same commit that records `Dispatched` |
| Hub restarts while an execution is `Dispatched` | its `Reconcile` effect (created in the dispatch commit) fires, asks the agent's journal, and settles the step; never a second dispatch |
| Agent offline when a dispatch is due | `WaitingAgent`; a `Wake` retries every `retry_interval`; after `agent_wait_limit` the step settles `Unavailable` |
| Agent speaks another protocol | step `Unavailable("agent upgrade required")` immediately |
| Plan hash differs at dispatch (re-enrolment, script changed) | step `Refused("the plan changed after approval")`; no dispatch |
| Approval expires | `Wake{approval_expiry}` → Approval `NotRun`, Execution `NotRun`; nothing dispatched |
| Operation cancelled while an execution is `Running` | operation `Cancelled`; the execution is **not** recalled; its result or reconciliation is still recorded on the step |
| `OutcomeUnknown` | execution step `OutcomeUnknown`, operation `Paused{OutcomeUnknown}`; the owner decides; a retry needs a new step with `retry_of` and its own approval |
| `hand_off` fails | proof nothing was sent: `NotRun("hub_not_handed_off")` |
| Result lost after `hand_off` | step stays `Dispatched`; reconciliation settles it |
| Budget spent | the offending step is stored `Refused` and the operation `Paused{Budget}`; `Resume` needs an extended limit |
| Unknown stored status or kind | `StoreError::Backend` naming the value; the writer pauses nothing and logs `error` |
| Operation service closing | commands return `OperationError::Unavailable`; queued commands are applied before the task ends |

## 11. Security requirements

- **Approval stays human-only (INV-1, ADR 0007, RW-11).** `authorize` takes an `Authorization` already built by `crates/domovoi-hub/src/auth/`; this prompt adds no `HumanApproval::new` call, no approve route, no chat command, no model tool. A policy `Authorization` returns `ExecutionError::NotYetSupported` from `ExecutionRecord::authorize`, `AuthorizedStep::of` and the writer; nothing writes `authorization_kind = 'policy'`.
- **One dispatch decision per path (RW-08).** `hand_off` and `await_handoff_result` appear only in the two files of 6.5; `AuthorizedStep::of` and `execute_step` are called once; `OperationCommand::Dispatch` is built once and matched once. The arch cards and the engine gate test enforce it. `AgentLink` is never named in `operations/` (EXEC-4 already covers the engine crate).
- **Bound plan.** v2 hash recomputed at dispatch; any difference refuses. `ExecutionRecord::authorize` refuses a binding that does not name the stored hash. The approval is useless for another execution id because the id is inside the hash.
- **Pinned targets.** An execution can name only a `(host, check, target)` pinned to its operation; the step is stored `Refused` otherwise.
- **No blind retry.** Same contract, host, subject and args as an earlier `Failed` or `OutcomeUnknown` execution is refused unless the new step is a `retry_of` with a stated change, and it still needs its own human approval.
- **Fail closed.** Missing session, missing enrolled agent, non-`Available` registry answer, unresolved host identity or an unreadable payload never dispatch: the step waits (host unknown or absent) or settles `Unavailable` or `Refused`.
- **Audit.** Every approval, denial, expiry, dispatch, settle and operator control is an audit row in the same transaction (6.4). A failed audit insert fails the change.
- **Secrets.** Outputs pass through `Redactor` before they are stored (`steps.output`, 16 KiB cap) and are never written to the journal, the audit log, the trace or a log line. Goals and summaries are redacted by their callers; the writer also redacts `summary` and `reason`. `HumanApproval`'s session reference stays the non-reversible value the hub gave it.
- **No new authority from retention or the trace.** The trace carries no plan hash, approval detail or output.

## 12. Testing requirements

All tests are named after behaviour. Fakes exist today: `InMemoryStore`, `FakeClock`, `FakeAgentLink` (`on_skill`, `on_tool`, `on_check`, `answer`, `set_journal_epoch`, `set_availability`, `new_session`, `requests()`), `ListRedactor`, `action_caps` (`crates/domovoi-testkit/src/link/`). Prompt 25 may have added testkit fakes for `CapabilityRegistry` and `TargetResolver`; use them when present, otherwise add `FakeRegistry` (answers a configured `Availability` per `(host, contract)`, default `Available`) and `FakeTargetResolver` (returns `host:{id}/{fingerprint}` for the enrolled key the `InMemoryStore` holds) **(new)**. Tests never sleep: they advance `FakeClock`.

### 12.1 Core (`crates/domovoi-core/tests/operation_trace.rs` and unit tests in the new modules)

| Test | Setup, action, expected |
|---|---|
| `operation_status_names_round_trip` | every `OperationStatus::NAMES` entry parses back; `is_terminal` is true for exactly four; `is_waiting` for exactly four |
| `step_kinds_outcomes_and_edge_kinds_round_trip` | `ALL` arrays round-trip through `as_str`/`parse`; unknown text is `None`; counts are 11, 10 and 5 |
| `execution_id_from_step_keeps_the_uuid` | `ExecutionId::from_step(s).as_uuid() == s.as_uuid()`; `as_step` inverts; `as_action` gives the same UUID |
| `execution_state_is_derived_from_the_row_and_the_step_outcome` | table: no approval + approval step `Pending` → `AwaitingApproval`; approval stored + step `Pending` → `Authorized`; `Running` → `Dispatched`; each final outcome → its same-named state |
| `authorize_with_the_displayed_hash_records_the_approval` | `Authorization::human(HumanApproval::new(..))` (tests may construct it), `PlanBinding::Full(plan_hash)` → `approval()` is `Some` with the approver, channel, time, displayed hash and provenance |
| `authorize_with_another_hash_changes_nothing` | a different hash → `ExecutionError::PlanChanged`, `approval()` still `None` |
| `authorize_with_a_policy_permit_is_not_yet_supported` | `Authorization::policy(PolicyPermit::issue(..))` → `NotYetSupported` |
| `authorize_after_expiry_is_refused`, `second_authorize_is_refused` | `now >= expires_at` → `Expired`; a second call → `AlreadyAuthorized` |
| `counted_budgets_allow_exactly_the_limit` | limit 2: `is_spent` false at used 0 and 1, true at 2; `None` is never spent; cycles use the argument |
| `measured_budgets_overrun_only_past_the_limit` | tokens limit 100: used 100 → `None`, 101 → `Some(Tokens)`; first listed kind wins when several overrun |
| `trace_orders_steps_by_ordinal_and_sorts_edges` | shuffled input snapshot → steps in ordinal order, edges by (ordinal of from, ordinal of to, kind) |
| `trace_waiting_for_lists_waits_in_priority_order` | open Approval, Question, Verification steps → `["approval","user","timer"]` |
| `trace_summary_is_cut_at_280_characters_on_a_character_boundary` | a 300-character multibyte summary becomes exactly 280 characters, valid UTF-8 |
| `trace_omits_output_intent_and_hashes` | serialise a snapshot with output and plan hashes; the JSON contains none of them |
| `trace_fixtures_match_the_builder` | each scenario equals its file in `web/src/test/fixtures/operation-trace/` (reads `concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/src/test/fixtures/operation-trace/")`); `OPERATION_TRACE_UPDATE=1` rewrites |
| `trace_fixtures_cover_every_kind_outcome_and_edge` | the union over the five files holds all 11 kinds, 10 outcomes and 5 edge kinds |
| `operation_trace_json_snapshot`, `step_intent_json_snapshot`, `operation_status_json_snapshot` | `insta::assert_json_snapshot!`; review every `.snap` |
| `journal_kinds_round_trip`, `aggregate_kinds_round_trip` | the existing tests extended with the six new kinds and `Operation` |

### 12.2 Contract suites (`crates/domovoi-testkit/src/contracts/operations.rs`; run on `InMemoryStore` in `crates/domovoi-testkit/tests/contracts.rs` and on SQLite in `crates/domovoi-store/tests/sqlite/contracts.rs`)

`operation_commit_writes_operation_steps_edges_journal_audit_and_effects_together`; `operation_new_over_existing_id_conflicts`; `operation_stale_version_conflicts_and_changes_nothing`;
`step_progress_is_guarded_by_the_stored_outcome`; `step_ordinals_are_dense_and_unique_within_an_operation`; `edge_to_a_later_or_equal_ordinal_is_refused`; `edge_across_operations_is_refused`;
`edge_to_a_missing_step_is_refused`; `execution_row_needs_an_execution_step_and_is_one_to_one`; `execution_version_guard_refuses_a_stale_write`;
`execution_id_equal_to_an_action_id_is_refused`; `execution_round_trips_a_human_approval_exactly`; `snapshot_load_returns_steps_in_ordinal_order_with_edges_and_executions`;
`dispatched_on_lists_only_running_executions_of_the_host`; `dispatched_since_counts_by_host_contract_and_time`; `with_status_returns_oldest_first`;
`incident_link_is_idempotent`; `cancel_effects_cancels_only_pending_effects_of_the_aggregate` (a claimed effect and another aggregate's pending effect survive);
`claim_filters_by_aggregate_kind`; `revert_claimed_at_start_is_scoped_to_aggregates`; `an_empty_aggregate_scope_claims_nothing`;
`failed_operation_commit_rolls_back_every_stage` (a later invalid write rolls back earlier operation, step, journal, audit and effect writes).

SQLite only (`crates/domovoi-store/tests/sqlite/operations.rs`): `migration_adds_the_operation_tables_and_keeps_existing_rows` (N-1 fixture); `step_intent_columns_cannot_be_updated`
(raw `UPDATE steps SET intent = ..` aborts with the immutability message, `UPDATE steps SET status = ..` succeeds); `step_edge_trigger_refuses_a_backwards_edge_written_by_raw_sql`;
`unknown_stored_operation_status_is_a_store_error` (raw insert of `status = 'zzz'`, `get` returns `StoreError::Backend` naming it).

### 12.3 `decide` (`crates/domovoi-engine/tests/operations/decide.rs`, pure, `CounterIds`, no store)

`admit_moves_queued_to_running_and_sets_started_at`; `admit_creates_a_wall_time_wake_only_with_a_wall_limit`; `propose_refuses_kinds_the_writer_owns` (Execution, Approval, Verification, PolicyDecision);
`propose_assigns_cycle_and_dense_ordinals`; `propose_births_each_kind_in_its_outcome` (table); `edges_must_point_to_committed_steps`; `edge_kind_endpoint_rules_are_enforced`;
`evidence_request_creates_a_collect_effect_and_counts_the_budget`; `evidence_budget_spent_stores_a_refused_step_and_pauses`;
`question_creates_a_timeout_wake`; `answer_creates_a_note_step_with_an_answers_edge_and_finishes_the_question`;
`execution_proposal_creates_approval_then_execution_with_depends_on_and_a_row`; `execution_proposal_for_an_unpinned_target_is_stored_refused`;
`no_blind_retry_of_failed_step` (same contract/host/subject/args as a `Failed` and as an `OutcomeUnknown` step → `Refused`); `retry_with_a_stated_change_is_linked_with_retry_of`;
`budget_exhaustion_pauses` (executions limit 1, second proposal → `Refused` step and `Paused{Budget{Executions}}`); `resume_needs_an_extended_budget`; `resume_with_an_extended_budget_returns_to_running`;
`authorization_creates_one_dispatch_effect_and_finishes_the_approval`; `denial_refuses_approval_and_execution`; `expiry_marks_approval_and_execution_not_run`;
`settle_enforces_allowed_outcome_pairs` (table); `successful_settle_creates_verification_with_verifies_edge_and_a_delayed_verify_effect`;
`outcome_unknown_pauses_the_operation`; `verification_passed_succeeds_the_operation_when_nothing_is_open`; `verification_passed_with_open_steps_keeps_running`;
`failed_verification_starts_new_cycle` (cycle 1, `BuildContext` effect with cycle 1 and `dedup_key build:<op>:1`); `cycle_limit_escalates`;
`cancel_cancels_pending_steps_and_keeps_a_dispatched_execution_running`; `terminal_operation_refuses_new_steps_but_records_a_late_settle`; `paused_operation_refuses_new_steps`;
`finish_skips_open_steps_and_refuses_while_an_execution_is_in_flight`; `status_shows_the_highest_priority_wait` (Approval beats User beats Agent beats Timer);
`tick_pauses_on_wall_time_overrun`; `decide_does_not_mutate_its_input` (clone before/after compare).
**Property test** `decide_never_breaks_trace_invariants` (`proptest`, `tests/operations/properties.rs`): generate sequences of up to 40 events from a menu over a growing state; after every accepted event assert **I1–I7** of 12.6.

### 12.4 Writer, boundary and handlers (`crates/domovoi-engine/tests/operations/{writer,execution,effects,restart}.rs`)

Harness (`harness.rs`): `InMemoryStore`, `FakeClock`, `FakeAgentLink` with one host offering `restart_container` (the `restart_skill()` helper of `tests/services/harness.rs`), the fake registry and resolver, a real `OperationService` and runner.

| Test | Setup, action, expected |
|---|---|
| `commit_writes_state_journal_audit_and_effects_together` | `create` then `propose_execution`; read store: operation, steps, edges, execution row, journal rows (`operation.created`, `operation.step_created` ×3), audit rows and the `Wake` effect are all present; with the store set to fail the audit insert nothing is present |
| `stale_operation_version_is_reloaded_and_retried_once` | write the operation row behind the writer's back; the command succeeds after one reload; a second concurrent change fails with `Conflict` |
| `authorize_requires_a_human_authorization` | a policy `Authorization` → `Execution(NotYetSupported)`, no row change, no audit row |
| `approve_with_the_wrong_plan_hash_changes_nothing` | `PlanBinding::Full(other)` → `PlanChanged`; execution still `AwaitingApproval`; no `Dispatch` effect |
| `approving_creates_a_dispatch_effect_and_an_audit_row_in_one_commit` | after `authorize`: approval step `succeeded`, one `dispatch` effect, audit `execution.approved` with the user |
| `deny_records_refusal_and_audit` | `deny` → approval and execution `refused`, audit `execution.denied`, no effect |
| `expired_authorization_settles_not_run` | advance the clock past `expires_at`; run the runner; both steps `not_run`, nothing dispatched, `FakeAgentLink::requests()` has no skill run |
| `dispatch_runs_the_skill_under_the_step_id` | approve, run; the link saw exactly one skill request whose action id equals the step id; step `succeeded` then a `Verify` effect |
| `dispatch_is_idempotent_by_step_id` | invoke the `Dispatch` handler twice for the same effect record, as a duplicate delivery would; the second finds the execution no longer `Pending` and completes without sending; exactly one skill request reaches the link |
| `dispatch_refused_when_the_plan_changed` | change the skill's manifest hash after approval; step `refused`, audit `execution.refused`, no request sent |
| `dispatch_refused_when_the_host_was_re_enrolled` | change the enrolled key; the recomputed hash differs; step `refused` |
| `dispatch_waits_for_an_absent_agent_then_runs` | availability `Absent`; operation `waiting_agent`, `waiting_since` set once; make the host `Known`, advance the clock one `retry_interval`; dispatch happens |
| `dispatch_becomes_unavailable_after_the_wait_limit` | stay absent past `agent_wait_limit`; execution `unavailable`; operation back to `running` |
| `dispatch_to_an_incompatible_host_is_unavailable_at_once` | `HostIncompatible` → `unavailable("agent upgrade required")` |
| `dispatch_refused_when_the_registry_says_disabled` | `DisabledByHub` → `unavailable`, never sent |
| `non_skill_contract_is_stored_refused_at_proposal` | contract `docker.restart` → step `refused`, no execution row |
| `loop_guard_blocks_the_fourth_run_in_an_hour` | three completed runs, the fourth is `refused` with audit `execution.blocked`; after the clock passes an hour a new one is allowed |
| `failed_run_settles_failed_and_creates_no_verification` | script exit code 1 → `failed`; no verification step |
| `verification_failure_starts_a_new_cycle` | verify check still firing → verification `failed`, operation cycle 1, one pending `build_context` effect, no handler claims it |
| `verification_pass_succeeds_the_operation` | check resolved → operation `succeeded`; pending effects of the operation are `cancelled` |
| `reconcile_settles_a_dispatched_execution_after_restart` | dispatch with the link held, drop the service, build a new one on the same store, script `answer(Finished{succeeded: true})`; the step becomes `succeeded`; the link saw one request |
| `reconcile_with_a_changed_epoch_settles_outcome_unknown_and_pauses` | change the journal epoch; step `outcome_unknown`, operation `paused` with `outcome_unknown` |
| `reconcile_not_found_settles_not_run` | `ActionAnswer::NotFound` → `not_run` |
| `hello_triggered_reconcile_creates_one_effect_per_dispatched_execution` | call `reconcile_host` twice → one pending `reconcile` effect per step (dedup) |
| `result_after_cancel_is_recorded` | cancel while dispatched, release the run; the step is `succeeded`, the operation stays `cancelled`, no verification step |
| `cancel_cancels_pending_effects_and_steps` | pending `Wake` and `Dispatch` effects become `cancelled` in the same commit |
| `operation_resumes_after_restart` | create an operation with an evidence request, stop the service before the runner claims, start a new service on the same store; the effect completes, the step `succeeded` |
| `startup_returns_a_claimed_dispatch_effect_to_pending` | seed a `claimed` `dispatch` effect for an operation whose execution is `Pending`; start; it is re-pended and the dispatch happens once |
| `evidence_unavailable_host_settles_unavailable_and_transport_error_retries` | `NotConnected` → step `unavailable`; a scripted timeout twice then success → `succeeded` with `attempt` 3 |
| `operation_runner_never_claims_legacy_wake_effects` | with a legacy `wake` effect (aggregate `action`) pending, run the operation runner: the effect stays `pending` and untouched; a pending operation `wake` is untouched by the legacy runner |
| `legacy_action_completes_under_original_id` | with the flag-on service running, a legacy `Approved` action is dispatched by the legacy executor under its `ActionId`; it never appears in `executions` |
| `max_running_queues_the_rest_and_admits_on_completion` | `max_running = 1`: second operation `queued`; finishing the first admits it |
| `budget_exhaustion_pauses` (writer level) | executions limit 1; second proposal → `Refused` step, operation `paused`, audit `operation.paused` |

### 12.5 Hub, gates and arch checks

| Test | Where | Expected |
|---|---|---|
| `orchestration_is_disabled_by_default` | `crates/domovoi-config/tests` or the config unit tests | `HubConfig::default().orchestration.enabled == false`; an empty `hub.toml` parses |
| `orchestration_rejects_zero_limits` | config validation | `max_running = 0`, `loop_guard_per_hour = 0`, a zero duration and a zero budget each fail with the key named |
| `orchestration_section_is_in_the_generated_reference` | `cargo xtask docs-config --check` | the page lists every key of 6.8 |
| `flag_off_starts_no_operation_service` | `crates/domovoi-hub/src/runtime/tests.rs` or `tests/hub` | with the default config the runtime holds no `OperationHandle` and the action service behaves as before |
| `flag_on_starts_the_operation_service_and_the_composite_reconciler` | same | the handle is alive; an authenticated `hello` for a host with a dispatched execution creates a `reconcile` effect and the legacy reconciler also runs |
| `operation_state_is_written_only_through_the_unit_of_work` | `crates/domovoi-engine/tests/gates.rs` (extend; never weaken the existing gates) | a scan of `crates/domovoi-engine/src` and `crates/domovoi-hub/src` finds no `INSERT INTO`/`UPDATE` of `operations`, `steps`, `step_edges`, `executions`, `operation_incidents` and no `OperationWrite` constructed outside `crates/domovoi-engine/src/operations/` |
| `operations_never_name_the_raw_link` | `gates.rs` | no `AgentLink`, `run_skill`, `run_tool`, `run_check`, `start_skill` in `operations/` |
| `model_cannot_reach_the_operation_writer` | `gates.rs` | `ReadTool::ALL` is unchanged and no tool schema mentions `propose_execution`, `authorize` or `deny` |
| arch-check unit tests (`xtask/src/arch/ast_tests.rs`) | xtask | for each card of 6.5: the exact allowed site passes; a second `hand_off(`, a second `AuthorizedStep::of`, a second `OperationCommand::Dispatch {` construction, or one in another file fails with `EXEC-2` |

### 12.6 The simulator and the trace invariants

`crates/domovoi-engine/tests/operations/sim.rs` **(new)** is a deterministic driver, not a mock. It is seeded (`u64`, a small LCG in the file; no new dependency), builds the harness of 12.4,
creates one operation with two pinned subjects and then, for `N = 60` steps, picks an action from a seeded menu and applies it through the real `OperationHandle`:
propose an evidence request (the link answers success, an error or a timeout, by seed), propose an execution (success, failure, hang-then-answer, or unknown), approve or deny it, answer a question,
advance the clock by a seeded amount, drop and rebuild the service on the same store (a restart), change the host's availability, cancel (rare), finish (rare). A scripted handler for `BuildContext`
(registered **only in the simulator**) stands in for the model: it proposes one deterministic step per cycle. After **every** action it loads the snapshot, calls `build_trace`, and asserts:

- **I1** every step, edge and execution belongs to the operation; ordinals are `0..n` with no gap.
- **I2** every edge satisfies the 6.1 edge rules (so the graph is acyclic), and a step's `parent` has a lower ordinal.
- **I3** each Execution step has exactly one `executions` row, `execution == step`, and the link received at most one skill request for that id.
- **I4** the stored operation status equals `derive_status` of the steps, except `Paused`, `Queued` and terminal.
- **I5** a terminal operation has no `Pending` step; its only `Running` step can be a dispatched Execution.
- **I6** journal `seq` for the operation is strictly increasing and every step outcome change has a matching journal entry.
- **I7** budgets: counted `used` never exceeds its limit; `Paused{Budget}` only when a limit is spent.
- **I8** no `claimed` effect of a terminal operation remains after the runner drains.
- **I9** no audit row or journal payload contains the text of any `output` (the simulator seeds outputs with a marker string).

Tests: `simulator_keeps_the_invariants_for_seeds_0_to_199`; `simulator_restart_equivalence` (for 20 seeds, the run with restarts and the run without end with the same trace after replacing ids by ordinals; a restart never changes
the outcome of any step); `simulator_replays_the_same_trace_for_the_same_seed` (determinism: same seed twice, identical id-normalised trace). On a failure the test prints the seed.

Web (`web/src/test/fixtures/operation-trace/trace.test.ts`): `operation trace fixtures satisfy the contract` — for each of the five files: `schema_version === 1`; step ids unique; every `parent` and every edge endpoint names a step in the same file; every edge `from` appears after its `to` in `steps` (ordinal rule); every `refs` item has kind `execution` and an id that is a step id; no `output`, `intent` or `plan_hash` key anywhere; the union covers all kinds, outcomes and edge kinds; `waiting_for` is a sub-sequence of `["approval","user","agent","timer"]`.

## 13. Acceptance criteria

1. With `[orchestration]` absent or `enabled = false`, no operation service starts; the legacy action and triage tests, the six triage drills and every existing gate pass unchanged, and `cargo xtask arch-check` is green.
2. The migration adds `operations`, `operation_incidents`, `steps`, `step_edges` and `executions` with the triggers and checks of section 8, changes no existing table, and the N-1 fixture upgrades with every old row intact.
3. A step's `intent`, `kind`, `phase`, `parent`, `cycle`, `ordinal` and `created_at` cannot be updated (trigger and writer); an edge to a later or equal ordinal, a missing step or another operation's step is refused by the store.
4. One `OperationWrite` set per command commits state, journal, audit, new effects and the held effect's completion in a single transaction; an injected audit failure leaves nothing behind.
5. `decide` is pure (no I/O, no clock, no randomness; input never mutated), implements the tables of 6.2 exactly, and the property test holds invariants I1–I7 over random event sequences.
6. A proposed execution creates an `Approval` step, an `Execution` step with a `depends_on` edge, one `executions` row whose id equals the step id, and a `Wake` expiry effect; it is stored `Refused` (no row, no approval step) when the target is not pinned, the contract is not a skill, the arguments are invalid, or it repeats a `Failed`/`OutcomeUnknown` run without `retry_of` and a stated change.
7. `authorize` accepts only a human `Authorization` whose binding names the stored v2 hash; a policy authorization, a wrong hash, an expired window and a second approval each change nothing.
8. Dispatch happens only through `execute_step` from the `Dispatch` handler; the agent receives exactly one skill request per execution, under `execution_id == step id`, carrying the v1 wire hash; the recomputed v2 hash must equal the stored one.
9. An absent agent leaves the operation `WaitingAgent` and retries by `Wake`; past `agent_wait_limit` the step is `Unavailable`; an incompatible host is `Unavailable` at once; nothing blocks a task while waiting.
10. A successful run creates a `Verification` step with a `verifies` edge and a delayed `Verify` effect; a passed check succeeds the operation when nothing else is open; a failed check starts cycle `cycle + 1` with a `BuildContext` effect, or escalates at the cycle limit.
11. `OutcomeUnknown` pauses the operation; cancel records a late result without recalling the run; restart never dispatches twice and reconciliation settles a dispatched step.
12. A spent counted budget stores a `Refused` step and pauses the operation; `Resume` requires an extended limit; wall time, tokens and cost pause on overrun.
13. `claim_due` and `revert_claimed_at_start` are scoped to aggregate kinds; the legacy runner never claims an operation effect and the operation runner never claims a legacy one; `EffectCancel` cancels only pending effects of one aggregate.
14. `build_trace` produces the JSON of 6.10; the five fixtures in `web/src/test/fixtures/operation-trace/` equal the Rust output, together cover every `StepKind`, `StepOutcome` and `EdgeKind`, and the web test passes.
15. The arch cards of 6.5 are in place with unit tests; there is exactly one `hand_off` and one `await_handoff_result` call in each of `actions/execute.rs` and `operations/execute.rs`; the ADR is committed.
16. The simulator passes seeds 0–199 with invariants I1–I9, and the restart-equivalence and determinism tests pass.
17. `cargo xtask docs-config` is current and lists `[orchestration]`; no HTTP route, OpenAPI file or wire message changed.

## 14. Migration and compatibility

- **Existing installs.** The migration only adds tables; they stay empty while the flag is off. Old binaries ignore them (`schema_compat.min_reader` unchanged), so a rollback to the previous release needs no restore.
- **Legacy actions.** Untouched. `actions` rows keep their ids, statuses, approvals and effects; pending legacy actions finish on the legacy path under their `ActionId`; the legacy runner now claims only `incident`/`action`/`note` aggregates, which is every effect that exists today.
- **Turning the flag on.** A restart is required. Operations created afterwards use `execution_id = step_id`. Nothing converts, projects or expires legacy actions here (27, and the removal prompt, own that).
- **Agents.** No agent change, no protocol change. An agent cannot tell a step id from an action id.
- **Config.** `[orchestration]` is optional; the defaults of 6.8 apply. Removing the section disables the feature.
- **Retention.** `operation_journal_days` remains "not applied yet"; operation rows, steps and edges are kept, as D-21 says. Say so in the PR.

## 15. Documentation requirements

- **ADR** `docs/adr/NNNN-operation-aggregate.md` (next free number; it is required because `xtask/src/arch/rules.rs` changes, `xtask/src/arch/git.rs:82-95`). Record: the Operation aggregate and its single writer; steps immutable with a derived projection and no second execution status; the ordinal rule that makes the graph acyclic; the **interim second dispatch site** and the exact cards, and that the legacy site goes when the legacy executor does; the aggregate-scoped claim and why kind-only claims were unsafe; `EffectCancel`; the decision that only `skill.<name>` executes; the decision that no policy evaluation exists yet so every execution needs a human. Add it to `docs/adr/README.md`'s index.
- **`ARCHITECTURE.md`** (D-15): §5 add the Operation aggregate (steps, edges, execution, status machine); §6 ports: `OperationRepo`, `OperationWrite`, `EffectCancel`, the aggregate scope on `EffectQueue`; §7.1 the operation service behind `orchestration.enabled`; §9 the five tables and the immutability triggers; §12 the second execution site and the cards; §14 `[orchestration]`; §15 the unchanged approval invariant for steps.
- **Book:** `docs/book/src/concepts/operations.md` (new, linked from `SUMMARY.md`): what an operation, step and edge are; that the feature is off by default and has no screen yet; the keys of `[orchestration]` and what each does; that an operation's executions need a person's approval. Run `cargo xtask docs-links`.
- **Config reference:** `cargo xtask docs-config`, commit the regenerated `docs/book/src/reference/configuration.md`.
- **OpenAPI:** not applicable (no endpoint). Do **not** run `cargo xtask openapi`.
- **Rework planning documents:** do not edit `docs/rework/`.

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
