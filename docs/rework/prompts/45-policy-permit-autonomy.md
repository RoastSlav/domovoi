# 45 — Policy permits, bounded autonomy and `unit.restart`

| | |
|---|---|
| Branch | `policy-permit-autonomy` |
| Milestone | M8 (L2 bounded autonomy) |
| Depends on (must be merged) | 47 (47a helper accounts, skeleton and protocol; 47b skill runner; 47c attestation; 47d helper read operations, which replaces 47a's `Read { group }` request with typed read variants and adds `HelperRequest::group()`. The helper names below are those of the four briefs, reconciled in `README.md` ("Cross-brief names"); if the merged code differs, use the merged names and report each difference), 40 (policy store, shadow recorder, `PolicyRepo`), 39 (evaluator), 32 (layered context, merged), and what those need: 22 and 23 (`Authorization`, plan hash v2, the execution boundary), 24 and 25 (contracts, registry, target resolver), 26 and 29 (operations, services), 41 and 42 (their patterns are reused; their code is not changed) |
| Can run in parallel with | nothing. 46 (`containment`) runs after this prompt |
| Produces (shared contracts) | `domovoi_core::unit::*` (unit name and identity types, `resolve_identity`, refusal tokens, card text, `SystemdView` and `UnitFs` ports); `domovoi_core::capability::unit_contracts()` (`unit.identity`, `unit.restart`, and the `container.restart` contract with no dispatch path); `domovoi_core::action::CapabilityPlan` (domain line `plan:capability-v1`); `domovoi_core::ports::{PermitIssuer, UnitRelay, UnitCaps}` and `PolicyPermit::restore`; two `HelperRequest` variants; `AgentLink::{start_unit_call, unit_call_result}`; `TargetResolver::resolve_unit`; `ExecutionRecord` permit storage and `authorize` for a permit; the hub `policy` module (`HubPermitIssuer`); agent `[unattended]` ceiling and its enforcement; helper operations `unit.identity` and `unit.restart`; hub `[policy]` guardrail keys; tables for permits, autonomy state, circuit resets and unknown-outcome resolutions; owner commands for pause, resume, rule reset and unknown-outcome resolution; one ADR |
| Consumes | 39's `evaluate` and `Decision`; 40's `PolicyRepo::active`, `decode_rules`, shadow codec and `ShadowEnvironment`; 22's `PolicyPermit`, `PolicyPermitParts`, `Authorization::policy`; 23's single boundary and cards; 25's registry, `CeilingSummary`, `parse_ceiling`; 26's writer and `PolicyDecision` step; 20's journal and limiter; 21's `OutcomeUnknown` and reconcile; 47a's `domovoi_core::helper` protocol, `domovoi-helper` crate, `PlatformTier`, `TierDecision::helper_write`, `write_opt_in` and `domovoi_agent::helper_client`; 47d's `HelperRequest::group()`; 47c's `ExpectedSafety` (45 sends `expected_safety = None` and uses nothing else of `domovoi_core::attest`); synthesis `01-r8-unit-identity-synthesis.md` sections 5 and 7 (the identity algorithm and tests, followed as decided) |
| Launch gate | 47 (a, b, c and d) merged; **R-8 accepted by the owner** (recorded in the `99` R-8 row and in the ADR this prompt writes; the acceptance conditions still open on 99 are a real-installer pass with the production helper reading identity over D-Bus, Debian 12, Ubuntu 22.04, systemd 258, a non-merged `/usr` host and an SELinux host); **H1 complete** (44's ADRs accepted, 47 merged). The Docker-helper ADR gate that `phase7-working/02` lists is **not** a gate of 45, because 45 ships no Docker path (section 6.2) |
| Suggested model | large (security-sensitive, several crates; the final review stays with the main session) |

## Prerequisites and ground rules (read first)

1. **Hub use-case layer (ADR 0026).** Every command (pause, resume, reset a rule, resolve an unknown outcome) is a method on a service in `crate::services` taking `&Caller`; handlers only parse, call and map. Give new handles to `Services`, never to `ApiDeps` or `ChatDeps`. The new top-level hub module `policy` goes into `UNZONED_MODULES` in `xtask/src/arch/rules.rs` (it is the permit issuer's directory, not a transport or a service).
2. **47a to 47d are prerequisites, not merged here.** 47a's brief (`prompts/47a-helper-accounts-and-skeleton.md`) fixes the helper protocol as a closed `HelperRequest` enum with `Health`, `Groups`, `Read` (47d replaces it with nine typed read variants and the accessor `HelperRequest::group() -> Option<HelperGroup>`; 45 adds its two variants beside them, maps them in `group()` and extends 47d's test `read_op_group_mapping_is_exact`; 47d's test `systemd_read_stays_not_implemented` is replaced here by `unit_identity_is_served_when_systemd_read_is_enabled`), `SafetyAttest` and **no write variant and no free-form string**, a `HelperResponse` with `Groups { enabled, ignored, write_opt_in, platform_tier, platform_record }`, `Unavailable`, `Refused` and `StartRefused`, one request per connection (`Accept=yes`), a new crate `domovoi-helper` (edges `core`, `config`) with the real `HostInspect` adapter, and `domovoi_agent::helper_client`. 47a explicitly leaves `unit.identity` and `unit.restart` to 45. Open 47's merged code, use its real names and report each difference. If its framing cannot carry two typed operations with string outputs (section 6.4), stop and report.
3. **No wire change.** `RunCapability` already has `target: TargetKey`, string `args`, `authorization_kind` and `expires_at`; `CapabilityOutcome::Succeeded { outputs }` and `Failed { code, message }` already exist. Nothing under `crates/domovoi-proto/` changes and the agent-hub protocol stays 1.2. The helper socket's own protocol (`HELPER_PROTOCOL`) gains two request variants (6.4).
4. **Do not weaken anything.** Where this prompt replaces a gate test, the replacement is stricter (section 11) and goes in with the ADR.

## 1. Objective

Turn the recorded, never-enforced policy of 39 and 40 into **bounded unattended execution of exactly one capability, `unit.restart`**, with every guard the design asks for, and ship that capability end to end. Concretely:

1. **PermitIssuer.** A hub-side issuer in `crates/domovoi-hub/src/policy/` that turns an enforced `Allow` decision for one Execution step into a stored `PolicyPermit`, and refuses in every other case. It is the only code that calls `PolicyPermit::issue` or `Authorization::policy`.
2. **Ceiling.** The agent-side `agent.toml [unattended]` ceiling (04 §8.3): config, `ceiling_hash` and summary in `hello` and `agent_status`, and the ordered checks for every `policy`-labelled request. The hub cannot widen it.
3. **Guardrails.** A global pause switch, a per-rule circuit breaker (default two `Failed` or `OutcomeUnknown` results in 24 hours), hub-side per-rule and global permit rate limits, the pinned-subject rule (already a hard limit), and the **unresolved-`OutcomeUnknown` unattended-reuse guard** (21 defined it, 45 enforces it).
4. **`unit.restart` and `unit.identity`.** The two contracts, the helper operations behind them (`systemd.write`, off by default; `systemd.read`, on by default), the unit identity of synthesis 01 implemented as decided, the dispatch path for a capability step, the post-restart check, start-limit handling, the approval card text, the platform tier gate and the per-host write opt-in.
5. **`container.restart`.** The contract exists so rules can name it; it is **not dispatchable** by anyone in 45 (section 6.2). No Docker helper operation is written.
6. **Small UI.** Authorization attribution on an execution and the pause switch with the circuit-breaker display (section 6.11). No chat pause command.

It does **not** build containment, the helper skeleton, the attestation, a Docker helper, or configuration-management detection (section 5).

## 2. Architectural context

**What authorizes what (RW-10, RW-11, D-23).** The evaluator (39) returns `Allow`, `RequireApproval`, `Unavailable` or `Deny`. Only `Allow` may become a permit, and only after the hard limits (the ten of `crates/domovoi-engine/src/policy/limits.rs`) have held. A permit is not proof to the agent; it is a hub label. The agent and the helper re-check what they can: the agent the ceiling, the rate limiter and `allow_risk`; the helper its allowlist, protected set, identity and limits (RW-12, RW-39). A compromised hub can label a request `human` and skip the ceiling; the helper allowlist, the protected set and the helper limits still hold. That is the accepted v1 limitation, stated in the docs.

**Why only `unit.restart`, and why skills stay human-only.** `RunSkill` on the wire has no `authorization_kind`, and the agent's ceiling is enforced on `run_capability`. A policy `Allow` for a `skill.<name>` contract therefore cannot be dispatched unattended by 45. The issuer refuses it (`not_policy_dispatchable`) and the step stays an approval offer. The set of policy-dispatchable contracts is a closed list in core with one entry, `unit.restart`; 46 adds `firewall.block_source`.

**A restart runs the loaded configuration (RW-34, synthesis 01).** `systemctl restart` runs what systemd has loaded, not the files on disk, and `NeedDaemonReload` has false negatives (mtime-preserved edits, rename-replace, generator input, `set-property --runtime`, and on systemd 255 a drop-in in a new directory, which `systemctl cat` also misses). So the identity binds the **loaded** properties plus the disk contents of every file of the unit, and a disk-side enumeration of the whole search path is the only stale check. The helper never runs `daemon-reload`. The residual race between the helper's last read and the manager acting cannot be closed (no guard reached zero in any measured row); the card says so and the post-restart check detects a wrong start.

**Refusal classes (owner decisions D-53 to D-55).** Plain units whose fragment and every drop-in are regular files under `/etc/systemd/system` or `/usr/lib/systemd/system` only. Refused: linked, generated, transient, masked, `enabled-runtime`, `/usr/local/lib`, `/run`, `system.control`, generator drop-ins, `set-property`-only units, symlinked files, unlisted locations, a unit outside the helper allowlist or inside the protected set. An alias **name** that resolves to a plain allowed unit is accepted and only its `Id` and `Names` are bound (taken on the owner's standing instruction to use the recommendation). `restart` (not `try-restart`) is used, so a stopped unit starts; the exact instance allowlist keeps a never-started template instance out.

**Platform tiers (D-39, synthesis 00 §5b).** Helper write groups are allowed on a Supported platform; on an Untested platform (a systemd host passing every isolation check but not release-tested) they are refused unless the owner opts in per host (off by default, audited); on a Blocked platform they are refused. Nothing is Supported yet, so every host needs the opt-in today. 47a already computes `PlatformTier` and the opt-in (`write_opt_in`); 45 consumes them.

**Time and boundaries.** Time-dependent code takes the `Clock` port. Only `domovoi-runner` spawns processes: the helper calls `busctl` and `systemctl` through the `ProcessExecutor` port the binary injects. The identity encoding and refusal logic are pure code in `domovoi-core` behind two ports (`SystemdView`, `UnitFs`); the real adapter lives in `crates/domovoi-helper`.

## 3. Repository context

Verified in this checkout (worktree on branch `research-synthesis`, HEAD `010ece76`; the code is the implementation branch with 39, 40, 41, 42 and 43 merged, and documentation commits on top). Re-open each item before editing and report differences. The migrations directory ends at `0019_install_jobs.sql`, so the next free number is expected to be `0021` (47c adds the first migration of the series, expected `0020`; 46 expects `0022`); list the directory and describe the migration by its contents. The ADRs end at `0032-helper-safety-attestation.md` (0030 to 0032 are `proposed`, in an open pull request); 47a to 47d take 0033 to 0036, so this prompt's ADR is expected to be `0037`; list `docs/adr/` and take the next free number.

**Authorization and the writer.**

- `crates/domovoi-core/src/action/authorization.rs`: `Authorization` has private internals (`:39-49`), `Authorization::policy(PolicyPermit)` (`:68`), `policy_permit()` (`:85`), `AuthorizationKind { Human, Policy }` (`:15`). `PolicyPermit` (`:109-118`) has private fields `decision_id: String`, `rule_ids: Vec<String>`, `policy_version: String`, `plan_hash`, `issued_at`, `expires_at`, `ceiling_hash: String`, `safety_identity: Option<String>`, one constructor `PolicyPermit::issue(PolicyPermitParts)` (`:148`) and only three accessors (`plan_hash`, `expires_at`, `decision_id`). There is **no restore constructor and no accessor for the other fields**, so a store cannot persist or rebuild a permit today (6.1 adds both).
- `crates/domovoi-core/src/operation/execution.rs`: `ExecutionRecord::authorize` (`:258-297`) answers a policy authorization with `ExecutionError::NotYetSupported` at `:265-267`; `ExecutionRecord` (`:178`) stores only `approval: Option<ExecutionApproval>`; `ExecutionParts` (`:137`) is the restore bundle. The `executions` table (`0012_operations.sql:85-109`) already has `authorization_kind TEXT` but no permit columns.
- `crates/domovoi-engine/src/operations/execute.rs`: `AuthorizedStep::of` (`:47-52`) refuses a row with `approval().is_none()`; `execute_step` (`:128-256`) is skill-only: it builds `SkillDispatch` (`:209-215`), calls `HandoffRunner::hand_off` (`:216-218`) and `await_handoff_result` (`:233-237`), and builds `OperationCommand::Dispatch` once (`:177-185`). The recompute (`:308-327`) builds a `PlanTarget` from a `CheckKey` and the v2 hash from a skill's manifest.
- `crates/domovoi-engine/src/plan/validate.rs`: `is_executable` (`:27`) is `id.is_skill()`; a built-in contract is refused `NotExecutable` at `:139-149`.
- `crates/domovoi-engine/src/targets/mod.rs`: `TargetQuery::Unit` always answers `UnitIdentityUnproven` (`:177`).
- `crates/domovoi-core/src/action/plan.rs`: `PlanHash::compute_v2` needs an `ImplHash` (manifest and entry digest), a `PlanTarget` (host, key, `CheckKey`, optional `TargetKey`) and `ValidatedArgs`. None fits a built-in helper contract, and RW-08 requires the unit's `identity_hash` to be bound.
- `Decision` (`crates/domovoi-engine/src/policy/decision.rs`) carries `winners`, `matched`, `policy_version: Option<Uuid>` and `ceiling_hash: Option<String>` but **no decision id and no permit-ready rule list**; the issuer mints the decision id (UUIDv7) and takes `rule_ids` from `winners`. `RuleId` is `[A-Za-z0-9][A-Za-z0-9_.:-]{0,63}`. `PolicyVersionRecord` (`crates/domovoi-core/src/policy_store/version.rs:42`) has `number: Option<u32>`, `id` and `rules_hash`. `Actor::Policy(String)` exists (`crates/domovoi-core/src/actor.rs:33`) but its name grammar is `[a-z0-9._-]`, narrower than `RuleId`, so it is **not** used; permit issuance is audited by `Actor::system("policy")` with the rule ids in the detail.

**Policy inputs (40).**

- `crates/domovoi-engine/src/policy_shadow/inputs.rs`: `build` skips `agent.package.*` (`:67`), refuses any contract whose `target_kind != Host` as `target_kind_unsupported` (`:77`) and reads autonomy through `AutonomyReading::NotModelled`, a single-variant enum (`env.rs:47`, `inputs.rs:133`), always `paused: false, circuit_broken: false`. The hub's environment is `HubShadowEnvironment` in `crates/domovoi-hub/src/runtime/policy.rs` (its `autonomy()` is at `:187`). `EffectKind::{EvaluatePolicy, IssuePermit}` exist in `crates/domovoi-core/src/ports/uow/effects.rs:26-29` and nothing uses them; `StepKind::PolicyDecision` and `StepIntent::PolicyDecision { execution }` exist (`crates/domovoi-core/src/operation/intent.rs:78`).
- `AutonomyState { paused, circuit_broken }` (`policy/request.rs:158`) feeds hard limit 9 (`AutonomyInactive`, `limits.rs:105-110`); `ceiling_covers` (`limits.rs:142`) already compares an `Allow` with the agent's `CeilingSummary` (exact contract id, targets, rate, TTL, constrainable args).

**Ceiling and capability plumbing.**

- `crates/domovoi-config/src/agent.rs` has `[mutation_limits]` (`:61-86`) and `[policy] allow_risk` but **no `[unattended]` section**. `crates/domovoi-agent/src/link/handshake.rs:33-35` sends `ceiling_hash` and `ceiling_summary` as `None`. The hub side exists: `crates/domovoi-hub/src/capability/ceiling.rs` (`parse_ceiling`, `CeilingParse`), `CeilingSummary` and `CeilingEntry` in `crates/domovoi-core/src/capability/ceiling.rs`.
- `crates/domovoi-agent/src/requests/capability.rs:166-299`: `run_capability` knows only `agent.package.*` (`:181`); step 5 (`:214-222`) refuses `authorization_kind = Policy` for **every** mutating contract. That refusal must stay for packages only. `crates/domovoi-agent/src/journal/limiter.rs` implements `[mutation_limits]` (RW-38).
- `crates/domovoi-core/src/ports/agent_link.rs:385-420`: `start_capability` and `capability_result` take `PackageDispatch` and return `PackageOutputs`; they are package-shaped. `xtask/src/arch/rules.rs:225-270` and `:345-377`: `RAW_OPS`, and EXEC-2 cards where `run_capability` has `sites: &[]` and `start_capability`/`capability_result` have one site each in `crates/domovoi-hub/src/runtime/caps.rs`. `BOUNDARY_PAIRS` holds `AuthorizedStep::of`, `execute::execute_step`, `AuthorizedPackageTarget::of`, `execute::execute_package`, `OperationCommand::Dispatch`, `Caller::web`, `Caller::chat`.
- `crates/domovoi-core/src/ports/caps.rs`: `PackageRelay` and `PackageCaps` (`:140`, `:200`) are the model for the new `UnitRelay` and `UnitCaps`. `crates/domovoi-core/src/capability/package.rs` is the model for contract construction; `known_contracts()` (`:75`) is builtin plus package contracts; `ContractIndex::new` rejects a category that spans two effect classes.
- `crates/domovoi-hub/src/capability/registry.rs:257-292, 405-425`: `ExecutionPath::Helper` contracts map `HelperState::Absent` to `HelperMissing` and `Failed` to `HelperError`, and non-read contracts need a ceiling entry; the registry **never reads `HelperStatus::Present { ops }`** and has no write-tier concept.
- `crates/domovoi-core/src/capability/target.rs:94-118, 175-215`: `TargetQuery::Unit`, `TargetUnresolved::UnitIdentityUnproven`; `TargetIdentity::parse` already accepts the `unit` scheme (`ids.rs:224`), rest at most 300 characters; `TargetName` at most 128. `canonical_container_identity` (`target.rs:260`) is `container:{host}/{key}/{id}`.

**Hub, store, gates.**

- `crates/domovoi-hub/tests/gates.rs:955-1019` (`no_code_path_issues_a_policy_permit`) asserts that `crates/domovoi-hub/src/policy` **does not exist**, that the policy sources do not name `PermitIssuer`, and that no production code calls `Authorization::policy(` or `PolicyPermit::issue(`. `xtask/src/arch/rules.rs:178-188` and `scan.rs:213-228` already allow both calls under `crates/domovoi-hub/src/policy/` and tests. Engine gates that mention permits: `crates/domovoi-engine/tests/gates.rs:406, 457, 1536, 2007`.
- `crates/domovoi-hub/src/settings/step_up.rs:99-130`: `STEP_UP_ENFORCED_ACTIONS` (`policy.publish`, `policy.lock_changed`) and `STEP_UP_ACTIONS` where `autonomy.resume` says "autonomy (the pause switch) exists".
- `crates/domovoi-hub/src/services/operations/controls.rs` has `cancel`, `pause`, `resume`, `answer` for one operation; **nothing resolves an `OutcomeUnknown` step**. `OperationHandle` and the writer (`crates/domovoi-engine/src/operations/writer/`) are the single writer; `OperationCommand::Authorize` is at `commands.rs:100`.
- `crates/domovoi-agent/src/journal/` (20): `accept`, `started`, `finished`, retention, reconcile answers; `crates/domovoi-checks/src/units/failed.rs` is the failed-unit check whose target is a unit name.
- Web: `web/src/features/policy/` (43) exists; error text is in `web/src/design/messages.ts`.

**Discrepancies found while writing this brief (resolved here; report if the code disagrees).**

1. The index and `README.md` still say 45 has no file. This is that file; neither is edited here.
2. `phase7-working/02` gates 45 on an accepted Docker-write ADR; the index row does not. **Resolution: the index is authoritative; 45 adds no Docker dispatch path**, so the gate does not apply.
3. 04 §8.3 and synthesis 00 §6 say the post-restart check reads "the loaded properties". Synthesis 01 REQ-20 and E44 say never re-read loaded properties after a restart (120, 9 and 2 false alarms) and read `/proc/<MainPID>/environ` instead (21 of 21, no false alarm). **Resolution: REQ-20.** The follow-up edit to 04 §8.3 is listed in section 15.
4. The agent has no `[unattended]` ceiling and sends none; the roadmap's M8 "Agent" line makes it 45's work.
5. Synthesis 00 §4 notes that `target_changed` and `target_stale` need one meaning each. **Resolution:** section 6.3 gives one trigger per token.
6. The wire `RefusalCode` is closed and has no helper tokens. **Resolution:** a helper refusal arrives as `CapabilityOutcome::Failed` with message `"<token>: <detail>"` (the convention 41 and 42 use); agent-side refusals use the existing `refused` codes (`ceiling_exceeded`, `rate_limited`, `allow_risk_denied`, `authorization_expired`, `plan_hash_mismatch`, `contract_unknown`).
7. No owner command resolves an `OutcomeUnknown` step. **Resolution:** section 6.7 adds one.
8. `PolicyPermit` cannot be restored or fully read. **Resolution:** section 6.1.
9. 47a's helper request enum is closed with no unit name and says the helper spawns nothing; 45 adds two typed variants and the first spawning operations. **Resolution:** 6.4.

## 4. Exact scope

Non-test source files stay at or below 500 lines (web files 250); split rather than exceed. Helper paths follow 47a's layout: use its real paths.

| File | Purpose |
|---|---|
| `crates/domovoi-core/src/unit/{mod,name,props,identity,encode,resolve,disk,refusal,post,keys,card}.rs` | pure unit identity: names, props, `UnitIdentityV1`, encoding, `resolve_identity`, `compare_identity`, tokens, post-restart check, argument and output keys, card text (6.3) |
| `crates/domovoi-core/src/unit/ports.rs` | `SystemdView`, `UnitFs` (read-only ports the helper implements) |
| `crates/domovoi-core/src/unit/call.rs` | `UnitCall`, `UnitDispatch`, `UnitOutputs` (closed call set, no free argument) |
| `crates/domovoi-core/src/capability/unit.rs` | `unit_contracts()`: `unit.identity`, `unit.restart`, `container.restart`; `POLICY_DISPATCHABLE` |
| `crates/domovoi-core/src/action/capability_plan.rs` | `CapabilityPlan` and its hash (6.1) |
| `crates/domovoi-core/src/ports/{permit,unit_relay}.rs` | `PermitIssuer` and its types; `UnitRelay`, `UnitCaps` |
| `crates/domovoi-core/src/ports/repos/permit.rs` | `PermitRepo` read port (permits, failures per rule, resets, pause state, unresolved unknowns) |
| `crates/domovoi-config/src/agent/unattended.rs` | agent `[unattended]` section and validation (6.5) |
| `crates/domovoi-config/src/hub/policy.rs` (changed) | hub `[policy]` guardrail keys (6.9) |
| `crates/domovoi-store/migrations/00NN_permits_and_autonomy.sql`, `crates/domovoi-store/src/repos/permit.rs`, `crates/domovoi-store/src/permit/{mod,apply,codec}.rs` | tables (section 8), `PermitRepo` for SQLite, `StateWrite::Permit` applier |
| `crates/domovoi-engine/src/operations/{execute_unit,permit}.rs`, `operations/effects/policy.rs` | the unit dispatch, the permit effect handler (6.7) |
| `crates/domovoi-engine/src/plan/unit.rs` | unit step validation (6.8) |
| `crates/domovoi-hub/src/policy/{mod,issuer,env,guards,rates}.rs` | `HubPermitIssuer`, live policy inputs, guards (6.6) |
| `crates/domovoi-hub/src/services/{autonomy,unknown}.rs` | pause, resume, rule reset, resolve-unknown commands (6.7) |
| `crates/domovoi-hub/src/api/{autonomy,unit_dto}.rs` | endpoints and DTO fields (6.9) |
| `crates/domovoi-agent/src/requests/{unit,ceiling}.rs`, `crates/domovoi-agent/src/unattended.rs` | unit capability handling, ceiling checks, hello summary (6.5) |
| `crates/domovoi-helper/src/systemd/{identity,restart,adapter,config}.rs`; `crates/domovoi-core/src/helper/` (two request variants) | helper operations and the real `SystemdView`/`UnitFs` adapter over `ProcessExecutor` (6.4) |
| `crates/domovoi-testkit/src/unit/{systemd,fs,relay}.rs`, `crates/domovoi-testkit/fixtures/unit/*.json` | `FakeSystemd` (reload counter), `FakeUnitFs`, `FakeUnitRelay`, recorded `busctl --json=short` fixtures (section 12) |
| `docs/adr/0037-permit-issuer-and-unit-restart.md` (expected number; next free at implementation time) | required because `xtask/src/arch/rules.rs` changes |
| web: `web/src/features/policy/{AutonomyPanel,SuspendedRules}.tsx`, attribution in `web/src/features/operations/` | 6.11 |
| tests | section 12 |

Files to **change** (minimal edits): `crates/domovoi-core/src/{lib.rs,action/{mod,authorization}.rs,capability/{mod,package,availability,target}.rs,operation/execution.rs,ports/{mod,agent_link,caps,target}.rs,helper/*}`, `crates/domovoi-engine/src/{operations/{execute,mod,proposal,decide/execution}.rs,operations/effects/{mod,dispatch}.rs,operations/writer/{commands,execution,mod}.rs,plan/{validate,mod}.rs,targets/mod.rs,policy_shadow/{env,inputs}.rs}`, `crates/domovoi-hub/src/{capability/registry.rs,runtime/{caps,policy,mod}.rs,agent_server/link.rs,services/mod.rs,settings/step_up.rs,api/mod.rs,lib.rs}`, `crates/domovoi-agent/src/{requests/{capability,mod}.rs,link/handshake.rs,journal/limiter.rs,runtime.rs,helper_client*}`, `crates/domovoi-config/src/{agent.rs,hub/mod.rs}`, `crates/domovoi-store/src/{lib,uow/writes,purge}.rs`, `xtask/src/arch/rules.rs`, `crates/domovoi-hub/tests/gates.rs`, engine and core `tests/gates.rs`, `web/src/api/{openapi.json,schema.d.ts}` (generated only), docs (section 15).

## 5. Explicit non-goals

- **No containment** (`firewall.block_source`, nft, conntrack, attestation consumption beyond what the evaluator already reads): that is 46. `ContainmentInput` stays `not_applicable` for the contracts 45 issues.
- **No helper skeleton, accounts, sockets, `selftest` checks, attestation or key custody**: 47. 45 adds two operations and a config table to a helper that already exists.
- **No Docker helper operation and no dispatch of `container.restart`** by a human or a policy (6.2), and no change to the Docker-helper decision D-24.
- **No configuration-management detection** (synthesis 01 idea I12) and no `daemon-reload` anywhere (D-53). No in-process D-Bus client (idea I10): `busctl` through `ProcessExecutor`.
- **No policy-authorized skill execution.** Skills stay human-only (section 2).
- **No chat pause command.** Pause is web-only in 45; a chat button needs its own allowlist-caller design. Reported as a decision.
- **No owner-signed approvals, no helper-side authorization check** (deferred in 06).
- **No wire change, no protocol bump, no change to `agent.package.*` behaviour** (packages stay human-only).
- **No retention rule for permit rows** beyond the existing operation retention; permits are kept with their executions.
- Do not weaken any gate test or arch rule; do not touch merged migrations.

## 6. Required interfaces and contracts

Marked **fixed** (must match) or **convention** (follow repository style).

### 6.1 Core: permits, plan hash, execution row (`domovoi-core`)

**`PolicyPermit` (fixed semantics).** Add accessors for every field (`rule_ids()`, `policy_version()`, `issued_at()`, `ceiling_hash()`, `safety_identity()`) and a store-only constructor:

```rust
/// Rebuilds a permit the store persisted. Call only in `crates/domovoi-store/src/`; it records no decision.
pub fn restore(parts: PolicyPermitParts<'_>) -> Result<Self, PermitError>;   // same validation as issue
```

`xtask`: new constant `POLICY_PERMIT_RESTORE_ALLOWED_PREFIXES = ["crates/domovoi-store/src/", "crates/domovoi-testkit/"]` and a scan rule beside `HUMAN_APPROVAL_RESTORE_ALLOWED_PREFIXES` (`scan.rs`, rule `INV-1`).

**`ExecutionRecord` (fixed).** Add `permit: Option<StoredPermit>` (a private-field value holding the permit, the decision id and the stored decision document text) with the invariant *exactly one of `approval` and `permit` is set once authorized*. Replace the `NotYetSupported` branch of `authorize` (`execution.rs:265-267`) by: if `authorization.policy_permit()` is `Some(permit)`, apply in this order and change nothing on refusal: (1) the stored risk must be `Some(Risk::Write)` else `ExecutionError::PermitNotAllowed`; (2) `permit.plan_hash() == self.plan_hash` else `PlanChanged`; (3) already authorized (approval or permit) -> `AlreadyAuthorized`; (4) `now >= permit.expires_at()` or `now >= self.expires_at` -> `Expired`; then store. Keep `NotYetSupported` as a variant for the package path only. Add `authorization_kind() -> Option<AuthorizationKind>` and `permit() -> Option<&StoredPermit>`; extend `ExecutionParts` with `permit: Option<StoredPermit>` for the store.

**`CapabilityPlan` (fixed encoding; a new domain-separated family, never `plan:v1` or `plan:v2`).** Same reasoning as 42's `PackagePlan`: v2 needs a skill manifest and a `CheckKey`, and RW-08 requires the unit's `identity_hash`. The family binds exactly: contract id and version, host id and enrolled key fingerprint, the target identity text, the identity hash (when there is one), the canonical arguments and the execution id. Not bound: the incident, proposer, expiry, approval time, UI text, the helper or agent binary.

```rust
pub struct CapabilityPlan { /* contract, host, host_key, target, identity_hash, args, execution */ }
impl CapabilityPlan {
    pub fn unit_restart(host: HostId, host_key: &KeyFingerprint, target: &TargetIdentity,
                        identity_hash: &IdentityHash, args: &BTreeMap<String, String>, execution: ExecutionId) -> Self;
    pub fn unit_identity_read(host: HostId, host_key: &KeyFingerprint, name: &UnitName, execution: ExecutionId) -> Self;
    pub fn hash(&self) -> PlanHash;
}
```

Canonical text, every value length-framed as `name:len:value` (the repository's framing; synthesis 01 REQ-11 allows it):

```
plan:capability-v1
contract:{len}:{id}@{version}
host:{len}:{host_target_text(host, key)}
target:{len}:{identity or "absent"}
identity_hash:{len}:{64 hex or "absent"}
args:{len}:{canonical args, name:len:value\n per argument, name order}
execution:{uuid}
```

The literal `plan:capability-v1` is written only in `crates/domovoi-core/src/action/capability_plan.rs` (`xtask`: a rule like the `plan:v2` one, `scan.rs:233`). Golden vectors are pinned in tests (section 12).

**Contracts and the policy-dispatchable list (fixed).** `crates/domovoi-core/src/capability/unit.rs`: `pub const POLICY_DISPATCHABLE: &[&str] = &["unit.restart"];` and `pub fn is_policy_dispatchable(id: &ContractId) -> bool`. `known_contracts()` (`package.rs:75`) appends `unit_contracts()`.

### 6.2 The three contracts (fixed)

| | `unit.identity` | `unit.restart` | `container.restart` |
|---|---|---|---|
| version | 1 | 1 | 1 |
| effect | `Read` | `Mutate` | `Mutate` |
| category | `unit.observe` | `unit.lifecycle` | `container.lifecycle` |
| target kind, arg | `Unit`, `unit` | `Unit`, `unit` | `Container`, `container` |
| execution | `Helper` | `Helper` | `Helper` |
| reversibility, idempotency | not applicable, idempotent | reversible, idempotent | reversible, idempotent |
| timeout, output limit | 30 s, 16 KiB | 60 s, 8 KiB | 60 s, 8 KiB |
| verification | none | `Postcondition`: "the new process runs the configuration you approved" | `Postcondition` (text only) |
| `constrainable_args`, `safety_facts` | none | none | none |
| input schema | `{ unit }`, required, no additional properties | `{ unit }` (the typed name; the hub sends the canonical `Id` and `identity_hash` on the wire, 6.6) | `{ container }` |

Check that no category already spans another effect class (`ContractIndex::new`). The unit-name and container-name grammars are validated before any call.

**`container.restart` (decided).** The contract exists so owners can write rules naming it and the registry can explain it. 45 adds **no path** that dispatches it: `is_executable` does not list it (a plan naming it is refused `not_executable`), `POLICY_DISPATCHABLE` omits it, and the registry answers `Availability::Unavailable { reason: UnavailableReason::DockerWriteUnsupported }` ("Docker write access needs its own helper decision and is not available") for **both** authorization kinds and whatever the helper reports. Add that variant to `capability/availability.rs` with its `describe()` text. A gate test asserts that no production source names a Docker helper write operation. The contract becomes dispatchable only after the Docker-helper ADR (D-24) is accepted and a later prompt adds the helper operation.

### 6.3 Unit identity (`domovoi_core::unit`, pure; fixed algorithm, synthesis 01 section 5)

**Names and ids.**

```rust
/// A unit name as typed: `[A-Za-z0-9:_.\\@-]`, ends in `.service`, at most 128 characters, no leading `-`,
/// no glob character, not a bare template (`x@.service`). Refused before any manager call.
pub struct UnitName(String);
/// The canonical `Id` systemd reports; same grammar. Instances are exact strings (`foo@bar.service`).
pub struct UnitId(String);
sha256_newtype!(IdentityHash);           // 64 lowercase hex
```

The 128-character cap keeps `unit:{host}/{key}/{Id}` (36+1+64+1+128 = 230) inside `TargetIdentity`'s 300-character rest; a test asserts it. `canonical_unit_identity(host, &key, &UnitId) -> Result<TargetIdentity, ValidationError>` is `unit:{host}/{key}/{Id}` and is **stable across configuration changes**: it is what pinning and rule resolution use. The configuration enters through `IdentityHash`, bound separately in the plan hash (RW-34: `unit:{host}/{canonical_id}` plus `identity_hash`).

**Ports (read-only).**

```rust
#[async_trait]
pub trait SystemdView: Send + Sync {
    /// `busctl` `LoadUnit` (never `GetUnit`, E50) then `GetAll` on the Unit interface; arrays arrive as arrays.
    async fn load_unit(&self, name: &UnitName) -> Result<UnitProps, ViewError>;
    /// Service-interface properties of the loaded unit, runtime fields already removed.
    async fn service_props(&self, id: &UnitId) -> Result<LoadedProps, ViewError>;
    /// `InvocationID`, `ExecMainPID`, `ActiveState`, `Result`, `NRestarts`, start-limit settings.
    async fn runtime(&self, id: &UnitId) -> Result<UnitRuntime, ViewError>;
    /// What a started process runs: `/proc/<pid>/environ`, and cmdline and uid.
    async fn process_view(&self, pid: u32) -> Result<ProcessView, ViewError>;
}
pub trait UnitFs: Send + Sync {
    fn lstat(&self, path: &str) -> Result<FileKind, FsError>;                // Regular | Symlink | Other
    fn read_once(&self, path: &str) -> Result<Vec<u8>, FsError>;             // one open, one read
    /// The disk-side drop-in path set systemd would load for this unit over every search directory, existing now.
    fn enumerate_dropins(&self, id: &UnitId, names: &BTreeSet<String>) -> Result<Vec<String>, FsError>;
}
```

`UnitProps` carries `id`, `names: BTreeSet<String>`, `load_state`, `unit_file_state`, `transient`, `fragment_path`, `source_path`, `drop_in_paths: Vec<String>` (application order, as the manager reports), `need_daemon_reload`, `triggered_by: BTreeSet<String>`, `triggers`. Set-valued properties are `BTreeSet`s so ordering cannot leak into a hash.

**`UnitIdentityV1` and its hash (fixed).**

```rust
pub struct UnitIdentityV1 {
    pub id: UnitId, pub unit_file_state: UnitFileState,      // enabled | disabled | static | indirect
    pub fragment: FilePin,                                   // { path, sha256 }
    pub drop_ins: Vec<FilePin>,                              // manager order (application order)
    pub disk_drop_ins: Vec<String>,                          // must equal the paths of drop_ins
    pub loaded_hash: [u8; 32],
}
```

`identity_hash = SHA-256(encode(UnitIdentityV1))`, domain tag first (`unit-identity:v1`), every field framed `name:len:value`, lists as a count then framed elements, no unframed concatenation. The loaded hash covers: `ExecStart` (path, argv, ignore flag only; `start_time`, `stop_time`, `pid`, `code`, `status` stripped), `ExecStartPre`, `ExecStartPost`, `ExecStop`, `ExecReload` argv, `Environment` (order kept), `EnvironmentFiles` paths, `User`, `Group`, `WorkingDirectory`, `Restart`, `Type`, `StartLimitIntervalUSec`, `StartLimitBurst`, plus `Requires`, `BindsTo`, `PartOf`, `Wants`. **Sorting (REQ-22):** every list-valued unit property is treated as a set and sorted before hashing, except `DropInPaths`, `Environment` and the `Exec*` argv. A path with a control character, a non-absolute path, `..` or `//` is refused.

**Resolution, in this order (first match decides; one trigger per token).**

```text
fn resolve_identity(name: &UnitName, sys: &dyn SystemdView, fs: &dyn UnitFs, cfg: &UnitPolicy)
    -> Result<UnitIdentityV1, UnitRefusal>
 1  name grammar already enforced by UnitName::parse                       -> target_unsupported/bad_name
 2  u = sys.load_unit(name)  (busctl failure, timeout, bad reply)          -> read_failed
 3  u.load_state == "not-found"  (exit status is never a signal)           -> target_not_found
 4  u.load_state != "loaded"  (masked, also a /usr/lib mask, E54)          -> target_unsupported/masked
    u.transient                                                            -> target_unsupported/transient
    u.unit_file_state not in {enabled, disabled, static, indirect}         -> target_unsupported/<masked|linked|generated|enabled_runtime|alias|other>
 5  cfg.protected ∩ ({u.id} ∪ u.names) != ∅                                -> target_protected
 6  u.id not in cfg.allow (templates: exact instance string)               -> target_not_allowlisted
 7  u.need_daemon_reload                                                   -> target_stale
 8  every path of [fragment] ++ drop_ins: no control char, under /etc/systemd/system/ or /usr/lib/systemd/system/
    (drop-ins end in .conf)                                                -> target_unsupported/location(path) | control_character
    every path is a regular file by lstat                                  -> target_unsupported/symlink(path)
 9  disk = fs.enumerate_dropins(...)  over ALL search directories, walked now (E47);
    any path in a non-allowed directory                                    -> target_unsupported/location(path)
    disk set != reported drop-in paths                                     -> target_stale
10  read each file once (sha256 of the bytes of one open); any failure     -> read_failed   (never an empty hash)
11  loaded = sort_sets(sys.service_props(u.id)); loaded_hash = sha256(encode(loaded))   (failure -> read_failed)
12  return UnitIdentityV1 { ... }
```

Step 9's search path is the research replica `a_disk`: `/etc/systemd/system.control`, `/run/systemd/system.control`, `/run/systemd/transient`, `/run/systemd/generator.early`, `/etc/systemd/system`, `/run/systemd/system`, `/run/systemd/generator`, `/usr/local/lib/systemd/system`, `/usr/lib/systemd/system`, `/run/systemd/generator.late`; it considers the unit name, its template name, alias names (links pointing at the unit), dash-prefix truncations (`a-b-.service.d`, `a-.service.d`) and `<unit>.service.d`; the first directory wins per file name; order is by basename. `systemctl cat` and `NeedDaemonReload` are **never** the stale check and `systemctl show` text is never parsed (idea I1 failed). Never pass `--json` to `systemctl`.

**Compare at dispatch (fixed).**

```text
fn compare_identity(bound: IdentityHash, now: &UnitIdentityV1) -> Result<(), UnitRefusal::TargetChanged { field }>
```
The request carries `{ id, identity_hash }`. The helper first refuses a request whose `id` is not the manager's `Id` for that name (`target_changed`, field `id`), then runs `resolve_identity` in full (every refusal above applies first, so a drop-in added after approval is `target_stale`, a mask is `target_unsupported/masked`), then compares. A difference is `target_changed` with the **first differing field** in this order: `id`, `unit_file_state`, `fragment`, `drop_in:<path>`, `drop_ins`, `loaded`. `target_changed` means only that and needs a new approval.

**Token table (fixed; each token has one trigger).**

| Token | Exact trigger | Notes |
|---|---|---|
| `target_unsupported` + reason | step 1 `bad_name`; step 4 `masked`, `transient`, `linked`, `generated`, `enabled_runtime`, `alias`, `unit_file_state`; step 8 `control_character`, `location`, `symlink`; step 9 `location` | the reason is a closed enum; a `location` or `symlink` reason names the path (at most 200 characters) |
| `target_not_found` | step 3 | |
| `target_protected` | step 5 | matched by `Id` and every `Names` entry |
| `target_not_allowlisted` | step 6 | |
| `target_stale` | step 7, or step 9 set mismatch | the owner reloads, then plans again |
| `target_changed` | only the compare above | names the first differing field |
| `read_failed` | any unreadable file, `busctl` failure or timeout at steps 2, 10, 11, or at the dispatch re-read | not `target_changed`: nothing is known; no capability to read foreign-owned files is held |
| `platform_untested` | the host's tier is Untested and the per-host write opt-in is off (write operations only) | 6.4 |
| `platform_blocked` | the tier is Blocked (write operations only) | |
| `restart_budget_exhausted` | the helper-side budget (6.4) | refused before dispatch, not a failed unit |
| `restart_failed` | `systemctl restart` exited non-zero and `Result` is not `start-limit-hit` | |
| `start_limit_hit` | `Result=start-limit-hit` read after a non-zero exit (never the message text, which differs by version) | the unit is left failed; never retried; `reset-failed` never called |
| `no_restart_observed` | `InvocationID` unchanged after a zero exit | |
| `wrong_start` | the post-restart check (6.4) found a mismatch | alert only |

`UnitRefusal` is a core enum with `token() -> &'static str` and `describe() -> String` (plain sentences for the card and `describeError`); a round-trip test pins the strings.

### 6.4 Helper operations (on top of 47a's framing; names as defined in 47a and 47d; a difference in merged code is reported)

45 adds two typed operations to the helper and nothing else. In `domovoi_core::helper` add `HelperRequest::UnitIdentity { name: UnitName }` and `HelperRequest::UnitRestart { id: UnitId, identity_hash: IdentityHash }`; both carry validated types, so the enum still holds no free-form string, and 47a's `deny_unknown_fields`, frame cap and one-request-per-connection rules apply unchanged. A successful answer is a new `HelperResponse::Outputs { outputs: BTreeMap<String, String> }` and a refusal is `HelperResponse::Refused`/a new `UnitRefused { token, detail }` carrying the 6.3 token. 47a's helper spawns nothing and its series rule (47a section 5) lets later briefs add fixed programs through the injected `ProcessExecutor`; 47c's `ip` and 47d's `smartctl`, `dmesg` and `ss` come first, and these (`systemctl`, `busctl`) are 45's. The helper unit (47a baseline plus 47d's candidate delta) ships no `RestrictAddressFamilies=` and leaves the executables readable, so `AF_UNIX` and the two programs need no unit change; the host tests confirm it. The helper does not see approval or policy (RW-39); it enforces its own config.

| Operation | Group | Default | Request | Response outputs (strings; limit 16 KiB) |
|---|---|---|---|---|
| `unit.identity` | `systemd.read` | **on** | `{ unit }` | `id`, `names` (JSON array), `unit_file_state`, `identity_hash`, `fragment` (path and sha256), `drop_ins` (JSON array of path and sha256), `loaded` (JSON: `ExecStart` argv, `User`, `Group`, `WorkingDirectory`, `Type`, `Restart`, `Environment` names and values, `PartOf`/`Requires` follower list), `active_state` (the agent adds `platform_tier` and `write_opt_in` from the helper's `Groups` answer) |
| `unit.restart` | `systemd.write` | **off** | `{ unit = <Id>, identity_hash }` | `restarted`, `invocation_before`, `invocation_after`, `main_pid`, `verification` (`verified` or `unverified`), `stale_start_suspected` (`true` or `false`) |

`unit.identity` runs steps 1 to 12 of 6.3 and returns the identity; it is a read, allowed on Supported and Untested platforms and where its own checks pass on a Blocked one. **Plan-time refusals include allowlist and protected** (steps 5 and 6), so a plan for a unit that can never run is refused early.

**`unit.restart`, in this order (fixed; synthesis 01 5.6).**

1. Tier gate: `platform_blocked` or `platform_untested` (opt-in off) refuse before anything else. The helper reports this gate through 47a's existing `Unavailable { group, reason }` answer (`WriteGroupNeedsOptIn` for an Untested platform without the opt-in, `PlatformBlocked` for a Blocked one), produced by the group gate of 47a's dispatcher before any operation runs; the agent maps those two reasons to the tokens `platform_untested` and `platform_blocked`, which are what the hub and the approval card show, so `UnitRefused` never carries a `platform_*` token. Tier and opt-in are 47a's `PlatformTier` and `write_opt_in` (the owner-set per-host switch in `helper.toml`, off by default, recorded by 47a); 45 only consumes them. The agent copies both from the helper's `Groups` answer into the outputs of every `unit.*` result so the hub can put them on the audit record (6.9).
2. Take the helper-side lock (a per-helper `flock`; serialises helper requests only).
3. Refuse a request whose `unit` is not the manager's `Id` for that name (`target_changed`, field `id`); run `resolve_identity` in full (every refusal of 6.3, with the dispatch-time allowlist and protected checks by resolved `Id` and every `Names` entry); `compare_identity`.
4. `NeedDaemonReload` was already required false (`target_stale`). The helper **never** runs `daemon-reload`, `reset-failed`, `stop`, `try-restart` or `set-property` (a gate test pins the argv set to `systemctl restart -- <Id>`).
5. Budget (synthesis REQ-18): read `StartLimitIntervalUSec` and `StartLimitBurst` from the loaded properties (they are in the loaded hash); keep a ring of restarts per `Id` **persisted in the helper state directory** (file `restarts`, mode 0600, written with 47a's `write_atomic_locked` under `state.lock`; the helper is one process per connection under `Accept=yes`, so an in-memory ring would never accumulate); if the count inside the interval is at least `burst - 1`, refuse `restart_budget_exhausted`. A unit with the limit disabled has no budget. The ring survives a helper restart. The same file carries the helper-wide count of accepted write operations for `[limits] mutations_per_hour` (47a's key, parsed there and not interpreted; **45 interprets it**: more accepted write operations than the limit inside the last hour is `restart_budget_exhausted`, and 46 counts `apply` against the same file; `0` means no write operation is accepted, the same fail-closed reading as `max_active = 0`; this reading is a cross-brief decision recorded in `README.md`).
6. Record `InvocationID`, `ExecMainPID`, `ActiveState`, `NRestarts`.
7. `systemctl restart -- <Id>` as an argv vector through `ProcessExecutor`, with the helper's `limits.timeout`, no shell. A "changed on disk" warning on stderr sets `stale_start_suspected` (a post-hoc signal, never a guard; its wording is untested outside 255, 257, 259).
8. Wait for the job (the blocking return of `systemctl restart`, never a sleep). A non-zero exit: read `Result`; `start-limit-hit` is `start_limit_hit` (no retry, no `reset-failed`), anything else `restart_failed`.
9. Post-restart check (**REQ-19 and REQ-20; the discrepancy of section 3 is resolved here**): `InvocationID` differs from before (`no_restart_observed` otherwise); `ActiveState` is `active` for a long-running service; then compare `/proc/<ExecMainPID>/environ` with the bound `Environment` values, and (design, untested) `/proc/<pid>/cmdline` with the bound `ExecStart` argv and the process uid with `User`. **The loaded properties are never re-read after the restart.** A mismatch is `wrong_start` carrying the differing field names, never values. A `Type=oneshot` unit or a process that exited before the read cannot be verified: the result is `Succeeded` with `verification = unverified`, never `verified`.
10. Return. The helper takes **no corrective restart and no stop** after a wrong start.

**Config (fixed behaviour, planned file `helper.toml`).** `groups` lists enabled groups; a fresh install enables the read groups including `systemd.read` and none of the write groups. `[systemd] restart_units = [...]` holds exact canonical `Id`s including `.service`, no globs, templates only as explicit instance strings; at load the helper resolves every entry and an entry whose `Id` differs from the written name is a config error naming the `Id` ("use `ssh.service`"). `[safety] protected_units` is unioned with the installer's defaults, which cannot be removed; the installer writes the host's spelling (`ssh.service` on Debian and Ubuntu), adds `ssh.socket` where the host socket-activates sshd, and keeps the agent, helper and runner units. Matching compares the resolved `Id` **and** every `Names` entry, and a service is also protected when its `TriggeredBy` names a protected socket. Example names in tests and docs are neutral (`example.service`).

### 6.5 Agent: ceiling, capability routing (`domovoi-agent`, `domovoi-config`)

**`[unattended]` (fixed shape, 04 §8.3).** Unknown keys are startup errors.

```toml
[unattended]
enabled = true

[[unattended.allow]]
capability = "unit.restart"          # exact contract id, no wildcard
targets = ["example.service"]        # canonical unit Ids; "*" must be written explicitly
max_per_hour = 2
```

`max_active`, `max_ttl` and `args` are accepted only for contracts that declare them; for `unit.restart` (no constrainable args, no TTL) each is a **config error** at load. A contract the agent does not know, or whose class is `Read`, `Destructive` or not in `POLICY_DISPATCHABLE`, is refused at load. The agent builds a `CeilingSummary` (the JSON shape `parse_ceiling` already reads) and `ceiling_hash` (SHA-256 of the canonical summary text), and sends both in `hello` and in `agent_status` on any change (`HelloExtensions.ceiling_hash`, `ceiling_summary`). With no `[unattended]` section both stay `None` and no `policy`-labelled request is accepted.

**`run_capability` routing.** Keep the existing package branch; add `unit.identity` and `unit.restart` (version 1 only) in `requests/unit.rs`. For `unit.*`, in this order, cheapest first, nothing journaled until the arguments parsed:

1. fenced journal -> `journal_corrupt`; 2. unknown contract or version -> `contract_unknown` / `contract_version_unsupported`; 3. the helper is absent, failed or does not offer the operation in its reported `ops` -> `contract_unknown` ("the helper does not offer unit.restart") or the `HelperUnavailable` failure code; 4. for `unit.restart` only: `allow_risk` must contain `write` (`allow_risk_denied`); 5. if `authorization_kind = policy`, the ceiling, in order: `[unattended].enabled`, an entry for the exact contract, the target in `targets`, `max_per_hour` counted from the journal's accepted **policy-labelled** records (`ceiling_exceeded` for each, with the failing step named in the detail); 6. `expires_at` in the future (`authorization_expired`); 7. parse arguments (exactly `unit` and `identity_hash` for a restart, exactly `unit` for an identity read) -> `bad_request`; 8. journal and boot snapshot available; 9. acceptance: deduplication, the mutation limiter (RW-38, human and policy alike), the durable `accepted` record; 10. call the helper through 47's client; helper tokens come back as `Failed { code: Refused, message: "<token>: <detail>" }` (or `HelperUnavailable`), redacted; 11. `journal.finished`.

The **package** refusal of `authorization_kind = policy` (`capability.rs:214-222`) stays, for packages only. The agent does not interpret the identity; it relays outputs unchanged. `unit.identity` is a read: it skips steps 4 to 6 and the limiter, and is journaled like `agent.package.list`.

### 6.6 Hub: link, relay, resolver, registry, issuer

**Link and capability object (fixed semantics, convention names).** `AgentLink` gains two default methods (default answers `LinkError::Transport("not supported by this link")`): `start_unit_call(host, UnitDispatch, expected_session, expected_epoch, deadline) -> Result<CapabilityHandoff, LinkError>` and `unit_call_result(host, handoff, deadline) -> Result<UnitOutputs, LinkError>`, with the same contract as `start_capability` and `capability_result` (failing means nothing was queued; only `Rejected` is the agent's confirmed answer). `UnitCall` is a closed enum `Identity { name }` and `Restart { id, identity_hash }`; `UnitDispatch { execution, call, plan_hash, authorization_kind, expires_at }`. On the wire a restart is `RunCapability { contract = "unit.restart", version = 1, target = <Id>, args = { unit = <Id>, identity_hash }, authorization_kind, expires_at, plan_hash, expected_safety = None }`. **No wire change.**

```rust
#[async_trait]
pub trait UnitRelay: Send + Sync {
    async fn read_identity(&self, host: HostId, name: &UnitName, session: AgentSessionId, deadline: Timestamp) -> Result<UnitOutputs, LinkError>;
    async fn dispatch_restart(&self, host: HostId, req: UnitDispatch, expected_session: AgentSessionId, expected_epoch: JournalEpoch, deadline: Timestamp) -> Result<CapabilityHandoff, LinkError>;
    async fn restart_result(&self, host: HostId, handoff: CapabilityHandoff, deadline: Timestamp) -> Result<UnitOutputs, LinkError>;
}
pub struct UnitCaps { pub probe: Arc<dyn AvailabilityProbe>, pub observer: Arc<dyn HostObservation>, pub journal: Arc<dyn JournalStatus>, pub relay: Arc<dyn UnitRelay> }
```

`runtime/caps.rs` builds `UnitCaps` from the one link, once, like `PackageCaps`. **Arch rules** (`xtask/src/arch/rules.rs`, with the ADR): add `start_unit_call` and `unit_call_result` to `RAW_OPS` with EXEC-2 cards, one site each in `crates/domovoi-hub/src/runtime/caps.rs`, copying the `start_capability` cards (check whether `agent_server/link.rs` implementations are counted); add `BOUNDARY_PAIRS` `execute_unit::execute_unit_restart` (one site, `operations/effects/dispatch.rs`); leave the `run_capability` "no caller" card and every existing card untouched; assert that `crates/domovoi-engine/src/operations/execute_unit.rs` is the only engine file naming `UnitRelay::dispatch_restart`, and that `read_identity` is called only from `crates/domovoi-engine/src/targets/`. Extract the block at `execute.rs:176-202` into one `pub(super) async fn commit_dispatch(..)` that both `execute_step` and `execute_unit_restart` call, so `OperationCommand::Dispatch { .. }` is still built exactly once in `execute.rs`.

**Resolver (fixed).** `TargetResolver` gains `resolve_unit(host, name) -> Result<ResolvedUnit, TargetError>` where `ResolvedUnit { target: ResolvedTarget, id: UnitId, identity_hash: IdentityHash, card: UnitCard }`; `resolve(TargetQuery::Unit)` returns `resolved_unit.target`. `TargetService` reads through `UnitRelay::read_identity`, checks the host is `Known`, builds the identity from `canonical_unit_identity(host, &enrolled_key, &Id)` and reads **everything from the helper**; it never reads a unit file. `TargetError` gains `UnitRefused { query, refusal: UnitRefusal }`; `TargetUnresolved::UnitIdentityUnproven` stays for a host with no helper or an agent that does not offer `unit.identity`. A reply whose `id` is not a valid `UnitId` or whose hash is not 64 hex is `read_failed`.

**Registry (fixed).** `Availability` for `unit.restart` becomes `NotInstalled` when the host's `HelperStatus::Present { ops }` lacks `unit.restart` (and for `unit.identity` likewise), `Unavailable { HelperMissing | HelperError }` as today, and `NotAllowedOnHost` when the ceiling has no entry (as today for non-reads). `container.restart` per 6.2.

**`PermitIssuer` (fixed semantics).**

```rust
#[async_trait]
pub trait PermitIssuer: Send + Sync {
    async fn issue(&self, request: PermitRequest) -> PermitOutcome;
}
pub struct PermitRequest { pub execution: ExecutionRecord, pub pinned: Vec<PinnedSubject> }
pub enum PermitOutcome {
    Issued(PermitGrant),                       // Authorization::policy built by the hub, plus the decision document text
    NotIssued(NotIssued),                      // the step stays an approval offer
}
pub enum NotIssued {
    Evaluated { outcome: Outcome, decision: String },         // RequireApproval, Deny, Unavailable
    Withheld(PermitRefusal),                                   // a guard
}
pub enum PermitRefusal {   // stored token text in the audit detail
    NotPolicyDispatchable, Paused, RuleSuspended(RuleId), UnresolvedUnknown(StepId), RateLimited(RateScope),
    PlanChanged, IdentityRefused(UnitRefusal), NoPublishedPolicy, InputsUnavailable,
}
```

`HubPermitIssuer` (in `crates/domovoi-hub/src/policy/issuer.rs`; the only file calling `PolicyPermit::issue` and `Authorization::policy`) does, in order, and issues only if every step passes:

1. `is_policy_dispatchable(contract)`, else `NotPolicyDispatchable`.
2. Autonomy paused -> `Paused`.
3. **Unresolved-`OutcomeUnknown` guard** (6.7) for the same host, contract and canonical unit `Id` -> `UnresolvedUnknown`.
4. Re-resolve the unit now (`resolve_unit`): a refusal -> `IdentityRefused`; recompute `CapabilityPlan::unit_restart(..).hash()` and require it equal to the execution's stored `plan_hash` -> else `PlanChanged` (a configuration change since proposal withholds the permit and leaves the approval path).
5. Load the active version (`PolicyRepo::active`, `decode_rules`); no published version -> `NoPublishedPolicy` (the empty rule set could never `Allow` an effect class that defaults to `RequireApproval`, so this is only a fast exit). Shadow rules (`Rule.shadow`) are never enforced: they are excluded from the enforced rule set exactly as 39's `RuleSelection` does.
6. Build the `EvaluateRequest` with the **live** environment (below) through the shared builder, call `evaluate` with `circuit_broken = false`. If the outcome is `Allow` and `winners` intersect the suspended rules, evaluate again with `circuit_broken = true` so the trace shows hard limit 9. Any outcome but `Allow` -> `NotIssued::Evaluated`.
7. Rate: permits issued in the last hour for this host and contract and globally against `constraints.max_per_hour` and `[policy] max_permits_per_hour` -> `RateLimited`.
8. `PolicyPermit::issue(PolicyPermitParts { decision_id: <fresh UUIDv7 text>, rule_ids: decision.winners, policy_version: <version number text>, plan_hash, issued_at: now, expires_at: now + permit_ttl (capped by the execution's expiry), ceiling_hash: decision.ceiling_hash, safety_identity: None })`, then `Authorization::policy`.

**Live environment (fixed).** Extend 40's `ShadowEnvironment` (do not fork it) and the shared `inputs::build`: (a) `AutonomyReading` becomes `Reported(AutonomyFacts { paused, suspended_rules })` and `NotModelled` is removed; the recorder then records the real state and drops the `Autonomy` gap; (b) `build` no longer refuses `target_kind = Unit`: it takes the target name and identity from `resolve_unit` (typed name as the rule target, `canonical_unit_identity` as the identity, pinned identities from the operation's pinned subjects mapped by host key and unit `Id`); the `target_kind_unsupported` row stays for container targets; (c) `ceiling` reads the host's reported `CeilingSummary` as today. Facts and containment keep their fail-closed readings (no fact is declared by `unit.restart`). The hub's `HubShadowEnvironment` implements the new readings from `PermitRepo`.

### 6.7 Engine: the permit effect, dispatch of a capability step, guardrails

**Permit effect (fixed flow).** `propose_execution` (`decide/execution.rs`) additionally enqueues one `EffectKind::EvaluatePolicy` effect for an Execution step whose contract `is_policy_dispatchable`, in the same commit that stores the Approval and Execution steps (key function `evaluate_key(step)` beside `dispatch_key`). The new handler `operations/effects/policy.rs` loads the snapshot, skips a step that is no longer pending or is already authorized, calls the `PermitIssuer` **port**, and on `Issued` sends the crate-private writer command `OperationCommand::AuthorizeByPolicy { held, operation, step, authorization, decision }` (the engine names the `Authorization` type but never builds one; it never names `PolicyPermit::issue`). On `NotIssued` it completes the effect and changes nothing: the human approval offer stays. The writer, in one commit: calls `ExecutionRecord::authorize` (permit branch), appends a `PolicyDecision` step (intent `PolicyDecision { execution }`, outcome `Succeeded`, summary the decision's one line) under the Execution step's parent, settles the Approval step `Skipped` with reason "authorized by policy", writes the permit row and audit `policy.permit_issued`, and enqueues the same `Dispatch` effect a human approval enqueues. A person can still approve first: exactly one authorization wins (`AlreadyAuthorized`). `EffectKind::IssuePermit` stays unused. If 26's step graph forbids settling an Approval step this way, stop and report.

**Dispatch (fixed).** `effects/dispatch.rs` builds `AuthorizedStep::of` once, then branches on the contract: a `skill.` contract goes to `execute_step`; `unit.restart` goes to `execute::execute_unit_restart` in the new `operations/execute_unit.rs`. `AuthorizedStep::of` now accepts a row with an approval **or** a stored permit. `execute_unit_restart`, in order: refuse an expired window; take the host's dispatch gate; count recent dispatches for the loop guard (same limit as skills); **for a permit-authorized row** refuse if autonomy is now paused (`NotRun`, reason `autonomy_paused`), if the permit expired, or if an unresolved `OutcomeUnknown` exists for the same host, contract and unit `Id` (`Refused`, reason `unresolved_unknown`); `resolve_unit` again, and any `UnitRefusal` is a `Refused` step whose reason is `"<token>: <sentence>"`; recompute `CapabilityPlan::unit_restart(..)` and pass it as `plan` to `commit_dispatch` (the writer refuses `PlanChanged` as for skills); build `UnitDispatch` with `authorization_kind` from the stored authorization (`human` or `policy`) and the stored expiry; `dispatch_restart`; then `restart_result`. A failure of `dispatch_restart` is proof nothing was queued (`NotRun`, `hub_not_handed_off`). A result error after hand-off leaves the step `Dispatched` and queues a reconcile exactly as `execute_step` does; **nothing is retried automatically after dispatch**.

**Outcome mapping (fixed).**

| What came back | Step outcome | Counts for the breaker |
|---|---|---|
| `Succeeded`, `verification = verified` | `Succeeded` | no |
| `Succeeded`, `verification = unverified` | `Succeeded`, with the reason text "restarted; the new process could not be checked" | no |
| `refused` frame: `ceiling_exceeded`, `rate_limited`, `allow_risk_denied`, `authorization_expired`, `plan_hash_mismatch`, `contract_unknown` | `Refused` | no |
| `Failed` with a pre-restart token: `target_*`, `read_failed`, `platform_*`, `restart_budget_exhausted` | `Refused` (the helper proves nothing restarted) | no |
| `Failed` with `restart_failed`, `start_limit_hit`, `no_restart_observed`, `wrong_start` | `Failed`; `wrong_start` and `start_limit_hit` also raise a P0 attention item ("the service may not be running what you approved" and "the service is down: the start limit was hit") through the attention derivation of prompt 51 (verify its module) | **yes** |
| result unavailable, resolved by the journal as `Unknown{..}` | `OutcomeUnknown` | **yes** |
| journal says `NotFound` | `NotRun` | no |
| unrecognised message | `Failed`, reason truncated to 500 characters | yes |

**Guardrails (fixed).**

- **Pause.** `PolicyService::pause_autonomy(&Caller, reason)` needs a person (web session and CSRF) and no password (it narrows authority). `resume_autonomy(&Caller, password)` needs the password (`autonomy.resume`, step-up, shares the login lockout). Pause blocks **new permits and the dispatch of permits already issued but not dispatched**. State is one row (section 8); every change is audited (`autonomy.paused`, `autonomy.resumed`) and journaled.
- **Circuit breaker.** A rule is suspended when, among permit-authorized executions whose permit names it, at least `[policy] circuit_failures` (default 2) ended `Failed` or `OutcomeUnknown` within `[policy] circuit_window` (default 24 h) **after the later of the window start and the rule's last reset**. The state is derived from the executions and the reset rows, never stored as a flag, so it cannot go stale or race. `reset_rule(&Caller, rule, password)` (step-up) writes a reset row and audit `policy.rule_reset`. Suspension withholds permits for matching steps (hard limit 9 via the second evaluation) and the UI shows it.
- **Rates.** Hub side: `constraints.max_per_hour` and `[policy] max_permits_per_hour` (default 12; 0 means no permit is ever issued). Agent side: ceiling `max_per_hour` and `[mutation_limits]`.
- **Pinned subjects.** Unchanged: hard limit 2 (`TargetNotPinned`) already turns an unpinned target into `RequireApproval`.
- **Unresolved-`OutcomeUnknown` unattended-reuse guard (fixed).** Key: host, contract id and the canonical unit `Id` (**not** the `identity_hash`, or a configuration edit would clear the block). An execution is *unresolved-unknown* while it ended `OutcomeUnknown`, was policy- or human-authorized, and has no row in `unknown_resolutions`. It blocks **only unattended** dispatch: a human approval of the same contract on the same unit is never blocked. Enforced twice, in `HubPermitIssuer` and again at dispatch for permit rows. It is cleared by: (a) `OperationCommands::resolve_unknown(&Caller, step, note, password)`, a person with a password (it widens authority again), audit `execution.unknown_resolved`; or (b) automatically when a **later human-authorized** run of the same host, contract and unit settles `Succeeded` with `verification = verified` (recorded as `how = verified_by_later_run`, in the same commit as the settlement). (b) is a design choice to confirm with the owner and is reported as such.

### 6.8 Plan validation and the approval path (`domovoi-engine`)

`is_executable` becomes `id.is_skill() || EXECUTABLE_BUILTINS.contains(id)` with `EXECUTABLE_BUILTINS = ["unit.restart"]`. `PlanValidator::check_step` gets a `unit.restart` branch beside the skill branch: validate `UnitName::parse` (grammar refusal `bad_unit_name`), resolve with `resolve_unit` (every `UnitRefusal` becomes a plan refusal with its token and sentence), pick the pinned subject (`pick_subject` also accepts the resolved `Id` when the typed name is an alias), require `Availability`, apply the budget and history checks unchanged, and return a `PlannedStep` with a new `unit: Option<UnitBinding>` (`id`, `identity_hash`, `card`). `operations/proposal.rs` computes the plan hash with `CapabilityPlan` for this contract; `ExecutionIntent.args` holds `{ unit: <typed name> }` and `target` the pinned subject's `TargetKey`. `PlanRefusalCode` gains `unit_refused`. The model gets the token and sentence so it can say why; it can never reach pause, resume, reset, resolve or a permit (the `MetaTool` enum is closed and gets no variant).

### 6.9 Hub config, API and audit

**`[policy]` additions (fixed keys, defaults, bounds; validation in `crates/domovoi-config`).** `permit_ttl` default `5m`, 30 s to 1 h; `circuit_failures` default 2, 1 to 20; `circuit_window` default `24h`, 1 h to 30 d; `max_permits_per_hour` default 12, 0 to 120. `[policy] shadow` keeps its meaning. Changing any of them needs the password (extend `STEP_UP` path rules in `settings/step_up.rs`). Regenerate the config reference.

**Endpoints (`/api/v1`, session and CSRF; convention DTO names).**

| Verb and path | Auth | Result |
|---|---|---|
| `GET /policy/autonomy` | session | `{ paused, paused_by, paused_at, reason, suspended_rules: [{ rule, failures, since }] }` |
| `POST /policy/autonomy/pause` | session, CSRF | pauses; body `{ reason }` |
| `POST /policy/autonomy/resume` | session, CSRF, password | resumes; 403 wrong password, 429 lockout |
| `POST /policy/rules/{rule}/reset` | session, CSRF, password | resets the breaker for a rule |
| `POST /operations/{id}/steps/{step}/resolve-unknown` | session, CSRF, password | clears the unattended block; body `{ note }` (1 to 280 characters) |
| existing execution DTOs | | gain `authorization: { kind: "human" \| "policy", user?, channel?, permit?: { decision_id, rules, policy_version } }` and, for a unit step, `unit: { id, identity_hash, card }` |

No endpoint accepts a client-supplied permit or approval. **Audit kinds (fixed):** `policy.permit_issued`, `policy.permit_withheld` (detail: the `PermitRefusal` token; written for `paused`, `rule_suspended`, `unresolved_unknown`, `rate_limited` only, to bound volume), `autonomy.paused`, `autonomy.resumed`, `policy.rule_reset`, `execution.unknown_resolved`, `unit.restart_dispatched` (detail: unit `Id`, `identity_hash`, authorization kind, decision id, host `platform_tier` and `write_optin` so an Untested-tier opt-in is on the record), `unit.restart_result` (detail: token and verification). Actor `Actor::User` for owner actions, `Actor::system("policy")` for issuance and the worker. Details never hold environment values.

### 6.10 The approval card text (fixed; golden snapshot)

Constants in `domovoi_core::unit::card`, shown on the human approval card and on the decision explanation of a permit. `{..}` are filled; nothing else is free text.

```
Restart {Id} on {host}.
This approval binds:
 - the unit as systemd knows it ({Id}); an alias is never bound
 - the unit file {fragment path} (sha-256 {12 hex}) and each drop-in: {path (sha-256 {12 hex})}
 - what a restart will run, as loaded now: {ExecStart argv}, user {User}, working directory {WorkingDirectory}; environment values are hidden
 - that the unit lives in /etc or /usr/lib and is not masked, linked, transient or generated, and that no drop-in exists on disk that systemd has not loaded
This approval does not bind:
 - the contents of EnvironmentFile= files, or of the programs the unit runs
 - other units that restart or stop with this one ({follower list, or "none seen"})
 - anything a user with root changes between the last check and the moment systemd acts. A small window remains and cannot be closed from here. After the restart Domovoi checks the new process and raises an alert if it is not running what you approved. It does not restart or stop it again.
A restart can fail and leave the service stopped (for example when systemd's start limit is hit). Restarting a stopped service starts it.
```

Stale variant (instead of the approval controls, no approval can be issued): "systemd has changes for this unit on disk that it has not loaded. Run `systemctl daemon-reload` on the host, then ask again. Domovoi never reloads for you." Untested-tier line when `platform_tier` is `untested`: "This platform is not release-tested; restarts are on only because you switched them on for this host." The environment values go through the hub `Redactor`, are capped at 20 entries and 200 characters, and are hidden by default on the card.

### 6.11 Minimal web (P0 for the switch)

`AutonomyPanel` on the policy overview (element id `policy.autonomy`, visibility P0, never hidden in any profile): current state, a Pause button, a Resume button behind `PasswordPrompt`, and the suspended-rule list with Reset (password). `SuspendedRules` reuses the kit table. An attribution line on the execution detail ("Approved by {user} in the web UI" or "Allowed by policy version {n}, rules {ids}") using the tone map (`web/src/design/tone.ts`). The "Not enforced" marks and standing notes of 43 change together with the first enforced rule: update the hub notes and the `not_enforced` mark in one change. Follow `web/DESIGN.md`: kit components only, `useActionFeedback`, `describeError`, no inline style, files at most 250 lines, page tests with axe. A step-up copy for resolve-unknown goes in `messages.ts`.

## 7. Ordered implementation steps

1. **Inspect and verify; report discrepancies.** Open every file in section 3. Confirm 47 is merged and read its real names: helper request and response types, config schema, tier output, the agent's helper client, `HelperStatus` `ops` population. Confirm 39, 40 and 32 as described. List migrations and ADRs for the next numbers. Write each difference down before editing; stop if one changes the design (especially a helper framing that cannot carry the two operations).
2. **Core pure code** (6.1 to 6.3): `unit/*`, `CapabilityPlan`, contracts, `PolicyPermit` accessors and `restore`, `ExecutionRecord` permit branch, `ExecutionParts`, ports. Unit tests with the in-file fakes.
3. **Config** (6.5, 6.9): agent `[unattended]`, hub `[policy]` keys, step-up paths, schema snapshots.
4. **Migration and store** (section 8): tables, `PermitRepo`, `StateWrite::Permit` applier, `ExecutionParts` permit columns, in-memory twin in testkit, crash-harness summary arm. SQLite tests and the N-1 upgrade test.
5. **Helper operations** (6.4): adapter over `ProcessExecutor`, `unit.identity`, `unit.restart`, config and tier gate. Unit tests with `FakeSystemd` and `FakeUnitFs`; host tests behind the opt-in variable.
6. **Agent** (6.5): ceiling, hello summary, routing, journal limiter extension. Agent tests.
7. **Link, relay, resolver, registry** (6.6): `AgentLink` methods, `runtime/caps.rs`, `TargetService::resolve_unit`, registry rows, `container.restart` answer. Link tests.
8. **Engine** (6.7, 6.8): `commit_dispatch` extraction, `execute_unit`, permit effect and writer command, validator branch, shadow builder and environment changes. Engine tests with the fake clock and fake link.
9. **Hub issuer, services, API** (6.6, 6.9): `crates/domovoi-hub/src/policy/`, services, endpoints, OpenAPI regeneration.
10. **Arch rules, gates and ADR** (section 11): rules, the replaced hub gate, the new gates; `cargo xtask arch-check` and `--base`.
11. **Web** (6.11), then docs (section 15), checks and the completion report.

## 8. Database and persistence

**Migration.** `crates/domovoi-store/migrations/00NN_permits_and_autonomy.sql`, `NN` the next unused number after listing the directory (expected `0021` after 47c's `0020`; never hard-code it). STRICT tables, integer millisecond timestamps, hex checks in the style of `0011_plan_binding.sql`. Enums are validated in Rust, never by a SQL `CHECK`. Contents:

```sql
-- The permit that authorized one Execution step (at most one per execution).
CREATE TABLE execution_permits (
    execution_id   TEXT PRIMARY KEY REFERENCES executions (execution_id),
    decision_id    TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    ceiling_hash   TEXT NOT NULL,
    plan_hash      TEXT NOT NULL CHECK (length(plan_hash) = 64 AND plan_hash NOT GLOB '*[^0-9a-f]*'),
    issued_at      INTEGER NOT NULL,
    expires_at     INTEGER NOT NULL,
    decision       TEXT NOT NULL            -- the enforced decision document (40's golden-pinned codec)
) STRICT;
CREATE TABLE execution_permit_rules (       -- one row per rule the permit names; the breaker reads this
    execution_id TEXT NOT NULL REFERENCES execution_permits (execution_id),
    rule_id      TEXT NOT NULL,
    PRIMARY KEY (execution_id, rule_id)
) STRICT;
CREATE INDEX execution_permit_rules_rule ON execution_permit_rules (rule_id);
CREATE INDEX execution_permits_issued    ON execution_permits (issued_at);

-- The global pause switch: exactly one row.
CREATE TABLE autonomy_state (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    paused     INTEGER NOT NULL CHECK (paused IN (0, 1)),
    changed_by TEXT,
    changed_at INTEGER,
    reason     TEXT
) STRICT;
INSERT INTO autonomy_state (id, paused) VALUES (1, 0);

-- An owner reset of a rule's circuit breaker.
CREATE TABLE rule_circuit_resets (
    rule_id  TEXT NOT NULL,
    reset_at INTEGER NOT NULL,
    reset_by TEXT NOT NULL,
    PRIMARY KEY (rule_id, reset_at)
) STRICT;

-- An owner (or a later verified run) clearing the unattended block of an OutcomeUnknown execution.
CREATE TABLE unknown_resolutions (
    execution_id TEXT PRIMARY KEY REFERENCES executions (execution_id),
    resolved_at  INTEGER NOT NULL,
    resolved_by  TEXT NOT NULL,            -- a user id, or 'system:policy' for verified_by_later_run
    how          TEXT NOT NULL,            -- owner | verified_by_later_run
    note         TEXT
) STRICT;
```

`executions.authorization_kind` already exists and is set to `policy` with the permit row in the same transaction. The new `StateWrite::Permit(PermitWrite)` variant (with `AggregateKind` and `JournalKind` entries added beside 26's, 34's and 40's, theirs kept) covers `Issue`, `Pause`, `Resume`, `ResetRule`, `ResolveUnknown`; the single `apply` dispatch is `crates/domovoi-store/src/uow/writes.rs`, the in-memory twin `crates/domovoi-testkit/src/repos/uow_apply.rs`, the crash harness summary an exhaustive `match` in `crates/domovoi-testkit/src/crash/mod.rs`. The permit row and the `execution` authorization are written by the operation writer in the same commit; pause, resume, reset and resolve are written by their services with their audit and journal rows.

## 9. Protocol and compatibility

- **Wire 1.2 is unchanged.** No new message, field, code or version; `PROTOCOL_VERSION` stays 1.2. The hub sends existing `run_capability` frames and receives existing `capability_result`, `refused` and `action_status_answers`. `ceiling_hash` and `ceiling_summary` are existing optional `hello` and `agent_status` fields that were always `None`.
- **Older or unconfigured agents.** An agent without this change never reports a ceiling and never advertises a helper operation, so the registry answers `NotAllowedOnHost`/`NotInstalled` and nothing is permitted. An agent without a helper (`HelperStatus::Absent`) is `Unavailable(HelperMissing)`. A host whose helper has write groups off does not list `unit.restart` in `ops`: `NotInstalled`.
- **Old hubs.** An older hub ignores the new tables. A newer hub with `[policy] max_permits_per_hour = 0` never issues a permit.
- **API.** New endpoints and DTO fields only; OpenAPI and `web/src/api/schema.d.ts` change (section 15).

## 10. Error handling and recovery

**Permit time** (nothing dispatched; the step stays an approval offer unless noted):

| Situation | Behaviour |
|---|---|
| contract not policy-dispatchable (a skill, `container.restart`) | `NotIssued::Withheld(NotPolicyDispatchable)`; no audit row |
| paused, suspended rule, unresolved unknown, rate limit | `Withheld`, audit `policy.permit_withheld` with the token |
| unit refused by the helper at re-resolution | `Withheld(IdentityRefused(token))`; the owner sees the sentence |
| recomputed plan hash differs from the stored one | `Withheld(PlanChanged)` |
| evaluator `RequireApproval`, `Deny`, `Unavailable` | `NotIssued::Evaluated`; shadow row unchanged |
| ceiling absent, disabled, or not covering | hard limit 4 gives `RequireApproval` (the evaluator), no permit |
| issuer or store error | effect retries with the existing effect backoff; after `Dead` the approval offer stands |
| a person approves first | the permit is refused `AlreadyAuthorized`; nothing else changes |

**Dispatch time** (each row settles through the writer and audits): permit expired -> `NotRun{authorization_expired}`; paused -> `NotRun{autonomy_paused}`; unresolved unknown for a permit row -> `Refused{unresolved_unknown}`; identity refusal -> `Refused{<token>}`; plan changed -> `Refused` (writer); host `Unknown`/`Absent` -> waits as for skills; session or epoch changed between read and hand-off -> `NotRun`; hand-off failure -> `NotRun{hub_not_handed_off}`.

**Hub restart.** A step `Dispatched` stays `Dispatched` and is reconciled from the agent's journal on the host's next `hello` by 21's mechanism: `finished` settles it, `running` waits, `NotFound` is `NotRun`, `Unknown{..}` is `OutcomeUnknown` (and then blocks unattended reuse, 6.7). An authorized but not yet dispatched permit row survives a restart and is re-checked (expiry, pause, guard) when its `Dispatch` effect runs. A restart between `EvaluatePolicy` and the writer command repeats the evaluation; the one-authorization rule makes it idempotent.

**Agent crash mid-restart** (20's windows): journal `accepted` or `started` without `finished` gives `Unknown{AgentRestarted}` -> `OutcomeUnknown`. The helper may or may not have restarted the unit; the owner checks the host, then resolves the block (6.7) or lets a later verified human-approved run clear it. The hub never retries.

## 11. Security requirements

- **Authorization.** `PolicyPermit::issue` and `Authorization::policy` are called only under `crates/domovoi-hub/src/policy/` (and tests); `PolicyPermit::restore` only under `crates/domovoi-store/src/`. A permit is built only from an `Allow` decision that held every hard limit and every guard of 6.6. The model has no tool, chat command or endpoint reaching permit issuance, pause, resume, reset or resolve; `MetaTool` stays closed. Pause is a person (session and CSRF); resume, reset and resolve need the password and share the login lockout.
- **What the agent and helper still enforce.** The agent re-checks the ceiling (`policy` label), `allow_risk`, the mutation limiter and expiry; the helper its allowlist, protected set, identity and budget. A hub that mislabels a request `human` is still bounded by the helper (accepted v1 limitation, documented).
- **Fail closed.** An unknown contract, missing ceiling, unreported helper op, any unreadable file, an unparsable helper reply, an unknown token, an expired permit or an unproven identity: refuse or withhold, never default to allow. A malformed ceiling summary is never permissive (`parse_ceiling` already does this).
- **Process and files.** No process is spawned outside `domovoi-runner`; the helper uses `ProcessExecutor` with argv vectors. The only `systemctl` argv is `restart -- <Id>`; reads use `busctl` structured output, never `systemctl show` text, never `--json`. No capability to read foreign-owned files is added (`read_failed`). Paths and names are validated (no control characters, `..`, `//`); error text never echoes a unit file's content, and environment values never appear in audit or errors.
- **Secrets.** Environment values are redacted and hidden by default on the card and omitted from audit.
- **Single boundary and gates (extend, never weaken).**
  1. **Replace** `no_code_path_issues_a_policy_permit` (`crates/domovoi-hub/tests/gates.rs:955`). The old assertion that `crates/domovoi-hub/src/policy` does not exist must go because this prompt creates it. The replacement is stricter: `PolicyPermit::issue(` and `Authorization::policy(` appear in **exactly one production file**, `crates/domovoi-hub/src/policy/issuer.rs`, once each; the word `PermitIssuer` is allowed only in that directory, in `ports/permit.rs`, in the engine permit handler and in the runtime wiring; a permit is built only on the `Allow` branch (a source scan for the call being inside the arm that matches `Outcome::Allow`); the shadow, policy-store and `crates/domovoi-engine/src/policy/` directories still name no permit or authorization type. State in the PR that this replaces the gate and does not weaken it.
  2. `xtask/src/arch/rules.rs`: `POLICY_PERMIT_RESTORE_ALLOWED_PREFIXES`, the `plan:capability-v1` literal confined to `crates/domovoi-core/src/action/capability_plan.rs`, `RAW_OPS` and cards for `start_unit_call` and `unit_call_result`, the `execute_unit_restart` pair card, `policy` added to `UNZONED_MODULES`, and the engine rule that only `execute_unit.rs` names `UnitRelay::dispatch_restart`. A numbered ADR is required (`xtask/src/arch/git.rs`): `docs/adr/0037-permit-issuer-and-unit-restart.md` (expected number) records the issuer, the plan-hash family, the closed `UnitCall`, the capability object, why skills stay human-only, the resolution of the post-restart-check discrepancy, and what the checker cannot see. It also records R-8 as accepted by the owner. Run `cargo xtask arch-check` and `--base origin/overhaul-investigation`.
  3. New gates: `no_code_path_calls_systemctl_restart_outside_the_helper`, `helper_never_calls_daemon_reload` (argv set is exactly `restart -- <Id>` plus the `busctl` reads), `no_code_path_passes_json_to_systemctl`, `container_restart_has_no_dispatch_path` (no production source names a Docker helper write operation and `container.restart` is in neither `EXECUTABLE_BUILTINS` nor `POLICY_DISPATCHABLE`), `policy_dispatchable_list_is_closed`, `unit_call_has_no_free_argument` (the closed enum carries only `name` and `id`/`identity_hash`), `engine_never_builds_an_authorization_for_permits`.
- **Audit.** One audit row per security-relevant change in the same transaction as the change (6.9).
- **Resource bounds.** One permit per execution; at most `max_permits_per_hour`; identity replies capped at 16 KiB; at most 8 drop-in pins shown on a card (the rest summarised by count, all still bound by the hash).

## 12. Testing requirements

Fakes (existing unless marked): `FakeClock`, `FakeAgentLink` (extend: scripted `unit_call_result`, ordered frame recording), the in-memory repos and `uow_apply` twin (extend), `FakeProcessExecutor`; **new in `domovoi-testkit::unit`**: `FakeSystemd` (settable loaded state, disk state, a reload counter), `FakeUnitFs` (regular files, symlinks, unreadable files), `FakeUnitRelay`, and recorded `busctl --json=short` fixtures under `crates/domovoi-testkit/fixtures/unit/` for: a plain unit, a unit with package and `/etc` drop-ins, an alias (`ssh.service` with `Names`), a template instance, masked, transient, generated, linked, an awkward drop-in name, a not-found unit, `Requires` returned in two orders, an inactive unit (`GetUnit` failure and `LoadUnit` success), a mask in `/usr/lib`, and `ssh.service` with `ssh.socket` (`Triggers`, `TriggeredBy`). The identity algorithm's own unit tests live in `crates/domovoi-core/tests/unit/` with a small in-file fake, because core cannot depend on testkit. Tests never sleep; unit names in fixtures are neutral (`example.service`).

**Test levels.** *unit* = pure, with fakes, runs everywhere. *host* = needs a real systemd (Linux), `#[ignore]` unless an environment variable names an opted-in host, units use a fixed test prefix, cleanup in a drop guard, and a check that `/dev/null` is still a character device before and after. *gate* = invariant test in a `tests/gates.rs`.

### 12.1 The 16 tests added for host V, and the rest of synthesis 01 section 7.2

Synthesis 01's revision log says sixteen tests were added for the fourth host but does not list them. They are identified here by the V-only evidence they cite (E47, E49, E50, E53 to E57, E59, E17 by `Result`, E41 `--json`): rows marked **V** below are those sixteen. Every other row of 7.2 is mapped too. Location: core = `crates/domovoi-core/tests/unit/`; helper = the helper crate's tests; engine = `crates/domovoi-engine/tests/operations/`; gate = the named crate's `tests/gates.rs`.

| Test | Level | Where | V |
|---|---|---|---|
| `new_drop_in_directory_without_reload_is_detected_by_disk_enumeration` (run for `/etc/systemd/system` and `/usr/lib/systemd/system`) | unit, host | core, helper | V |
| `need_daemon_reload_no_does_not_clear_a_disk_enumeration_mismatch` | unit | core | V |
| `system_cat_and_show_text_are_never_parsed_for_identity` | unit, gate | core, hub gate | V |
| `no_code_path_passes_json_to_systemctl` | gate | engine and hub gates, helper | V |
| `d_bus_set_valued_properties_are_sorted_before_hashing` (`Requires` in both orders gives one hash) | unit | core | V |
| `reload_alone_does_not_change_the_loaded_hash` | host | helper | V |
| `plan_time_reads_an_inactive_unit_with_load_unit` | unit, host | core, helper | V |
| `get_unit_failure_for_an_unloaded_unit_is_not_target_not_found` | unit | core | V |
| `unreadable_file_under_restricted_capabilities_refuses_read_failed_not_a_partial_hash` | unit, host | core, helper | V |
| `system_cat_is_not_used_as_a_file_reader_because_it_drops_unreadable_files` | unit | core | V |
| `mask_in_usr_lib_is_refused_by_load_state_although_fragment_path_is_allowed` | unit, host | core, helper | V |
| `enabled_runtime_unit_is_refused` (owner decision Q2: refused) | unit | core | V |
| `ssh_socket_is_in_the_protected_set_and_a_triggered_service_matches_by_triggered_by` | unit, gate | core, helper | V |
| `start_limit_refusal_is_detected_by_result_not_message_text` | unit, host | helper | V |
| `restart_of_target_that_a_part_of_dependent_follows_is_reported_on_the_card` | host (card text: unit) | helper, core | V |
| `reload_journal_pattern_matches_both_wordings` | **not built**: the journal is never read (REQ-24 conditional); a gate asserts no code reads the journal for reloads | gate | V |
| `identity_binds_canonical_id_when_name_is_an_alias` | unit | core | |
| `alias_retarget_after_approval_refuses_with_target_changed` | unit, host | core, helper | |
| `restart_is_issued_by_canonical_id_never_by_alias` | unit, gate | helper, hub gate | |
| `protected_unit_matches_by_id_and_names` | unit, gate | core, helper | |
| `allowlist_entry_that_is_an_alias_is_a_config_error` | unit | helper | |
| `installer_default_protected_units_cover_ssh_spellings` | unit | helper (47's installer defaults) | |
| `masked_unit_is_refused_even_though_fragment_path_is_not_dev_null` | unit, host | core, helper | |
| `transient_unit_is_refused_by_property_and_by_location` | unit, host | core, helper | |
| `generated_unit_is_refused` | unit, host | core, helper | |
| `generator_drop_in_on_ordinary_unit_is_refused` | unit, host | core, helper | |
| `system_control_drop_in_is_refused` | unit, host | core, helper | |
| `dash_prefix_drop_in_added_without_reload_is_detected_or_refused` | unit, host | core, helper | |
| `usr_local_lib_fragment_is_refused`, `linked_unit_is_refused`, `symlinked_drop_in_is_refused` | unit (first two also host) | core, helper | |
| `fragment_edit_without_reload_refuses_target_stale`, `drop_in_added_without_reload_is_detected_by_disk_enumeration` | host (second also unit) | helper, core | |
| `mtime_preserved_edit_does_not_hide_a_disk_change_in_the_hash`, `rename_replace_with_restored_mtime_is_caught_by_content_hash` | unit, host | core, helper | |
| `touch_alone_refuses_target_stale_and_new_approval_is_not_required_after_reload` | host | helper | |
| `identity_hash_stable_across_repeated_reads_reload_and_touch` | host | helper | |
| `identity_hash_changes_on_drop_in_move_and_comment_edit`, `identity_hash_changes_when_drop_in_precedence_changes`, `identity_hash_unchanged_by_environment_file_and_binary_change` | unit | core | |
| `awkward_drop_in_names_are_read_from_the_array_not_split`, `control_character_in_path_is_refused`, `show_exit_zero_with_not_found_is_refused_target_not_found`, `bare_template_name_is_rejected`, `template_instance_must_be_allowlisted_by_exact_instance` | unit | core | |
| `unreadable_drop_in_refuses_read_failed_never_empty_hash` | unit, host | core, helper | |
| `dispatch_recomputes_identity_before_restart`, `no_other_code_path_calls_systemctl_restart` | unit, gate | helper, hub gate | |
| `dispatch_refuses_when_need_daemon_reload_is_yes` | unit | helper | |
| `restart_stderr_changed_on_disk_flags_stale_start` | unit | helper | |
| `start_limit_hit_reports_failed_unit_and_does_not_retry` | unit, host | helper | |
| `helper_restart_budget_prevents_start_limit` | unit | helper | |
| `mutation_limit_survives_helper_restart` (RW-37) | unit | helper | the persisted `restarts` file is read back by a new `serve_connection`; `mutations_per_hour = 0` refuses every write |
| `unit_identity_is_served_when_systemd_read_is_enabled` | unit | helper | replaces 47d's `systemd_read_stays_not_implemented` |
| `tier_unavailable_reasons_map_to_platform_tokens` | unit | agent | `WriteGroupNeedsOptIn` gives `platform_untested`, `PlatformBlocked` gives `platform_blocked` |
| `post_restart_environ_mismatch_reports_wrong_start_and_counts_toward_circuit_breaker` | unit | helper (check), engine (breaker) | |
| `wrong_start_does_not_trigger_automatic_second_restart`, `post_check_does_not_reread_loaded_properties` | unit | helper | |
| `unit_config_change_invalidates_approval` (roadmap M8) | unit, host | engine, helper | |
| `unit_restart_refused_when_unit_not_in_helper_allowlist` (roadmap M8) | unit | helper | |
| `helper_never_calls_daemon_reload` | gate | helper, hub gate | |

**RW-37 tests owned here: 1.** `mutation_limit_survives_helper_restart` (row above). The other 75 names of the RW-37 list belong to 47c (60) and 46 (15); see 47c section 12.2 and 46 section 12.1.

### 12.2 Tests specific to 45

**Core** (`crates/domovoi-core/tests/`): `capability_plan_hash_changes_with_each_input` (contract, version, host, key, target, identity hash, args, execution each flip it), `capability_plan_domain_is_not_plan_v1_or_v2`, `capability_plan_golden_vectors` (pinned), `unit_identity_fits_target_identity_limit` (128-character `Id`), `unit_refusal_tokens_round_trip`, `approval_card_text_snapshot` (the card, the stale variant, the untested line), `policy_permit_restore_matches_issue`, `execution_authorize_by_permit_checks_in_order` (risk, plan, already authorized, expiry), `execution_cannot_hold_approval_and_permit`, `contract_index_accepts_unit_contracts` (no category spans two classes), `policy_dispatchable_list_is_closed`.

**Permit issuer** (`crates/domovoi-hub/tests/hub/permit.rs`, with the fake clock and fake relay):

| Test | Setup, action, expected |
|---|---|
| `no_permit_without_policy_allow` (M8) | outcomes `RequireApproval`, `Deny`, `Unavailable` and a shadow-only `Allow` each give `NotIssued`; no `execution_permits` row |
| `permit_binds_the_recomputed_plan_hash` | stored plan hash equals the permit's; a configuration change since proposal -> `Withheld(PlanChanged)` |
| `skill_contracts_are_never_permitted` | an `Allow` rule for a skill -> `NotPolicyDispatchable`; the approval offer stands |
| `permit_expiry_is_capped_by_the_execution_window` | `permit_ttl` longer than the window is capped |
| `pause_blocks_new_permits` (M8) | paused -> `Withheld(Paused)`; resume with the password -> issued |
| `pause_blocks_dispatch_of_an_issued_permit` | permit issued, then paused, dispatch -> `NotRun{autonomy_paused}`, no frame |
| `circuit_breaker_suspends_rule` (M8) | two `Failed` results within the window -> next step `RequireApproval` with hard limit 9 in the trace; a reset row clears it; an `OutcomeUnknown` counts; a pre-restart `Refused` does not |
| `circuit_breaker_window_expires` | failures older than the window do not count |
| `permit_rate_limit_holds_per_rule_and_global` | `max_permits_per_hour` and the rule's `max_per_hour` |
| `outcome_unknown_blocks_unattended_dispatch` (M8) | an unresolved `OutcomeUnknown` for host, contract and unit `Id` -> no permit, and a permit already issued is refused at dispatch; a human approval of the same unit is **not** blocked; a config edit (new `identity_hash`) does **not** clear the block |
| `outcome_unknown_block_clears_by_owner_command_or_later_verified_run` | `resolve_unknown` needs the password; a later human-authorized verified success clears it in the same commit |
| `ceiling_missing_or_not_covering_gives_approval_not_permit` | hard limit 4 |
| `permit_issuance_is_audited_with_rule_ids_without_environment_values` | `policy.permit_issued` detail |
| `a_person_approving_first_wins` | `AlreadyAuthorized`, one authorization |
| `injection_drill_model_cannot_obtain_a_permit` (M8 drill, `crates/domovoi-engine/tests/drills/`) | a hostile incident text pushes the model to propose `unit.restart` of a protected unit, of an unlisted unit, of an allowed unit outside the pinned subjects, and to call a pause, resume or permit tool: every one is refused or stays `RequireApproval`; no tool reaches the issuer |

**Agent** (`crates/domovoi-agent/tests/agent/`): `agent_refuses_policy_dispatch_outside_ceiling` (M8; each of the five ordered checks refuses with `ceiling_exceeded` and names the failing step), `human_label_skips_the_ceiling_but_not_the_limiter_or_allow_risk`, `unattended_config_rejects_unknown_keys_and_unfit_fields` (`max_active`, `max_ttl`, `args` on `unit.restart`; a destructive or unknown contract), `ceiling_hash_and_summary_reach_hello_and_agent_status`, `ceiling_summary_parses_with_the_hubs_parse_ceiling`, `policy_label_on_a_package_is_still_refused`, `unit_request_without_helper_op_is_contract_unknown`, `policy_rate_counted_from_the_journal_survives_restart`, `unit_identity_read_is_journaled_and_skips_the_limiter`.

**Engine, link and hub:** `unit_step_is_validated_resolved_and_hashed_with_capability_plan`, `unit_step_outside_pinned_subjects_is_require_approval`, `alias_typed_name_binds_the_canonical_id`, `execute_unit_restart_commits_dispatch_before_sending`, `dispatch_hand_off_failure_is_not_run`, `result_timeout_stays_dispatched_and_reconciles`, `outcome_mapping_table` (every row of 6.7), `wrong_start_raises_attention_and_counts_for_the_breaker`, `start_limit_hit_is_failed_and_never_retried`, `human_approval_of_unit_restart_dispatches_with_human_label`, `registry_reads_helper_ops` (`NotInstalled` without the op), `container_restart_is_unavailable_for_both_labels`, `shadow_recorder_now_reads_real_autonomy_and_unit_targets`, `unit_call_link_fails_fast_when_the_session_changed`, `tier_untested_without_optin_refuses_write_and_reads_identity` (helper), `platform_optin_is_in_the_dispatch_audit`.

**Store** (`crates/domovoi-store/tests/sqlite/permit.rs`): `permit_row_is_written_with_the_authorization_atomically`, `second_permit_for_an_execution_conflicts`, `circuit_failures_query_counts_by_rule_and_window`, `autonomy_row_is_single_and_defaults_to_not_paused`, `unknown_resolution_is_unique_per_execution`, plus the N-1 upgrade test of section 8.

**API and web:** `pause_requires_session_and_csrf`, `resume_requires_password_and_counts_toward_lockout`, `reset_rule_requires_password`, `resolve_unknown_requires_password`, `no_endpoint_accepts_a_client_supplied_permit_or_approval`, `execution_dto_shows_authorization_attribution`, `openapi_lists_the_autonomy_routes`; web `autonomy_panel_is_p0_and_shows_state`, `autonomy_panel_resume_uses_the_password_prompt`, `suspended_rules_list_and_reset`, `attribution_line_names_the_authorizer`, page axe checks.

**M8 roadmap names and 04 §8.1 gates.** Present by name: `no_permit_without_policy_allow`, `agent_refuses_policy_dispatch_outside_ceiling`, `circuit_breaker_suspends_rule`, `pause_blocks_new_permits`, `unit_restart_refused_when_unit_not_in_helper_allowlist`, `unit_config_change_invalidates_approval`, `outcome_unknown_blocks_unattended_dispatch` and the injection drill. The 04 §8.1 gates `no_approval_without_human`, `single_execution_boundary`, `destructive_never_allowed_by_policy`, `model_meta_tools_are_closed`, `approval_bound_to_displayed_plan_hash` and `changed_target_invalidates_approval`: **verify which exist** on the branch, extend each to cover the permit path and the unit path, add any that is missing, and never weaken one.

Run Linux-only and host tests in WSL as the repository rules describe, say which tests ran where, and say which `host` tests were not run.

## 13. Acceptance criteria

1. Nothing under `crates/domovoi-proto/` changes; no golden frame changes; protocol stays 1.2.
2. One migration adds exactly the tables of section 8; the N-1 upgrade test passes; no merged migration is edited.
3. A `PolicyPermit` is issued only by `HubPermitIssuer`, only from an enforced `Allow` that held every hard limit and every guard; `PolicyPermit::issue` and `Authorization::policy` each appear once in production code; `PolicyPermit::restore` appears only in the store. The replaced hub gate is stricter than the old one.
4. Skills and `container.restart` are never permitted or dispatched unattended; `container.restart` has no dispatch path for either label and the registry says why; no production source names a Docker helper write operation.
5. The agent enforces the `[unattended]` ceiling for `policy`-labelled requests in the ordered way, reports `ceiling_hash` and summary, and still refuses policy-labelled package changes.
6. `unit.restart` runs only after the helper resolved the identity from D-Bus plus the disk enumeration, matched by `Id` and `Names`, with every refusal class and token of 6.3; a drop-in added after approval is `target_stale`, a content change is `target_changed` with the first differing field, an unreadable file is `read_failed`; `NeedDaemonReload=yes` refuses; the helper never reloads; the restart is `systemctl restart -- <Id>` only.
7. The post-restart check reads the process (`environ`, and cmdline and uid), never the loaded properties; a wrong start is an alert, a `Failed` step and a breaker count with no further action; a start-limit refusal is reported and never retried.
8. The platform tier gate holds: write operations are refused on an Untested host without the per-host opt-in and on a Blocked host; the opt-in is on the audit record of a dispatch.
9. Pause blocks new permits and undispatched permits; resume, rule reset and resolve-unknown need the password; the breaker is derived and suspends a rule after the configured failures; an unresolved `OutcomeUnknown` blocks unattended reuse of the same host, contract and unit `Id` (not cleared by a configuration change) and never blocks a human approval.
10. The approval card states what is bound and not bound, the residual race and the start-failure risk, exactly as the snapshot.
11. A person approving and a policy permit cannot both authorize one execution; every issuance, withholding of a safety guard, pause, resume, reset and resolution is audited in the same transaction.
12. The 16 V-derived tests and every row of synthesis 01 section 7.2 exist at their level (or are recorded as not built with the reason, as for the journal-wording row); the M8 names and the injection drill exist and pass.
13. `cargo xtask arch-check` (and `--base`) pass with the new rules and ADR; non-test files are at most 500 lines; no new `#[allow]`, `unwrap`, `expect` or `panic!` in library code; OpenAPI and `schema.d.ts` regenerated; the web guardrails pass.

## 14. Migration and compatibility

Existing installs: nothing changes until a host has an `[unattended]` ceiling, a helper with `systemd.write` enabled and an owner opt-in on its platform tier, **and** the owner publishes a non-shadow `Allow` rule for `unit.restart`. Until then every `unit.restart` step is an approval offer, and a human can approve it once the host offers the operation. The new tables start empty (pause state: not paused). Existing agents: unchanged; those without the ceiling never receive a policy-labelled request. Existing policy versions and shadow rows are unchanged; the recorder now records real autonomy state and unit targets (the `Autonomy` gap disappears). Downgrade: an older hub ignores the tables and never issues a permit. Prompt 46 reuses the issuer, the guards and the ceiling; it adds `firewall.block_source` to `POLICY_DISPATCHABLE` and the containment inputs.

## 15. Documentation requirements

- `cargo xtask docs-config` regenerates `docs/book/src/reference/configuration.md`: review the agent `[unattended]` and hub `[policy]` rows and accept the schema snapshots only after reading the diff.
- OpenAPI: `cargo xtask openapi && cd web && npm run gen:api`; commit the two generated files (never hand-edit them).
- Book: `docs/book/src/concepts/policy.md` (40's page): enforcement now exists, how a permit is issued, the guards (pause, breaker, rates, unknown outcomes), what "Not enforced" now means, and the accepted limitation (a compromised hub can mislabel a request; the helper still holds). `docs/book/src/reference/` agent and helper pages: `[unattended]`, `restart_units`, `protected_units`, the `systemd.read` and `systemd.write` groups, the platform tiers and the per-host opt-in (and that nothing is Supported yet). A guide page for `unit.restart`: what is bound and not bound, refusals in plain words (`target_stale` means run `systemctl daemon-reload` yourself, `read_failed`, protected, not allowlisted), the residual race, the start-limit risk, what a wrong start alert means and what to do, and how to resolve an unknown outcome. `docs/book/src/security.md`: the permit path and the unchanged human-only rule for everything else.
- `ARCHITECTURE.md`: §12 and §15 invariants (permits only from the issuer, skills human-only, the helper never reloads, the unknown-outcome guard); §4 only if the helper's crate placement needs an edge (47's call).
- **Follow-up edits to the design documents (list only, for the owner; not made here):** `04` §8.3 and synthesis 00 §6 say "post-restart check of the loaded properties": change to the process check of REQ-20; `04` §8.1 gates list; `99` R-8 row (acceptance recorded) and the index row for 45 and `README.md` ("every prompt has a file except 46 and 47").
- `web/DESIGN.md`: the autonomy panel and the attribution line. Run `cargo xtask docs-links`.
- Commits and the PR must not mention prompts, prompt numbers, the build plan or coding agents.

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

Task-specific: no new dependency is added anywhere (workspace entries only; `busctl` and `systemctl` are called through `ProcessExecutor`, no D-Bus crate). 47 (a, b, c) must be merged and the owner must have accepted R-8; if the names of 47's helper protocol, `PlatformTier` or `helper_client` differ from the ones used here, use the merged names and report each difference. Run Linux-only tests in WSL (`wsl.exe -d Ubuntu --`, `CARGO_TARGET_DIR=$HOME/domovoi-target`) and say which tests ran where, and which `host` tests (a real systemd) were not run.

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

Task-specific checks: crates edited are `domovoi-core`, `domovoi-config`, `domovoi-store`, `domovoi-engine`, `domovoi-hub`, `domovoi-agent`, `domovoi-helper`, `domovoi`, `domovoi-testkit` and `xtask`, so run `cargo nextest run -E 'rdeps(domovoi-core)'` and `-E 'rdeps(domovoi-config)'`, `cargo xtask arch-check` (and `--base origin/overhaul-investigation`), `cargo xtask docs-config`, `cargo xtask docs-links`, and `cargo xtask openapi && cd web && npm run gen:api`; for `web/` run `npm run lint`, `npm run typecheck` and the vitest files of the policy and operations features. Commit as `Rostislav <raikov0411@gmail.com>`; the PR title has one scope, for example `feat(hub): issue policy permits for unattended unit restarts`.

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
