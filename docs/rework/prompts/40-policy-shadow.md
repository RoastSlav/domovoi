# 40 — Policy versions, shadow decisions, simulator and publication

| | |
|---|---|
| Branch | `policy-shadow` |
| Milestone | M7 |
| Depends on (must be merged) | 27 (Investigate operation: Execution steps are created and read through `OperationHandle`/`OperationRepo`), 39 (pure evaluator), and everything they need: 24, 25, 26, 28, 29. **24, 25, 26, 27, 28, 29 and 39 are written but may not be merged when this brief is read: every name taken from them is "introduced by prompt NN — verify in code before use".** Also needs 22 and 23 (merged) |
| Can run in parallel with | 35, 36 (order-flexible, never concurrent). 34, 36, 42 and 59 also add migrations: this prompt runs only after the previous migration-adding prompt has merged |
| Produces (shared contracts) | Tables `policy_versions`, `policy_rules`, `policy_acknowledgements`, `policy_shadow_decisions`, `policy_shadow_state`; `AggregateKind::Policy`, `StateWrite::Policy(PolicyWrite)`, `JournalKind::PolicyPublished`; ports `PolicyRepo` (core) and `ShadowEnvironment` (engine); `domovoi_engine::policy_shadow::{ShadowRecorder, RecordedInputs, DecisionDoc, …}`; hub `services::PolicyService`; HTTP `/api/v1/policy/*`; config `[policy] shadow`; audit kinds `policy.published`, `policy.lock_changed`, `policy.draft_saved`, `policy.draft_discarded`. Consumed by 43 (policy pages) and 45 (permit issuance reads published versions) |
| Consumes | 39: `domovoi_engine::policy::{evaluate, evaluate_shadow, simulate, validate_rules, validate_publication, names_to_resolve, resolve_in_index, EvaluateRequest, Decision, Outcome, OutcomeReason, HardLimit, PublicationReport, Finding, FindingKind, PolicyError, RuleSelection, Rule, RuleId, ContainmentInput, AutonomyState, CanonicalArgs, FactState}`. 24/25: `ContractIndex`, `ContractMeta`, `CeilingSummary`, `FactId`, `TargetName`, `TargetIdentity`, `CapabilityRegistry`, `TargetResolver`. 26/27: `OperationRepo`, `ExecutionRecord`, `PinnedSubject`, the `executions` table. 28: the journal topic mapping only (policy kinds map to no topic). 29: `Services`, `Caller`, `ServicesDeps`. Existing: `UnitOfWork`, `AuditDraft`, `AuthState::confirm_password`, `STEP_UP_ACTIONS` |
| Launch gate | none. `TimeWindow` rules are evaluated in UTC (owner decision D-30) through 39's `Condition::holds`; the overview reports `time_window_zone = "UTC"` |
| Suggested model | medium |

## 1. Objective

Make the policy of `04` §7 **real data and observable, without enforcing anything**:

1. **Policy versions.** The owner can keep one editable **draft** rule set, validate it, publish it as immutable version 1, 2, 3 …, and read every past version. Exactly one published version is *active*.
2. **Shadow decisions.** With `[policy] shadow = true`, every Execution step (one `executions` row, prompt 26) gets one recorded **decision trace**: what the active policy would decide for it (`evaluate`) and what its shadow rules would decide (`evaluate_shadow`), with the exact inputs used, so the evaluation can be replayed later. **Nothing is enforced**: the approval flow, dispatch and the agent behave exactly as before; no code reads a shadow decision to decide anything; **no `PolicyPermit` is issued anywhere** (RW-11: permit issuance is M8, prompt 45).
3. **Honest inputs.** Several evaluator inputs come from the host and do not exist before H1/M8 (ceiling summary, helper attestation, fact producers, autonomy state). The recorder substitutes the fail-closed value the evaluator expects and records, in the same row, exactly which inputs were unavailable and why. It never blocks, retries forever or invents a value.
4. **Simulator endpoint.** `POST /api/v1/policy/simulate` evaluates a draft, a version or inline rules over historical Execution steps (replaying the recorded inputs) or current values, and shows a diff against the recorded decision.
5. **Publication report.** Publishing runs 39's `validate_publication`; the owner must acknowledge every blocking finding (D-19), and publishing needs **step-up** (password). Creating, editing, unlocking or removing a **locked Deny** also needs step-up, a simulation preview of affected decisions, and an audit entry `policy.lock_changed` (D-26).

With `[policy] shadow = false` (the default) **no decision is recorded**; the policy store, editor endpoints, report and simulator still work (they are configuration and read-only analysis). No UI is built here (prompt 43); this prompt lists the DTOs 43 consumes.

## 2. Architectural context

**RW-10 (policy), D-05/D-19/D-23/D-26.** The evaluator (prompt 39) is a pure function. The most specific matching rule wins, even over a broader `Deny`; incomparable conflicting winners fall back `Deny > RequireApproval > Allow`; `Deny` means "not offered", `Allow` means unattended, `RequireApproval` means offered; a `locked` `Deny` is an absolute prohibition. Ten **hard limits** then apply after rule resolution (ceiling, pinning, safety facts, containment, root agent, autonomy). The result is a `Decision` trace. **This prompt owns what 39 deliberately left out**: storing rules in versions, recording decisions, loading history, publication workflow, acknowledgements, step-up and audit.

**RW-11 (authorization types).** `PolicyPermit` exists in `domovoi-core` (prompt 22) but `PolicyPermit::issue` and `Authorization::policy` may be called only under `crates/domovoi-hub/src/policy/` (prompt 45) and in tests. This prompt creates **no** `crates/domovoi-hub/src/policy/` directory and calls neither. A `Decision` is data; turning an `Allow` into a permit is prompt 45's `PermitIssuer`. A gate test pins this (section 12).

**RW-12/D-03 and honesty before M8.** The unattended ceiling lives on the agent. Until prompt 45 ships it, `hello.ceiling_summary` is `None` for every agent (prompt 25 §6.7 item 7), the helper (H1) does not exist, no fact producer exists and the autonomy pause state is not modelled. If the recorder passed `ceiling = None` and said nothing more, every recorded `Allow` would silently read as "RequireApproval because of hard limit 4" with no explanation. So each row carries `gaps` and `degraded`.

**RW-15 (one journal).** A published policy version is a journal row (`policy.published`) and an audit row. Shadow decisions are **not** journalled and **not** audited per row: they are derived, replayable records (the inputs are stored), high volume, and carry no authority.

**RW-19 (use-case layer, prompt 29).** Every policy *command* (save draft, discard draft, publish) is a method on `crate::services::PolicyService` taking `&Caller`; handlers only authenticate, parse and map. Plain reads may stay in `api/` handlers. Never add an engine handle to `ApiDeps`/`ChatDeps`; the service goes into `Services`. A new top-level hub module must be in `ZONES`/`UNZONED_MODULES` (`xtask/src/arch/rules.rs`): this prompt adds none (the recorder wiring lives in `runtime/`, which is unzoned).

**RW-24 (step-up).** A change that widens authority needs the admin's password again even with a valid session. The existing pattern: password in the request body, `proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies)`, then `AuthState::confirm_password(ip, user_id, password)`; a wrong or missing password is 403, a lockout is 429 and shares the login limiter. `STEP_UP_ACTIONS` already declares `policy.publish`.

**Decisions fixed here (do not revisit):**

| # | Decision | Why |
|---|---|---|
| P1 | Shadow decisions are written by a **sweep**, not by the operation writer: the `ShadowRecorder` task repeatedly asks the store for Execution steps without a decision row and records one each. No edit to prompt 26's `decide` or writer | keeps the operation writer the only creator of safety-relevant steps; idempotent (`INSERT … ON CONFLICT DO NOTHING`), crash-safe with no cursor, and a recorder failure cannot block an operation |
| P2 | One row per Execution step (`execution_id` primary key) holding **both** the enforced-rules decision and the shadow-rules decision. Re-evaluating under a later version is the simulator's job, not a second row | simple, and the M7 acceptance ("the simulator reproduces each recorded decision") needs exactly one recorded decision per step |
| P3 | Executions with **no published version** are evaluated against the empty rule set (`version_number = 0`): the effect-class defaults of `04` §7.3 | an empty policy is a policy; it shows what the defaults would do |
| P4 | Rows are written for every Execution step with an `executions` row, whatever its effect class. A step stored `Refused` has no `executions` row and gets none. `agent.package.*` contracts are never evaluated (section 5) | `04` §7.5 says the trace is persisted for every mutating or containing step |
| P5 | A draft edit that creates, edits, unlocks or removes a **locked** rule needs the password at that call; **publishing always** needs the password; a publish whose locked set differs from the active version must echo the server's `preview_hash` | D-26 literal reading plus RW-10's "simulation preview" made checkable |
| P6 | Rule bodies are stored as JSON text in an envelope `{"v":1,"rule":<Rule serde>}` pinned by a golden test | storage survives refactors of 39's types only through an explicit, tested format |

## 3. Repository context

Verify every item first. Marks: **(existing, path:line)** or **(new)**. Paths below were read in this checkout before prompts 24–29 and 39 merged; line numbers may have moved.

### 3.1 Existing code this prompt builds on

- `UnitOfWork::commit(Transaction)`; `Transaction { writes, journal, audit, new_effects, … }` with builders `.write`, `.journal`, `.audit` **(existing, `crates/domovoi-core/src/ports/uow/transaction.rs:149-260`, `mod.rs:25-37`)**. `AggregateKind { Incident, Action, Note }` with `as_str`/`parse` and `AggregateRef` constructors **(`transaction.rs:13-85`)**; `StateWrite` is a closed enum, each variant built only by its owning writer **(`transaction.rs:~188-230`)**; the store applies it in one `match` **(existing, `crates/domovoi-store/src/uow/writes.rs:104-139`)**. Prompts 26 and 34 each add one variant (`StateWrite::Operation`, `StateWrite::Knowledge`) with a matching `AggregateKind`; follow exactly that shape.
- `JournalKind` is closed with `as_str`/`parse` **(existing, `crates/domovoi-core/src/ports/uow/journal.rs:11-40`)**; `AuditDraft::new(actor, at, kind, subject, detail)` **(`journal.rs:93-110`)**. Unknown journal kinds map to no SSE topic (prompt 28).
- Journal and effects DDL: `crates/domovoi-store/migrations/0006_unit_of_work.sql`; the migrations directory ends at `0011_plan_binding.sql` on this checkout. **Take the next free number on your branch; never hard-code one.**
- Step-up today: `crates/domovoi-hub/src/settings/step_up.rs` has `STEP_UP_SETTINGS` (enforced) and `STEP_UP_ACTIONS` (declared only: `policy.publish`, `capability.enable`, `installation.start`, `autonomy.resume`), and the test `step_up_list_declares_future_actions` **(existing, `step_up.rs:80-103` and its tests)**. `AuthState::confirm_password(ip, user, password)` **(existing, `crates/domovoi-hub/src/auth/mod.rs:280-305`)**; the settings handler shows the HTTP shape **(existing, `crates/domovoi-hub/src/api/settings_write.rs:156-191`)**; audit writes go through `AuditLog::append` or an `AuditDraft` in a transaction.
- API: routes in `routes()` with `utoipa` annotations, tags in `ApiDoc` **(existing, `crates/domovoi-hub/src/api/mod.rs:295-366`)**; `ApiError` variants `Forbidden`, `Conflict`, `TooMany`, `BadRequest`, `NotFound`, `Unavailable` **(existing, `api/error.rs:48-91`)**; `Authed(Current)` extractor **(`mod.rs:248-265`)**. OpenAPI output and `web/src/api/{openapi.json,schema.d.ts}` are generated, never hand-edited.
- Config: `HubConfig` and its sections in `crates/domovoi-config/src/hub/mod.rs` (one submodule per section, e.g. `retention.rs`); `foreign_sections()` returns `AGENT_SECTIONS` **(existing, `hub/mod.rs:497-499`)**, and `AGENT_SECTIONS` **already contains `"policy"`** because `agent.toml` has a `[policy] allow_risk` table **(existing, `crates/domovoi-config/src/env.rs:32-39`, `agent.rs:86-90`)**. See section 6.11.
- Clock: `Clock::now()` and `sleep_until(t)` **(existing, `crates/domovoi-core/src/ports/clock.rs`)**. Engine tasks stop on a `CancellationToken` (`tokio-util` is already an engine dependency, `crates/domovoi-engine/Cargo.toml:25`), as in `effects/runner.rs:34,95`.
- Typed ids use the `define_id!` macro **(existing, `crates/domovoi-core/src/id.rs:8-58`)**. Repository traits live in `crates/domovoi-core/src/ports/repos/` with a store impl in `crates/domovoi-store/src/repos/` and an in-memory twin in `crates/domovoi-testkit/src/repos/`; contract suites in `crates/domovoi-testkit/src/contracts/` run against both.
- `PolicyPermit::issue` and `Authorization::policy` are scanned by `xtask arch-check`; allowed prefix `crates/domovoi-hub/src/policy/` **(existing, `xtask/src/arch/rules.rs:178-186`, `scan.rs:212-231`)**. That directory does not exist.
- The engine already has a module `policy` (`crates/domovoi-engine/src/policy.rs`, holding `EnginePolicy`; prompt 39 adds `policy/` submodules). The new recorder module is named **`policy_shadow`** so that 39's own gate (no `HumanApproval`/`PolicyPermit`/`Authorization` text anywhere under `src/policy/`) stays valid.

### 3.2 What other prompts provide (introduced by — verify every name in code)

- **39**: the entry points and types in the "Consumes" row. `EvaluateRequest` is `Debug, Clone, PartialEq` only; `Decision` has no `Serialize`. 39 §6.5 lists every `EvaluateRequest` field (host, contract, contract_version, target, target_identity, args, requested_rate_per_hour, requested_ttl, pinned, rule_targets, ceiling, expected_ceiling_hash, facts, runs_as_root, containment, autonomy, now, policy_version). `Rule` derives `Serialize, Deserialize` with `RuleId`, `HostScope`, `CapabilityScope`, `TargetScope`, `Condition`.
  - **Likely defect to check in step 1.** 39 §6.2–6.4 puts `#[serde(tag = "kind")]` on `CapabilityScope` (newtype variants `Effect(EffectClass)`, `Category(..)`, `Contract(..)`), `TargetScope` (`Exact(BTreeSet<..>)`) and `FactState` (`Known(bool)`). serde cannot serialise an internally tagged newtype variant that wraps a string, a sequence or a bool: it fails at run time. Write the round-trip test of section 12 first. If it fails, the **one permitted edit of 39's code** is to change those three attributes to adjacent tagging (`#[serde(tag = "kind", content = "value")]`), with a unit test in 39's file; say so in the report. If 39 already fixed it, no edit.
- **24/25**: `ContractIndex::meta(&ContractId) -> Option<ContractMeta>`; `skill_contract(&SkillSummary, Option<SkillFloor>)`; skill contracts have categories `skill.read|skill.mutate|skill.destructive` by the **effective** class and are **built per host from `hello.skills`, not part of the compiled index** (24 §6.7, 25 §6). `CapabilityRegistry::{availability, catalog, contracts, effective_effect(host, &SkillName), enabled, enabled_skills}`. `TargetResolver::resolve(TargetQuery::{Host{host}, Container{host,name}, Unit{host,name}}) -> Result<ResolvedTarget{identity, name, kind, host_key}, TargetError>`; `Unit` always fails `UnitIdentityUnproven` (R-8 open). `HubRegistry::inventory(host)` reports `root_agent` per contract; `parse_ceiling` maps the wire summary to `CeilingParse::{Absent, Parsed, Malformed}`.
- **26/27**: table `executions` (`execution_id` primary key = step id, `operation_id`, `host_id`, `contract`, `contract_version`, `check_key`, `target`, `args` JSON object, `plan_hash`, `created_at`, …); `ExecutionRecord`; `OperationRepo::{get, load, …}`; `Operation.pinned: Vec<PinnedSubject{host, check, target: Option<TargetKey>}>`; `ExecutionId` (prompt 22, `ExecutionId::from_step`); `ExecutionState` derived from step outcomes. A step stored `Refused` has no `executions` row.
- **28**: `JournalFeed::wait_after(seq)` (completes when the committed head exceeds `seq`).
- **29**: `Services`/`ServicesDeps`, `Caller` (`caller.actor()`, `caller.user()`), arch rules `MOD-1..4`.

### 3.3 Tests today

`crates/domovoi-store/tests/sqlite/{main,uow,upgrade,migrations,compat,contracts}.rs` (N-1 upgrade pattern: `upgrade.rs` copies `tests/fixtures/n1-schema5.db`); `crates/domovoi-engine/tests/{engine,services,drills,crash}`; `crates/domovoi-hub/tests/hub/` (`api_harness.rs` builds the API; `api_routes.rs`), `crates/domovoi-hub/tests/gates.rs` (never weaken, extend); `crates/domovoi-testkit` (`InMemoryStore` in `src/repos/uow*.rs`, `FakeClock`, `FakeAgentLink`, contract suites under `src/contracts/`).

## 4. Exact scope

| Area | File | Change |
|---|---|---|
| core | `crates/domovoi-core/src/policy_store/` **(new)**: `mod.rs`, `version.rs`, `shadow.rs`, `write.rs` | `PolicyVersionId`, `PolicyStatus`, `PolicyVersionRecord`, `StoredRule`, `AckRecord`, `ShadowRecord`, `ShadowStatus`, `ShadowCandidate`, `ShadowSummary`, `PolicyWrite`, `DraftGuard`, constants. Serde only; no engine types |
| core | `crates/domovoi-core/src/id.rs` | `define_id!(PolicyVersionId)` |
| core | `crates/domovoi-core/src/ports/repos/policy.rs` **(new)**, `repos/mod.rs` | `PolicyRepo` |
| core | `ports/uow/transaction.rs`, `ports/uow/journal.rs`, `ports/uow/mod.rs`, `src/lib.rs` | `AggregateKind::Policy` (+`as_str`, `parse`, `AggregateRef::policy`), `StateWrite::Policy(PolicyWrite)`, `JournalKind::PolicyPublished`, re-exports; extend the existing round-trip tests |
| store | `migrations/NNNN_policy_versions.sql` **(new)** | section 8 |
| store | `crates/domovoi-store/src/policy/{mod,apply,rows,queries}.rs`, `repos/policy.rs`, `uow/writes.rs` (one arm), `lib.rs` | `PolicyRepo` for SQLite and the `StateWrite::Policy` applier |
| engine | `crates/domovoi-engine/src/policy_shadow/{mod,doc,codec,inputs,env,recorder,replay,report}.rs` **(new)** | section 6; `lib.rs` adds `pub mod policy_shadow;` |
| config | `crates/domovoi-config/src/hub/policy.rs` **(new)**, `hub/mod.rs`, `env.rs` (constants and the key-granular override skip) | `[policy] shadow` and the shared `[policy]` table (6.11) |
| hub | `src/services/policy.rs`, `src/services/policy_publish.rs` **(new)**, `services/mod.rs`, `ServicesDeps` | `PolicyService` (commands, report, simulate) |
| hub | `src/runtime/policy.rs` **(new)**, `runtime/mod.rs`, `runtime/settings.rs` | builds `HubShadowEnvironment`, the recorder (only when `config.policy.shadow`), `PolicyService`; the three `ApiDeps` constructors (`runtime/mod.rs`, `examples/dev_hub.rs`, `tests/hub/api_harness.rs`) gain `policy: Arc<dyn PolicyRepo>` |
| hub | `src/api/policy.rs`, `api/policy_shadow.rs`, `api/dto_policy.rs`, `api/dto_policy_trace.rs` **(new)**, `api/mod.rs` | routes, DTOs, tag `policy` |
| hub | `src/settings/step_up.rs` | `policy.publish` documented as enforced; new `policy.lock_changed` entry |
| testkit | `src/repos/policy.rs`, `repos/uow_apply.rs`, `src/contracts/policy.rs` **(new)**, `src/policy_env.rs` **(new)** | in-memory `PolicyRepo` + applier, contract suite, `FakeShadowEnvironment` |
| tests | store, engine, hub, gates | section 12 |
| docs | `docs/adr/NNNN-policy-shadow-recording.md`, `docs/book/src/concepts/policy.md`, `SUMMARY.md`, `reference/api.md`, regenerated `reference/configuration.md`, `ARCHITECTURE.md`, `web/src/api/{openapi.json,schema.d.ts}` (generated) | section 15 |

Every source file stays at or below 500 lines (R8); split by responsibility. Web source files do not change.

## 5. Explicit non-goals

- No enforcement of any kind: no approval, dispatch, ceiling or limiter decision reads a policy decision. No change to `ActionHandle`, the operation writer's `decide`, `OperationHandle::authorize`, the agent or the protocol.
- No `PolicyPermit`, `PermitIssuer`, `Authorization::policy`, `crates/domovoi-hub/src/policy/`, no autonomy switch, no pause switch (prompts 45).
- No policy editor, explanation, simulator or report **screens** (43). No change under `web/src` except generated API files.
- No change to the evaluator's algorithm (39). The only permitted edit of 39's files is the serde attribute fix of section 3.2, if needed.
- No fact producer, no helper attestation, no ceiling editing, no host tags.
- **Package contracts** (`agent.package.install`, `agent.package.uninstall`, `agent.package.list`) are **never policy-routed**: package installs are standalone jobs, not Execution steps (prompt 42), and the agent refuses a policy authorization for them. The recorder skips any contract id starting with `agent.package.`; a rule that names one by `CapabilityScope::Contract` is refused at save. The publication report carries a standing note saying so.
- No rule-level shadow promotion workflow (45). `Rule.shadow` is edited like any field.
- No journal or audit row per shadow decision. No retention policy for shadow rows (they are small; retention is a later change, say so in the report).
- No SSE topic for policy changes.
- No `JournalFeed` wake-up for the recorder: it polls on a timer (decision P1). No new `Actor` variant (`Actor::Policy` is for rules acting as principals, which is prompt 45).
- No host-existence check on `HostScope::Host(id)` at save time (hosts are removable; a rule naming a missing host simply never matches).
- No change to prompt 27's operation endpoints or DTOs. The decision for one step is read from `/api/v1/policy/shadow/{execution_id}`; prompt 43 links the two.

## 6. Required interfaces and contracts

Mark: **fixed** = names, variants, field names, JSON and SQL must match exactly; **convention** = follow repository style. Every public item gets a doc comment saying what it is for. Every name taken from prompts 24–29 and 39 is "introduced by prompt NN — verify in code before use"; prompt 24 is already merged (`crates/domovoi-core/src/capability/`).

### 6.1 Core types (`crates/domovoi-core/src/policy_store/`, no I/O) **fixed**

```rust
// crates/domovoi-core/src/id.rs
define_id!(/** Identifies one policy version, draft or published. */ PolicyVersionId);

// version.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum PolicyStatus { Draft, Published }          // as_str/parse: "draft" "published"

/// One policy version's header. Rules are read separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyVersionRecord {
    pub id: PolicyVersionId,
    pub status: PolicyStatus,
    pub number: Option<u32>,            // Some(1..) exactly when Published; never reused
    pub revision: u64,                  // starts at 1, +1 on every draft save; frozen at publication
    pub based_on: Option<u32>,          // the active version number when the draft was first created
    pub rules_hash: String,             // 64 lowercase hex; codec::rules_hash of the stored bodies
    pub rule_count: u32, pub locked_count: u32, pub shadow_count: u32,
    pub created_at: Timestamp, pub created_by: Actor,
    pub updated_at: Timestamp, pub updated_by: Actor,
    pub published_at: Option<Timestamp>, pub published_by: Option<Actor>,
}

/// One rule as stored: projections for filtering plus the opaque envelope text (decision P6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRule { pub rule_id: String, pub ordinal: u32, pub enabled: bool, pub shadow: bool, pub locked: bool, pub body: String }

/// The owner's acknowledgement of one blocking publication finding (D-19), stored with the version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AckRecord {
    pub finding_key: String, pub kind: String, pub first_rule: String, pub second_rule: String,
    pub resolution: Option<String>, pub note: String, pub acknowledged_by: Actor, pub acknowledged_at: Timestamp,
}

// write.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftGuard { Absent, Revision(u64) }       // Absent: no draft may exist; Revision(n): the stored draft is at n

/// The single new `StateWrite` payload. Variants 1-3 are built only by `PolicyService` (hub); variant 4 only by the recorder (engine).
#[derive(Debug, Clone)]
pub enum PolicyWrite {
    SaveDraft { id: PolicyVersionId, expected: DraftGuard, based_on: Option<u32>, rules: Vec<StoredRule>,
                rules_hash: String, actor: Actor, at: Timestamp },
    DiscardDraft { id: PolicyVersionId, expected_revision: u64 },
    Publish { id: PolicyVersionId, expected_revision: u64, number: u32, acknowledgements: Vec<AckRecord>, actor: Actor, at: Timestamp },
    /// `error` is set (with no records) when a sweep failed, so the failure is visible in `ShadowState::last_error`.
    RecordShadow { records: Vec<ShadowRecord>, sweep: SweepStats, at: Timestamp, error: Option<String> },
}
```

```rust
// shadow.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum ShadowStatus { Evaluated, Unevaluable, Skipped }     // as_str/parse

/// One Execution step waiting for its decision row (query result of `PolicyRepo::shadow_candidates`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowCandidate {
    pub execution: ExecutionId, pub operation: OperationId, pub host: HostId,
    pub contract: ContractId, pub contract_version: u32, pub check: CheckKey, pub target: Option<TargetKey>,
    pub args: BTreeMap<String, String>, pub pinned: Vec<PinnedSubject>, pub created_at: Timestamp,
}

/// One decision row. The four JSON columns are opaque to core: the engine owns their formats (`policy_shadow::doc`).
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowRecord {
    pub execution: ExecutionId, pub operation: OperationId, pub host: HostId, pub contract: String,
    pub step_created_at: Timestamp, pub evaluated_at: Timestamp,
    pub status: ShadowStatus,
    pub reason: Option<String>,                // snake_case code, set for Unevaluable and Skipped
    pub policy_version: u32,                   // 0 = no published version existed (decision P3)
    pub rules_hash: String,                    // of the rule set evaluated (the empty set has a hash too)
    pub enforced_outcome: Option<String>,      // "allow" | "require_approval" | "unavailable" | "deny" | "error"
    pub shadow_outcome: Option<String>,        // same vocabulary
    pub degraded: bool,                        // gaps is non-empty
    pub candidate: String,                     // CandidateDoc JSON
    pub inputs: Option<String>,                // InputsEnvelope JSON; None unless Evaluated
    pub enforced: Option<String>,              // DecisionEnvelope JSON
    pub shadow: Option<String>,                // DecisionEnvelope JSON
    pub gaps: String,                          // JSON array of InputGap
}
pub struct SweepStats { pub evaluated: u32, pub unevaluable: u32, pub skipped: u32 }
pub struct ShadowState { pub last_sweep_at: Option<Timestamp>, pub last_error: Option<String>,
                         pub total_evaluated: u64, pub total_unevaluable: u64, pub total_skipped: u64 }
pub struct ShadowFilter { pub host: Option<HostId>, pub contract: Option<String>, pub status: Option<ShadowStatus>,
                          pub enforced_outcome: Option<String>, pub differing: bool,    // enforced != shadow outcome
                          pub since: Option<Timestamp>, pub until: Option<Timestamp> }
pub struct ShadowSummary { pub since: Timestamp, pub total: u64, pub by_status: BTreeMap<String, u64>,
                           pub by_enforced_outcome: BTreeMap<String, u64>, pub degraded: u64, pub differing: u64 }
/// Hard bounds, constants in `policy_store/mod.rs`.
pub const MAX_RULES: usize = 500;  pub const MAX_NOTE_CHARS: usize = 500;
pub const MAX_ROW_JSON_BYTES: usize = 65_536;  // per JSON column of a ShadowRecord
```

`AggregateKind::Policy` (`as_str` = `"policy"`; `AggregateRef::policy(PolicyVersionId)`), `StateWrite::Policy(PolicyWrite)`, `JournalKind::PolicyPublished` (`"policy.published"`). Extend the existing `aggregate_kind_round_trips` and `journal_kinds_round_trip` tests. A shadow write has an empty `journal` and `audit` list and therefore changes no journal head.

### 6.2 `PolicyRepo` (`crates/domovoi-core/src/ports/repos/policy.rs`) **fixed**

Read-only port; every write is a `PolicyWrite` in a `Transaction`.

```rust
#[async_trait]
pub trait PolicyRepo: Send + Sync {
    /// The published version with the highest number, if any. This is the *active* version.
    async fn active(&self) -> Result<Option<PolicyVersionRecord>, StoreError>;
    async fn version(&self, number: u32) -> Result<Option<PolicyVersionRecord>, StoreError>;
    async fn by_id(&self, id: PolicyVersionId) -> Result<Option<PolicyVersionRecord>, StoreError>;
    /// Published versions, newest first.
    async fn versions(&self, page: &Page) -> Result<Paged<PolicyVersionRecord>, StoreError>;
    async fn draft(&self) -> Result<Option<PolicyVersionRecord>, StoreError>;
    /// Rules of one version in ordinal order.
    async fn rules(&self, id: PolicyVersionId) -> Result<Vec<StoredRule>, StoreError>;
    async fn acknowledgements(&self, id: PolicyVersionId) -> Result<Vec<AckRecord>, StoreError>;
    /// Execution steps with no decision row, oldest first (`created_at`, then execution id). At most `limit`.
    async fn shadow_candidates(&self, limit: u16) -> Result<Vec<ShadowCandidate>, StoreError>;
    async fn shadow(&self, execution: ExecutionId) -> Result<Option<ShadowRecord>, StoreError>;
    /// Newest `evaluated_at` first; keyset cursor over `(evaluated_at, execution_id)`.
    async fn shadow_records(&self, filter: &ShadowFilter, page: &Page) -> Result<Paged<ShadowRecord>, StoreError>;
    async fn shadow_summary(&self, since: Timestamp) -> Result<ShadowSummary, StoreError>;
    async fn shadow_state(&self) -> Result<ShadowState, StoreError>;
}
```
Contract suite `crates/domovoi-testkit/src/contracts/policy.rs` **(new)** runs on `InMemoryStore` and on SQLite (section 12.1).

### 6.3 The rule envelope and the hash (`policy_shadow/codec.rs`) **fixed**

```rust
/// `{"v":1,"rule":<Rule as serde_json>}`, compact, no whitespace. Decision P6.
pub fn encode_rule(rule: &Rule) -> String;
pub fn decode_rule(body: &str) -> Result<Rule, CodecError>;            // v != 1, extra top-level keys or a bad rule -> Err
pub fn stored_rules(rules: &[Rule]) -> Vec<StoredRule>;                // ordinal = position; projections copied from the Rule
pub fn decode_rules(stored: &[StoredRule]) -> Result<Vec<Rule>, CodecError>;  // ordinal order; projections must agree with the body, else Err
/// Lowercase hex SHA-256 of `"policy-rules-v1\n"` then, per rule in ordinal order, `rule_id`, `\n`, `body`, `\n`.
pub fn rules_hash(stored: &[StoredRule]) -> String;
pub enum CodecError { Version(u64), Json(String), ProjectionMismatch { rule: String } }   // thiserror
```
The empty rule set hashes to the SHA-256 of `"policy-rules-v1\n"` alone. `sha2` and `serde_json` are already engine dependencies (verify in `crates/domovoi-engine/Cargo.toml`); add nothing. The hub-side DTO for a rule is separate (6.9), so the envelope can change only through a new `v`.

### 6.4 The recorded documents (`policy_shadow/doc.rs`, `inputs.rs`) **fixed**

`Decision` (prompt 39) is not `Serialize` and `ContractMeta`/`ConstrainableArg` have no `Deserialize` (prompt 24), so the recorder stores its own mirror documents. They are plain serde structs; none holds a secret.

```rust
/// `{"v":1,"decision":<DecisionDoc>}` or `{"v":1,"error":"<PolicyError code>"}`.
pub enum DecisionEnvelope { Decision(DecisionDoc), Error { code: String } }
pub struct DecisionDoc {
    pub outcome: Outcome, pub reason: OutcomeReason,                  // 39's serde shapes, unchanged
    pub matched: Vec<RuleId>, pub winners: Vec<RuleId>, pub locked: Vec<RuleId>,
    pub overridden: Vec<Overridden>, pub would_have_been: Option<Box<DecisionDoc>>,
    pub constraints: ConstraintsDoc, pub hard_limits_applied: Vec<HardLimit>,
    pub facts_used: Vec<FactUse>, pub typed_confirmation: ConfirmationRequirement,
    pub warnings: Vec<Warning>, pub policy_version: Option<Uuid>, pub ceiling_hash: Option<String>,
}
pub struct FactUse { pub fact: FactId, pub status: FactStatus }
pub struct ConstraintsDoc { pub max_per_hour: Option<u32>, pub max_ttl_secs: Option<i64>,
                            pub targets: Option<BTreeSet<TargetName>>, pub conditions: Vec<Condition> }
impl DecisionDoc { pub fn from_decision(decision: &Decision) -> Self; }   // total, field for field; sorted as 39's determinism rules
pub fn error_code(err: &PolicyError) -> String;                           // the variant name in snake_case, e.g. "contract_not_indexed"; never owner text
```
`PolicyError` carries rule ids and owner-derived names, so only the code is stored. The trace carries no observed fact values (39 security rule 13); `FactUse` holds `FactStatus` only.

```rust
/// Everything `evaluate` was given, in a form that replays byte-for-byte. `{"v":1, ...}` envelope `InputsEnvelope`.
pub struct RecordedInputs {
    pub host: HostId,
    pub contract: ContractDoc, pub contract_version: u32,
    pub target: String,                         // TargetName text
    pub target_identity: String,                // TargetIdentity text
    pub args: String,                           // CanonicalArgs text, as given
    pub requested_rate_per_hour: Option<u32>, pub requested_ttl_secs: Option<i64>,
    pub pinned: BTreeSet<String>,               // TargetIdentity texts
    pub rule_targets: BTreeMap<String, String>, // name -> identity
    pub ceiling: Option<CeilingSummary>,        // as reported (Serialize + Deserialize in 24)
    pub facts: BTreeMap<String, FactStateDoc>,  // FactId text -> {"kind":"known","value":true} | {"kind":"unknown"} | stale | disputed
    pub runs_as_root: bool, pub containment: ContainmentDoc, pub autonomy: AutonomyDoc,
    pub now: String,                            // RFC 3339 UTC, the evaluation instant
}
pub struct ContractDoc { pub id: String, pub category: String, pub effect: EffectClass, pub target_kind: TargetKind,
                         pub safety_facts: Vec<String>, pub constrainable_args: Vec<ConstrainableArgDoc> }
pub struct ConstrainableArgDoc { pub key: String, pub kind: String /* "int_at_least" | "int_at_most" */, pub description: String }
```
Map `ConstraintKind` to and from its snake_case string with an explicit `match` (do not add `Deserialize` to prompt 24's types). `CandidateDoc` mirrors `ShadowCandidate` (ids as text, `args` as a JSON object, `pinned` as `[{host, check, target}]`). `InputGap`:

```rust
pub struct InputGap { pub input: GapInput, pub reason: GapReason }
#[serde(rename_all = "snake_case")] pub enum GapInput { Contract, Target, Ceiling, Facts, RunsAsRoot, Containment, Autonomy, PinnedTargets }
#[serde(rename_all = "snake_case")] pub enum GapReason {
    NotProducedYet,     // the feature that supplies it ships later (ceiling before the agent reports one, fact producers, helper, autonomy)
    HostNotConnected,   // no live session to ask
    Unresolvable,       // the hub tried and could not establish it (identity unproven, unknown contract)
}
```
**Golden snapshots** (`insta`, review each): one `RecordedInputs`, one `DecisionDoc` with every optional field populated, one `StoredRule` envelope per `Condition` variant, one `InputGap` list. They pin the stored formats.

### 6.5 `ShadowEnvironment` (`policy_shadow/env.rs`) **fixed**

The engine never imports the hub. The hub implements this port in `runtime/policy.rs` over the prompt 25 registry and target resolver; the testkit provides `FakeShadowEnvironment`. Every method is read-only, never blocks on a host for longer than the registry's own deadlines, and never errors: an unknown answer is a value.

```rust
#[async_trait]
pub trait ShadowEnvironment: Send + Sync {
    /// The contract as the hub sees it on `host` now: a compiled contract through `ContractIndex::meta`, or a `skill.<name>`
    /// contract built from the host's announced skills with the registry's effective class. `None` when it cannot be established.
    async fn contract(&self, host: HostId, id: &ContractId) -> Option<ContractMeta>;
    /// `TargetResolver::resolve(TargetQuery::Host { host })`. Host-level contracts (every skill) use the host's own name and identity.
    async fn host_target(&self, host: HostId) -> Result<HostTarget, GapReason>;
    async fn ceiling(&self, host: HostId) -> CeilingReading;
    async fn runs_as_root(&self, host: HostId) -> Option<bool>;               // None: no live hello
    async fn facts(&self, host: HostId, wanted: &BTreeSet<FactId>) -> BTreeMap<FactId, FactState>;   // empty until fact producers ship
    async fn containment(&self, host: HostId) -> ContainmentReading;
    async fn autonomy(&self) -> AutonomyReading;
    /// Compiled contracts plus one representative contract per skill name announced anywhere (strictest effective class wins).
    async fn publication_index(&self) -> ContractIndex;
    /// The parsed ceiling of every host that reported a valid one; absent and malformed summaries are omitted.
    async fn ceilings(&self) -> BTreeMap<HostId, CeilingSummary>;
}
pub struct HostTarget { pub name: TargetName, pub identity: TargetIdentity }
pub enum CeilingReading { Reported(CeilingSummary), Absent, HostNotConnected }
pub enum ContainmentReading { NotApplicable, Unavailable }               // Verified { .. } arrives with the helper (prompt 47)
pub enum AutonomyReading { NotModelled }                                  // a real state arrives with prompt 45
```

### 6.6 Building the inputs (`policy_shadow/inputs.rs`) **fixed**

```rust
pub struct Built { pub request: Option<EvaluateRequest>, pub inputs: Option<RecordedInputs>,
                   pub gaps: Vec<InputGap>, pub verdict: Verdict }
pub enum Verdict { Evaluate, Skip(&'static str), Unevaluable(&'static str) }     // the &str is the stored `reason` code
pub async fn build(candidate: &ShadowCandidate, rules: &[Rule], env: &dyn ShadowEnvironment, now: Timestamp) -> Built;
```
The decision table `build` implements, in order (first match wins for the verdict; gaps accumulate only on `Evaluate`):

| Situation | Verdict / value | Gap recorded |
|---|---|---|
| `candidate.contract` starts with `agent.package.` | `Skip("package_contract")` | none |
| `env.contract(host, id)` is `None` | `Unevaluable("contract_unknown")` | none (the verdict is the explanation) |
| `env.host_target(host)` is `Err(r)` | `Unevaluable("target_unresolved")` | none |
| contract `target_kind` is not `Host` | `Unevaluable("target_kind_unsupported")` (only host-level contracts exist as steps today) | none |
| else `Evaluate` with: `target` = host name, `target_identity` = host identity, `args` = `CanonicalArgs` built from `candidate.args` in name order as `name:<byte_len>:<value>\n`, `contract_version` as stored, `requested_rate_per_hour = None`, `requested_ttl = None`, `now` = the argument | | |
| `pinned` = identity of every distinct host in `candidate.pinned`, each resolved with `env.host_target`; an unresolvable host is omitted | | `PinnedTargets/Unresolvable` per omitted host; `PinnedTargets/NotProducedYet` once when any pinned subject names a container or unit target (not resolved: no live read from the sweep) |
| `rule_targets` = `{host name -> host identity}` when `names_to_resolve(rules)` contains the host name, else empty | | none |
| ceiling: `Reported(s)` -> `Some(s)`; `Absent` -> `None`; `HostNotConnected` -> `None` | | `Ceiling/NotProducedYet` for `Absent`; `Ceiling/HostNotConnected` for `HostNotConnected` |
| facts: `env.facts(host, wanted)` where `wanted` = contract `safety_facts` plus every `RequiresFact` fact in `rules`; absent facts stay absent (read as `Unknown`) | | `Facts/NotProducedYet` only when `wanted` is non-empty and some wanted fact is missing from the answer |
| `runs_as_root`: `Some(b)` -> `b`; `None` -> **`true`** (fail closed) | | `RunsAsRoot/HostNotConnected` when `None` |
| containment: contract effect is not `Contain` -> `ContainmentInput::not_applicable()`; `Contain` -> `helper_incomplete = true`, everything else false | | `Containment/NotProducedYet` for `Contain` only |
| autonomy: `AutonomyState { paused: false, circuit_broken: false }` (neither switch exists, so both are true statements) | | `Autonomy/NotProducedYet`, always |
| `expected_ceiling_hash` | `None` at record time | none |

`gaps` is sorted by `(GapInput, GapReason)` declaration order and deduplicated. `FakeShadowEnvironment` defaults (documented on it): compiled contracts plus skills added by the test, host identity `host:<id>/<fixed test fingerprint>` and name from the test, ceiling `Absent`, `runs_as_root` `Some(false)`, no facts, containment `NotApplicable`, autonomy `NotModelled`; every answer is overridable per host. `build` never fails. It never reads `Clock` (the caller passes `now`), never writes and never retries. `degraded` on the row is `!gaps.is_empty()`. A `Contain` contract cannot be a step today (skill contracts are never `Contain`); the branch exists so a future helper contract fails closed rather than reading as attested.

### 6.7 The recorder (`policy_shadow/recorder.rs`) **fixed**

```rust
pub struct ShadowRecorder { /* repo: Arc<dyn PolicyRepo>, uow: Arc<dyn UnitOfWork>, env: Arc<dyn ShadowEnvironment>, clock: Arc<dyn Clock> */ }
pub struct SweepReport { pub evaluated: u32, pub unevaluable: u32, pub skipped: u32, pub batches: u32 }
pub const SWEEP_EVERY: SignedDuration = SignedDuration::from_secs(5);
pub const BATCH: u16 = 50;  pub const MAX_BATCHES_PER_SWEEP: u32 = 10;
pub const LATE_AFTER: SignedDuration = SignedDuration::from_secs(300);
impl ShadowRecorder {
    pub fn new(repo: Arc<dyn PolicyRepo>, uow: Arc<dyn UnitOfWork>, env: Arc<dyn ShadowEnvironment>, clock: Arc<dyn Clock>) -> Self;
    /// One pass. Tests call this directly; they never sleep.
    pub async fn sweep_once(&self) -> Result<SweepReport, ShadowError>;
    /// Loops `sweep_once` every `SWEEP_EVERY` on the `Clock` port until `cancel` fires; logs and survives a failed sweep.
    pub async fn run(self, cancel: CancellationToken);
}
pub enum ShadowError { Store(StoreError), Commit(CommitError), Rules(CodecError) }     // thiserror
```
`sweep_once`, in order:

1. `now = clock.now()`. Load the active version (`repo.active()`), then its rules (`repo.rules`), then `decode_rules`. No active version: `rules = []`, `policy_version = 0`, `rules_hash = rules_hash(&[])`. A `CodecError` fails the sweep with `ShadowError::Rules`, writes `last_error` (through a `RecordShadow` with no records), and records nothing: a row is never written against rules the recorder could not read.
2. Up to `MAX_BATCHES_PER_SWEEP` times: `candidates = repo.shadow_candidates(BATCH)`; empty -> stop. For each candidate, `build(..)`, then:
   - `Skip(code)` -> a `ShadowRecord` with `status = Skipped`, `reason = Some(code)`, `inputs/enforced/shadow = None`, outcomes `None`.
   - `Unevaluable(code)` -> `status = Unevaluable`, same shape.
   - `Evaluate` -> `evaluate(&rules, &request)` and `evaluate_shadow(&rules, &request)`, each mapped to a `DecisionEnvelope` (`Err(e)` -> `Error { code: error_code(&e) }`, outcome text `"error"`). `status = Evaluated`.
   - Every row sets `policy_version`, `rules_hash`, `evaluated_at = now`, `degraded`, `gaps` and the candidate document. If `now - candidate.created_at > LATE_AFTER`, the row's `reason` is `Some("late")` (inputs reflect evaluation time, not step creation); `status` is still `Evaluated`.
   - A JSON column larger than `MAX_ROW_JSON_BYTES` turns the row into `Unevaluable("too_large")` with `inputs/enforced/shadow = None` (the candidate document is bounded by skill argument validation; truncate nothing).
3. Commit one `Transaction` per batch: `StateWrite::Policy(PolicyWrite::RecordShadow { records, sweep, at: now })`, no journal, no audit, no effects. The applier uses `INSERT ... ON CONFLICT(execution_id) DO NOTHING`, so a row written by a concurrent or earlier sweep is kept and a crash between batches loses nothing (decision P1).
4. Return the report. A store or commit error aborts the sweep after logging (`tracing::warn!`, no secret, no rule text) and sets `last_error` on the next successful commit. The next tick retries from the same query: there is no cursor to repair.

The recorder reads `ShadowEnvironment` and the repo only. It holds no `OperationHandle`, `ActionHandle`, link, approval, permit or store connection of its own, and it never calls `UnitOfWork` with anything but `RecordShadow`.

### 6.8 Replay and analysis (`policy_shadow/replay.rs`, `report.rs`) **fixed**

```rust
/// Rebuilds the exact request `evaluate` saw. `expected_ceiling_hash` = the recorded summary's hash (39 section 6.5). Err on a malformed document.
pub fn request_from_inputs(inputs: &RecordedInputs, version: Option<Uuid>) -> Result<EvaluateRequest, ReplayError>;
pub fn decode_inputs(text: &str) -> Result<RecordedInputs, ReplayError>;          // v != 1 -> Err
pub fn decode_decision(text: &str) -> Result<DecisionEnvelope, ReplayError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "snake_case")]
pub enum Comparison { Same, Changed, NotReplayable }       // NotReplayable: status != Evaluated or a column failed to decode
pub struct Replayed { pub simulated: DecisionEnvelope, pub baseline: Option<DecisionEnvelope>, pub comparison: Comparison }
/// Evaluates `rules` for one recorded row under `selection` and compares it with the row's own recorded decision for that selection.
pub fn replay(rules: &[Rule], record: &ShadowRecord, selection: RuleSelection) -> Replayed;

pub enum LockChangeKind { Created, Edited, Unlocked, Removed }       // serde snake_case
pub struct LockChange { pub rule: RuleId, pub change: LockChangeKind }
/// Locked-Deny differences between two rule sets, sorted by rule id. A rule is "locked" when `locked` is set. Created: locked in `next`,
/// absent in `base`. Removed: locked in `base`, absent in `next`. Unlocked: locked in `base`, present but not locked in `next`.
/// Edited: locked in both and any field (including `enabled`, `note`, scope, conditions) differs.
pub fn locked_changes(base: &[Rule], next: &[Rule]) -> Vec<LockChange>;
/// `"<kind>:<first>:<second>"`, kind in snake_case (`equal_scope_conflict`, `incomparable_conflict`, `ineffective_under_lock`, `target_outside_every_ceiling`).
pub fn finding_key(finding: &Finding) -> String;
/// Lowercase hex SHA-256 of the JSON `{"v":1,"base":<active number or 0>,"rules":"<rules_hash>","blocking":[sorted keys],"locked":[[rule,change]..sorted]}`.
pub fn preview_hash(base_number: u32, rules_hash: &str, blocking: &[String], locked: &[LockChange]) -> String;

pub struct SimulationSummary { pub evaluated: u32, pub same: u32, pub changed: u32, pub not_replayable: u32,
                               pub newly_allowed: u32, pub newly_denied: u32, pub newly_requires_approval: u32 }
pub fn summarise(items: &[Replayed]) -> SimulationSummary;     // a change to Allow is newly_allowed, to Deny newly_denied, to RequireApproval newly_requires_approval; Unavailable counts only in `changed`
```
`replay` is deterministic: the same rules and the same row give an equal result. For a row whose rules are the version it was recorded under and whose selection is `Enforced`, `comparison == Same` always (acceptance 5).

### 6.9 `PolicyService` (`crates/domovoi-hub/src/services/policy.rs`, `policy_publish.rs`) **fixed** names, **convention** layout

It follows prompt 29's rules: `Clone + Send + Sync`, spawns nothing, never imports `crate::api`, `crate::auth::interactions`, `crate::ingest`, `crate::runtime` or `crate::router`, returns domain types and its own error enum (never `ApiError`, never a DTO), never builds a `Caller`, `HumanApproval` or `Authorization`. It may import `crate::auth::AuthState` for step-up (the settings service already does, `settings/service.rs:235`).

```rust
pub struct PolicyDeps { pub repo: Arc<dyn PolicyRepo>, pub uow: Arc<dyn UnitOfWork>, pub env: Arc<dyn ShadowEnvironment>,
                        pub auth: Arc<AuthState>, pub clock: Arc<dyn Clock>, pub shadow_enabled: bool }
// A new draft's id is `PolicyVersionId::new()` (UUIDv7, the `define_id!` default), like every other id in the hub.
/// The step-up proof a transport collects: the client address (after trusted-proxy handling) and the password from the request body.
pub struct StepUp<'a> { pub ip: IpAddr, pub password: Option<&'a str> }
impl PolicyService {
    pub fn new(deps: PolicyDeps) -> Self;
    /// Replaces the whole draft. `base_revision: None` creates it (fails if one exists); `Some(n)` replaces draft revision n.
    pub async fn save_draft(&self, caller: &Caller, rules: Vec<Rule>, base_revision: Option<u64>,
                            preview_hash: Option<&str>, step_up: StepUp<'_>) -> Result<SavedDraft, PolicyCommandError>;
    pub async fn discard_draft(&self, caller: &Caller, revision: u64) -> Result<(), PolicyCommandError>;
    /// Publishes draft `revision` as the next version number.
    pub async fn publish(&self, caller: &Caller, revision: u64, preview_hash: &str, acknowledged: &[String],
                         step_up: StepUp<'_>) -> Result<PublishedVersion, PolicyCommandError>;
    /// Read-only analysis of a candidate rule set against its base. No password, no write.
    pub async fn preview(&self, rules: Vec<Rule>, base_revision: Option<u64>) -> Result<Preview, PolicyCommandError>;
    /// Replays or re-evaluates historical Execution steps. No password, no write.
    pub async fn simulate(&self, request: SimulationRequest) -> Result<SimulationResult, PolicyCommandError>;
    pub fn shadow_enabled(&self) -> bool;
}
pub struct SavedDraft { pub record: PolicyVersionRecord, pub locked_changes: Vec<LockChange> }
pub struct PublishedVersion { pub record: PolicyVersionRecord, pub locked_changes: Vec<LockChange>, pub acknowledged: Vec<String> }
pub struct Preview { pub base_number: u32, pub rules_hash: String, pub validation: Vec<RuleIssue>, pub report: Option<PublicationReport>,
                     pub locked_changes: Vec<LockChange>, pub simulation: Option<SimulationSummary>, pub preview_hash: String,
                     pub blocking: Vec<String>, pub notes: Vec<&'static str> }
pub struct RuleIssue { pub rule: Option<String>, pub message: String }          // from PolicyError's Display; no owner text beyond ids
pub enum RulesSource { Draft, Version(u32), Inline(Vec<Rule>) }
pub enum InputsMode { Recorded, Current }
pub struct SimulationRequest { pub rules: RulesSource, pub selection: SelectionChoice /* Enforced | Shadow | Both */, pub inputs: InputsMode,
                               pub filter: ShadowFilter, pub execution_ids: Vec<ExecutionId>, pub limit: u16 }
pub struct SimulationResult { pub label: String, pub items: Vec<SimulationItem>, pub summary: SimulationSummary, pub truncated: bool, pub notes: Vec<&'static str> }
pub struct SimulationItem { pub record: ShadowRecord, pub enforced: Option<Replayed>, pub shadow: Option<Replayed> }
pub enum PolicyCommandError {
    PasswordRequired, BadPassword, TooMany { retry_after_secs: i64 },
    Invalid(Vec<RuleIssue>), TooManyRules { max: usize }, NoteTooLong { rule: String }, PackageContract { rule: String },
    NoDraft, DraftExists, StaleDraft { current: u64 }, StaleActive { current: u32 },
    StalePreview, Unacknowledged { keys: Vec<String> }, UnknownAcknowledgement { keys: Vec<String> },
    TooLarge, NotFound, Store(StoreError), Commit(CommitError),
}
```
Constants in `services/policy.rs`: `PASSWORD_REQUIRED_TEXT = "enter your password to change the policy"`, `BAD_PASSWORD_TEXT = "wrong password"`.

**Command order (every command; the first failure returns and nothing is written):**

`save_draft`: (1) structural checks on the input: `rules.len() <= MAX_RULES` (`TooManyRules`), each `note.chars().count() <= MAX_NOTE_CHARS` (`NoteTooLong`), no `CapabilityScope::Contract(id)` with `id` starting `agent.package.` (`PackageContract`), then `validate_rules` (`Invalid`); encoded size over 512 KiB is `TooLarge`. (2) Load the base: the current draft when `base_revision` is `Some` (its revision must equal it, else `StaleDraft{current}`; `None` while a draft exists is `DraftExists`; `Some` with no draft is `NoDraft`), otherwise the active version. (3) `changes = locked_changes(base_rules, &rules)` where `base_rules` are the draft's rules when editing a draft, else the active version's (empty when none). (4) If `changes` is non-empty: the step-up is required (below) **and** `preview_hash` must be `Some` and equal the hash `preview` computes for these rules and this base (`StalePreview` otherwise). (5) Build one `Transaction`: `StateWrite::Policy(PolicyWrite::SaveDraft{..})`, audit `policy.draft_saved` (detail `{revision, rules, locked, shadow}`) and, per `LockChange`, audit `policy.lock_changed` (detail `{rule, change, scope: "draft"}`), actor `caller.actor()`. No journal row for a draft. (6) Commit; map `CommitError::Conflict` to `StaleDraft`.

`discard_draft`: no step-up (a discard changes nothing live). Writes `PolicyWrite::DiscardDraft` and audit `policy.draft_discarded` (detail `{revision}`); `NoDraft` or `StaleDraft` when it does not match.

`publish`: (1) **Step-up first, always** (`step_up(...)` below), before any state is read. (2) Load the draft: `NoDraft`; its revision must equal `revision` (`StaleDraft`). (3) `rules = decode_rules(draft rules)`; the active version must still be the one the draft's `based_on` names, else `StaleActive{current}` (another publish happened: the owner must re-base by saving the draft again). (4) `index = env.publication_index()`, `ceilings = env.ceilings()`, `report = validate_publication(&rules, &index, &ceilings)` (`Err(e)` -> `Invalid`). (5) `blocking = report.blocking().map(finding_key)` sorted; `changes = locked_changes(active_rules, &rules)`; `expected = preview_hash(active_number, draft.rules_hash, &blocking, &changes)`; `preview_hash` must equal it (`StalePreview`). Decision P5 requires the echo when the locked set differs; this prompt requires it for every publish (a superset), so the acknowledgements always bind to the exact rules. (6) `acknowledged` (sorted, deduplicated) must equal `blocking` exactly: a missing key is `Unacknowledged{keys}`, a key that is not blocking is `UnknownAcknowledgement{keys}`. (7) One `Transaction`: `PolicyWrite::Publish { number: active_number + 1, acknowledgements: one `AckRecord` per finding (`resolution` as `allow|require_approval|deny`), .. }`; journal `JournalKind::PolicyPublished` on `AggregateRef::policy(id)` (6.12); audit `policy.published`; per `LockChange` audit `policy.lock_changed` (detail `{rule, change, scope: "published", version: number}`). (8) Commit; `CommitError::Conflict` -> `StaleActive`. Publishing is the only way a rule becomes part of the active version.

`preview`: base = the draft when `base_revision` matches it, else the active version. Runs `validate_rules` (an `Err` becomes `validation` and `report: None`), then, if the rules pass, `validate_publication` against `env.publication_index()` and `env.ceilings()` (`Err` becomes a `validation` entry), `locked_changes`, a **bounded simulation** (`SIM_PREVIEW_LIMIT = 200` most recent evaluated rows, `Recorded` inputs, `Enforced` selection, summary only) and `preview_hash`. `notes` always carries the standing notes of 6.11. An empty history gives `simulation: Some` with zeros.

`simulate`: resolves `RulesSource` (`Draft` -> the stored draft, `NoDraft` when absent; `Version(n)` -> `NotFound`; `Inline` -> run `validate_rules` and refuse `PackageContract` exactly like `save_draft`), loads rows (`execution_ids` when given, at most 100, else `repo.shadow_records(filter, Page::first(limit))` with `limit` clamped to 1..=500, default 200; `truncated` when a next cursor exists), then per row and selection: `Recorded` -> `replay(rules, &row, selection)`; `Current` -> rebuild the request from the row's stored candidate with `build(..)` and `now = clock.now()`, evaluate, and compare with the recorded decision for that selection (`NotReplayable` when the row is not `Evaluated`). `SelectionChoice::Both` fills both `enforced` and `shadow`. A row whose stored columns fail to decode counts in `not_replayable`; it never aborts the request. The service never constructs an approval, permit or authorization, and never writes.

**Step-up** (`fn step_up(&self, caller, step_up) -> Result<(), PolicyCommandError>`, one private function, used by `publish` and by `save_draft` when `changes` is non-empty): `password = None` -> `PasswordRequired`; `AuthState::confirm_password(step_up.ip, caller.user(), password)` -> `Err(AuthError::LockedOut { retry_after_secs })` is `TooMany`, any other error is `BadPassword`. It shares the login limiter, so it cannot be used to guess faster.

### 6.10 HTTP API and the DTOs prompt 43 consumes **fixed** paths, methods and JSON; **convention** Rust names

All routes need a session (`Authed`) and are tagged `policy` in `ApiDoc`. Handlers parse, call `state.services().policy` (commands, preview, simulate) or `state.deps().policy` (plain reads) and map errors; they hold no logic. Every route gets a `#[utoipa::path]` with the full status list. The client address is `proxy::client_ip(peer, &headers, &state.auth.web().trusted_proxies)` exactly as `api/settings_write.rs:156-191`.

| Method and path | Request | Response | Notes |
|---|---|---|---|
| `GET /api/v1/policy` | none | `PolicyOverviewDto` | |
| `GET /api/v1/policy/versions?cursor=&limit=` | | `VersionPageDto { items: VersionSummaryDto[], next_cursor }` | published only, newest first; `limit` 1..=100 default 25 |
| `GET /api/v1/policy/versions/{number}` | | `VersionDetailDto` | 404 for an unknown number |
| `GET /api/v1/policy/draft` | | `DraftDetailDto` | 404 when no draft |
| `PUT /api/v1/policy/draft` | `SaveDraftRequest` | `DraftSavedDto` | step-up only when a locked rule changes |
| `DELETE /api/v1/policy/draft?revision=` | | 204 | no body, no password |
| `POST /api/v1/policy/preview` | `PreviewRequest` | `PreviewDto` | read-only |
| `POST /api/v1/policy/draft/publish` | `PublishRequest` | `PublishedDto` | always step-up |
| `POST /api/v1/policy/simulate` | `SimulateRequest` | `SimulationDto` | read-only |
| `GET /api/v1/policy/shadow?host=&contract=&status=&outcome=&differing=&since=&until=&cursor=&limit=` | | `ShadowPageDto { items: ShadowSummaryRowDto[], next_cursor }` | `limit` 1..=100 default 25 |
| `GET /api/v1/policy/shadow/{execution_id}` | | `ShadowDetailDto` | 404 when no row yet |

Status mapping (reusing `ApiError` variants and `ErrorBody { error, message, issues }`; **no new variant, no new code**): `PasswordRequired`, `BadPassword` -> `Forbidden(<constant text>)` (403 `forbidden`); `TooMany` -> `TooMany(secs)` (429); `Invalid`, `TooManyRules`, `NoteTooLong`, `PackageContract`, `TooLarge`, `UnknownAcknowledgement` -> 400 (`Invalid(issues)` with `path = "rules.<rule id>"` or `""`, `severity = "error"` for rule problems; `BadRequest` otherwise); `NoDraft`, `NotFound` -> 404 (`NoDraft` on a command that needs one is 404); `DraftExists`, `StaleDraft`, `StaleActive`, `StalePreview`, `Unacknowledged` -> 409 `Conflict(message)` where an `Unacknowledged` message lists the keys; `Store`, `Commit` (non-conflict) -> `ApiError::internal`.

Wire shapes (DTOs derive `Serialize, Deserialize, ToSchema`; ids and timestamps are strings, timestamps RFC 3339 UTC with a trailing `Z`; snake_case; **fixed**):

```json
// RuleDto — the editor's rule shape, independent of the stored envelope and of the evaluator's serde attributes
{ "id": "plex-restart", "enabled": true, "shadow": false, "locked": false,
  "host": { "kind": "all" },                                         // or { "kind": "host", "host_id": "<uuid>" }
  "capability": { "kind": "effect", "effect": "mutate" },            // any | effect{effect} | category{category} | contract{contract}
  "targets": { "kind": "exact", "names": ["nas"] },                  // or { "kind": "any" }
  "conditions": [
    { "kind": "arg_constraint", "arg": "count", "op": "int_at_least", "value": 3 },
    { "kind": "time_window", "start": "22:00", "end": "06:00" },
    { "kind": "requires_fact", "fact": "host.not_in_maintenance", "expected": true },
    { "kind": "max_rate", "max_per_hour": 3 },
    { "kind": "max_ttl", "max_ttl_secs": 600 } ],
  "outcome": "allow",                                                // allow | require_approval | deny
  "note": "Plex may restart itself" }
```
`RuleDto` converts to `Rule` with `TryFrom` (RuleId, ContractId, TargetName, FactId parse; `HH:MM` for `TimeOfDay`; any parse failure is a 400 `Invalid` issue at `rules.<id>`), and from `Rule` infallibly. `outcome: "unavailable"` is rejected as invalid input.

```json
// GET /api/v1/policy -> PolicyOverviewDto
{ "shadow_enabled": true,
  "time_window_zone": "UTC",                                         // the zone 39 implemented for TimeWindow; the UI must label it
  "active": { "number": 3, "published_at": "..", "published_by": "user:<uuid>", "rule_count": 12, "locked_count": 1, "shadow_count": 2, "rules_hash": ".." },
  "draft":  { "revision": 4, "based_on": 3, "updated_at": "..", "updated_by": "user:<uuid>", "rule_count": 13, "locked_count": 1, "shadow_count": 2 },
  "shadow": { "last_sweep_at": "..", "last_error": null, "total_evaluated": 120, "total_unevaluable": 3, "total_skipped": 0,
              "last_24h": { "total": 40, "by_status": {"evaluated": 38, "unevaluable": 2}, "by_enforced_outcome": {"require_approval": 30, "allow": 8},
                            "degraded": 40, "differing": 5 } },
  "notes": [ "No agent reports an unattended ceiling yet, so every recorded decision treats the ceiling as unavailable.",
             "Package installs are standalone jobs and are never evaluated by policy.",
             "Nothing here is enforced: decisions are recorded for comparison only." ] }
```
`VersionSummaryDto` = the `active` shape above. `VersionDetailDto { summary, rules: RuleDto[], acknowledgements: AckDto[] }`; `DraftDetailDto { revision, based_on, updated_at, updated_by, rules_hash, rules: RuleDto[] }`; `AckDto { finding_key, kind, first_rule, second_rule, resolution, note, acknowledged_by, acknowledged_at }`.

```json
// PUT /api/v1/policy/draft
{ "base_revision": null, "rules": [ RuleDto ], "password": null, "preview_hash": null }          // SaveDraftRequest
// -> DraftSavedDto { "draft": DraftDetailDto, "locked_changes": [ { "rule": "no-prune", "change": "edited" } ] }

// POST /api/v1/policy/preview     PreviewRequest { "base_revision": 4, "rules": [ RuleDto ] }
{ "base_number": 3, "rules_hash": "..", "preview_hash": "..",
  "validation": [ { "rule": "plex-restart", "message": "..." } ],        // non-empty: report is null
  "report": { "findings": [ FindingDto ], "blocked": true },
  "blocking": [ "incomparable_conflict:rule-a:rule-b" ],                 // the keys the owner must acknowledge
  "locked_changes": [ { "rule": "no-prune", "change": "created" } ],
  "simulation": { "evaluated": 200, "same": 190, "changed": 6, "not_replayable": 4, "newly_allowed": 0, "newly_denied": 5, "newly_requires_approval": 1 },
  "notes": [ "..." ] }
// FindingDto { "key": "incomparable_conflict:rule-a:rule-b", "kind": "incomparable_conflict", "first": "rule-a", "second": "rule-b",
//              "resolution": "deny", "shadow": false, "note": "..", "requires_acknowledgement": true }

// POST /api/v1/policy/draft/publish
{ "revision": 4, "preview_hash": "..", "acknowledged": [ "incomparable_conflict:rule-a:rule-b" ], "password": "..." }
// -> PublishedDto { "version": VersionSummaryDto, "locked_changes": [..], "acknowledged": [..] }

// POST /api/v1/policy/simulate
{ "rules": { "source": "draft" },                 // or { "source": "version", "number": 3 } or { "source": "inline", "rules": [ RuleDto ] }
  "selection": "both",                            // enforced | shadow | both (default both)
  "inputs": "recorded",                           // recorded (default) | current
  "filter": { "host": null, "contract": null, "status": "evaluated", "since": null, "until": null },
  "execution_ids": [], "limit": 200 }
// -> SimulationDto
{ "label": "draft revision 4", "summary": SimulationSummary, "truncated": false, "notes": [ ".." ],
  "items": [ { "execution_id": "<uuid>", "operation_id": "<uuid>", "host_id": "<uuid>", "contract": "skill.restart_plex",
               "evaluated_at": "..", "recorded_policy_version": 3, "degraded": true, "gaps": [ { "input": "ceiling", "reason": "not_produced_yet" } ],
               "enforced": { "comparison": "changed", "baseline": DecisionDto, "simulated": DecisionDto },
               "shadow": null } ] }
```
`DecisionDto` is `DecisionDoc` with the same field names (so 43 renders one type for recorded and simulated decisions); `{ "error": "<code>" }` replaces `decision` for an evaluation error. `ShadowSummaryRowDto { execution_id, operation_id, host_id, contract, evaluated_at, status, reason, policy_version, enforced_outcome, shadow_outcome, degraded }`; `ShadowDetailDto` adds `gaps`, `candidate`, `inputs` (the `RecordedInputs`), `enforced` and `shadow` (`DecisionDto`). Inputs shown by the API contain no secret by construction.

### 6.11 Configuration, journal payload, audit kinds, step-up registry **fixed**

```toml
[policy]
# Record, for every Execution step, what the active policy and its shadow rules would decide. Nothing is enforced.
shadow = false      # default false
```
`crates/domovoi-config/src/hub/policy.rs`: `pub struct HubPolicySection { pub shadow: bool }` (`Deserialize, JsonSchema`, `deny_unknown_fields`, `Default` = `false`, doc comments feed `docs-config`); `HubConfig.policy: HubPolicySection` with `#[serde(default)]`. A validation arm is not needed. **The `[policy]` table name is shared with `agent.toml`** (`[policy] allow_risk`). Today the hub skips every `DOMOVOI_POLICY__*` environment variable as a foreign (agent) section, so `DOMOVOI_POLICY__SHADOW` would be silently ignored. Simply dropping `"policy"` from the foreign list is wrong too: in single-box mode one environment feeds both roles, and the hub would then reject `DOMOVOI_POLICY__ALLOW_RISK` as an unknown key. Resolve it key by key: `env::apply_overrides_tracked` skips a variable when `foreign.contains(first segment)` **or** `foreign.contains(full dotted path)`; replace `"policy"` in `AGENT_SECTIONS` (same array length) with `"policy.allow_risk"`, add `"policy.shadow"` to `HUB_SECTIONS`, and update the doc comments of both constants ("a top-level table, or one dotted key of a table both files share"). Tests in `domovoi-config`: `hub_reads_policy_shadow_from_the_environment`, `hub_ignores_the_agents_policy_allow_risk_variable`, `agent_ignores_the_hubs_policy_shadow_variable`, `agent_still_reads_policy_allow_risk_from_the_environment`. A change to the setting applies on restart (the settings page already restarts the hub); the recorder starts only when it is `true` at start.

Journal `policy.published`, `payload_v 1`, aggregate `AggregateRef::policy(version id)`, actor = the human: `{"version": <number>, "rules": <n>, "locked": <n>, "shadow": <n>, "rules_hash": "<64 hex>", "acknowledged": ["<finding key>", ..], "locked_changes": [{"rule": "..", "change": ".."}]}`. No rule bodies, no notes. It maps to no SSE topic.

Audit kinds (constants in `crates/domovoi-hub/src/services/policy.rs`; subject `policy:<version id>`): `policy.published` (detail `{number, rules, locked, shadow, acknowledged, preview_hash: <first 8 hex>}`), `policy.lock_changed` (detail `{rule, change, scope, version?}`), `policy.draft_saved`, `policy.draft_discarded`. The audit actor is `caller.actor()`. No password, no hash of a password and no rule note appears in an audit row, a journal row, a log line or an error text. An audit failure fails the commit (same transaction).

`crates/domovoi-hub/src/settings/step_up.rs`: add `pub struct EnforcedStepUp { pub id: &'static str, pub enforced_by: &'static str }` and `pub const STEP_UP_ENFORCED_ACTIONS: &[EnforcedStepUp]` holding `policy.publish` and `policy.lock_changed` (`enforced_by: "PolicyService"`); **remove `policy.publish` from `STEP_UP_ACTIONS`** (keep the other three; if prompt 42 already introduced an enforced list, add to it instead and keep one list). Extend the module doc. The existing `step_up_list_declares_future_actions` keeps passing; add `enforced_step_up_actions_are_not_declared_as_future`.

Standing notes on every overview and preview (constants `STANDING_NOTES` in `services/policy.rs`, exactly these three texts as in the overview example above). The first is shown only while no host reports a ceiling.

## 7. Ordered implementation steps

Each step compiles and carries its own tests. Commit in small Conventional Commits (`feat(core): ...`, `feat(store): ...`, one scope each).

1. **Inspect and verify; write the differences down before editing.** Open every file named in sections 3 and 6. Confirm: prompts 24 (merged), 25, 26, 27, 28, 29 and 39 are merged and every name in "Consumes" resolves (`ls crates/domovoi-core/src/capability`, `crates/domovoi-core/src/operation`, `crates/domovoi-engine/src/policy/`, `crates/domovoi-hub/src/services/`); the `executions` and `operations` tables exist and `ExecutionRecord`, `PinnedSubject`, `ExecutionId::from_step` match 26; whether a step stored `Refused` really has no `executions` row (if it has one, the recorder records it as usual and says so in the report); `TargetResolver`/`TargetQuery::Host`, `HubRegistry`, `parse_ceiling`; `Services`/`ServicesDeps`/`Caller` match 29; the migrations directory (take the next free number); the latest `upgrade.rs` fixture pattern; whether prompt 42 already added an enforced step-up list. **Check `99` D-30 and read how prompt 39 implemented `TimeWindow`**: record which zone it uses and set the overview's `time_window_zone` from that fact, never from this brief. **Run 39's serde round trip first** (section 3.2): serialise and deserialise a `Rule` containing `CapabilityScope::Effect`, `TargetScope::Exact`, every `Condition` and a `FactState::Known(true)` inside a `RequiresFact` request fact; also confirm `ConstraintKind` derives `Deserialize` (24 derives only `Serialize`; 39's `Condition::ArgConstraint` needs both). If either fails, make the **only permitted edits of 24's and 39's files**: adjacent tagging (`#[serde(tag = "kind", content = "value")]`) and/or the missing derive, each with a unit test in that file, and say so in the report. Report any other discrepancy and stop if it changes what you would build.
2. **Config.** `hub/policy.rs`, the `HubConfig.policy` field, the `env.rs` constants and the key-granular skip, the four config tests, `cargo xtask docs-config`.
3. **Core.** `PolicyVersionId`, `policy_store/`, `PolicyRepo`, `AggregateKind::Policy`, `StateWrite::Policy`, `JournalKind::PolicyPublished`, re-exports; extend the round-trip tests. Compile every crate that matches on `StateWrite` or `AggregateKind` (`cargo check --workspace`): the store's `apply` match and any exhaustive match elsewhere gain an arm (a `Policy` arm that is unreachable in a crate is still an explicit arm, never `_ =>`).
4. **Store.** Migration, `policy/{mod,apply,rows,queries}.rs`, `repos/policy.rs`, the `writes.rs` arm. Applier rules in section 8. Run the shared contract suite on SQLite.
5. **Testkit.** In-memory `PolicyRepo` and applier (including `shadow_candidates` over the in-memory executions), `contracts/policy.rs`, `FakeShadowEnvironment`; register the suite for `InMemoryStore` in `crates/domovoi-testkit/tests/contracts.rs`.
6. **Engine, formats.** `policy_shadow/{codec,doc,env}.rs`; golden snapshots; `codec` and `doc` unit tests.
7. **Engine, inputs and recorder.** `inputs.rs` (the table of 6.6, one test per row) then `recorder.rs`. Add `pub mod policy_shadow;` to `lib.rs`.
8. **Engine, replay and report.** `replay.rs`, `report.rs`.
9. **Hub service.** `services/policy.rs`, `services/policy_publish.rs`, `Services.policy`, `ServicesDeps.policy`, the `step_up.rs` registry change.
10. **Hub environment and wiring.** `runtime/policy.rs`: `HubShadowEnvironment` over the registry and resolver; `PolicyService` always built; `ShadowRecorder` spawned only when `config.policy.shadow`, stopped through the same `CancellationToken` and join pattern the other runtime tasks use. Update the three `ApiDeps` constructors (`runtime/mod.rs`, `examples/dev_hub.rs`, `tests/hub/api_harness.rs`).
11. **API.** DTOs, handlers, routes, `ApiDoc` tag. Then `cargo xtask openapi && cd web && npm run gen:api` (generated files only).
12. **Gates and arch.** Section 12.4. `cargo xtask arch-check` (new engine, core, store and hub modules; no new zone).
13. **Docs.** Section 15.
14. **Checks and completion report.** Run the scoped checks for every crate touched (`domovoi-config`, `domovoi-core`, `domovoi-store`, `domovoi-engine`, `domovoi-hub`, `domovoi-testkit`), `cargo nextest run -E 'rdeps(domovoi-core)'` (a public enum gained variants), `cargo xtask arch-check`, `cargo xtask docs-config`, `cargo xtask docs-links`, and the web `typecheck` after `gen:api`. Write the completion report; name the D-30 zone and every interpretation you took.

## 8. Database and persistence

**Migration** `crates/domovoi-store/migrations/NNNN_policy_versions.sql`, `NNNN` the next free number on your branch (list the directory; never copy a number from this brief; describe it by contents). Additive only: new tables, nothing existing altered, `schema_compat.min_reader` unchanged. Enums are validated in Rust; the only `CHECK`s are formats and booleans.

```sql
CREATE TABLE policy_versions (
    id             TEXT PRIMARY KEY,
    status         TEXT NOT NULL,                       -- 'draft' | 'published'; validated in Rust
    number         INTEGER,                             -- set exactly when published
    draft_revision INTEGER NOT NULL DEFAULT 1,
    based_on       INTEGER,
    rules_hash     TEXT NOT NULL CHECK (length(rules_hash) = 64 AND rules_hash NOT GLOB '*[^0-9a-f]*'),
    rule_count     INTEGER NOT NULL,
    locked_count   INTEGER NOT NULL,
    shadow_count   INTEGER NOT NULL,
    created_at     INTEGER NOT NULL,
    created_by     TEXT NOT NULL,                       -- Actor string form
    updated_at     INTEGER NOT NULL,
    updated_by     TEXT NOT NULL,
    published_at   INTEGER,
    published_by   TEXT,
    CHECK ((status = 'published') = (number IS NOT NULL)),
    CHECK ((status = 'published') = (published_at IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX policy_versions_number    ON policy_versions (number) WHERE number IS NOT NULL;
CREATE UNIQUE INDEX policy_versions_one_draft ON policy_versions (status) WHERE status = 'draft';

CREATE TABLE policy_rules (
    version_id TEXT NOT NULL REFERENCES policy_versions(id),
    rule_id    TEXT NOT NULL,
    ordinal    INTEGER NOT NULL,
    enabled    INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    shadow     INTEGER NOT NULL CHECK (shadow  IN (0, 1)),
    locked     INTEGER NOT NULL CHECK (locked  IN (0, 1)),
    body       TEXT NOT NULL,                           -- the envelope of 6.3
    PRIMARY KEY (version_id, rule_id),
    UNIQUE (version_id, ordinal)
) STRICT;
CREATE TRIGGER policy_rules_frozen_when_published
BEFORE UPDATE ON policy_rules
WHEN (SELECT status FROM policy_versions WHERE id = OLD.version_id) = 'published'
BEGIN SELECT RAISE(ABORT, 'a published policy version is immutable'); END;
CREATE TRIGGER policy_rules_no_delete_when_published
BEFORE DELETE ON policy_rules
WHEN (SELECT status FROM policy_versions WHERE id = OLD.version_id) = 'published'
BEGIN SELECT RAISE(ABORT, 'a published policy version is immutable'); END;

CREATE TABLE policy_acknowledgements (
    version_id      TEXT NOT NULL REFERENCES policy_versions(id),
    finding_key     TEXT NOT NULL,
    kind            TEXT NOT NULL,
    first_rule      TEXT NOT NULL,
    second_rule     TEXT NOT NULL,
    resolution      TEXT,
    note            TEXT NOT NULL,
    acknowledged_by TEXT NOT NULL,
    acknowledged_at INTEGER NOT NULL,
    PRIMARY KEY (version_id, finding_key)
) STRICT;

CREATE TABLE policy_shadow_decisions (
    execution_id    TEXT PRIMARY KEY,                   -- = the Execution step id; no foreign key: rows outlive pruned executions
    operation_id    TEXT NOT NULL,
    host_id         TEXT NOT NULL,
    contract        TEXT NOT NULL,
    step_created_at INTEGER NOT NULL,
    evaluated_at    INTEGER NOT NULL,
    status          TEXT NOT NULL,                      -- evaluated | unevaluable | skipped; validated in Rust
    reason          TEXT,
    policy_version  INTEGER NOT NULL,                   -- 0 = no published version
    rules_hash      TEXT NOT NULL,
    enforced_outcome TEXT,
    shadow_outcome  TEXT,
    degraded        INTEGER NOT NULL CHECK (degraded IN (0, 1)),
    candidate       TEXT NOT NULL,
    inputs          TEXT,
    enforced        TEXT,
    shadow          TEXT,
    gaps            TEXT NOT NULL
) STRICT;
CREATE INDEX policy_shadow_evaluated ON policy_shadow_decisions (evaluated_at DESC, execution_id DESC);
CREATE INDEX policy_shadow_host      ON policy_shadow_decisions (host_id, contract, evaluated_at);
CREATE INDEX policy_shadow_version   ON policy_shadow_decisions (policy_version, evaluated_at);

CREATE TABLE policy_shadow_state (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    last_sweep_at    INTEGER,
    last_error       TEXT,
    total_evaluated  INTEGER NOT NULL DEFAULT 0,
    total_unevaluable INTEGER NOT NULL DEFAULT 0,
    total_skipped    INTEGER NOT NULL DEFAULT 0
) STRICT;
INSERT INTO policy_shadow_state (id) VALUES (1);
```

**Applier rules** (`crates/domovoi-store/src/policy/apply.rs`, typed `CommitError`, never a panic; verify `CommitError`'s variants and use `Conflict`/`Constraint` as the other appliers do):
- `SaveDraft{Absent}`: insert the version row (`status = 'draft'`, `draft_revision = 1`, counts from `rules`) and the rules; a draft already exists -> `Conflict`. `SaveDraft{Revision(n)}`: the stored draft must have `id` and `draft_revision = n`, else `Conflict`; delete its rules, insert the new ones, set `draft_revision = n + 1`, `rules_hash`, counts, `updated_*`. The applier does not re-check that a rule's projections (`enabled`, `shadow`, `locked`) agree with its body: only the service builds `StoredRule` (through `stored_rules`), and `decode_rules` re-checks the agreement on every read.
- `DiscardDraft`: guarded by revision; delete rules, then the version row. A published row is never matched (`status = 'draft'` in the `WHERE`).
- `Publish`: the draft must exist at `expected_revision`; `number` must equal `COALESCE(MAX(number), 0) + 1` (a concurrent publish makes this fail with `Conflict`); update the row (`status = 'published'`, `number`, `published_at`, `published_by`, `updated_*`), insert the acknowledgements. Rules are not touched, and the triggers above make them immutable from that moment.
- `RecordShadow`: for each record `INSERT ... ON CONFLICT(execution_id) DO NOTHING`; then one `UPDATE policy_shadow_state` adding the counts **of rows actually inserted**, setting `last_sweep_at = at` and `last_error = NULL`. When `error` is `Some`, the applier instead sets `last_error = error` and leaves `last_sweep_at` unchanged.
- Unknown stored `status` text found on read is `StoreError::Backend` naming the value, not a default.

`shadow_candidates`: `SELECT ... FROM executions e JOIN operations o ON o.id = e.operation_id LEFT JOIN policy_shadow_decisions d ON d.execution_id = e.execution_id WHERE d.execution_id IS NULL ORDER BY e.created_at, e.execution_id LIMIT ?` (no recorded-before cursor: the anti-join is the cursor). Decode `args` and `pinned` JSON; an undecodable row is `StoreError::Backend` for that call (the recorder logs and the sweep stops: a corrupt step must not be silently skipped forever, and `last_error` shows it).

**Transaction boundaries.** One `UnitOfWork::commit` per command: `SaveDraft`, `DiscardDraft` and `Publish` each carry their audit (and the journal row for `Publish`) in the same transaction; one `RecordShadow` per batch with empty journal and audit.

**N-1 upgrade test** `migration_adds_the_policy_tables_and_keeps_existing_rows` (`crates/domovoi-store/tests/sqlite/upgrade.rs`, same pattern as the existing one): open the committed fixture database with the current binary and assert every pre-existing table keeps its row count, the five new tables exist, `policy_shadow_state` has exactly its one seed row and the other four are empty, `PRAGMA foreign_key_check` is empty and `schema_compat.min_reader` is unchanged.

## 9. Protocol and compatibility

- **No wire change.** No `domovoi-proto` change, no agent change, `PROTOCOL_VERSION` stays 1.2 (RW-42). Nothing is sent to an agent because of a policy decision.
- **No behaviour change anywhere else.** With `shadow = false` no task is spawned and no row is written; with it `true`, approvals, dispatch, the ceiling and the loop guard behave exactly as before. A gate (12.4) pins that no enforcement path names a policy decision.
- **HTTP.** New routes only; `cargo xtask openapi` output differs from the previous document by the eleven routes and their schemas. `web/src/api/{openapi.json,schema.d.ts}` are regenerated, never edited. Existing endpoints, operation DTOs and the journal stream are unchanged; `policy.published` maps to no SSE topic.
- **Config.** One new key, `[policy] shadow`, default `false`. The environment variable `DOMOVOI_POLICY__SHADOW` reaches the hub; `DOMOVOI_POLICY__ALLOW_RISK` still reaches only the agent.
- **Journal.** `policy.published` is a new `JournalKind`; readers already tolerate unknown kinds (`JournalEntry::kind` is text).

## 10. Error handling and recovery

| Situation | Exact behaviour |
|---|---|
| `[policy] shadow = false` | no recorder task, no rows; overview reports `shadow_enabled: false`; editor, preview, publish and simulate work; the simulator reads rows recorded earlier, if any |
| No published version when a step is swept | rules = `[]`, row `policy_version = 0`; effect-class defaults decide (P3) |
| Stored rule fails to decode during a sweep | sweep aborts with `ShadowError::Rules`; no row is written; `last_error` = `"stored rules unreadable"`; the next tick retries |
| Contract not announced any more, or host unknown | row `Unevaluable("contract_unknown")`, no gap list needed; the step itself is untouched |
| Host identity unproven (no enrolment) | row `Unevaluable("target_unresolved")` |
| `agent.package.*` contract on a step | row `Skipped("package_contract")`, never evaluated |
| `evaluate` returns `Err(PolicyError)` | the `DecisionEnvelope::Error { code }` is stored, outcome text `"error"`; the sweep continues |
| Hub offline for hours, backlog on restart | the sweep drains it oldest first, 10 batches of 50 per tick; each row is marked `reason = "late"` when evaluated more than 5 minutes after the step; inputs are those at evaluation time |
| Crash between batches | nothing lost; anti-join re-selects the unrecorded steps; `ON CONFLICT DO NOTHING` makes a repeat harmless |
| Two hubs, or a recorder restarted mid-batch | same: the first writer wins, counters count inserted rows only |
| JSON column over 64 KiB | row `Unevaluable("too_large")` |
| `ShadowEnvironment` cannot establish a host input | a value plus an `InputGap`, never an error, never a retry loop |
| Save with a locked-rule change and no password | 403 `forbidden` "enter your password to change the policy"; nothing written, no audit row |
| Wrong password | 403 `forbidden` "wrong password"; shares the login limiter; lockout -> 429 |
| Save or publish with a stale `preview_hash` | 409; the client re-runs the preview |
| Publish with a missing acknowledgement | 409 listing the missing keys; with an unknown key -> 400 |
| Publish while another publish landed | 409 `StaleActive`; the draft is kept; save it again to re-base |
| Draft save against a stale revision | 409 `StaleDraft`; nothing written |
| Rule names `agent.package.*` | 400 at save and at inline simulate |
| Rule names a contract or category the publication index does not know | the draft saves; `preview` returns the error under `validation`; `publish` returns 400 `Invalid`. An owner can write a skill rule only for a skill some host currently announces |
| `simulate` with `source: draft` and no draft | 404 |
| A recorded row whose columns no longer decode | counted in `not_replayable`; the rest of the batch is answered |
| Audit append fails | the whole commit fails; nothing is published |
| Store failure | `ApiError::internal`; details logged without rule text or password |

## 11. Security requirements

1. **Nothing here authorizes anything.** No `PolicyPermit`, `Authorization`, `HumanApproval` or `PermitIssuer` is named by any new file; `crates/domovoi-hub/src/policy/` is not created; `PolicyPermit::issue` and `Authorization::policy` keep zero production call sites (gate, 12.4). A recorded `Allow` is data.
2. **No enforcement path reads a decision.** `OperationHandle::authorize`, `execute_step`, the operation writer, `ActionHandle`, `auth/` and the agent link never import `policy_shadow`, `PolicyRepo`, `ShadowRecord` or call `policy::evaluate*` (gate).
3. **Step-up.** Publishing always, and any draft save that creates, edits, unlocks or removes a locked rule, requires the password at that request, checked through `AuthState::confirm_password` with the trusted-proxy client address, sharing the login limiter. The check is the first thing a command does; a refused password writes nothing, not even an audit row.
4. **Acknowledgement is exact (D-19).** Publication succeeds only when the acknowledged keys equal the blocking finding keys, and the acknowledgements are stored with the version, who and when.
5. **The preview binds the decision.** `preview_hash` covers the exact rules (`rules_hash`), the active version number, the blocking keys and the locked changes; a hash from a different rule set, base or finding set never publishes.
6. **Published versions are immutable**: enforced by the applier (a published row never matches a draft-only write) and by the two SQL triggers; versions are never deleted.
7. **Fail closed in the recorder.** An unknown root-agent state is recorded as `true`; a helper-dependent contract as `helper_incomplete`; absent facts as `Unknown`; a missing ceiling as `None`. Nothing is invented as safe, and every such substitution is listed in `gaps`.
8. **No secrets.** The stored documents hold ids, names, argument values the skill already validated and ceiling summaries. They hold no password, token, key or prompt. `PolicyError` text is never stored (only its code); notes are owner text and are never copied into audit or journal rows. Skill argument values are already redacted by the operation layer's rules; the recorder stores `ExecutionRecord.args` as the operation stored them and adds no new source of values.
9. **Read access** to every route needs a session; none is public. The service never builds a `Caller`; `Caller::web` still has exactly one production call site (prompt 29's gate stays green).
10. **Audit.** `policy.published`, `policy.lock_changed`, `policy.draft_saved`, `policy.draft_discarded` are appended in the same transaction as the change they describe. Shadow rows are deliberately not audited (derived, replayable, no authority; RW-15).
11. **Bounded work.** 500 rules, 500-character notes, 512 KiB draft, batches of 50 (10 per tick), simulation at most 500 rows (100 explicit ids), preview simulation 200 rows, 64 KiB per stored JSON column.

## 12. Testing requirements

Use `FakeClock`, `InMemoryStore`, `FakeShadowEnvironment` **(new)** and the existing `ListRedactor`/harnesses. No test sleeps; the recorder is driven by `sweep_once`. Name tests after behaviour.

### 12.1 Store and testkit

Contract suite `crates/domovoi-testkit/src/contracts/policy.rs` **(new)**, run on `InMemoryStore` (`crates/domovoi-testkit/tests/contracts.rs`) and SQLite (`crates/domovoi-store/tests/sqlite/contracts.rs`):

| Test | Setup -> action -> expected |
|---|---|
| `draft_create_replace_discard_round_trip` | save `Absent` -> revision 1; save `Revision(1)` -> revision 2 with the new rules; discard -> `draft()` is `None` and no rule rows remain |
| `second_draft_is_refused` | save `Absent` twice -> second is `Conflict` |
| `stale_draft_revision_is_refused` | save `Revision(1)` after revision 2 exists -> `Conflict`, rules unchanged |
| `publish_assigns_consecutive_numbers_and_freezes_rules` | publish draft -> number 1, `active()` is it, `draft()` is `None`; save and publish again -> number 2; `rules(v1)` unchanged |
| `publish_with_wrong_number_conflicts` | `Publish { number: 5 }` while max is 1 -> `Conflict`, draft kept |
| `published_rules_cannot_be_rewritten` | (SQLite only) a raw `UPDATE` or `DELETE` on a published version's rule row aborts with the trigger text |
| `acknowledgements_are_stored_with_the_version` | publish with two `AckRecord`s -> `acknowledgements(id)` returns both, ordered by key |
| `shadow_candidates_are_unrecorded_executions_oldest_first` | three executions, record the middle one -> candidates are the other two in `(created_at, id)` order; `limit` honoured |
| `record_shadow_is_idempotent_and_counts_inserted_rows` | record the same `execution_id` twice -> one row, `total_evaluated` 1, first writer's `evaluated_at` kept |
| `shadow_records_filter_and_page_newest_first` | rows across two hosts and statuses; filter by host, status, `differing`, window; keyset pages cover all rows once |
| `shadow_summary_counts_by_status_and_outcome` | known mix -> exact `by_status`, `by_enforced_outcome`, `degraded`, `differing` |
| `empty_store_reads_are_empty` | `active`, `draft`, `versions`, `shadow_records` all empty; `shadow_state` is the zero state |

SQLite: `migration_adds_the_policy_tables_and_keeps_existing_rows` (section 8) and `policy_write_survives_reopen` (publish, reopen the database, read it back).

### 12.2 Engine (`crates/domovoi-engine/tests/engine/policy_shadow*.rs` for the pure parts, registered in `tests/engine/main.rs`; the recorder tests in `crates/domovoi-engine/tests/operations/policy_shadow.rs`, registered in that target's `main.rs`, using the operation harness of prompts 26/27 to create Execution steps)

| Test | Behaviour |
|---|---|
| `rule_envelope_matches_golden` | snapshot of one envelope per `Condition` variant, a locked deny and a shadow allow |
| `rule_envelope_round_trips_every_condition` | `decode_rule(encode_rule(r)) == r` for each variant; `{"v":2,..}`, an extra key and a bad rule are `Err` |
| `rules_hash_is_order_and_content_sensitive` | swapping two rules, or editing a note, changes it; the empty set has the documented value |
| `decision_doc_mirrors_decision_field_for_field` | evaluate a fixture that fills every field (locked, overridden, limits, warnings, would-have-been) -> `DecisionDoc` serialises to the golden snapshot |
| `recorded_inputs_round_trip_and_rebuild_the_same_request` | `build` -> `RecordedInputs` -> JSON -> `request_from_inputs` equals the original `EvaluateRequest` (`PartialEq`) |
| `build_skips_package_contracts` | `agent.package.install` -> `Skip("package_contract")` |
| `build_marks_unknown_contract_unevaluable` | env returns `None` -> `Unevaluable("contract_unknown")` |
| `build_marks_unresolved_host_unevaluable` | `host_target` fails -> `Unevaluable("target_unresolved")` |
| `build_records_every_unavailable_input_as_a_gap` | default fake env (no ceiling, no facts, autonomy not modelled) with a rule that has `RequiresFact` -> gaps exactly `[Ceiling/NotProducedYet, Facts/NotProducedYet, Autonomy/NotProducedYet]` in `GapInput` declaration order |
| `unknown_root_state_is_recorded_as_root` | `runs_as_root` `None` -> `request.runs_as_root == true`, gap `RunsAsRoot/HostNotConnected` |
| `pinned_identities_come_from_resolvable_hosts_only` | two pinned hosts, one unresolvable -> one identity, one `PinnedTargets/Unresolvable` gap; a container-targeted pinned subject adds `PinnedTargets/NotProducedYet` once |
| `host_name_resolves_for_rules_that_name_it` | rule `Exact({"nas"})` -> `rule_targets == {"nas": host identity}`; no such rule -> empty |
| `recorder_records_one_row_per_execution_without_a_policy` | no published version, three steps -> three `Evaluated` rows, `policy_version 0`, enforced outcome `require_approval` for a `Mutate` skill |
| `recorder_evaluates_enforced_and_shadow_rules_separately` | active version with an enforced deny and a shadow allow -> `enforced_outcome deny`, `shadow_outcome allow`, and the `differing` filter finds the row |
| `recorder_is_idempotent_across_sweeps_and_restarts` | `sweep_once` twice, then a new recorder over the same store -> still one row per step |
| `recorder_skips_packages_and_marks_unevaluable_steps` | mixed fixtures -> `Skipped` and `Unevaluable` rows with their codes; none reappears in the next sweep |
| `recorder_marks_late_rows` | advance the fake clock 10 minutes before the sweep -> `reason == Some("late")`, status `Evaluated` |
| `recorder_writes_no_journal_and_no_audit` | after sweeps, the journal head and audit chain length are unchanged |
| `recorder_fails_closed_on_unreadable_rules` | corrupt one stored rule body -> `sweep_once` is `Err(Rules)`, zero rows, `last_error` set; fix the rule -> next sweep records |
| `recorder_drains_a_backlog_in_bounded_batches` | 120 steps -> one sweep records 120 in 3 commits; with 600 steps one sweep stops at 500 and the next finishes |
| `shadow_mode_does_not_change_the_operation_outcome` | run the same scripted investigation with the recorder running and not running -> identical operation trace and identical `Execution` states |
| `replay_reproduces_every_recorded_decision` | record under version 1, `replay(rules of v1, row, Enforced)` and `Shadow` -> `Comparison::Same` for every row (acceptance 5) |
| `replay_with_a_changed_rule_reports_the_difference` | edit one rule in the draft -> rows it affects are `Changed`, the rest `Same`; `summarise` counts `newly_denied`/`newly_allowed` correctly |
| `locked_changes_classify_created_edited_unlocked_removed` | one case each, plus an unrelated unlocked edit -> empty |
| `finding_keys_and_preview_hash_are_stable` | same inputs -> same hash; a changed note on a locked rule, a different base number or an added blocking key -> a different hash |
| `current_inputs_mode_uses_the_clock_not_the_recorded_instant` | a `TimeWindow` rule that holds at the recorded time but not at the fake clock's time -> `Recorded` is `Same`, `Current` is `Changed` |

### 12.3 Hub (`crates/domovoi-hub/tests/hub/policy_api.rs`, harness `api_harness.rs`; unit tests beside `services/policy*.rs`)

| Test | Behaviour |
|---|---|
| `policy_routes_require_a_session` | each of the eleven routes -> 401 without a cookie |
| `draft_lifecycle_over_http` | PUT create, GET, PUT replace with the returned revision, DELETE; stale revision -> 409 |
| `saving_a_draft_without_locked_changes_needs_no_password` | PUT with ordinary rules and no password -> 200 and one `policy.draft_saved` audit row |
| `locked_rule_change_needs_password_and_preview_hash` | create a locked deny: no password -> 403, nothing written; password but no hash -> 409; both -> 200, one `policy.lock_changed` row per change; editing, unlocking and removing it behave the same |
| `wrong_password_is_forbidden_and_lockout_is_429` | repeated wrong passwords share the login limiter |
| `publish_always_needs_the_password` | correct hash and acknowledgements, no password -> 403, no version created |
| `publish_requires_exact_acknowledgements` | draft with an incomparable conflict: none -> 409 listing the key; an extra unknown key -> 400; the exact key -> 200 and `AckDto` stored with the version, who and when |
| `publish_with_stale_preview_hash_conflicts` | edit the draft after previewing -> 409 |
| `publish_records_journal_and_audit_in_one_commit` | exactly one `policy.published` journal row (payload shape of 6.11, no rule note) and one `policy.published` audit row; failing the audit append (fake) publishes nothing |
| `publish_twice_assigns_numbers_and_keeps_the_old_version_readable` | v1 then v2; `GET versions/1` unchanged; overview `active.number == 2` |
| `publish_refuses_package_contract_and_unknown_contract_rules` | 400 with `issues[].path = "rules.<id>"` |
| `preview_reports_conflicts_locks_and_a_bounded_simulation` | known fixture history -> exact `blocking`, `locked_changes`, simulation counts, and a `preview_hash` equal to the one `publish` expects |
| `simulate_reproduces_recorded_decisions_for_the_recorded_version` | after the recorder ran under v1, `POST simulate {source: version 1, selection: both}` -> every item `same` (acceptance 5) |
| `simulate_draft_shows_what_would_change` | a draft that denies a recorded allow -> that item `changed`, `summary.newly_denied` 1 |
| `simulate_never_writes` | row counts, audit length and journal head identical before and after |
| `shadow_endpoints_page_and_filter_and_404` | list filters, cursor, detail 404 before the recorder has seen the step, detail includes `gaps` and `inputs` |
| `overview_reports_standing_notes_and_zone` | notes present, `shadow_enabled` follows the config, `time_window_zone` equals the zone 39 implemented |
| `policy_openapi_covers_the_eleven_routes` | the generated document contains each path and operation id, and no schema name contains `Permit` |

### 12.4 Gates (extend `crates/domovoi-hub/tests/gates.rs` and `crates/domovoi-engine/tests/gates.rs`; never weaken an existing gate)

- `no_code_path_issues_a_policy_permit` (hub gates): (a) the directory `crates/domovoi-hub/src/policy/` does not exist; (b) walk every `.rs` under `crates/domovoi-core/src/policy_store/`, `crates/domovoi-store/src/policy/`, `crates/domovoi-engine/src/policy_shadow/`, `crates/domovoi-engine/src/policy/`, `crates/domovoi-hub/src/services/policy*.rs`, `crates/domovoi-hub/src/runtime/policy.rs` and `crates/domovoi-hub/src/api/*policy*.rs` and assert none contains `PolicyPermit`, `Authorization`, `HumanApproval` or `PermitIssuer` (not even in a comment; mirror the walk in `crates/domovoi-engine/tests/gates.rs:30-56`; a test path is exempt, so the names appear only in the gate itself); (c) assert `Authorization::policy(` and `PolicyPermit::issue(` have no site in any production source under `crates/` (the test paths are exempt, the existing `xtask arch-check` card stays as the second line of defence).
- `no_enforcement_path_reads_a_policy_decision` (engine gates): no `.rs` under `crates/domovoi-engine/src/operations/`, `crates/domovoi-engine/src/actions/` or `crates/domovoi-hub/src/auth/` names `policy_shadow`, `PolicyRepo`, `ShadowRecord`, `policy::evaluate`, `evaluate_shadow` or `StateWrite::Policy`.
- `policy_writes_are_built_only_by_their_owners` (hub gates): `PolicyWrite::SaveDraft`, `DiscardDraft` and `Publish` appear in production source only under `crates/domovoi-hub/src/services/policy*.rs` (and the store applier's `match`); `PolicyWrite::RecordShadow` only under `crates/domovoi-engine/src/policy_shadow/` (and the store applier).
- `policy_commands_take_a_caller_and_step_up` (hub unit test over `services/policy.rs` source text): `save_draft`, `discard_draft` and `publish` have `caller: &Caller` as their first parameter, and `publish` calls `step_up(` before `repo.` reads (a text-order scan, so reordering fails the test).
- `policy_publish_is_a_registered_step_up_action`: `policy.publish` and `policy.lock_changed` are in `STEP_UP_ENFORCED_ACTIONS` and `policy.publish` is no longer in `STEP_UP_ACTIONS`.
- Arch cards: if `cargo xtask arch-check` flags a module placement, fix the placement; never add an exception. Prompt 29's `MOD-1..4` and `CALLER-1` stay green (`PolicyService` does not name `ActionHandle`/`OperationHandle`).

### 12.5 Existing suites

The whole of `crates/domovoi-store`, `crates/domovoi-engine` operations tests, `crates/domovoi-hub` and `crates/domovoi-config` suites run unchanged apart from the exhaustive-match arms and the three `ApiDeps` constructors. If any existing snapshot changes (the `ApiDoc` document is regenerated, not snapshotted), stop and report.

## 13. Acceptance criteria

1. Migration `*_policy_versions.sql` adds the five tables and two triggers, is additive, and the N-1 upgrade test passes.
2. A draft can be created, replaced under a revision guard, previewed, discarded and published; versions are numbered 1, 2, 3 without gaps; exactly one published version (the highest number) is active; a published version cannot be modified or deleted, and every past version and its acknowledgements are readable.
3. With `[policy] shadow = true`, every Execution step gets exactly one `policy_shadow_decisions` row (also across restarts and repeated sweeps), holding the enforced-rules decision, the shadow-rules decision, the exact recorded inputs, the input gaps and `degraded`; with `false` no recorder runs and no row is written.
4. **Nothing is enforced**: with the recorder on or off, the same scripted operation produces the same trace, approvals and dispatches (`shadow_mode_does_not_change_the_operation_outcome`); no enforcement path names a policy decision (gate); no `PolicyPermit` is issued anywhere (gate `no_code_path_issues_a_policy_permit`).
5. **The simulator reproduces each recorded decision**: replaying a row's recorded inputs against the version it was recorded under yields `Comparison::Same` for the enforced and the shadow selection, over every `Evaluated` row of the fixture history.
6. `POST /api/v1/policy/simulate` evaluates a draft, a stored version or inline rules over recorded or current inputs, never writes, and reports per item the baseline, the simulated decision and `same | changed | not_replayable`, plus a summary.
7. Inputs that do not exist before the helper and the ceiling ship are recorded as gaps with the fail-closed substitution (`ceiling: null`, no facts, `runs_as_root` true when unknown, helper-dependent contracts `helper_incomplete`, autonomy `NotModelled`), never as a made-up safe value, and never block a sweep.
8. Publication runs `validate_publication` over the compiled contracts plus announced skills, returns every finding with a stable key, and succeeds only when the acknowledged keys equal the blocking keys; acknowledgements are stored with the version.
9. Publishing always needs the password; saving a draft that creates, edits, unlocks or removes a locked rule needs the password and the matching `preview_hash`; each locked change writes a `policy.lock_changed` audit row; publication writes one `policy.published` journal row and one audit row in the same transaction; a wrong password writes nothing and shares the login limiter.
10. `agent.package.*` contracts are skipped by the recorder (`Skipped` row), refused when a rule names them, and the standing note says so.
11. `HubConfig.policy.shadow` exists (default false); `DOMOVOI_POLICY__SHADOW` reaches the hub, `DOMOVOI_POLICY__ALLOW_RISK` still reaches only the agent; `docs/book/src/reference/configuration.md` is regenerated.
12. All eleven routes exist with `utoipa` annotations; `openapi.json` and `schema.d.ts` are regenerated; `RuleDto`, `PreviewDto`, `FindingDto`, `SimulationDto`, `DecisionDto`, `ShadowDetailDto`, `PolicyOverviewDto`, `VersionDetailDto` and `DraftDetailDto` match section 6.10 field for field (prompt 43 consumes them).
13. The stored rule envelope, `DecisionDoc`, `RecordedInputs` and gap list match their golden snapshots.
14. No `Authorization`, `PolicyPermit`, `HumanApproval` or `PermitIssuer` text appears in any new file except the gate test; `Caller::web` still has one call site; no engine handle was added to `ApiDeps` or `ChatDeps`.
15. Every source file is at or below 500 lines; every public item has a doc comment; no `unwrap`/`expect`/`panic!` in library code; no new `#[allow]`; no new dependency.
16. `cargo fmt --all`, `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo nextest run -p <crate>` pass for each crate touched, plus `cargo xtask arch-check`, `cargo xtask docs-config`, `cargo xtask docs-links`; web `npm run typecheck` passes after `gen:api`.

## 14. Migration and compatibility

- Existing installs: the migration is additive and runs on first start; an install with no operations has empty tables; `shadow` defaults to `false`, so behaviour is identical until the owner turns it on.
- An older binary reading a newer database ignores the new tables (`min_reader` unchanged). Downgrading after publishing a version loses nothing; the rules are inert data to an older hub.
- No agent, protocol, skill, incident, action or notification change. Legacy `actions` are not Execution steps and are never recorded; only `executions` rows from the operation layer are.
- The owner's first publication creates version 1; before it, the recorder evaluates the empty rule set (P3), so the defaults can be inspected before any rule is written.
- The shadow table grows by one small row per Execution step and has no retention rule in this change (a later retention change owns it; say so in the report).
- If prompt 45 later reads published versions for permit issuance, it reads `PolicyRepo::active` and `decode_rules`; the format is pinned by the golden snapshots.

## 15. Documentation requirements

- **ADR** `docs/adr/NNNN-policy-shadow-recording.md` (next free number): the decisions P1-P6 and the five in this prompt that are interpretations (publication index = compiled contracts plus announced skills; `preview_hash` binding; key-granular environment scoping for `[policy]`; recorder substitutions for unavailable inputs; no retention yet). Plain language, no prompt numbers, no build-plan or agent references.
- **Book** `docs/book/src/concepts/policy.md` **(new)**, added to `SUMMARY.md`: what rules are, how the most specific rule wins, `Deny`/`RequireApproval`/`Allow` meanings, locked denies, shadow mode (what it records, that nothing is enforced, what `degraded` means), versions and publication, the acknowledgement and password steps, the simulator. Say plainly that the ceiling and fact inputs are not yet available. `docs/book/src/reference/api.md`: the eleven routes. `cargo xtask docs-config` regenerates `reference/configuration.md` (the `[policy] shadow` key; confirm the agent's `[policy]` page is not clobbered). `cargo xtask docs-links` must pass.
- **`ARCHITECTURE.md`**: add the new core module, engine module, store tables and hub service to the crate descriptions, and the sentence that no code path issues a `PolicyPermit` before the permit issuer exists. Keep §4 edges unchanged (no new edge).
- **`AGENTS.md`**: no change unless a command or rule changed (it did not).
- **Module docs**: each new file gets a `//!` line; the doc comments of `ShadowEnvironment`, `InputGap` and `PolicyWrite` say why each fail-closed substitution is made and who may build each variant.
- **No prompt numbers, build-plan or agent references** in any committed file, commit or PR text (the standing rule; `AGENTS.md` alone may name them).


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
